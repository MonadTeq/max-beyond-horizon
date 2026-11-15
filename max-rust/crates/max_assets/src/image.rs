//! Картинки интерфейса из `MAX.RES`: `ImageSimpleHeader` и `ImageBigHeader`.
//!
//! Оригинал (и C++-порт, `window_manager.cpp`) рисует весь хром окна картинками:
//! `FRAMEPIC` — фон окна 640×480 со **своей** палитрой и RLE, `PANELTOP`/
//! `PANELBTM` — половинки левой панели 171×239/241 (9-байтный заголовок, пиксели
//! по системной палитре `COLOR.PAL`).
//!
//! ```text
//! ImageSimpleHeader (9 байт, packed):
//!   i16 width, i16 height, i16 ulx, i16 uly, u8 transparent_color
//!   далее u8[width*height] — индексы палитры
//!
//! ImageBigHeader (777 байт, packed):
//!   i16 ulx, i16 uly, i16 width, i16 height
//!   u8 palette[3 * 256]   (RGB)
//!   u8 transparent_color
//!   далее по строкам RLE: i16 n; n > 0 → n литеральных байт, n < 0 → -n раз
//!   следующий байт (см. `WindowManager_DecodeBigImage`)
//! ```

/// Ошибка разбора картинки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// Данных меньше, чем обещает заголовок.
    Truncated { needed: usize, got: usize },
    /// Нулевой/подозрительно большой размер.
    BadSize { width: i32, height: i32 },
    /// RLE-строка вылезла за границы или за конец данных.
    BadRle { row: usize, offset: i32 },
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated { needed, got } => {
                write!(f, "картинка обрезана: нужно {needed} байт, есть {got}")
            }
            Self::BadSize { width, height } => {
                write!(f, "некорректный размер картинки {width}x{height}")
            }
            Self::BadRle { row, offset } => {
                write!(f, "RLE-строка {row}: позиция {offset} вне строки")
            }
        }
    }
}

impl std::error::Error for ImageError {}

/// Размер заголовка [`SimpleImage`] в файле.
///
/// В C++ `sizeof(ImageSimpleHeader) == 9` (8 байт полей + `transparent_color`),
/// но `WindowManager_DecodeSimpleImage` берёт данные с `&image->transparent_color`,
/// то есть с 8-го байта: девятый байт структуры — это **первый пиксель**, а не
/// отдельное поле. Проверено на `PANELTOP`/`PANELBTM`: `8 + w*h == size` точно.
pub const SIMPLE_HEADER: usize = 8;
/// Размер заголовка [`BigImage`] (`static_assert(sizeof(ImageBigHeader) == 777)`).
pub const BIG_HEADER: usize = 777;
/// Байт на цвет в палитре big-картинки.
pub const PALETTE_BYTES: usize = 3 * 256;

/// Картинка без собственной палитры (индексы → `COLOR.PAL`).
#[derive(Debug, Clone)]
pub struct SimpleImage {
    /// Ширина в пикселях.
    pub width: i16,
    /// Высота в пикселях.
    pub height: i16,
    /// Смещение от левого верхнего угла окна.
    pub ulx: i16,
    /// Смещение от верха окна.
    pub uly: i16,
    /// Индекс прозрачного цвета (в файле поля нет — оставлено для совместимости
    /// с C++, где он читается как первый пиксель; здесь всегда 0).
    pub transparent: u8,
    pixels: Vec<u8>,
}

impl SimpleImage {
    /// Разбирает `ImageSimpleHeader` + пиксели.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageError> {
        if bytes.len() < SIMPLE_HEADER {
            return Err(ImageError::Truncated {
                needed: SIMPLE_HEADER,
                got: bytes.len(),
            });
        }

        let width = i16::from_le_bytes([bytes[0], bytes[1]]);
        let height = i16::from_le_bytes([bytes[2], bytes[3]]);
        let ulx = i16::from_le_bytes([bytes[4], bytes[5]]);
        let uly = i16::from_le_bytes([bytes[6], bytes[7]]);
        // Поле `transparent` в файле отсутствует (см. комментарий к SIMPLE_HEADER).
        let transparent = 0u8;

        if width <= 0 || height <= 0 {
            return Err(ImageError::BadSize {
                width: i32::from(width),
                height: i32::from(height),
            });
        }

        let size = width as usize * height as usize;
        if bytes.len() < SIMPLE_HEADER + size {
            return Err(ImageError::Truncated {
                needed: SIMPLE_HEADER + size,
                got: bytes.len(),
            });
        }

