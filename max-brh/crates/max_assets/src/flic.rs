//! FLC/FLI — Autodesk FLIC animations (`*.FLC` next to `MAX.RES`).
//!
//! Oracle — `max-cpp-port/src/flicsmgr.cpp` (decoder `flicsmgr_*`) and
//! `game_manager.cpp::GameManager_MenuCreateFlic`/`AdvanceFlic` (the
//! `WINDOW_CORNER_FLIC` portrait). The format is described in the public Autodesk
//! FLIC specification (`flic.htm`): a 128-byte header, then frames with a 16-byte
//! header, and chunks inside a frame.
//!
//! ```text
//! +0x00 u32 size            — file size (matches in the reference files)
//! +0x04 u16 type            — 0xAF12 (FLC) / 0xAF11 (FLI; not supported)
//! +0x06 u16 frames          — number of frames
//! +0x08 u16 width, +0x0A height, +0x0C depth, +0x0E flags
//! +0x10 u32 speed           — ms per frame (in MAX.RES: 71, for 128×256 — 67)
//! +0x50 u32 oframe1         — offset of the first frame
//! +0x54 u32 oframe2         — offset of the second frame (ring frame)
//! ```
//!
//! Frame: `u32 size` (including the header), `u16 type = 0xF1FA`, `u16 chunks`,
//! 8 bytes of reserve. Chunk: `u32 size` (including the 6-byte header, the size
//! is even), `u16 type`, then the body.
//!
//! | Chunk | ID | What it does |
//! |---|---|---|
//! | `COLOR_256` | 4 | palette: `u16 packets`, packets `u8 skip, u8 count (0→256), count×RGB` |
//! | `DELTA_FLC` | 7 | word-based delta (`FLI_SS2`): rows, optional words, packets `u8 skip, i8 words` |
//! | `COLOR_64` | 11 | palette in 6-bit components |
//! | `DELTA_FLI` | 12 | byte-based delta (`FLI_LC`): `u16 first_line, u16 lines`, packets `u8 skip, i8 count` |
//! | `BLACK` | 13 | frame to black |
//! | `BYTE_RUN` | 15 | full RLE frame: per row `u8 packets`, then `i8 count` + byte/literals |
//! | `LITERAL` | 16 | full frame without compression (`width*height` bytes) |
//! | `PSTAMP` | 18 | "postage stamp" (preview) — ignored in the C++ port |
//!
//! # Differences from the C++ port (and why they are safe)
//!
//! * **Bounds are checked**: C++ reads with `memcpy` without length control, our
//!   decoder returns [`FlicError`] instead of a panic/heap corruption. On all 71
//!   MAX.RES resources both decoders give identical pixels (`flic_vectors`).
//! * **`DELTA_FLI` (12) is implemented**: C++ does not have it, and it does not occur
//!   in the 71 MAX.RES resources — it is needed only for format completeness.
//! * **Palette chunks accumulate in 6-bit form** — like the system palette of the
//!   C++ port: `COLOR_256` bytes are shifted `>> 2`, `COLOR_64` are already 6-bit
//!   (for 8-bit RGB multiply by 4). The palette is not applied to the portrait:
//!   `load_flic_palette = 0` in `GameManager_MenuCreateFlic`, the frame indices go
//!   through the window palette (`FRAMEPIC`). The palette is needed for format completeness and
//!   future screens with `load_flic_palette = 1`.
//! * **`PSTAMP` is skipped** (like `default:` in C++), but its size
//!   is validated.
//!
//! The frames are delta-encoded: frame `N` requires applying frames `0..=N`.
//! [`Flic::decode_frame`] does this from scratch (deterministically, without state),
//! [`FlicPlayer`] — incrementally, like `flicsmgr_advance_animation`.
//!
//! Format errors are returned as values; there are no panics on corrupt data:
//! `#![forbid(unsafe_code)]` in the crate and checks on every read.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// FLIC header size (`sizeof(FlicHeader)` in the C++ port).
pub const FLIC_HEADER_SIZE: usize = 128;
/// Frame header size (`sizeof(FrameHeader)`).
pub const FLIC_FRAME_HEADER_SIZE: usize = 16;
/// Chunk header size (`sizeof(FlicChunkHeader)`).
pub const FLIC_CHUNK_HEADER_SIZE: usize = 6;
/// FLC signature (`FLICSMGR_FLC_TYPE`).
pub const FLC_MAGIC: u16 = 0xAF12;
/// FLI signature (`FLICSMGR_FLI_TYPE`) — the format is not supported.
pub const FLI_MAGIC: u16 = 0xAF11;
/// Frame type (`FLICSMGR_FRAME_TYPE`).
pub const FRAME_MAGIC: u16 = 0xF1FA;
/// Colors in the palette.
pub const FLIC_PALETTE_SIZE: usize = 256;

