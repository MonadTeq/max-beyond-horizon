//! WRL (World) — карты планет M.A.X.
//!
//! Формат описан в `max-original-dasm/formats/wrl.md`:
//!
//! ```text
//! +0x0000  u8[3]                сигнатура "WRL" (у SNOW_1.WRL — "DMO")
//! +0x0003  u16 LE               layer_count
//! +0x0005  Point[layer_count]   dims: {i16 x; i16 y} (используется dims[0])
//! +…       u8[W*H]              миникарта (индексы палитры)
//! +…       u16[W*H] LE          карта tile-id, row-major
//! +…       u16 LE               tile_count
//! +…       u8[tile_count*4096]  пиксели тайлов 64×64, 1 байт на пиксель
//! +…       u8[768]              палитра 256 × RGB (8 бит на канал)
//! +…       u8[tile_count]       pass-таблица (0..3)
//! ```
//!
//! Сжатия нет: тайлы — сырые индексы палитры. Загрузчик порта **не проверяет**
//! сигнатуру (`SNOW_1.WRL` начинается с `"DMO"`), поэтому сигнатура `"DMO"`
//! принимается с предупреждением, а любая другая — ошибка.
//!
//! [`WrlMap::surface_map`] строит [`SurfaceMap`] по формуле `world.cpp:382-412`
//! (`formats/wrl.md` §3.1):
//!
//! ```text
//! PassData = {LAND, WATER, COAST, AIR} = {1, 2, 4, 8}
//! surface[i] = PassData[pass_table[tile_ids[i]]]
//! ```
//!
//! Как и `max_core::setup::derive_surface_map`, клетки с `tile_id >= tile_count`
//! и тайлы с `pass_byte > 3` получают [`Surface::NONE`] (в C++ это было бы
//! чтение за границей массива, §7.6). Баг-фиксы порта (`ApplyBugFixes`,
//! §3.3) привязаны к SHA-256 конкретных файлов и здесь не применяются: это
//! отдельный слой политики, а не разбор формата.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use max_types::enums::Surface;
use max_types::geo::MAP_TILES;
use max_types::map::{MapSize, SurfaceMap};

/// Штатная сигнатура WRL (3 байта, без NUL).
pub const WRL_SIGNATURE: [u8; 3] = *b"WRL";
/// Известное исключение: битая сигнатура `SNOW_1.WRL` (§1.1).
pub const DMO_SIGNATURE: [u8; 3] = *b"DMO";
/// Длина сигнатуры.
pub const SIGNATURE_LEN: usize = 3;
/// Сторона тайла (`GFX_MAP_TILE_SIZE`).
pub const TILE_SIZE: usize = 64;
/// Пикселей (байт) в одном тайле: 64×64.
pub const TILE_PIXELS: usize = TILE_SIZE * TILE_SIZE;
/// Цветов в палитре (`PALETTE_SIZE`).
pub const PALETTE_COLORS: usize = 256;
/// Каналов на цвет (RGB).
pub const PALETTE_STRIDE: usize = 3;
/// Байт в палитре: 256 × 3.
pub const PALETTE_BYTES: usize = PALETTE_COLORS * PALETTE_STRIDE;
/// Индексы pass-таблицы -> [`Surface`] (`PassData[]` из `world.cpp:382`).
pub const PASS_TABLE_SURFACES: [Surface; 4] =
    [Surface::LAND, Surface::WATER, Surface::COAST, Surface::AIR];
/// Штатная ширина карты.
pub const STANDARD_WIDTH: i16 = 112;
/// Штатная высота карты.
pub const STANDARD_HEIGHT: i16 = 112;
/// Размер стекового массива `Point map_layer_dimensions[12]` в C++-порте.
pub const MAX_CPP_LAYERS: u16 = 12;

/// Смещения секций разобранного WRL (в байтах от начала файла).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WrlOffsets {
    /// Сигнатура (`0`).
    pub signature: usize,
    /// `layer_count`.
    pub layer_count: usize,
    /// Массив `dims[layer_count]`.
    pub dims: usize,
    /// Миникарта (`W*H` байт).
    pub minimap: usize,
    /// Карта tile-id (`W*H × u16`).
    pub tile_ids: usize,
    /// Поле `tile_count`.
    pub tile_count: usize,
    /// Пиксели тайлов (`tile_count × 4096`).
    pub tiles: usize,
    /// Палитра (768 байт).
    pub palette: usize,
    /// Pass-таблица (`tile_count` байт).
    pub pass_table: usize,
    /// Конец данных (`pass_table + tile_count`).
    pub end: usize,
}