        Ok(Self {
            width,
            height,
            ulx,
            uly,
            transparent,
            pixels: bytes[SIMPLE_HEADER..SIMPLE_HEADER + size].to_vec(),
        })
    }

    /// Индекс палитры в точке (`None` — вне картинки).
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width as u32 || y >= self.height as u32 {
            return None;
        }

        self.pixels.get(y as usize * self.width as usize + x as usize).copied()
    }

    /// Индексы палитры (row-major).
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// Картинка со своей палитрой и RLE (фон окна).
#[derive(Debug, Clone)]
pub struct BigImage {
    /// Смещение от левого верхнего угла окна.
    pub ulx: i16,
    /// Смещение от верха окна.
    pub uly: i16,
    /// Ширина в пикселях.
    pub width: i16,
    /// Высота в пикселях.
    pub height: i16,
    /// Палитра RGB (256 × 3).
    pub palette: [u8; PALETTE_BYTES],
    /// Индекс прозрачного цвета.
    pub transparent: u8,
    pixels: Vec<u8>,
}

impl BigImage {
    /// Разбирает `ImageBigHeader` + RLE-строки.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageError> {
        if bytes.len() < BIG_HEADER {
            return Err(ImageError::Truncated {
                needed: BIG_HEADER,
                got: bytes.len(),
            });
        }

        let ulx = i16::from_le_bytes([bytes[0], bytes[1]]);
        let uly = i16::from_le_bytes([bytes[2], bytes[3]]);
        let width = i16::from_le_bytes([bytes[4], bytes[5]]);
        let height = i16::from_le_bytes([bytes[6], bytes[7]]);

        if width <= 0 || height <= 0 {
            return Err(ImageError::BadSize {
                width: i32::from(width),
                height: i32::from(height),
            });
        }

        let mut palette = [0u8; PALETTE_BYTES];
        palette.copy_from_slice(&bytes[8..8 + PALETTE_BYTES]);
        let transparent = bytes[8 + PALETTE_BYTES];

        let row_len = width as usize;
        let mut pixels = vec![0u8; row_len * height as usize];
        let mut offset = BIG_HEADER;

        'rows: for row in 0..height as usize {
            let mut line_position = 0usize;

