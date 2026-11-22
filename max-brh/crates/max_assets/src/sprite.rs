//! Multi-frame sprites of M.A.X. — bodies of RES resources with tags `COMMTWR`…`WALDO`
//! and `S_*` (shadows).
//!
//! The format is described in `max-original-dasm/formats/spw.md` (important: the `*.SPW` files in
//! the game directory are RIFF/WAVE sounds, not sprites, §1):
//!
//! ```text
//! ImageMultiHeader:
//! +0x00  u16 image_count
//! +0x02  u32 frame_offset[image_count]   — frame offsets FROM THE START OF THE SPRITE
//!
//! ImageMultiFrameHeader (at frame_offset[i]):
//! +0x00  i16 width, i16 height, i16 hotx, i16 hoty
//! +0x08  u32 row_offset[height]          — row offsets FROM THE START OF THE SPRITE
//!
//! sprite row: {u8 skip, u8 count, u8 pixels[count]}*, 0xFF — end of row
//! shadow row:    {u8 skip, u8 count}*,                     0xFF — end of row
//! ```
//!
//! Key properties of the format reproduced here (§4, §9):
//! * transparency is **structural**: skipped pixels are absent from the file,
//!   so [`SpriteFrame::pixels`] stores `None` in their place;
//! * the sum of `skip + count` over the runs of a row may be less than `width` —
//!   the tail of the row is transparent and not recorded in the file (`TANK` frame 0: 39 of 44, §4.2);
//! * the `0xFF` byte is a row terminator only in the position of the `skip` counter; inside
//!   the pixel field it is allowed and is read as an ordinary palette index (§4.1);
//! * a run with `count == 0` is valid ("skip only");
//! * in shadow frames there are no pixel bytes at all: a run is a pair of counters
//!   `{skip, count}`, so the shadow decodes into a darkening mask (§4.3);
//! * sprites have neither a signature nor flags — "shadow or not" is determined
//!   only by the resource tag, so the mode is set by the [`decode`] parameter.
//!
//! The decoder does not panic on corrupt data: structural errors are returned as
//! [`SpriteError`]; reading past the resource boundary is impossible (§9.13).

use std::fmt;

/// Sprite header size: `u16 image_count`.
pub const HEADER_SIZE: usize = 2;
/// Frame header size: 4 × `i16` (width, height, hotx, hoty).
pub const FRAME_HEADER_SIZE: usize = 8;
/// Row terminator (`row_delimiter` in `Gfx_DecodeSprite`).
pub const ROW_DELIMITER: u8 = 0xFF;
/// How many colors are in the palette (a pixel index is `u8`).
pub const PALETTE_SIZE: usize = 256;
/// Upper bound on the pixels of one frame: protection against OOM on a corrupt header.
pub const MAX_FRAME_PIXELS: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Sprite body parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpriteError {
    /// The body is shorter than the two-byte `image_count`.
    TooShort {
        /// Actual length of the resource body.
        len: usize,
        /// Minimum required length.
        needed: usize,
    },
    /// `image_count == 0` — the sprite has no frames at all (§9.13).
    ZeroImages,
    /// The frame offset table does not fit into the resource body.
    OffsetTableTruncated {
        /// `image_count` from the header.
        image_count: u16,
        /// Required end of the table (in bytes from the start of the sprite).
        needed: usize,
        /// Actual body length.
        len: usize,
    },
    /// `frame_offset[i]` points beyond the resource body.
    FrameOutOfBounds {
        /// Frame index.
        frame: usize,
        /// Frame offset from the table.
        offset: u32,
        /// Actual body length.
        len: usize,
    },
    /// The frame header or the row offset table does not fit into the body.
    FrameTruncated {
        /// Frame index.
        frame: usize,
        /// Frame offset.
        offset: u32,
        /// Required end of the frame header together with the row table.
        needed: usize,
        /// Actual body length.
        len: usize,
    },
    /// The frame dimensions are not positive (`width <= 0` or `height <= 0`).
    BadDimensions {
        /// Frame index.
        frame: usize,
        /// `width` from the frame header.
        width: i16,
        /// `height` from the frame header.
        height: i16,
    },
    /// The frame requires more pixels than [`MAX_FRAME_PIXELS`] — the header is corrupt.
    FrameTooLarge {
        /// Frame index.
        frame: usize,
        /// `width` from the frame header.
        width: u16,
        /// `height` from the frame header.
        height: u16,
    },
    /// `row_offset[y]` points beyond the resource body.
    RowOutOfBounds {
        /// Frame index.
        frame: usize,
        /// Row index.
        row: usize,
        /// Row offset from the table.
        offset: u32,
        /// Actual body length.
        len: usize,
    },
    /// The row did not end with the `0xFF` terminator before the end of the resource body.
    RowUnterminated {
        /// Frame index.
        frame: usize,
        /// Row index.
        row: usize,
        /// Row offset.
        offset: u32,
        /// Actual body length.
        len: usize,
    },
    /// The counter pair `{skip, count}` or the terminator byte is cut off by the end of the body.
    RunHeaderTruncated {
        /// Frame index.
        frame: usize,
        /// Row index.
        row: usize,
        /// Read offset.
        offset: usize,
        /// Actual body length.
        len: usize,
    },
    /// The pixel field of a run is cut off by the end of the resource body.
    PixelPayloadTruncated {
        /// Frame index.
        frame: usize,
        /// Row index.
        row: usize,
        /// Read offset.
        offset: usize,
        /// Actual body length.
        len: usize,
    },
    /// The sum of `skip + count` over the runs of a row exceeds `width` (§4.2).
    RowOverrun {
        /// Frame index.
        frame: usize,
        /// Row index.
        row: usize,
        /// Accumulated x position after the run.
        x: u32,
        /// `width` of the frame.
        width: u16,
    },
}

