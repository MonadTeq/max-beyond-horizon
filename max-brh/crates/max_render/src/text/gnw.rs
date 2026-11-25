//! GNW fonts of the original: `GNW_TEXT_FONT_1/2/5` from `FONT1.FON`/`FONT2.FON`/
//! `FONT5.FON`.
//!
//! # Where the data comes from
//!
//! The C++ port draws text through FreeType from the TTF resources `FONT_1`/`FONT_2`/
//! `FONT_5` (`PATCHES.RES`), and those TTFs were converted from the **GNW raster
//! fonts** `FONT1.FON`/`FONT2.FON`/`FONT5.FON` (files next to `MAX.RES`).
//! The original game drew exactly these bitmaps. Verified with the tool
//! [`tools/font_vectors`](../../../../tools/font_vectors): for ASCII both paths
//! (FreeType over the TTF and the `.FON` decoder) give **byte-identical** bitmaps
//! of the strings `M.A.X.`, `Hits 15/15`, `End Turn`, `SURVEY STATUS COLORS`, `044-034`
//! for all three fonts.
//!
//! # The `.FON` (GNW) format
//!
//! ```text
//! +0x00  u32 version        (0x00000100)
//! +0x04  u32 height         16 / 9 / 10 — glyph cell height
//! +0x08  u32                0
//! +0x0C  u8[16]             signature/reserved (0xFF,0xFF,0xF0,0x05,…)
//! +0x1C  255 × { u32 width; u32 offset }   glyphs of characters 1…255
//! +0x81C (2068)            bitmaps: rows of ceil(width/8) bytes, MSB-first,
//!                          bit 1 = "ink"; offset — from the start of this zone
//! ```
//!
//! The glyph of character 0 is empty, of width 0, and lies in the first `height` bytes of the
//! bitmap zone (so the first `offset` equals `height`). The sum of the glyph sizes
//! equals the file length — the decoder checks this and does not panic on corrupt data.
//!
//! # Metrics
//!
//! * glyph width = `advance` (verified against the TTF `hmtx`: it matches for all
//!   ASCII characters of `FONT_2`/`FONT_5` and for 91 of 95 in `FONT_1`; for `,`/`@`/`Z`/
//!   `}` they differ by 1 px — that is TTF hinting, see `vectors.txt`);
//! * line height — `height` (`Text_GetHeight`), the line spacing in
//!   `Text_TextBox` — `height + 1`;
//! * `Text_TextLine` draws the shadow/outline to the right and down: for every pixel
//!   of the text `(x+1, y+1)` is painted with `outline` if there is text there too, otherwise with
//!   `shadow`. This reproduces [`draw_text_gnw_shadowed`].
//!
//! # Fill and outline (`Text_Blit`)
//!
//! The original paints the shifted outline/shadow copy **first** and the glyph fill
//! **on top of it** (`text.cpp:492-496` draws the `GNW_TEXT_OUTLINE` copy, then
//! `:521-523` writes the fill colour), so the fill always wins and stays complete.
//! [`draw_text_gnw_shadowed`] follows the same order: the back layer (outline/shadow)
//! is painted first, the fill last. With the opposite order the outline colour ate the
//! diagonal pixels of the fill; two-pixel-wide stems of `FONT1` lost their second
//! column and the text looked hollow/outline-only instead of filled.

use std::sync::OnceLock;

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};

/// The `.FON` header is 28 bytes.
pub const GNW_HEADER: usize = 28;
/// Glyphs in the table (characters `1..=255`; the zero one is empty).
pub const GNW_GLYPHS: usize = 255;
/// Size of a glyph record: `width` + `offset`.
pub const GNW_RECORD: usize = 8;
/// Start of the bitmap zone: 28 + 255·8.
pub const GNW_BITMAP_BASE: usize = GNW_HEADER + GNW_GLYPHS * GNW_RECORD;
/// Width of the glyph that replaces a missing character.
pub const GNW_FALLBACK: char = '?';

/// The original `.FON` compiled into `max_render` (from `max-original/`).
const FONT_1_BYTES: &[u8] = include_bytes!("../../data/gnw_font_1.fnt");
/// The original `.FON` `FONT2.FON` (9 px, uppercase only).
const FONT_2_BYTES: &[u8] = include_bytes!("../../data/gnw_font_2.fnt");
/// The original `.FON` `FONT5.FON` (10 px).
const FONT_5_BYTES: &[u8] = include_bytes!("../../data/gnw_font_5.fnt");

/// Which of the GNW fonts is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GnwFontId {
    /// `GNW_TEXT_FONT_1` — `FONT1.FON`, 16 px (headings/menus).
    Font1,
    /// `GNW_TEXT_FONT_2` — `FONT2.FON`, 9 px (button labels, statistics).
    Font2,
    /// `GNW_TEXT_FONT_5` — `FONT5.FON`, 10 px (End Turn, counters, coordinates).
    Font5,
}

