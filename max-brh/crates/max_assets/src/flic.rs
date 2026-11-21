//! FLC/FLI — анимации Autodesk FLIC (`*.FLC` рядом с `MAX.RES`).
//!
//! Оракул — `max-cpp-port/src/flicsmgr.cpp` (декодер `flicsmgr_*`) и
//! `game_manager.cpp::GameManager_MenuCreateFlic`/`AdvanceFlic` (портрет
//! `WINDOW_CORNER_FLIC`). Формат описан в публичной спецификации Autodesk
//! FLIC (`flic.htm`): 128-байтный заголовок, далее кадры по 16-байтному
//! заголовку, внутри кадра — чанки.
//!
//! ```text
//! +0x00 u32 size            — размер файла (в эталонных файлах совпадает)
//! +0x04 u16 type            — 0xAF12 (FLC) / 0xAF11 (FLI; не поддерживается)
//! +0x06 u16 frames          — число кадров
//! +0x08 u16 width, +0x0A height, +0x0C depth, +0x0E flags
//! +0x10 u32 speed           — мс на кадр (в MAX.RES: 71, у 128×256 — 67)
//! +0x50 u32 oframe1         — смещение первого кадра
//! +0x54 u32 oframe2         — смещение второго кадра (ring frame)
//! ```
//!
//! Кадр: `u32 size` (включая заголовок), `u16 type = 0xF1FA`, `u16 chunks`,
//! 8 байт резерва. Чанк: `u32 size` (включая 6-байтный заголовок, размер
//! чётный), `u16 type`, далее тело.
//!
//! | Чанк | ID | Что делает |
//! |---|---|---|
//! | `COLOR_256` | 4 | палитра: `u16 packets`, пакеты `u8 skip, u8 count (0→256), count×RGB` |
//! | `DELTA_FLC` | 7 | дельта по словам (`FLI_SS2`): строки, опциональные слова, пакеты `u8 skip, i8 words` |
//! | `COLOR_64` | 11 | палитра в 6-битных компонентах |
//! | `DELTA_FLI` | 12 | дельта по байтам (`FLI_LC`): `u16 first_line, u16 lines`, пакеты `u8 skip, i8 count` |
//! | `BLACK` | 13 | кадр в чёрный |
//! | `BYTE_RUN` | 15 | полный кадр RLE: на строку `u8 packets`, затем `i8 count` + байт/литералы |
//! | `LITERAL` | 16 | полный кадр без сжатия (`width*height` байт) |
//! | `PSTAMP` | 18 | «почтовая марка» (превью) — в C++-порте игнорируется |
//!
//! # Отличия от C++-порта (и почему они безопасны)
//!
//! * **Границы проверяются**: C++ читает `memcpy`-ями без контроля длины, наш
//!   декодер возвращает [`FlicError`] вместо паники/порчи кучи. На всех 71
//!   ресурсе MAX.RES оба декодера дают одинаковые пиксели (`flic_vectors`).
//! * **`DELTA_FLI` (12) реализован**: в C++ его нет, в 71 ресурсе MAX.RES он не
//!   встречается — нужен только для полноты формата.
//! * **Палитровые чанки копятся в 6-битном виде** — как системная палитра
//!   C++-порта: байты `COLOR_256` сдвигаются `>> 2`, `COLOR_64` уже 6-битные
//!   (для 8-битного RGB умножьте на 4). К портрету палитра не применяется:
//!   `load_flic_palette = 0` в `GameManager_MenuCreateFlic`, индексы кадра идут
//!   через палитру окна (`FRAMEPIC`). Палитра нужна для полноты формата и
//!   будущих экранов с `load_flic_palette = 1`.
//! * **`PSTAMP` пропускается** (как `default:` в C++), но его размер
//!   валидируется.
//!
//! Кадры — дельта-кодированные: кадр `N` требует применения кадров `0..=N`.
//! [`Flic::decode_frame`] делает это с нуля (детерминированно, без состояния),
//! [`FlicPlayer`] — инкрементально, как `flicsmgr_advance_animation`.
//!
//! Ошибки формата возвращаются значениями, паник на битых данных нет:
//! `#![forbid(unsafe_code)]` в крейте и проверки каждого чтения.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Размер заголовка FLIC (`sizeof(FlicHeader)` в C++-порте).
pub const FLIC_HEADER_SIZE: usize = 128;
/// Размер заголовка кадра (`sizeof(FrameHeader)`).
pub const FLIC_FRAME_HEADER_SIZE: usize = 16;
/// Размер заголовка чанка (`sizeof(FlicChunkHeader)`).
pub const FLIC_CHUNK_HEADER_SIZE: usize = 6;
/// Сигнатура FLC (`FLICSMGR_FLC_TYPE`).
pub const FLC_MAGIC: u16 = 0xAF12;
/// Сигнатура FLI (`FLICSMGR_FLI_TYPE`) — формат не поддерживается.
pub const FLI_MAGIC: u16 = 0xAF11;
/// Тип кадра (`FLICSMGR_FRAME_TYPE`).
pub const FRAME_MAGIC: u16 = 0xF1FA;
/// Цветов в палитре.
pub const FLIC_PALETTE_SIZE: usize = 256;

/// Палитра кадра: 256 × RGB в 6-битном виде (как системная палитра C++-порта;
/// для 8-битного RGB умножьте компоненты на 4).
pub type FlicPalette = [[u8; 3]; FLIC_PALETTE_SIZE];

// ---------------------------------------------------------------------------
// Ошибки
// ---------------------------------------------------------------------------

