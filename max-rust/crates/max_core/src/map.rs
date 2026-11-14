//! Карта мира: поверхность (проходимость) и ресурсы/видимость по тайлам.
//!
//! Порт структур оракула `max-cpp-port/src`:
//! * `SurfaceMap` — `uint8_t[112*112]` из `World::LoadTilesAndMetadata()`
//!   (`world.cpp`, «Load pass table and build surface map»): байт на клетку,
//!   значение — битовая маска [`Surface`] (`SURFACE_TYPE_*`, `enums.hpp`);
//! * `ResourceManager_CargoMap` — `uint16_t[112*112]` (`resource_manager.hpp`),
//!   он же `short GridResourceMapEntry[112][112]` из state-model.md §3.10:
//!   битфилд «количество (0–4) | тип (5–7) | видимость команд (10–13)».
//!
//! Инварианты раскладки взяты из state-model.md §3.10 и §7.3:
//! * биты 0–4 (`0x001F`) — количество ресурса (0..=15 по маске; игра пишет до 16);
//! * биты 5–7 (`0x00E0`) — ровно один бит типа (`CARGO_FUEL`/`CARGO_GOLD`/
//!   `CARGO_MATERIALS`), 0 — пустой тайл;
//! * биты 10–13 (`0x3C00`) — разведанность командой `i` в бите `i`
//!   (save.md объявляет маску `0x1C00`, но она не накрывает красных —
//!   фактическая маска `0x3C00`, DESIGN.md риск 5 / state-model §7.3).
//!
//! Модуль не делает I/O: и поверхность, и ресурсы — обычные массивы фиксированного
//! размера [`MAP_TILES`], которые заполняет [`crate::setup`] (генерация партии)
//! или загрузчик сейвов (`save.rs`, вне этого крейта).

use core::fmt;

use serde::de::{Error as DeError, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::enums::{Surface, CARGO_FUEL, CARGO_GOLD, CARGO_MATERIALS, CARGO_MASK};
use crate::geo::{MAP_HEIGHT, MAP_TILES, MAP_WIDTH};

/// Размеры карты в тайлах — каркасный `map::MapSize` (`Point` в C++).
///
/// В M.A.X. размер берётся из `.wrl`-файла планеты; для штатных планет — 112×112
/// ([`crate::geo::MAP_SIZE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MapSize {
    /// Ширина в тайлах.
    pub width: i16,
    /// Высота в тайлах.
    pub height: i16,
}

impl MapSize {
    /// Штатный размер карты (`MAP_SIZE`).
    pub const STANDARD: Self = Self {
        width: MAP_WIDTH,
        height: MAP_HEIGHT,
    };

    /// Количество тайлов (`width * height`).
    #[inline]
    pub const fn tiles(self) -> usize {
        self.width as usize * self.height as usize
    }
}

impl Default for MapSize {
    #[inline]
    fn default() -> Self {
        Self::STANDARD
    }
}

// ---------------------------------------------------------------------------
// Ресурсы: CARGO_* и ResourceMapEntry
// ---------------------------------------------------------------------------

/// Маска количества ресурса — биты 0–4 (`RESOURCE_AMOUNT_MASK`).
pub const RESOURCE_AMOUNT_MASK: u16 = CARGO_MASK as u16; // 0x001F
/// Маска типа ресурса — биты 5–7 (`RESOURCE_TYPE_MASK`).
pub const RESOURCE_TYPE_MASK: u16 = (CARGO_FUEL | CARGO_GOLD | CARGO_MATERIALS) as u16; // 0x00E0
/// Сдвиг битов видимости команд.
pub const TEAM_VISIBILITY_SHIFT: u16 = 10;
/// Маска видимости всех четырёх играбельных команд (`0x3C00`, см. §7.3).
pub const TEAM_VISIBILITY_MASK: u16 = 0x3C00;
/// Маска видимости из save.md (`0x1C00`, не накрывает красных — оставлена для
/// сверки с V70).
pub const TEAM_VISIBILITY_MASK_SAVE: u16 = 0x1C00;
/// Количество играбельных команд, у которых есть бит разведанности.
pub const VISIBILITY_TEAM_COUNT: u8 = 4;

