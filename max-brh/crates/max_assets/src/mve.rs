//! Interplay MVE video decoder (`INTROFLC` → `maxint.mve`).
//!
//! Format — the Interplay MVE container (M.A.X. plays the intro through
//! `movie_run(INTROFLC, …)` → `mvelib32.c`). Layout:
//!
//! ```text
//! file: 20-byte signature "Interplay MVE File\x1A\0",
//!       u16 magic 0x001A, u16 version 0x0100, u16 checksum
//!       (checksum == !version + 0x1234);
//! then records (chunk): u16 payload_size, u16 type
//!       type 0 init_audio, 1 audio_only, 2 init_video, 3 video,
//!       4 shutdown, 5 end;
//! inside a record — an opcode stream: u16 size, u8 opcode, u8 version, data.
//! ```
//!
//! # Video formats
//!
//! The opcodes needed by the intro are supported: `CREATE_TIMER` (2), `INIT_VIDEO_BUFFERS`
//! (5), `SET_PALETTE` (12), `SET_DECODING_MAP` (15), `VIDEO_DATA_11` (17). Video is 8-bit
//! indexed frames sliced into 8×8 blocks; the decoding map of `VIDEO_DATA_11` is one nibble
//! per block (the low nibble is the left block, the high one the right). The block codec is an
//! exact port of `nfPkDecomp` from `max-cpp-port/src/mvelib32.c` (0x0 — copy of the previous
//! frame, 0x1 — of the one before, 0x2/0x3 — motion vectors, 0x4/0x5 — copy with a small
//! shift, 0x7…0xF — patterns of 2/4/16/64 colors, 0x6 does not occur in M.A.X.
//! resources and is left a no-op, as in FFmpeg). The frame's `flags & 1` flag
//! toggles double buffering (like `nfAdvance`), so frame N is fully
//! determined by frames N−1/N−2 — the decoder is deterministic.
//!
//! All three video-data opcodes of the container are decoded (C4):
//!
//! * `VIDEO_DATA_11` (0x11) — block codes from decoding map 2 (`SET_DECODING_MAP`, 0x0F);
//!   this is the only format M.A.X. movies use (`maxint.mve`, `maxmve1/2.mve`);
//! * `VIDEO_DATA_06` (0x06) — `nfDecomp` in the oracle: raw 8×8 blocks, no decoding map
//!   (64 bytes of indices per block, row by row);
//! * `VIDEO_DATA_10` (0x10) — `nfDecompChg` in the oracle: decoding map 1 (`0x0E`) is a
//!   change mask (one bit per 8×8 block), decoding map 2 (`0x0F`) holds the block codes;
//!   blocks with a clear change bit are kept from the previous frame.
//!
//! The oracle (`max-cpp-port/src/mvelib32.c`) dispatches all three opcodes but leaves
//! `nfDecomp`/`nfDecompChg` as `\todo` stubs, and no M.A.X. resource exercises 0x06/0x10, so
//! the reconstruction follows the documented Interplay/FFmpeg contract of those two opcodes
//! (see [`MveInfo::video_formats`] and the `mve` tests with synthetic containers).
//!
//! # Audio
//!
//! The audio track is decoded into PCM16 chunks instead of being skipped:
//! `ALLOC_AUDIO_BUFFERS` (3) sets the format (channels/sample rate/bits/delta compression),
//! `AUDIO_FRAME` (8) and `SILENCE_FRAME` (9) carry one video-frame worth of sound each.
//! The decompression is an exact port of `sndDecompM16`/`sndDecompS16` (the 8-bit delta
//! table `snd_8to16`) and the track mask selection matches `rm_track_bit & opcode->mask`.
//! Decoded chunks are drained with [`MveDecoder::take_audio`]; the layer itself never
//! touches an output device (the oracle's `sndAdd` without a driver is a no-op as well).
//! `INTROSUB` subtitles are a separate layer (`max_app::subtitles`, `moviesubtitle.cpp`).
//!
//! Verification: the unit tests below check the metadata of the real `MAXINT.MVE`
//! (432×320, 4391 frames, 15 FPS), the FNV-1a hashes of frames 0/50/100 against a reference
//! captured by a C harness over the oracle's `nfPkDecomp`, the real audio track
//! (22050 Hz stereo 16-bit delta, one chunk per frame) and synthetic 0x06/0x10 containers.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

/// File signature (19 bytes; in the file it is followed by NUL).
pub const MVE_SIGNATURE: &[u8; 19] = b"Interplay MVE File\x1a";
/// File header size: signature + magic + version + checksum.
pub const FILE_HEADER_SIZE: usize = 26;
/// Opcode header size: `u16 size`, `u8 opcode`, `u8 version`.
pub const OPCODE_HEADER_SIZE: usize = 4;
/// Header of the `VIDEO_DATA_11` video opcode: 7 × `u16` (unused×2, x, y, w, h, flags).
pub const VIDEO_HEADER_SIZE: usize = 14;
/// How many colors are in the palette.
pub const PALETTE_SIZE: usize = 256;
/// Default frame delay (15 FPS) if `CREATE_TIMER` was not encountered.
pub const DEFAULT_FRAME_DELAY_US: u32 = 66_667;
/// Codec block: 8×8 pixels.
pub const BLOCK_SIZE: usize = 8;

/// Container record type.
const CHUNK_INIT_AUDIO: u16 = 0;
const CHUNK_AUDIO_ONLY: u16 = 1;
const CHUNK_INIT_VIDEO: u16 = 2;
const CHUNK_VIDEO: u16 = 3;
const CHUNK_SHUTDOWN: u16 = 4;
const CHUNK_END: u16 = 5;

/// Opcodes.
const OP_END_OF_STREAM: u8 = 0x00;
const OP_END_OF_CHUNK: u8 = 0x01;
const OP_CREATE_TIMER: u8 = 0x02;
const OP_ALLOC_AUDIO_BUFFERS: u8 = 0x03;
const OP_SYNCH_AUDIO: u8 = 0x04;
const OP_INIT_VIDEO_BUFFERS: u8 = 0x05;
const OP_VIDEO_DATA_06: u8 = 0x06;
const OP_AUDIO_FRAME: u8 = 0x08;
const OP_SILENCE_FRAME: u8 = 0x09;
const OP_SET_PALETTE: u8 = 0x0C;
const OP_SET_DECODING_MAP1: u8 = 0x0E;
const OP_SET_DECODING_MAP2: u8 = 0x0F;
const OP_VIDEO_DATA_10: u8 = 0x10;
const OP_VIDEO_DATA_11: u8 = 0x11;

/// Header of an audio opcode (`AUDIO_FRAME`/`SILENCE_FRAME`): `id`, `mask`, `length`.
pub const AUDIO_HEADER_SIZE: usize = 6;
/// Audio tracks of an MVE movie (`Track_English`…`Track_Spanish` in `movie.cpp`).
pub const AUDIO_TRACKS: u8 = 5;
/// How many decoded audio chunks the decoder keeps before the consumer drains them.
///
/// The frame loop drains them after every decoded frame; the cap only protects a
/// consumer that never asks for the sound (QA/headless) from unbounded memory.
pub const MAX_PENDING_AUDIO_CHUNKS: usize = 64;
/// Default M.A.X. movie soundtrack format, used when the stream has no
/// `ALLOC_AUDIO_BUFFERS` (the real files have it: stereo, 22050 Hz, 16-bit, delta).
pub const DEFAULT_AUDIO_FORMAT: MveAudioFormat = MveAudioFormat {
    channels: 2,
    sample_rate: 22_050,
    bits: 16,
    compressed: true,
};

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

/// Audio format of an MVE movie (`ALLOC_AUDIO_BUFFERS` opcode 3).
///
/// The oracle passes the same bits to `sndConfigure`: `flags & 1` — stereo,
/// `flags & 2` — 16 bit, `flags & 4` — delta compression (`snd_comp16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MveAudioFormat {
    /// 1 — mono, 2 — stereo.
    pub channels: u8,
    /// Sample rate in Hz (22050 in `maxint.mve`).
    pub sample_rate: u32,
    /// Bits per sample: 8 or 16.
    pub bits: u8,
    /// 16-bit stream is delta-compressed (`sndDecompM16`/`sndDecompS16`).
    pub compressed: bool,
}

impl Default for MveAudioFormat {
    fn default() -> Self {
        DEFAULT_AUDIO_FORMAT
    }
}

impl MveAudioFormat {
    /// Bytes per output sample.
    pub fn bytes_per_sample(&self) -> usize {
        usize::from(self.bits / 8)
    }

    /// Number of PCM16 samples for `decoded_len` bytes of the stream.
    pub fn samples_for(&self, decoded_len: usize) -> usize {
        match self.bits {
            8 => decoded_len,
            _ => decoded_len / 2,
        }
    }
}

/// One decoded audio chunk (PCM16, interleaved) — one video frame of the track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MveAudioChunk {
    /// Interleaved PCM16 samples (`[L0, R0, L1, R1, …]` for stereo).
    pub samples: Vec<i16>,
    /// Number of channels of the track.
    pub channels: u8,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// The chunk is a `SILENCE_FRAME` (still a chunk: the track keeps its timing).
    pub silent: bool,
}

impl MveAudioChunk {
    /// Number of PCM frames (samples per channel).
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / usize::from(self.channels)
        }
    }

    /// Duration of the chunk in seconds (0 for a broken sample rate).
    pub fn seconds(&self) -> f32 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f32 / self.sample_rate as f32
        }
    }
}

