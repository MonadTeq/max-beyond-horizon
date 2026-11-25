//! RIFF/WAVE sounds of M.A.X. (`*.SPW` in the game directory).
//!
//! All 272 `*.SPW` files from `max-original/` are ordinary WAV files: PCM,
//! 22050 Hz, mono, 16 bits (`max-original-dasm/formats/spw.md` §1, §8.5).
//! The `.SPW` extension is a trace of the WAV renaming in later print runs; the format has no
//! structure of its own, so the parser is an ordinary RIFF chunk
//! walk.
//!
//! Supported is what actually occurs in the game (and is minimally useful):
//!
//! * the `RIFF`/`WAVE` container;
//! * the `fmt ` chunk with `wFormatTag == 1` (PCM), 1–2 channels, 16 bits;
//! * the `data` chunk; the other chunks (`smpl`, `fact`, `LIST`, …) are skipped;
//! * an odd chunk size — with a padding byte (the standard RIFF rule).
//!
//! Samples are returned as `Vec<i16>` in **interleaved** order: for stereo
//! `[L0, R0, L1, R1, …]`. The number of frames is [`WavClip::frames`].
//!
//! Format errors are returned as values; there are no panics on corrupt data. A
//! `data` chunk size that is not a multiple of `block_align` is not an error: the tail is trimmed
//! (as any player would do).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Minimum RIFF header length (`RIFF` + size + `WAVE`).
pub const RIFF_HEADER_SIZE: usize = 12;
/// Minimum `fmt ` chunk length for PCM (fields up to `wBitsPerSample`).
pub const PCM_FMT_MIN_SIZE: usize = 16;
/// Size of one PCM16 sample.
pub const SAMPLE_BYTES: usize = 2;
/// `wFormatTag` for uncompressed PCM.
pub const WAVE_FORMAT_PCM: u16 = 1;

/// Parsed WAV file (PCM16).
#[derive(Clone, PartialEq, Eq)]
pub struct WavClip {
    /// Sample rate, Hz (for M.A.X. sounds — 22050).
    pub sample_rate: u32,
    /// Number of channels (1 — mono, 2 — stereo).
    pub channels: u16,
    /// PCM16 samples in interleaved order.
    pub samples: Vec<i16>,
}

impl WavClip {
    /// Parses a WAV from bytes.
    ///
    /// # Errors
    ///
    /// [`WavError`] — if this is not RIFF/WAVE, `fmt `/`data` is missing, the format is not
    /// PCM16, or the file is truncated. Unknown chunks are ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self, WavError> {
        if bytes.len() < RIFF_HEADER_SIZE {
            return Err(WavError::TooShort {
                len: bytes.len(),
                needed: RIFF_HEADER_SIZE,
            });
        }

        if &bytes[0..4] != b"RIFF" {
            let mut found = [0u8; 4];
            found.copy_from_slice(&bytes[0..4]);

            return Err(WavError::BadRiff { found });
        }

        if &bytes[8..12] != b"WAVE" {
            let mut found = [0u8; 4];
            found.copy_from_slice(&bytes[8..12]);

            return Err(WavError::BadWave { found });
        }

        let mut fmt: Option<FmtChunk> = None;
        let mut data: Option<(usize, usize)> = None;
        let mut pos = RIFF_HEADER_SIZE;