impl GnwFontId {
    /// All fonts that are ported.
    pub const ALL: [Self; 3] = [Self::Font1, Self::Font2, Self::Font5];

    /// Glyph cell height (`Text_GetHeight`).
    pub const fn height(self) -> i32 {
        match self {
            Self::Font1 => 16,
            Self::Font2 => 9,
            Self::Font5 => 10,
        }
    }

    /// Source bytes of the `.FON`.
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Font1 => FONT_1_BYTES,
            Self::Font2 => FONT_2_BYTES,
            Self::Font5 => FONT_5_BYTES,
        }
    }

    /// Original file name (for diagnostics).
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Font1 => "FONT1.FON",
            Self::Font2 => "FONT2.FON",
            Self::Font5 => "FONT5.FON",
        }
    }

    /// Parsed font (decoded once per process).
    pub fn font(self) -> &'static GnwFont {
        static FONT_1: OnceLock<GnwFont> = OnceLock::new();
        static FONT_2: OnceLock<GnwFont> = OnceLock::new();
        static FONT_5: OnceLock<GnwFont> = OnceLock::new();

        let cell = match self {
            Self::Font1 => &FONT_1,
            Self::Font2 => &FONT_2,
            Self::Font5 => &FONT_5,
        };

        cell.get_or_init(|| {
            GnwFont::decode(self.bytes()).unwrap_or_else(|error| {
                panic!("встроенный {} не разобрался: {error}", self.file_name())
            })
        })
    }
}

/// `.FON` parsing error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GnwFontError {
    /// The file is shorter than the header and the glyph table.
    TooShort {
        /// Actual length.
        len: usize,
        /// Minimum required.
        needed: usize,
    },
    /// The font height is zero or unreasonable.
    BadHeight {
        /// The value from the header.
        height: u32,
    },
    /// A glyph record contradicts the sequential bitmap layout.
    BadRecord {
        /// Glyph number (1-based).
        glyph: usize,
        /// Width from the record.
        width: u32,
        /// Offset from the record.
        offset: u32,
        /// Expected offset.
        expected: u32,
    },
    /// The bitmaps extend beyond the end of the file.
    Truncated {
        /// Needed bytes from the start of the file.
        needed: usize,
        /// Available bytes.
        len: usize,
    },
}

impl std::fmt::Display for GnwFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { len, needed } => {
                write!(f, "шрифт GNW обрезан: {len} байт (нужно минимум {needed})")
            }
            Self::BadHeight { height } => write!(f, "некорректная высота шрифта GNW: {height}"),
            Self::BadRecord {
                glyph,
                width,
                offset,
                expected,
            } => write!(
                f,
                "глиф {glyph}: ширина {width}, смещение {offset} (ожидалось {expected})"
            ),
            Self::Truncated { needed, len } => {
                write!(
                    f,
                    "битмапы шрифта GNW выходят за файл: нужно {needed}, есть {len}"
                )
            }
        }
    }
}

impl std::error::Error for GnwFontError {}

/// Metrics of one glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GnwGlyph {
    /// Width in pixels (= advance).
    pub width: u16,
    /// Glyph row height (= font height).
    pub height: u16,
    /// Bytes per bitmap row (`ceil(width/8)`).
    pub stride: u16,
    /// Bitmap offset from [`GnwFont::bitmap_base`].
    pub offset: u32,
    /// Bitmap size (`stride * height`).
    pub size: u32,
}

/// Parsed GNW raster font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GnwFont {
    height: u16,
    glyphs: Vec<GnwGlyph>,
    bitmap: Vec<u8>,
}

