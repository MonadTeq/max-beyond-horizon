//! WRL (World) — planet maps of M.A.X.
//!
//! The format is described in `max-original-dasm/formats/wrl.md`:
//!
//! ```text
//! +0x0000  u8[3]                signature "WRL" (for SNOW_1.WRL — "DMO")
//! +0x0003  u16 LE               layer_count
//! +0x0005  Point[layer_count]   dims: {i16 x; i16 y} (dims[0] is used)
//! +…       u8[W*H]              minimap (palette indices)
//! +…       u16[W*H] LE          tile-id map, row-major
//! +…       u16 LE               tile_count
//! +…       u8[tile_count*4096]  pixels of 64×64 tiles, 1 byte per pixel
//! +…       u8[768]              palette 256 × RGB (8 bits per channel)
//! +…       u8[tile_count]       pass table (0..3)
//! ```
//!
//! There is no compression: the tiles are raw palette indices. The port's loader does **not check**
//! the signature (`SNOW_1.WRL` starts with `"DMO"`), so the `"DMO"` signature
//! is accepted with a warning, and any other one is an error.
//!
//! [`WrlMap::surface_map`] builds a [`SurfaceMap`] by the formula of `world.cpp:382-412`
//! (`formats/wrl.md` §3.1):
//!
//! ```text
//! PassData = {LAND, WATER, COAST, AIR} = {1, 2, 4, 8}
//! surface[i] = PassData[pass_table[tile_ids[i]]]
//! ```
//!
//! As in `max_core::setup::derive_surface_map`, cells with `tile_id >= tile_count`
//! and tiles with `pass_byte > 3` get [`Surface::NONE`] (in C++ this would be
//! a read past the array boundary, §7.6). The port's bug fixes (`ApplyBugFixes`,
//! §3.3) are tied to the SHA-256 of specific files and are not applied here: that is
//! a separate policy layer, not format parsing.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use max_types::enums::Surface;
use max_types::geo::MAP_TILES;
use max_types::map::{MapSize, SurfaceMap};

/// Standard WRL signature (3 bytes, no NUL).
pub const WRL_SIGNATURE: [u8; 3] = *b"WRL";
/// A known exception: the corrupt `SNOW_1.WRL` signature (§1.1).
pub const DMO_SIGNATURE: [u8; 3] = *b"DMO";
/// Signature length.
pub const SIGNATURE_LEN: usize = 3;
/// Tile side (`GFX_MAP_TILE_SIZE`).
pub const TILE_SIZE: usize = 64;
/// Pixels (bytes) in one tile: 64×64.
pub const TILE_PIXELS: usize = TILE_SIZE * TILE_SIZE;
/// Colors in the palette (`PALETTE_SIZE`).
pub const PALETTE_COLORS: usize = 256;
/// Channels per color (RGB).
pub const PALETTE_STRIDE: usize = 3;
/// Bytes in the palette: 256 × 3.
pub const PALETTE_BYTES: usize = PALETTE_COLORS * PALETTE_STRIDE;
/// Pass-table indices -> [`Surface`] (`PassData[]` from `world.cpp:382`).
pub const PASS_TABLE_SURFACES: [Surface; 4] =
    [Surface::LAND, Surface::WATER, Surface::COAST, Surface::AIR];
/// Standard map width.
pub const STANDARD_WIDTH: i16 = 112;
/// Standard map height.
pub const STANDARD_HEIGHT: i16 = 112;
/// Size of the stack array `Point map_layer_dimensions[12]` in the C++ port.
pub const MAX_CPP_LAYERS: u16 = 12;

/// Offsets of the parsed WRL sections (in bytes from the start of the file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WrlOffsets {
    /// Signature (`0`).
    pub signature: usize,
    /// `layer_count`.
    pub layer_count: usize,
    /// The `dims[layer_count]` array.
    pub dims: usize,
    /// Minimap (`W*H` bytes).
    pub minimap: usize,
    /// Tile-id map (`W*H × u16`).
    pub tile_ids: usize,
    /// The `tile_count` field.
    pub tile_count: usize,
    /// Tile pixels (`tile_count × 4096`).
    pub tiles: usize,
    /// Palette (768 bytes).
    pub palette: usize,
    /// Pass table (`tile_count` bytes).
    pub pass_table: usize,
    /// End of the data (`pass_table + tile_count`).
    pub end: usize,
}