        // RIFF chunks: id[4] + u32 size + body + padding to an even size.
        while pos + 8 <= bytes.len() {
            let mut id = [0u8; 4];
            id.copy_from_slice(&bytes[pos..pos + 4]);
            let size = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]);
            let body_start = pos + 8;
            let body_end =
                body_start
                    .checked_add(size as usize)
                    .ok_or(WavError::ChunkOverflow {
                        id,
                        offset: pos,
                        size,
                    })?;

            if body_end > bytes.len() {
                return Err(WavError::TruncatedChunk {
                    id,
                    offset: pos,
                    size,
                    len: bytes.len(),
                });
            }

            match &id {
                b"fmt " if fmt.is_none() => {
                    fmt = Some(parse_fmt(&bytes[body_start..body_end], pos)?);
                }
                b"data" if data.is_none() => {
                    data = Some((body_start, body_end));
                }
                _ => {}
            }

            // An odd size is padded with one byte; at the end of the file
            // the padding may be absent — then the loop simply ends.
            pos = body_end + (size as usize & 1);
        }

        let fmt = fmt.ok_or(WavError::MissingFmt)?;
        let (data_start, data_end) = data.ok_or(WavError::MissingData)?;
        let block_align = usize::from(fmt.block_align);
        let usable = (data_end - data_start) / block_align * block_align;
        let mut samples = Vec::with_capacity(usable / SAMPLE_BYTES);

        for pair in bytes[data_start..data_start + usable].chunks_exact(SAMPLE_BYTES) {
            samples.push(i16::from_le_bytes([pair[0], pair[1]]));
        }

        // A tail that does not form a whole frame is discarded (corrupt/glued
        // files), so that the interleaved stream is always consistent with the channels.
        let channels = usize::from(fmt.channels);
        let frames = samples.len() / channels;
        samples.truncate(frames * channels);

        Ok(Self {
            sample_rate: fmt.sample_rate,
            channels: fmt.channels,
            samples,
        })
    }

    /// Reads and parses a WAV file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WavError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| WavError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::parse(&bytes)
    }

    /// Number of frames (samples per channel).
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / usize::from(self.channels)
        }
    }

    /// Duration of the clip.
    pub fn duration(&self) -> Duration {
        if self.sample_rate == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(self.frames() as f64 / f64::from(self.sample_rate))
        }
    }

    /// Clip without samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

impl fmt::Debug for WavClip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WavClip")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("frames", &self.frames())
            .field("samples", &self.samples.len())
            .finish()
    }
}

/// `fmt ` chunk fields needed for PCM16.
#[derive(Debug, Clone, Copy)]
struct FmtChunk {
    channels: u16,
    sample_rate: u32,
    block_align: u16,
}

/// Parses `fmt ` and checks that it is PCM16 mono/stereo.
fn parse_fmt(body: &[u8], chunk_offset: usize) -> Result<FmtChunk, WavError> {
    if body.len() < PCM_FMT_MIN_SIZE {
        return Err(WavError::ShortFmt {
            len: body.len(),
            needed: PCM_FMT_MIN_SIZE,
        });
    }

    let format = u16::from_le_bytes([body[0], body[1]]);
    let channels = u16::from_le_bytes([body[2], body[3]]);
    let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
    let block_align = u16::from_le_bytes([body[12], body[13]]);
    let bits = u16::from_le_bytes([body[14], body[15]]);

    if format != WAVE_FORMAT_PCM {
        return Err(WavError::UnsupportedFormat { format });
    }
    if !(1..=2).contains(&channels) {
        return Err(WavError::UnsupportedChannels { channels });
    }
    if sample_rate == 0 {
        return Err(WavError::BadSampleRate { sample_rate });
    }
    if bits != 16 {
        return Err(WavError::UnsupportedBits { bits });
    }

    let expected_align = channels * (bits / 8);
    if block_align != expected_align {
        return Err(WavError::BadBlockAlign {
            block_align,
            expected: expected_align,
            chunk_offset,
        });
    }

    Ok(FmtChunk {
        channels,
        sample_rate,
        block_align,
    })
}