/// Тип ресурса — биты 5–7 `GridResourceMapEntry` (`CARGO_*` в `enums.hpp`).
///
/// Порядок вариантов сохранён из каркаса (`Raw`, `Fuel`, `Gold`); значения —
/// фактические биты типа в `ResourceManager_CargoMap`, чтобы конверсия
/// «битфилд ⇄ enum» была без таблиц.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum ResourceKind {
    /// Материалы — `CARGO_MATERIALS` (0x80).
    Raw = CARGO_MATERIALS as u16,
    /// Топливо — `CARGO_FUEL` (0x20).
    Fuel = CARGO_FUEL as u16,
    /// Золото — `CARGO_GOLD` (0x40).
    Gold = CARGO_GOLD as u16,
}

impl ResourceKind {
    /// Все типы в порядке C++ (`CARGO_MATERIALS`, `CARGO_FUEL`, `CARGO_GOLD`).
    pub const ALL: [Self; 3] = [Self::Raw, Self::Fuel, Self::Gold];

    /// Биты типа в `GridResourceMapEntry`.
    #[inline]
    pub const fn cargo_bits(self) -> u16 {
        self as u16
    }

    /// Декодирует биты типа (5–7); `None`, если не установлен ровно один бит.
    #[inline]
    pub const fn from_cargo_bits(bits: u16) -> Option<Self> {
        let type_bits = bits & RESOURCE_TYPE_MASK;

        if type_bits == CARGO_MATERIALS as u16 {
            Some(Self::Raw)
        } else if type_bits == CARGO_FUEL as u16 {
            Some(Self::Fuel)
        } else if type_bits == CARGO_GOLD as u16 {
            Some(Self::Gold)
        } else {
            None
        }
    }
}

/// Один тайл карты ресурсов — `short GridResourceMapEntry` (`uint16_t`).
///
/// Битфилд, а не структура полей: раскладка обязана совпадать с
/// `ResourceManager_CargoMap` побайтово (state-model.md §3.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceMapEntry(u16);

impl ResourceMapEntry {
    /// Пустой тайл (`CARGO_MATERIALS` не установлен, количество 0).
    pub const NONE: Self = Self(0);

    /// Создаёт запись из сырого битфилда (как `CargoMap[...]`).
    #[inline]
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// Создаёт запись из полей; `amount` маскируется `0x1F`, `visibility` — `0x0F`.
    #[inline]
    pub const fn new(amount: u8, kind: Option<ResourceKind>, visibility: u8) -> Self {
        let mut bits = (amount as u16) & RESOURCE_AMOUNT_MASK;

        if let Some(kind) = kind {
            bits |= kind.cargo_bits();
        }

        bits |= ((visibility & 0x0F) as u16) << TEAM_VISIBILITY_SHIFT;

        Self(bits)
    }

    /// Сырой битфилд (для дампов/сейвов).
    #[inline]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Количество ресурса — биты 0–4 (`& 0x1F`, как в `resource_allocator.cpp`).
    #[inline]
    pub const fn amount(self) -> u8 {
        (self.0 & RESOURCE_AMOUNT_MASK) as u8
    }

    /// Тип ресурса — биты 5–7; `None`, если тип не установлен.
    #[inline]
    pub const fn resource_type(self) -> Option<ResourceKind> {
        ResourceKind::from_cargo_bits(self.0)
    }

    /// Разведанность: бит `i` — команда `i` (`RED=0 .. GRAY=3`), биты 10–13.
    #[inline]
    pub const fn visibility(self) -> u8 {
        ((self.0 & TEAM_VISIBILITY_MASK) >> TEAM_VISIBILITY_SHIFT) as u8
    }