/// Ошибка разбора/декодирования FLC.
#[derive(Debug)]
pub enum FlicError {
    /// Файл не читается.
    Io {
        /// Путь, который читался.
        path: PathBuf,
        /// Причина от ОС.
        source: io::Error,
    },
    /// Данных меньше, чем нужно для заголовка.
    TooShort {
        /// Фактическая длина.
        len: usize,
        /// Минимально необходимая длина.
        needed: usize,
    },
    /// Сигнатура не `0xAF12`.
    BadMagic {
        /// Найденная сигнатура.
        found: u16,
    },
    /// В заголовке ноль кадров — анимировать нечего.
    NoFrames,
    /// Нулевые или неразумно большие размеры кадра.
    BadGeometry {
        /// Ширина из заголовка.
        width: u16,
        /// Высота из заголовка.
        height: u16,
    },
    /// Кадр с таким номером не существует.
    FrameIndex {
        /// Запрошенный номер.
        index: usize,
        /// Сколько кадров в файле.
        frames: usize,
    },
    /// Структурная ошибка в кадре.
    Frame {
        /// Номер кадра.
        index: usize,
        /// Смещение кадра в файле.
        offset: usize,
        /// Что именно не так.
        reason: FrameError,
    },
    /// Структурная ошибка при чтении/декодировании.
    Format {
        /// Смещение в файле, где обнаружена ошибка.
        offset: usize,
        /// Что ожидалось.
        what: &'static str,
    },
}

/// Чем плох кадр.
#[derive(Debug)]
pub enum FrameError {
    /// Кадр выходит за конец файла.
    Truncated {
        /// Нужно байт.
        needed: usize,
        /// Доступно байт.
        available: usize,
    },
    /// Тип кадра не `0xF1FA`.
    BadType {
        /// Найденный тип.
        found: u16,
    },
    /// Размер кадра меньше заголовка.
    BadSize {
        /// Найденный размер.
        size: u32,
    },
    /// Чанк выходит за границы кадра или имеет нулевой размер.
    BadChunk {
        /// Номер чанка в кадре.
        index: usize,
        /// Смещение чанка.
        offset: usize,
        /// Что не так.
        reason: &'static str,
    },
    /// Неизвестный/неподдерживаемый тип чанка.
    UnknownChunk {
        /// Номер чанка в кадре.
        index: usize,
        /// Тип чанка.
        kind: u16,
    },
}

impl fmt::Display for FlicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "не удалось прочитать {}: {source}", path.display())
            }
            Self::TooShort { len, needed } => write!(
                f,
                "FLC слишком короткий: {len} байт (нужно минимум {needed})"
            ),
            Self::BadMagic { found } => write!(
                f,
                "некорректная сигнатура FLC: ожидалось 0xAF12, получено 0x{found:04X}"
            ),
            Self::NoFrames => write!(f, "в заголовке FLC ноль кадров"),
            Self::BadGeometry { width, height } => {
                write!(f, "некорректный размер кадра FLC: {width}×{height}")
            }
            Self::FrameIndex { index, frames } => {
                write!(f, "кадр {index} за пределами FLC: всего кадров {frames}")
            }
            Self::Frame {
                index,
                offset,
                reason,
            } => write!(f, "кадр {index} (смещение {offset}): {reason}"),
            Self::Format { offset, what } => {
                write!(f, "битые данные FLC на смещении {offset}: {what}")
            }
        }
    }
}

impl std::error::Error for FlicError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { needed, available } => {
                write!(f, "кадр обрезан: нужно {needed} байт, доступно {available}")
            }
            Self::BadType { found } => write!(f, "тип кадра не 0xF1FA, а 0x{found:04X}"),
            Self::BadSize { size } => {
                write!(f, "размер кадра {size} меньше {FLIC_FRAME_HEADER_SIZE}")
            }
            Self::BadChunk {
                index,
                offset,
                reason,
            } => write!(f, "чанк {index} (смещение {offset}): {reason}"),
            Self::UnknownChunk { index, kind } => {
                write!(f, "чанк {index}: неизвестный тип 0x{kind:04X}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Заголовок
// ---------------------------------------------------------------------------

/// Заголовок FLIC (128 байт).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlicHeader {
    /// Размер файла из заголовка.
    pub size: u32,
    /// `0xAF12` (FLC) или `0xAF11` (FLI).
    pub kind: u16,
    /// Число кадров.
    pub frames: u16,
    /// Ширина кадра.
    pub width: u16,
    /// Высота кадра.
    pub height: u16,
    /// Глубина цвета (8).
    pub depth: u16,
    /// Флаги (в ресурсах MAX — 3).
    pub flags: u16,
    /// Задержка кадра в миллисекундах (`speed`).
    pub speed_ms: u32,
    /// Смещение первого кадра.
    pub first_frame: u32,
    /// Смещение второго кадра (`oframe2`).
    pub second_frame: u32,
}

// ---------------------------------------------------------------------------
// Чтение
// ---------------------------------------------------------------------------

/// Курсор по байтам с проверками границ.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    base: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            base: pos,
        }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn offset(&self) -> usize {
        self.pos
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], FlicError> {
        let end = self.pos.checked_add(count).ok_or(FlicError::Format {
            offset: self.base,
            what: "переполнение смещения",
        })?;

        if end > self.data.len() {
            return Err(FlicError::Format {
                offset: self.pos,
                what: "данные обрезаны",
            });
        }

        let slice = &self.data[self.pos..end];
        self.pos = end;

        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, FlicError> {
        Ok(self.take(1)?[0])
    }

    fn i8(&mut self) -> Result<i8, FlicError> {
        Ok(self.u8()? as i8)
    }

    fn u16(&mut self) -> Result<u16, FlicError> {
        let bytes = self.take(2)?;

        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn i16(&mut self) -> Result<i16, FlicError> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32, FlicError> {
        let bytes = self.take(4)?;

        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

// ---------------------------------------------------------------------------
// Полотно кадра
// ---------------------------------------------------------------------------

/// Индексное полотно кадра: `width × height` байт палитровых индексов.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    fn set(&mut self, x: usize, y: usize, value: u8) -> Result<(), FlicError> {
        if x >= self.width || y >= self.height {
            return Err(FlicError::Format {
                offset: y * self.width + x,
                what: "пиксель за пределами кадра",
            });
        }

        self.pixels[y * self.width + x] = value;

        Ok(())
    }

    fn run(&mut self, x: usize, y: usize, value: u8, count: usize) -> Result<(), FlicError> {
        if x + count > self.width || y >= self.height {
            return Err(FlicError::Format {
                offset: y * self.width + x,
                what: "серия пикселей выходит за строку кадра",
            });
        }

        let start = y * self.width + x;
        self.pixels[start..start + count].fill(value);

        Ok(())
    }

    fn copy(&mut self, x: usize, y: usize, bytes: &[u8]) -> Result<(), FlicError> {
        if x + bytes.len() > self.width || y >= self.height {
            return Err(FlicError::Format {
                offset: y * self.width + x,
                what: "литералы выходят за строку кадра",
            });
        }

        let start = y * self.width + x;
        self.pixels[start..start + bytes.len()].copy_from_slice(bytes);

        Ok(())
    }

    /// Заполняет кадр одним индексом (`BLACK` = 0).
    fn fill(&mut self, value: u8) {
        self.pixels.fill(value);
    }
}

// ---------------------------------------------------------------------------
// Flic
// ---------------------------------------------------------------------------

/// Один кадр после декодирования.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlicFrame {
    /// Ширина.
    pub width: usize,
    /// Высота.
    pub height: usize,
    /// Индексы палитры, построчно.
    pub pixels: Vec<u8>,
    /// Палитра после применения всех чанков до этого кадра включительно.
    pub palette: FlicPalette,
}