impl fmt::Display for SpriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, needed } => write!(
                f,
                "спрайт слишком короткий: {len} байт (нужно минимум {needed})"
            ),
            Self::ZeroImages => write!(f, "image_count = 0: в спрайте нет кадров"),
            Self::OffsetTableTruncated {
                image_count,
                needed,
                len,
            } => write!(
                f,
                "таблица смещений кадров ({image_count} записей) требует {needed} байт, тело {len}"
            ),
            Self::FrameOutOfBounds { frame, offset, len } => write!(
                f,
                "кадр {frame}: смещение 0x{offset:X} за концом спрайта (0x{len:X})"
            ),
            Self::FrameTruncated {
                frame,
                offset,
                needed,
                len,
            } => write!(
                f,
                "кадр {frame} (0x{offset:X}): заголовок со строками требует {needed} байт, тело {len}"
            ),
            Self::BadDimensions {
                frame,
                width,
                height,
            } => write!(
                f,
                "кадр {frame}: неположительные размеры {width}×{height}"
            ),
            Self::FrameTooLarge {
                frame,
                width,
                height,
            } => write!(
                f,
                "кадр {frame}: размеры {width}×{height} превышают предел в {MAX_FRAME_PIXELS} пикселей"
            ),
            Self::RowOutOfBounds {
                frame,
                row,
                offset,
                len,
            } => write!(
                f,
                "кадр {frame} строка {row}: смещение 0x{offset:X} за концом спрайта (0x{len:X})"
            ),
            Self::RowUnterminated {
                frame,
                row,
                offset,
                len,
            } => write!(
                f,
                "кадр {frame} строка {row} (0x{offset:X}): нет терминатора 0xFF до конца тела (0x{len:X})"
            ),
            Self::RunHeaderTruncated {
                frame,
                row,
                offset,
                len,
            } => write!(
                f,
                "кадр {frame} строка {row}: пара счётчиков на 0x{offset:X} обрывается концом тела (0x{len:X})"
            ),
            Self::PixelPayloadTruncated {
                frame,
                row,
                offset,
                len,
            } => write!(
                f,
                "кадр {frame} строка {row}: пиксели прогона на 0x{offset:X} обрываются концом тела (0x{len:X})"
            ),
            Self::RowOverrun {
                frame,
                row,
                x,
                width,
            } => write!(
                f,
                "кадр {frame} строка {row}: прогоны выходят за width={width} (x={x})"
            ),
        }
    }
}

impl std::error::Error for SpriteError {}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// One row run as recorded in the file.
///
/// For an ordinary sprite `pixels.len() == count`; for a shadow row `pixels` is empty:
/// there are no pixel bytes after the counter pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteRun {
    /// `u8 skip` — how many transparent pixels to skip.
    pub skip: u8,
    /// `u8 count` — how many pixels (for a shadow, darkened ones) follow.
    pub count: u8,
    /// Palette indices of the run; empty for shadows.
    pub pixels: Vec<u8>,
}

impl SpriteRun {
    /// The run contains no pixel bytes (a shadow row).
    #[inline]
    pub fn is_shadow(&self) -> bool {
        self.pixels.is_empty()
    }

    /// How many pixels of the row the run covers (`skip + count`).
    #[inline]
    pub fn span(&self) -> u32 {
        u32::from(self.skip) + u32::from(self.count)
    }
}

/// One frame row: a sequence of runs up to the `0xFF` terminator.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpriteRow {
    /// Runs in file order.
    pub runs: Vec<SpriteRun>,
    /// Sum of `skip + count` over the runs; may be less than `width` (§4.2).
    pub painted: u32,
}

impl SpriteRow {
    /// How many runs have a non-empty pixel field.
    pub fn painted_runs(&self) -> usize {
        self.runs.iter().filter(|run| run.count > 0).count()
    }
}