/// `snd_8to16` from `mvelib32.c`: the 8-bit delta → 16-bit step table.
const SND_8TO16: [i16; 256] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
    12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
    24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35,
    36, 37, 38, 39, 40, 41, 42, 43, 47, 51, 56, 61,
    66, 72, 79, 86, 94, 102, 112, 122, 133, 145, 158, 173,
    189, 206, 225, 245, 267, 292, 318, 348, 379, 414, 452, 493,
    538, 587, 640, 699, 763, 832, 908, 991, 1081, 1180, 1288, 1405,
    1534, 1673, 1826, 1993, 2175, 2373, 2590, 2826, 3084, 3365, 3672, 4008,
    4373, 4772, 5208, 5683, 6202, 6767, 7385, 8059, 8794, 9597, 10472, 11428,
    12471, 13609, 14851, 16206, 17685, 19298, 21060, 22981, 25078, 27367, 29864, 32589,
    -29973, -26728, -23186, -19322, -15105, -10503, -5481, -1, 1, 1, 5481, 10503,
    15105, 19322, 23186, 26728, 29973, -32589, -29864, -27367, -25078, -22981, -21060, -19298,
    -17685, -16206, -14851, -13609, -12471, -11428, -10472, -9597, -8794, -8059, -7385, -6767,
    -6202, -5683, -5208, -4772, -4373, -4008, -3672, -3365, -3084, -2826, -2590, -2373,
    -2175, -1993, -1826, -1673, -1534, -1405, -1288, -1180, -1081, -991, -908, -832,
    -763, -699, -640, -587, -538, -493, -452, -414, -379, -348, -318, -292,
    -267, -245, -225, -206, -189, -173, -158, -145, -133, -122, -112, -102,
    -94, -86, -79, -72, -66, -61, -56, -51, -47, -43, -42, -41,
    -40, -39, -38, -37, -36, -35, -34, -33, -32, -31, -30, -29,
    -28, -27, -26, -25, -24, -23, -22, -21, -20, -19, -18, -17,
    -16, -15, -14, -13, -12, -11, -10, -9, -8, -7, -6, -5,
    -4, -3, -2, -1,
];

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// MVE parse/decoding error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MveError {
    /// The file is shorter than the header.
    TooShort {
        /// Actual length.
        len: usize,
        /// Minimum required.
        needed: usize,
    },
    /// No `Interplay MVE File` signature.
    BadSignature,
    /// Magic/version/checksum did not match.
    BadHeader,
    /// A container record extends beyond the end of the file.
    TruncatedChunk {
        /// Record offset.
        offset: usize,
        /// Required end.
        needed: usize,
        /// Actual length.
        len: usize,
    },
    /// An opcode extends beyond the end of the record.
    TruncatedOpcode {
        /// Opcode offset.
        offset: usize,
        /// Required end.
        needed: usize,
        /// Actual length.
        len: usize,
    },
    /// The first video opcode appeared before the decoding map.
    MissingDecodingMap {
        /// Frame index.
        frame: usize,
    },
    /// A `VIDEO_DATA_10` frame has no change map (`SET_DECODING_MAP1`, opcode 0x0E).
    MissingChangeMap {
        /// Frame index.
        frame: usize,
    },
    /// The frame dimensions/coordinates do not fit into the buffer.
    BadFrameRect {
        /// Frame index.
        frame: usize,
        /// x offset (in pixels).
        x: u16,
        /// y offset (in pixels).
        y: u16,
        /// Width (in pixels).
        width: u16,
        /// Height (in pixels).
        height: u16,
    },
    /// The decoding map is shorter than `width*height/2` bytes.
    BadDecodingMap {
        /// Frame index.
        frame: usize,
        /// Minimum required length.
        needed: usize,
        /// Actual length.
        len: usize,
    },
    /// The change map (`SET_DECODING_MAP1`) is shorter than `width*height/8` bytes.
    BadChangeMap {
        /// Frame index.
        frame: usize,
        /// Minimum required length.
        needed: usize,
        /// Actual length.
        len: usize,
    },
    /// The video opcode is shorter than the 14-byte header.
    TruncatedVideoHeader {
        /// Frame index.
        frame: usize,
        /// Actual length.
        len: usize,
    },
    /// The block stream is cut off.
    TruncatedVideo {
        /// Frame index.
        frame: usize,
    },
    /// Video format `0x06`/`0x10` is not supported (M.A.X. resources use `0x11`).
    UnsupportedVideoFormat {
        /// Opcode format.
        format: u8,
    },
    /// Unknown block code (does not occur in the resources).
    UnsupportedBlock {
        /// Frame index.
        frame: usize,
        /// Block code.
        opcode: u8,
    },
    /// There is no video in the file (no `INIT_VIDEO_BUFFERS`/`VIDEO_DATA`).
    NoVideo,
}

impl fmt::Display for MveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, needed } => write!(
                f,
                "MVE слишком короткий: {len} байт (нужно минимум {needed})"
            ),
            Self::BadSignature => write!(f, "нет подписи Interplay MVE"),
            Self::BadHeader => write!(f, "magic/version/checksum MVE не совпали"),
            Self::TruncatedChunk {
                offset,
                needed,
                len,
            } => write!(
                f,
                "запись на 0x{offset:X} требует {needed} байт, файл {len}"
            ),
            Self::TruncatedOpcode {
                offset,
                needed,
                len,
            } => write!(
                f,
                "опкод на 0x{offset:X} требует {needed} байт, запись {len}"
            ),
            Self::MissingDecodingMap { frame } => {
                write!(f, "кадр {frame}: видео без карты декодирования")
            }
            Self::BadFrameRect {
                frame,
                x,
                y,
                width,
                height,
            } => write!(
                f,
                "кадр {frame}: прямоугольник ({x},{y}) {width}×{height} вне буфера"
            ),
            Self::BadDecodingMap { frame, needed, len } => write!(
                f,
                "кадр {frame}: карта декодирования {len} байт (нужно {needed})"
            ),
            Self::MissingChangeMap { frame } => {
                write!(f, "кадр {frame}: VIDEO_DATA_10 без карты изменений")
            }
            Self::BadChangeMap { frame, needed, len } => write!(
                f,
                "кадр {frame}: карта изменений {len} байт (нужно {needed})"
            ),
            Self::TruncatedVideoHeader { frame, len } => write!(
                f,
                "кадр {frame}: видео-опкод {len} байт (нужен заголовок 14)"
            ),
            Self::TruncatedVideo { frame } => {
                write!(f, "кадр {frame}: поток блоков обрывается")
            }
            Self::UnsupportedVideoFormat { format } => {
                write!(f, "формат видео 0x{format:02X} не поддержан")
            }
            Self::UnsupportedBlock { frame, opcode } => {
                write!(f, "кадр {frame}: неизвестный код блока {opcode}")
            }
            Self::NoVideo => write!(f, "в MVE нет видео"),
        }
    }
}

impl std::error::Error for MveError {}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

/// MVE file passport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MveInfo {
    /// Frame width in pixels (`width_blocks * 8`).
    pub width: u16,
    /// Frame height in pixels (`height_blocks * fqty * 8`).
    pub height: u16,
    /// Number of video frames (all video-data opcodes).
    pub frame_count: usize,
    /// Frame duration in microseconds (`rate * divider`).
    pub frame_delay_us: u32,
    /// Video format of the first frame (`0x11` in `maxint.mve`).
    ///
    /// A container may mix formats; the decoder dispatches per frame, this field
    /// is the file's passport (C4).
    pub video_format: u8,
    /// Distinct video formats of the file, in order of appearance.
    pub video_formats: [u8; 3],
    /// How many of [`Self::video_formats`] entries are used.
    pub video_format_count: u8,
    /// Audio track format (`ALLOC_AUDIO_BUFFERS`), `None` — the movie is silent.
    pub audio: Option<MveAudioFormat>,
}

impl MveInfo {
    /// Frame duration in seconds.
    pub fn frame_delay_seconds(&self) -> f32 {
        if self.frame_delay_us == 0 {
            DEFAULT_FRAME_DELAY_US as f32 / 1_000_000.0
        } else {
            self.frame_delay_us as f32 / 1_000_000.0
        }
    }

    /// Frames per second (for logs/tests).
    pub fn frames_per_second(&self) -> f32 {
        1.0 / self.frame_delay_seconds()
    }

    /// Video formats of the file actually used by frames.
    pub fn used_video_formats(&self) -> &[u8] {
        &self.video_formats[..usize::from(self.video_format_count).min(3)]
    }

    /// Whether the file has an audio track.
    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }
}

// ---------------------------------------------------------------------------
// File parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct PaletteUpdate {
    start: u16,
    count: u16,
    /// Absolute offset of 3·count bytes of the 6-bit palette.
    offset: usize,
}

#[derive(Debug, Clone, Copy)]
struct FrameHeader {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    flags: u16,
}

#[derive(Debug, Clone, Copy)]
struct FrameSpec {
    /// Video-data opcode (`0x06`, `0x10`, `0x11`).
    format: u8,
    /// Decoding map 2 (block codes) — `0x10`/`0x11`.
    map_offset: usize,
    map_len: usize,
    /// Decoding map 1 (change mask, one bit per 8×8 block) — `0x10` only.
    change_map_offset: usize,
    change_map_len: usize,
    video_offset: usize,
    video_len: usize,
    header: FrameHeader,
}

/// One audio opcode (`AUDIO_FRAME`/`SILENCE_FRAME`).
#[derive(Debug, Clone, Copy)]
struct AudioSpec {
    /// Absolute offset of the compressed payload (after the 6-byte header).
    data_offset: usize,
    /// Compressed payload length (`opcode size - 6`).
    data_len: usize,
    /// Decoded length in bytes from the opcode header (`length`).
    decoded_len: usize,
    /// Track mask (`rm_track_bit & mask` selects the language track).
    mask: u16,
    /// `SILENCE_FRAME` — the chunk carries no payload.
    silent: bool,
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Palette(PaletteUpdate),
    Frame(FrameSpec),
    AudioFormat(MveAudioFormat),
    Audio(AudioSpec),
}

#[derive(Debug)]
struct Inner {
    /// The whole file, owned by the shared `Arc<Inner>`. `Mve::parse` moves the
    /// caller's `Vec` in, so opening a 68 MiB movie does not copy it into an
    /// `Arc<[u8]>` (which would need a second 68 MiB allocation: the `Arc`
    /// header cannot be prepended to the `Vec` buffer). Every `Mve`/`MveDecoder`
    /// clone shares these bytes through the `Arc<Inner>`.
    data: Vec<u8>,
    events: Vec<Event>,
    info: MveInfo,
}

/// Parsed MVE file: metadata + indexed frames.
///
/// The file bytes live inside the shared `Arc<Inner>` and are never copied;
/// frames are decoded on demand by [`MveDecoder`], so the 71 MB of
/// `maxint.mve` do not turn into 600 MB of decompressed frames.
#[derive(Debug, Clone)]
pub struct Mve {
    inner: Arc<Inner>,
}

impl Mve {
    /// Reads and parses an MVE file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MveError> {
        let bytes = std::fs::read(path.as_ref()).map_err(|_| MveError::TooShort {
            len: 0,
            needed: FILE_HEADER_SIZE,
        })?;