impl GnwFont {
    /// Parses a GNW `.FON`.
    ///
    /// Checks that the glyph records are sequential (`offset` equals the accumulated
    /// size) and that the bitmaps do not extend beyond the file. Format — see the module
    /// documentation.
    pub fn decode(bytes: &[u8]) -> Result<Self, GnwFontError> {
        if bytes.len() < GNW_BITMAP_BASE {
            return Err(GnwFontError::TooShort {
                len: bytes.len(),
                needed: GNW_BITMAP_BASE,
            });
        }

        let height = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if height == 0 || height > 64 {
            return Err(GnwFontError::BadHeight { height });
        }

        let mut glyphs = Vec::with_capacity(GNW_GLYPHS + 1);
        // Glyph 0 is empty (in the file it occupies `height` bytes at the start of the zone).
        glyphs.push(GnwGlyph {
            width: 0,
            height: height as u16,
            stride: 0,
            offset: 0,
            size: height,
        });

        let mut cursor = height;

        for index in 0..GNW_GLYPHS {
            let record = GNW_HEADER + index * GNW_RECORD;
            let width = u32::from_le_bytes([
                bytes[record],
                bytes[record + 1],
                bytes[record + 2],
                bytes[record + 3],
            ]);
            let offset = u32::from_le_bytes([
                bytes[record + 4],
                bytes[record + 5],
                bytes[record + 6],
                bytes[record + 7],
            ]);

            if width > u32::from(u16::MAX) || offset != cursor {
                return Err(GnwFontError::BadRecord {
                    glyph: index + 1,
                    width,
                    offset,
                    expected: cursor,
                });
            }

            let stride = width.div_ceil(8);
            let size = stride * height;
            glyphs.push(GnwGlyph {
                width: width as u16,
                height: height as u16,
                stride: stride as u16,
                offset,
                size,
            });
            cursor += size;
        }

        let needed = GNW_BITMAP_BASE + cursor as usize;
        if needed > bytes.len() {
            return Err(GnwFontError::Truncated {
                needed,
                len: bytes.len(),
            });
        }

        Ok(Self {
            height: height as u16,
            glyphs,
            bitmap: bytes[GNW_BITMAP_BASE..needed].to_vec(),
        })
    }

    /// Glyph cell height (`Text_GetHeight`).
    pub fn height(&self) -> i32 {
        i32::from(self.height)
    }

    /// Line spacing of the original (`Text_TextBox`/`Text_TextLine`: height+1).
    pub fn line_height(&self) -> i32 {
        i32::from(self.height) + 1
    }

    /// Number of glyphs in the table (256, including the empty zero one).
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Start of the bitmap zone (for tests/diagnostics).
    pub fn bitmap_base(&self) -> usize {
        GNW_BITMAP_BASE
    }

    /// Metrics of a character; outside Latin-1 — the [`GNW_FALLBACK`] glyph.
    pub fn glyph(&self, ch: char) -> GnwGlyph {
        let code = ch as usize;
        let index = if code <= GNW_GLYPHS {
            code
        } else {
            GNW_FALLBACK as usize
        };

        self.glyphs[index]
    }

    /// Bitmap of a character (rows of `stride` bytes, MSB-first).
    pub fn glyph_bitmap(&self, ch: char) -> &[u8] {
        let glyph = self.glyph(ch);
        let start = glyph.offset as usize;
        &self.bitmap[start..start + glyph.size as usize]
    }

    /// Glyph pixel (`true` — "ink").
    pub fn glyph_ink(&self, ch: char, x: i32, y: i32) -> bool {
        let glyph = self.glyph(ch);

        if x < 0 || y < 0 || x >= i32::from(glyph.width) || y >= i32::from(glyph.height) {
            return false;
        }

        let row = y as usize * usize::from(glyph.stride);
        let byte = self.glyph_bitmap(ch)[row + x as usize / 8];

        byte & (0x80 >> (x as usize % 8)) != 0
    }

    /// Character width (advance).
    pub fn char_width(&self, ch: char) -> i32 {
        i32::from(self.glyph(ch).width)
    }

    /// Line width in pixels (`Text_GetWidth`).
    pub fn text_width(&self, text: &str) -> i32 {
        text.chars().map(|ch| self.char_width(ch)).sum()
    }
}

/// Draws a line with the GNW font at `(x, y)` (top-left of the cell).
///
/// Returns the line width in pixels. Drawing is clipped by the bounds of the
/// buffer; outside Latin-1 `?` is used.
pub fn draw_text_gnw(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    font: &GnwFont,
    text: &str,
    color: Rgba,
) -> i32 {
    let mut pen_x = x;

    for ch in text.chars() {
        let glyph = font.glyph(ch);
        let bitmap = font.glyph_bitmap(ch);
        let width = i32::from(glyph.width);

        if pen_x >= buffer.width() as i32 {
            // The pen only moves right: nothing further will be visible.
            break;
        }

        for row in 0..i32::from(glyph.height) {
            let pixel_y = y + row;

            if pixel_y < 0 || pixel_y >= buffer.height() as i32 {
                continue;
            }

            let start = row as usize * usize::from(glyph.stride);

            for column in 0..width {
                let byte = bitmap[start + column as usize / 8];

                if byte & (0x80 >> (column as usize % 8)) == 0 {
                    continue;
                }

                buffer.set(pen_x + column, pixel_y, color);
            }
        }

        pen_x += width;
    }

    font.text_width(text)
}