impl FlicFrame {
    /// RGBA-пиксели кадра по системной палитре окна (как C++ `win_draw_rect`:
    /// индексы кадра переводятся текущей палитрой экрана).
    ///
    /// `palette` — 256 цветов `[r, g, b, a]`; для портрета это палитра
    /// `FRAMEPIC` (`crate::panels::UiPalette`), потому что C++ рисует FLIC
    /// в 8-битный буфер окна, не подменяя системную палитру.
    pub fn rgba(&self, palette: &[[u8; 4]; FLIC_PALETTE_SIZE]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);

        for &index in &self.pixels {
            out.extend_from_slice(&palette[usize::from(index)]);
        }

        out
    }

    /// Число пикселей.
    #[inline]
    pub fn len(&self) -> usize {
        self.pixels.len()
    }

    /// Пустой кадр (не бывает у валидного FLC, нужен для `clippy`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }
}

/// Разобранный FLC-файл: заголовок, найденные кадры, сырые байты.
#[derive(Debug, Clone)]
pub struct Flic {
    header: FlicHeader,
    frames: Vec<FrameSpan>,
    data: Vec<u8>,
}

/// Границы кадра в файле.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameSpan {
    offset: usize,
    size: usize,
    chunks: u16,
}

impl Flic {
    /// Разбирает FLC из байтов: заголовок + список кадров (без декодирования).
    ///
    /// # Ошибки
    ///
    /// [`FlicError::BadMagic`] — не `0xAF12` (FLI не поддерживается, как в C++),
    /// [`FlicError::NoFrames`], [`FlicError::BadGeometry`], а также структурные
    /// ошибки кадров ([`FlicError::Frame`]).
    pub fn parse(bytes: &[u8]) -> Result<Self, FlicError> {
        if bytes.len() < FLIC_HEADER_SIZE {
            return Err(FlicError::TooShort {
                len: bytes.len(),
                needed: FLIC_HEADER_SIZE,
            });
        }

        let header = FlicHeader {
            size: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            kind: u16::from_le_bytes([bytes[4], bytes[5]]),
            frames: u16::from_le_bytes([bytes[6], bytes[7]]),
            width: u16::from_le_bytes([bytes[8], bytes[9]]),
            height: u16::from_le_bytes([bytes[10], bytes[11]]),
            depth: u16::from_le_bytes([bytes[12], bytes[13]]),
            flags: u16::from_le_bytes([bytes[14], bytes[15]]),
            speed_ms: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            first_frame: u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]),
            second_frame: u32::from_le_bytes([bytes[84], bytes[85], bytes[86], bytes[87]]),
        };

        if header.kind != FLC_MAGIC {
            return Err(FlicError::BadMagic { found: header.kind });
        }

        if header.frames == 0 {
            return Err(FlicError::NoFrames);
        }

        if header.width == 0 || header.height == 0 {
            return Err(FlicError::BadGeometry {
                width: header.width,
                height: header.height,
            });
        }

        let mut frames = Vec::with_capacity(usize::from(header.frames));
        let mut offset = header.first_frame as usize;

        for index in 0..usize::from(header.frames) {
            let span = Self::frame_span(bytes, offset, index)?;
            offset = span.offset.checked_add(span.size).ok_or(FlicError::Frame {
                index,
                offset: span.offset,
                reason: FrameError::BadSize { size: 0 },
            })?;
            frames.push(span);
        }

        Ok(Self {
            header,
            frames,
            data: bytes.to_vec(),
        })
    }

    /// Читает и разбирает FLC-файл.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FlicError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| FlicError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::parse(&bytes)
    }

    fn frame_span(bytes: &[u8], offset: usize, index: usize) -> Result<FrameSpan, FlicError> {
        if offset + FLIC_FRAME_HEADER_SIZE > bytes.len() {
            return Err(FlicError::Frame {
                index,
                offset,
                reason: FrameError::Truncated {
                    needed: offset + FLIC_FRAME_HEADER_SIZE,
                    available: bytes.len(),
                },
            });
        }

        let size = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]);
        let kind = u16::from_le_bytes([bytes[offset + 4], bytes[offset + 5]]);
        let chunks = u16::from_le_bytes([bytes[offset + 6], bytes[offset + 7]]);

        if kind != FRAME_MAGIC {
            return Err(FlicError::Frame {
                index,
                offset,
                reason: FrameError::BadType { found: kind },
            });
        }

        if (size as usize) < FLIC_FRAME_HEADER_SIZE {
            return Err(FlicError::Frame {
                index,
                offset,
                reason: FrameError::BadSize { size },
            });
        }

        let size = size as usize;

        if offset + size > bytes.len() {
            return Err(FlicError::Frame {
                index,
                offset,
                reason: FrameError::Truncated {
                    needed: offset + size,
                    available: bytes.len(),
                },
            });
        }

        Ok(FrameSpan {
            offset,
            size,
            chunks,
        })
    }

    /// Заголовок.
    #[inline]
    pub fn header(&self) -> &FlicHeader {
        &self.header
    }

    /// Ширина кадра.
    #[inline]
    pub fn width(&self) -> usize {
        usize::from(self.header.width)
    }

    /// Высота кадра.
    #[inline]
    pub fn height(&self) -> usize {
        usize::from(self.header.height)
    }

    /// Число кадров (из заголовка).
    #[inline]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Длительность кадра из заголовка (мс); для портрета C++ её не использует
    /// (анимация идёт фиксированные 15 FPS).
    #[inline]
    pub fn speed_ms(&self) -> u32 {
        self.header.speed_ms
    }

    /// Размер файла из заголовка.
    #[inline]
    pub fn declared_size(&self) -> u32 {
        self.header.size
    }

    /// Сырые байты файла.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Анимированный FLC (`frames > 1`); одиночные кадры C++ считает статикой.
    #[inline]
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// Смещения кадров (для тестов/диагностики): `(offset, size, chunks)`.
    pub fn frame_spans(&self) -> Vec<(usize, usize, u16)> {
        self.frames
            .iter()
            .map(|span| (span.offset, span.size, span.chunks))
            .collect()
    }

    /// Декодирует кадр `index` с нуля: применяет кадры `0..=index`.
    ///
    /// # Ошибки
    ///
    /// [`FlicError::FrameIndex`] — такого кадра нет; [`FlicError::Format`] /
    /// [`FlicError::Frame`] — битые данные.
    pub fn decode_frame(&self, index: usize) -> Result<FlicFrame, FlicError> {
        if index >= self.frames.len() {
            return Err(FlicError::FrameIndex {
                index,
                frames: self.frames.len(),
            });
        }

        let mut canvas = Canvas::new(self.width(), self.height());
        let mut palette: FlicPalette = [[0; 3]; FLIC_PALETTE_SIZE];

        for (frame_index, span) in self.frames[..=index].iter().enumerate() {
            self.decode_span(span, frame_index, &mut canvas, &mut palette)?;
        }

        Ok(FlicFrame {
            width: canvas.width,
            height: canvas.height,
            pixels: canvas.pixels,
            palette,
        })
    }

    /// Декодирует все кадры подряд (для тестов и предпросчёта анимации).
    pub fn decode_all(&self) -> Result<Vec<FlicFrame>, FlicError> {
        let mut canvas = Canvas::new(self.width(), self.height());
        let mut palette: FlicPalette = [[0; 3]; FLIC_PALETTE_SIZE];
        let mut out = Vec::with_capacity(self.frames.len());

        for (index, span) in self.frames.iter().enumerate() {
            self.decode_span(span, index, &mut canvas, &mut palette)?;
            out.push(FlicFrame {
                width: canvas.width,
                height: canvas.height,
                pixels: canvas.pixels.clone(),
                palette,
            });
        }

        Ok(out)
    }

    /// Палитра после всех кадров (6-битные компоненты, как системная палитра
    /// C++-порта).
    pub fn palette(&self) -> Result<FlicPalette, FlicError> {
        let mut canvas = Canvas::new(self.width(), self.height());
        let mut palette: FlicPalette = [[0; 3]; FLIC_PALETTE_SIZE];

        for (index, span) in self.frames.iter().enumerate() {
            self.decode_span(span, index, &mut canvas, &mut palette)?;
        }

        Ok(palette)
    }

    /// Применяет чанки одного кадра.
    fn decode_span(
        &self,
        span: &FrameSpan,
        index: usize,
        canvas: &mut Canvas,
        palette: &mut FlicPalette,
    ) -> Result<(), FlicError> {
        let frame_end = span.offset + span.size;
        let mut reader = Reader::new(
            &self.data[..frame_end],
            span.offset + FLIC_FRAME_HEADER_SIZE,
        );

        for chunk_index in 0..usize::from(span.chunks) {
            let chunk_offset = reader.offset();

            if reader.remaining() < FLIC_CHUNK_HEADER_SIZE {
                return Err(FlicError::Frame {
                    index,
                    offset: span.offset,
                    reason: FrameError::BadChunk {
                        index: chunk_index,
                        offset: chunk_offset,
                        reason: "заголовок чанка не помещается в кадр",
                    },
                });
            }

            let mut header = Reader::new(&self.data[..frame_end], chunk_offset);
            let size = header.u32().map_err(|_| FlicError::Frame {
                index,
                offset: span.offset,
                reason: FrameError::BadChunk {
                    index: chunk_index,
                    offset: chunk_offset,
                    reason: "размер чанка не читается",
                },
            })?;
            let kind = header.u16().map_err(|_| FlicError::Frame {
                index,
                offset: span.offset,
                reason: FrameError::BadChunk {
                    index: chunk_index,
                    offset: chunk_offset,
                    reason: "тип чанка не читается",
                },
            })?;

            if size < FLIC_CHUNK_HEADER_SIZE as u32 {
                return Err(FlicError::Frame {
                    index,
                    offset: span.offset,
                    reason: FrameError::BadChunk {
                        index: chunk_index,
                        offset: chunk_offset,
                        reason: "размер чанка меньше заголовка",
                    },
                });
            }

            let size = size as usize;

            if chunk_offset + size > frame_end {
                return Err(FlicError::Frame {
                    index,
                    offset: span.offset,
                    reason: FrameError::BadChunk {
                        index: chunk_index,
                        offset: chunk_offset,
                        reason: "чанк выходит за конец кадра",
                    },
                });
            }

            let body = &self.data[chunk_offset + FLIC_CHUNK_HEADER_SIZE..chunk_offset + size];

            self.decode_chunk(kind, body, canvas, palette, index, chunk_index)?;
            reader.pos = chunk_offset + size;
        }

        Ok(())
    }

    /// Декодирует один чанк (см. таблицу в документации модуля).
    fn decode_chunk(
        &self,
        kind: u16,
        body: &[u8],
        canvas: &mut Canvas,
        palette: &mut FlicPalette,
        frame: usize,
        chunk: usize,
    ) -> Result<(), FlicError> {
        match kind {
            4 => Self::decode_color(body, 2, palette)?,
            7 => Self::decode_delta_flc(body, canvas)?,
            11 => Self::decode_color(body, 0, palette)?,
            12 => Self::decode_delta_fli(body, canvas)?,
            13 => canvas.fill(0),
            15 => Self::decode_byte_run(body, canvas)?,
            16 => {
                let needed = canvas.width * canvas.height;

                if body.len() < needed {
                    return Err(FlicError::Format {
                        offset: 0,
                        what: "литеральный кадр короче кадра",
                    });
                }

                canvas.pixels.copy_from_slice(&body[..needed]);
            }
            18 => {
                // PSTAMP: превью-«марка», C++ его не рисует. Размер чанка уже
                // проверен в `decode_span`.
            }
            other => {
                return Err(FlicError::Frame {
                    index: frame,
                    offset: 0,
                    reason: FrameError::UnknownChunk {
                        index: chunk,
                        kind: other,
                    },
                });
            }
        }

        Ok(())
    }

    /// Палитровый чанк: `u16 packets`, пакеты `u8 skip, u8 count (0→256), RGB`.
    ///
    /// `shift` — сдвиг в 6-битную системную палитру C++-порта (`2` для
    /// `COLOR_256` = 8-битные байты файла, `0` для `COLOR_64`).
    fn decode_color(body: &[u8], shift: u32, palette: &mut FlicPalette) -> Result<(), FlicError> {
        let mut reader = Reader::new(body, 0);
        let packets = reader.u16()?;
        let mut entry = 0usize;

        for _ in 0..packets {
            let skip = usize::from(reader.u8()?);
            let count = match reader.u8()? {
                0 => 256,
                value => usize::from(value),
            };

            entry += skip;

            if entry + count > FLIC_PALETTE_SIZE {
                return Err(FlicError::Format {
                    offset: reader.offset(),
                    what: "палитровый пакет выходит за 256 цветов",
                });
            }

            for slot in palette.iter_mut().skip(entry).take(count) {
                let rgb = reader.take(3)?;
                *slot = [
                    (u32::from(rgb[0]) >> shift) as u8,
                    (u32::from(rgb[1]) >> shift) as u8,
                    (u32::from(rgb[2]) >> shift) as u8,
                ];
            }

            entry += count;
        }

        Ok(())
    }

    /// `BYTE_RUN` (15): полный кадр, на строку `u8` (число пакетов, не
    /// используется), затем пакеты `i8 count`: `>= 0` — повтор следующего
    /// байта, `< 0` — `-count` литеральных байт.
    fn decode_byte_run(body: &[u8], canvas: &mut Canvas) -> Result<(), FlicError> {
        let mut reader = Reader::new(body, 0);

        for y in 0..canvas.height {
            // Первый байт строки — счётчик пакетов; C++ его пропускает.
            let _packets = reader.u8()?;
            let mut x = 0usize;

            while x < canvas.width {
                let count = reader.i8()?;

                if count >= 0 {
                    let count = usize::from(count as u8);

                    if count == 0 {
                        return Err(FlicError::Format {
                            offset: reader.offset(),
                            what: "нулевая серия в BYTE_RUN",
                        });
                    }

                    let value = reader.u8()?;
                    canvas.run(x, y, value, count)?;
                    x += count;
                } else {
                    let count = count.unsigned_abs() as usize;
                    let bytes = reader.take(count)?;
                    canvas.copy(x, y, bytes)?;
                    x += count;
                }
            }
        }

        Ok(())
    }

    /// `DELTA_FLC` (7), он же `FLI_SS2`: строки со словами-пикселями.
    ///
    /// Формат строки: опциональные слова (`0x8000`), затем `packet_count`,
    /// затем пакеты `u8 skip, i8 words`.
    ///
    /// Опциональное слово: если выставлен бит `0x4000` — это отрицательное
    /// число пропускаемых строк (`-word`), иначе младший байт — последний
    /// пиксель текущей строки (ровно как в `flicsmgr_decode_delta_flc`).
    fn decode_delta_flc(body: &[u8], canvas: &mut Canvas) -> Result<(), FlicError> {
        let mut reader = Reader::new(body, 0);
        let lines = reader.u16()?;
        let mut line = 0usize;

        for _ in 0..lines {
            let mut opt = reader.i16()? as u16;

            while opt & 0x8000 != 0 {
                if opt & 0x4000 != 0 {
                    line += (opt as i16).unsigned_abs() as usize;
                } else {
                    canvas.set(canvas.width - 1, line, (opt & 0xFF) as u8)?;
                }

                opt = reader.i16()? as u16;
            }

            let packets = opt;
            let mut offset = 0usize;

            for _ in 0..packets {
                let skip = usize::from(reader.u8()?);
                let kind = reader.i8()?;

                offset += skip;

                if kind >= 0 {
                    let count = usize::from(kind as u8) * 2;
                    let bytes = reader.take(count)?;
                    canvas.copy(offset, line, bytes)?;
                    offset += count;
                } else {
                    let count = kind.unsigned_abs() as usize;
                    let word = reader.u16()?;

                    for _ in 0..count {
                        canvas.set(offset, line, (word & 0xFF) as u8)?;
                        canvas.set(offset + 1, line, (word >> 8) as u8)?;
                        offset += 2;
                    }
                }
            }

            line += 1;
        }

        Ok(())
    }

    /// `DELTA_FLI` (12), он же `FLI_LC`: `u16 first_line, u16 lines`, в каждой
    /// строке `u8 packets` и пакеты `u8 skip, i8 count` (байтовые).
    ///
    /// В C++-порте чанка нет (в 71 ресурсе MAX.RES он не встречается);
    /// реализован по спецификации для полноты формата.
    fn decode_delta_fli(body: &[u8], canvas: &mut Canvas) -> Result<(), FlicError> {
        let mut reader = Reader::new(body, 0);
        let first_line = usize::from(reader.u16()?);
        let lines = usize::from(reader.u16()?);
        let mut line = first_line;

        for _ in 0..lines {
            if line >= canvas.height {
                return Err(FlicError::Format {
                    offset: reader.offset(),
                    what: "DELTA_FLI за пределами кадра",
                });
            }

            let packets = reader.u8()?;
            let mut offset = 0usize;

            for _ in 0..packets {
                let skip = usize::from(reader.u8()?);
                let count = reader.i8()?;

                offset += skip;

                if count >= 0 {
                    let count = usize::from(count as u8);
                    let bytes = reader.take(count)?;
                    canvas.copy(offset, line, bytes)?;
                    offset += count;
                } else {
                    let count = count.unsigned_abs() as usize;
                    let value = reader.u8()?;
                    canvas.run(offset, line, value, count)?;
                    offset += count;
                }
            }

            line += 1;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Проигрыватель
// ---------------------------------------------------------------------------

/// Инкрементальный проигрыватель кадров — порт `flicsmgr_advance_animation`:
/// кадры применяются по одному, без повторного декодирования с нуля.
#[derive(Debug, Clone)]
pub struct FlicPlayer<'a> {
    flic: &'a Flic,
    canvas: Canvas,
    palette: FlicPalette,
    frame_index: usize,
}

impl<'a> FlicPlayer<'a> {
    /// Начинает с кадра 0.
    pub fn new(flic: &'a Flic) -> Result<Self, FlicError> {
        let mut player = Self {
            flic,
            canvas: Canvas::new(flic.width(), flic.height()),
            palette: [[0; 3]; FLIC_PALETTE_SIZE],
            frame_index: 0,
        };

        player.apply(0)?;

        Ok(player)
    }

    fn apply(&mut self, index: usize) -> Result<(), FlicError> {
        let span = self
            .flic
            .frames
            .get(index)
            .copied()
            .ok_or(FlicError::FrameIndex {
                index,
                frames: self.flic.frames.len(),
            })?;

        self.flic
            .decode_span(&span, index, &mut self.canvas, &mut self.palette)?;
        self.frame_index = index;

        Ok(())
    }

    /// Номер текущего кадра.
    #[inline]
    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    /// Текущие индексы палитры.
    #[inline]
    pub fn pixels(&self) -> &[u8] {
        &self.canvas.pixels
    }

    /// Палитра после текущего кадра.
    #[inline]
    pub fn palette(&self) -> &FlicPalette {
        &self.palette
    }

    /// Следующий кадр. После последнего — снова кадр 1 (как
    /// `flicsmgr_advance_animation` при `animate == 2`), у статичного FLC
    /// (`frames == 1`) остаётся кадр 0.
    pub fn advance(&mut self) -> Result<usize, FlicError> {
        if self.flic.frames.len() <= 1 {
            return Ok(self.frame_index);
        }

        let next = self.frame_index + 1;

        if next >= self.flic.frames.len() {
            self.apply(1)?;
        } else {
            self.apply(next)?;
        }

        Ok(self.frame_index)
    }

    /// Текущий кадр как [`FlicFrame`] (для сверки с `decode_frame`).
    pub fn frame(&self) -> FlicFrame {
        FlicFrame {
            width: self.canvas.width,
            height: self.canvas.height,
            pixels: self.canvas.pixels.clone(),
            palette: self.palette,
        }
    }
}

// ---------------------------------------------------------------------------
// Тесты
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Минимальный FLC из готовых кадров: `frames` кадров по `chunks`.
    fn build_flic(width: u16, height: u16, frames: &[&[(u16, Vec<u8>)]], speed: u32) -> Vec<u8> {
        let mut out = vec![0u8; FLIC_HEADER_SIZE];
        let mut frame_offsets = Vec::new();

        for chunks in frames {
            frame_offsets.push(out.len() as u32);

            let mut body = Vec::new();

            for (kind, payload) in *chunks {
                body.extend_from_slice(
                    &(payload.len() as u32 + FLIC_CHUNK_HEADER_SIZE as u32).to_le_bytes(),
                );
                body.extend_from_slice(&kind.to_le_bytes());
                body.extend_from_slice(payload);
            }

            let size = body.len() as u32 + FLIC_FRAME_HEADER_SIZE as u32;
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
            out.extend_from_slice(&(chunks.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0u8; 8]);
            out.extend_from_slice(&body);
        }

        let total = out.len() as u32;

        out[0..4].copy_from_slice(&total.to_le_bytes());
        out[4..6].copy_from_slice(&FLC_MAGIC.to_le_bytes());
        out[6..8].copy_from_slice(&(frames.len() as u16).to_le_bytes());
        out[8..10].copy_from_slice(&width.to_le_bytes());
        out[10..12].copy_from_slice(&height.to_le_bytes());
        out[12..14].copy_from_slice(&8u16.to_le_bytes());
        out[14..16].copy_from_slice(&3u16.to_le_bytes());
        out[16..20].copy_from_slice(&speed.to_le_bytes());
        out[80..84].copy_from_slice(&frame_offsets[0].to_le_bytes());
        out[84..88].copy_from_slice(&frame_offsets.last().copied().unwrap_or(0).to_le_bytes());

        out
    }

    /// `BYTE_RUN`-кадр: каждая строка — байт пакетов и литеральные пиксели.
    fn byte_run(width: usize, height: usize, value: u8) -> Vec<u8> {
        let mut body = Vec::new();

        for _ in 0..height {
            body.push(1u8);
            body.push(width as u8); // count, < 128 — повтор
            body.push(value);
        }

        body
    }

    #[test]
    fn rejects_short_fli_and_bad_geometry() {
        assert!(matches!(
            Flic::parse(&[0u8; 4]),
            Err(FlicError::TooShort { .. })
        ));

        let mut fli = build_flic(4, 4, &[&[(15, byte_run(4, 4, 7))]], 71);
        fli[4..6].copy_from_slice(&FLI_MAGIC.to_le_bytes());
        assert!(matches!(
            Flic::parse(&fli),
            Err(FlicError::BadMagic { found: FLI_MAGIC })
        ));

        let mut zero = build_flic(0, 4, &[&[(15, Vec::new())]], 71);
        zero[6..8].copy_from_slice(&0u16.to_le_bytes());
        assert!(matches!(Flic::parse(&zero), Err(FlicError::NoFrames)));

        let zero = build_flic(0, 4, &[&[(15, byte_run(1, 4, 1))]], 71);
        assert!(matches!(
            Flic::parse(&zero),
            Err(FlicError::BadGeometry { .. })
        ));
    }

    #[test]
    fn decodes_byte_run_and_literal_frames() {
        let literal: Vec<u8> = (0..16u8).collect();
        let bytes = build_flic(
            4,
            4,
            &[
                &[(15, byte_run(4, 4, 5))],
                &[(16, literal)],
                &[(15, byte_run(4, 4, 0))],
            ],
            71,
        );
        let flic = Flic::parse(&bytes).unwrap();

        assert_eq!((flic.width(), flic.height()), (4, 4));
        assert_eq!(flic.frame_count(), 3);
        assert_eq!(flic.speed_ms(), 71);
        assert_eq!(flic.declared_size(), bytes.len() as u32);
        assert!(flic.is_animated());

        assert_eq!(flic.decode_frame(0).unwrap().pixels, vec![5; 16]);
        assert_eq!(
            flic.decode_frame(1).unwrap().pixels,
            (0..16u8).collect::<Vec<_>>()
        );
        assert_eq!(flic.decode_frame(2).unwrap().pixels, vec![0; 16]);

        let frame = flic.decode_frame(1).unwrap();
        let palette = [[1u8, 2, 3, 4]; 256];
        let rgba = frame.rgba(&palette);
        assert_eq!(rgba.len(), 16 * 4);
        assert_eq!(&rgba[0..4], &[1, 2, 3, 4]);
    }

    #[test]
    fn decodes_color_chunks_into_palette() {
        // Пакет палитры: skip 0, count 2, два цвета (8-битные).
        let mut color = Vec::new();
        color.extend_from_slice(&1u16.to_le_bytes());
        color.extend_from_slice(&[0, 2, 255, 0, 0, 0, 255, 0]);
        // COLOR_64: 6-битные значения без сдвига, skip 1 → индекс 1.
        let mut color64 = Vec::new();
        color64.extend_from_slice(&1u16.to_le_bytes());
        color64.extend_from_slice(&[1, 1, 63, 0, 0, 0, 63, 0]);

        let bytes = build_flic(
            2,
            2,
            &[
                &[(4, color), (15, byte_run(2, 2, 0))],
                &[(11, color64), (15, byte_run(2, 2, 1))],
            ],
            71,
        );
        let flic = Flic::parse(&bytes).unwrap();
        let first = flic.decode_frame(0).unwrap();
        let second = flic.decode_frame(1).unwrap();

        // COLOR_256 сдвигает на 2 бита — как C++ (`>> 2` в 6-битную палитру).
        assert_eq!(first.palette[0], [63, 0, 0]);
        assert_eq!(first.palette[1], [0, 63, 0]);
        // Второй кадр: COLOR_64 без сдвига перезаписал индекс 1.
        assert_eq!(second.palette[0], [63, 0, 0]);
        assert_eq!(second.palette[1], [63, 0, 0]);
        assert_eq!(second.palette[2], [0, 0, 0]);
        assert_eq!(first.pixels, vec![0; 4]);
        assert_eq!(second.pixels, vec![1; 4]);
    }

    #[test]
    fn decodes_delta_flc_skip_last_byte_and_runs() {
        // 4×4, кадр 0 — все 9, кадр 1 — дельта:
        // строк 2: первая строка начинается с пропуска 1 строки, затем пакет
        // «пропустить 1, повторить 1 слово (0x0A0A)», во второй строке —
        // опциональное слово с последним байтом 0x0B.
        let mut delta = Vec::new();
        delta.extend_from_slice(&2u16.to_le_bytes()); // 2 строки
                                                      // опциональное слово -1 = 0xFFFF → пропуск одной строки
        delta.extend_from_slice(&0xFFFFu16.to_le_bytes());
        delta.extend_from_slice(&1u16.to_le_bytes()); // 1 пакет
        delta.push(1); // skip
        delta.push(0xFF); // -1 слово
        delta.extend_from_slice(&0x0A0Au16.to_le_bytes());
        // вторая строка: опциональное слово 0x800B — последний байт 0x0B
        delta.extend_from_slice(&0x800Bu16.to_le_bytes());
        delta.extend_from_slice(&0u16.to_le_bytes()); // 0 пакетов

        let bytes = build_flic(4, 4, &[&[(15, byte_run(4, 4, 9))], &[(7, delta)]], 71);
        let flic = Flic::parse(&bytes).unwrap();
        let first = flic.decode_frame(0).unwrap();
        let second = flic.decode_frame(1).unwrap();

        assert_eq!(&second.pixels[0..4], &[9, 9, 9, 9]);
        assert_eq!(&second.pixels[4..8], &[9, 0x0A, 0x0A, 9]);
        assert_eq!(&second.pixels[8..12], &[9, 9, 9, 0x0B]);
        assert_eq!(first.pixels, vec![9; 16]);
        // Кадры независимы: повторное декодирование даёт те же пиксели.
        assert_eq!(flic.decode_frame(0).unwrap(), first);
    }

    #[test]
    fn player_matches_random_access_decoding() {
        // Дельта кадра 1: в первой строке пакет «skip 0, повторить слово 0x0202».
        let mut delta = Vec::new();
        delta.extend_from_slice(&1u16.to_le_bytes()); // 1 строка
        delta.extend_from_slice(&1u16.to_le_bytes()); // 1 пакет
        delta.push(0); // skip
        delta.push(0xFF); // -1 слово
        delta.extend_from_slice(&0x0202u16.to_le_bytes());

        let bytes = build_flic(
            4,
            4,
            &[
                &[(15, byte_run(4, 4, 1))],
                &[(7, delta)],
                &[(13, Vec::new())],
            ],
            71,
        );
        let flic = Flic::parse(&bytes).unwrap();
        let mut player = FlicPlayer::new(&flic).unwrap();

        assert_eq!(player.frame_index(), 0);
        assert_eq!(player.frame(), flic.decode_frame(0).unwrap());
        assert_eq!(player.pixels(), vec![1; 16].as_slice());

        assert_eq!(player.advance().unwrap(), 1);
        assert_eq!(player.frame(), flic.decode_frame(1).unwrap());
        assert_eq!(&player.pixels()[0..4], &[2, 2, 1, 1]);

        assert_eq!(player.advance().unwrap(), 2);
        assert_eq!(player.frame(), flic.decode_frame(2).unwrap());
        assert_eq!(player.pixels(), vec![0; 16].as_slice());

        // Через последний кадр — снова кадр 1 (как `animate == 2` в C++:
        // дельта применяется к текущему буферу, а не к кадру 0).
        assert_eq!(player.advance().unwrap(), 1);
        let mut expected = vec![0u8; 16];
        expected[0] = 2;
        expected[1] = 2;
        assert_eq!(player.pixels(), expected.as_slice());
        // И это не то же самое, что декодирование с нуля: так работает C++.
        assert_ne!(player.frame().pixels, flic.decode_frame(1).unwrap().pixels);

        // Статичный FLC: advance не двигает кадр.
        let single = build_flic(2, 2, &[&[(15, byte_run(2, 2, 3))]], 71);
        let single = Flic::parse(&single).unwrap();
        let mut player = FlicPlayer::new(&single).unwrap();

        assert_eq!(player.advance().unwrap(), 0);
        assert_eq!(player.pixels(), vec![3; 4].as_slice());
    }

    #[test]
    fn rejects_truncated_and_broken_chunks() {
        let good = build_flic(4, 4, &[&[(15, byte_run(4, 4, 1))]], 71);

        // Обрезанный файл.
        assert!(matches!(
            Flic::parse(&good[..good.len() - 2]),
            Err(FlicError::Frame { .. })
        ));

        // Чанк с нулевым размером: кадр разбирается, но не декодируется.
        let mut broken = good.clone();
        let chunk_offset = FLIC_HEADER_SIZE + FLIC_FRAME_HEADER_SIZE;
        broken[chunk_offset..chunk_offset + 4].copy_from_slice(&0u32.to_le_bytes());
        let flic = Flic::parse(&broken).unwrap();
        assert!(matches!(
            flic.decode_frame(0),
            Err(FlicError::Frame {
                reason: FrameError::BadChunk { .. },
                ..
            })
        ));

        // Неизвестный чанк.
        let mut broken = good.clone();
        broken[chunk_offset + 4..chunk_offset + 6].copy_from_slice(&77u16.to_le_bytes());
        let flic = Flic::parse(&broken).unwrap();
        assert!(matches!(
            flic.decode_frame(0),
            Err(FlicError::Frame {
                reason: FrameError::UnknownChunk { kind: 77, .. },
                ..
            })
        ));

        // Нулевая серия BYTE_RUN — не зацикливаемся.
        let mut zero_run = Vec::new();
        zero_run.push(0u8);
        zero_run.extend_from_slice(&[0, 0, 0, 0, 0]);
        let bytes = build_flic(4, 1, &[&[(15, zero_run)]], 71);
        let flic = Flic::parse(&bytes).unwrap();
        assert!(matches!(
            flic.decode_frame(0),
            Err(FlicError::Format { .. })
        ));

        // Индекс кадра за пределами.
        let flic = Flic::parse(&good).unwrap();
        assert!(matches!(
            flic.decode_frame(5),
            Err(FlicError::FrameIndex {
                index: 5,
                frames: 1
            })
        ));
    }

    #[test]
    fn palette_and_black_chunks_reset_canvas() {
        let bytes = build_flic(
            2,
            1,
            &[
                &[(15, byte_run(2, 1, 4))],
                &[(13, Vec::new())],
                &[(16, vec![1, 2])],
            ],
            71,
        );
        let flic = Flic::parse(&bytes).unwrap();

        assert_eq!(flic.decode_frame(1).unwrap().pixels, vec![0, 0]);
        assert_eq!(flic.decode_frame(2).unwrap().pixels, vec![1, 2]);
        // PTSAMP/неизвестные кадры не ломают `palette()`.
        assert_eq!(flic.palette().unwrap(), [[0; 3]; 256]);
    }
}
