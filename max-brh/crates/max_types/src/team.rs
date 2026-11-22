//! Leaf team types (Infrastructure).
//!
//! Only types that do not read `max_core` data: [`TeamId`], [`TeamKind`]
//! and [`Team`]. Team state (`TeamInfo`, `TeamInfoUi`), economy
//! (`TeamUnits`) and the attribute table (`UnitValues`) stay in
//! `max_core::team` — they depend on simulation data.
//!
//! Oracle: `max-cpp-port/src/enums.hpp` (`PlayerTeamType`, `TeamClanType`),
//! `ctinfo.hpp` (`CTInfo`), `teamunits.hpp` (`TeamUnits`, `Complex`, `Cargo`).

use serde::{Deserialize, Serialize};

use crate::enums::{ResearchTopic, RESEARCH_TOPIC_COUNT};

/// Team identifier (0-3 in the original, 4 = aliens).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TeamId(pub u8);

/// Who controls the team (C++ `PlayerType`, save.md `TeamType`).
///
/// Values are fixed as in C++/V70: `None=0`, `Player=1` (HUMAN),
/// `Computer=2`, `Remote=3`, `Eliminated=4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TeamKind {
    /// Empty slot.
    None = 0,
    /// Local player (`TEAM_TYPE_HUMAN`).
    Player = 1,
    /// Computer (the AI is simulated by the server).
    Computer = 2,
    /// Remote player (another client).
    Remote = 3,
    /// Eliminated.
    Eliminated = 4,
}

/// Team — the minimal skeleton for consumers (`max_net`, `max_render`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    /// Team index.
    pub id: TeamId,
    /// Team type.
    pub kind: TeamKind,
    /// Team name.
    pub name: String,
    /// Team clan (raw `TeamClan` value).
    pub clan: u8,
    /// Team gold.
    pub gold: i32,
    /// Starting credits.
    pub start_gold: i32,
}

impl Team {
    /// Creates a team with default settings.
    pub fn new(id: TeamId, kind: TeamKind) -> Self {
        Self {
            id,
            kind,
            name: String::new(),
            clan: 0,
            gold: 0,
            start_gold: 0,
        }
    }
}

/// `TeamUnits::hash_team_id` — the team color bit in the unit flags.
///
/// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:179`) assembles the flags as
/// `unit_flags | UnitsManager_TeamInfo[team].team_units->hash_team_id`, and
/// `hash_team_id` is assigned to slots in `ResourceManager_InitTeamUnits`
/// (`resource_manager.cpp:969-981`): RED = `HASH_TEAM_RED` 0x2000,
/// GREEN = 0x1000, BLUE = 0x800, GRAY = 0x400, ALIEN/derelict = 0x8000.
/// These very bits are read by the `COLORS` frame (`DrawMap_RenderUnitDisplays`,
/// `drawmap.cpp:581-598`), the minimap (`drawmap.cpp:844-858`) and the sprite
/// recolor (`unitinfo.cpp:3112-3126`), while `team_clan` affects only the frames
/// of the mine (`unitinfo.cpp:282`) and the clan trade modifiers.
///
/// The port does **not** write these bits into `UnitInfo::flags` on spawn (the flag
/// set is part of the canonical digest, and the reference C++ vector was built without them) —
/// the rendering layer gets the same color from this mapping
/// (`max_app::overlays::team_color_index`), and the bits from loaded saves are
/// honoured directly.
pub const fn hash_team_id(team: TeamId) -> u16 {
    use crate::enums::unit_flags;

    match team.0 {
        0 => unit_flags::HASH_TEAM_RED as u16,
        1 => unit_flags::HASH_TEAM_GREEN as u16,
        2 => unit_flags::HASH_TEAM_BLUE as u16,
        3 => unit_flags::HASH_TEAM_GRAY as u16,
        _ => unit_flags::HASH_TEAM_DERELICT as u16,
    }
}