        Self::parse(bytes)
    }

    /// Parses MVE from bytes.
    ///
    /// The `Vec` is moved into the film, not copied: the caller's buffer becomes
    /// the decoder's buffer (one allocation for the whole file).
    pub fn parse(bytes: Vec<u8>) -> Result<Self, MveError> {
        if bytes.len() < FILE_HEADER_SIZE {
            return Err(MveError::TooShort {
                len: bytes.len(),
                needed: FILE_HEADER_SIZE,
            });
        }

        if &bytes[..MVE_SIGNATURE.len()] != MVE_SIGNATURE {
            return Err(MveError::BadSignature);
        }

        let magic = u16::from_le_bytes([bytes[20], bytes[21]]);
        let version = u16::from_le_bytes([bytes[22], bytes[23]]);
        let checksum = u16::from_le_bytes([bytes[24], bytes[25]]);

        if magic != 0x001A || version != 0x0100 || checksum != expected_checksum(version) {
            return Err(MveError::BadHeader);
        }

        // No `Arc::from(bytes)` here: `Arc<[u8]>` would allocate a new buffer and
        // memcpy all 68 MiB of `maxint.mve`. The `Vec` is stored as is inside
        // `Arc<Inner>`, which already provides the sharing.
        let data = bytes;
        let mut events = Vec::new();
        let mut width = 0u16;
        let mut height = 0u16;
        let mut frame_count = 0usize;
        let mut frame_delay_us = 0u32;
        let mut video_formats = [0u8; 3];
        let mut video_format_count = 0u8;
        let mut audio: Option<MveAudioFormat> = None;
        // Decoding map 2 (`SET_DECODING_MAP`, opcode 0x0F): block codes of 0x10/0x11.
        let mut map: Option<(usize, usize)> = None;
        // Decoding map 1 (`SET_DECODING_MAP1`, opcode 0x0E): change mask of 0x10.
        let mut change_map: Option<(usize, usize)> = None;
        let mut offset = FILE_HEADER_SIZE;
        let mut end_of_stream = false;

        while offset + 4 <= data.len() {
            let size = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            let kind = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            let payload_start = offset + 4;
            let payload_end = payload_start
                .checked_add(size)
                .ok_or(MveError::TruncatedChunk {
                    offset,
                    needed: payload_start,
                    len: data.len(),
                })?;

            if size == 0 && kind == CHUNK_END {
                break;
            }

            if payload_end > data.len() {
                return Err(MveError::TruncatedChunk {
                    offset,
                    needed: payload_end,
                    len: data.len(),
                });
            }

            if kind == CHUNK_SHUTDOWN {
                break;
            }

            if !matches!(
                kind,
                CHUNK_INIT_AUDIO | CHUNK_AUDIO_ONLY | CHUNK_INIT_VIDEO | CHUNK_VIDEO
            ) {
                // Unknown record type: skip it entirely (like FFmpeg CHUNK_BAD).
                offset = payload_end;
                continue;
            }

            let mut pos = payload_start;

            while pos + OPCODE_HEADER_SIZE <= payload_end {
                let opcode_size = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                let opcode = data[pos + 2];
                let data_start = pos + OPCODE_HEADER_SIZE;
                let data_end =
                    data_start
                        .checked_add(opcode_size)
                        .ok_or(MveError::TruncatedOpcode {
                            offset: pos,
                            needed: data_start,
                            len: payload_end,
                        })?;

                if data_end > payload_end {
                    return Err(MveError::TruncatedOpcode {
                        offset: pos,
                        needed: data_end,
                        len: payload_end,
                    });
                }

                match opcode {
                    OP_END_OF_STREAM => {
                        end_of_stream = true;
                        break;
                    }
                    OP_END_OF_CHUNK => break,
                    OP_CREATE_TIMER => {
                        if opcode_size >= 6 {
                            let rate = u32::from_le_bytes([
                                data[data_start],
                                data[data_start + 1],
                                data[data_start + 2],
                                data[data_start + 3],
                            ]);
                            let divider =
                                u16::from_le_bytes([data[data_start + 4], data[data_start + 5]]);
                            frame_delay_us = rate.saturating_mul(u32::from(divider));
                        }
                    }
                    OP_ALLOC_AUDIO_BUFFERS => {
                        // Version 0 and 1 share the first 6 bytes: flags, sample rate.
                        // `sndConfigure`: bit0 stereo, bit1 16 bit, bit2 delta compression.
                        if opcode_size >= 6 {
                            let flags =
                                u16::from_le_bytes([data[data_start + 2], data[data_start + 3]]);
                            let sample_rate =
                                u16::from_le_bytes([data[data_start + 4], data[data_start + 5]]);
                            let format = MveAudioFormat {
                                channels: if flags & 0x01 != 0 { 2 } else { 1 },
                                sample_rate: u32::from(sample_rate),
                                bits: if flags & 0x02 != 0 { 16 } else { 8 },
                                compressed: flags & 0x04 != 0,
                            };

                            audio = Some(format);
                            events.push(Event::AudioFormat(format));
                        }
                    }
                    OP_SYNCH_AUDIO => {
                        // The oracle calls `sndSync`: a presentation clock marker only.
                    }
                    OP_INIT_VIDEO_BUFFERS => {
                        if opcode_size >= 6 {
                            let width_blocks =
                                u16::from_le_bytes([data[data_start], data[data_start + 1]]);
                            let height_blocks =
                                u16::from_le_bytes([data[data_start + 2], data[data_start + 3]]);
                            let fqty = if opcode_size >= 8 {
                                u16::from_le_bytes([data[data_start + 4], data[data_start + 5]])
                            } else {
                                1
                            };
                            let fqty = fqty.max(1);

                            width = width_blocks.saturating_mul(8);
                            height = height_blocks.saturating_mul(fqty).saturating_mul(8);
                        }
                    }
                    OP_SET_PALETTE => {
                        if opcode_size >= 4 {
                            let start =
                                u16::from_le_bytes([data[data_start], data[data_start + 1]]);
                            let count =
                                u16::from_le_bytes([data[data_start + 2], data[data_start + 3]]);
                            let available = (data_end - data_start - 4) / 3;

                            if available > 0 {
                                events.push(Event::Palette(PaletteUpdate {
                                    start,
                                    count: count.min(available as u16),
                                    offset: data_start + 4,
                                }));
                            }
                        }
                    }
                    OP_SET_DECODING_MAP1 => {
                        change_map = Some((data_start, opcode_size));
                    }
                    OP_SET_DECODING_MAP2 => {
                        map = Some((data_start, opcode_size));
                    }
                    OP_AUDIO_FRAME | OP_SILENCE_FRAME => {
                        // `MveOpcodeAudioFrame`: id u16, mask u16, length u16, data.
                        if opcode_size >= AUDIO_HEADER_SIZE {
                            let mask =
                                u16::from_le_bytes([data[data_start + 2], data[data_start + 3]]);
                            let decoded_len =
                                u16::from_le_bytes([data[data_start + 4], data[data_start + 5]])
                                    as usize;

                            events.push(Event::Audio(AudioSpec {
                                data_offset: data_start + AUDIO_HEADER_SIZE,
                                data_len: opcode_size - AUDIO_HEADER_SIZE,
                                decoded_len,
                                mask,
                                silent: opcode == OP_SILENCE_FRAME,
                            }));
                        }
                    }
                    OP_VIDEO_DATA_11 | OP_VIDEO_DATA_10 | OP_VIDEO_DATA_06 => {
                        if opcode_size < VIDEO_HEADER_SIZE {
                            return Err(MveError::TruncatedVideoHeader {
                                frame: frame_count,
                                len: opcode_size,
                            });
                        }

                        // `nfDecomp` (0x06) has no decoding map; `nfDecompChg` (0x10) needs
                        // the change map (0x0E) and the block-code map (0x0F); `nfPkDecomp`
                        // (0x11) needs the block-code map only.
                        let (map_offset, map_len, change_map_offset, change_map_len) =
                            match opcode {
                                OP_VIDEO_DATA_06 => (data_start, 0, data_start, 0),
                                OP_VIDEO_DATA_10 => {
                                    let Some((map_offset, map_len)) = map else {
                                        return Err(MveError::MissingDecodingMap {
                                            frame: frame_count,
                                        });
                                    };
                                    let Some((change_offset, change_len)) = change_map else {
                                        return Err(MveError::MissingChangeMap {
                                            frame: frame_count,
                                        });
                                    };

                                    (map_offset, map_len, change_offset, change_len)
                                }
                                _ => {
                                    let Some((map_offset, map_len)) = map else {
                                        return Err(MveError::MissingDecodingMap {
                                            frame: frame_count,
                                        });
                                    };

                                    (map_offset, map_len, data_start, 0)
                                }
                            };

                        if video_format_count == 0 {
                            video_formats[0] = opcode;
                            video_format_count = 1;
                        } else if !video_formats[..usize::from(video_format_count)]
                            .contains(&opcode)
                            && usize::from(video_format_count) < video_formats.len()
                        {
                            video_formats[usize::from(video_format_count)] = opcode;
                            video_format_count += 1;
                        }

                        events.push(Event::Frame(FrameSpec {
                            format: opcode,
                            map_offset,
                            map_len,
                            change_map_offset,
                            change_map_len,
                            video_offset: data_start,
                            video_len: opcode_size,
                            header: parse_frame_header(&data, data_start),
                        }));
                        frame_count += 1;
                    }
                    _ => {}
                }

                pos = data_end;
            }

            if end_of_stream {
                break;
            }

            offset = payload_end;
        }

        if width == 0 || height == 0 || frame_count == 0 {
            return Err(MveError::NoVideo);
        }

        if frame_delay_us == 0 {
            frame_delay_us = DEFAULT_FRAME_DELAY_US;
        }

        Ok(Self {
            inner: Arc::new(Inner {
                data,
                events,
                info: MveInfo {
                    width,
                    height,
                    frame_count,
                    frame_delay_us,
                    video_format: video_formats[0],
                    video_formats,
                    video_format_count,
                    audio,
                },
            }),
        })
    }

    /// File metadata.
    pub fn info(&self) -> MveInfo {
        self.inner.info
    }

    /// Creates a decoder with zeroed frames and the initial palette
    /// (`movie_init_palette`: all black, index 255 = 50/50/50).
    pub fn decoder(&self) -> MveDecoder {
        self.decoder_with_track(0)
    }

    /// Creates a decoder with an audio track selected (`movie_run(..., track, ...)`).
    ///
    /// `track` is `1 << track` against the `mask` of an audio frame, exactly like
    /// `rm_track_bit` in `MVELIB32.c` (`Track_English` = 0 in `movie.cpp`).
    /// A track without sound in the container stays silent — the video still plays.
    pub fn decoder_with_track(&self, track: u8) -> MveDecoder {
        let width = usize::from(self.inner.info.width);
        let height = usize::from(self.inner.info.height);
        let mut palette = [[0u8; 3]; PALETTE_SIZE];

        palette[PALETTE_SIZE - 1] = [50, 50, 50];

        MveDecoder {
            inner: Arc::clone(&self.inner),
            event: 0,
            frame_index: 0,
            cur: vec![0u8; width * height],
            prv: vec![0u8; width * height],
            palette,
            finished: false,
            track: track % AUDIO_TRACKS,
            audio_format: self.inner.info.audio,
            pending_audio: Vec::new(),
            dropped_audio: 0,
            audio_events_without_format: 0,
        }
    }
}

fn expected_checksum(version: u16) -> u16 {
    (!version).wrapping_add(0x1234)
}

fn parse_frame_header(data: &[u8], offset: usize) -> FrameHeader {
    let word = |index: usize| -> u16 {
        u16::from_le_bytes([data[offset + index * 2], data[offset + index * 2 + 1]])
    };

    FrameHeader {
        x: word(2),
        y: word(3),
        width: word(4),
        height: word(5),
        flags: word(6),
    }
}

// ---------------------------------------------------------------------------
// Frame decoder
// ---------------------------------------------------------------------------

/// Frame-by-frame MVE decoder.
///
/// The frames are delta-encoded, so they must be decoded sequentially from
/// zero. `next_frame()` returns an indexed frame
/// (`width * height` bytes) and advances; the palette is [`Self::palette`].
#[derive(Debug, Clone)]
pub struct MveDecoder {
    inner: Arc<Inner>,
    event: usize,
    frame_index: usize,
    /// Buffer the current frame is written into.
    cur: Vec<u8>,
    /// The previously shown frame (for copying blocks).
    prv: Vec<u8>,
    palette: [[u8; 3]; PALETTE_SIZE],
    finished: bool,
    /// Language track (`1 << track` against the audio-frame mask).
    track: u8,
    /// Audio format of the current record (`ALLOC_AUDIO_BUFFERS`).
    audio_format: Option<MveAudioFormat>,
    /// Decoded audio chunks the consumer has not taken yet.
    pending_audio: Vec<MveAudioChunk>,
    /// Audio chunks dropped by the [`MAX_PENDING_AUDIO_CHUNKS`] cap.
    dropped_audio: usize,
    /// Audio frames that arrived before any `ALLOC_AUDIO_BUFFERS`.
    audio_events_without_format: usize,
}