/// Frame palette: 256 × RGB in 6-bit form (like the system palette of the C++ port;
/// for 8-bit RGB multiply the components by 4).
pub type FlicPalette = [[u8; 3]; FLIC_PALETTE_SIZE];

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// FLC parse/decoding error.
#[derive(Debug)]
pub enum FlicError {
    /// The file cannot be read.
    Io {
        /// The path that was being read.
        path: PathBuf,
        /// Reason from the OS.
        source: io::Error,
    },
    /// Less data than needed for the header.
    TooShort {
        /// Actual length.
        len: usize,
        /// Minimum required length.
        needed: usize,
    },
    /// The signature is not `0xAF12`.
    BadMagic {
        /// The signature found.
        found: u16,
    },
    /// Zero frames in the header — nothing to animate.
    NoFrames,
    /// Zero or unreasonably large frame dimensions.
    BadGeometry {
        /// Width from the header.
        width: u16,
        /// Height from the header.
        height: u16,
    },
    /// A frame with that number does not exist.
    FrameIndex {
        /// The requested number.
        index: usize,
        /// How many frames are in the file.
        frames: usize,
    },
    /// Structural error in a frame.
    Frame {
        /// Frame number.
        index: usize,
        /// Offset of the frame in the file.
        offset: usize,
        /// What exactly is wrong.
        reason: FrameError,
    },
    /// Structural error while reading/decoding.
    Format {
        /// Offset in the file where the error was detected.
        offset: usize,
        /// What was expected.
        what: &'static str,
    },
}

/// What is wrong with the frame.
#[derive(Debug)]
pub enum FrameError {
    /// The frame extends beyond the end of the file.
    Truncated {
        /// Bytes needed.
        needed: usize,
        /// Bytes available.
        available: usize,
    },
    /// The frame type is not `0xF1FA`.
    BadType {
        /// The type found.
        found: u16,
    },
    /// The frame size is smaller than the header.
    BadSize {
        /// The size found.
        size: u32,
    },
    /// The chunk extends beyond the frame bounds or has a zero size.
    BadChunk {
        /// Chunk number in the frame.
        index: usize,
        /// Chunk offset.
        offset: usize,
        /// What is wrong.
        reason: &'static str,
    },
    /// Unknown/unsupported chunk type.
    UnknownChunk {
        /// Chunk number in the frame.
        index: usize,
        /// Chunk type.
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
// Header
// ---------------------------------------------------------------------------

/// FLIC header (128 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlicHeader {
    /// File size from the header.
    pub size: u32,
    /// `0xAF12` (FLC) or `0xAF11` (FLI).
    pub kind: u16,
    /// Number of frames.
    pub frames: u16,
    /// Frame width.
    pub width: u16,
    /// Frame height.
    pub height: u16,
    /// Color depth (8).
    pub depth: u16,
    /// Flags (3 in the MAX resources).
    pub flags: u16,
    /// Frame delay in milliseconds (`speed`).
    pub speed_ms: u32,
    /// Offset of the first frame.
    pub first_frame: u32,
    /// Offset of the second frame (`oframe2`).
    pub second_frame: u32,
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Byte cursor with bounds checks.
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
// Frame canvas
// ---------------------------------------------------------------------------

/// Indexed frame canvas: `width × height` bytes of palette indices.
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

    /// Fills the frame with one index (`BLACK` = 0).
    fn fill(&mut self, value: u8) {
        self.pixels.fill(value);
    }
}

// ---------------------------------------------------------------------------
// Flic
// ---------------------------------------------------------------------------

/// One frame after decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlicFrame {
    /// Width.
    pub width: usize,
    /// Height.
    pub height: usize,
    /// Palette indices, row by row.
    pub pixels: Vec<u8>,
    /// Palette after applying all chunks up to and including this frame.
    pub palette: FlicPalette,
}

impl FlicFrame {
    /// RGBA pixels of the frame via the window system palette (like C++ `win_draw_rect`:
    /// the frame indices are translated by the current screen palette).
    ///
    /// `palette` — 256 colors `[r, g, b, a]`; for the portrait this is the
    /// `FRAMEPIC` palette (`crate::panels::UiPalette`), because C++ draws the FLIC
    /// into the 8-bit window buffer without replacing the system palette.
    pub fn rgba(&self, palette: &[[u8; 4]; FLIC_PALETTE_SIZE]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);

        for &index in &self.pixels {
            out.extend_from_slice(&palette[usize::from(index)]);
        }

        out
    }

    /// Number of pixels.
    #[inline]
    pub fn len(&self) -> usize {
        self.pixels.len()
    }

    /// Empty frame (never occurs in a valid FLC, needed for `clippy`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }
}

/// Parsed FLC file: header, found frames, raw bytes.
#[derive(Debug, Clone)]
pub struct Flic {
    header: FlicHeader,
    frames: Vec<FrameSpan>,
    data: Vec<u8>,
}

/// Frame bounds in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameSpan {
    offset: usize,
    size: usize,
    chunks: u16,
}

impl Flic {
    /// Parses an FLC from bytes: header + frame list (without decoding).
    ///
    /// # Errors
    ///
    /// [`FlicError::BadMagic`] — not `0xAF12` (FLI is not supported, as in C++),
    /// [`FlicError::NoFrames`], [`FlicError::BadGeometry`], as well as structural
    /// frame errors ([`FlicError::Frame`]).
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