/// The same as [`draw_text_gnw`], but with a shadow/outline like `Text_TextLine`.
///
/// For every "ink" pixel `(x, y)` the original paints `(x+1, y+1)`:
/// `outline` if there is text there too, otherwise `shadow`. The traversal order is bottom
/// to top, so an already repainted pixel no longer counts as text (as
/// in C++, where the loop reads and writes one buffer).
///
/// The back layer is painted first and the glyph fill last, exactly like
/// `Text_Blit` (outline copy at `text.cpp:492-496`, fill at `:521-523`): the fill is
/// never eaten by the outline, so the glyphs stay solid (not hollow).
#[allow(clippy::too_many_arguments)]
pub fn draw_text_gnw_shadowed(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    font: &GnwFont,
    text: &str,
    base: Rgba,
    outline: Rgba,
    shadow: Rgba,
) -> i32 {
    let width = font.text_width(text) + 1;
    let height = font.height() + 1;
    let stride = width.max(1) as usize;

    // 0 — empty, 1 — base, 2 — outline, 3 — shadow.
    let mut mask = vec![0u8; stride * height.max(1) as usize];
    let mut pen_x = 0i32;

    for ch in text.chars() {
        let glyph = font.glyph(ch);
        let bitmap = font.glyph_bitmap(ch);

        for row in 0..i32::from(glyph.height) {
            let start = row as usize * usize::from(glyph.stride);

            for column in 0..i32::from(glyph.width) {
                let byte = bitmap[start + column as usize / 8];

                if byte & (0x80 >> (column as usize % 8)) == 0 {
                    continue;
                }

                let px = pen_x + column;

                if px >= 0 && px < width && row < height {
                    mask[row as usize * stride + px as usize] = 1;
                }
            }
        }

        pen_x += i32::from(glyph.width);
    }

    // 0 — nothing, 2 — outline, 3 — shadow: the back layer, painted under the fill.
    let mut back = vec![0u8; stride * height.max(1) as usize];

    for row in (0..height - 1).rev() {
        for column in 0..width - 1 {
            let index = row as usize * stride + column as usize;

            if mask[index] != 1 {
                continue;
            }

            let target = index + stride + 1;

            back[target] = if mask[target] == 1 { 2 } else { 3 };
        }
    }

    // The back layer first (outline/shadow), the glyph fill on top — the composition
    // of `Text_Blit` with `GNW_TEXT_OUTLINE` (`text.cpp:492-523`). The fill must not
    // be replaced by the outline colour: otherwise a two-pixel stem keeps only one
    // column of fill and the text looks hollow.
    for row in 0..height {
        for column in 0..width {
            let color = match back[row as usize * stride + column as usize] {
                2 => outline,
                3 => shadow,
                _ => continue,
            };

            buffer.set(x + column, y + row, color);
        }
    }

    for row in 0..height {
        for column in 0..width {
            if mask[row as usize * stride + column as usize] != 1 {
                continue;
            }

            buffer.set(x + column, y + row, base);
        }
    }

    font.text_width(text)
}

/// `Text_Blit` with the `GNW_TEXT_OUTLINE` flag (`text.cpp:492-523`): a one-pixel
/// copy of the text shifted down and right in `outline`, then the glyph fill in
/// `base` on top of it (`Color_RGB2Color(0)` — black — is the outline in C++).
pub fn draw_text_gnw_outlined(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    font: &GnwFont,
    text: &str,
    base: Rgba,
    outline: Rgba,
) -> i32 {
    draw_text_gnw(buffer, x + 1, y + 1, font, text, outline);
    draw_text_gnw(buffer, x, y, font, text, base)
}

/// Draws one line centered in the rectangle (like `Text_TextBox` with
/// `horizontal_align`, `vertical_align`).
///
/// Returns the drawn width of the line.
pub fn draw_text_gnw_centered(
    buffer: &mut RgbaBuffer,
    rect: PixelRect,
    font: &GnwFont,
    text: &str,
    color: Rgba,
) -> i32 {
    let text_width = font.text_width(text);
    let x = rect.ulx + (rect.width - (text_width + 1)).max(0) / 2;
    let y = rect.uly + ((rect.height + 1) - font.height()).max(0) / 2;

    draw_text_gnw(buffer, x, y, font, text, color)
}

/// Centered line with a shadow (`Text_TextLine` + `Button::SetCaption`).
#[allow(clippy::too_many_arguments)]
pub fn draw_text_gnw_centered_shadowed(
    buffer: &mut RgbaBuffer,
    rect: PixelRect,
    font: &GnwFont,
    text: &str,
    base: Rgba,
    outline: Rgba,
    shadow: Rgba,
) -> i32 {
    let text_width = font.text_width(text);
    let x = rect.ulx + (rect.width - (text_width + 1)).max(0) / 2;
    let y = rect.uly + ((rect.height + 1) - font.height()).max(0) / 2;

    draw_text_gnw_shadowed(buffer, x, y, font, text, base, outline, shadow)
}

#[cfg(test)]
#[path = "gnw_tests.rs"]
mod tests;