impl MveDecoder {
    /// Metadata.
    pub fn info(&self) -> MveInfo {
        self.inner.info
    }

    /// Current palette (8-bit components; updated by `SET_PALETTE` opcodes).
    pub fn palette(&self) -> &[[u8; 3]; PALETTE_SIZE] {
        &self.palette
    }

    /// How many frames have already been decoded.
    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    /// The decoder reached the end of the file.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Selected audio track (`0` — English, as `movie_get_track_for_language`).
    pub fn track(&self) -> u8 {
        self.track
    }

    /// Audio format seen by the decoder so far.
    pub fn audio_format(&self) -> Option<MveAudioFormat> {
        self.audio_format
    }

    /// Decoded audio chunks accumulated since the previous call.
    ///
    /// The chunks belong to the video frames decoded since the previous
    /// [`Self::take_audio`] (one chunk per frame in the M.A.X. movies); the
    /// caller schedules them on its own clock, the decoder has none.
    pub fn take_audio(&mut self) -> Vec<MveAudioChunk> {
        std::mem::take(&mut self.pending_audio)
    }

    /// Audio chunks dropped because the consumer did not drain them
    /// (bounded memory; a non-zero value is a presentation diagnostic).
    pub fn dropped_audio_chunks(&self) -> usize {
        self.dropped_audio
    }

    /// Audio frames skipped because the stream had no `ALLOC_AUDIO_BUFFERS`.
    pub fn audio_events_without_format(&self) -> usize {
        self.audio_events_without_format
    }

    /// Decodes the next frame. `Ok(None)` — the file has ended.
    ///
    /// Audio opcodes met on the way are decoded into [`Self::take_audio`]; they
    /// never fail the video stream.
    pub fn next_frame(&mut self) -> Result<Option<&[u8]>, MveError> {
        loop {
            if self.event >= self.inner.events.len() {
                self.finished = true;

                return Ok(None);
            }

            let event = self.inner.events[self.event];

            self.event += 1;

            match event {
                Event::Palette(update) => self.apply_palette(update),
                Event::AudioFormat(format) => self.audio_format = Some(format),
                Event::Audio(spec) => self.decode_audio(spec),
                Event::Frame(frame) => {
                    let inner = Arc::clone(&self.inner);

                    self.decode_frame(&inner, frame)?;

                    return Ok(Some(&self.cur));
                }
            }
        }
    }

    /// Decodes one audio opcode into PCM16 (`sndAdd` without the device).
    fn decode_audio(&mut self, spec: AudioSpec) {
        // `if (rm_track_bit & opcode->mask)` — the same gate for sound and silence.
        if u16::from(1u8 << self.track.min(15)) & spec.mask == 0 {
            return;
        }

        let Some(format) = self.audio_format else {
            self.audio_events_without_format += 1;

            return;
        };

        let samples = if spec.silent {
            vec![0i16; format.samples_for(spec.decoded_len)]
        } else {
            let data = &self.inner.data;
            let start = spec.data_offset;
            let end = start.saturating_add(spec.data_len).min(data.len());

            if start >= data.len() || end < start {
                return;
            }

            decode_pcm(format, &data[start..end], spec.decoded_len)
        };

        if self.pending_audio.len() >= MAX_PENDING_AUDIO_CHUNKS {
            self.dropped_audio += 1;

            return;
        }

        self.pending_audio.push(MveAudioChunk {
            samples,
            channels: format.channels,
            sample_rate: format.sample_rate,
            silent: spec.silent,
        });
    }

    fn apply_palette(&mut self, update: PaletteUpdate) {
        let data = &self.inner.data;

        for index in 0..usize::from(update.count) {
            let color = usize::from(update.start) + index;

            if color >= PALETTE_SIZE {
                break;
            }

            let offset = update.offset + index * 3;

            if offset + 2 >= data.len() {
                break;
            }

            // 6-bit VGA palette -> 8-bit components, like `movie_cb_set_palette`.
            self.palette[color] = [
                data[offset].saturating_mul(4),
                data[offset + 1].saturating_mul(4),
                data[offset + 2].saturating_mul(4),
            ];
        }
    }

    fn decode_frame(&mut self, inner: &Inner, frame: FrameSpec) -> Result<(), MveError> {
        let info = inner.info;
        let width = usize::from(info.width);
        let height = usize::from(info.height);
        let header = frame.header;

        if !matches!(
            frame.format,
            OP_VIDEO_DATA_06 | OP_VIDEO_DATA_10 | OP_VIDEO_DATA_11
        ) {
            return Err(MveError::UnsupportedVideoFormat {
                format: frame.format,
            });
        }

        if header.width == 0 || header.height == 0 {
            return Err(MveError::BadFrameRect {
                frame: self.frame_index,
                x: header.x,
                y: header.y,
                width: header.width,
                height: header.height,
            });
        }

        let frame_width = usize::from(header.width) * BLOCK_SIZE;
        let frame_height = usize::from(header.height) * BLOCK_SIZE;

        if usize::from(header.x) + frame_width > width
            || usize::from(header.y) + frame_height > height
        {
            return Err(MveError::BadFrameRect {
                frame: self.frame_index,
                x: header.x,
                y: header.y,
                width: header.width,
                height: header.height,
            });
        }

        let map_end = frame.map_offset + frame.map_len;
        let change_map_end = frame.change_map_offset + frame.change_map_len;
        let video_end = frame.video_offset + frame.video_len;

        if map_end > inner.data.len()
            || change_map_end > inner.data.len()
            || video_end > inner.data.len()
        {
            return Err(MveError::TruncatedVideo {
                frame: self.frame_index,
            });
        }

        let map = &inner.data[frame.map_offset..map_end];
        let change_map = &inner.data[frame.change_map_offset..change_map_end];
        let video = &inner.data[frame.video_offset..video_end];

        if video.len() < VIDEO_HEADER_SIZE {
            return Err(MveError::TruncatedVideoHeader {
                frame: self.frame_index,
                len: video.len(),
            });
        }

        let blocks = usize::from(header.width) * usize::from(header.height);

        if frame.format != OP_VIDEO_DATA_06 && map.len() < blocks / 2 {
            return Err(MveError::BadDecodingMap {
                frame: self.frame_index,
                needed: blocks / 2,
                len: map.len(),
            });
        }

        if frame.format == OP_VIDEO_DATA_10 {
            let needed = blocks.div_ceil(8);

            if change_map.len() < needed {
                return Err(MveError::BadChangeMap {
                    frame: self.frame_index,
                    needed,
                    len: change_map.len(),
                });
            }
        }

        if header.flags & 0x01 != 0 {
            std::mem::swap(&mut self.cur, &mut self.prv);
        }

        match frame.format {
            OP_VIDEO_DATA_06 => decode_raw_frame(
                &video[VIDEO_HEADER_SIZE..],
                self.frame_index,
                &header,
                width,
                &mut self.cur,
            )?,
            OP_VIDEO_DATA_10 => decode_change_frame(
                map,
                change_map,
                &video[VIDEO_HEADER_SIZE..],
                self.frame_index,
                &header,
                width,
                &mut self.cur,
                &self.prv,
            )?,
            _ => decode_block_frame(
                map,
                &video[VIDEO_HEADER_SIZE..],
                self.frame_index,
                &header,
                width,
                &mut self.cur,
                &self.prv,
            )?,
        }

        self.frame_index += 1;

        Ok(())
    }
}

/// `VIDEO_DATA_11` — `nfPkDecomp`: block codes from decoding map 2.
#[allow(clippy::too_many_arguments)]
fn decode_block_frame(
    map: &[u8],
    stream: &[u8],
    frame: usize,
    header: &FrameHeader,
    stride: usize,
    cur: &mut [u8],
    prv: &[u8],
) -> Result<(), MveError> {
    let mut cursor = VideoCursor::new(stream, frame);
    let mut pattern = PatternScratch::new();
    let mut map_pos = 0usize;

    for block_y in 0..usize::from(header.height) {
        for pair in 0..usize::from(header.width) / 2 {
            let byte = map[map_pos];

            map_pos += 1;

            for (offset, opcode) in [(0usize, byte & 0x0F), (1usize, byte >> 4)] {
                let block_x = pair * 2 + offset;

                decode_block(
                    opcode,
                    frame,
                    block_x,
                    block_y,
                    header,
                    stride,
                    cur,
                    prv,
                    &mut cursor,
                    &mut pattern,
                )?;
            }
        }
    }

    Ok(())
}

/// `VIDEO_DATA_06` — `nfDecomp`: raw 8×8 blocks without a decoding map.
///
/// The oracle dispatches the opcode to `nfDecomp` (a `\todo` stub there); the payload is
/// the literal block stream of the Interplay container: 64 palette indices per block,
/// row by row, blocks in the same order as the decoding map of `VIDEO_DATA_11`.
fn decode_raw_frame(
    stream: &[u8],
    frame: usize,
    header: &FrameHeader,
    stride: usize,
    cur: &mut [u8],
) -> Result<(), MveError> {
    let mut offset = 0usize;

    for block_y in 0..usize::from(header.height) {
        for block_x in 0..usize::from(header.width) {
            let dst = block_offset(header, block_x, block_y, stride);
            let end = offset + 64;

            let Some(block) = stream.get(offset..end) else {
                return Err(MveError::TruncatedVideo { frame });
            };

            for row in 0..BLOCK_SIZE {
                let at = dst + row * stride;

                if at + BLOCK_SIZE <= cur.len() {
                    cur[at..at + BLOCK_SIZE]
                        .copy_from_slice(&block[row * BLOCK_SIZE..row * BLOCK_SIZE + BLOCK_SIZE]);
                }
            }

            offset = end;
        }
    }

    Ok(())
}

/// `VIDEO_DATA_10` — `nfDecompChg`: only the blocks with a set change bit are decoded.
///
/// Decoding map 1 (opcode 0x0E) is the change mask — one bit per 8×8 block in row-major
/// order, bit 0 of a byte is the earlier block. The block codes come from decoding map 2
/// (opcode 0x0F), the same nibble-pair layout as `VIDEO_DATA_11` (low nibble — the left
/// block of the pair). Blocks with a clear bit are copied from the previous frame, exactly
/// what `nfAdvance` + `nfDecompChg` do in the oracle (the double buffer after the swap
/// holds the frame before the last one, so the copy source is `prv`).
#[allow(clippy::too_many_arguments)]
fn decode_change_frame(
    map: &[u8],
    change_map: &[u8],
    stream: &[u8],
    frame: usize,
    header: &FrameHeader,
    stride: usize,
    cur: &mut [u8],
    prv: &[u8],
) -> Result<(), MveError> {
    let mut cursor = VideoCursor::new(stream, frame);
    let mut pattern = PatternScratch::new();
    let blocks_x = usize::from(header.width);

    for block_y in 0..usize::from(header.height) {
        for block_x in 0..blocks_x {
            let block = block_y * blocks_x + block_x;

            if change_map[block / 8] & (1 << (block % 8)) == 0 {
                // The block did not change: take it from the previous frame. The
                // double buffer after `nfAdvance` holds the frame *before* the last
                // one, so the block is copied from `prv` (the last shown frame).
                let dst = block_offset(header, block_x, block_y, stride);

                copy_block(cur, dst, prv, dst as isize, stride);
                continue;
            }

            let pair_byte = map[block_y * blocks_x / 2 + block_x / 2];
            let opcode = if block_x % 2 == 0 {
                pair_byte & 0x0F
            } else {
                pair_byte >> 4
            };

            decode_block(
                opcode,
                frame,
                block_x,
                block_y,
                header,
                stride,
                cur,
                prv,
                &mut cursor,
                &mut pattern,
            )?;
        }
    }

    Ok(())
}

