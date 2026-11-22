//! World map: surface (passability) and resources/visibility per tile.
//!
//! Port of the oracle structures `max-cpp-port/src`:
//! * `SurfaceMap` — `uint8_t[112*112]` from `World::LoadTilesAndMetadata()`
//!   (`world.cpp`, "Load pass table and build surface map"): one byte per cell,
//!   the value is the [`Surface`] bit mask (`SURFACE_TYPE_*`, `enums.hpp`);
//! * `ResourceManager_CargoMap` — `uint16_t[112*112]` (`resource_manager.hpp`),
//!   it is also `short GridResourceMapEntry[112][112]` from state-model.md §3.10:
//!   the bitfield "amount (0–4) | type (5–7) | team visibility (10–13)".
//!
//! The layout invariants are taken from state-model.md §3.10 and §7.3:
//! * bits 0–4 (`0x001F`) — resource amount (0..=15 by the mask; the game writes up to 16);
//! * bits 5–7 (`0x00E0`) — exactly one type bit (`CARGO_FUEL`/`CARGO_GOLD`/
//!   `CARGO_MATERIALS`), 0 — an empty tile;
//! * bits 10–13 (`0x3C00`) — surveyed by team `i` in bit `i`
//!   (save.md declares the mask `0x1C00`, but it does not cover the reds —
//!   the actual mask is `0x3C00`, DESIGN.md risk 5 / state-model §7.3).
//!
//! The module does not do I/O: both the surface and the resources are ordinary
//! fixed-size arrays [`MAP_TILES`] filled by `max_core::setup` (match generation)
//! or by the save loader (`max_core::save`, outside this crate).

use core::fmt;

use serde::de::{Error as DeError, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::enums::{Surface, CARGO_FUEL, CARGO_GOLD, CARGO_MASK, CARGO_MATERIALS};
use crate::geo::{MAP_HEIGHT, MAP_TILES, MAP_WIDTH};

/// Map dimensions in tiles — skeleton `map::MapSize` (`Point` in C++).
///
/// In M.A.X. the size is taken from the planet's `.wrl` file; for the stock planets it is 112×112
/// ([`crate::geo::MAP_SIZE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MapSize {
    /// Width in tiles.
    pub width: i16,
    /// Height in tiles.
    pub height: i16,
}

impl MapSize {
    /// Stock map size (`MAP_SIZE`).
    pub const STANDARD: Self = Self {
        width: MAP_WIDTH,
        height: MAP_HEIGHT,
    };

    /// Number of tiles (`width * height`).
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
// Resources: CARGO_* and ResourceMapEntry
// ---------------------------------------------------------------------------

/// Resource amount mask — bits 0–4 (`RESOURCE_AMOUNT_MASK`).
pub const RESOURCE_AMOUNT_MASK: u16 = CARGO_MASK as u16; // 0x001F
/// Resource type mask — bits 5–7 (`RESOURCE_TYPE_MASK`).
pub const RESOURCE_TYPE_MASK: u16 = (CARGO_FUEL | CARGO_GOLD | CARGO_MATERIALS) as u16; // 0x00E0
/// Bit shift of the team visibility.
pub const TEAM_VISIBILITY_SHIFT: u16 = 10;
/// Visibility mask of all four playable teams (`0x3C00`, see §7.3).
pub const TEAM_VISIBILITY_MASK: u16 = 0x3C00;
/// Visibility mask from save.md (`0x1C00`, does not cover the reds — kept for
/// comparison with V70).
pub const TEAM_VISIBILITY_MASK_SAVE: u16 = 0x1C00;
/// Number of playable teams that have a survey bit.
pub const VISIBILITY_TEAM_COUNT: u8 = 4;

/// Team bit in the resource survey — C++ `hash_team_id`
/// (`enums.hpp:40-43`): `HASH_TEAM_RED = 0x2000` (bit 13),
/// `GREEN = 0x1000` (12), `BLUE = 0x0800` (11), `GRAY = 0x0400` (10).
///
/// The order is **reverse** to the team number: in `ResourceManager_CargoMap`
/// the reds are marked by the highest bit of the mask `0x3C00`. This is exactly how
/// `Survey_SurveyArea` (`survey.cpp:116`) writes `CargoMap |= team_units->hash_team_id`,
/// and `Survey_RenderMarkers` (`survey.cpp:143-150`) reads `CargoMap & mask`.
/// Returns the mask **inside** the 4-bit field [`visibility`]
/// ([`ResourceMapEntry::visibility`]).
#[inline]
pub const fn team_visibility_bit(team: u8) -> u8 {
    if team < VISIBILITY_TEAM_COUNT {
        1 << (VISIBILITY_TEAM_COUNT - 1 - team)
    } else {
        0
    }
}

/// Resource type — bits 5–7 of `GridResourceMapEntry` (`CARGO_*` in `enums.hpp`).
///
/// The order of variants is kept from the skeleton (`Raw`, `Fuel`, `Gold`); the values are
/// the actual type bits in `ResourceManager_CargoMap`, so that the
/// "bitfield ⇄ enum" conversion needs no tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum ResourceKind {
    /// Materials — `CARGO_MATERIALS` (0x80).
    Raw = CARGO_MATERIALS as u16,
    /// Fuel — `CARGO_FUEL` (0x20).
    Fuel = CARGO_FUEL as u16,
    /// Gold — `CARGO_GOLD` (0x40).
    Gold = CARGO_GOLD as u16,
}

