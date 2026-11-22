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
mod tests {
    use super::*;

    /// Builds a PCM16 WAV from samples (mono/stereo).
    fn build_wav(channels: u16, sample_rate: u32, samples: &[i16]) -> Vec<u8> {
        let data_len = samples.len() * SAMPLE_BYTES;
        let block_align = channels * 2;
        let mut out = Vec::with_capacity(44 + data_len);

        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());

        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }

        out
    }

    #[test]
    fn parses_pcm16_mono_and_stereo() {
        let mono = WavClip::parse(&build_wav(1, 22050, &[1, -2, 3])).expect("mono");
        assert_eq!(mono.sample_rate, 22050);
        assert_eq!(mono.channels, 1);
        assert_eq!(mono.samples, vec![1, -2, 3]);
        assert_eq!(mono.frames(), 3);

        let stereo = WavClip::parse(&build_wav(2, 44100, &[10, 20, 30, 40])).expect("stereo");
        assert_eq!(stereo.channels, 2);
        assert_eq!(stereo.samples, vec![10, 20, 30, 40]);
        assert_eq!(stereo.frames(), 2);
    }

    #[test]
    fn skips_unknown_odd_and_trailing_chunks() {
        let mut wav = build_wav(1, 22050, &[7, 8]);
        // Insert "junk" (3 bytes + padding) after WAVE and "smpl" at the end:
        // the walk must account for the odd size and not be picky about foreign chunks.
        let mut junk = Vec::new();
        junk.extend_from_slice(b"junk");
        junk.extend_from_slice(&3u32.to_le_bytes());
        junk.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0x00]);

        wav.splice(12..12, junk);
        // We do not recompute the RIFF size: the parser follows the chunks, not the header
        // (in real files the size is sometimes inaccurate).
        wav.extend_from_slice(b"smpl");
        wav.extend_from_slice(&60u32.to_le_bytes());
        wav.extend_from_slice(&[0u8; 60]);

        let clip = WavClip::parse(&wav).expect("чанки пропущены");
        assert_eq!(clip.samples, vec![7, 8]);
        assert_eq!(clip.sample_rate, 22050);
    }

    #[test]
    fn truncates_data_to_whole_frames() {
        // Stereo with a "half" frame: 3 samples instead of 4.
        let mut wav = build_wav(2, 22050, &[1, 2, 3, 4]);
        // data starts at 44: 6 bytes = 3 samples, the last frame is incomplete.
        wav.truncate(wav.len() - 2);
        wav[40..44].copy_from_slice(&6u32.to_le_bytes());

        let clip = WavClip::parse(&wav).expect("хвост обрезан");
        assert_eq!(clip.channels, 2);
        assert_eq!(clip.samples, vec![1, 2]);
        assert_eq!(clip.frames(), 1);
    }

    #[test]
    fn rejects_malformed_files() {
        assert!(matches!(
            WavClip::parse(b""),
            Err(WavError::TooShort { .. })
        ));
        assert!(matches!(
            WavClip::parse(b"NOPE\0\0\0\0WAVE"),
            Err(WavError::BadRiff { .. })
        ));
        assert!(matches!(
            WavClip::parse(b"RIFF\0\0\0\0NOPE"),
            Err(WavError::BadWave { .. })
        ));

        let wav = build_wav(1, 22050, &[1]);
        // No data.
        let mut no_data = wav.clone();
        no_data[36..40].copy_from_slice(b"junk");
        assert!(matches!(
            WavClip::parse(&no_data),
            Err(WavError::MissingData)
        ));

        // No fmt.
        let mut no_fmt = wav.clone();
        no_fmt[12..16].copy_from_slice(b"junk");
        assert!(matches!(WavClip::parse(&no_fmt), Err(WavError::MissingFmt)));

        // Truncated data chunk.
        let mut truncated = wav.clone();
        truncated.truncate(truncated.len() - 1);
        assert!(matches!(
            WavClip::parse(&truncated),
            Err(WavError::TruncatedChunk { .. })
        ));

        // 8 bits.
        let mut eight_bit = wav.clone();
        eight_bit[34..36].copy_from_slice(&8u16.to_le_bytes());
        assert!(matches!(
            WavClip::parse(&eight_bit),
            Err(WavError::UnsupportedBits { bits: 8 })
        ));

        // 3 channels and a wrong block_align.
        let mut three_channels = wav.clone();
        three_channels[22..24].copy_from_slice(&3u16.to_le_bytes());
        assert!(matches!(
            WavClip::parse(&three_channels),
            Err(WavError::UnsupportedChannels { channels: 3 })
        ));

        let mut bad_align = wav.clone();
        bad_align[32..34].copy_from_slice(&6u16.to_le_bytes());
        assert!(matches!(
            WavClip::parse(&bad_align),
            Err(WavError::BadBlockAlign { .. })
        ));

        // Non-PCM (IEEE float).
        let mut float = wav.clone();
        float[20..22].copy_from_slice(&3u16.to_le_bytes());
        assert!(matches!(
            WavClip::parse(&float),
            Err(WavError::UnsupportedFormat { format: 3 })
        ));

        // Zero sample rate.
        let mut zero_rate = wav;
        zero_rate[24..28].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            WavClip::parse(&zero_rate),
            Err(WavError::BadSampleRate { sample_rate: 0 })
        ));
    }

    /// A real game sound: the `max-original/KBUY0.SPW` fixture (2 604 bytes).
    ///
    /// The path is baked in at compile time: the file lies in the repository, so the
    /// test does not depend on the current directory and is not silently skipped.
    #[test]
    fn parses_real_kbuy0_spw() {
        const KBUY0: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../max-original/KBUY0.SPW"
        ));

        let clip = WavClip::parse(KBUY0).expect("KBUY0.SPW — валидный PCM16 WAV");

        assert_eq!(clip.sample_rate, 22050);
        assert_eq!(clip.channels, 1);
        assert_eq!(clip.samples.len(), 1280);
        assert_eq!(clip.frames(), 1280);
        assert!(!clip.is_empty());
        assert!((clip.duration().as_secs_f64() - 1280.0 / 22050.0).abs() < 1e-9);
    }

    /// All 272 `*.SPW` of the game directory are PCM16 22050 Hz mono (`spw.md` §8.5).
    ///
    /// The test reads the directory rather than a single file: this catches any discrepancy
    /// of the real data with the parser's assumptions. If the directory is absent (a build without
    /// game data), the check is skipped.
    #[test]
    fn all_real_spw_files_are_pcm16_22050_mono() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original");
        let Ok(entries) = fs::read_dir(&dir) else {
            return;
        };
        let mut checked = 0usize;

        for entry in entries.flatten() {
            let path = entry.path();
            let is_spw = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("spw"));
            if !is_spw {
                continue;
            }

            let clip =
                WavClip::open(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(clip.sample_rate, 22050, "{}", path.display());
            assert_eq!(clip.channels, 1, "{}", path.display());
            assert!(!clip.is_empty(), "{}", path.display());
            checked += 1;
        }

        assert!(checked >= 200, "ожидались сотни .SPW, найдено {checked}");
    }
}