            while line_position < row_len {
                if offset + 2 > bytes.len() {
                    // Поток может кончиться на пару байт раньше, чем ждёт цикл
                    // строк: C++ читает за границей записи и получает мусор.
                    // Остаток кадра оставляем нулевым вместо ошибки.
                    break 'rows;
                }

                let word = i16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
                offset += 2;

                // Кодировщик может выпустить чанк, который выходит за строку
                // (например, у FRAMEPIC первая строка: литерал 255 + повтор
                // 1616 при ширине 640). C++ (`WindowManager_DecodeBigImage`)
                // пишет его подряд и просто переходит к следующей строке, а
                // следующая строка начинается с её начала и перекрывает «хвост».
                // Воспроизводим это поведение 1:1: пишем с clipping'ом по
                // концу буфера, счётчик строки увеличиваем на длину чанка.
                let start = row * row_len + line_position;
                let limit = pixels.len();

                if word > 0 {
                    let count = word as usize;
                    if offset + count > bytes.len() {
                        break 'rows;
                    }

                    let end = (start + count).min(limit);
                    let written = end.saturating_sub(start);

                    pixels[start..end].copy_from_slice(&bytes[offset..offset + written]);
                    offset += count;
                    line_position += count;
                } else {
                    let count = (-i32::from(word)) as usize;
                    if offset >= bytes.len() {
                        break 'rows;
                    }

                    let value = bytes[offset];
                    offset += 1;

                    let end = (start + count).min(limit);
                    for pixel in &mut pixels[start..end] {
                        *pixel = value;
                    }
                    line_position += count;
                }
            }
        }

        Ok(Self {
            ulx,
            uly,
            width,
            height,
            palette,
            transparent,
            pixels,
        })
    }

    /// Цвет RGB по индексу палитры картинки.
    pub fn color(&self, index: u8) -> [u8; 3] {
        let base = index as usize * 3;

        [
            self.palette[base],
            self.palette[base + 1],
            self.palette[base + 2],
        ]
    }

    /// Индекс палитры в точке.
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width as u32 || y >= self.height as u32 {
            return None;
        }

        self.pixels.get(y as usize * self.width as usize + x as usize).copied()
    }

    /// Цвет RGB в точке.
    pub fn rgb(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        self.pixel(x, y).map(|index| self.color(index))
    }

    /// Индексы палитры (row-major).
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_image_header_and_pixels() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3i16.to_le_bytes());
        bytes.extend_from_slice(&2i16.to_le_bytes());
        bytes.extend_from_slice(&7i16.to_le_bytes());
        bytes.extend_from_slice(&9i16.to_le_bytes());
        // Пиксели начинаются сразу за 8-байтным заголовком: в C++ девятый байт
        // структуры (`transparent_color`) — это первый пиксель.
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6]);

        let image = SimpleImage::decode(&bytes).expect("должна разобраться");

        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!((image.ulx, image.uly), (7, 9));
        assert_eq!(image.pixel(0, 0), Some(1));
        assert_eq!(image.pixel(2, 1), Some(6));
        assert_eq!(image.pixel(3, 1), None);
        assert_eq!(bytes.len(), SIMPLE_HEADER + 6, "8-байтный заголовок + пиксели");
    }

    #[test]
    fn simple_image_rejects_truncated() {
        // Заголовок просит 3×2, а пикселей четыре.
        let bytes = [3u8, 0, 2, 0, 0, 0, 0, 0, 0, 1, 2, 3];

        assert!(matches!(
            SimpleImage::decode(&bytes),
            Err(ImageError::Truncated { .. })
        ));
    }

    /// Заголовок big-картинки + RLE-строки.
    fn big_image_bytes(rows: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&2i16.to_le_bytes()); // width
        bytes.extend_from_slice(&(rows.len() as i16).to_le_bytes());
        // палитра: индекс 1 = (10,20,30), индекс 2 = (40,50,60)
        let mut palette = [0u8; PALETTE_BYTES];
        palette[3..6].copy_from_slice(&[10, 20, 30]);
        palette[6..9].copy_from_slice(&[40, 50, 60]);
        bytes.extend_from_slice(&palette);
        bytes.push(0);
        for row in rows {
            bytes.extend_from_slice(row);
        }
        bytes
    }

    #[test]
    fn big_image_rle_literal_and_repeat() {
        // Строка 1: литерал из 2 байт; строка 2: повтор -2 значения 2.
        let mut row1 = Vec::new();
        row1.extend_from_slice(&2i16.to_le_bytes());
        row1.extend_from_slice(&[1, 2]);
        let mut row2 = Vec::new();
        row2.extend_from_slice(&(-2i16).to_le_bytes());
        row2.push(2);

        let image = BigImage::decode(&big_image_bytes(&[row1, row2])).expect("RLE разбирается");

        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.pixel(0, 0), Some(1));
        assert_eq!(image.pixel(1, 0), Some(2));
        assert_eq!(image.pixel(0, 1), Some(2));
        assert_eq!(image.pixel(1, 1), Some(2));
        assert_eq!(image.rgb(0, 0), Some([10, 20, 30]));
        assert_eq!(image.rgb(1, 1), Some([40, 50, 60]));
    }

    #[test]
    fn big_image_allows_chunk_crossing_row_boundary() {
        // Как у FRAMEPIC: чанк шире строки. C++ пишет подряд и переходит к
        // следующей строке; хвост перекрывается началом следующей строки.
        let mut rows = Vec::new();
        // строка 0: литерал 3 байта при ширине 2 (залезает в строку 1)
        let mut row0 = Vec::new();
        row0.extend_from_slice(&3i16.to_le_bytes());
        row0.extend_from_slice(&[1, 2, 1]);
        rows.push(row0);
        // строка 1: повтор -2 значения 2
        let mut row1 = Vec::new();
        row1.extend_from_slice(&(-2i16).to_le_bytes());
        row1.push(2);
        rows.push(row1);

        let image = BigImage::decode(&big_image_bytes(&rows)).expect("пересечение строк допустимо");

        assert_eq!(image.pixel(0, 0), Some(1));
        assert_eq!(image.pixel(1, 0), Some(2));
        assert_eq!(image.pixel(0, 1), Some(2), "хвост перекрыт началом строки 1");
        assert_eq!(image.pixel(1, 1), Some(2));
    }

    #[test]
    fn big_image_tolerates_short_stream() {
        // Первый литерал читается, второй обрывается: остаток кадра — нули.
        let mut row = Vec::new();
        row.extend_from_slice(&1i16.to_le_bytes());
        row.push(1);
        row.extend_from_slice(&9i16.to_le_bytes()); // данных для него уже нет
        row.extend_from_slice(&[2, 3]);

        let image = BigImage::decode(&big_image_bytes(&[row])).expect("короткий поток допустим");

        assert_eq!(image.pixel(0, 0), Some(1));
        assert_eq!(image.pixel(1, 0), Some(0), "недостающее — нули");
    }
}
