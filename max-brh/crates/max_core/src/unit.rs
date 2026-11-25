//! Units: identifiers, paths, `UnitInfo` (simulation fields).
//!
//! Oracle: `max-cpp-port/src/unitinfo.hpp` / `unitinfo.cpp` (`class UnitInfo`,
//! `FileLoad`/`FileSave`), `unitpath.hpp`, `groundpath.hpp`, `airpath.hpp`,
//! `builderpath.hpp` (path data), `point.hpp` (`PathStep`), `unit.hpp`
//! (flags/`SfxType`).
//!
//! Field projection — `max-brh-docs/state-model.md` §3.9/§6.4. `UnitInfo`
//! keeps the simulation fields (including the frame state of orders and pixel
//! `x`/`y`). The presentation fields of C++ `UnitInfo` (shadow, sprite bounds,
//! brightness, sound, turret offsets/frames, cursor, bobbing) are moved to
//! [`max_types::unit::UnitPresentation`] (F5.3) and stored in parallel to
//! the slots in `GameState`; `save.rs` reads/writes them in the same V70 order as
//! before (byte-exact round-trip).
//!
//! An intentional exception is the pixel `x`/`y`: they participate in the simulation
//! (pixel step of `AirPath`, line-of-fire check `Access_IsWithinAttackRange`,
//! `UnitInfo::Move`) and therefore stay in `UnitInfo`.

use serde::{Deserialize, Serialize};

use crate::enums::{ResearchTopic, UnitOrder, UnitOrderState, UnitType};
use crate::geo::{PathStep, Point};
use crate::team::{ComplexId, TeamId};
use crate::unit_values::UnitValues;

// `UnitId`, `UnitListKind`, `Rect` and `UnitPresentation` are leaf types
// of the lower layer (`max_types`, F5.2/F5.3); here they are re-exported so that the path
// `max_core::unit::…` does not change.
pub use max_types::unit::{Rect, UnitId, UnitListKind, UnitPresentation};

// ---------------------------------------------------------------------------
// Paths (UnitPath)
// ---------------------------------------------------------------------------

/// Ground path — C++ `class GroundPath : UnitPath` (`groundpath.hpp`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundPath {
    /// End cell X (`UnitPath::m_end_x`).
    pub end_x: i16,
    /// End cell Y (`UnitPath::m_end_y`).
    pub end_y: i16,
    /// Current step index (u16 in V70, `uint32_t` in C++).
    pub step_index: u32,
    /// Path steps in traversal order.
    pub steps: Vec<PathStep>,
}

/// Air path — C++ `class AirPath : UnitPath` (`airpath.hpp`) — a straight
/// segment in pixel coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirPath {
    /// Length (`m_length`).
    pub length: i16,
    /// Angle (`m_angle`, `char` = i8).
    pub angle: i8,
    /// Start X (`m_start_x`).
    pub start_x: i16,
    /// Start Y (`m_start_y`).
    pub start_y: i16,
    /// End X.
    pub end_x: i16,
    /// End Y.
    pub end_y: i16,
    /// Step X (`m_step_x`).
    pub step_x: i32,
    /// Step Y (`m_step_y`).
    pub step_y: i32,
    /// Delta X (`m_delta_x`).
    pub delta_x: i32,
    /// Delta Y (`m_delta_y`).
    pub delta_y: i32,
}

/// Builder path — C++ `class BuilderPath : UnitPath` (`builderpath.hpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuilderPath {
    /// Direction X (`m_direction_x`).
    pub direction_x: i16,
    /// Direction Y (`m_direction_y`).
    pub direction_y: i16,
}

impl Default for BuilderPath {
    /// C++ `BuilderPath::BuilderPath` — direction (1, 1).
    fn default() -> Self {
        Self {
            direction_x: 1,
            direction_y: 1,
        }
    }
}

/// Unit path — C++ `UnitPath` (three implementations).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitPath {
    /// `AirPath`.
    Air(AirPath),
    /// `BuilderPath`.
    Builder(BuilderPath),
    /// `GroundPath`.
    Ground(GroundPath),
}