/// WRL file parse/read error.
#[derive(Debug)]
pub enum WrlError {
    /// Failed to read the file entirely.
    Io {
        /// The path that was being read.
        path: PathBuf,
        /// Reason from the OS.
        source: io::Error,
    },
    /// The file is shorter than the header (3 + 2 bytes).
    TooShort {
        /// Actual file length.
        len: usize,
        /// Minimum required length.
        needed: usize,
    },
    /// The first 3 bytes are neither `"WRL"` nor the known exception `"DMO"`.
    BadSignature {
        /// Actual signature.
        found: [u8; 3],
    },
    /// `layer_count == 0`: the file has no `dims[0]`, the map is corrupt.
    ZeroLayers,
    /// `dims[0]` contains non-positive dimensions.
    InvalidDimensions {
        /// Width from `dims[0]`.
        width: i16,
        /// Height from `dims[0]`.
        height: i16,
    },
    /// The next section does not fit into the file.
    Truncated {
        /// Name of the section.
        section: &'static str,
        /// Required end of the section (offset from the start of the file).
        needed: usize,
        /// Actual file length.
        file_len: usize,
    },
}

impl fmt::Display for WrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "не удалось прочитать {}: {source}", path.display())
            }
            Self::TooShort { len, needed } => write!(
                f,
                "файл слишком короткий для заголовка WRL: {len} байт (нужно минимум {needed})"
            ),
            Self::BadSignature { found } => write!(
                f,
                "некорректная сигнатура WRL: ожидалось \"WRL\" (допустимо известное исключение \"DMO\"), получено {found:02X?}"
            ),
            Self::ZeroLayers => {
                write!(f, "layer_count = 0: в файле нет dims[0], карта повреждена")
            }
            Self::InvalidDimensions { width, height } => write!(
                f,
                "некорректные размеры слоя 0: ({width}, {height}) — W и H должны быть > 0"
            ),
            Self::Truncated {
                section,
                needed,
                file_len,
            } => write!(
                f,
                "файл обрезан: секция {section} требует минимум 0x{needed:X} байт, файл 0x{file_len:X}"
            ),
        }
    }
}

impl std::error::Error for WrlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Parsed WRL map.
///
/// The file bytes are stored whole: `minimap`/`tile_ids`/`pass_table`/`palette`
/// are parsed into convenient structures, and tile pixels are read as slices from
/// [`WrlMap::tiles`] without copying.
#[derive(Clone)]
pub struct WrlMap {
    data: Vec<u8>,
    signature: [u8; 3],
    layer_count: u16,
    layer_dimensions: Vec<MapSize>,
    width: i16,
    height: i16,
    cells: usize,
    tile_count: u16,
    minimap: Vec<u8>,
    tile_ids: Vec<u16>,
    palette: [u8; PALETTE_BYTES],
    pass_table: Vec<u8>,
    offsets: WrlOffsets,
    warnings: Vec<String>,
}

impl WrlMap {
    /// Parses the map from bytes (copies the slice into ownership).
    pub fn parse(data: &[u8]) -> Result<Self, WrlError> {
        Self::from_vec(data.to_vec())
    }

