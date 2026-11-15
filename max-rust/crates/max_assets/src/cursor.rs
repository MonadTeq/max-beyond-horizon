//! Курсоры мыши M.A.X. — `Cursor_Descriptor` из `MAX.RES`.
//!
//! Оракул — `max-cpp-port/src/cursor.cpp` (`Cursor_Init`). В C++ курсор
//! грузится как **simple image** (`ImageSimpleHeader`) и используется так:
//!
//! ```cpp
//! struct ImageSimpleHeader* sprite = ResourceManager_LoadResource(id);
//! cursor->data       = &sprite->transparent_color;  // offset 8 в файле
//! cursor->width      = sprite->width;
//! cursor->height     = sprite->height;
//! cursor->ulx        = sprite->ulx;                 // hotspot x
//! cursor->uly        = sprite->uly;                 // hotspot y
//! cursor->frame_count = 1;
//! if (cursor->width < cursor->height) {
//!     cursor->frame_count = cursor->height / cursor->width;
//!     cursor->height      = cursor->height / cursor->frame_count;
//! }
//! ```
//!
//! То есть отдельного `u16 frame_count` в файле нет: кадры анимации уложены
//! вертикально, а число кадров выводится из отношения высоты к ширине. Байт с
//! offset 8 — прозрачный индекс (он же первый пиксель кадра), пиксели кадра —
//! `width * height * frame_count` индексов палитры.
//!
//! Проверка по реальному `MAX.RES` (все размеры сходятся без остатка):
//!
//! | тег | заголовок | кадры | байт |
//! |---|---|---|---|
//! | `HIDDNPTR` | 1×1, hotspot (0,0) | 1 | 9 |
//! | `HANDPTR` | 18×18, hotspot (0,1) | 1 | 332 |
//! | `ARROW_N` | 16×16, hotspot (7,1) | 1 | 264 |
//! | `ENMY_PTR` | 37×33, hotspot (18,13) | 1 | 1 229 |
//! | `PTR_PATH` | 23×230, hotspot (13,13) | 10 по 23×23 | 5 298 |

use std::fmt;

use crate::res::{ResArchive, ResError};

/// Сколько курсоров грузит оригинал (`CURSOR_CURSOR_COUNT`).
pub const CURSOR_COUNT: usize = 30;

/// Таблица ресурсов курсоров — точный порядок `Cursor_ResourceLut` из
/// `cursor.cpp:55-58` (индекс = `CursorId`).
pub const CURSOR_RESOURCES: [&str; CURSOR_COUNT] = [
    "HIDDNPTR", "HANDPTR", "ARROW_N", "ARROW_NE", "ARROW_E", "ARROW_SE", "ARROW_S", "ARROW_SW",
    "ARROW_W", "ARROW_NW", "MAP_PTR", "MINI_PTR", "FRND_FIX", "FRND_XFR", "FRND_FUE", "PTR_RLD",
    "FRND_LOD", "FRND_PTR", "ENMY_PTR", "PTR_FTRG", "UNIT_GO", "UNIT_NGO", "WAY_PTR", "GROUPPTR",
    "ACTVTPTR", "MAP_PTR", "STEALPTR", "DISBLPTR", "PTR_PATH", "PTR_HELP",
];

/// Размер `ImageSimpleHeader` в файле (8 байт полей; 9-й байт — первый пиксель).
pub const CURSOR_HEADER: usize = 8;

/// Ошибка разбора курсора.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorError {
    /// Данных меньше 8-байтного заголовка.
    TooShort {
        /// Фактическая длина ресурса.
        len: usize,
        /// Минимально необходимая длина.
        needed: usize,
    },
    /// Ширина/высота неположительны.
    BadSize {
        /// Ширина из заголовка.
        width: i32,
        /// Высота из заголовка.
        height: i32,
    },
    /// Пикселей меньше, чем `width * frame_height * frame_count`.
    Truncated {
        /// Нужно байт (заголовок + пиксели).
        needed: usize,
        /// Фактическая длина.
        got: usize,
    },
}

impl fmt::Display for CursorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, needed } => {
                write!(f, "курсор обрезан: {len} байт (нужно минимум {needed})")
            }
            Self::BadSize { width, height } => {
                write!(f, "некорректный размер курсора {width}×{height}")
            }
            Self::Truncated { needed, got } => write!(
                f,
                "пиксели курсора обрезаны: нужно {needed} байт, есть {got}"
            ),
        }
    }
}

impl std::error::Error for CursorError {}

/// Разобранный `Cursor_Descriptor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorDescriptor {
    /// Ширина одного кадра.
    pub width: i32,
    /// Высота одного кадра.
    pub height: i32,
    /// Горячая точка (hotspot) по x — `ulx` заголовка.
    pub hot_x: i32,
    /// Горячая точка (hotspot) по y — `uly` заголовка.
    pub hot_y: i32,
    /// Число кадров анимации (1 у статичных курсоров).
    pub frame_count: u16,
    /// Прозрачный индекс (первый байт пикселей, `ImageSimpleHeader.transparent_color`).
    pub transparent: u8,
    pixels: Vec<u8>,
}