impl ResourceKind {
    /// All types in C++ order (`CARGO_MATERIALS`, `CARGO_FUEL`, `CARGO_GOLD`).
    pub const ALL: [Self; 3] = [Self::Raw, Self::Fuel, Self::Gold];

    /// Type bits in `GridResourceMapEntry`.
    #[inline]
    pub const fn cargo_bits(self) -> u16 {
        self as u16
    }

    /// Decodes the type bits (5–7); `None` if exactly one bit is not set.
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

/// One tile of the resource map — `short GridResourceMapEntry` (`uint16_t`).
///
/// A bitfield rather than a struct of fields: the layout must match
/// `ResourceManager_CargoMap` byte for byte (state-model.md §3.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceMapEntry(u16);

impl ResourceMapEntry {
    /// Empty tile (`CARGO_MATERIALS` not set, amount 0).
    pub const NONE: Self = Self(0);

    /// Creates an entry from a raw bitfield (as `CargoMap[...]`).
    #[inline]
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// Creates an entry from fields; `amount` is masked with `0x1F`, `visibility` with `0x0F`.
    #[inline]
    pub const fn new(amount: u8, kind: Option<ResourceKind>, visibility: u8) -> Self {
        let mut bits = (amount as u16) & RESOURCE_AMOUNT_MASK;

        if let Some(kind) = kind {
            bits |= kind.cargo_bits();
        }

        bits |= ((visibility & 0x0F) as u16) << TEAM_VISIBILITY_SHIFT;

        Self(bits)
    }

    /// Raw bitfield (for dumps/saves).
    #[inline]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Resource amount — bits 0–4 (`& 0x1F`, as in `resource_allocator.cpp`).
    #[inline]
    pub const fn amount(self) -> u8 {
        (self.0 & RESOURCE_AMOUNT_MASK) as u8
    }

    /// Resource type — bits 5–7; `None` if the type is not set.
    #[inline]
    pub const fn resource_type(self) -> Option<ResourceKind> {
        ResourceKind::from_cargo_bits(self.0)
    }

    /// Survey: bits 10–13, order as in C++ `HASH_TEAM_*` (see
    /// [`team_visibility_bit`]): RED is the highest bit (13), GRAY the lowest (10).
    #[inline]
    pub const fn visibility(self) -> u8 {
        ((self.0 & TEAM_VISIBILITY_MASK) >> TEAM_VISIBILITY_SHIFT) as u8
    }

    /// Whether team `team` (0..=3) has surveyed the tile.
    #[inline]
    pub const fn is_visible_to(self, team: u8) -> bool {
        team < VISIBILITY_TEAM_COUNT && (self.visibility() & team_visibility_bit(team)) != 0
    }

    /// A copy of the entry with the knowledge of team `team` added (port of
    /// `ResourceManager_CargoMap[...] |= hash_team_id`, `survey.cpp:116`).
    #[inline]
    pub const fn with_team_visible(self, team: u8) -> Self {
        if team >= VISIBILITY_TEAM_COUNT {
            return self;
        }

        Self(self.0 | ((team_visibility_bit(team) as u16) << TEAM_VISIBILITY_SHIFT))
    }

    /// Empty tile (no type, no amount, no survey).
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// A copy of the entry with a different amount (bits 0–4).
    #[inline]
    pub const fn with_amount(self, amount: u8) -> Self {
        Self((self.0 & !RESOURCE_AMOUNT_MASK) | ((amount as u16) & RESOURCE_AMOUNT_MASK))
    }

    /// A copy of the entry with a different type (bits 5–7 are overwritten).
    #[inline]
    pub const fn with_kind(self, kind: Option<ResourceKind>) -> Self {
        let type_bits = match kind {
            Some(kind) => kind.cargo_bits(),
            None => 0,
        };

        Self((self.0 & !RESOURCE_TYPE_MASK) | type_bits)
    }