/// Frame of an ordinary (non-shadow) sprite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteFrame {
    /// Frame width in pixels.
    pub width: u16,
    /// Frame height in pixels.
    pub height: u16,
    /// Hot spot (anchor) along x.
    pub hotx: i16,
    /// Hot spot (anchor) along y.
    pub hoty: i16,
    /// Parsed rows — as in the file (no data is lost).
    pub rows: Vec<SpriteRow>,
    /// Frame pixels `width * height`, row-major: `None` is transparency.
    pub pixels: Vec<Option<u8>>,
}

impl SpriteFrame {
    /// Pixel by frame coordinates; `None` outside the boundary and on transparency.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= u32::from(self.width) || y >= u32::from(self.height) {
            return None;
        }

        self.pixels[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Frame row by index.
    #[inline]
    pub fn row(&self, y: usize) -> Option<&SpriteRow> {
        self.rows.get(y)
    }

    /// Number of opaque pixels in the frame.
    pub fn opaque_pixels(&self) -> usize {
        self.pixels.iter().filter(|pixel| pixel.is_some()).count()
    }

    /// Frame rectangle taking the hot spot into account in "anchor" coordinates:
    /// `ulx = point.x - hotx`, `uly = point.y - hoty` (§5.1 with `divisor = 2`).
    pub fn bounds_for_anchor(&self, x: i32, y: i32) -> (i32, i32, i32, i32) {
        let ulx = x - i32::from(self.hotx);
        let uly = y - i32::from(self.hoty);

        (
            ulx,
            uly,
            ulx + i32::from(self.width),
            uly + i32::from(self.height),
        )
    }
}

/// Multi-frame sprite: `u16 image_count` + frames in sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteSheet {
    /// `image_count` from the header.
    pub image_count: u16,
    /// Frames in `frame_offset[]` order (`len() == image_count`).
    pub frames: Vec<SpriteFrame>,
}

impl SpriteSheet {
    /// Frame by index (`GetSpriteFrame`).
    #[inline]
    pub fn frame(&self, index: usize) -> Option<&SpriteFrame> {
        self.frames.get(index)
    }

    /// Number of frames (matches `image_count`).
    #[inline]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// No frames (impossible for a successfully parsed sprite).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Frame by index with wrap-around modulo 8 — the unit frame layout is
    /// "phase × 8 + angle" (§5.3): `image_base + phase * 8 + angle`.
    ///
    /// Returns `None` if the resulting index is outside `image_count`.
    #[inline]
    pub fn directional_frame(
        &self,
        image_base: usize,
        phase: usize,
        angle: usize,
    ) -> Option<&SpriteFrame> {
        self.frames.get(image_base + phase * 8 + angle)
    }
}

/// Unit frame header — the `D_*` resource (`Unit::GetFrameInfo`,
/// `max-cpp-port/src/unit.cpp:51-76`).
///
/// This is **not** a sprite but an 8-byte (or 24-byte for turrets) block of
/// metadata: bases and counts of hull, turret, firing and connector frames.
/// For units with `TURRET_SPRITE`/`SPINNING_TURRET` there follow 8 pairs
/// `(dx, dy): i8` — `angle_offsets[body_angle]`, by how many pixels to shift
/// the turret relative to the anchor of the hull.
///
/// C++ reads the counters as `int8_t` (`D_INFANT[1] = 0xC8` → -56); here
/// the counters are `u8`, because negative frame counts do not exist and
/// the value is used only to choose a frame (200 for infantry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameInfo {
    /// Hull frame base (`image_base`).
    pub image_base: u8,
    /// Number of hull frames (`image_count`).
    pub image_count: u8,
    /// Turret frame base (`turret_image_base`).
    pub turret_image_base: u8,
    /// Number of turret frames (`turret_image_count`).
    pub turret_image_count: u8,
    /// Firing frame base (`firing_image_base`).
    pub firing_image_base: u8,
    /// Number of firing frames (`firing_image_count`).
    pub firing_image_count: u8,
    /// Connector frame base (`connector_image_base`).
    pub connector_image_base: u8,
    /// Number of connector frames (`connector_image_count`).
    pub connector_image_count: u8,
    /// Turret offset for each of the 8 hull angles; zeros if the block is shorter.
    pub angle_offsets: [(i8, i8); 8],
}

impl FrameInfo {
    /// Size of the minimal block (8 counters).
    pub const HEADER_SIZE: usize = 8;
    /// Size of the block with `angle_offsets` (8 counters + 8 pairs).
    pub const TURRET_SIZE: usize = 24;