/// Decodes the payload of `AUDIO_FRAME` into PCM16 (an exact port of `sndAdd` +
/// `sndDecompM16`/`sndDecompS16`).
///
/// 16-bit delta streams: two initial samples (left, then right for stereo), then one
/// byte per sample added through the oracle's `snd_8to16` table. Uncompressed 8-bit
/// PCM is converted to signed 16-bit exactly as miniaudio would play unsigned 8-bit
/// samples (the oracle hands the raw bytes to the device).
fn decode_pcm(format: MveAudioFormat, input: &[u8], decoded_len: usize) -> Vec<i16> {
    let samples = format.samples_for(decoded_len);

    if format.bits == 8 {
        return input
            .iter()
            .map(|&byte| (i16::from(byte) - 128) * 256)
            .collect();
    }

    if !format.compressed {
        return input
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
    }

    let mut out = Vec::with_capacity(samples);

    if format.channels >= 2 {
        // `sndDecompS16`: left/right alternate, the first two 16-bit values are absolute.
        if input.len() < 4 {
            return out;
        }

        let mut left = i16::from_le_bytes([input[0], input[1]]);
        let mut right = i16::from_le_bytes([input[2], input[3]]);

        out.push(left);
        out.push(right);

        let mut index = 4usize;

        while out.len() < samples {
            let Some(&delta) = input.get(index) else {
                break;
            };

            left = left.wrapping_add(SND_8TO16[usize::from(delta)]);
            out.push(left);
            index += 1;

            if out.len() >= samples {
                break;
            }

            let Some(&delta) = input.get(index) else {
                break;
            };

            right = right.wrapping_add(SND_8TO16[usize::from(delta)]);
            out.push(right);
            index += 1;
        }
    } else {
        // `sndDecompM16`: one absolute sample, then one delta byte per sample.
        if input.len() < 2 {
            return out;
        }

        let mut left = i16::from_le_bytes([input[0], input[1]]);

        out.push(left);

        for &delta in &input[2..] {
            if out.len() >= samples {
                break;
            }

            left = left.wrapping_add(SND_8TO16[usize::from(delta)]);
            out.push(left);
        }
    }

    out.truncate(samples);

    out
}

/// Cursor over the compressed block stream of a frame.
struct VideoCursor<'a> {
    data: &'a [u8],
    pos: usize,
    frame: usize,
}

impl<'a> VideoCursor<'a> {
    fn new(data: &'a [u8], frame: usize) -> Self {
        Self {
            data,
            pos: 0,
            frame,
        }
    }

    fn byte(&mut self) -> Result<u8, MveError> {
        let value = *self
            .data
            .get(self.pos)
            .ok_or(MveError::TruncatedVideo { frame: self.frame })?;

        self.pos += 1;

        Ok(value)
    }