    /// A copy of the entry with a different survey (bits 10–13).
    #[inline]
    pub const fn with_visibility(self, visibility: u8) -> Self {
        Self(
            (self.0 & !TEAM_VISIBILITY_MASK)
                | (((visibility & 0x0F) as u16) << TEAM_VISIBILITY_SHIFT),
        )
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

/// Passability map — `SurfaceMap` (one byte per cell, [`MAP_TILES`] tiles).
///
/// Indexing as throughout the C++ port: `index = y * width + x`.
#[derive(Clone, PartialEq, Eq)]
pub struct SurfaceMap {
    /// Surface bytes in the order `y * MAP_WIDTH + x`.
    tiles: [Surface; MAP_TILES],
}

impl SurfaceMap {
    /// Empty map (all tiles [`Surface::NONE`]).
    #[inline]
    pub const fn new() -> Self {
        Self {
            tiles: [Surface::NONE; MAP_TILES],
        }
    }

    /// Map from a ready byte array (`World::GetSurfaceMap()`).
    #[inline]
    pub const fn from_tiles(tiles: [Surface; MAP_TILES]) -> Self {
        Self { tiles }
    }

    /// All tiles (order `y * MAP_WIDTH + x`).
    #[inline]
    pub fn tiles(&self) -> &[Surface; MAP_TILES] {
        &self.tiles
    }

    /// Mutable access to all tiles.
    #[inline]
    pub fn tiles_mut(&mut self) -> &mut [Surface; MAP_TILES] {
        &mut self.tiles
    }

    /// Tile index `(x, y)` or `None` outside the map.
    #[inline]
    pub fn index(x: i16, y: i16) -> Option<usize> {
        if x < 0 || y < 0 || x >= MAP_WIDTH || y >= MAP_HEIGHT {
            None
        } else {
            Some(y as usize * MAP_WIDTH as usize + x as usize)
        }
    }

    /// Tile surface; outside the map — [`Surface::NONE`] (as `World::GetSurfaceType`).
    #[inline]
    pub fn get(&self, x: i16, y: i16) -> Surface {
        match Self::index(x, y) {
            Some(index) => self.tiles[index],
            None => Surface::NONE,
        }
    }

    /// Writes the tile surface; `false` if the coordinates are outside the map.
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

    /// Raw map bytes (dump of reference vectors, V70 save).
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
        // A full 112×112 dump is unreadable in Debug; show a histogram instead.
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

/// Resource map — `ResourceManager_CargoMap` (`uint16_t[MAP_TILES]`).
#[derive(Clone, PartialEq, Eq)]
pub struct ResourceMap {
    /// Resource bitfields in the order `y * MAP_WIDTH + x`.
    tiles: [ResourceMapEntry; MAP_TILES],
}

impl ResourceMap {
    /// Empty map (all tiles [`ResourceMapEntry::NONE`]).
    #[inline]
    pub const fn new() -> Self {
        Self {
            tiles: [ResourceMapEntry::NONE; MAP_TILES],
        }
    }

    /// Map from a ready array of entries.
    #[inline]
    pub const fn from_tiles(tiles: [ResourceMapEntry; MAP_TILES]) -> Self {
        Self { tiles }
    }

    /// All tiles (order `y * MAP_WIDTH + x`).
    #[inline]
    pub fn tiles(&self) -> &[ResourceMapEntry; MAP_TILES] {
        &self.tiles
    }

    /// Mutable access to all tiles.
    #[inline]
    pub fn tiles_mut(&mut self) -> &mut [ResourceMapEntry; MAP_TILES] {
        &mut self.tiles
    }

    /// Tile index `(x, y)` or `None` outside the map.
    #[inline]
    pub fn index(x: i16, y: i16) -> Option<usize> {
        SurfaceMap::index(x, y)
    }

    /// Tile entry; outside the map — [`ResourceMapEntry::NONE`].
    #[inline]
    pub fn get(&self, x: i16, y: i16) -> ResourceMapEntry {
        match Self::index(x, y) {
            Some(index) => self.tiles[index],
            None => ResourceMapEntry::NONE,
        }
    }

    /// Writes the tile; `false` if the coordinates are outside the map.
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

    /// Raw tile bitfield by index (`ResourceManager_CargoMap[index]`).
    ///
    /// Resource generation (`resource_allocator.cpp`) works with the flat index
    /// `y * MAP_WIDTH + x`, so indexed access is part of the contract.
    #[inline]
    pub fn bits_at(&self, index: usize) -> u16 {
        self.tiles[index].bits()
    }

    /// Writes the raw bitfield by index (`CargoMap[index] = value`).
    #[inline]
    pub fn set_bits_at(&mut self, index: usize, bits: u16) {
        self.tiles[index] = ResourceMapEntry::from_bits(bits);
    }

    /// Number of resources of each type (for property tests and debugging).
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

    /// Raw map bytes in little-endian (dump of reference vectors, `short`V70).
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

/// Summary of the map resources (`ResourceMap::counts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceCounts {
    /// Tiles with materials.
    pub raw_tiles: u32,
    /// Total amount of materials.
    pub raw_amount: u32,
    /// Tiles with fuel.
    pub fuel_tiles: u32,
    /// Total amount of fuel.
    pub fuel_amount: u32,
    /// Tiles with gold.
    pub gold_tiles: u32,
    /// Total amount of gold.
    pub gold_amount: u32,
}

// ---------------------------------------------------------------------------
// Map
// ---------------------------------------------------------------------------

/// Match map: size + surface + resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    /// Map size (from the planet's `.wrl` file; 112×112 as standard).
    pub size: MapSize,
    /// Passability per tile.
    pub surface: SurfaceMap,
    /// Resources and survey per tile.
    pub resources: ResourceMap,
}

impl Map {
    /// Empty map of the stock size.
    #[inline]
    pub const fn new() -> Self {
        Self {
            size: MapSize::STANDARD,
            surface: SurfaceMap::new(),
            resources: ResourceMap::new(),
        }
    }

    /// Map from surface and resources.
    #[inline]
    pub const fn from_parts(surface: SurfaceMap, resources: ResourceMap) -> Self {
        Self {
            size: MapSize::STANDARD,
            surface,
            resources,
        }
    }

    /// Tile index `(x, y)` or `None` outside the map.
    #[inline]
    pub fn index(&self, x: i16, y: i16) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.size.width || y >= self.size.height {
            None
        } else {
            Some(y as usize * self.size.width as usize + x as usize)
        }
    }

    /// Tile surface; outside the map — [`Surface::NONE`].
    #[inline]
    pub fn surface_at(&self, x: i16, y: i16) -> Surface {
        match self.index(x, y) {
            Some(index) => self.surface.tiles()[index],
            None => Surface::NONE,
        }
    }

    /// Tile resource entry; outside the map — [`ResourceMapEntry::NONE`].
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
// Tests
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
        // Example from the state-model.md §3.10 / save.md: 0x208C = 12 materials,
        // surveyed only by the reds. The reds are `HASH_TEAM_RED = 0x2000`
        // (bit 13), i.e. `visibility() == 0b1000` — that is how
        // `Survey_SurveyArea` (`survey.cpp:116`).
        let entry = ResourceMapEntry::new(12, Some(ResourceKind::Raw), 0b1000);

        assert_eq!(entry.bits(), 0x208C);
        assert_eq!(entry.amount(), 12);
        assert_eq!(entry.resource_type(), Some(ResourceKind::Raw));
        assert_eq!(entry.visibility(), 0b1000);
        assert!(entry.is_visible_to(0), "0x2000 — бит красных (команда 0)");
        assert!(!entry.is_visible_to(3));

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

        // Two type bits at once — an invalid tile, the type is not decoded.
        let invalid = ResourceMapEntry::from_bits(CARGO_FUEL as u16 | CARGO_GOLD as u16);
        assert_eq!(invalid.resource_type(), None);

        for team in 0..=3u8 {
            let entry = ResourceMapEntry::from_bits(
                (team_visibility_bit(team) as u16) << TEAM_VISIBILITY_SHIFT,
            );
            assert_eq!(entry.visibility(), team_visibility_bit(team));
            assert_eq!(entry.resource_type(), None);
            assert!(entry.is_visible_to(team));

            // C++ `hash_team_id`: the reds are the highest bit of the field (13), the grays
            // the lowest (10).
            assert_eq!(
                entry.bits(),
                match team {
                    0 => 0x2000,
                    1 => 0x1000,
                    2 => 0x0800,
                    _ => 0x0400,
                },
                "бит команды {team} — HASH_TEAM_* из enums.hpp:40-43"
            );
        }

        // Knowledge is added without losing the amount/type and without duplicates.
        let entry = ResourceMapEntry::new(7, Some(ResourceKind::Gold), 0).with_team_visible(0);
        assert_eq!(entry.amount(), 7);
        assert_eq!(entry.resource_type(), Some(ResourceKind::Gold));
        assert!(entry.is_visible_to(0));
        assert!(!entry.is_visible_to(1));
        assert_eq!(
            entry.with_team_visible(0),
            entry,
            "повторная отметка идемпотентна"
        );
        assert!(entry.with_team_visible(1).is_visible_to(1));

        // Bits 14/15 are not part of the visibility mask.
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
        // Serialization is manual (arrays of 12544 elements are not supported
        // by the serde derive macro); we check that the impls are declared and usable
        // as a trait object.
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