    /// Установлена ли разведанность команды `team` (0..=3).
    #[inline]
    pub const fn is_visible_to(self, team: u8) -> bool {
        team < VISIBILITY_TEAM_COUNT && (self.visibility() & (1 << team)) != 0
    }

    /// Пустой тайл (ни типа, ни количества, ни разведанности).
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Копия записи с другим количеством (биты 0–4).
    #[inline]
    pub const fn with_amount(self, amount: u8) -> Self {
        Self((self.0 & !RESOURCE_AMOUNT_MASK) | ((amount as u16) & RESOURCE_AMOUNT_MASK))
    }

    /// Копия записи с другим типом (биты 5–7 перезаписываются).
    #[inline]
    pub const fn with_kind(self, kind: Option<ResourceKind>) -> Self {
        let type_bits = match kind {
            Some(kind) => kind.cargo_bits(),
            None => 0,
        };

        Self((self.0 & !RESOURCE_TYPE_MASK) | type_bits)
    }

    /// Копия записи с другой разведанностью (биты 10–13).
    #[inline]
    pub const fn with_visibility(self, visibility: u8) -> Self {
        Self((self.0 & !TEAM_VISIBILITY_MASK) | (((visibility & 0x0F) as u16) << TEAM_VISIBILITY_SHIFT))
    }
}

impl fmt::Display for ResourceMapEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:04X}", self.0)
    }
}

// ---------------------------------------------------------------------------
// SurfaceMap
// ---------------------------------------------------------------------------

/// Карта проходимости — `SurfaceMap` (байт на клетку, [`MAP_TILES`] тайлов).
///
/// Индексация — как во всём C++-порте: `index = y * width + x`.
#[derive(Clone, PartialEq, Eq)]
pub struct SurfaceMap {
    /// Байты поверхности в порядке `y * MAP_WIDTH + x`.
    tiles: [Surface; MAP_TILES],
}

impl SurfaceMap {
    /// Пустая карта (все тайлы [`Surface::NONE`]).
    #[inline]
    pub const fn new() -> Self {
        Self {
            tiles: [Surface::NONE; MAP_TILES],
        }
    }

    /// Карта из готового массива байтов (`World::GetSurfaceMap()`).
    #[inline]
    pub const fn from_tiles(tiles: [Surface; MAP_TILES]) -> Self {
        Self { tiles }
    }

    /// Все тайлы (порядок `y * MAP_WIDTH + x`).
    #[inline]
    pub fn tiles(&self) -> &[Surface; MAP_TILES] {
        &self.tiles
    }

    /// Изменяемый доступ ко всем тайлам.
    #[inline]
    pub fn tiles_mut(&mut self) -> &mut [Surface; MAP_TILES] {
        &mut self.tiles
    }

    /// Индекс тайла `(x, y)` или `None` вне карты.
    #[inline]
    pub fn index(x: i16, y: i16) -> Option<usize> {
        if x < 0 || y < 0 || x >= MAP_WIDTH || y >= MAP_HEIGHT {
            None
        } else {
            Some(y as usize * MAP_WIDTH as usize + x as usize)
        }
    }

    /// Поверхность тайла; вне карты — [`Surface::NONE`] (как `World::GetSurfaceType`).
    #[inline]
    pub fn get(&self, x: i16, y: i16) -> Surface {
        match Self::index(x, y) {
            Some(index) => self.tiles[index],
            None => Surface::NONE,
        }
    }

    /// Записывает поверхность тайла; `false`, если координаты вне карты.
    #[inline]
    pub fn set(&mut self, x: i16, y: i16, surface: Surface) -> bool {
        match Self::index(x, y) {
            Some(index) => {
                self.tiles[index] = surface;

                true
            }
            None => false,
        }
    }

    /// Сырые байты карты (дамп эталонных векторов, сейв V70).
    #[inline]
    pub fn to_bytes(&self) -> [u8; MAP_TILES] {
        let mut bytes = [0u8; MAP_TILES];

        for (byte, surface) in bytes.iter_mut().zip(self.tiles.iter()) {
            *byte = surface.bits();
        }

        bytes
    }
}