    /// Parses the map from an existing `Vec<u8>` (without copying the data).
    pub fn from_vec(data: Vec<u8>) -> Result<Self, WrlError> {
        let file_len = data.len();

        if file_len < SIGNATURE_LEN + 2 {
            return Err(WrlError::TooShort {
                len: file_len,
                needed: SIGNATURE_LEN + 2,
            });
        }

        let mut signature = [0u8; SIGNATURE_LEN];
        signature.copy_from_slice(&data[0..SIGNATURE_LEN]);

        if signature != WRL_SIGNATURE && signature != DMO_SIGNATURE {
            return Err(WrlError::BadSignature { found: signature });
        }

        let mut warnings = Vec::new();

        if signature == DMO_SIGNATURE {
            warnings.push(format!(
                "сигнатура \"DMO\" ({DMO_SIGNATURE:02X?}) вместо \"WRL\" — известное исключение SNOW_1.WRL; файл читается как валидный"
            ));
        }

        let layer_count = u16::from_le_bytes([data[3], data[4]]);

        if layer_count == 0 {
            return Err(WrlError::ZeroLayers);
        }

        if layer_count > MAX_CPP_LAYERS {
            warnings.push(format!(
                "layer_count={layer_count} > {MAX_CPP_LAYERS}: C++-порт переполнил бы стековый массив Point[12]; читаем все записи в Vec"
            ));
        }

        let dims_offset = SIGNATURE_LEN + 2;
        let dims_end = dims_offset + 4 * layer_count as usize;

        if file_len < dims_end {
            return Err(WrlError::Truncated {
                section: "таблица размеров слоёв",
                needed: dims_end,
                file_len,
            });
        }

        let mut layer_dimensions = Vec::with_capacity(layer_count as usize);

        for i in 0..layer_count as usize {
            let pos = dims_offset + 4 * i;
            let width = i16::from_le_bytes([data[pos], data[pos + 1]]);
            let height = i16::from_le_bytes([data[pos + 2], data[pos + 3]]);

            layer_dimensions.push(MapSize { width, height });
        }

        let dims = layer_dimensions[0];

        if dims.width <= 0 || dims.height <= 0 {
            return Err(WrlError::InvalidDimensions {
                width: dims.width,
                height: dims.height,
            });
        }

        if dims != MapSize::STANDARD {
            warnings.push(format!(
                "нестандартный размер карты {}×{} (реальные файлы — 112×112); формат допускает любые dims[0]",
                dims.width, dims.height
            ));
        }

        if layer_count != 1 {
            warnings.push(format!(
                "layer_count={layer_count}: используется только dims[0], миникарта читается как один слой W*H"
            ));
        }

        let width = dims.width;
        let height = dims.height;
        let cells = width as usize * height as usize;

        let minimap_offset = dims_end;
        let tile_ids_offset = minimap_offset + cells;
        let tile_count_offset = tile_ids_offset + 2 * cells;

        if file_len < tile_count_offset + 2 {
            return Err(WrlError::Truncated {
                section: "поле tile_count",
                needed: tile_count_offset + 2,
                file_len,
            });
        }

        let tile_count = u16::from_le_bytes([data[tile_count_offset], data[tile_count_offset + 1]]);
        let tiles_offset = tile_count_offset + 2;
        let palette_offset = tiles_offset + TILE_PIXELS * tile_count as usize;
        let pass_offset = palette_offset + PALETTE_BYTES;
        let data_end = pass_offset + tile_count as usize;

        if file_len < palette_offset {
            return Err(WrlError::Truncated {
                section: "пиксели тайлов",
                needed: palette_offset,
                file_len,
            });
        }

        if file_len < pass_offset {
            return Err(WrlError::Truncated {
                section: "палитра",
                needed: pass_offset,
                file_len,
            });
        }

        if file_len < data_end {
            return Err(WrlError::Truncated {
                section: "pass-таблица",
                needed: data_end,
                file_len,
            });
        }

        if file_len > data_end {
            warnings.push(format!(
                "после pass-таблицы {} лишних байт (0x{data_end:X}..0x{file_len:X}); в реальных файлах pass-таблица заканчивается на EOF",
                file_len - data_end
            ));
        }

        let offsets = WrlOffsets {
            signature: 0,
            layer_count: SIGNATURE_LEN,
            dims: dims_offset,
            minimap: minimap_offset,
            tile_ids: tile_ids_offset,
            tile_count: tile_count_offset,
            tiles: tiles_offset,
            palette: palette_offset,
            pass_table: pass_offset,
            end: data_end,
        };

        let minimap = data[minimap_offset..minimap_offset + cells].to_vec();

        let mut tile_ids = Vec::with_capacity(cells);

        for chunk in data[tile_ids_offset..tile_ids_offset + 2 * cells].chunks_exact(2) {
            tile_ids.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }

        let mut palette = [0u8; PALETTE_BYTES];
        palette.copy_from_slice(&data[palette_offset..pass_offset]);

        let pass_table = data[pass_offset..data_end].to_vec();

        if let Some(index) = tile_ids.iter().position(|&tile_id| tile_id >= tile_count) {
            warnings.push(format!(
                "клетка {index} ссылается на tile_id={} >= tile_count={tile_count}; такая клетка получит Surface::NONE",
                tile_ids[index]
            ));
        }

        if let Some(index) = pass_table.iter().position(|&value| value > 3) {
            warnings.push(format!(
                "байт pass-таблицы {index}={} вне 0..3; такой тайл получит Surface::NONE",
                pass_table[index]
            ));
        }

        if tile_count == 0 {
            warnings.push("tile_count = 0: тайлов и pass-таблицы нет".to_owned());
        }

        Ok(Self {
            data,
            signature,
            layer_count,
            layer_dimensions,
            width,
            height,
            cells,
            tile_count,
            minimap,
            tile_ids,
            palette,
            pass_table,
            offsets,
            warnings,
        })
    }

    /// Reads and parses the map from a file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WrlError> {
        let path = path.as_ref();
        let data = fs::read(path).map_err(|source| WrlError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::from_vec(data)
    }

    /// Raw bytes of the whole file.
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// File length in bytes.
    #[inline]
    pub fn file_size(&self) -> usize {
        self.data.len()
    }

    /// Signature from the first 3 bytes (may be `"DMO"`).
    #[inline]
    pub fn signature(&self) -> [u8; 3] {
        self.signature
    }

    /// Signature as text (Latin-1, lossless).
    pub fn signature_text(&self) -> String {
        self.signature.iter().map(|&byte| byte as char).collect()
    }

    /// Number of layers from the header (real files have 1).
    #[inline]
    pub fn layer_count(&self) -> u16 {
        self.layer_count
    }

