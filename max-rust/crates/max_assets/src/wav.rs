//! RIFF/WAVE-звуки M.A.X. (`*.SPW` в каталоге игры).
//!
//! Все 272 файла `*.SPW` из `max-original/` — обычные WAV-файлы: PCM,
//! 22050 Гц, моно, 16 бит (`max-original-dasm/formats/spw.md` §1, §8.5).
//! Расширение `.SPW` — след переименования WAV на поздних тиражах; никакой
//! собственной структуры у формата нет, поэтому парсер — обычный RIFF-обход
//! чанков.
//!
//! Поддерживается то, что реально встречается в игре (и минимально полезно):
//!
//! * контейнер `RIFF`/`WAVE`;
//! * чанк `fmt ` с `wFormatTag == 1` (PCM), 1–2 канала, 16 бит;
//! * чанк `data`; остальные чанки (`smpl`, `fact`, `LIST`, …) пропускаются;
//! * нечётный размер чанка — с паддинг-байтом (стандартное правило RIFF).
//!
//! Сэмплы возвращаются как `Vec<i16>` в **interleaved**-порядке: для стерео
//! `[L0, R0, L1, R1, …]`. Число кадров — [`WavClip::frames`].
//!
//! Ошибки формата возвращаются значениями; паник на битых данных нет. Размер
//! `data`-чанка, не кратный `block_align`, не ошибка: хвост обрезается (как
//! поступил бы любой проигрыватель).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Минимальная длина RIFF-заголовка (`RIFF` + size + `WAVE`).
pub const RIFF_HEADER_SIZE: usize = 12;
/// Минимальная длина чанка `fmt ` для PCM (поля до `wBitsPerSample`).
pub const PCM_FMT_MIN_SIZE: usize = 16;
/// Размер одного сэмпла PCM16.
pub const SAMPLE_BYTES: usize = 2;
/// `wFormatTag` для несжатого PCM.
pub const WAVE_FORMAT_PCM: u16 = 1;

/// Разобранный WAV-файл (PCM16).
#[derive(Clone, PartialEq, Eq)]
pub struct WavClip {
    /// Частота дискретизации, Гц (у звуков M.A.X. — 22050).
    pub sample_rate: u32,
    /// Число каналов (1 — моно, 2 — стерео).
    pub channels: u16,
    /// Сэмплы PCM16 в interleaved-порядке.
    pub samples: Vec<i16>,
}

impl WavClip {
    /// Разбирает WAV из байтов.
    ///
    /// # Ошибки
    ///
    /// [`WavError`] — если это не RIFF/WAVE, нет `fmt `/`data`, формат не
    /// PCM16 или файл обрезан. Неизвестные чанки игнорируются.
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

        // RIFF-чанки: id[4] + u32 size + тело + паддинг до чётного размера.
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

            // Нечётный размер дополняется одним байтом; на конце файла
            // паддинга может не быть — тогда цикл просто завершится.
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

        // Хвост, не составивший целый кадр, отбрасываем (битые/склеенные
        // файлы), чтобы interleaved-поток всегда был согласован с каналами.
        let channels = usize::from(fmt.channels);
        let frames = samples.len() / channels;
        samples.truncate(frames * channels);

        Ok(Self {
            sample_rate: fmt.sample_rate,
            channels: fmt.channels,
            samples,
        })
    }

    /// Читает и разбирает WAV-файл.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WavError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| WavError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::parse(&bytes)
    }

    /// Число кадров (сэмплов на канал).
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / usize::from(self.channels)
        }
    }

    /// Длительность клипа.
    pub fn duration(&self) -> Duration {
        if self.sample_rate == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(self.frames() as f64 / f64::from(self.sample_rate))
        }
    }

    /// Клип без сэмплов.
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

/// Поля чанка `fmt `, нужные для PCM16.
#[derive(Debug, Clone, Copy)]
struct FmtChunk {
    channels: u16,
    sample_rate: u32,
    block_align: u16,
}

/// Разбирает `fmt ` и проверяет, что это PCM16 моно/стерео.
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