impl CursorDescriptor {
    /// Разбирает тело ресурса курсора.
    pub fn decode(bytes: &[u8]) -> Result<Self, CursorError> {
        if bytes.len() < CURSOR_HEADER {
            return Err(CursorError::TooShort {
                len: bytes.len(),
                needed: CURSOR_HEADER,
            });
        }

        let width = i32::from(i16::from_le_bytes([bytes[0], bytes[1]]));
        let height = i32::from(i16::from_le_bytes([bytes[2], bytes[3]]));
        let hot_x = i32::from(i16::from_le_bytes([bytes[4], bytes[5]]));
        let hot_y = i32::from(i16::from_le_bytes([bytes[6], bytes[7]]));

        if width <= 0 || height <= 0 {
            return Err(CursorError::BadSize { width, height });
        }

        let mut frame_count = 1u16;
        let mut frame_height = height;

        if width < height {
            // C++: `frame_count = height / width; height /= frame_count;` —
            // деление целочисленное. У `MINI_PTR` (15×16) получается 1 кадр
            // высотой 16, хотя высота и не кратна ширине.
            frame_count = u16::try_from(height / width).unwrap_or(1);
            frame_height = height / i32::from(frame_count);
        }

        let pixels_len = width as usize
            * frame_height as usize
            * usize::from(frame_count);
        let needed = CURSOR_HEADER + pixels_len;

        if bytes.len() < needed {
            return Err(CursorError::Truncated {
                needed,
                got: bytes.len(),
            });
        }

        Ok(Self {
            width,
            height: frame_height,
            hot_x,
            hot_y,
            frame_count,
            transparent: bytes[CURSOR_HEADER],
            pixels: bytes[CURSOR_HEADER..needed].to_vec(),
        })
    }

    /// Число пикселей одного кадра.
    pub fn frame_pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Пиксели кадра (`None`, если индекс кадра вне диапазона).
    pub fn frame(&self, index: usize) -> Option<&[u8]> {
        if index >= usize::from(self.frame_count) {
            return None;
        }

        let start = index * self.frame_pixels();
        let end = start + self.frame_pixels();

        self.pixels.get(start..end)
    }

    /// Индекс палитры в кадре; `None` за границей кадра.
    pub fn pixel(&self, frame: usize, x: u32, y: u32) -> Option<u8> {
        if x >= self.width as u32 || y >= self.height as u32 {
            return None;
        }

        let frame = self.frame(frame)?;

        frame.get(y as usize * self.width as usize + x as usize).copied()
    }

    /// Все пиксели (кадры подряд, row-major).
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Курсор анимированный (больше одного кадра).
    pub fn is_animated(&self) -> bool {
        self.frame_count > 1
    }
}

/// Набор курсоров в порядке [`CURSOR_RESOURCES`].
#[derive(Debug, Clone, Default)]
pub struct CursorSet {
    slots: Vec<Option<CursorDescriptor>>,
}

impl CursorSet {
    /// Грузит все курсоры, которые есть и разбираются в архиве.
    ///
    /// Отсутствующий/битый курсор — просто `None` в слоте: `Cursor_Init` в C++
    /// тоже пропускает ресурсы, которые не загрузились.
    pub fn load(archive: &ResArchive) -> Self {
        let mut slots = Vec::with_capacity(CURSOR_COUNT);

        for name in CURSOR_RESOURCES {
            slots.push(
                archive
                    .read(name)
                    .ok()
                    .and_then(|bytes| CursorDescriptor::decode(bytes).ok()),
            );
        }

        Self { slots }
    }

    /// Курсор по индексу (`CursorId`).
    pub fn get(&self, index: usize) -> Option<&CursorDescriptor> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// Сколько курсоров загрузилось.
    pub fn loaded(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }

    /// Индексы незагруженных курсоров (для отчёта/предупреждений).
    pub fn missing(&self) -> Vec<&'static str> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_none())
            .map(|(index, _)| CURSOR_RESOURCES[index])
            .collect()
    }

    /// Разбирает ошибку чтения ресурса (для отчёта).
    pub fn read_error(archive: &ResArchive, name: &str) -> Option<ResError> {
        archive.read(name).err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает курсор: заголовок + кадры.
    fn build(width: i16, height: i16, hot_x: i16, hot_y: i16, frames: &[&[u8]]) -> Vec<u8> {
        let mut bytes = Vec::new();

        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&hot_x.to_le_bytes());
        bytes.extend_from_slice(&hot_y.to_le_bytes());

        for frame in frames {
            bytes.extend_from_slice(frame);
        }

        bytes
    }

    #[test]
    fn animated_cursor_frames_are_derived_from_height() {
        // C++: width < height -> frame_count = height / width, height /= frames.
        let frame0 = vec![0u8; 2 * 2];
        let frame1 = vec![1u8; 2 * 2];
        let frame2 = vec![2u8; 2 * 2];
        let bytes = build(2, 6, 1, 1, &[&frame0, &frame1, &frame2]);
        let cursor = CursorDescriptor::decode(&bytes).expect("курсор");

        assert_eq!((cursor.width, cursor.height), (2, 2));
        assert_eq!(cursor.frame_count, 3);
        assert!(cursor.is_animated());
        assert_eq!(cursor.hot_x, 1);
        assert_eq!(cursor.transparent, 0);
        assert_eq!(cursor.frame(0), Some(frame0.as_slice()));
        assert_eq!(cursor.frame(2), Some(frame2.as_slice()));
        assert_eq!(cursor.frame(3), None);
        assert_eq!(cursor.pixel(1, 0, 0), Some(1));
    }

    #[test]
    fn static_cursor_keeps_the_full_height() {
        let pixels = vec![7u8; 4 * 3];
        let bytes = build(4, 3, 2, 1, &[&pixels]);
        let cursor = CursorDescriptor::decode(&bytes).expect("курсор");

        assert_eq!((cursor.width, cursor.height), (4, 3));
        assert_eq!(cursor.frame_count, 1);
        assert!(!cursor.is_animated());
        assert_eq!(cursor.transparent, 7);
        assert_eq!(cursor.frame(0).map(<[u8]>::len), Some(12));
    }

    #[test]
    fn truncated_and_invalid_cursors_are_errors() {
        assert!(matches!(
            CursorDescriptor::decode(&[0u8; 4]),
            Err(CursorError::TooShort { .. })
        ));
        assert!(matches!(
            CursorDescriptor::decode(&build(0, 4, 0, 0, &[])),
            Err(CursorError::BadSize { .. })
        ));

        // Высота 5 при ширине 2: C++ даёт 2 кадра по 2 строки (деление
        // нацело), а не ошибку.
        let bytes = build(2, 5, 0, 0, &[&[7u8; 8]]);
        let cursor = CursorDescriptor::decode(&bytes).expect("MINI_PTR-подобный курсор");
        assert_eq!((cursor.frame_count, cursor.height), (2, 2));

        // Заголовок просит 4×4 пикселя, а их нет.
        assert!(matches!(
            CursorDescriptor::decode(&build(4, 4, 0, 0, &[&[0u8; 3]])),
            Err(CursorError::Truncated { .. })
        ));
    }

    #[test]
    fn real_max_res_cursors_match_the_cpp_descriptors() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../max-original/MAX.RES");

        if !path.is_file() {
            return;
        }

        let archive = ResArchive::open(&path).expect("MAX.RES");
        let cursors = CursorSet::load(&archive);

        assert_eq!(cursors.loaded(), CURSOR_COUNT, "в MAX.RES 30 курсоров");
        assert!(cursors.missing().is_empty());

        // Значения из cursor.cpp/MAX.RES (см. таблицу в шапке модуля).
        let hand = cursors.get(1).expect("HANDPTR");
        assert_eq!((hand.width, hand.height), (18, 18));
        assert_eq!((hand.hot_x, hand.hot_y), (0, 1));
        assert_eq!(hand.frame_count, 1);

        let arrow = cursors.get(2).expect("ARROW_N");
        assert_eq!((arrow.width, arrow.height), (16, 16));
        assert_eq!((arrow.hot_x, arrow.hot_y), (7, 1));

        let enemy = cursors.get(18).expect("ENMY_PTR");
        assert_eq!((enemy.width, enemy.height), (37, 33));
        assert_eq!((enemy.hot_x, enemy.hot_y), (18, 13));

        let path_cursor = cursors.get(28).expect("PTR_PATH");
        assert_eq!((path_cursor.width, path_cursor.height), (23, 23));
        assert_eq!(path_cursor.frame_count, 10);
        assert_eq!((path_cursor.hot_x, path_cursor.hot_y), (13, 13));

        let hidden = cursors.get(0).expect("HIDDNPTR");
        assert_eq!((hidden.width, hidden.height), (1, 1));

        // Каждый кадр каждого курсора имеет ровно width*height пикселей.
        for index in 0..CURSOR_COUNT {
            let cursor = cursors.get(index).expect(CURSOR_RESOURCES[index]);

            for frame in 0..usize::from(cursor.frame_count) {
                assert_eq!(
                    cursor.frame(frame).map(<[u8]>::len),
                    Some(cursor.frame_pixels()),
                    "{} кадр {frame}",
                    CURSOR_RESOURCES[index]
                );
            }
        }
    }
}