    /// Parses the `D_*` resource; `None` if it is shorter than 8 bytes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::HEADER_SIZE {
            return None;
        }

        let mut angle_offsets = [(0i8, 0i8); 8];

        if bytes.len() >= Self::TURRET_SIZE {
            for (index, offset) in angle_offsets.iter_mut().enumerate() {
                offset.0 = bytes[Self::HEADER_SIZE + index * 2] as i8;
                offset.1 = bytes[Self::HEADER_SIZE + index * 2 + 1] as i8;
            }
        }

        Some(Self {
            image_base: bytes[0],
            image_count: bytes[1],
            turret_image_base: bytes[2],
            turret_image_count: bytes[3],
            firing_image_base: bytes[4],
            firing_image_count: bytes[5],
            connector_image_base: bytes[6],
            connector_image_count: bytes[7],
            angle_offsets,
        })
    }

    /// `angle_offsets[body_angle]` — where to draw the turret (`angle` 0..7).
    pub fn turret_offset(&self, body_angle: u8) -> (i32, i32) {
        let (x, y) = self.angle_offsets[usize::from(body_angle & 7)];

        (i32::from(x), i32::from(y))
    }

    /// Turret frame index: `turret_image_base + turret_angle` (0..7).
    pub fn turret_frame_index(&self, turret_angle: u8) -> usize {
        usize::from(self.turret_image_base) + usize::from(turret_angle & 7)
    }
}

/// Shadow frame: a darkening mask instead of pixels (§4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowFrame {
    /// Frame width in pixels.
    pub width: u16,
    /// Frame height in pixels.
    pub height: u16,
    /// Hot spot (anchor) along x.
    pub hotx: i16,
    /// Hot spot (anchor) along y.
    pub hoty: i16,
    /// Parsed rows: `{skip, count}` pairs without pixel bytes.
    pub rows: Vec<SpriteRow>,
    /// Mask `width * height`, row-major: `true` — darken the map pixel.
    pub mask: Vec<bool>,
}

impl ShadowFrame {
    /// Whether the frame pixel is darkened by coordinates.
    #[inline]
    pub fn darkened(&self, x: u32, y: u32) -> bool {
        if x >= u32::from(self.width) || y >= u32::from(self.height) {
            return false;
        }

        self.mask[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Number of darkened pixels in the frame.
    pub fn darkened_pixels(&self) -> usize {
        self.mask.iter().filter(|&&cell| cell).count()
    }

    /// Shadow rectangle taking the hot spot into account (like [`SpriteFrame::bounds_for_anchor`]).
    pub fn bounds_for_anchor(&self, x: i32, y: i32) -> (i32, i32, i32, i32) {
        let ulx = x - i32::from(self.hotx);
        let uly = y - i32::from(self.hoty);

        (
            ulx,
            uly,
            ulx + i32::from(self.width),
            uly + i32::from(self.height),
        )
    }
}

/// Multi-frame shadow (`S_*` resource).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowSheet {
    /// `image_count` from the header.
    pub image_count: u16,
    /// Frames in `frame_offset[]` order.
    pub frames: Vec<ShadowFrame>,
}

impl ShadowSheet {
    /// Frame by index.
    #[inline]
    pub fn frame(&self, index: usize) -> Option<&ShadowFrame> {
        self.frames.get(index)
    }

    /// Number of frames.
    #[inline]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// No frames (impossible for a successfully parsed shadow).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// Result of [`decode`]: a sprite or a shadow — the mode is set by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedSprite {
    /// Ordinary sprite: palette index pixels.
    Sprite(SpriteSheet),
    /// Shadow: a darkening mask.
    Shadow(ShadowSheet),
}

impl DecodedSprite {
    /// Sprite, if the body was decoded in ordinary mode.
    pub fn as_sprite(&self) -> Option<&SpriteSheet> {
        match self {
            Self::Sprite(sheet) => Some(sheet),
            Self::Shadow(_) => None,
        }
    }

    /// Shadow, if the body was decoded in shadow mode.
    pub fn as_shadow(&self) -> Option<&ShadowSheet> {
        match self {
            Self::Sprite(_) => None,
            Self::Shadow(sheet) => Some(sheet),
        }
    }