/// Screen position — C++ `struct ScreenLocation` (`ctinfo.hpp`, UI).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenLocation {
    /// X coordinate.
    pub x: i8,
    /// Y coordinate.
    pub y: i8,
}

/// Research progress — C++ `struct ResearchTopic` (`ctinfo.hpp`),
/// save.md `ResearchTopicInfo`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchTopicInfo {
    /// Completed research cycles.
    pub research_level: u32,
    /// Turns per cycle with one research center (0 = not started).
    pub turns_to_complete: u32,
    /// How many research centers are allocated.
    pub allocation: u32,
}

impl ResearchTopicInfo {
    /// Actual number of turns until completion:
    /// `ceil(turns_to_complete / allocation)` (`researchmenu.cpp:448`:
    /// `(allocation + turns_to_complete - 1) / allocation`); 0 when `allocation == 0`.
    pub fn turns_remaining(&self) -> u32 {
        if self.allocation == 0 {
            0
        } else {
            (self.allocation + self.turns_to_complete - 1) / self.allocation
        }
    }

    /// Progress of one turn — C++ `game_manager.cpp:2072` /
    /// `ResearchMenu_UpdateResearchProgress(team, topic, 0)`: if the topic
    /// is allocated but progress has not started, it is recomputed by the simulation (P4).
    pub fn is_started(&self) -> bool {
        self.turns_to_complete > 0
    }
}

/// Complex identifier (`Complex::id`, `int16_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComplexId(pub i16);

/// Building complex — C++ `class Complex : FileObject` (`complex.hpp`).
///
/// A complex is a connected group of buildings sharing power and cargo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Complex {
    /// Raw materials (stock + harvest) — DSL `raw` (C++ `Complex::material`).
    pub raw: i16,
    /// Fuel.
    pub fuel: i16,
    /// Gold.
    pub gold: i16,
    /// Power surplus.
    pub power: i16,
    /// Free workforce.
    pub workers: i16,
    /// Buildings in the complex.
    pub buildings: i16,
    /// Unique complex index within the team.
    pub id: i16,
}

impl Complex {
    /// `Complex::Complex(id)`: all stocks 0, `buildings = 0`.
    pub const fn new(id: i16) -> Self {
        Self {
            raw: 0,
            fuel: 0,
            gold: 0,
            power: 0,
            workers: 0,
            buildings: 0,
            id,
        }
    }

    /// Complex identifier.
    #[inline]
    pub const fn complex_id(&self) -> ComplexId {
        ComplexId(self.id)
    }

    /// `Complex::Grow` — a building joined the complex.
    pub fn grow(&mut self) {
        self.buildings = self.buildings.wrapping_add(1);
    }

    /// `Complex::Shrink` — a building disconnected; `true` if the complex became empty
    /// (C++ removes it from the team list).
    pub fn shrink(&mut self) -> bool {
        self.buildings = self.buildings.wrapping_sub(1);
        self.buildings == 0
    }
}

/// Cargo/stock — C++ `class Cargo` (`cargo.hpp`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cargo {
    /// Materials.
    pub raw: i32,
    /// Fuel.
    pub fuel: i32,
    /// Gold.
    pub gold: i32,
    /// Free capacity.
    pub free_capacity: i32,
}

impl Cargo {
    /// `Cargo::Init` — all fields 0.
    #[inline]
    pub const fn init() -> Self {
        Self {
            raw: 0,
            fuel: 0,
            gold: 0,
            free_capacity: 0,
        }
    }
}

/// Research topics in `ATTRIB_*` order (for iterating over the 8 topics).
pub const RESEARCH_TOPICS: [ResearchTopic; RESEARCH_TOPIC_COUNT] = [
    ResearchTopic::Attack,
    ResearchTopic::Shots,
    ResearchTopic::Range,
    ResearchTopic::Armor,
    ResearchTopic::Hits,
    ResearchTopic::Speed,
    ResearchTopic::Scan,
    ResearchTopic::Cost,
];
