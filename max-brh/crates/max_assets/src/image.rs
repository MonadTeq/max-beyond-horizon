//! Interface images from `MAX.RES`: `ImageSimpleHeader` and `ImageBigHeader`.
//!
//! The original (and the C++ port, `window_manager.cpp`) draws the whole window chrome with images:
//! `FRAMEPIC` — the 640×480 window background with **its own** palette and RLE, `PANELTOP`/
//! `PANELBTM` — halves of the left panel 171×239/241 (9-byte header, pixels
//! via the system palette `COLOR.PAL`).
//!
//! ```text
//! ImageSimpleHeader (9 bytes, packed):
//!   i16 width, i16 height, i16 ulx, i16 uly, u8 transparent_color
//!   then u8[width*height] — palette indices
//!
//! ImageBigHeader (777 bytes, packed):
//!   i16 ulx, i16 uly, i16 width, i16 height
//!   u8 palette[3 * 256]   (RGB)
//!   u8 transparent_color
//!   then per row RLE: i16 n; n > 0 → n literal bytes, n < 0 → -n times
//!   the next byte (see `WindowManager_DecodeBigImage`)
//! ```

/// Image parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// Less data than the header promises.
    Truncated { needed: usize, got: usize },
    /// Zero/suspiciously large size.
    BadSize { width: i32, height: i32 },
    /// An RLE row ran past the bounds or past the end of the data.
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

/// Size of the [`SimpleImage`] header in the file.
///
/// In C++ `sizeof(ImageSimpleHeader) == 9` (8 bytes of fields + `transparent_color`),
/// but `WindowManager_DecodeSimpleImage` takes the data from `&image->transparent_color`,
/// that is, from the 8th byte: the ninth byte of the structure is the **first pixel**, not
/// a separate field. Verified on `PANELTOP`/`PANELBTM`: `8 + w*h == size` exactly.
pub const SIMPLE_HEADER: usize = 8;
/// Offset of the [`BigImage`] RLE stream in the resource.
///
/// C++ `sizeof(ImageBigHeader) == 777` (after the palette there is a field
/// `transparent_color`), but `WindowManager_DecodeBigImage` takes the data from
/// `&image->transparent_color`, i.e. the RLE starts **at** byte 776. Verified
/// on real resources: decoding from 776 gives a complete frame (`FRAMEPIC`, `ENDGAME*`),
/// while from 777 — a one-byte shift and "scanline" artifacts. Therefore the offset is 776;
/// `transparent` in [`BigImage`] is read from the same byte (for background images
/// C++ does not use this field).
pub const BIG_HEADER: usize = 776;
/// Bytes per color in the big-image palette.
pub const PALETTE_BYTES: usize = 3 * 256;

/// Image without its own palette (indices → `COLOR.PAL`).
#[derive(Debug, Clone)]
pub struct SimpleImage {
    /// Width in pixels.
    pub width: i16,
    /// Height in pixels.
    pub height: i16,
    /// Offset from the top-left corner of the window.
    pub ulx: i16,
    /// Offset from the top of the window.
    pub uly: i16,
    /// Transparent color index: the first byte of the pixels (in C++ the field
    /// `transparent_color` is `&pixels[0]`).
    pub transparent: u8,
    pixels: Vec<u8>,
}

impl SimpleImage {
    /// Parses `ImageSimpleHeader` + pixels.
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
        // There is no `transparent_color` field in the file: C++ reads it as the first
        // byte of the pixels (`&image->transparent_color`), so the transparent
        // index is `pixels[0]`.
        let transparent = bytes[SIMPLE_HEADER];

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

    /// Palette index at a point (`None` — outside the image).
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width as u32 || y >= self.height as u32 {
            return None;
        }

        self.pixels
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    /// Palette indices (row-major).
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// Image with its own palette and RLE (the window background).
#[derive(Debug, Clone)]
pub struct BigImage {
    /// Offset from the top-left corner of the window.
    pub ulx: i16,
    /// Offset from the top of the window.
    pub uly: i16,
    /// Width in pixels.
    pub width: i16,
    /// Height in pixels.
    pub height: i16,
    /// RGB palette (256 × 3).
    pub palette: [u8; PALETTE_BYTES],
    /// Transparent color index.
    pub transparent: u8,
    pixels: Vec<u8>,
}

impl BigImage {
    /// Parses `ImageBigHeader` + RLE rows.
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
                    // The stream may end a couple of bytes earlier than the row loop
                    // expects: C++ reads past the record boundary and gets garbage.
                    // We leave the rest of the frame zeroed instead of erroring.
                    break 'rows;
                }

                let word = i16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
                offset += 2;

                // The encoder may emit a chunk that runs past the row
                // (e.g. in FRAMEPIC the first row: a literal of 255 + a repeat of
                // 1616 at a width of 640). C++ (`WindowManager_DecodeBigImage`)
                // writes it sequentially and simply moves to the next row, and
                // the next row starts at its beginning and overwrites the "tail".
                // We reproduce this behavior 1:1: we write with clipping at
                // the end of the buffer and advance the row counter by the chunk length.
                let start = row * row_len + line_position;
                let limit = pixels.len();