    fn block(&mut self) -> Result<[u8; 64], MveError> {
        let mut block = [0u8; 64];

        for pixel in &mut block {
            *pixel = self.byte()?;
        }

        Ok(block)
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_block(
    opcode: u8,
    frame: usize,
    block_x: usize,
    block_y: usize,
    header: &FrameHeader,
    stride: usize,
    cur: &mut [u8],
    prv: &[u8],
    cursor: &mut VideoCursor<'_>,
    pattern: &mut PatternScratch,
) -> Result<(), MveError> {
    let dst = block_offset(header, block_x, block_y, stride);

    // Pattern blocks 0x7…0xA are ported from the oracle's `nfFillPattern*`
    // (tables `nf_pk_lut_4/5/6`), otherwise the order of pairs in some branches
    // differs from `nfPkDecomp` by a couple of pixels.
    if (0x7..=0xA).contains(&opcode) {
        return decode_pattern_block(opcode, frame, dst, stride, cur, cursor, pattern);
    }

    match opcode {
        // Copy of a block from the previous frame.
        0x0 => {
            copy_block(cur, dst, prv, dst as isize, stride);
        }
        // The block remains from the frame before (the double buffer already contains it).
        0x1 => {}
        // Motion vector from the frame before (positive).
        0x2 => {
            let byte = cursor.byte()?;
            let (dx, dy) = motion_vector(byte);
            let src = dst as isize + dy * stride as isize + dx;

            copy_within_block(cur, dst, src, stride);
        }
        // Motion vector from the current frame (negative).
        0x3 => {
            let byte = cursor.byte()?;
            let (dx, dy) = motion_vector(byte);
            let src = dst as isize - dy * stride as isize - dx;

            copy_within_block(cur, dst, src, stride);
        }
        // Copy from the previous frame with a shift of −8..7 on both axes.
        0x4 => {
            let byte = cursor.byte()?;
            let dx = isize::from(byte & 0x0F) - 8;
            let dy = isize::from(byte >> 4) - 8;
            let src = dst as isize + dy * stride as isize + dx;

            copy_block(cur, dst, prv, src, stride);
        }
        // Copy from the previous frame with an extended range (i8, i8).
        0x5 => {
            let dx = cursor.byte()? as i8 as isize;
            let dy = cursor.byte()? as i8 as isize;
            let src = dst as isize + dy * stride as isize + dx;

            copy_block(cur, dst, prv, src, stride);
        }
        // Does not occur in M.A.X.; FFmpeg does not touch the buffer either.
        0x6 => {}
        0xB => {
            let block = cursor.block()?;

            write_block(cur, dst, stride, &block);
        }
        0xC => {
            let mut block = [0u8; 64];

            for y in (0..8).step_by(2) {
                for x in (0..8).step_by(2) {
                    let value = cursor.byte()?;

                    block[y * 8 + x] = value;
                    block[y * 8 + x + 1] = value;
                    block[(y + 1) * 8 + x] = value;
                    block[(y + 1) * 8 + x + 1] = value;
                }
            }

            write_block(cur, dst, stride, &block);
        }
        0xD => {
            let mut block = [0u8; 64];
            let mut colors = [0u8; 2];

            for y in 0..8 {
                if y % 4 == 0 {
                    colors[0] = cursor.byte()?;
                    colors[1] = cursor.byte()?;
                }

                for x in 0..4 {
                    block[y * 8 + x] = colors[0];
                    block[y * 8 + 4 + x] = colors[1];
                }
            }

            write_block(cur, dst, stride, &block);
        }
        0xE => {
            let value = cursor.byte()?;
            let block = [value; 64];

            write_block(cur, dst, stride, &block);
        }
        0xF => {
            let mut block = [0u8; 64];
            let sample = [cursor.byte()?, cursor.byte()?];

            for y in 0..8 {
                for x in (0..8).step_by(2) {
                    block[y * 8 + x] = sample[y & 1];
                    block[y * 8 + x + 1] = sample[1 - (y & 1)];
                }
            }

            write_block(cur, dst, stride, &block);
        }
        other => {
            return Err(MveError::UnsupportedBlock {
                frame,
                opcode: other,
            });
        }
    }

    Ok(())
}

const NF_PK_LUT_4: [u32; 16] = [
    0xC3C3C3C3, 0xC3C3C1C3, 0xC3C3C3C1, 0xC3C3C1C1, 0xC1C3C3C3, 0xC1C3C1C3, 0xC1C3C3C1, 0xC1C3C1C1,
    0xC3C1C3C3, 0xC3C1C1C3, 0xC3C1C3C1, 0xC3C1C1C1, 0xC1C1C3C3, 0xC1C1C1C3, 0xC1C1C3C1, 0xC1C1C1C1,
];

const NF_PK_LUT_5: [u32; 256] = [
    0xC3C3C3C3, 0xC3C3C2C3, 0xC3C3C1C3, 0xC3C3C5C3, 0xC3C3C3C2, 0xC3C3C2C2, 0xC3C3C1C2, 0xC3C3C5C2,
    0xC3C3C3C1, 0xC3C3C2C1, 0xC3C3C1C1, 0xC3C3C5C1, 0xC3C3C3C5, 0xC3C3C2C5, 0xC3C3C1C5, 0xC3C3C5C5,
    0xC2C3C3C3, 0xC2C3C2C3, 0xC2C3C1C3, 0xC2C3C5C3, 0xC2C3C3C2, 0xC2C3C2C2, 0xC2C3C1C2, 0xC2C3C5C2,
    0xC2C3C3C1, 0xC2C3C2C1, 0xC2C3C1C1, 0xC2C3C5C1, 0xC2C3C3C5, 0xC2C3C2C5, 0xC2C3C1C5, 0xC2C3C5C5,
    0xC1C3C3C3, 0xC1C3C2C3, 0xC1C3C1C3, 0xC1C3C5C3, 0xC1C3C3C2, 0xC1C3C2C2, 0xC1C3C1C2, 0xC1C3C5C2,
    0xC1C3C3C1, 0xC1C3C2C1, 0xC1C3C1C1, 0xC1C3C5C1, 0xC1C3C3C5, 0xC1C3C2C5, 0xC1C3C1C5, 0xC1C3C5C5,
    0xC5C3C3C3, 0xC5C3C2C3, 0xC5C3C1C3, 0xC5C3C5C3, 0xC5C3C3C2, 0xC5C3C2C2, 0xC5C3C1C2, 0xC5C3C5C2,
    0xC5C3C3C1, 0xC5C3C2C1, 0xC5C3C1C1, 0xC5C3C5C1, 0xC5C3C3C5, 0xC5C3C2C5, 0xC5C3C1C5, 0xC5C3C5C5,
    0xC3C2C3C3, 0xC3C2C2C3, 0xC3C2C1C3, 0xC3C2C5C3, 0xC3C2C3C2, 0xC3C2C2C2, 0xC3C2C1C2, 0xC3C2C5C2,
    0xC3C2C3C1, 0xC3C2C2C1, 0xC3C2C1C1, 0xC3C2C5C1, 0xC3C2C3C5, 0xC3C2C2C5, 0xC3C2C1C5, 0xC3C2C5C5,
    0xC2C2C3C3, 0xC2C2C2C3, 0xC2C2C1C3, 0xC2C2C5C3, 0xC2C2C3C2, 0xC2C2C2C2, 0xC2C2C1C2, 0xC2C2C5C2,
    0xC2C2C3C1, 0xC2C2C2C1, 0xC2C2C1C1, 0xC2C2C5C1, 0xC2C2C3C5, 0xC2C2C2C5, 0xC2C2C1C5, 0xC2C2C5C5,
    0xC1C2C3C3, 0xC1C2C2C3, 0xC1C2C1C3, 0xC1C2C5C3, 0xC1C2C3C2, 0xC1C2C2C2, 0xC1C2C1C2, 0xC1C2C5C2,
    0xC1C2C3C1, 0xC1C2C2C1, 0xC1C2C1C1, 0xC1C2C5C1, 0xC1C2C3C5, 0xC1C2C2C5, 0xC1C2C1C5, 0xC1C2C5C5,
    0xC5C2C3C3, 0xC5C2C2C3, 0xC5C2C1C3, 0xC5C2C5C3, 0xC5C2C3C2, 0xC5C2C2C2, 0xC5C2C1C2, 0xC5C2C5C2,
    0xC5C2C3C1, 0xC5C2C2C1, 0xC5C2C1C1, 0xC5C2C5C1, 0xC5C2C3C5, 0xC5C2C2C5, 0xC5C2C1C5, 0xC5C2C5C5,
    0xC3C1C3C3, 0xC3C1C2C3, 0xC3C1C1C3, 0xC3C1C5C3, 0xC3C1C3C2, 0xC3C1C2C2, 0xC3C1C1C2, 0xC3C1C5C2,
    0xC3C1C3C1, 0xC3C1C2C1, 0xC3C1C1C1, 0xC3C1C5C1, 0xC3C1C3C5, 0xC3C1C2C5, 0xC3C1C1C5, 0xC3C1C5C5,
    0xC2C1C3C3, 0xC2C1C2C3, 0xC2C1C1C3, 0xC2C1C5C3, 0xC2C1C3C2, 0xC2C1C2C2, 0xC2C1C1C2, 0xC2C1C5C2,
    0xC2C1C3C1, 0xC2C1C2C1, 0xC2C1C1C1, 0xC2C1C5C1, 0xC2C1C3C5, 0xC2C1C2C5, 0xC2C1C1C5, 0xC2C1C5C5,
    0xC1C1C3C3, 0xC1C1C2C3, 0xC1C1C1C3, 0xC1C1C5C3, 0xC1C1C3C2, 0xC1C1C2C2, 0xC1C1C1C2, 0xC1C1C5C2,
    0xC1C1C3C1, 0xC1C1C2C1, 0xC1C1C1C1, 0xC1C1C5C1, 0xC1C1C3C5, 0xC1C1C2C5, 0xC1C1C1C5, 0xC1C1C5C5,
    0xC5C1C3C3, 0xC5C1C2C3, 0xC5C1C1C3, 0xC5C1C5C3, 0xC5C1C3C2, 0xC5C1C2C2, 0xC5C1C1C2, 0xC5C1C5C2,
    0xC5C1C3C1, 0xC5C1C2C1, 0xC5C1C1C1, 0xC5C1C5C1, 0xC5C1C3C5, 0xC5C1C2C5, 0xC5C1C1C5, 0xC5C1C5C5,
    0xC3C5C3C3, 0xC3C5C2C3, 0xC3C5C1C3, 0xC3C5C5C3, 0xC3C5C3C2, 0xC3C5C2C2, 0xC3C5C1C2, 0xC3C5C5C2,
    0xC3C5C3C1, 0xC3C5C2C1, 0xC3C5C1C1, 0xC3C5C5C1, 0xC3C5C3C5, 0xC3C5C2C5, 0xC3C5C1C5, 0xC3C5C5C5,
    0xC2C5C3C3, 0xC2C5C2C3, 0xC2C5C1C3, 0xC2C5C5C3, 0xC2C5C3C2, 0xC2C5C2C2, 0xC2C5C1C2, 0xC2C5C5C2,
    0xC2C5C3C1, 0xC2C5C2C1, 0xC2C5C1C1, 0xC2C5C5C1, 0xC2C5C3C5, 0xC2C5C2C5, 0xC2C5C1C5, 0xC2C5C5C5,
    0xC1C5C3C3, 0xC1C5C2C3, 0xC1C5C1C3, 0xC1C5C5C3, 0xC1C5C3C2, 0xC1C5C2C2, 0xC1C5C1C2, 0xC1C5C5C2,
    0xC1C5C3C1, 0xC1C5C2C1, 0xC1C5C1C1, 0xC1C5C5C1, 0xC1C5C3C5, 0xC1C5C2C5, 0xC1C5C1C5, 0xC1C5C5C5,
    0xC5C5C3C3, 0xC5C5C2C3, 0xC5C5C1C3, 0xC5C5C5C3, 0xC5C5C3C2, 0xC5C5C2C2, 0xC5C5C1C2, 0xC5C5C5C2,
    0xC5C5C3C1, 0xC5C5C2C1, 0xC5C5C1C1, 0xC5C5C5C1, 0xC5C5C3C5, 0xC5C5C2C5, 0xC5C5C1C5, 0xC5C5C5C5,
];

const NF_PK_LUT_6: [u32; 256] = [
    0xE3C3E3C3, 0xE3C7E3C3, 0xE3C1E3C3, 0xE3C5E3C3, 0xE7C3E3C3, 0xE7C7E3C3, 0xE7C1E3C3, 0xE7C5E3C3,
    0xE1C3E3C3, 0xE1C7E3C3, 0xE1C1E3C3, 0xE1C5E3C3, 0xE5C3E3C3, 0xE5C7E3C3, 0xE5C1E3C3, 0xE5C5E3C3,
    0xE3C3E3C7, 0xE3C7E3C7, 0xE3C1E3C7, 0xE3C5E3C7, 0xE7C3E3C7, 0xE7C7E3C7, 0xE7C1E3C7, 0xE7C5E3C7,
    0xE1C3E3C7, 0xE1C7E3C7, 0xE1C1E3C7, 0xE1C5E3C7, 0xE5C3E3C7, 0xE5C7E3C7, 0xE5C1E3C7, 0xE5C5E3C7,
    0xE3C3E3C1, 0xE3C7E3C1, 0xE3C1E3C1, 0xE3C5E3C1, 0xE7C3E3C1, 0xE7C7E3C1, 0xE7C1E3C1, 0xE7C5E3C1,
    0xE1C3E3C1, 0xE1C7E3C1, 0xE1C1E3C1, 0xE1C5E3C1, 0xE5C3E3C1, 0xE5C7E3C1, 0xE5C1E3C1, 0xE5C5E3C1,
    0xE3C3E3C5, 0xE3C7E3C5, 0xE3C1E3C5, 0xE3C5E3C5, 0xE7C3E3C5, 0xE7C7E3C5, 0xE7C1E3C5, 0xE7C5E3C5,
    0xE1C3E3C5, 0xE1C7E3C5, 0xE1C1E3C5, 0xE1C5E3C5, 0xE5C3E3C5, 0xE5C7E3C5, 0xE5C1E3C5, 0xE5C5E3C5,
    0xE3C3E7C3, 0xE3C7E7C3, 0xE3C1E7C3, 0xE3C5E7C3, 0xE7C3E7C3, 0xE7C7E7C3, 0xE7C1E7C3, 0xE7C5E7C3,
    0xE1C3E7C3, 0xE1C7E7C3, 0xE1C1E7C3, 0xE1C5E7C3, 0xE5C3E7C3, 0xE5C7E7C3, 0xE5C1E7C3, 0xE5C5E7C3,
    0xE3C3E7C7, 0xE3C7E7C7, 0xE3C1E7C7, 0xE3C5E7C7, 0xE7C3E7C7, 0xE7C7E7C7, 0xE7C1E7C7, 0xE7C5E7C7,
    0xE1C3E7C7, 0xE1C7E7C7, 0xE1C1E7C7, 0xE1C5E7C7, 0xE5C3E7C7, 0xE5C7E7C7, 0xE5C1E7C7, 0xE5C5E7C7,
    0xE3C3E7C1, 0xE3C7E7C1, 0xE3C1E7C1, 0xE3C5E7C1, 0xE7C3E7C1, 0xE7C7E7C1, 0xE7C1E7C1, 0xE7C5E7C1,
    0xE1C3E7C1, 0xE1C7E7C1, 0xE1C1E7C1, 0xE1C5E7C1, 0xE5C3E7C1, 0xE5C7E7C1, 0xE5C1E7C1, 0xE5C5E7C1,
    0xE3C3E7C5, 0xE3C7E7C5, 0xE3C1E7C5, 0xE3C5E7C5, 0xE7C3E7C5, 0xE7C7E7C5, 0xE7C1E7C5, 0xE7C5E7C5,
    0xE1C3E7C5, 0xE1C7E7C5, 0xE1C1E7C5, 0xE1C5E7C5, 0xE5C3E7C5, 0xE5C7E7C5, 0xE5C1E7C5, 0xE5C5E7C5,
    0xE3C3E1C3, 0xE3C7E1C3, 0xE3C1E1C3, 0xE3C5E1C3, 0xE7C3E1C3, 0xE7C7E1C3, 0xE7C1E1C3, 0xE7C5E1C3,
    0xE1C3E1C3, 0xE1C7E1C3, 0xE1C1E1C3, 0xE1C5E1C3, 0xE5C3E1C3, 0xE5C7E1C3, 0xE5C1E1C3, 0xE5C5E1C3,
    0xE3C3E1C7, 0xE3C7E1C7, 0xE3C1E1C7, 0xE3C5E1C7, 0xE7C3E1C7, 0xE7C7E1C7, 0xE7C1E1C7, 0xE7C5E1C7,
    0xE1C3E1C7, 0xE1C7E1C7, 0xE1C1E1C7, 0xE1C5E1C7, 0xE5C3E1C7, 0xE5C7E1C7, 0xE5C1E1C7, 0xE5C5E1C7,
    0xE3C3E1C1, 0xE3C7E1C1, 0xE3C1E1C1, 0xE3C5E1C1, 0xE7C3E1C1, 0xE7C7E1C1, 0xE7C1E1C1, 0xE7C5E1C1,
    0xE1C3E1C1, 0xE1C7E1C1, 0xE1C1E1C1, 0xE1C5E1C1, 0xE5C3E1C1, 0xE5C7E1C1, 0xE5C1E1C1, 0xE5C5E1C1,
    0xE3C3E1C5, 0xE3C7E1C5, 0xE3C1E1C5, 0xE3C5E1C5, 0xE7C3E1C5, 0xE7C7E1C5, 0xE7C1E1C5, 0xE7C5E1C5,
    0xE1C3E1C5, 0xE1C7E1C5, 0xE1C1E1C5, 0xE1C5E1C5, 0xE5C3E1C5, 0xE5C7E1C5, 0xE5C1E1C5, 0xE5C5E1C5,
    0xE3C3E5C3, 0xE3C7E5C3, 0xE3C1E5C3, 0xE3C5E5C3, 0xE7C3E5C3, 0xE7C7E5C3, 0xE7C1E5C3, 0xE7C5E5C3,
    0xE1C3E5C3, 0xE1C7E5C3, 0xE1C1E5C3, 0xE1C5E5C3, 0xE5C3E5C3, 0xE5C7E5C3, 0xE5C1E5C3, 0xE5C5E5C3,
    0xE3C3E5C7, 0xE3C7E5C7, 0xE3C1E5C7, 0xE3C5E5C7, 0xE7C3E5C7, 0xE7C7E5C7, 0xE7C1E5C7, 0xE7C5E5C7,
    0xE1C3E5C7, 0xE1C7E5C7, 0xE1C1E5C7, 0xE1C5E5C7, 0xE5C3E5C7, 0xE5C7E5C7, 0xE5C1E5C7, 0xE5C5E5C7,
    0xE3C3E5C1, 0xE3C7E5C1, 0xE3C1E5C1, 0xE3C5E5C1, 0xE7C3E5C1, 0xE7C7E5C1, 0xE7C1E5C1, 0xE7C5E5C1,
    0xE1C3E5C1, 0xE1C7E5C1, 0xE1C1E5C1, 0xE1C5E5C1, 0xE5C3E5C1, 0xE5C7E5C1, 0xE5C1E5C1, 0xE5C5E5C1,
    0xE3C3E5C5, 0xE3C7E5C5, 0xE3C1E5C5, 0xE3C5E5C5, 0xE7C3E5C5, 0xE7C7E5C5, 0xE7C1E5C5, 0xE7C5E5C5,
    0xE1C3E5C5, 0xE1C7E5C5, 0xE1C1E5C5, 0xE1C5E5C5, 0xE5C3E5C5, 0xE5C7E5C5, 0xE5C1E5C5, 0xE5C5E5C5,
];

/// Scratch of pattern blocks: `map1/map2` as in `nfPkDecomp` (per frame).
struct PatternScratch {
    map1: [u8; 512],
    map2: [u32; 256],
}

impl PatternScratch {
    fn new() -> Self {
        Self {
            map1: [0u8; 512],
            map2: [0u32; 256],
        }
    }
}

/// Exact port of the pattern branches of `nfPkDecomp` (block codes 0x7…0xA).
fn decode_pattern_block(
    opcode: u8,
    _frame: usize,
    mut dst: usize,
    stride: usize,
    cur: &mut [u8],
    cursor: &mut VideoCursor<'_>,
    pattern: &mut PatternScratch,
) -> Result<(), MveError> {
    let available = cursor.data.len().saturating_sub(cursor.pos);
    let take = available.min(64);
    let mut buffer = [0u8; 64];
    buffer[..take].copy_from_slice(&cursor.data[cursor.pos..cursor.pos + take]);

    // Code 0xA — a row-by-row port of `nfPkDecomp` (in it every
    // adjustment of `dst` matters, not just the final picture of the block).
    if opcode == 0xA {
        let result = write_case_10(
            cur,
            &mut dst,
            &buffer,
            &mut pattern.map1,
            &mut pattern.map2,
            stride,
        );

        // The `src[0] > src[1]` branch (A) reads 24 bytes, otherwise (B) — 32.
        let consumed = if buffer[0] > buffer[1] { 24 } else { 32 };

        cursor.pos = (cursor.pos + consumed).min(cursor.data.len());

        return result;
    }

    match opcode {
        0x7 => {
            let src = &buffer[..];

            if src[0] > src[1] {
                for i in 0..2 {
                    let value = NF_PK_LUT_4[usize::from(src[2 + i] & 0x0F)];
                    pattern.map1[i * 8..i * 8 + 4].copy_from_slice(&value.to_le_bytes());
                    let value = NF_PK_LUT_4[usize::from(src[2 + i] >> 4)];
                    pattern.map1[4 + i * 8..4 + i * 8 + 4].copy_from_slice(&value.to_le_bytes());
                }

                pattern.map2[0xC1] = (u32::from(src[1]) << 8) | u32::from(src[1]);
                pattern.map2[0xC3] = (u32::from(src[0]) << 8) | u32::from(src[0]);

                for pair in 0..4 {
                    let base = pair * 4;
                    let word0 = (pattern.map2[usize::from(pattern.map1[base])] << 16)
                        | pattern.map2[usize::from(pattern.map1[base + 1])];
                    let word1 = (pattern.map2[usize::from(pattern.map1[base + 2])] << 16)
                        | pattern.map2[usize::from(pattern.map1[base + 3])];

                    for row in [pair * 2, pair * 2 + 1] {
                        put_u32(cur, dst + row * stride, word0);
                        put_u32(cur, dst + row * stride + 4, word1);
                    }
                }

                cursor.pos += 4;
            } else {
                fill_pattern(
                    cur,
                    dst,
                    stride,
                    src,
                    &mut pattern.map1,
                    &mut pattern.map2,
                    8,
                    0,
                    0,
                );
                cursor.pos += 10;
            }
        }
        0x8 => {
            let src = &buffer[..];

            if src[0] > src[1] {
                if src[6] > src[7] {
                    fill_pattern(
                        cur,
                        dst,
                        stride,
                        src,
                        &mut pattern.map1,
                        &mut pattern.map2,
                        4,
                        0,
                        0,
                    );
                    fill_pattern(
                        cur,
                        dst + 4 * stride,
                        stride,
                        &src[6..],
                        &mut pattern.map1,
                        &mut pattern.map2,
                        4,
                        16,
                        0,
                    );
                } else {
                    fill_pattern(
                        cur,
                        dst,
                        stride,
                        src,
                        &mut pattern.map1,
                        &mut pattern.map2,
                        4,
                        0,
                        1,
                    );
                    fill_pattern(
                        cur,
                        dst + 4,
                        stride,
                        &src[6..],
                        &mut pattern.map1,
                        &mut pattern.map2,
                        4,
                        16,
                        2,
                    );
                }

                cursor.pos += 12;
            } else {
                fill_pattern(
                    cur,
                    dst,
                    stride,
                    src,
                    &mut pattern.map1,
                    &mut pattern.map2,
                    2,
                    0,
                    2,
                );
                fill_pattern(
                    cur,
                    dst + 4 * stride,
                    stride,
                    &src[4..],
                    &mut pattern.map1,
                    &mut pattern.map2,
                    2,
                    8,
                    2,
                );
                fill_pattern(
                    cur,
                    dst + 4,
                    stride,
                    &src[8..],
                    &mut pattern.map1,
                    &mut pattern.map2,
                    2,
                    16,
                    2,
                );
                fill_pattern(
                    cur,
                    dst + 4 * stride + 4,
                    stride,
                    &src[12..],
                    &mut pattern.map1,
                    &mut pattern.map2,
                    2,
                    24,
                    2,
                );
                cursor.pos += 16;
            }
        }
        0x9 => {
            let src = &buffer[..];

            if src[0] > src[1] {
                if src[2] > src[3] {
                    fill_pattern11(src, &mut pattern.map1, &mut pattern.map2, 8, 0);

                    for i in 0..4 {
                        fill_pattern12(
                            cur,
                            dst + i * 2 * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                        fill_pattern12(
                            cur,
                            dst + (i * 2 + 1) * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                    }

                    cursor.pos += 12;
                } else {
                    fill_pattern11(src, &mut pattern.map1, &mut pattern.map2, 8, 1);

                    for i in 0..8 {
                        fill_pattern12(
                            cur,
                            dst + i * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 4,
                            1,
                        );
                    }

                    cursor.pos += 12;
                }
            } else if src[2] > src[3] {
                fill_pattern11(src, &mut pattern.map1, &mut pattern.map2, 8, 1);

                for i in 0..4 {
                    fill_pattern12(
                        cur,
                        dst + i * 2 * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 4,
                        1,
                    );
                    fill_pattern12(
                        cur,
                        dst + (i * 2 + 1) * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 4,
                        1,
                    );
                }

                cursor.pos += 8;
            } else {
                fill_pattern11(src, &mut pattern.map1, &mut pattern.map2, 16, 0);

                for i in 0..8 {
                    fill_pattern12(
                        cur,
                        dst + i * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 8,
                        0,
                    );
                }

                cursor.pos += 20;
            }
        }
        0xA => {
            let src = &buffer[..];

            if src[0] > src[1] {
                for i in 0..8 {
                    let value = NF_PK_LUT_6[usize::from(src[4 + i])];
                    pattern.map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                for i in 0..8 {
                    let value = NF_PK_LUT_6[usize::from(src[16 + i])];
                    pattern.map1[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }

                set_colors4(&mut pattern.map2, &src[0..4]);

                if src[12] > src[13] {
                    for i in 0..4 {
                        fill_pattern12(
                            cur,
                            dst + i * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                    }

                    set_colors4(&mut pattern.map2, &src[12..16]);

                    for i in 0..4 {
                        fill_pattern12(
                            cur,
                            dst + (4 + i) * stride,
                            &pattern.map1,
                            &pattern.map2,
                            32 + i * 8,
                            0,
                        );
                    }
                } else {
                    // The left half — full 8-pixel rows.
                    for i in 0..4 {
                        fill_pattern12(
                            cur,
                            dst + i * 2 * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                        fill_pattern12(
                            cur,
                            dst + (i * 2 + 1) * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                    }

                    // The right half — 4-pixel words over the right columns.
                    set_colors4(&mut pattern.map2, &src[12..16]);

                    for i in 0..4 {
                        put_lut6_word(
                            cur,
                            dst + i * 2 * stride + 4,
                            &pattern.map1,
                            &pattern.map2,
                            32 + i * 8,
                        );
                        put_lut6_word(
                            cur,
                            dst + (i * 2 + 1) * stride + 4,
                            &pattern.map1,
                            &pattern.map2,
                            32 + i * 8 + 4,
                        );
                    }
                }

                cursor.pos += 24;
            } else {
                for i in 0..4 {
                    let value = NF_PK_LUT_6[usize::from(src[4 + i])];
                    pattern.map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                for i in 0..4 {
                    let value = NF_PK_LUT_6[usize::from(src[12 + i])];
                    pattern.map1[16 + i * 4..16 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                for i in 0..4 {
                    let value = NF_PK_LUT_6[usize::from(src[20 + i])];
                    pattern.map1[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                for i in 0..4 {
                    let value = NF_PK_LUT_6[usize::from(src[28 + i])];
                    pattern.map1[48 + i * 4..48 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }

                // Four 4x4 quadrants, one u32 per row.
                set_colors4(&mut pattern.map2, &src[0..4]);

                for i in 0..2 {
                    put_lut6_word(
                        cur,
                        dst + i * 2 * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 8,
                    );
                    put_lut6_word(
                        cur,
                        dst + (i * 2 + 1) * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 8 + 4,
                    );
                }

                set_colors4(&mut pattern.map2, &src[8..12]);

                for i in 0..2 {
                    put_lut6_word(
                        cur,
                        dst + (4 + i * 2) * stride,
                        &pattern.map1,
                        &pattern.map2,
                        16 + i * 8,
                    );
                    put_lut6_word(
                        cur,
                        dst + (4 + i * 2 + 1) * stride,
                        &pattern.map1,
                        &pattern.map2,
                        16 + i * 8 + 4,
                    );
                }

                set_colors4(&mut pattern.map2, &src[16..20]);

                for i in 0..2 {
                    put_lut6_word(
                        cur,
                        dst + i * 2 * stride + 4,
                        &pattern.map1,
                        &pattern.map2,
                        32 + i * 8,
                    );
                    put_lut6_word(
                        cur,
                        dst + (i * 2 + 1) * stride + 4,
                        &pattern.map1,
                        &pattern.map2,
                        32 + i * 8 + 4,
                    );
                }

                set_colors4(&mut pattern.map2, &src[24..28]);

                for i in 0..2 {
                    put_lut6_word(
                        cur,
                        dst + (4 + i * 2) * stride + 4,
                        &pattern.map1,
                        &pattern.map2,
                        48 + i * 8,
                    );
                    put_lut6_word(
                        cur,
                        dst + (4 + i * 2 + 1) * stride + 4,
                        &pattern.map1,
                        &pattern.map2,
                        48 + i * 8 + 4,
                    );
                }

                cursor.pos += 32;
            }
        }
        _ => unreachable!("decode_pattern_block: 0x7..=0xA"),
    }

    cursor.pos = cursor.pos.min(cursor.data.len());

    Ok(())
}

/// `nfFillPattern`: 2-color pattern (`run` iterations of 1/2 rows).
#[allow(clippy::too_many_arguments)]
fn fill_pattern(
    buf: &mut [u8],
    dst: usize,
    stride: usize,
    src: &[u8],
    map1: &mut [u8],
    map2: &mut [u32],
    run: usize,
    offset: usize,
    mode: usize,
) {
    for i in 0..run {
        let value = NF_PK_LUT_5[usize::from(src[2 + i])];

        map1[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }

    map2[0xC1] = (u32::from(src[1]) << 8) | u32::from(src[0]);
    map2[0xC2] = (u32::from(src[0]) << 8) | u32::from(src[1]);
    map2[0xC3] = (u32::from(src[0]) << 8) | u32::from(src[0]);
    map2[0xC5] = (u32::from(src[1]) << 8) | u32::from(src[1]);

    let mut row = dst;

    for i in 0..run {
        let index = offset + i * 4;

        match mode {
            2 => {
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index])] << 16) | map2[usize::from(map1[index + 1])],
                );
                row += stride;
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index + 2])] << 16) | map2[usize::from(map1[index + 3])],
                );
                row += stride;
            }
            1 => {
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index + 2])] << 16) | map2[usize::from(map1[index + 1])],
                );
                row += stride;
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index + 2])] << 16) | map2[usize::from(map1[index + 3])],
                );
                row += stride;
            }
            _ => {
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index])] << 16) | map2[usize::from(map1[index + 1])],
                );
                put_u32(
                    buf,
                    row + 4,
                    (map2[usize::from(map1[index + 2])] << 16) | map2[usize::from(map1[index + 3])],
                );
                row += stride;
            }
        }
    }
}