/// Ошибка разбора/чтения WRL-файла.
#[derive(Debug)]
pub enum WrlError {
    /// Не удалось прочитать файл целиком.
    Io {
        /// Путь, который читался.
        path: PathBuf,
        /// Причина от ОС.
        source: io::Error,
    },
    /// Файл короче заголовка (3 + 2 байта).
    TooShort {
        /// Фактическая длина файла.
        len: usize,
        /// Минимально необходимая длина.
        needed: usize,
    },
    /// Первые 3 байта — не `"WRL"` и не известное исключение `"DMO"`.
    BadSignature {
        /// Фактическая сигнатура.
        found: [u8; 3],
    },
    /// `layer_count == 0`: в файле нет `dims[0]`, карта повреждена.
    ZeroLayers,
    /// `dims[0]` содержит неположительные размеры.
    InvalidDimensions {
        /// Ширина из `dims[0]`.
        width: i16,
        /// Высота из `dims[0]`.
        height: i16,
    },
    /// Очередная секция не помещается в файл.
    Truncated {
        /// Название секции.
        section: &'static str,
        /// Требуемый конец секции (смещение от начала файла).
        needed: usize,
        /// Фактическая длина файла.
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

/// Разобранная карта WRL.
///
/// Байты файла хранятся целиком: `minimap`/`tile_ids`/`pass_table`/`palette`
/// разбираются в удобные структуры, а пиксели тайлов читаются срезами из
/// [`WrlMap::tiles`] без копирования.
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
    /// Разбирает карту из байтов (копирует срез во владение).
    pub fn parse(data: &[u8]) -> Result<Self, WrlError> {
        Self::from_vec(data.to_vec())
    }

    /// Разбирает карту из уже имеющегося `Vec<u8>` (без копирования данных).
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

    /// Читает и разбирает карту из файла.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WrlError> {
        let path = path.as_ref();
        let data = fs::read(path).map_err(|source| WrlError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::from_vec(data)
    }

    /// Сырые байты файла целиком.
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Длина файла в байтах.
    #[inline]
    pub fn file_size(&self) -> usize {
        self.data.len()
    }

    /// Сигнатура из первых 3 байт (может быть `"DMO"`).
    #[inline]
    pub fn signature(&self) -> [u8; 3] {
        self.signature
    }

    /// Сигнатура как текст (Latin-1, без потерь).
    pub fn signature_text(&self) -> String {
        self.signature.iter().map(|&byte| byte as char).collect()
    }

    /// Число слоёв из заголовка (реальные файлы — 1).
    #[inline]
    pub fn layer_count(&self) -> u16 {
        self.layer_count
    }

    /// Размеры всех слоёв `dims[layer_count]`.
    #[inline]
    pub fn layer_dimensions(&self) -> &[MapSize] {
        &self.layer_dimensions
    }

    /// Ширина карты (`dims[0].x`).
    #[inline]
    pub fn width(&self) -> i16 {
        self.width
    }

    /// Высота карты (`dims[0].y`).
    #[inline]
    pub fn height(&self) -> i16 {
        self.height
    }

    /// Число клеток `W*H`.
    #[inline]
    pub fn cells(&self) -> usize {
        self.cells
    }

    /// `dims[0]` совпадает со штатным 112×112.
    #[inline]
    pub fn is_standard_size(&self) -> bool {
        (self.width, self.height) == (STANDARD_WIDTH, STANDARD_HEIGHT)
    }

    /// Миникарта: `W*H` байт индексов палитры, row-major.
    #[inline]
    pub fn minimap(&self) -> &[u8] {
        &self.minimap
    }

    /// Карта tile-id: `W*H` значений `u16`, row-major (`y*W + x`).
    #[inline]
    pub fn tile_map(&self) -> &[u16] {
        &self.tile_ids
    }

    /// Число тайлов в наборе.
    #[inline]
    pub fn tile_count(&self) -> u16 {
        self.tile_count
    }

    /// Все пиксели тайлов подряд: `tile_count × 64×64` байт.
    ///
    /// Пиксель — индекс палитры этого же файла; порядок — по id тайла,
    /// внутри тайла row-major.
    #[inline]
    pub fn tiles(&self) -> &[u8] {
        let end = self.offsets.tiles + TILE_PIXELS * self.tile_count as usize;

        &self.data[self.offsets.tiles..end]
    }

    /// Сырые 64×64 байта одного тайла; `None`, если `tile_id` вне набора.
    pub fn tile_pixels(&self, tile_id: u16) -> Option<&[u8]> {
        if tile_id >= self.tile_count {
            return None;
        }

        let start = self.offsets.tiles + TILE_PIXELS * tile_id as usize;

        Some(&self.data[start..start + TILE_PIXELS])
    }

    /// Палитра: сырые 768 байт (`256 × RGB`, 8 бит на канал).
    #[inline]
    pub fn palette(&self) -> &[u8; PALETTE_BYTES] {
        &self.palette
    }

    /// Pass-таблица: `tile_count` сырых байт 0..3.
    #[inline]
    pub fn pass_table(&self) -> &[u8] {
        &self.pass_table
    }

    /// Pass-таблица, отображённая в [`Surface`] (`0→LAND, 1→WATER, 2→COAST,
    /// 3→AIR`); байты вне 0..3 дают [`Surface::NONE`].
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

    /// Карта поверхностей по формуле `formats/wrl.md` §3.1.
    ///
    /// `surface[i] = PassData[pass_table[tile_ids[i]]]`, клетки с
    /// `tile_id >= tile_count` остаются [`Surface::NONE`]. Результат — карта
    /// фиксированного размера `max_core::geo::MAP_TILES`; для штатных 112×112
    /// раскладка совпадает с `max_core::setup::derive_surface_map` байт-в-байт.
    /// Для нестандартных dims[0] клетки копируются по плоскому row-major
    /// индексу (`y*W + x`), а недостающие остаются [`Surface::NONE`].
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

    /// Смещения секций (для отладки и тестов).
    #[inline]
    pub fn offsets(&self) -> &WrlOffsets {
        &self.offsets
    }

    /// Некритичные расхождения с форматом (пусто для эталонных файлов).
    #[inline]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Хвост после pass-таблицы в байтах (в реальных файлах — 0).
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
mod tests {
    use super::*;

    /// Собирает минимальный WRL заданного размера (layer_count = 1).
    fn build_wrl(
        width: i16,
        height: i16,
        tile_ids: &[u16],
        tile_count: u16,
        pass_table: &[u8],
    ) -> Vec<u8> {
        let cells = width as usize * height as usize;

        assert_eq!(tile_ids.len(), cells);
        assert_eq!(pass_table.len(), tile_count as usize);

        let mut out = Vec::new();
        out.extend_from_slice(&WRL_SIGNATURE);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend(std::iter::repeat_n(0u8, cells));

        for tile_id in tile_ids {
            out.extend_from_slice(&tile_id.to_le_bytes());
        }

        out.extend_from_slice(&tile_count.to_le_bytes());
        out.extend(std::iter::repeat_n(7u8, TILE_PIXELS * tile_count as usize));
        out.extend_from_slice(&[0u8; PALETTE_BYTES]);
        out.extend_from_slice(pass_table);
        out
    }

    #[test]
    fn zero_tile_count_is_valid_and_yields_none_surface() {
        let bytes = build_wrl(112, 112, &[0u16; 12_544], 0, &[]);
        let map = WrlMap::parse(&bytes).unwrap();

        assert_eq!(map.tile_count(), 0);
        assert!(map.tiles().is_empty());
        assert_eq!(map.offsets().end, bytes.len());
        assert!(map.surface_map().to_bytes().iter().all(|&byte| byte == 0));
        assert!(map
            .warnings()
            .iter()
            .any(|warning| warning.contains("tile_count = 0")));
    }

    #[test]
    fn out_of_range_values_become_none() {
        // 2×2: tile_id 5 вне набора (tile_count = 1), pass 9 вне 0..3.
        let bytes = build_wrl(2, 2, &[0, 5, 0, 0], 1, &[9]);
        let map = WrlMap::parse(&bytes).unwrap();

        assert_eq!(map.pass_surfaces(), vec![Surface::NONE]);
        assert_eq!(&map.surface_map().to_bytes()[..4], &[0u8; 4]);
        assert!(map
            .warnings()
            .iter()
            .any(|warning| warning.contains("tile_id")));
        assert!(map
            .warnings()
            .iter()
            .any(|warning| warning.contains("pass-таблицы")));

        // Валидный pass 2 -> COAST.
        let bytes = build_wrl(2, 2, &[0, 5, 0, 0], 1, &[2]);
        let map = WrlMap::parse(&bytes).unwrap();

        assert_eq!(
            &map.surface_map().to_bytes()[..4],
            &[
                Surface::COAST.bits(),
                0,
                Surface::COAST.bits(),
                Surface::COAST.bits()
            ]
        );
    }

    #[test]
    fn errors_are_returned_instead_of_panicking() {
        assert!(matches!(WrlMap::parse(b""), Err(WrlError::TooShort { .. })));
        assert!(matches!(
            WrlMap::parse(b"WRL\x01"),
            Err(WrlError::TooShort { .. })
        ));

        let mut bad_signature = build_wrl(2, 2, &[0; 4], 1, &[0]);
        bad_signature[0..3].copy_from_slice(b"XYZ");
        assert!(matches!(
            WrlMap::parse(&bad_signature),
            Err(WrlError::BadSignature { .. })
        ));

        let mut zero_layers = build_wrl(2, 2, &[0; 4], 1, &[0]);
        zero_layers[3..5].copy_from_slice(&0u16.to_le_bytes());
        assert!(matches!(
            WrlMap::parse(&zero_layers),
            Err(WrlError::ZeroLayers)
        ));

        let invalid_dims = {
            let mut bytes = build_wrl(2, 2, &[0; 4], 1, &[0]);
            bytes[5..7].copy_from_slice(&0i16.to_le_bytes());
            bytes
        };
        assert!(matches!(
            WrlMap::parse(&invalid_dims),
            Err(WrlError::InvalidDimensions { .. })
        ));
    }

    #[test]
    fn dmo_signature_is_accepted_with_a_warning() {
        let mut bytes = build_wrl(2, 2, &[0; 4], 1, &[0]);
        bytes[0..3].copy_from_slice(&DMO_SIGNATURE);

        let map = WrlMap::parse(&bytes).unwrap();

        assert_eq!(map.signature(), DMO_SIGNATURE);
        assert!(map.warnings().iter().any(|warning| warning.contains("DMO")));
    }
}
