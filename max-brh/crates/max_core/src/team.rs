//! Teams: types, team state (`TeamInfo`), economy and technologies
//! (`TeamUnits`), complexes (`Complex`).
//!
//! Oracle: `max-cpp-port/src/ctinfo.hpp` (`CTInfo`, `ResearchTopic`),
//! `teamunits.hpp/.cpp` (`TeamUnits`), `complex.hpp/.cpp` (`Complex`),
//! `cargo.hpp/.cpp` (`Cargo`), `enums.hpp` (`PlayerTeamType`, `TeamClanType`).
//!
//! `S`/`UI` projection — per `max-brh-docs/state-model.md` §3.4/§3.6/§3.7 and §6.3: UI
//! fields of `TeamInfo` (camera, zoom, display toggles, markers) do not
//! enter the simulation; they are collected in [`TeamInfoUi`] and are needed only by the save layer (V70).

use serde::{Deserialize, Serialize};

use crate::enums::{TeamClan, RESEARCH_TOPIC_COUNT, UNIT_END};
use crate::geo::Point;
use crate::unit_values::{UnitValues, UNIT_COUNT};

// Leaf team types (`TeamId`, `TeamKind`, `Team`, `ResearchTopicInfo`,
// `ScreenLocation`, `Complex`/`ComplexId`, `Cargo`, `hash_team_id`,
// `RESEARCH_TOPICS`) live in the lower layer (`max_types`, F5.2); here they are
// re-exported so that the path `max_core::team::…` does not change.
pub use max_types::team::{
    hash_team_id, Cargo, Complex, ComplexId, ResearchTopicInfo, ScreenLocation, Team, TeamId,
    TeamKind, RESEARCH_TOPICS,
};

// Complex connectors and cargo types live in `enums` (P0); here they are re-exported
// so that the domain team/complex types are self-contained.
pub use crate::enums::{
    CARGO_FUEL, CARGO_GOLD, CARGO_MASK, CARGO_MATERIALS, CONNECTION_BEING_TESTED,
    CONNECTOR_EAST_BOTTOM, CONNECTOR_EAST_TOP, CONNECTOR_NORTH_LEFT, CONNECTOR_NORTH_RIGHT,
    CONNECTOR_SOUTH_LEFT, CONNECTOR_SOUTH_RIGHT, CONNECTOR_WEST_BOTTOM, CONNECTOR_WEST_TOP,
};

// ---------------------------------------------------------------------------
// TeamInfo helper structures
// ---------------------------------------------------------------------------

/// How many turns of the score graph are stored (`score_graph[50]`).
pub const SCORE_GRAPH_LEN: usize = 50;

/// Number of camera positions in V70 (`camera_positions[6]`).
pub const CAMERA_POSITION_COUNT: usize = 6;

/// Number of markers in V70 (`markers[10]`, the feature is cut).
pub const MARKER_COUNT: usize = 10;

/// Number of `TeamInfo` display toggles (UI).
pub const DISPLAY_BUTTON_COUNT: usize = 11;

/// Team state — C++ `struct CTInfo` (`ctinfo.hpp`), save.md `TeamInfo`.
///
/// `S` fields only (state-model §6.3); UI fields are in [`TeamInfoUi`].
///
/// `serde` derives are not emitted: arrays of 93/50 elements are not
/// supported by the standard `serde` array impls. The canonical
/// byte format of the state is `save.rs`/`crc.rs` (DESIGN §2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamInfo {
    /// Team type (`team_type`).
    pub team_type: TeamKind,
    /// The team has finished its turn.
    pub finished_turn: bool,
    /// Team clan.
    pub team_clan: TeamClan,
    /// Progress of 8 research topics.
    pub research_topics: [ResearchTopicInfo; RESEARCH_TOPIC_COUNT],
    /// Victory points.
    pub team_points: u32,
    /// Created-object counter 1..8191 — the basis for the hash id
    /// (`number_of_objects_created`).
    pub number_of_objects_created: u16,
    /// Per-type name counters — the production ordinal of a unit instance
    /// (C++ `CTInfo::unit_counters[UNIT_END]`, in V70 — `u8[93]`).
    ///
    /// The counter is per team **and** type, starts at 1 (`CTInfo::Reset`,
    /// `ctinfo.hpp:49-51`) and advances on every deployment
    /// ([`TeamInfo::take_team_unit_counter`]). The consumer is
    /// [`crate::unit::UnitInfo::unit_id`] (a `u8`), so the numbers an instance
    /// can get cycle through the u8 space — 256 values (V70 stores the counter
    /// as one byte, `saveload.cpp:839/852-856`).
    pub team_unit_counters: [u32; UNIT_COUNT],
    /// Team points for the last 50 turns.
    pub score_graph: [i16; SCORE_GRAPH_LEN],
    /// How many factories were built.
    pub stats_factories_built: i16,
    /// How many mines were built.
    pub stats_mines_built: i16,
    /// How many buildings were built.
    pub stats_buildings_built: i16,
    /// How many mobile units were built.
    pub stats_units_built: i16,
    /// Losses by type (V70 — u16[93], C++ — u32[93]).
    pub casualties: [u32; UNIT_COUNT],
    /// Gold spent on upgrades (V70 — i16, C++ — u32).
    pub stats_gold_spent_on_upgrades: u32,
}