    /// The body was decoded as a shadow.
    pub fn is_shadow(&self) -> bool {
        matches!(self, Self::Shadow(_))
    }
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

/// Decodes the sprite body. `shadow = true` reads rows as `{skip, count}` pairs
/// without pixel bytes (tags `S_*`, §4.3).
///
/// The mode cannot be determined from the contents (§3.4), so it comes from outside.
pub fn decode(bytes: &[u8], shadow: bool) -> Result<DecodedSprite, SpriteError> {
    if bytes.len() < HEADER_SIZE {
        return Err(SpriteError::TooShort {
            len: bytes.len(),
            needed: HEADER_SIZE,
        });
    }

    let image_count = u16::from_le_bytes([bytes[0], bytes[1]]);

    if image_count == 0 {
        return Err(SpriteError::ZeroImages);
    }

    let table_end = HEADER_SIZE + 4 * image_count as usize;

    if bytes.len() < table_end {
        return Err(SpriteError::OffsetTableTruncated {
            image_count,
            needed: table_end,
            len: bytes.len(),
        });
    }

    let frames = if shadow {
        let mut frames = Vec::with_capacity(image_count as usize);

        for index in 0..image_count as usize {
            frames.push(decode_frame(
                bytes,
                index,
                frame_offset(bytes, index),
                true,
            )?);
        }

        DecodedSprite::Shadow(ShadowSheet {
            image_count,
            frames: frames
                .into_iter()
                .map(ShadowFrame::from_raw)
                .collect::<Vec<ShadowFrame>>(),
        })
    } else {
        let mut frames = Vec::with_capacity(image_count as usize);

        for index in 0..image_count as usize {
            frames.push(sprite_frame_from_raw(decode_frame(
                bytes,
                index,
                frame_offset(bytes, index),
                false,
            )?));
        }

        DecodedSprite::Sprite(SpriteSheet {
            image_count,
            frames,
        })
    };

    Ok(frames)
}

/// Decodes the body of an ordinary sprite (tags `COMMTWR`…`WALDO`).
pub fn decode_sprite(bytes: &[u8]) -> Result<SpriteSheet, SpriteError> {
    match decode(bytes, false)? {
        DecodedSprite::Sprite(sheet) => Ok(sheet),
        DecodedSprite::Shadow(_) => unreachable!("decode(false) всегда даёт Sprite"),
    }
}

/// Decodes the body of a shadow (tags `S_*`).
pub fn decode_shadow(bytes: &[u8]) -> Result<ShadowSheet, SpriteError> {
    match decode(bytes, true)? {
        DecodedSprite::Shadow(sheet) => Ok(sheet),
        DecodedSprite::Sprite(_) => unreachable!("decode(true) всегда даёт Shadow"),
    }
}

#[inline]
fn frame_offset(bytes: &[u8], index: usize) -> u32 {
    let pos = HEADER_SIZE + 4 * index;

    u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
}

/// Raw (not yet laid out into `pixels`/`mask`) frame.
struct RawFrame {
    width: u16,
    height: u16,
    hotx: i16,
    hoty: i16,
    rows: Vec<SpriteRow>,
}

impl ShadowFrame {
    fn from_raw(raw: RawFrame) -> Self {
        let width = raw.width as usize;
        let height = raw.height as usize;
        let mut mask = vec![false; width * height];

        for (y, row) in raw.rows.iter().enumerate() {
            let mut x = 0usize;

            for run in &row.runs {
                x += usize::from(run.skip);

                for i in 0..usize::from(run.count) {
                    let index = y * width + x + i;

                    if let Some(cell) = mask.get_mut(index) {
                        *cell = true;
                    }
                }

                x += usize::from(run.count);
            }
        }

        Self {
            width: raw.width,
            height: raw.height,
            hotx: raw.hotx,
            hoty: raw.hoty,
            rows: raw.rows,
            mask,
        }
    }
}

fn sprite_frame_from_raw(raw: RawFrame) -> SpriteFrame {
    let width = raw.width as usize;
    let height = raw.height as usize;
    let mut pixels = vec![None; width * height];

    for (y, row) in raw.rows.iter().enumerate() {
        let mut x = 0usize;

        for run in &row.runs {
            x += usize::from(run.skip);

            for &pixel in &run.pixels {
                let index = y * width + x;

                if let Some(cell) = pixels.get_mut(index) {
                    *cell = Some(pixel);
                }

                x += 1;
            }
        }
    }

    SpriteFrame {
        width: raw.width,
        height: raw.height,
        hotx: raw.hotx,
        hoty: raw.hoty,
        rows: raw.rows,
        pixels,
    }
}

/// Parses one frame: header, row table and the rows themselves.
fn decode_frame(
    bytes: &[u8],
    index: usize,
    offset: u32,
    shadow: bool,
) -> Result<RawFrame, SpriteError> {
    let start = offset as usize;

    if start + FRAME_HEADER_SIZE > bytes.len() {
        return Err(SpriteError::FrameOutOfBounds {
            frame: index,
            offset,
            len: bytes.len(),
        });
    }

    let width = i16::from_le_bytes([bytes[start], bytes[start + 1]]);
    let height = i16::from_le_bytes([bytes[start + 2], bytes[start + 3]]);
    let hotx = i16::from_le_bytes([bytes[start + 4], bytes[start + 5]]);
    let hoty = i16::from_le_bytes([bytes[start + 6], bytes[start + 7]]);

    if width <= 0 || height <= 0 {
        return Err(SpriteError::BadDimensions {
            frame: index,
            width,
            height,
        });
    }

    let width = width as u16;
    let height = height as u16;
    let pixels = usize::from(width) * usize::from(height);

    if pixels > MAX_FRAME_PIXELS {
        return Err(SpriteError::FrameTooLarge {
            frame: index,
            width,
            height,
        });
    }

    let header_end = start + FRAME_HEADER_SIZE + 4 * height as usize;

    if header_end > bytes.len() {
        return Err(SpriteError::FrameTruncated {
            frame: index,
            offset,
            needed: header_end,
            len: bytes.len(),
        });
    }

    let mut rows = Vec::with_capacity(height as usize);

    for y in 0..height as usize {
        let pos = start + FRAME_HEADER_SIZE + 4 * y;
        let row_offset =
            u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);

        if row_offset as usize >= bytes.len() {
            return Err(SpriteError::RowOutOfBounds {
                frame: index,
                row: y,
                offset: row_offset,
                len: bytes.len(),
            });
        }

        rows.push(decode_row(bytes, index, y, row_offset, width, shadow)?);
    }