impl Default for SurfaceMap {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SurfaceMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Полный дамп 112×112 в Debug нечитаем; показываем гистограмму.
        let mut counts = [0usize; 16];

        for tile in &self.tiles {
            counts[tile.bits() as usize] += 1;
        }

        f.debug_struct("SurfaceMap")
            .field("tiles", &MAP_TILES)
            .field("histogram", &counts)
            .finish()
    }
}

impl Serialize for SurfaceMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(MAP_TILES))?;

        for tile in &self.tiles {
            seq.serialize_element(tile)?;
        }

        seq.end()
    }
}

impl<'de> Deserialize<'de> for SurfaceMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SurfaceMapVisitor;

        impl<'de> Visitor<'de> for SurfaceMapVisitor {
            type Value = SurfaceMap;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a sequence of {MAP_TILES} surface tiles")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut tiles = [Surface::NONE; MAP_TILES];

                for (index, slot) in tiles.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| A::Error::invalid_length(index, &self))?;
                }

                Ok(SurfaceMap { tiles })
            }
        }

        deserializer.deserialize_seq(SurfaceMapVisitor)
    }
}

// ---------------------------------------------------------------------------
// ResourceMap
// ---------------------------------------------------------------------------

/// Карта ресурсов — `ResourceManager_CargoMap` (`uint16_t[MAP_TILES]`).
#[derive(Clone, PartialEq, Eq)]
pub struct ResourceMap {
    /// Битфилды ресурсов в порядке `y * MAP_WIDTH + x`.
    tiles: [ResourceMapEntry; MAP_TILES],
}

impl ResourceMap {
    /// Пустая карта (все тайлы [`ResourceMapEntry::NONE`]).
    #[inline]
    pub const fn new() -> Self {
        Self {
            tiles: [ResourceMapEntry::NONE; MAP_TILES],
        }
    }

    /// Карта из готового массива записей.
    #[inline]
    pub const fn from_tiles(tiles: [ResourceMapEntry; MAP_TILES]) -> Self {
        Self { tiles }
    }

    /// Все тайлы (порядок `y * MAP_WIDTH + x`).
    #[inline]
    pub fn tiles(&self) -> &[ResourceMapEntry; MAP_TILES] {
        &self.tiles
    }

    /// Изменяемый доступ ко всем тайлам.
    #[inline]
    pub fn tiles_mut(&mut self) -> &mut [ResourceMapEntry; MAP_TILES] {
        &mut self.tiles
    }

    /// Индекс тайла `(x, y)` или `None` вне карты.
    #[inline]
    pub fn index(x: i16, y: i16) -> Option<usize> {
        SurfaceMap::index(x, y)
    }

    /// Запись тайла; вне карты — [`ResourceMapEntry::NONE`].
    #[inline]
    pub fn get(&self, x: i16, y: i16) -> ResourceMapEntry {
        match Self::index(x, y) {
            Some(index) => self.tiles[index],
            None => ResourceMapEntry::NONE,
        }
    }

    /// Записывает тайл; `false`, если координаты вне карты.
    #[inline]
    pub fn set(&mut self, x: i16, y: i16, entry: ResourceMapEntry) -> bool {
        match Self::index(x, y) {
            Some(index) => {
                self.tiles[index] = entry;

                true
            }
            None => false,
        }
    }

    /// Сырой битфилд тайла по индексу (`ResourceManager_CargoMap[index]`).
    ///
    /// Генерация ресурсов (`resource_allocator.cpp`) работает с плоским индексом
    /// `y * MAP_WIDTH + x`, поэтому индексный доступ — часть контракта.
    #[inline]
    pub fn bits_at(&self, index: usize) -> u16 {
        self.tiles[index].bits()
    }

    /// Записывает сырой битфилд по индексу (`CargoMap[index] = value`).
    #[inline]
    pub fn set_bits_at(&mut self, index: usize, bits: u16) {
        self.tiles[index] = ResourceMapEntry::from_bits(bits);
    }