/// `nfFillPattern11`: 4-color pattern, fills `map1`.
fn fill_pattern11(src: &[u8], map1: &mut [u8], map2: &mut [u32], stride: usize, mode: usize) {
    for i in 0..stride {
        let mut value = NF_PK_LUT_6[usize::from(src[4 + i])];

        if mode != 0 {
            value = value.swap_bytes();
        }

        map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }

    map2[0xC1] = u32::from(src[2]);
    map2[0xC3] = u32::from(src[0]);
    map2[0xC5] = u32::from(src[3]);
    map2[0xC7] = u32::from(src[1]);
    map2[0xE1] = u32::from(src[2]);
    map2[0xE3] = u32::from(src[0]);
    map2[0xE5] = u32::from(src[3]);
    map2[0xE7] = u32::from(src[1]);
}

/// `nfFillPattern12`: a 4-color row/two rows over `map1`.
fn fill_pattern12(
    buf: &mut [u8],
    dst: usize,
    map1: &[u8],
    map2: &[u32],
    index: usize,
    mode: usize,
) {
    let at = |offset: usize| -> u32 { map2[usize::from(map1[index + offset])] };

    match mode {
        0 => {
            put_u32(
                buf,
                dst,
                (at(0) << 16) | (at(1) << 24) | at(2) | (at(3) << 8),
            );
            put_u32(
                buf,
                dst + 4,
                (at(4) << 16) | (at(5) << 24) | at(6) | (at(7) << 8),
            );
        }
        1 => {
            put_u32(
                buf,
                dst,
                (at(0) << 24) | (at(0) << 16) | (at(1) << 8) | at(1),
            );
            put_u32(
                buf,
                dst + 4,
                (at(2) << 24) | (at(2) << 16) | (at(3) << 8) | at(3),
            );
        }
        _ => unreachable!("fill_pattern12 mode"),
    }
}