                if word > 0 {
                    let count = word as usize;
                    if offset + count > bytes.len() {
                        // In `SNOW_PIC` (128×128) the stream contains a service
                        // literal of 29744 that runs past the record: if all rows are cut off,
                        // the picture stays almost black. We start
                        // the next row at the next word — that way the decoder
                        // resynchronizes, and all the other big images
                        // (FRAMEPIC/PLANETSE/MULTGAME/P_*) stay
                        // byte-identical (verified by the test
                        // `snow_pic_decodes_all_rows`).
                        break;
                    }

                    let end = (start + count).min(limit);
                    let written = end.saturating_sub(start);

                    pixels[start..end].copy_from_slice(&bytes[offset..offset + written]);
                    offset += count;
                    line_position += count;
                } else {
                    let count = (-i32::from(word)) as usize;
                    if offset >= bytes.len() {
                        break;
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

    /// RGB color by the image's palette index.
    pub fn color(&self, index: u8) -> [u8; 3] {
        let base = index as usize * 3;

        [
            self.palette[base],
            self.palette[base + 1],
            self.palette[base + 2],
        ]
    }

    /// Palette index at a point.
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width as u32 || y >= self.height as u32 {
            return None;
        }

        self.pixels
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    /// RGB color at a point.
    pub fn rgb(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        self.pixel(x, y).map(|index| self.color(index))
    }

    /// Palette indices (row-major).
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
        // The pixels start right after the 8-byte header: in C++ the ninth byte
        // of the structure (`transparent_color`) is the first pixel.
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6]);

        let image = SimpleImage::decode(&bytes).expect("должна разобраться");

        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!((image.ulx, image.uly), (7, 9));
        assert_eq!(image.pixel(0, 0), Some(1));
        assert_eq!(image.pixel(2, 1), Some(6));
        assert_eq!(image.pixel(3, 1), None);
        assert_eq!(
            image.transparent, 1,
            "прозрачный индекс — первый пиксель (C++ читает его с offset 8)"
        );
        assert_eq!(
            bytes.len(),
            SIMPLE_HEADER + 6,
            "8-байтный заголовок + пиксели"
        );
    }

    #[test]
    fn simple_image_rejects_truncated() {
        // The header asks for 3×2, but there are four pixels.
        let bytes = [3u8, 0, 2, 0, 0, 0, 0, 0, 0, 1, 2, 3];

        assert!(matches!(
            SimpleImage::decode(&bytes),
            Err(ImageError::Truncated { .. })
        ));
    }

    /// Big-image header + RLE rows.
    fn big_image_bytes(rows: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&2i16.to_le_bytes()); // width
        bytes.extend_from_slice(&(rows.len() as i16).to_le_bytes());
        // palette: index 1 = (10,20,30), index 2 = (40,50,60)
        let mut palette = [0u8; PALETTE_BYTES];
        palette[3..6].copy_from_slice(&[10, 20, 30]);
        palette[6..9].copy_from_slice(&[40, 50, 60]);
        bytes.extend_from_slice(&palette);
        for row in rows {
            bytes.extend_from_slice(row);
        }
        bytes
    }

    #[test]
    fn big_image_rle_literal_and_repeat() {
        // Row 1: a literal of 2 bytes; row 2: a repeat of -2 of value 2.
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
        // As in FRAMEPIC: the chunk is wider than the row. C++ writes sequentially and moves to
        // the next row; the tail is overwritten by the start of the next row.
        let mut rows = Vec::new();
        // row 0: a literal of 3 bytes at a width of 2 (spills into row 1)
        let mut row0 = Vec::new();
        row0.extend_from_slice(&3i16.to_le_bytes());
        row0.extend_from_slice(&[1, 2, 1]);
        rows.push(row0);
        // row 1: a repeat of -2 of value 2
        let mut row1 = Vec::new();
        row1.extend_from_slice(&(-2i16).to_le_bytes());
        row1.push(2);
        rows.push(row1);

        let image = BigImage::decode(&big_image_bytes(&rows)).expect("пересечение строк допустимо");

        assert_eq!(image.pixel(0, 0), Some(1));
        assert_eq!(image.pixel(1, 0), Some(2));
        assert_eq!(
            image.pixel(0, 1),
            Some(2),
            "хвост перекрыт началом строки 1"
        );
        assert_eq!(image.pixel(1, 1), Some(2));
    }

    #[test]
    fn big_image_tolerates_short_stream() {
        // The first literal is read, the second is cut off: the rest of the frame is zeros.
        let mut row = Vec::new();
        row.extend_from_slice(&1i16.to_le_bytes());
        row.push(1);
        row.extend_from_slice(&9i16.to_le_bytes()); // there is no more data for it
        row.extend_from_slice(&[2, 3]);

        let image = BigImage::decode(&big_image_bytes(&[row])).expect("короткий поток допустим");

        assert_eq!(image.pixel(0, 0), Some(1));
        assert_eq!(image.pixel(1, 0), Some(0), "недостающее — нули");
    }

    /// `SNOW_PIC` — decoder regression: its RLE contains a literal that
    /// runs past the record (a service chunk). If **all** rows are cut off there,
    /// the planet picture stays almost black (visible in the 8×3 grid).
    #[test]
    fn snow_pic_decodes_all_rows() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let path = root.join("max-original/MAX.RES");

        if !path.is_file() {
            return;
        }

        let archive = crate::res::ResArchive::open(&path).expect("MAX.RES");
        let bytes = archive.read("SNOW_PIC").expect("SNOW_PIC");
        let image = BigImage::decode(bytes).expect("SNOW_PIC");

        assert_eq!((image.width, image.height), (128, 128));

        let nonzero = image.pixels().iter().filter(|&&value| value != 0).count();

        assert!(
            nonzero > 10_000,
            "SNOW_PIC почти пустая: {nonzero} ненулевых"
        );
        assert!(
            (0..128).any(|x| image.pixel(x, 100) != Some(0)),
            "нижние строки планеты должны декодироваться"
        );
    }
}