impl Default for TeamInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamInfo {
    /// Analog of `CTInfo::Reset`: an empty slot, name counters = 1.
    pub fn new() -> Self {
        Self {
            team_type: TeamKind::None,
            finished_turn: false,
            team_clan: TeamClan::Random,
            research_topics: [ResearchTopicInfo::default(); RESEARCH_TOPIC_COUNT],
            team_points: 0,
            number_of_objects_created: 0,
            team_unit_counters: [1; UNIT_COUNT],
            score_graph: [0; SCORE_GRAPH_LEN],
            stats_factories_built: 0,
            stats_mines_built: 0,
            stats_buildings_built: 0,
            stats_units_built: 0,
            casualties: [0; UNIT_COUNT],
            stats_gold_spent_on_upgrades: 0,
        }
    }

    /// `CTInfo::Reset` — reset to the empty-slot state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// `team_unit_counters` counter for a type — the value the next instance of
    /// that type will carry in [`crate::unit::UnitInfo::unit_id`].
    ///
    /// Read-only: the counter is advanced only by [`Self::take_team_unit_counter`],
    /// the C++ `UnitsManager_DeployUnit` path.
    #[inline]
    pub fn team_unit_counter(&self, unit_type: u16) -> u32 {
        self.team_unit_counters[unit_type as usize]
    }

    /// `UnitsManager_DeployUnit` (`units_manager.cpp:2377-2379`): returns the
    /// instance number for the unit being deployed and advances the counter.
    ///
    /// ```c
    /// unit->unit_id = team_info->unit_counters[unit_type];
    /// ++team_info->unit_counters[unit_type];
    /// ```
    ///
    /// `unit_id` is a `u8` in both engines, so the returned number is the low
    /// byte of the counter: `1, 2, …, 255, 0, 1, …` — a 256-value cycle. The
    /// counter itself stays a `u32` like `CTInfo`; V70 only persists its low
    /// byte (`saveload.cpp:839/852-856`), which is exactly what the truncation
    /// gives.
    pub fn take_team_unit_counter(&mut self, unit_type: u16) -> u8 {
        let counter = &mut self.team_unit_counters[unit_type as usize];
        let unit_id = *counter as u8;
        *counter = counter.wrapping_add(1);

        unit_id
    }

    /// Losses by type (used by victory/defeat conditions).
    #[inline]
    pub fn casualties_for(&self, unit_type: u16) -> u32 {
        self.casualties[unit_type as usize]
    }
}

/// `TeamInfo` UI fields from V70 (state-model §6.3 — they do not enter the snapshot/simulation).
///
/// Needed only for the byte-exact save round-trip: in the original, a snapshot of the screen
/// ended up in the file because the whole process is saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamInfoUi {
    /// Map markers (`markers[10]`), the feature is cut — always {-1,-1}.
    pub markers: [Point; MARKER_COUNT],
    /// Saved camera positions F5–F8 (`camera_positions[6]`).
    pub screen_locations: [ScreenLocation; CAMERA_POSITION_COUNT],
    /// Selected unit (`selected_unit`, 0xFFFF = none).
    pub selected_unit: Option<u16>,
    /// Map zoom 4–64.
    pub zoom_level: u16,
    /// Screen center.
    pub camera_position: Point,
    /// 11 display toggles (`display_button_*`).
    pub display_buttons: [i8; DISPLAY_BUTTON_COUNT],
}

impl Default for TeamInfoUi {
    fn default() -> Self {
        Self {
            markers: [Point::new(-1, -1); MARKER_COUNT],
            screen_locations: [ScreenLocation { x: -1, y: -1 }; CAMERA_POSITION_COUNT],
            selected_unit: None,
            zoom_level: 0,
            camera_position: Point::ZERO,
            display_buttons: [0; DISPLAY_BUTTON_COUNT],
        }
    }
}

// ---------------------------------------------------------------------------
// TeamUnits
// ---------------------------------------------------------------------------

/// Team economy and technologies — C++ `class TeamUnits` (`teamunits.hpp`).
///
/// In C++ these are the global objects `ResourceManager_TeamUnitsRed/Green/Blue/Gray`;
/// in `max_core` — a `GameState` element indexed by team.
///
/// `serde` derives are not emitted because of the 93-element arrays (see
/// [`TeamInfo`]); the byte format is `save.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamUnits {
    /// Team credits (C++ `uint32_t`, in V70 — i16/u16).
    pub gold: u32,
    /// Base attributes (clan-adjusted) — 93 types.
    pub base_values: [UnitValues; UNIT_COUNT],
    /// Current attributes (after research) — units are created from them.
    pub current_values: [UnitValues; UNIT_COUNT],
    /// Team complexes — `SmartList` order (ascending id).
    pub complexes: Vec<Complex>,
    /// `hash_team_id` — a service field of C++ (not serialized).
    pub hash_team_id: u16,
}