    /// Количество ресурсов каждого типа (для property-тестов и отладки).
    pub fn counts(&self) -> ResourceCounts {
        let mut counts = ResourceCounts::default();

        for entry in &self.tiles {
            if let Some(kind) = entry.resource_type() {
                let amount = entry.amount() as u32;

                match kind {
                    ResourceKind::Raw => {
                        counts.raw_tiles += 1;
                        counts.raw_amount += amount;
                    }
                    ResourceKind::Fuel => {
                        counts.fuel_tiles += 1;
                        counts.fuel_amount += amount;
                    }
                    ResourceKind::Gold => {
                        counts.gold_tiles += 1;
                        counts.gold_amount += amount;
                    }
                }
            }
        }

        counts
    }

    /// Сырые байты карты в little-endian (дамп эталонных векторов, `short`V70).
    #[inline]
    pub fn to_le_bytes(&self) -> [u8; MAP_TILES * 2] {
        let mut bytes = [0u8; MAP_TILES * 2];

        for (chunk, entry) in bytes.chunks_exact_mut(2).zip(self.tiles.iter()) {
            chunk.copy_from_slice(&entry.bits().to_le_bytes());
        }

        bytes
    }
}

impl Default for ResourceMap {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ResourceMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResourceMap")
            .field("tiles", &MAP_TILES)
            .field("counts", &self.counts())
            .finish()
    }
}

impl Serialize for ResourceMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(MAP_TILES))?;

        for entry in &self.tiles {
            seq.serialize_element(entry)?;
        }

        seq.end()
    }
}

impl<'de> Deserialize<'de> for ResourceMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ResourceMapVisitor;

        impl<'de> Visitor<'de> for ResourceMapVisitor {
            type Value = ResourceMap;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a sequence of {MAP_TILES} resource map entries")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut tiles = [ResourceMapEntry::NONE; MAP_TILES];

                for (index, slot) in tiles.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| A::Error::invalid_length(index, &self))?;
                }

                Ok(ResourceMap { tiles })
            }
        }

        deserializer.deserialize_seq(ResourceMapVisitor)
    }
}

/// Сводка по ресурсам карты (`ResourceMap::counts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceCounts {
    /// Тайлов с материалами.
    pub raw_tiles: u32,
    /// Суммарное количество материалов.
    pub raw_amount: u32,
    /// Тайлов с топливом.
    pub fuel_tiles: u32,
    /// Суммарное количество топлива.
    pub fuel_amount: u32,
    /// Тайлов с золотом.
    pub gold_tiles: u32,
    /// Суммарное количество золота.
    pub gold_amount: u32,
}

// ---------------------------------------------------------------------------
// Map
// ---------------------------------------------------------------------------

/// Карта партии: размер + поверхность + ресурсы.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    /// Размер карты (по `.wrl`-файлу планеты; штатно 112×112).
    pub size: MapSize,
    /// Проходимость по тайлам.
    pub surface: SurfaceMap,
    /// Ресурсы и разведка по тайлам.
    pub resources: ResourceMap,
}

impl Map {
    /// Пустая карта штатного размера.
    #[inline]
    pub const fn new() -> Self {
        Self {
            size: MapSize::STANDARD,
            surface: SurfaceMap::new(),
            resources: ResourceMap::new(),
        }
    }

    /// Карта из поверхности и ресурсов.
    #[inline]
    pub const fn from_parts(surface: SurfaceMap, resources: ResourceMap) -> Self {
        Self {
            size: MapSize::STANDARD,
            surface,
            resources,
        }
    }