/// WAV parse/read error.
#[derive(Debug)]
pub enum WavError {
    /// Failed to read the file.
    Io {
        /// The path that was being read.
        path: PathBuf,
        /// Reason from the OS.
        source: io::Error,
    },
    /// The file is shorter than the RIFF header.
    TooShort {
        /// Actual length.
        len: usize,
        /// Minimum required length.
        needed: usize,
    },
    /// The first 4 bytes are not `"RIFF"`.
    BadRiff {
        /// Actual signature.
        found: [u8; 4],
    },
    /// Bytes 8..12 are not `"WAVE"`.
    BadWave {
        /// Actual signature.
        found: [u8; 4],
    },
    /// No `fmt ` chunk.
    MissingFmt,
    /// No `data` chunk.
    MissingData,
    /// The `fmt ` size is less than 16 bytes.
    ShortFmt {
        /// Actual chunk body length.
        len: usize,
        /// Minimum required length.
        needed: usize,
    },
    /// The chunk extends beyond the end of the file.
    TruncatedChunk {
        /// Chunk identifier.
        id: [u8; 4],
        /// Offset of the chunk header.
        offset: usize,
        /// Body size from the header.
        size: u32,
        /// Actual file length.
        len: usize,
    },
    /// `body_start + size` overflows `usize` (corrupt size).
    ChunkOverflow {
        /// Chunk identifier.
        id: [u8; 4],
        /// Offset of the chunk header.
        offset: usize,
        /// Body size from the header.
        size: u32,
    },
    /// `wFormatTag` is not PCM.
    UnsupportedFormat {
        /// Format code from `fmt `.
        format: u16,
    },
    /// The number of channels is neither 1 nor 2.
    UnsupportedChannels {
        /// Number of channels from `fmt `.
        channels: u16,
    },
    /// Bits per sample are not 16.
    UnsupportedBits {
        /// `wBitsPerSample` from `fmt `.
        bits: u16,
    },
    /// The sample rate is zero.
    BadSampleRate {
        /// Value from `fmt `.
        sample_rate: u32,
    },
    /// `block_align` does not match `channels * 2`.
    BadBlockAlign {
        /// Value from `fmt `.
        block_align: u16,
        /// Expected value.
        expected: u16,
        /// Offset of the `fmt ` chunk (for diagnostics).
        chunk_offset: usize,
    },
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "не удалось прочитать {}: {source}", path.display())
            }
            Self::TooShort { len, needed } => write!(
                f,
                "файл слишком короткий для RIFF: {len} байт (нужно минимум {needed})"
            ),
            Self::BadRiff { found } => {
                write!(f, "некорректная сигнатура RIFF: {found:02X?}")
            }
            Self::BadWave { found } => {
                write!(f, "некорректная сигнатура WAVE: {found:02X?}")
            }
            Self::MissingFmt => f.write_str("в WAV нет чанка fmt "),
            Self::MissingData => f.write_str("в WAV нет чанка data"),
            Self::ShortFmt { len, needed } => write!(
                f,
                "чанк fmt слишком короткий: {len} байт (нужно минимум {needed})"
            ),
            Self::TruncatedChunk {
                id,
                offset,
                size,
                len,
            } => write!(
                f,
                "чанк {:?} по смещению 0x{offset:X} выходит за конец файла: size={size}, файл {len} байт",
                String::from_utf8_lossy(id)
            ),
            Self::ChunkOverflow { id, offset, size } => write!(
                f,
                "размер чанка {:?} по смещению 0x{offset:X} переполняет адресацию: {size}",
                String::from_utf8_lossy(id)
            ),
            Self::UnsupportedFormat { format } => write!(
                f,
                "поддерживается только PCM (format=1), в файле format={format}"
            ),
            Self::UnsupportedChannels { channels } => {
                write!(f, "поддерживается 1–2 канала, в файле {channels}")
            }
            Self::UnsupportedBits { bits } => {
                write!(f, "поддерживается только 16 бит, в файле {bits}")
            }
            Self::BadSampleRate { sample_rate } => {
                write!(f, "некорректная частота дискретизации: {sample_rate}")
            }
            Self::BadBlockAlign {
                block_align,
                expected,
                chunk_offset,
            } => write!(
                f,
                "block_align={block_align} не совпадает с channels*2={expected} (fmt @0x{chunk_offset:X})"
            ),
        }
    }
}

impl std::error::Error for WavError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "wav_tests.rs"]
mod tests;