/// Ошибка разбора/чтения WAV.
#[derive(Debug)]
pub enum WavError {
    /// Не удалось прочитать файл.
    Io {
        /// Путь, который читался.
        path: PathBuf,
        /// Причина от ОС.
        source: io::Error,
    },
    /// Файл короче RIFF-заголовка.
    TooShort {
        /// Фактическая длина.
        len: usize,
        /// Минимально необходимая длина.
        needed: usize,
    },
    /// Первые 4 байта — не `"RIFF"`.
    BadRiff {
        /// Фактическая сигнатура.
        found: [u8; 4],
    },
    /// Байты 8..12 — не `"WAVE"`.
    BadWave {
        /// Фактическая сигнатура.
        found: [u8; 4],
    },
    /// Нет чанка `fmt `.
    MissingFmt,
    /// Нет чанка `data`.
    MissingData,
    /// Размер `fmt ` меньше 16 байт.
    ShortFmt {
        /// Фактическая длина тела чанка.
        len: usize,
        /// Минимально необходимая длина.
        needed: usize,
    },
    /// Чанк выходит за конец файла.
    TruncatedChunk {
        /// Идентификатор чанка.
        id: [u8; 4],
        /// Смещение заголовка чанка.
        offset: usize,
        /// Размер тела из заголовка.
        size: u32,
        /// Фактическая длина файла.
        len: usize,
    },
    /// `body_start + size` переполняет `usize` (битый размер).
    ChunkOverflow {
        /// Идентификатор чанка.
        id: [u8; 4],
        /// Смещение заголовка чанка.
        offset: usize,
        /// Размер тела из заголовка.
        size: u32,
    },
    /// `wFormatTag` — не PCM.
    UnsupportedFormat {
        /// Код формата из `fmt `.
        format: u16,
    },
    /// Число каналов не 1 и не 2.
    UnsupportedChannels {
        /// Число каналов из `fmt `.
        channels: u16,
    },
    /// Биты на сэмпл — не 16.
    UnsupportedBits {
        /// `wBitsPerSample` из `fmt `.
        bits: u16,
    },
    /// Частота дискретизации равна нулю.
    BadSampleRate {
        /// Значение из `fmt `.
        sample_rate: u32,
    },
    /// `block_align` не совпадает с `channels * 2`.
    BadBlockAlign {
        /// Значение из `fmt `.
        block_align: u16,
        /// Ожидаемое значение.
        expected: u16,
        /// Смещение чанка `fmt ` (для диагностики).
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

    /// Собирает WAV PCM16 из сэмплов (mono/stereo).
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
        // Вставляем "junk" (3 байта + паддинг) после WAVE и "smpl" в конец:
        // обход обязан учитывать нечётный размер и не придираться к чужим чанкам.
        let mut junk = Vec::new();
        junk.extend_from_slice(b"junk");
        junk.extend_from_slice(&3u32.to_le_bytes());
        junk.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0x00]);

        wav.splice(12..12, junk);
        // RIFF size не пересчитываем: парсер следует чанкам, а не заголовку
        // (в реальных файлах размер бывает неточным).
        wav.extend_from_slice(b"smpl");
        wav.extend_from_slice(&60u32.to_le_bytes());
        wav.extend_from_slice(&[0u8; 60]);

        let clip = WavClip::parse(&wav).expect("чанки пропущены");
        assert_eq!(clip.samples, vec![7, 8]);
        assert_eq!(clip.sample_rate, 22050);
    }

    #[test]
    fn truncates_data_to_whole_frames() {
        // Стерео с «половинкой» кадра: 3 сэмпла вместо 4.
        let mut wav = build_wav(2, 22050, &[1, 2, 3, 4]);
        // data начинается с 44: 6 байт = 3 сэмпла, последний кадр неполный.
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
        // Нет data.
        let mut no_data = wav.clone();
        no_data[36..40].copy_from_slice(b"junk");
        assert!(matches!(
            WavClip::parse(&no_data),
            Err(WavError::MissingData)
        ));

        // Нет fmt.
        let mut no_fmt = wav.clone();
        no_fmt[12..16].copy_from_slice(b"junk");
        assert!(matches!(WavClip::parse(&no_fmt), Err(WavError::MissingFmt)));

        // Обрезанный data-чанк.
        let mut truncated = wav.clone();
        truncated.truncate(truncated.len() - 1);
        assert!(matches!(
            WavClip::parse(&truncated),
            Err(WavError::TruncatedChunk { .. })
        ));

        // 8 бит.
        let mut eight_bit = wav.clone();
        eight_bit[34..36].copy_from_slice(&8u16.to_le_bytes());
        assert!(matches!(
            WavClip::parse(&eight_bit),
            Err(WavError::UnsupportedBits { bits: 8 })
        ));

        // 3 канала и неверный block_align.
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

        // Не-PCM (IEEE float).
        let mut float = wav.clone();
        float[20..22].copy_from_slice(&3u16.to_le_bytes());
        assert!(matches!(
            WavClip::parse(&float),
            Err(WavError::UnsupportedFormat { format: 3 })
        ));

        // Нулевая частота.
        let mut zero_rate = wav;
        zero_rate[24..28].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            WavClip::parse(&zero_rate),
            Err(WavError::BadSampleRate { sample_rate: 0 })
        ));
    }

    /// Реальный звук игры: фикстура `max-original/KBUY0.SPW` (2 604 байта).
    ///
    /// Путь включается на этапе компиляции: файл лежит в репозитории, поэтому
    /// тест не зависит от текущего каталога и не «тихо пропускается».
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

    /// Все 272 `*.SPW` каталога игры — PCM16 22050 Гц моно (`spw.md` §8.5).
    ///
    /// Тест читает каталог, а не отдельный файл: так ловится любое расхождение
    /// реальных данных с допущениями парсера. Если каталога нет (сборка без
    /// игровых данных), проверка пропускается.
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