    /// Индекс тайла `(x, y)` или `None` вне карты.
    #[inline]
    pub fn index(&self, x: i16, y: i16) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.size.width || y >= self.size.height {
            None
        } else {
            Some(y as usize * self.size.width as usize + x as usize)
        }
    }

    /// Поверхность тайла; вне карты — [`Surface::NONE`].
    #[inline]
    pub fn surface_at(&self, x: i16, y: i16) -> Surface {
        match self.index(x, y) {
            Some(index) => self.surface.tiles()[index],
            None => Surface::NONE,
        }
    }

    /// Запись ресурсов тайла; вне карты — [`ResourceMapEntry::NONE`].
    #[inline]
    pub fn resource_at(&self, x: i16, y: i16) -> ResourceMapEntry {
        match self.index(x, y) {
            Some(index) => self.resources.tiles()[index],
            None => ResourceMapEntry::NONE,
        }
    }
}

impl Default for Map {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Тесты
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_size_matches_geometry() {
        assert_eq!(MAP_TILES, 12_544);
        assert_eq!(MapSize::STANDARD.tiles(), MAP_TILES);
        let map = Map::new();

        assert_eq!(map.size, crate::geo::MAP_SIZE);
        assert_eq!(map.surface.tiles().len(), MAP_TILES);
        assert_eq!(map.resources.tiles().len(), MAP_TILES);
    }

    #[test]
    fn resource_entry_layout_matches_save_example() {
        // Пример из state-model.md §3.10 / save.md: 0x208C = 12 материалов,
        // разведано только красными (бит 13).
        let entry = ResourceMapEntry::new(12, Some(ResourceKind::Raw), 0b1000);

        assert_eq!(entry.bits(), 0x208C);
        assert_eq!(entry.amount(), 12);
        assert_eq!(entry.resource_type(), Some(ResourceKind::Raw));
        assert_eq!(entry.visibility(), 0b1000);
        assert!(entry.is_visible_to(3));
        assert!(!entry.is_visible_to(0));

        let decoded = ResourceMapEntry::from_bits(0x208C);
        assert_eq!(decoded, entry);
    }

    #[test]
    fn resource_entry_bit_ranges() {
        for amount in 0..=0x0Fu16 {
            let entry = ResourceMapEntry::from_bits(amount);
            assert_eq!(entry.amount() as u16, amount);
            assert_eq!(entry.resource_type(), None);
            assert_eq!(entry.visibility(), 0);
        }

        for (bits, kind) in [
            (CARGO_MATERIALS as u16, ResourceKind::Raw),
            (CARGO_FUEL as u16, ResourceKind::Fuel),
            (CARGO_GOLD as u16, ResourceKind::Gold),
        ] {
            let entry = ResourceMapEntry::from_bits(bits | 0x000F);
            assert_eq!(entry.resource_type(), Some(kind));
            assert_eq!(entry.amount(), 0x0F);
        }

        // Два бита типа одновременно — невалидный тайл, тип не декодируется.
        let invalid = ResourceMapEntry::from_bits(CARGO_FUEL as u16 | CARGO_GOLD as u16);
        assert_eq!(invalid.resource_type(), None);

        for team in 0..=3u8 {
            let entry = ResourceMapEntry::from_bits(1 << (10 + team));
            assert_eq!(entry.visibility(), 1 << team);
            assert_eq!(entry.resource_type(), None);
            assert!(entry.is_visible_to(team));
        }

        // Бит 14/15 в маску видимости не входят.
        assert_eq!(ResourceMapEntry::from_bits(0xC000).visibility(), 0);
    }

    #[test]
    fn resource_entry_with_methods_keep_other_fields() {
        let entry = ResourceMapEntry::new(5, Some(ResourceKind::Fuel), 0b0011);

        let gold = entry.with_kind(Some(ResourceKind::Gold));
        assert_eq!(gold.amount(), 5);
        assert_eq!(gold.visibility(), 0b0011);
        assert_eq!(gold.resource_type(), Some(ResourceKind::Gold));

        let empty = gold.with_kind(None);
        assert_eq!(empty.resource_type(), None);
        assert_eq!(empty.bits(), (5 | (0b0011 << TEAM_VISIBILITY_SHIFT)));

        let big = entry.with_amount(0xFF);
        assert_eq!(big.amount(), 0x1F);

        assert_eq!(entry.with_visibility(0xFF).visibility(), 0x0F);
    }