/// One 32-bit word (4 pixels) from `map1/map2` (branch 0xA).
fn put_lut6_word(buf: &mut [u8], dst: usize, map1: &[u8], map2: &[u32], index: usize) {
    let at = |offset: usize| -> u32 { map2[usize::from(map1[index + offset])] };
    let word = (at(0) << 16) | (at(1) << 24) | at(2) | (at(3) << 8);

    put_u32(buf, dst, word);
}

fn put_u32(buf: &mut [u8], offset: usize, value: u32) {
    if offset + 4 <= buf.len() {
        buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

/// Exact row-by-row port of the `case 10` branch of `nfPkDecomp`.
///
/// Unlike the other patterns, here not only the 8×8 picture matters but also
/// the write order/shifts of `dst`: the branch first writes full rows and then
/// overlays the right half. We repeat the C code verbatim.
fn write_case_10(
    cur: &mut [u8],
    dst: &mut usize,
    src: &[u8],
    map1: &mut [u8; 512],
    map2: &mut [u32; 256],
    stride: usize,
) -> Result<(), MveError> {
    let back_right = stride * 7 - 8;
    let mut position = *dst as isize;

    let put = |cur: &mut [u8], at: isize, value: u32| {
        if at >= 0 {
            put_u32(cur, at as usize, value);
        }
    };
    let word = |map1: &[u8; 512], map2: &[u32; 256], index: usize| -> u32 {
        (map2[usize::from(map1[index])] << 16)
            | (map2[usize::from(map1[index + 1])] << 24)
            | map2[usize::from(map1[index + 2])]
            | (map2[usize::from(map1[index + 3])] << 8)
    };

    if src[0] > src[1] {
        for i in 0..8 {
            let value = NF_PK_LUT_6[usize::from(src[4 + i])];
            map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for i in 0..8 {
            let value = NF_PK_LUT_6[usize::from(src[16 + i])];
            map1[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }

        set_colors4(map2, &src[0..4]);

        if src[12] > src[13] {
            for i in 0..4 {
                put(cur, position, word(map1, map2, i * 8));
                put(cur, position + 4, word(map1, map2, i * 8 + 4));
                position += stride as isize;
            }

            set_colors4(map2, &src[12..16]);

            for i in 0..4 {
                put(cur, position, word(map1, map2, 32 + i * 8));
                put(cur, position + 4, word(map1, map2, 32 + i * 8 + 4));
                position += stride as isize;
            }

            position -= stride as isize;
            position -= back_right as isize;
        } else {
            for i in 0..4 {
                put(cur, position, word(map1, map2, i * 8));
                position += stride as isize;
                put(cur, position, word(map1, map2, i * 8 + 4));
                position += stride as isize;
            }

            position -= (stride * 8 - 4) as isize;

            set_colors4(map2, &src[12..16]);

            for i in 0..4 {
                put(cur, position, word(map1, map2, 32 + i * 8));
                position += stride as isize;
                put(cur, position, word(map1, map2, 32 + i * 8 + 4));
                position += stride as isize;
            }

            position -= stride as isize;
            position -= 4;
            position -= back_right as isize;
        }
    } else {
        for i in 0..4 {
            let value = NF_PK_LUT_6[usize::from(src[4 + i])];
            map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for i in 0..4 {
            let value = NF_PK_LUT_6[usize::from(src[12 + i])];
            map1[16 + i * 4..16 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for i in 0..4 {
            let value = NF_PK_LUT_6[usize::from(src[20 + i])];
            map1[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for i in 0..4 {
            let value = NF_PK_LUT_6[usize::from(src[28 + i])];
            map1[48 + i * 4..48 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }

        set_colors4(map2, &src[0..4]);

        for i in 0..2 {
            put(cur, position, word(map1, map2, i * 8));
            position += stride as isize;
            put(cur, position, word(map1, map2, i * 8 + 4));
            position += stride as isize;
        }

        set_colors4(map2, &src[8..12]);

        for i in 0..2 {
            put(cur, position, word(map1, map2, 16 + i * 8));
            position += stride as isize;
            put(cur, position, word(map1, map2, 16 + i * 8 + 4));
            position += stride as isize;
        }

        position -= (stride * 8 - 4) as isize;

        set_colors4(map2, &src[16..20]);

        for i in 0..2 {
            put(cur, position, word(map1, map2, 32 + i * 8));
            position += stride as isize;
            put(cur, position, word(map1, map2, 32 + i * 8 + 4));
            position += stride as isize;
        }

        set_colors4(map2, &src[24..28]);

        for i in 0..2 {
            put(cur, position, word(map1, map2, 48 + i * 8));
            position += stride as isize;
            put(cur, position, word(map1, map2, 48 + i * 8 + 4));
            position += stride as isize;
        }

        position -= stride as isize;
        position -= 4;
        position -= back_right as isize;
    }

    *dst = position.max(0) as usize;

    Ok(())
}

fn set_colors4(map2: &mut [u32; 256], src: &[u8]) {
    map2[0xC1] = u32::from(src[2]);
    map2[0xC3] = u32::from(src[0]);
    map2[0xC5] = u32::from(src[3]);
    map2[0xC7] = u32::from(src[1]);
    map2[0xE1] = u32::from(src[2]);
    map2[0xE3] = u32::from(src[0]);
    map2[0xE5] = u32::from(src[3]);
    map2[0xE7] = u32::from(src[1]);
}

/// Offset of a block in the frame buffer.
#[inline]
fn block_offset(header: &FrameHeader, block_x: usize, block_y: usize, stride: usize) -> usize {
    let x = usize::from(header.x) + block_x * BLOCK_SIZE;
    let y = usize::from(header.y) + block_y * BLOCK_SIZE;

    y * stride + x
}

/// Decoding of a motion vector byte (`nf_pk_lut_3`): (dx, dy).
///
/// The oracle table unfolds into the same formula as in FFmpeg:
/// `B < 56` — positive vector `(8 + B % 7, B / 7)`, otherwise —
/// `(-14 + (B - 56) % 29, 8 + (B - 56) / 29)`.
#[inline]
fn motion_vector(byte: u8) -> (isize, isize) {
    let value = isize::from(byte);

    if value < 56 {
        (8 + value % 7, value / 7)
    } else {
        (-14 + (value - 56) % 29, 8 + (value - 56) / 29)
    }
}

/// Copies 8×8 from `src` at the linear offset `src_offset` into `dst` at `dst_offset`.
fn copy_block(dst: &mut [u8], dst_offset: usize, src: &[u8], src_offset: isize, stride: usize) {
    let mut block = [0u8; 64];

    for row in 0..8 {
        for column in 0..8 {
            let source = src_offset + (row * stride + column) as isize;

            block[row * 8 + column] = if source >= 0 && (source as usize) < src.len() {
                src[source as usize]
            } else {
                0
            };
        }
    }

    write_block(dst, dst_offset, stride, &block);
}

/// Copies 8×8 inside one buffer (reading into a temporary block).
fn copy_within_block(dst: &mut [u8], dst_offset: usize, src_offset: isize, stride: usize) {
    let mut block = [0u8; 64];

    for row in 0..8 {
        for column in 0..8 {
            let source = src_offset + (row * stride + column) as isize;

            block[row * 8 + column] = if source >= 0 && (source as usize) < dst.len() {
                dst[source as usize]
            } else {
                0
            };
        }
    }

    write_block(dst, dst_offset, stride, &block);
}

fn write_block(dst: &mut [u8], dst_offset: usize, stride: usize, block: &[u8; 64]) {
    for row in 0..8 {
        let offset = dst_offset + row * stride;

        if offset + 8 <= dst.len() {
            dst[offset..offset + 8].copy_from_slice(&block[row * 8..row * 8 + 8]);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "mve_tests.rs"]
mod tests;