impl Default for TeamUnits {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamUnits {
    /// Empty team: default `UnitValues()` values.
    pub fn new() -> Self {
        Self {
            gold: 0,
            base_values: [UnitValues::new(); UNIT_COUNT],
            current_values: [UnitValues::new(); UNIT_COUNT],
            complexes: Vec::new(),
            hash_team_id: 0,
        }
    }

    /// A team initialized with the clan-adjusted attribute table —
    /// C++ `TeamUnits::Init` + `ResourceManager_InitClanUnitValues`
    /// (base and current reference equal values until the first upgrades).
    pub fn with_clan(clan: TeamClan) -> Self {
        let mut units = Self::new();
        units.init_clan_unit_values(clan);
        units
    }

    /// Rebuilds the team attribute table for a clan —
    /// C++ `ResourceManager_InitClanUnitValues` (`resource_manager.cpp:1249`).
    ///
    /// Repeats the C++ order: `ClearComplexes()` → `TeamUnits::Init()` (a clean
    /// `SC_ATTRI` base, not the previous table) → `SetClanUpgradesFromClans` →
    /// the result is written to both `base_values` and `current_values`.
    /// Because the base is rebuilt from scratch, repeated application is
    /// idempotent: clan deltas never stack on top of a previous clan.
    pub fn init_clan_unit_values(&mut self, clan: TeamClan) {
        self.clear_complexes();
        let values = crate::unit_values::team_base_values(clan);
        self.base_values = values;
        self.current_values = values;
    }

    /// `TeamUnits::GetGold`.
    #[inline]
    pub const fn gold(&self) -> u32 {
        self.gold
    }

    /// `TeamUnits::SetGold`.
    #[inline]
    pub fn set_gold(&mut self, value: u32) {
        self.gold = value;
    }

    /// `TeamUnits::GetBaseUnitValues`.
    #[inline]
    pub fn base_unit_values(&self, unit_type: u16) -> &UnitValues {
        &self.base_values[unit_type as usize]
    }

    /// `TeamUnits::GetCurrentUnitValues`.
    #[inline]
    pub fn current_unit_values(&self, unit_type: u16) -> &UnitValues {
        &self.current_values[unit_type as usize]
    }

    /// `TeamUnits::GetBaseUnitValues` (mut).
    #[inline]
    pub fn base_unit_values_mut(&mut self, unit_type: u16) -> &mut UnitValues {
        &mut self.base_values[unit_type as usize]
    }

    /// `TeamUnits::GetCurrentUnitValues` (mut).
    #[inline]
    pub fn current_unit_values_mut(&mut self, unit_type: u16) -> &mut UnitValues {
        &mut self.current_values[unit_type as usize]
    }

    /// `TeamUnits::CreateComplex`: inserts a complex with the lowest free id,
    /// preserving the id sort of the list (C++ `InsertBefore` at the first gap).
    pub fn create_complex(&mut self) -> ComplexId {
        let mut complex_id: i16 = 1;
        let mut index = 0usize;
        while index < self.complexes.len() && self.complexes[index].id == complex_id {
            complex_id = complex_id.wrapping_add(1);
            index += 1;
        }
        let complex = Complex::new(complex_id);
        self.complexes.insert(index, complex);
        ComplexId(complex_id)
    }

    /// `TeamUnits::GetComplex`.
    pub fn get_complex(&self, complex_id: ComplexId) -> Option<&Complex> {
        self.complexes.iter().find(|item| item.id == complex_id.0)
    }

    /// `TeamUnits::GetComplex` (mut).
    pub fn get_complex_mut(&mut self, complex_id: ComplexId) -> Option<&mut Complex> {
        self.complexes.iter_mut().find(|item| item.id == complex_id.0)
    }

    /// `TeamUnits::GetComplexes`.
    #[inline]
    pub fn complexes(&self) -> &[Complex] {
        &self.complexes
    }

    /// `TeamUnits::RemoveComplex`.
    pub fn remove_complex(&mut self, complex_id: ComplexId) -> bool {
        let before = self.complexes.len();
        self.complexes.retain(|item| item.id != complex_id.0);
        self.complexes.len() != before
    }

    /// `TeamUnits::ClearComplexes`.
    pub fn clear_complexes(&mut self) {
        self.complexes.clear();
    }
}

/// Number of unit types for team counters (`UNIT_END`).
pub const TEAM_UNIT_TYPE_COUNT: u16 = UNIT_END;

#[cfg(test)]
#[path = "team_tests.rs"]
mod tests;