    #[test]
    fn surface_map_indexing_roundtrip() {
        let mut surface = SurfaceMap::new();

        assert_eq!(surface.get(0, 0), Surface::NONE);
        assert!(surface.set(111, 111, Surface::LAND | Surface::COAST));
        assert_eq!(
            surface.get(111, 111),
            Surface::LAND | Surface::COAST,
            "tile bytes are a bitmask, not an exclusive enum"
        );
        assert_eq!(SurfaceMap::index(111, 111), Some(MAP_TILES - 1));
        assert_eq!(SurfaceMap::index(112, 0), None);
        assert_eq!(SurfaceMap::index(-1, 0), None);
        assert_eq!(SurfaceMap::index(0, 112), None);

        assert!(!surface.set(-1, 5, Surface::WATER));
        assert!(!surface.set(5, 112, Surface::WATER));
        assert_eq!(surface.get(-1, 5), Surface::NONE);
        assert_eq!(surface.get(5, 112), Surface::NONE);

        let bytes = surface.to_bytes();
        assert_eq!(bytes.len(), MAP_TILES);
        assert_eq!(bytes[MAP_TILES - 1], 0x05);
        assert_eq!(bytes[0], 0x00);
    }

    #[test]
    fn map_accessors_use_size_invariant() {
        let mut map = Map::new();

        assert!(map.surface.set(3, 4, Surface::WATER));
        assert!(map
            .resources
            .set(3, 4, ResourceMapEntry::new(7, Some(ResourceKind::Gold), 0)));

        assert_eq!(map.surface_at(3, 4), Surface::WATER);
        assert_eq!(map.resource_at(3, 4).amount(), 7);
        assert_eq!(map.index(3, 4), Some(4 * 112 + 3));
        assert_eq!(map.surface_at(112, 0), Surface::NONE);
        assert_eq!(map.resource_at(0, 112), ResourceMapEntry::NONE);
    }

    #[test]
    fn resource_map_counts_and_byte_layout() {
        let mut map = ResourceMap::new();

        map.set(0, 0, ResourceMapEntry::new(4, Some(ResourceKind::Raw), 0));
        map.set(1, 0, ResourceMapEntry::new(9, Some(ResourceKind::Fuel), 0));
        map.set(2, 0, ResourceMapEntry::new(2, Some(ResourceKind::Gold), 0));
        map.set(3, 0, ResourceMapEntry::new(0, None, 0xF));

        let counts = map.counts();

        assert_eq!(counts.raw_tiles, 1);
        assert_eq!(counts.raw_amount, 4);
        assert_eq!(counts.fuel_tiles, 1);
        assert_eq!(counts.fuel_amount, 9);
        assert_eq!(counts.gold_tiles, 1);
        assert_eq!(counts.gold_amount, 2);

        let bytes = map.to_le_bytes();
        assert_eq!(bytes.len(), MAP_TILES * 2);
        assert_eq!(&bytes[0..2], &0x0084u16.to_le_bytes());
        assert_eq!(&bytes[2..4], &0x0029u16.to_le_bytes());
        assert_eq!(&bytes[4..6], &0x0042u16.to_le_bytes());
        assert_eq!(&bytes[6..8], &0x3C00u16.to_le_bytes());
    }

    #[test]
    fn serde_impls_are_available_for_map_types() {
        // Сериализация — ручная (массивы на 12544 элемента не поддержаны
        // derive-макросом serde); проверяем, что impl'ы объявлены и пригодны
        // как trait-object.
        fn assert_serialize<T: Serialize>() {}
        fn assert_deserialize<T: for<'de> Deserialize<'de>>() {}

        assert_serialize::<Map>();
        assert_deserialize::<Map>();
        assert_serialize::<SurfaceMap>();
        assert_deserialize::<SurfaceMap>();
        assert_serialize::<ResourceMap>();
        assert_deserialize::<ResourceMap>();
        assert_serialize::<ResourceMapEntry>();
    }
}