    /// Dimensions of all layers `dims[layer_count]`.
    #[inline]
    pub fn layer_dimensions(&self) -> &[MapSize] {
        &self.layer_dimensions
    }

    /// Map width (`dims[0].x`).
    #[inline]
    pub fn width(&self) -> i16 {
        self.width
    }

    /// Map height (`dims[0].y`).
    #[inline]
    pub fn height(&self) -> i16 {
        self.height
    }

    /// Number of cells `W*H`.
    #[inline]
    pub fn cells(&self) -> usize {
        self.cells
    }

    /// `dims[0]` matches the standard 112×112.
    #[inline]
    pub fn is_standard_size(&self) -> bool {
        (self.width, self.height) == (STANDARD_WIDTH, STANDARD_HEIGHT)
    }

    /// Minimap: `W*H` bytes of palette indices, row-major.
    #[inline]
    pub fn minimap(&self) -> &[u8] {
        &self.minimap
    }

    /// Tile-id map: `W*H` `u16` values, row-major (`y*W + x`).
    #[inline]
    pub fn tile_map(&self) -> &[u16] {
        &self.tile_ids
    }

    /// Number of tiles in the set.
    #[inline]
    pub fn tile_count(&self) -> u16 {
        self.tile_count
    }

    /// All tile pixels in sequence: `tile_count × 64×64` bytes.
    ///
    /// A pixel is a palette index of this same file; the order is by tile id,
    /// inside a tile row-major.
    #[inline]
    pub fn tiles(&self) -> &[u8] {
        let end = self.offsets.tiles + TILE_PIXELS * self.tile_count as usize;

        &self.data[self.offsets.tiles..end]
    }

    /// Raw 64×64 bytes of one tile; `None` if `tile_id` is outside the set.
    pub fn tile_pixels(&self, tile_id: u16) -> Option<&[u8]> {
        if tile_id >= self.tile_count {
            return None;
        }

        let start = self.offsets.tiles + TILE_PIXELS * tile_id as usize;

        Some(&self.data[start..start + TILE_PIXELS])
    }

    /// Palette: raw 768 bytes (`256 × RGB`, 8 bits per channel).
    #[inline]
    pub fn palette(&self) -> &[u8; PALETTE_BYTES] {
        &self.palette
    }

    /// Pass table: `tile_count` raw bytes 0..3.
    #[inline]
    pub fn pass_table(&self) -> &[u8] {
        &self.pass_table
    }

    /// Pass table mapped to [`Surface`] (`0→LAND, 1→WATER, 2→COAST,
    /// 3→AIR`); bytes outside 0..3 give [`Surface::NONE`].
    pub fn pass_surfaces(&self) -> Vec<Surface> {
        self.pass_table
            .iter()
            .map(|&value| {
                PASS_TABLE_SURFACES
                    .get(value as usize)
                    .copied()
                    .unwrap_or(Surface::NONE)
            })
            .collect()
    }

    /// Surface map by the formula of `formats/wrl.md` §3.1.
    ///
    /// `surface[i] = PassData[pass_table[tile_ids[i]]]`, cells with
    /// `tile_id >= tile_count` stay [`Surface::NONE`]. The result is a map
    /// of the fixed size `max_core::geo::MAP_TILES`; for the stock 112×112
    /// the layout matches `max_core::setup::derive_surface_map` byte for byte.
    /// For non-standard dims[0] the cells are copied by the flat row-major
    /// index (`y*W + x`), and the missing ones stay [`Surface::NONE`].
    pub fn surface_map(&self) -> SurfaceMap {
        let surfaces = self.pass_surfaces();
        let mut tiles = [Surface::NONE; MAP_TILES];

        for (slot, &tile_id) in tiles.iter_mut().zip(self.tile_ids.iter()) {
            *slot = surfaces
                .get(tile_id as usize)
                .copied()
                .unwrap_or(Surface::NONE);
        }

        SurfaceMap::from_tiles(tiles)
    }

    /// Section offsets (for debugging and tests).
    #[inline]
    pub fn offsets(&self) -> &WrlOffsets {
        &self.offsets
    }

    /// Non-critical discrepancies with the format (empty for reference files).
    #[inline]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Tail after the pass table in bytes (0 in real files).
    #[inline]
    pub fn trailing_bytes(&self) -> usize {
        self.data.len().saturating_sub(self.offsets.end)
    }
}

impl fmt::Debug for WrlMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrlMap")
            .field("file_size", &self.data.len())
            .field("signature", &self.signature_text())
            .field("layer_count", &self.layer_count)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("tile_count", &self.tile_count)
            .field("warnings", &self.warnings.len())
            .finish()
    }
}

#[cfg(test)]
#[path = "wrl_tests.rs"]
mod tests;