    /// Reads and parses an FLC file.
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

    /// Header.
    #[inline]
    pub fn header(&self) -> &FlicHeader {
        &self.header
    }

    /// Frame width.
    #[inline]
    pub fn width(&self) -> usize {
        usize::from(self.header.width)
    }

    /// Frame height.
    #[inline]
    pub fn height(&self) -> usize {
        usize::from(self.header.height)
    }

    /// Number of frames (from the header).
    #[inline]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Frame duration from the header (ms); for the portrait C++ does not use it
    /// (the animation runs at a fixed 15 FPS).
    #[inline]
    pub fn speed_ms(&self) -> u32 {
        self.header.speed_ms
    }

    /// File size from the header.
    #[inline]
    pub fn declared_size(&self) -> u32 {
        self.header.size
    }

    /// Raw bytes of the file.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Animated FLC (`frames > 1`); C++ treats single frames as static.
    #[inline]
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// Frame offsets (for tests/diagnostics): `(offset, size, chunks)`.
    pub fn frame_spans(&self) -> Vec<(usize, usize, u16)> {
        self.frames
            .iter()
            .map(|span| (span.offset, span.size, span.chunks))
            .collect()
    }

    /// Decodes frame `index` from scratch: applies frames `0..=index`.
    ///
    /// # Errors
    ///
    /// [`FlicError::FrameIndex`] — no such frame; [`FlicError::Format`] /
    /// [`FlicError::Frame`] — corrupt data.
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

    /// Decodes all frames in sequence (for tests and animation precomputation).
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

    /// Palette after all frames (6-bit components, like the system palette of the
    /// C++ port).
    pub fn palette(&self) -> Result<FlicPalette, FlicError> {
        let mut canvas = Canvas::new(self.width(), self.height());
        let mut palette: FlicPalette = [[0; 3]; FLIC_PALETTE_SIZE];

        for (index, span) in self.frames.iter().enumerate() {
            self.decode_span(span, index, &mut canvas, &mut palette)?;
        }

        Ok(palette)
    }

    /// Applies the chunks of one frame.
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

    /// Decodes one chunk (see the table in the module documentation).
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
                // PSTAMP: a preview "stamp", C++ does not draw it. The chunk size has already
                // been checked in `decode_span`.
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

    /// Palette chunk: `u16 packets`, packets `u8 skip, u8 count (0→256), RGB`.
    ///
    /// `shift` — shift into the 6-bit system palette of the C++ port (`2` for
    /// `COLOR_256` = 8-bit file bytes, `0` for `COLOR_64`).
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

    /// `BYTE_RUN` (15): a full frame, per row a `u8` (number of packets, not
    /// used), then packets `i8 count`: `>= 0` — repeat the next
    /// byte, `< 0` — `-count` literal bytes.
    fn decode_byte_run(body: &[u8], canvas: &mut Canvas) -> Result<(), FlicError> {
        let mut reader = Reader::new(body, 0);

        for y in 0..canvas.height {
            // The first byte of a row is the packet counter; C++ skips it.
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

    /// `DELTA_FLC` (7), also `FLI_SS2`: rows with word pixels.
    ///
    /// Row format: optional words (`0x8000`), then `packet_count`,
    /// then packets `u8 skip, i8 words`.
    ///
    /// Optional word: if bit `0x4000` is set, it is a negative
    /// number of skipped rows (`-word`), otherwise the low byte is the last
    /// pixel of the current row (exactly as in `flicsmgr_decode_delta_flc`).
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

    /// `DELTA_FLI` (12), also `FLI_LC`: `u16 first_line, u16 lines`, in each
    /// row `u8 packets` and packets `u8 skip, i8 count` (byte-based).
    ///
    /// The C++ port has no such chunk (it does not occur in the 71 MAX.RES resources);
    /// it is implemented per the specification for format completeness.
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
// Player
// ---------------------------------------------------------------------------

/// Incremental frame player — a port of `flicsmgr_advance_animation`:
/// frames are applied one by one, without re-decoding from scratch.
#[derive(Debug, Clone)]
pub struct FlicPlayer<'a> {
    flic: &'a Flic,
    canvas: Canvas,
    palette: FlicPalette,
    frame_index: usize,
}

impl<'a> FlicPlayer<'a> {
    /// Starts at frame 0.
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

    /// Number of the current frame.
    #[inline]
    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    /// Current palette indices.
    #[inline]
    pub fn pixels(&self) -> &[u8] {
        &self.canvas.pixels
    }

    /// Palette after the current frame.
    #[inline]
    pub fn palette(&self) -> &FlicPalette {
        &self.palette
    }

    /// Next frame. After the last one — frame 1 again (like
    /// `flicsmgr_advance_animation` with `animate == 2`); for a static FLC
    /// (`frames == 1`) frame 0 stays.
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

    /// Current frame as a [`FlicFrame`] (for comparison with `decode_frame`).
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "flic_tests.rs"]
mod tests;