    Ok(RawFrame {
        width,
        height,
        hotx,
        hoty,
        rows,
    })
}

/// Parses a row up to the `0xFF` terminator (§4.1 / §4.3).
fn decode_row(
    bytes: &[u8],
    frame: usize,
    row: usize,
    row_offset: u32,
    width: u16,
    shadow: bool,
) -> Result<SpriteRow, SpriteError> {
    let len = bytes.len();
    let mut pos = row_offset as usize;
    let mut x = 0u32;
    let mut runs = Vec::new();

    loop {
        if pos >= len {
            return Err(SpriteError::RowUnterminated {
                frame,
                row,
                offset: row_offset,
                len,
            });
        }

        if bytes[pos] == ROW_DELIMITER {
            break;
        }

        if pos + 1 >= len {
            return Err(SpriteError::RunHeaderTruncated {
                frame,
                row,
                offset: pos,
                len,
            });
        }

        let skip = bytes[pos];
        let count = bytes[pos + 1];

        pos += 2;
        x += u32::from(skip);

        let pixels = if shadow {
            Vec::new()
        } else {
            let end = pos + usize::from(count);

            if end > len {
                return Err(SpriteError::PixelPayloadTruncated {
                    frame,
                    row,
                    offset: pos,
                    len,
                });
            }

            let pixels = bytes[pos..end].to_vec();

            pos = end;

            pixels
        };

        x += u32::from(count);

        if x > u32::from(width) {
            return Err(SpriteError::RowOverrun {
                frame,
                row,
                x,
                width,
            });
        }

        runs.push(SpriteRun {
            skip,
            count,
            pixels,
        });
    }

    Ok(SpriteRow { runs, painted: x })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a sprite body from frames of the form `(width, height, hotx, hoty, rows)`,
    /// where `rows` are already encoded rows (bytes together with the terminator).
    fn build_sprite(frames: &[(u16, u16, i16, i16, Vec<Vec<u8>>)]) -> Vec<u8> {
        let image_count = frames.len() as u16;
        let mut out = Vec::new();

        out.extend_from_slice(&image_count.to_le_bytes());
        // Frame offset stubs, filled in below.
        out.extend(std::iter::repeat_n(0u8, 4 * frames.len()));

        for (index, (width, height, hotx, hoty, rows)) in frames.iter().enumerate() {
            assert_eq!(rows.len(), *height as usize);

            let frame_offset = out.len() as u32;
            out[HEADER_SIZE + 4 * index..HEADER_SIZE + 4 * index + 4]
                .copy_from_slice(&frame_offset.to_le_bytes());

            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.extend_from_slice(&hotx.to_le_bytes());
            out.extend_from_slice(&hoty.to_le_bytes());

            let row_table_start = out.len();
            out.extend(std::iter::repeat_n(0u8, 4 * rows.len()));

            for (y, row) in rows.iter().enumerate() {
                let row_offset = out.len() as u32;
                out[row_table_start + 4 * y..row_table_start + 4 * y + 4]
                    .copy_from_slice(&row_offset.to_le_bytes());
                out.extend_from_slice(row);
            }
        }

        out
    }

    /// `{skip, count, pixels[]}`.
    fn run(skip: u8, pixels: &[u8]) -> Vec<u8> {
        let mut out = vec![skip, pixels.len() as u8];

        out.extend_from_slice(pixels);
        out
    }

    #[test]
    fn decodes_rows_with_transparent_tail_and_hot_spot() {
        // width = 6, row = {skip 1, count 2, [A, B]}, {skip 1, count 2, [C, D]} -> x = 6? no: 1+2+1+2 = 6.
        let mut row = run(1, &[0xAA, 0xBB]);

        row.extend(run(1, &[0xCC, 0xDD]));
        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(6, 1, 3, 4, vec![row])]);
        let sheet = decode_sprite(&bytes).unwrap();

        assert_eq!(sheet.image_count, 1);
        assert_eq!(sheet.len(), 1);

        let frame = sheet.frame(0).unwrap();