impl UnitPath {
    /// End cell (`UnitPath::GetEndX/GetEndY`).
    #[inline]
    pub fn end(&self) -> (i16, i16) {
        match self {
            Self::Air(path) => (path.end_x, path.end_y),
            Self::Builder(path) => (i16::from(path.direction_x), i16::from(path.direction_y)),
            Self::Ground(path) => (path.end_x, path.end_y),
        }
    }

    /// For a ground path — the number of remaining steps.
    #[inline]
    pub fn remaining_steps(&self) -> usize {
        match self {
            Self::Ground(path) => path.steps.len().saturating_sub(path.step_index as usize),
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// UnitInfo
// ---------------------------------------------------------------------------

/// A unit — C++ `class UnitInfo` (`unitinfo.hpp`), the simulation subset of
/// `S` fields (state-model §3.9).
///
/// Presentation fields from list B.1.2 are no longer here: they are in
/// [`max_types::unit::UnitPresentation`] (F5.3/B), which `GameState` keeps
/// in parallel to unit slots. The V70 layout has not changed — `save.rs` reads
/// both objects in the original byte order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitInfo {
    // --- identity and position ---
    /// Unit type.
    pub unit_type: UnitType,
    /// Unique id (`hash_id`; `NO_ID` = 0xFFFF).
    pub id: UnitId,
    /// Flags (`unit_flags::*`): unit class, behavior, owning team in the hash.
    pub flags: u32,
    /// Pixel position X. **Simulation field**: `UnitInfo::Move`/
    /// `AirPath` step pixel by pixel, `Access_IsWithinAttackRange` checks
    /// the line of fire by `x/y`; V70 stores it as is.
    pub x: u16,
    /// Pixel position Y. **Simulation field** (see [`Self::x`]).
    pub y: u16,
    /// Map cell.
    pub grid: Point,
    /// Unit name (variable length in V70, without a terminator).
    pub name: String,
    /// Owning team (including [`TeamId::ALIEN`] = 4).
    pub team: TeamId,
    /// Type sequence number within the team (for names).
    pub unit_id: u8,
    /// Hull angle 0..7 (0 = north, clockwise). **Simulation field**:
    /// `UnitInfo::Move` steps along `DIRECTION_OFFSETS[angle]`, the angle changes
    /// via `Paths_UpdateAngle`/`GroundPath::UpdateUnitAngle` 1:1 with C++.
    pub angle: u8,
    /// Visibility for each of the 5 teams (fog of war).
    pub visible_to_team: [bool; 5],
    /// Detected by teams.
    pub spotted_by_team: [bool; 5],
    /// Maximum animation speed (UI/V70).
    pub max_velocity: u8,
    /// Current animation speed (UI/V70).
    pub velocity: u8,
    /// Sprite scale (UI/V70).
    pub scaler_adjust: u8,
    /// Turret angle (UI/V70).
    pub turret_angle: u8,
    /// Sprite base (UI/V70).
    pub image_base: i16,
    /// Turret sprite base (UI/V70).
    pub turret_image_base: i16,
    /// Current frame (UI/V70).
    pub image_index: i16,
    /// Current turret frame (UI/V70).
    pub turret_image_index: i16,
    /// Maximum frame (UI/V70).
    pub image_index_max: i16,

    // --- orders ---
    /// Current order.
    pub orders: UnitOrder,
    /// Order execution state.
    pub state: UnitOrderState,
    /// Deferred order (restored after the current one completes).
    pub prior_orders: UnitOrder,
    /// Deferred state.
    pub prior_state: UnitOrderState,
    /// Camouflage/mine laying (submarine/minelayer).
    pub laying_state: u8,
    /// Target cell (C++ `move_to_grid_*` = `fire_on_grid_*` in V70).
    pub target_grid: Point,

    // --- construction and mining ---
    /// Turns remaining until construction completes.
    pub build_time: u8,
    /// Total mining.
    pub total_mining: u8,
    /// Raw material mining.
    pub raw_mining: u8,
    /// Fuel mining.
    pub fuel_mining: u8,
    /// Gold mining.
    pub gold_mining: u8,
    /// Maximum raw material mining.
    pub raw_mining_max: u8,
    /// Maximum gold mining.
    pub gold_mining_max: u8,
    /// Maximum fuel mining.
    pub fuel_mining_max: u8,

    // --- combat/economic attributes ---
    /// Hits (C++ u16, u8 in V70).
    pub hits: u16,
    /// Speed (C++ u16, u8 in V70).
    pub speed: u16,
    /// Shots this turn.
    pub shots: u8,
    /// Move and fire.
    pub move_and_fire: u8,
    /// Cargo; for `COMMANDO`/`REGENERATING_UNIT` — experience (see [`UnitInfo::experience`]).
    pub storage: i16,
    /// Experience (stored in `storage` in V70).
    pub experience: i16,
    /// Transferable cargo (runtime; in V70 — `move_to_grid_x`).
    pub transfer_cargo: i16,
    /// Stealth roll (runtime; in V70 — the low byte of `move_to_grid_x`).
    pub stealth_dice_roll: u8,
    /// Ammo.
    pub ammo: u8,
    /// Targeting mode (u8, not an enum).
    pub targeting_mode: u8,
    /// Boarding/landing mode (u8, not an enum).
    pub enter_mode: u8,
    /// Firing recoil frames (C++; in V70 — the single `recoil_delay`).
    pub firing_recoil_frames: u8,
    /// Remaining disabled turns (C++; in V70 — the single `recoil_delay`).
    pub disabled_turns_remaining: u8,
    /// Delayed reaction (network synchronization).
    pub delayed_reaction: u8,
    /// Damaged this turn.
    pub damaged_this_turn: bool,
    /// Building research topic.
    pub research_topic: ResearchTopic,
    /// Movement counter (per-turn limit).
    pub moved: u8,
    /// Shake effect (UI/V70).
    pub shake_effect_state: u8,
    /// Engine animation state (UI/V70).
    pub engine: u8,
    /// Weapon animation state (UI/V70).
    pub weapon: u8,
    /// A V70 field not recovered by the C++ port (preserved as is, §7.6).
    pub comm: u8,
    /// A V70 field not recovered by the C++ port (preserved as is).
    pub fuel_distance: u8,
    /// Movement fraction between cells (UI/V70).
    pub move_fraction: u8,
    /// A V70 field not recovered by the C++ port (preserved as is).
    pub energized: bool,
    /// Movement wait counter (limit `MOVEMENT_WAIT_LIMIT` = 512).
    pub movement_wait_count: u16,
    /// Repeat build (factory) — C++ `uint8_t` with bool semantics.
    pub repeat_build: bool,
    /// Build rate (multiplier; 0 in the file is treated as 1).
    pub build_rate: u16,
    /// Transient: this turn the construction cannot pay (`UpdateProduction` saw
    /// `storage < Cargo_GetRawConsumptionRate`), so `GameManager_SpawnNewUnits`
    /// in C++ would still advance `build_time`, but the port does not: the resource must not
    /// go negative (see `production::charge_units`).
    ///
    /// Not part of the save/CRC: recomputed every turn from `storage`.
    pub build_paused: bool,
    /// Return fire disabled.
    pub disabled_reaction_fire: bool,
    /// Auto survey.
    pub auto_survey: bool,
    /// AI state (`AI_STATE_*`; in V70 — `field_221`).
    pub ai_state_bits: u32,

    // --- structure ---
    /// Movement route.
    pub path: Option<UnitPath>,
    /// Complex connection bitfield (`CONNECTOR_*`).
    pub connectors: u16,
    /// This unit's attributes (with upgrades) — C++ `base_values` (always present).
    pub base_values: UnitValues,
    /// Complex membership (`None` — autonomous).
    pub complex: Option<ComplexId>,
    /// Parent (factory building/transport).
    pub parent_unit: Option<UnitId>,
    /// Current enemy target.
    pub enemy_unit: Option<UnitId>,
    /// Factory build queue (`UnitTypeArray`).
    pub build_list: Vec<UnitType>,

    // --- C++ runtime fields (not serialized in V70) ---
    /// Group speed.
    pub group_speed: u16,
    /// Live attack number (0 = none).
    pub live_attack: u32,
    /// Unit "in transit" (moving between cells).
    pub in_transit: bool,
    /// Number of pins (task pins).
    pub pin_count: i16,
    /// AI tasks enabled.
    pub tasks_enabled: bool,
    /// Which of the 5 lists the unit is in.
    pub unit_list: UnitListKind,
}

impl Default for UnitInfo {
    fn default() -> Self {
        Self::new(UnitType::GoldRefinery, TeamId::RED, UnitId::NO_ID)
    }
}

impl UnitInfo {
    /// Minimal constructor (C++ `UnitInfo()` in "special cases"):
    /// orders `AWAIT/EXECUTING_ORDER`, position (-1, -1).
    ///
    /// The presentation fields of the C++ constructor (`brightness = 255` and zeros)
    /// are set by [`UnitPresentation::new`] — `GameState` creates it alongside.
    pub fn new(unit_type: UnitType, team: TeamId, id: UnitId) -> Self {
        // `UnitInfo::UnitInfo` (`unitinfo.cpp:276-294`): the type's frame data from
        // the `D_*` resource — `image_base`, `turret_image_base`, `image_index =
        // image_base + angle` and `image_index_max = image_count + image_index - 1`.
        // `angle` is still 0 in this constructor, as in `UnitInfo(unit_type, team,
        // id)` without an angle.
        let frames = crate::frames::frame_info(unit_type);

        Self {
            unit_type,
            id,
            flags: 0,
            x: 0,
            y: 0,
            grid: Point::new(-1, -1),
            name: String::new(),
            team,
            unit_id: 0,
            angle: 0,
            visible_to_team: [false; 5],
            spotted_by_team: [false; 5],
            max_velocity: 0,
            velocity: 0,
            scaler_adjust: 0,
            turret_angle: 0,
            image_base: frames.image_base,
            turret_image_base: frames.turret_image_base,
            image_index: frames.image_base,
            turret_image_index: frames.turret_image_base,
            image_index_max: crate::frames::image_index_max(unit_type, 0),
            // `UnitInfo::UnitInfo` (`unitinfo.cpp:201-204`): a new unit is born
            // in `ORDER_AWAIT`/`ORDER_STATE_EXECUTING_ORDER`, and `prior` is the same
            // (`Await`/`ExecutingOrder`). Previously the port set `Init`, which made
            // `RestoreOrders` after completion return the building to
            // `Await`/`Init` instead of `Await`/`ExecutingOrder`.
            orders: UnitOrder::Await,
            state: UnitOrderState::ExecutingOrder,
            prior_orders: UnitOrder::Await,
            prior_state: UnitOrderState::ExecutingOrder,
            laying_state: 0,
            target_grid: Point::ZERO,
            build_time: 0,
            total_mining: 0,
            raw_mining: 0,
            fuel_mining: 0,
            gold_mining: 0,
            raw_mining_max: 0,
            gold_mining_max: 0,
            fuel_mining_max: 0,
            hits: 0,
            speed: 0,
            shots: 0,
            move_and_fire: 0,
            storage: 0,
            experience: 0,
            transfer_cargo: 0,
            stealth_dice_roll: 0,
            ammo: 0,
            targeting_mode: 0,
            enter_mode: 0,
            firing_recoil_frames: 0,
            disabled_turns_remaining: 0,
            delayed_reaction: 0,
            damaged_this_turn: false,
            research_topic: ResearchTopic::Attack,
            moved: 0,
            shake_effect_state: 0,
            engine: 0,
            weapon: 0,
            comm: 0,
            fuel_distance: 0,
            move_fraction: 0,
            energized: false,
            movement_wait_count: 0,
            repeat_build: false,
            build_rate: 1,
            build_paused: false,
            disabled_reaction_fire: false,
            auto_survey: false,
            ai_state_bits: 0,
            path: None,
            connectors: 0,
            base_values: crate::unit_values::base_unit_values(unit_type),
            complex: None,
            parent_unit: None,
            enemy_unit: None,
            build_list: Vec::new(),
            group_speed: 0,
            live_attack: 0,
            in_transit: false,
            pin_count: 0,
            tasks_enabled: true,
            unit_list: UnitListKind::Stationary,
        }
    }

    /// Whether the unit is visible to the team (`UnitInfo::IsVisibleToTeam`).
    #[inline]
    pub fn is_visible_to_team(&self, team: TeamId) -> bool {
        self.visible_to_team
            .get(team.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Mark visibility for a team (`UnitInfo::SetVisibleToTeam`).
    #[inline]
    pub fn set_visible_to_team(&mut self, team: TeamId, visible: bool) {
        if let Some(slot) = self.visible_to_team.get_mut(team.0 as usize) {
            *slot = visible;
        }
    }

    /// Whether the unit is detected by the team (`UnitInfo::IsDetectedByTeam`).
    #[inline]
    pub fn is_detected_by_team(&self, team: TeamId) -> bool {
        self.spotted_by_team
            .get(team.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Mark detection (`UnitInfo::SpotByTeam`).
    #[inline]
    pub fn spot_by_team(&mut self, team: TeamId) {
        if let Some(slot) = self.spotted_by_team.get_mut(team.0 as usize) {
            *slot = true;
        }
    }

    /// C++ `UnitInfo::GetId`.
    #[inline]
    pub const fn get_id(&self) -> UnitId {
        self.id
    }

    /// Experience: for `COMMANDO`/`REGENERATING_UNIT` C++ stores it in `storage`
    /// (state-model §3.9, DESIGN risk 16).
    #[inline]
    pub fn experience(&self) -> i16 {
        self.experience
    }

    /// The "unit is a complex member" predicate (`UnitInfo::IsComplexMember`).
    ///
    /// The raw `CONNECTOR_UNIT | BUILDING | STANDALONE` bits are read, not
    /// `is_building()`: C++ (`unitinfo.hpp:255-256`) counts a unit as a complex member
    /// when **any** of these three flags is set, excluding only `GROUND_COVER` and
    /// `NO_ID`. A fixed turret (`GUNTURRT`/`ARTYTRRT`/`ANTIAIR`/`ANTIMSSL`) has the
    /// `BUILDING` bit cleared in `SC_UNITS` but carries `STANDALONE`, so it **is** a
    /// complex member — the semantic `Mobility::Building` axis and complex membership
    /// are not the same predicate. Connectors (`CNCT_4W`: `CONNECTOR_UNIT` without
    /// `BUILDING`) and 1×1 standalone structures (`ADUMP`/`FDUMP`/`GOLDSM`:
    /// `STANDALONE` without `BUILDING`) are members too, although `is_building()` is
    /// false for them; the flag predicate is therefore the only correct source.
    #[inline]
    pub fn is_complex_member(&self) -> bool {
        use crate::enums::unit_flags::{BUILDING, CONNECTOR_UNIT, GROUND_COVER, STANDALONE};
        (self.flags & (CONNECTOR_UNIT | BUILDING | STANDALONE)) != 0
            && (self.flags & GROUND_COVER) == 0
            && !self.id.is_no_id()
    }

    /// The moving-order predicate (`UnitInfo::HasMoveOrder`).
    #[inline]
    pub fn has_move_order(&self) -> bool {
        matches!(
            self.orders,
            UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
        )
    }

    /// Unit list by flags (`UnitInfo::GetUnitList`/`AllocateUnitList`).
    pub fn list_kind_for_flags(flags: u32) -> UnitListKind {
        use crate::enums::unit_flags::{
            GROUND_COVER, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, MOBILE_SEA_UNIT, STATIONARY,
        };
        if flags & GROUND_COVER != 0 {
            UnitListKind::GroundCover
        } else if flags & MOBILE_AIR_UNIT != 0 {
            UnitListKind::MobileAir
        } else if flags & (MOBILE_LAND_UNIT | MOBILE_SEA_UNIT) != 0 {
            UnitListKind::MobileLandSea
        } else if flags & STATIONARY != 0 {
            UnitListKind::Stationary
        } else {
            UnitListKind::Particle
        }
    }
}

#[cfg(test)]
#[path = "unit_tests.rs"]
mod tests;