        assert_eq!(
            (frame.width, frame.height, frame.hotx, frame.hoty),
            (6, 1, 3, 4)
        );
        assert_eq!(
            frame.pixels,
            vec![None, Some(0xAA), Some(0xBB), None, Some(0xCC), Some(0xDD)]
        );
        assert_eq!(frame.opaque_pixels(), 4);
        assert_eq!(frame.rows[0].painted, 6);
        // height = 1, hot = (3, 4), point (10, 20) -> frame (7, 16)..(13, 17).
        assert_eq!(frame.bounds_for_anchor(10, 20), (7, 16, 13, 17));
    }

    #[test]
    fn tail_shorter_than_width_is_transparent() {
        // A real edge case from TANK §3.1: the sum of the counters is less than width.
        let mut row = run(1, &[0x11]);

        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(5, 1, 0, 0, vec![row])]);
        let frame = &decode_sprite(&bytes).unwrap().frames[0];

        assert_eq!(frame.pixels, vec![None, Some(0x11), None, None, None]);
        assert_eq!(frame.rows[0].painted, 2);
    }

    #[test]
    fn ff_inside_pixel_payload_is_a_palette_index() {
        // 0xFF is allowed inside the pixel field: the decoder reads exactly count bytes.
        let mut row = run(0, &[ROW_DELIMITER, 0x00, ROW_DELIMITER]);

        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(3, 1, 0, 0, vec![row])]);
        let frame = &decode_sprite(&bytes).unwrap().frames[0];

        assert_eq!(frame.pixels, vec![Some(0xFF), Some(0x00), Some(0xFF)]);
    }

    #[test]
    fn zero_count_run_only_skips() {
        let mut row = run(2, &[]);

        row.extend(run(1, &[0x42]));
        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(4, 1, 0, 0, vec![row])]);
        let frame = &decode_sprite(&bytes).unwrap().frames[0];

        assert_eq!(frame.pixels, vec![None, None, None, Some(0x42)]);
        assert_eq!(frame.rows[0].runs.len(), 2);
        assert_eq!(frame.rows[0].runs[0].span(), 2);
    }

    #[test]
    fn shadow_rows_are_pairs_of_counters() {
        // S_TANK §3.2: {4, 5}, {25, 5}, 0xFF at width = 49.
        let mut row = vec![4, 5, 25, 5];

        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(49, 1, 20, 23, vec![row])]);
        let sheet = decode_shadow(&bytes).unwrap();
        let frame = sheet.frame(0).unwrap();

        assert_eq!(frame.width, 49);
        assert_eq!((frame.hotx, frame.hoty), (20, 23));
        assert_eq!(frame.darkened_pixels(), 10);
        // {4, 5}: x = 4..=8; {25, 5}: x = 4 + 5 + 25 = 34..=38.
        assert!(frame.darkened(4, 0));
        assert!(frame.darkened(8, 0));
        assert!(frame.darkened(34, 0));
        assert!(frame.darkened(38, 0));
        assert!(!frame.darkened(3, 0));
        assert!(!frame.darkened(9, 0));
        assert!(!frame.darkened(33, 0));
        assert!(!frame.darkened(39, 0));
        assert!(!frame.darkened(48, 0));
        assert!(frame.rows[0].runs.iter().all(SpriteRun::is_shadow));
        assert!(frame.rows[0].runs.iter().all(|run| run.pixels.is_empty()));
    }

    #[test]
    fn multi_frame_offsets_are_absolute() {
        let mut first = run(0, &[1, 2]);

        first.push(ROW_DELIMITER);

        let mut second = run(0, &[3]);

        second.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(2, 1, 0, 0, vec![first]), (1, 1, 7, 8, vec![second])]);
        let sheet = decode_sprite(&bytes).unwrap();

        assert_eq!(sheet.len(), 2);
        assert_eq!(sheet.frames[0].pixels, vec![Some(1), Some(2)]);
        assert_eq!(sheet.frames[1].pixels, vec![Some(3)]);
        assert_eq!((sheet.frames[1].hotx, sheet.frames[1].hoty), (7, 8));

        // Unit frames are laid out as "phase × 8 + angle" (§5.3).
        assert!(sheet.directional_frame(0, 0, 0).is_some());
        assert!(sheet.directional_frame(0, 1, 0).is_none());
    }

    #[test]
    fn decode_returns_the_requested_mode() {
        let mut row = run(0, &[9, 9]);

        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(2, 1, 0, 0, vec![row])]);

        assert!(decode(&bytes, false).unwrap().as_sprite().is_some());
        assert!(!decode(&bytes, false).unwrap().is_shadow());
        assert_eq!(
            decode(&bytes, false)
                .unwrap()
                .as_sprite()
                .unwrap()
                .image_count,
            1
        );

        // The same width, but a shadow row: {0, 2} pairs without pixel bytes.
        let mut shadow_row = vec![0, 2];

        shadow_row.push(ROW_DELIMITER);

        let shadow_bytes = build_sprite(&[(2, 1, 0, 0, vec![shadow_row])]);
        let shadow = decode(&shadow_bytes, true).unwrap();

        assert!(shadow.is_shadow());
        assert!(shadow.as_sprite().is_none());
        assert_eq!(
            shadow
                .as_shadow()
                .unwrap()
                .frame(0)
                .unwrap()
                .darkened_pixels(),
            2
        );
    }

    #[test]
    fn structural_errors_do_not_panic() {
        assert!(matches!(
            decode_sprite(b""),
            Err(SpriteError::TooShort { .. })
        ));
        assert!(matches!(
            decode_sprite(&0u16.to_le_bytes()),
            Err(SpriteError::ZeroImages)
        ));

        // image_count = 5, but the table is truncated.
        let mut truncated = vec![5, 0, 0, 0];

        assert!(matches!(
            decode_sprite(&truncated),
            Err(SpriteError::OffsetTableTruncated { .. })
        ));

        // A frame beyond the end of the body.
        truncated = vec![1, 0, 0xFF, 0x00, 0x00, 0x00];

        assert!(matches!(
            decode_sprite(&truncated),
            Err(SpriteError::FrameOutOfBounds { .. })
        ));

        // Zero frame width.
        let zero_width = build_sprite(&[(0, 1, 0, 0, vec![vec![ROW_DELIMITER]])]);

        assert!(matches!(
            decode_sprite(&zero_width),
            Err(SpriteError::BadDimensions { .. })
        ));

        // A row without a terminator: it reaches the end of the body.
        // height = 1, run {skip 0, count 1, 0xAA} and no 0xFF at the end.
        let broken = build_sprite(&[(1, 1, 0, 0, vec![vec![0u8, 1, 0xAA]])]);

        assert!(matches!(
            decode_sprite(&broken),
            Err(SpriteError::RowUnterminated { .. })
        ));

        // The row goes beyond width.
        let mut overrun = run(3, &[1, 2, 3]);

        overrun.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(5, 1, 0, 0, vec![overrun])]);

        assert!(matches!(
            decode_sprite(&bytes),
            Err(SpriteError::RowOverrun { .. })
        ));

        // A truncated pixel field.
        let mut clipped = vec![0u8, 5, 1, 2]; // count = 5, but only 2 bytes

        clipped.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(5, 1, 0, 0, vec![clipped])]);

        assert!(matches!(
            decode_sprite(&bytes),
            Err(SpriteError::PixelPayloadTruncated { .. })
        ));
    }

    /// `D_TANK` from `MAX.RES` — 24 bytes: hull 0..7, turret 8..15,
    /// firing 16..23 and `angle_offsets` (the same numbers C++ reads).
    #[test]
    fn frame_info_decodes_tank_data() {
        let bytes = [
            0u8, 8, 8, 8, 16, 8, 0, 0, // counters
            0, 7, 0xFF, 7, 0xF8, 0, 0xF8, 0xF9, // (0,7) (-7,7) (-8,0) (-8,-7)
            0, 0xF8, 6, 0xF8, 7, 0, 7, 6, // (0,-8) (6,-8) (7,0) (7,6)
        ];
        let info = FrameInfo::decode(&bytes).expect("D_TANK");

        assert_eq!(info.image_base, 0);
        assert_eq!(info.image_count, 8);
        assert_eq!(info.turret_image_base, 8);
        assert_eq!(info.turret_image_count, 8);
        assert_eq!(info.firing_image_base, 16);
        assert_eq!(info.firing_image_count, 8);
        assert_eq!(info.turret_frame_index(0), 8);
        assert_eq!(info.turret_frame_index(7), 15);
        assert_eq!(info.turret_offset(0), (0, 7));
        assert_eq!(info.turret_offset(2), (-8, 0));
        assert_eq!(info.turret_offset(7), (7, 6));
        // `angle` is masked to 0..7, as in C++ (`image_index & 0x07`).
        assert_eq!(info.turret_offset(10), (-8, 0));
    }

    /// 8-byte resource (`D_FIRING`) — without turret/offsets (`angle_offsets` = 0).
    #[test]
    fn frame_info_without_offsets_is_valid() {
        let info = FrameInfo::decode(&[0, 16, 0, 0, 16, 16, 0, 0]).expect("D_FIRING");

        assert_eq!(info.image_count, 16);
        assert_eq!(info.turret_image_count, 0);
        assert_eq!(info.angle_offsets, [(0, 0); 8]);
        assert_eq!(info.turret_offset(2), (0, 0));
        // Infantry: `image_count` = 200 (0xC8), C++ reads int8 → -56.
        assert_eq!(FrameInfo::decode(&[0, 200, 0, 0, 0, 0, 0, 0]).unwrap().image_count, 200);
        assert_eq!(FrameInfo::decode(&[0, 1, 2]), None, "короче заголовка");
    }
}
