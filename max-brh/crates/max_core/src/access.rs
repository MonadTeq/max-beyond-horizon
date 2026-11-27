//! Passability, cell occupancy and group orders.
//!
//! Oracle: `max-cpp-port/src/access.cpp` (`Access_IsAccessible`,
//! `Access_GetModifiedSurfaceType`, `Access_GetAttackTarget`,
//! `Access_GetAttackTarget2`, `Access_GetTeamUnit`,
//! `Access_GetActiveUnitWithFlags`, `Access_GetReceiverUnit`, `Access_GetUnit2`,
//! `Access_CanReceiveUnits`, `Access_GetStoredUnitCount`,
//! `Access_IsWithinAttackRange`, `Access_GetSquaredDistance`,
//! `Access_GetApproximateDistance`, `Access_GetValidAttackTargetTypes`,
//! `Access_IsValidAttackTarget*`, `Access_GetAttackPosition` — the latter lives in
//! `units_manager.cpp`, but by meaning belongs to access), `Access_SetUnitDestination`,
//! `Access_TriggerEnemyMine`, `Access_ProcessNewGroupOrder`,
//! `Access_ProcessGroupAirPath`; `paths.cpp` (`Paths_IsOccupied`); `unitinfo.cpp`
//! (`UnitInfo::AttemptSideStep`, `UnitInfo::AreTherePins`,
//! `UnitsManager_IsEngaged`/`IsEngagedByInfiltrator` from `units_manager.cpp`),
//! `units.cpp` (`SC_UNITS`: flags and `land_type` of 93 types), DESIGN.md §2.
//!
//! # Type table
//!
//! The C++ port reads unit flags and `land_type` from the `SC_UNITS` resource
//! (`Units::LoadResource`, `assets/PATCHES.RES`). `max_core` has no file I/O,
//! so the [`UNIT_TYPE_PROPERTIES`] table is generated from the same resource and
//! committed (values are C++ `UNIT_FLAGS`/`SURFACE_TYPE_*`). Row order —
//! `UnitType` order (0x00–0x5C), which is also `ResourceID` order.
//!
//! # Occupancy and groups
//!
//! `GameState` keeps a `MapHash` (`hash.rs`) — an exact analog of C++ `Hash_MapHash`
//! (cell → units in insertion order; for buildings — 2×2 cells). C++ `UnitInfoGroup`
//! group lists are modeled by parallel `GameState` tables
//! (`unit_group`/`groups`): the lists are not serialized in V70, so there is no
//! separate field in [`crate::unit::UnitInfo`].

use crate::classification::{Environment, Footprint, UnitTypeClassify};
use crate::enums::unit_flags::{
    GROUND_COVER, HOVERING, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, MOBILE_SEA_UNIT, SELECTABLE,
    STATIONARY,
};
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::{Point, DIRECTION_OFFSETS};
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo};
use crate::unit_order;
use crate::unit_values::{ATTRIB_RANGE, ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS};

// ---------------------------------------------------------------------------
// C++ constants (access.cpp / enums.hpp)
// ---------------------------------------------------------------------------

/// `AccessModifier_NoModifiers`.
pub const ACCESS_MODIFIER_NONE: u32 = 0x00;
/// `AccessModifier_EnemySameClassBlocks`.
pub const ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS: u32 = 0x01;
/// `AccessModifier_SameClassBlocks`.
pub const ACCESS_MODIFIER_SAME_CLASS_BLOCKS: u32 = 0x02;
/// `AccessModifier_Unused`.
pub const ACCESS_MODIFIER_UNUSED: u32 = 0x04;
/// `AccessModifier_MovesUnderBridge`.
pub const ACCESS_MODIFIER_MOVES_UNDER_BRIDGE: u32 = 0x08;
/// `AccessModifier_IgnoreVisibility`.
pub const ACCESS_MODIFIER_IGNORE_VISIBILITY: u32 = 0x10;
/// `AccessModifier_IgnoreSurfaceClass`.
pub const ACCESS_MODIFIER_IGNORE_SURFACE_CLASS: u32 = 0x20;

/// `TARGET_CLASS_NONE` (`access.cpp`).
pub const TARGET_CLASS_NONE: u32 = 0x0;
/// `TARGET_CLASS_LAND_STEALTH`.
pub const TARGET_CLASS_LAND_STEALTH: u32 = 0x1;
/// `TARGET_CLASS_SEA_STEALTH`.
pub const TARGET_CLASS_SEA_STEALTH: u32 = 0x2;
/// `TARGET_CLASS_LAND`.
pub const TARGET_CLASS_LAND: u32 = 0x4;
/// `TARGET_CLASS_WATER`.
pub const TARGET_CLASS_WATER: u32 = 0x8;
/// `TARGET_CLASS_AIR`.
pub const TARGET_CLASS_AIR: u32 = 0x10;

/// `SURFACE_TYPE_LAND`.
pub const SURFACE_TYPE_LAND: u8 = 0x1;
/// `SURFACE_TYPE_WATER`.
pub const SURFACE_TYPE_WATER: u8 = 0x2;
/// `SURFACE_TYPE_COAST`.
pub const SURFACE_TYPE_COAST: u8 = 0x4;
/// `SURFACE_TYPE_AIR`.
pub const SURFACE_TYPE_AIR: u8 = 0x8;
/// `SURFACE_TYPE_ALL`.
pub const SURFACE_TYPE_ALL: u8 = 0x0F;

/// `Access_IsWithinScanRange` (`access.cpp:353`) for an **explicit** anchor cell.
///
/// The simulation passes the unit's `grid` (`GameState::is_within_scan_range`);
/// the frame passes the cell the sprite stands in (`max_app::vision`), because
/// `UnitInfo::Move` (`unitinfo.cpp:1787-1816`) moves `grid_x/grid_y` — and with
/// them the scan — only when the sprite crosses into the next cell.
pub fn is_within_scan_range_at(anchor: Point, grid_x: i32, grid_y: i32, scan_range: i32) -> bool {
    let scan_area = (scan_range * 64) * (scan_range * 64);
    let radius_x = (grid_x - i32::from(anchor.x)) * 64;
    let radius_y = (grid_y - i32::from(anchor.y)) * 64;

    radius_x * radius_x + radius_y * radius_y <= scan_area
}

// ---------------------------------------------------------------------------
// Unit-type property table (SC_UNITS)
// ---------------------------------------------------------------------------

/// Unit-type flags and passability — C++ `Unit` fields (`units.json`/`SC_UNITS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitTypeProperties {
    /// `Unit::m_flags` (`crate::enums::unit_flags::*`).
    pub flags: u32,
    /// `Unit::m_land_type` (`SURFACE_TYPE_*`).
    pub land_type: u8,
}

impl UnitTypeProperties {
    const fn new(flags: u32, land_type: u8) -> Self {
        Self { flags, land_type }
    }
}

/// Properties of all 93 types — values from `assets/PATCHES.RES` (`SC_UNITS`),
/// the order matches [`UnitType`].
pub const UNIT_TYPE_PROPERTIES: [UnitTypeProperties; 93] = [
    // COMMTWR
    UnitTypeProperties::new(0x0160_4210, 0x1),
    // POWERSTN
    UnitTypeProperties::new(0x0160_4210, 0x1),
    // POWGEN
    UnitTypeProperties::new(0x01E0_4200, 0x1),
    // BARRACKS
    UnitTypeProperties::new(0x0140_4210, 0x1),
    // SHIELDGN
    UnitTypeProperties::new(0x0100_0210, 0x1),
    // RADAR
    UnitTypeProperties::new(0x01E0_4204, 0x1),
    // ADUMP
    UnitTypeProperties::new(0x01C0_4200, 0x1),
    // FDUMP
    UnitTypeProperties::new(0x01C0_4200, 0x1),
    // GOLDSM
    UnitTypeProperties::new(0x01C0_4200, 0x1),
    // DEPOT
    UnitTypeProperties::new(0x0140_4210, 0x1),
    // HANGAR
    UnitTypeProperties::new(0x0140_4210, 0x1),
    // DOCK
    UnitTypeProperties::new(0x0040_4210, 0x2),
    // CNCT_4W
    UnitTypeProperties::new(0x0040_4208, 0x7),
    // LRGRUBLE
    UnitTypeProperties::new(0x0000_0211, 0x1),
    // SMLRUBLE
    UnitTypeProperties::new(0x0000_0201, 0x1),
    // LRGTAPE
    UnitTypeProperties::new(0x0040_0211, 0x7),
    // SMLTAPE
    UnitTypeProperties::new(0x0040_0201, 0x7),
    // LRGSLAB
    UnitTypeProperties::new(0x0000_0211, 0x1),
    // SMLSLAB
    UnitTypeProperties::new(0x0000_0201, 0x1),
    // LRGCONES
    UnitTypeProperties::new(0x0000_0211, 0x1),
    // SMLCONES
    UnitTypeProperties::new(0x0000_0201, 0x1),
    // ROAD
    UnitTypeProperties::new(0x0040_0201, 0x1),
    // LANDPAD
    UnitTypeProperties::new(0x0040_4200, 0x1),
    // SHIPYARD
    UnitTypeProperties::new(0x0068_4210, 0x2),
    // LIGHTPLT
    UnitTypeProperties::new(0x0168_4210, 0x1),
    // LANDPLT
    UnitTypeProperties::new(0x0168_4210, 0x1),
    // SUPRTPLT
    UnitTypeProperties::new(0x0100_0210, 0x1),
    // AIRPLT
    UnitTypeProperties::new(0x0168_4210, 0x1),
    // HABITAT
    UnitTypeProperties::new(0x0140_4210, 0x1),
    // RESEARCH
    UnitTypeProperties::new(0x0160_4210, 0x1),
    // GREENHSE
    UnitTypeProperties::new(0x0160_4210, 0x1),
    // RECCENTR
    UnitTypeProperties::new(0x0100_0210, 0x1),
    // TRAINHAL
    UnitTypeProperties::new(0x0160_4210, 0x1),
    // WTRPLTFM
    UnitTypeProperties::new(0x0040_4201, 0x6),
    // GUNTURRT
    UnitTypeProperties::new(0x07E2_4200, 0x1),
    // ANTIAIR
    UnitTypeProperties::new(0x07E2_4200, 0x1),
    // ARTYTRRT
    UnitTypeProperties::new(0x07E2_4200, 0x1),
    // ANTIMSSL
    UnitTypeProperties::new(0x07E6_4200, 0x1),
    // BLOCK
    UnitTypeProperties::new(0x00C0_4200, 0x1),
    // BRIDGE
    UnitTypeProperties::new(0x0040_4201, 0x6),
    // MININGST
    UnitTypeProperties::new(0x0160_4210, 0x1),
    // LANDMINE
    UnitTypeProperties::new(0x0440_4201, 0x1),
    // SEAMINE
    UnitTypeProperties::new(0x0440_4201, 0x2),
    // LNDEXPLD
    UnitTypeProperties::new(0x0000_0206, 0x0),
    // AIREXPLD
    UnitTypeProperties::new(0x0000_0206, 0x0),
    // SEAEXPLD
    UnitTypeProperties::new(0x0000_0206, 0x0),
    // BLDEXPLD
    UnitTypeProperties::new(0x0000_0206, 0x0),
    // HITEXPLD
    UnitTypeProperties::new(0x0000_0206, 0x0),
    // MASTER
    UnitTypeProperties::new(0x0460_0100, 0x1),
    // CONSTRCT
    UnitTypeProperties::new(0x0468_4180, 0x7),
    // SCOUT
    UnitTypeProperties::new(0x0462_4180, 0x7),
    // TANK
    UnitTypeProperties::new(0x0662_4100, 0x1),
    // ARTILLRY
    UnitTypeProperties::new(0x0462_4100, 0x1),
    // ROCKTLCH
    UnitTypeProperties::new(0x0466_4100, 0x1),
    // MISSLLCH
    UnitTypeProperties::new(0x0466_4100, 0x1),
    // SP_FLAK
    UnitTypeProperties::new(0x0662_4100, 0x1),
    // MINELAYR
    UnitTypeProperties::new(0x0460_4100, 0x1),
    // SURVEYOR
    UnitTypeProperties::new(0x0460_4180, 0x7),
    // SCANNER
    UnitTypeProperties::new(0x0C60_4100, 0x1),
    // SPLYTRCK
    UnitTypeProperties::new(0x0460_4100, 0x1),
    // GOLDTRCK
    UnitTypeProperties::new(0x0460_4100, 0x1),
    // ENGINEER
    UnitTypeProperties::new(0x0468_4180, 0x7),
    // BULLDOZR
    UnitTypeProperties::new(0x0460_4100, 0x1),
    // REPAIR
    UnitTypeProperties::new(0x0460_4100, 0x1),
    // FUELTRCK
    UnitTypeProperties::new(0x0460_4100, 0x1),
    // CLNTRANS
    UnitTypeProperties::new(0x0460_4180, 0x7),
    // COMMANDO
    UnitTypeProperties::new(0x0442_4100, 0x5),
    // INFANTRY
    UnitTypeProperties::new(0x0442_4100, 0x5),
    // FASTBOAT
    UnitTypeProperties::new(0x0662_4080, 0x2),
    // CORVETTE
    UnitTypeProperties::new(0x0466_4080, 0x2),
    // BATTLSHP
    UnitTypeProperties::new(0x0662_4080, 0x2),
    // SUBMARNE
    UnitTypeProperties::new(0x0464_4080, 0x2),
    // SEATRANS
    UnitTypeProperties::new(0x0460_4080, 0x6),
    // MSSLBOAT
    UnitTypeProperties::new(0x0466_4080, 0x2),
    // SEAMNLYR
    UnitTypeProperties::new(0x0460_4080, 0x2),
    // CARGOSHP
    UnitTypeProperties::new(0x0460_4080, 0x6),
    // FIGHTER
    UnitTypeProperties::new(0x0466_4040, 0xF),
    // BOMBER
    UnitTypeProperties::new(0x0466_4040, 0xF),
    // AIRTRANS
    UnitTypeProperties::new(0x0460_4040, 0xF),
    // AWAC
    UnitTypeProperties::new(0x0C60_4040, 0xF),
    // JUGGRNT
    UnitTypeProperties::new(0x1466_0080, 0x2),
    // ALNTANK
    UnitTypeProperties::new(0x1666_0100, 0x1),
    // ALNASGUN
    UnitTypeProperties::new(0x1466_0100, 0x1),
    // ALNPLANE
    UnitTypeProperties::new(0x1466_0040, 0xF),
    // ROCKET
    UnitTypeProperties::new(0x0000_0020, 0x0),
    // TORPEDO
    UnitTypeProperties::new(0x0000_0021, 0x0),
    // ALNMISSL
    UnitTypeProperties::new(0x0000_0020, 0x0),
    // ALNTBALL
    UnitTypeProperties::new(0x0000_0020, 0x0),
    // ALNABALL
    UnitTypeProperties::new(0x0000_0020, 0x0),
    // RKTSMOKE
    UnitTypeProperties::new(0x0000_0026, 0x0),
    // TRPBUBLE
    UnitTypeProperties::new(0x0000_0027, 0x0),
    // HARVSTER
    UnitTypeProperties::new(0x0000_0200, 0x1),
    // WALDO
    UnitTypeProperties::new(0x0000_0201, 0x1),
];

/// Unit-type properties (`ResourceManager_GetUnit(type).GetFlags()/GetLandType()`).
#[inline]
pub fn unit_type_properties(unit_type: UnitType) -> UnitTypeProperties {
    UNIT_TYPE_PROPERTIES[unit_type as usize]
}

/// Unit-type flags (`ResourceManager_GetUnit(type).GetFlags()`).
#[inline]
pub fn unit_type_flags(unit_type: UnitType) -> u32 {
    unit_type_properties(unit_type).flags
}

/// Unit-type passability (`ResourceManager_GetUnit(type).GetLandType()`).
#[inline]
pub fn unit_type_land_type(unit_type: UnitType) -> u8 {
    unit_type_properties(unit_type).land_type
}

// ---------------------------------------------------------------------------
// Surface
// ---------------------------------------------------------------------------

/// `Access_GetSurfaceType` (`access.cpp:936`) — the raw surface of a cell.
#[inline]
pub(crate) fn surface_type(state: &GameState, grid: Point) -> u8 {
    state.map.surface_at(grid.x, grid.y).bits()
}

/// `Access_GetModifiedSurfaceType` (`access.cpp:950`): a bridge/platform changes
/// the cell surface for paths. `mode == true` — from a naval unit's point of view
/// (a bridge does not hide water), otherwise a ground one (both bridge and platform = land).
pub(crate) fn modified_surface_type(state: &GameState, grid: Point, mode: bool) -> u8 {
    let mut surface = surface_type(state, grid);

    if surface == SURFACE_TYPE_WATER || surface == SURFACE_TYPE_COAST {
        for id in state.units_at(grid).iter().copied() {
            let Some(unit) = state.unit(id) else { continue };
            let unit_type = unit.unit_type;

            if mode {
                if unit_type == UnitType::WaterPlatform {
                    surface = SURFACE_TYPE_LAND;
                    break;
                } else if unit_type == UnitType::Bridge {
                    break;
                }
            } else if unit_type == UnitType::WaterPlatform || unit_type == UnitType::Bridge {
                surface = SURFACE_TYPE_LAND;
                break;
            }
        }
    }

    surface
}

// ---------------------------------------------------------------------------
// Cell accessibility
// ---------------------------------------------------------------------------

/// `Access_IsAccessible` (`access.cpp:107`) — the cost of entering a cell
/// (`0` — impassable). `unit_type` — the type of the entering unit, `team` — its team.
///
/// This is an exact port of the loop over `Hash_MapHash[Point(grid)]`: the unit order in
/// a cell is the insertion order in the [`crate::hash::MapHash`] bucket.
pub(crate) fn is_accessible(
    state: &GameState,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
    flags: u32,
) -> u32 {
    is_accessible_ignoring(state, unit_type, team, grid, flags, &[])
}

/// [`is_accessible`] with units that are **not** in the hash during the check.
///
/// C++ `UnitsManager_MoveUnitAndParent`/`UnitsManager_IssueBuildOrder` temporarily
/// remove the unit (and the builder parent) from `Hash_MapHash`, otherwise the unit's own
/// mobile unit blocks the 2×2 area of the future building. The same technique is used here without
/// changing state: the listed ids are skipped in the cell loop.
pub(crate) fn is_accessible_ignoring(
    state: &GameState,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
    flags: u32,
    ignored: &[crate::unit::UnitId],
) -> u32 {
    let props = unit_type_properties(unit_type);

    if grid.x < 0
        || grid.y < 0
        || grid.x >= crate::geo::MAP_WIDTH
        || grid.y >= crate::geo::MAP_HEIGHT
    {
        return 0;
    }

    let mut result = 4u32;

    if flags & ACCESS_MODIFIER_IGNORE_SURFACE_CLASS == 0 {
        let mut surface = modified_surface_type(state, grid, false);

        if surface & SURFACE_TYPE_WATER != 0
            && (unit_type.is_ground() || unit_type.is_amphibious())
            && unit_type != UnitType::Surveyor
        {
            result = 4 + 8;
        }

        for id in state.units_at(grid).iter().copied() {
            if ignored.contains(&id) {
                continue;
            }
            let Some(other) = state.unit(id) else {
                continue;
            };

            if !(other.is_visible_to_team(team)
                || flags & ACCESS_MODIFIER_IGNORE_VISIBILITY != 0
                || (other.is_detected_by_team(team) && !other.is_mobile()))
            {
                continue;
            }

            if other.orders == UnitOrder::Idle && other.is_mobile() {
                continue;
            }

            if unit_type.is_air() {
                if (flags & ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0
                    || (flags & ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0 && other.team != team))
                    && other.is_air()
                {
                    return 0;
                }
            } else {
                if flags & ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0
                    || (flags & ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0 && other.team != team)
                {
                    if matches!(
                        other.environment(),
                        Some(Environment::Sea | Environment::Ground | Environment::Amphibious)
                    ) {
                        return 0;
                    }

                    // A landed aircraft (not in flight) blocks a ground path.
                    if other.is_air() && other.flags & HOVERING == 0 {
                        return 0;
                    }
                }

                if !unit_type.is_mobile()
                    && unit_type != UnitType::Connector
                    && (other.unit_type == UnitType::Bridge
                        || (other.unit_type as u16 >= UnitType::LargeRubble as u16
                            && other.unit_type as u16 <= UnitType::SmallCones as u16)
                        || other.unit_type == UnitType::LandMine
                        || other.unit_type == UnitType::SeaMine)
                {
                    return 0;
                }

                if !other.is_mobile() {
                    match other.unit_type {
                        UnitType::Road => {
                            if unit_type == UnitType::Road {
                                return 0;
                            }
                            result = 2;
                        }
                        UnitType::Bridge => {
                            result = 4;
                            if unit_type.is_ground() || unit_type.is_amphibious() {
                                result = 2;
                                if is_bridge_elevated(other)
                                    && flags & ACCESS_MODIFIER_MOVES_UNDER_BRIDGE != 0
                                {
                                    surface = SURFACE_TYPE_WATER;
                                } else {
                                    surface = SURFACE_TYPE_LAND;
                                }
                            } else if !is_bridge_elevated(other)
                                && flags & ACCESS_MODIFIER_MOVES_UNDER_BRIDGE != 0
                            {
                                surface = SURFACE_TYPE_LAND;
                            } else {
                                surface = SURFACE_TYPE_WATER;
                            }
                        }
                        UnitType::LandMine | UnitType::SeaMine => {
                            if other.team != team && other.is_detected_by_team(team) {
                                return 0;
                            }
                        }
                        UnitType::LargeRubble | UnitType::SmallRubble => {
                            surface = SURFACE_TYPE_LAND;
                        }
                        UnitType::LargeSlab | UnitType::SmallSlab => {
                            if unit_type == UnitType::Road {
                                return 0;
                            }
                            result = 2;
                        }
                        UnitType::Connector => {
                            if unit_type == UnitType::Connector {
                                return 0;
                            } else if !unit_type.is_mobile()
                                && other.team != team
                                && unit_type != UnitType::WaterPlatform
                                && unit_type != UnitType::Bridge
                                && unit_type != UnitType::Road
                            {
                                return 0;
                            }
                        }
                        UnitType::DeadWaldo => {}
                        UnitType::WaterPlatform => {
                            surface = SURFACE_TYPE_LAND;
                            result = 4;
                        }
                        _ => return 0,
                    }
                }
            }
        }

        if props.land_type & surface == 0 {
            result = 0;
        }
    }

    result
}

/// Accessibility for a concrete unit (type and team are taken from it).
#[inline]
pub(crate) fn is_accessible_unit(state: &GameState, unit: &UnitInfo, grid: Point, flags: u32) -> u32 {
    is_accessible(state, unit.unit_type, unit.team, grid, flags)
}

/// `UnitInfo::IsBridgeElevated` (`unitinfo.cpp:3030`).
#[inline]
pub fn is_bridge_elevated(unit: &UnitInfo) -> bool {
    unit.unit_type == UnitType::Bridge && unit.image_index != unit.image_base
}

// ---------------------------------------------------------------------------
// Geometry and attack range
// ---------------------------------------------------------------------------

/// `Access_GetSquaredDistance` (`access.cpp`).
#[inline]
pub fn squared_distance(first: Point, second: Point) -> i32 {
    let dx = i32::from(first.x) - i32::from(second.x);
    let dy = i32::from(first.y) - i32::from(second.y);
    dx * dx + dy * dy
}

/// `Access_GetTargetClass` (`access.cpp:724`) — the target class that the
/// unit itself is (not to be confused with the attack group `Access_GetAttackTargetGroup`,
/// [`crate::game::GameState::attack_target_group`]).
///
/// Needed by `enemy_halt`: `Access_UpdateMapStatus` compares the class of spotted
/// enemies with the class of the unit itself (and its group) so as not to stop
/// movement because of a target it cannot attack anyway.
pub(crate) fn target_class(state: &GameState, unit: &UnitInfo) -> u32 {
    if unit.unit_type == UnitType::Infiltrator || state.is_unit_under_water(unit) {
        for team_index in 0..crate::game::PLAYER_TEAM_COUNT {
            let team = TeamId(team_index as u8);

            if unit.team != team
                && state.teams[team_index].team_type != TeamKind::None
                && unit.is_visible_to_team(team)
            {
                return if unit.unit_type == UnitType::Infiltrator {
                    TARGET_CLASS_LAND
                } else {
                    TARGET_CLASS_WATER
                };
            }
        }

        if unit.unit_type == UnitType::Infiltrator {
            TARGET_CLASS_LAND_STEALTH
        } else {
            TARGET_CLASS_SEA_STEALTH
        }
    } else if unit.is_air() {
        TARGET_CLASS_AIR
    } else if unit.is_sea() || unit.is_amphibious() {
        TARGET_CLASS_WATER
    } else {
        TARGET_CLASS_LAND
    }
}

/// `Access_GetApproximateDistance(distance_x, distance_y)`.
#[inline]
pub fn approximate_distance(distance_x: i32, distance_y: i32) -> i32 {
    let dx = distance_x.abs();
    let dy = distance_y.abs();
    if dx >= dy {
        2 * dx + dy
    } else {
        2 * dy + dx
    }
}

/// `Access_GetApproximateDistance(point1, point2)`.
#[inline]
pub fn approximate_distance_points(first: Point, second: Point) -> i32 {
    approximate_distance(
        i32::from(first.x) - i32::from(second.x),
        i32::from(first.y) - i32::from(second.y),
    )
}

/// `Access_IsWithinAttackRange` (`access.cpp:395`).
///
/// For submarines and corvettes it is additionally checked that the line of fire does not
/// cross land: C++ walks the pixel line and checks `Access_GetSurfaceType`.
pub(crate) fn is_within_attack_range(state: &GameState, unit: &UnitInfo, grid: Point, range: i32) -> bool {
    let attack_range = (range * 64) * (range * 64);
    let mut distance_x = (i32::from(grid.x) - i32::from(unit.grid.x)) * 64;
    let mut distance_y = (i32::from(grid.y) - i32::from(unit.grid.y)) * 64;
    let distance = distance_x * distance_x + distance_y * distance_y;

    if distance > attack_range {
        return false;
    }

    if unit.unit_type != UnitType::Submarine && unit.unit_type != UnitType::Corvette {
        return true;
    }

    let mut distance = distance_x.abs().max(distance_y.abs()) / 64;

    if distance <= 0 {
        return true;
    }

    let ratio_x = (distance_x << 16) / distance;
    let ratio_y = (distance_y << 16) / distance;
    distance_x = ratio_x + ((unit.x as i32) << 16);
    distance_y = ratio_y + ((unit.y as i32) << 16);

    while {
        distance -= 1;
        distance > 0
    } {
        let surface = surface_type(
            state,
            Point::new(
                ((distance_x >> 16) >> 6) as i16,
                ((distance_y >> 16) >> 6) as i16,
            ),
        );

        if surface != SURFACE_TYPE_WATER && surface != SURFACE_TYPE_COAST {
            return false;
        }

        distance_x += ratio_x;
        distance_y += ratio_y;
    }

    true
}

// ---------------------------------------------------------------------------
// Attack targets
// ---------------------------------------------------------------------------

/// `Access_GetValidAttackTargetTypes` (`access.cpp:1841`).
pub fn valid_attack_target_types(unit_type: UnitType) -> u32 {
    use UnitType::*;
    match unit_type {
        Scout | Infiltrator | Infantry | Corvette | Gunboat | Submarine | MissileCruiser
        | Bomber | Juggernaut | AlienTank | AlienAssaultGun | Tank | AssaultGun
        | RocketLauncher | MissileCrawler | GunTurret | Artillery | MissileLauncher => {
            MOBILE_SEA_UNIT | MOBILE_LAND_UNIT | STATIONARY
        }
        Escort | Fighter | MobileAntiAir | AntiAir => MOBILE_AIR_UNIT,
        AlienPlane => MOBILE_AIR_UNIT | MOBILE_SEA_UNIT | MOBILE_LAND_UNIT | STATIONARY,
        LandMine | SeaMine => MOBILE_SEA_UNIT | MOBILE_LAND_UNIT,
        _ => 0,
    }
}

/// `Access_IsValidAttackTargetTypeEx` (`access.cpp:1890`).
pub fn is_valid_attack_target_type_ex(
    attacker: UnitType,
    target: UnitType,
    mut target_flags: u32,
) -> bool {
    use UnitType::*;
    match target {
        Bridge | WaterPlatform | SeaMine => target_flags = MOBILE_LAND_UNIT,
        Submarine => match attacker {
            Bomber | Submarine | Corvette | AlienPlane | SeaMine => {}
            _ => return false,
        },
        LargeTape | SmallTape => return false,
        _ => {}
    }

    target_flags & valid_attack_target_types(attacker) != 0
}

/// `Access_IsValidAttackTargetType` (`access.cpp:1939`).
#[inline]
pub fn is_valid_attack_target_type(attacker: UnitType, target: UnitType) -> bool {
    is_valid_attack_target_type_ex(attacker, target, unit_type_flags(target))
}

/// `Access_IsValidAttackTargetEx` (`access.cpp:1922`).
pub(crate) fn is_valid_attack_target_ex(
    state: &GameState,
    attacker: UnitType,
    target: UnitType,
    target_flags: u32,
    point: Point,
) -> bool {
    let surface = surface_type(state, point);

    if (attacker == UnitType::Submarine || attacker == UnitType::Corvette)
        && surface != SURFACE_TYPE_WATER
        && surface != SURFACE_TYPE_COAST
    {
        return false;
    }

    is_valid_attack_target_type_ex(attacker, target, target_flags)
}

/// `Access_IsValidAttackTarget(attacker, target, point)` (`access.cpp:1968`).
pub(crate) fn is_valid_attack_target_point(
    state: &GameState,
    attacker: &UnitInfo,
    target: &UnitInfo,
    point: Point,
) -> bool {
    let mut target_flags = target.flags;
    let mut target_type = target.unit_type;

    if attacker.is_air() && attacker.flags & HOVERING == 0 {
        return false;
    }

    if target.is_air() && target_flags & HOVERING == 0 {
        target_flags &= !MOBILE_AIR_UNIT;
        target_flags |= MOBILE_LAND_UNIT;
    }

    if target_type == UnitType::LargeTape || target_type == UnitType::SmallTape {
        let Some(parent) = target.parent_unit.and_then(|id| state.unit(id)) else {
            return false;
        };
        target_type = parent.unit_type;
    }

    is_valid_attack_target_ex(state, attacker.unit_type, target_type, target_flags, point)
}

/// `Access_IsValidAttackTarget(attacker, target)` (`access.cpp:1995`).
#[inline]
pub(crate) fn is_valid_attack_target(state: &GameState, attacker: &UnitInfo, target: &UnitInfo) -> bool {
    is_valid_attack_target_point(state, attacker, target, target.grid)
}

/// `Access_IsUnitBusyAtLocation` (`access.cpp:1949`).
pub(crate) fn is_unit_busy_at_location(state: &GameState, unit: &UnitInfo) -> bool {
    for id in state.units_at(unit.grid).iter().copied() {
        let Some(other) = state.unit(id) else {
            continue;
        };
        if (other.is_sea() || other.is_ground() || other.is_amphibious() || !other.is_mobile())
            && other.flags & GROUND_COVER == 0
            && other.orders != UnitOrder::Idle
        {
            return true;
        }
    }
    false
}

/// `Access_GetAttackTarget` (`access.cpp:1999`).
pub(crate) fn get_attack_target(
    state: &GameState,
    unit: &UnitInfo,
    grid: Point,
    mode: bool,
) -> Option<UnitId> {
    let mut normal_unit = false;
    let mut result = None;

    if grid.x < 0
        || grid.y < 0
        || grid.x >= crate::geo::MAP_WIDTH
        || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else {
            continue;
        };

        if other.team == unit.team {
            if other.flags & crate::enums::unit_flags::MISSILE_UNIT == 0 {
                normal_unit = true;
            }
        } else if other.flags & SELECTABLE != 0
            && other.orders != UnitOrder::Explode
            && other.state != UnitOrderState::Destroy
            && other.orders != UnitOrder::Idle
            && is_valid_attack_target_point(state, unit, other, grid)
        {
            if mode {
                // `mode == true`: SpotByTeam is performed by the caller layer
                // (C++ does it here; Rust needs a mutable reference).
            }

            if other.is_visible_to_team(unit.team) {
                result = Some(id);
                break;
            }
        }
    }

    if let Some(id) = result {
        let unit_ref = state.unit(id)?;
        if unit_ref.unit_type == UnitType::LargeTape || unit_ref.unit_type == UnitType::SmallTape {
            return unit_ref.parent_unit;
        }
        if normal_unit && unit_ref.flags & GROUND_COVER != 0 {
            return None;
        }
    }

    result
}

/// `Access_GetAttackTarget2` (`access.cpp:2077`) — a target not yet visible
/// to the attacker (spotting on fire).
pub(crate) fn get_attack_target2(state: &GameState, unit: &UnitInfo, grid: Point) -> Option<UnitId> {
    if grid.x < 0
        || grid.y < 0
        || grid.x >= crate::geo::MAP_WIDTH
        || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else {
            continue;
        };
        if other.team != unit.team
            && !other.is_visible_to_team(unit.team)
            && other.orders != UnitOrder::Idle
            && is_valid_attack_target(state, unit, other)
        {
            return Some(id);
        }
    }

    None
}

/// `Access_GetTeamUnit` (`access.cpp:1804`).
pub(crate) fn get_team_unit(state: &GameState, grid: Point, team: TeamId, flags: u32) -> Option<UnitId> {
    get_team_unit_matching(state, grid, team, |other| other.flags & flags != 0)
}

/// [`get_team_unit`] with a typed environment filter instead of the raw
/// `MOBILE_*` mask — the same `Access_GetTeamUnit` (`access.cpp:1804`).
///
/// A unit matches if its environment (instance classification by `unit.flags`,
/// [`UnitInfo::environment`]) is among `environments`. For flags from `SC_UNITS`
/// this is bit-for-bit the same as the C++ raw mask: the `MOBILE_LAND_UNIT` bit is
/// `[Environment::Ground, Environment::Amphibious]` (it includes amphibians),
/// the `MOBILE_SEA_UNIT` bit — `[Environment::Sea, Environment::Amphibious]`, the
/// `MOBILE_AIR_UNIT` bit — `[Environment::Air]`. At the same time `[Environment::Ground]`
/// is **narrower** than the bit: pure ground units only, without amphibians.
///
/// A divergence from the raw mask is possible only with manually assembled
/// contradictory flags (for example `MOBILE_AIR_UNIT` together with
/// `MOBILE_LAND_UNIT`): the classification gives priority to `Air`, while the
/// `MOBILE_LAND_UNIT` bit would match in C++. In `SC_UNITS` and at runtime such flags
/// do not occur (`HOVERING` is a separate flight bit), so in-game behavior does not
/// change.
pub(crate) fn get_team_unit_by_environments(
    state: &GameState,
    grid: Point,
    team: TeamId,
    environments: &[Environment],
) -> Option<UnitId> {
    get_team_unit_matching(state, grid, team, |other| {
        other
            .environment()
            .is_some_and(|environment| environments.contains(&environment))
    })
}

/// The common `Access_GetTeamUnit` walk: `matches` replaces the
/// `other.flags & flags != 0` check, two passes and tape handling — as in C++.
fn get_team_unit_matching(
    state: &GameState,
    grid: Point,
    team: TeamId,
    matches: impl Fn(&UnitInfo) -> bool,
) -> Option<UnitId> {
    if grid.x < 0
        || grid.y < 0
        || grid.x >= crate::geo::MAP_WIDTH
        || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    let mut result = None;

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else {
            continue;
        };
        if other.team == team
            && matches(other)
            && other.orders != UnitOrder::Idle
            && !other.id.is_no_id()
            && other.unit_type != UnitType::LargeTape
            && other.unit_type != UnitType::SmallTape
            && other.flags & GROUND_COVER == 0
        {
            result = Some(id);
            break;
        }
    }

    if result.is_none() {
        for id in state.units_at(grid).iter().copied() {
            let Some(other) = state.unit(id) else {
                continue;
            };
            if other.team == team
                && matches(other)
                && other.orders != UnitOrder::Idle
                && !other.id.is_no_id()
            {
                result = Some(id);
                break;
            }
        }
    }

    if let Some(id) = result {
        let unit = state.unit(id)?;
        if unit.unit_type == UnitType::LargeTape || unit.unit_type == UnitType::SmallTape {
            return unit.parent_unit;
        }
    }

    result
}

/// `Access_GetActiveUnitWithFlags` (`access.cpp:1492`).
pub(crate) fn get_active_unit_with_flags(state: &GameState, grid: Point, flags: u32) -> Option<UnitId> {
    if grid.x < 0
        || grid.y < 0
        || grid.x >= crate::geo::MAP_WIDTH
        || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    for id in state.units_at(grid).iter().copied() {
        let Some(unit) = state.unit(id) else { continue };
        if unit.flags & flags != 0
            && unit.flags & GROUND_COVER == 0
            && unit.orders != UnitOrder::Idle
        {
            return Some(id);
        }
    }

    None
}

/// `Access_GetBridge` (`access.cpp:1533`) — the bridge standing on the cell.
///
/// `UnitsManager_ActivateUnit` uses it for a naval unit released onto a bridge
/// cell: the bridge is raised (`ORDER_MOVE`/`ORDER_STATE_ELEVATE`) so the ship
/// can pass underneath (`units_manager.cpp:3891-3897`).
pub(crate) fn get_bridge(state: &GameState, grid: Point) -> Option<UnitId> {
    if grid.x < 0
        || grid.y < 0
        || grid.x >= crate::geo::MAP_WIDTH
        || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    state
        .units_at(grid)
        .iter()
        .copied()
        .find(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::Bridge)
        })
}

/// `Access_GetEnemyMineOnSentry` (`access.cpp:2050`).
pub(crate) fn get_enemy_mine_on_sentry(state: &GameState, team: TeamId, grid: Point) -> Option<UnitId> {
    for id in state.units_at(grid).iter().copied() {
        let Some(unit) = state.unit(id) else { continue };
        if (unit.unit_type == UnitType::LandMine || unit.unit_type == UnitType::SeaMine)
            && unit.team != team
            && unit.orders == UnitOrder::Sentry
        {
            return Some(id);
        }
    }
    None
}

/// `Access_TriggerEnemyMine` (`access.cpp:2068`): a detected mine explodes.
pub(crate) fn trigger_enemy_mine(state: &mut GameState, team: TeamId, grid: Point) {
    let Some(mine) = get_enemy_mine_on_sentry(state, team, grid) else {
        return;
    };

    state.set_order(mine, UnitOrder::Explode, UnitOrderState::Explode);
    if let Some(unit) = state.unit_mut(mine) {
        unit.set_visible_to_team(team, true);
    }
    if let Some(mut unit) = state.unit(mine).cloned() {
        unit.spot_by_team(team);
        state.write_unit(mine, unit);
    }
}

// ---------------------------------------------------------------------------
// Unit boarding (transport/buildings)
// ---------------------------------------------------------------------------

/// `Access_CanReceiveUnits` (`access.cpp:2150`).
#[inline]
pub fn can_receive_units(receiver: &UnitInfo) -> bool {
    receiver.hits > 0 && receiver.state != UnitOrderState::Destroy
}

/// `Access_GetReceiverUnit` (`access.cpp:2100`).
pub(crate) fn get_receiver_unit(state: &GameState, unit: &UnitInfo, grid: Point) -> Option<UnitId> {
    if grid.x < 0
        || grid.y < 0
        || grid.x >= crate::geo::MAP_WIDTH
        || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    let team = unit.team;

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else {
            continue;
        };
        if other.team != team || other.orders == UnitOrder::Idle || !can_receive_units(other) {
            continue;
        }

        if unit.is_ground() || unit.is_amphibious() {
            if (other.unit_type == UnitType::Barracks
                || other.unit_type == UnitType::ArmouredPersonnelCarrier
                || other.unit_type == UnitType::SeaTransport)
                && (unit.unit_type == UnitType::Infiltrator || unit.unit_type == UnitType::Infantry)
            {
                return Some(id);
            } else if (other.unit_type == UnitType::Depot
                || other.unit_type == UnitType::SeaTransport)
                && unit.unit_type != UnitType::Infiltrator
                && unit.unit_type != UnitType::Infantry
            {
                return Some(id);
            }
        }

        if unit.is_sea() && other.unit_type == UnitType::Dock {
            return Some(id);
        }

        if unit.is_air()
            && (other.unit_type == UnitType::LandingPad || other.unit_type == UnitType::Hangar)
        {
            return Some(id);
        }
    }

    None
}

/// `Access_GetUnit2` (`access.cpp:1555`) — another selectable, non-flying and
/// non-ground-cover unit of the team on the cell; a tape reports its own parent.
///
/// Used by `UnitsManager_LoadUnit` (`units_manager.cpp:5034`): a landing pad accepts an
/// aircraft only when no other unit occupies its cell.
pub(crate) fn get_unit2(state: &GameState, grid: Point, team: TeamId) -> Option<UnitId> {
    let result = state.units_at(grid).iter().copied().find(|id| {
        state.unit(*id).is_some_and(|other| {
            other.team == team
                && other.flags & SELECTABLE != 0
                && other.flags & (HOVERING | GROUND_COVER) == 0
                && other.unit_type != UnitType::LandingPad
                && other.orders != UnitOrder::Idle
                && !other.id.is_no_id()
        })
    });

    let unit = state.unit(result?)?;
    if unit.unit_type == UnitType::LargeTape || unit.unit_type == UnitType::SmallTape {
        unit.parent_unit
    } else {
        result
    }
}

/// `Access_GetStoredUnitCount` (`access.cpp:1291`).
pub(crate) fn get_stored_unit_count(state: &GameState, receiver: &UnitInfo) -> i32 {
    let list = if receiver.unit_type == UnitType::Hangar {
        crate::unit::UnitListKind::MobileAir
    } else {
        crate::unit::UnitListKind::MobileLandSea
    };

    let mut result = 0;
    for id in state.unit_list(list) {
        let Some(unit) = state.unit(*id) else {
            continue;
        };
        if unit.orders == UnitOrder::Idle && unit.parent_unit == Some(receiver.id) {
            result += 1;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Engaged / pins / stealth
// ---------------------------------------------------------------------------

/// `UnitInfo::AreTherePins` (`unitinfo.cpp`): the target is pinned by a live attack.
#[inline]
pub fn are_there_pins(unit: &UnitInfo) -> bool {
    unit.pin_count > 0
}

/// `UnitsManager_IsStealthActionInProgress` (`units_manager.cpp:1383`).
pub fn is_stealth_action_in_progress(unit: &UnitInfo) -> bool {
    let order = unit.orders;
    let prior_order = unit.prior_orders;

    unit.hits > 0
        && unit.state != UnitOrderState::Destroy
        && (order == UnitOrder::AwaitStealUnit
            || order == UnitOrder::AwaitDisableUnit
            || (order == UnitOrder::AwaitScaling
                && (prior_order == UnitOrder::AwaitStealUnit
                    || prior_order == UnitOrder::AwaitDisableUnit)))
}

/// `UnitsManager_IsEngagedByInfiltrator` (`units_manager.cpp:1408`): a saboteur
/// holds the target from the order until the action resolves.
pub(crate) fn is_engaged_by_infiltrator(state: &GameState, unit: &UnitInfo) -> bool {
    for id in state.unit_list(crate::unit::UnitListKind::MobileLandSea) {
        let Some(other) = state.unit(*id) else {
            continue;
        };
        if other.unit_type == UnitType::Infiltrator
            && other.parent_unit == Some(unit.id)
            && is_stealth_action_in_progress(other)
        {
            return true;
        }
    }
    false
}

/// `UnitsManager_IsEngaged` (`units_manager.cpp:1422`).
pub(crate) fn is_engaged(state: &GameState, unit: &UnitInfo) -> bool {
    are_there_pins(unit) || is_engaged_by_infiltrator(state, unit)
}

/// `UnitsManager_GetAttackPosition` (`units_manager.cpp:4847`).
pub(crate) fn get_attack_position(state: &GameState, attacker: &UnitInfo, target: &UnitInfo) -> Point {
    if target.unit_type == UnitType::Constructor && target.orders == UnitOrder::Build {
        // `Access_GetConstructionUtility` — a P5 simplification: the build site itself.
    }

    let mut position = target.grid;

    // A 2×2 target: the attacker must stand not at its center but past the footprint (offset
    // +1 along axes where it is to the right/below). A turret (1×1) does not fall here —
    // this is layout (`is_quad()`), not "building" (`is_building()`).
    if target.is_quad() {
        if attacker.grid.x > position.x {
            position.x += 1;
        }
        if attacker.grid.y > position.y {
            position.y += 1;
        }

        if attacker.unit_type == UnitType::Submarine || attacker.unit_type == UnitType::Corvette {
            let mut direction = 4;
            while direction >= 0 {
                let surface = surface_type(state, position);
                if surface == SURFACE_TYPE_WATER || surface == SURFACE_TYPE_COAST {
                    break;
                }

                position.x = target.grid.x + (direction & 0x01);
                position.y = target.grid.y + if direction & 0x02 != 0 { 1 } else { 0 };
                direction -= 1;
            }
        }
    }

    position
}

/// `Paths_IsOccupied` (`paths.cpp:439`): a stealthy unit (submarine/saboteur/
/// transport) on a path cell tries to step aside; otherwise it is revealed and
/// blocks movement. Returns `true` if the cell is considered occupied.
pub(crate) fn paths_is_occupied(state: &mut GameState, grid: Point, angle: i32, team: TeamId) -> bool {
    let Some(ids) = state.units_at_opt(grid).map(|units| units.to_vec()) else {
        return false;
    };

    for id in ids {
        let Some(unit) = state.unit(id) else { continue };
        if (unit.unit_type == UnitType::Submarine
            || unit.unit_type == UnitType::Infiltrator
            || unit.unit_type == UnitType::ArmouredPersonnelCarrier)
            && !unit.is_visible_to_team(team)
        {
            if !attempt_side_step(state, id, grid, angle) {
                if let Some(unit) = state.unit_mut(id) {
                    unit.spot_by_team(team);
                    unit.set_visible_to_team(team, true);
                }
                return true;
            }
        }
    }

    false
}

/// `UnitInfo::AttemptSideStep` (`unitinfo.cpp:3243`) — an abridged port:
/// states/engagement/speed are checked, then a neighboring cell is chosen
/// by the same rule (maximum angle to the movement direction,
/// minimum cost; for `CLNTRANS` water has priority).
fn attempt_side_step(state: &mut GameState, id: UnitId, grid: Point, angle: i32) -> bool {
    let Some(unit) = state.unit(id).cloned() else {
        return false;
    };

    if !matches!(
        unit.orders,
        UnitOrder::Await | UnitOrder::Sentry | UnitOrder::Move
    ) || is_engaged(state, &unit)
    {
        return false;
    }

    let team = unit.team;
    let team_type = state.teams[team.0 as usize].team_type;
    if unit.speed == 0 && team_type != TeamKind::Computer {
        return false;
    }

    if unit.grid != grid {
        // C++ checks the destination cell, having temporarily removed the unit from `Hash_MapHash`, and
        // on success only stops it (`BlockedOnPathRequest(true)`):
        // the actual sidestep is performed by its own order.
        let flags = unit.flags;
        state.map_hash_remove(id, unit.grid, flags);
        let cost = is_accessible_unit(state, &unit, grid, ACCESS_MODIFIER_SAME_CLASS_BLOCKS);
        state.map_hash_add(id, unit.grid, flags);

        if cost != 0 {
            if let Some(mut unit) = state.unit(id).cloned() {
                unit_order::blocked_on_path_request(&mut unit, true, state.disable_fire);
                state.write_unit(id, unit);
            }
            return true;
        }

        return false;
    }

    if unit.orders == UnitOrder::Move
        && (unit.state == UnitOrderState::InProgress || unit.state == UnitOrderState::InTransition)
    {
        return true;
    }

    let is_clntrans = unit.unit_type == UnitType::ArmouredPersonnelCarrier;
    let mut best_cost = 0i32;
    let mut best_angle = 0i32;
    let mut best_site = unit.grid;
    let mut best_is_water = false;

    for direction in 0..8 {
        let position = Point::new(
            unit.grid.x + DIRECTION_OFFSETS[direction].x,
            unit.grid.y + DIRECTION_OFFSETS[direction].y,
        );
        let mut unit_angle = (direction as i32 - angle + 8) & 0x03;
        if unit_angle > 2 {
            unit_angle = 4 - unit_angle;
        }

        if best_cost == 0 || unit_angle > best_angle {
            let flags = unit.flags;
            state.map_hash_remove(id, unit.grid, flags);
            let mut step_cost =
                is_accessible_unit(state, &unit, position, ACCESS_MODIFIER_SAME_CLASS_BLOCKS)
                    as i32;
            state.map_hash_add(id, unit.grid, flags);

            if direction & 0x01 != 0 {
                step_cost = (step_cost * 3) / 2;
            }

            if step_cost == 0 {
                continue;
            }

            if step_cost as i64 > i64::from(unit.speed) * 4 + i64::from(unit.move_fraction)
                && team_type == TeamKind::Computer
            {
                continue;
            }

            let candidate_is_water =
                is_clntrans && modified_surface_type(state, position, false) == SURFACE_TYPE_WATER;

            let is_better = if is_clntrans {
                (candidate_is_water && !best_is_water)
                    || (candidate_is_water == best_is_water
                        && (unit_angle > best_angle
                            || (unit_angle == best_angle && step_cost < best_cost)))
            } else {
                unit_angle > best_angle
            };

            if is_better {
                best_angle = unit_angle;
                best_cost = step_cost;
                best_site = position;
                best_is_water = candidate_is_water;
            }
        }
    }

    if best_cost == 0 {
        return false;
    }

    let mut unit = unit;
    if team_type == TeamKind::Computer {
        unit.speed = unit.speed.wrapping_add(((best_cost + 3) / 4) as u16);
    }

    let flags = unit.flags;
    state.map_hash_remove(id, unit.grid, flags);
    state.map_hash_add(id, best_site, flags);

    unit.grid = best_site;
    // The sidestep changes the cell, so the pixel position follows it
    // (`UnitInfo::SetPosition`, `unitinfo.cpp:1301`): a unit destroyed right after
    // the sidestep must explode on the new cell, not on the old one.
    crate::unit_order::update_grid_position(&mut unit);
    unit.path = Some(crate::unit::UnitPath::Ground(crate::unit::GroundPath {
        end_x: best_site.x,
        end_y: best_site.y,
        step_index: 0,
        steps: vec![crate::geo::PathStep::new(
            (best_site.x - grid.x) as i8,
            (best_site.y - grid.y) as i8,
        )],
    }));
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::InProgress;
    state.write_unit(id, unit);
    true
}

// ---------------------------------------------------------------------------
// SetUnitDestination
// ---------------------------------------------------------------------------

/// `Access_SetUnitDestination` (`access.cpp:67`).
///
/// Returns `true` if the cell "yields" (the unit will move away or there is a bridge), and
/// `false` if the path must be found anew.
pub(crate) fn set_unit_destination(
    state: &mut GameState,
    _unit: &UnitInfo,
    from: Point,
    to: Point,
    mode: bool,
) -> bool {
    let mut bridge = None;

    for id in state.units_at(to).to_vec() {
        let Some(other) = state.unit(id) else {
            continue;
        };
        if other.is_sea() || other.is_ground() || other.is_amphibious() {
            if other.orders == UnitOrder::Idle {
                continue;
            }

            if other.has_move_order()
                && other.state != UnitOrderState::ExecutingOrder
                && other.path.as_ref().is_some()
                && other.grid == to
                && !path_is_in_path(other, from)
            {
                return true;
            }

            return false;
        } else if other.unit_type == UnitType::Bridge {
            bridge = Some(id);
        }
    }

    if let Some(id) = bridge {
        if let Some(mut bridge_unit) = state.unit(id).cloned() {
            if bridge_unit.orders == UnitOrder::Await {
                let order_state = if mode {
                    UnitOrderState::Lower
                } else {
                    UnitOrderState::Elevate
                };
                unit_order::set_new_order_int(&mut bridge_unit, UnitOrder::Move, order_state);
                state.write_unit(id, bridge_unit);
            }
        }
        return true;
    }

    false
}

/// `GroundPath::IsInPath` (`groundpath.cpp:146`) for a relative target.
fn path_is_in_path(unit: &UnitInfo, grid: Point) -> bool {
    let Some(crate::unit::UnitPath::Ground(path)) = unit.path.as_ref() else {
        return false;
    };
    let mut point = Point::ZERO;
    for step in path.steps.iter().skip(path.step_index as usize) {
        if point == grid {
            return true;
        }
        point.x += i16::from(step.x);
        point.y += i16::from(step.y);
    }
    point == grid
}

// ---------------------------------------------------------------------------
// Group orders (`UnitInfoGroup`)
// ---------------------------------------------------------------------------

/// `Access_ProcessGroupAirPath` (`access.cpp`): the air path of a group member.
pub(crate) fn process_group_air_path(state: &mut GameState, id: UnitId) -> bool {
    let Some(unit) = state.unit(id).cloned() else {
        return false;
    };
    let target = state.order_data(id).move_to;
    let mut unit = unit;
    let path = crate::pathfinding::paths_get_air_path(state, &mut unit, target);
    unit.path = path.map(crate::unit::UnitPath::Air);
    let has_path = unit.path.is_some();
    if !has_path {
        unit_order::blocked_on_path_request(&mut unit, false, state.disable_fire);
        state.write_unit(id, unit);
        clear_unit_group(state, id);
        return false;
    }
    state.write_unit(id, unit);
    true
}

/// `Access_ProcessNewGroupOrder` (`access.cpp:2321`).
///
/// In C++ a group is a `SmartList<UnitInfo>`; here — `GameState::groups`.
/// Returns `false` if the path is still being built (the leader stays in
/// `PendingAirGroupLeader`), `true` if the group order is allowed.
pub(crate) fn process_new_group_order(state: &mut GameState, leader: UnitId) -> bool {
    let Some(group) = state.group_members(leader) else {
        return false;
    };

    for id in group.iter().copied() {
        let Some(unit) = state.unit(id) else { continue };
        if unit.state == UnitOrderState::NewOrder && unit.path.is_none() {
            if unit.hits > 0 {
                if unit.is_air() && unit.orders != UnitOrder::MoveToAttack {
                    process_group_air_path(state, id);
                }
                return false;
            }
            clear_unit_group(state, id);
        }
    }

    for id in group {
        let Some(unit) = state.unit(id) else { continue };
        if unit.state == UnitOrderState::NewOrder {
            let result =
                crate::pathfinding::request_path(state, id, ACCESS_MODIFIER_SAME_CLASS_BLOCKS);
            if let Some(mut unit) = state.unit(id).cloned() {
                unit.state = if result {
                    UnitOrderState::MoveGettingPath
                } else {
                    UnitOrderState::IssuingPath
                };
                state.write_unit(id, unit);
            }
        }
    }

    true
}

/// Assign units to the group led by `leader` (`AllocateUnitList` +
/// `AssignUnitList`). Returns the group number.
#[cfg(test)]
pub(crate) fn assign_unit_group(state: &mut GameState, leader: UnitId, members: &[UnitId]) -> u32 {
    clear_unit_group(state, leader);
    let group_id = state.create_group_with_leader(leader);
    for id in members {
        state.assign_group_member(*id, group_id);
    }
    group_id
}

/// `UnitInfo::ClearUnitList` — remove a unit from a group list.
pub(crate) fn clear_unit_group(state: &mut GameState, id: UnitId) {
    state.clear_group_member(id);
}

/// `Access_GroupAttackOrder` (`access.cpp`): a group attack order. In C++
/// it walks the group list; here — only member routes (without `Ai_*`).
pub(crate) fn group_attack_order(state: &mut GameState, leader: UnitId, mode: bool) {
    let Some(group) = state.group_members(leader) else {
        return;
    };
    for id in group {
        let Some(unit) = state.unit(id) else { continue };
        if unit.is_air() && unit.orders != UnitOrder::MoveToAttack {
            if mode {
                process_group_air_path(state, id);
            }
        } else if unit.state == UnitOrderState::NewOrder {
            crate::pathfinding::request_path(state, id, ACCESS_MODIFIER_SAME_CLASS_BLOCKS);
            if let Some(mut unit) = state.unit(id).cloned() {
                unit.state = UnitOrderState::MoveGettingPath;
                state.write_unit(id, unit);
            }
        }
    }
}

/// The unit's group (a copy of the list).
#[inline]
pub(crate) fn group_members_of(state: &GameState, id: UnitId) -> Option<Vec<UnitId>> {
    state.group_members(id)
}

// ---------------------------------------------------------------------------
// C++ helper values (`Access_*`)
// ---------------------------------------------------------------------------

/// `Access_IsTeamInUnitLists` (`access.cpp`) — whether the team has a unit in the lists.
#[allow(dead_code)] // Oracle trace: C++ parity, no gameplay caller yet.
pub(crate) fn is_team_in_unit_lists(state: &GameState, team: TeamId) -> bool {
    for kind in crate::unit::UnitListKind::ALL {
        for id in state.unit_list(kind) {
            if let Some(unit) = state.unit(*id) {
                if unit.team == team {
                    return true;
                }
            }
        }
    }
    false
}

/// An `Access_GetUnit2`-like search: the first live unit on the cell.
#[allow(dead_code)] // Oracle trace: C++ parity, no gameplay caller yet.
#[inline]
pub(crate) fn first_unit_at(state: &GameState, grid: Point) -> Option<UnitId> {
    state.units_at(grid).first().copied()
}

/// `Access_IsAnyLandPresent` (`access.cpp:1030`) — whether there is land in the 2×2 area.
#[allow(dead_code)] // Oracle trace: C++ parity, no gameplay caller yet.
pub(crate) fn is_any_land_present(state: &GameState, grid: Point, flags: u32) -> bool {
    let is_land = |x: i16, y: i16| surface_type(state, Point::new(x, y)) == SURFACE_TYPE_LAND;

    if Footprint::from_flags(flags).is_quad() {
        is_land(grid.x + 1, grid.y)
            || is_land(grid.x, grid.y + 1)
            || is_land(grid.x + 1, grid.y + 1)
            || is_land(grid.x, grid.y)
    } else {
        is_land(grid.x, grid.y)
    }
}

/// `Access_IsFullyLandCovered` (`access.cpp:1050`).
pub(crate) fn is_fully_land_covered(state: &GameState, grid: Point, flags: u32) -> bool {
    let is_land = |x: i16, y: i16| surface_type(state, Point::new(x, y)) == SURFACE_TYPE_LAND;

    if Footprint::from_flags(flags).is_single()
        || (is_land(grid.x + 1, grid.y)
            && is_land(grid.x, grid.y + 1)
            && is_land(grid.x + 1, grid.y + 1))
    {
        is_land(grid.x, grid.y)
    } else {
        false
    }
}

/// `UnitInfo::GetAttackRange` (`unitinfo.cpp:3888`) — the maximum attack
/// distance given the remaining movement.
pub fn get_attack_range(unit: &UnitInfo) -> i32 {
    let rounds = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
    if rounds > 0 {
        if unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE)
            != 0
        {
            unit.base_values.get_attribute(ATTRIB_RANGE)
                + unit.base_values.get_attribute(ATTRIB_SPEED) / 2
        } else {
            ((rounds - 1) * unit.base_values.get_attribute(ATTRIB_SPEED)) / rounds
                + unit.base_values.get_attribute(ATTRIB_RANGE)
        }
    } else {
        0
    }
}

/// `UnitsManager_IsAccessible` (`units_manager.cpp:4987`): the 2×2 site for
/// a building (all four cells are passable for its type).
#[allow(dead_code)] // Oracle trace: C++ parity, no gameplay caller yet.
pub(crate) fn is_accessible_for_building(
    state: &GameState,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
) -> bool {
    if grid.x < 0
        || grid.y < 0
        || grid.x > crate::geo::MAP_WIDTH - 2
        || grid.y > crate::geo::MAP_HEIGHT - 2
    {
        return false;
    }

    is_accessible(
        state,
        unit_type,
        team,
        grid,
        ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
    ) != 0
        && is_accessible(
            state,
            unit_type,
            team,
            Point::new(grid.x + 1, grid.y),
            ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
        ) != 0
        && is_accessible(
            state,
            unit_type,
            team,
            Point::new(grid.x, grid.y + 1),
            ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
        ) != 0
        && is_accessible(
            state,
            unit_type,
            team,
            Point::new(grid.x + 1, grid.y + 1),
            ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
        ) != 0
}

/// `Access_IsWithinMovementRange` (`access.cpp`): whether the remaining speed is enough
/// for the current path.
#[allow(dead_code)] // Oracle trace: C++ parity, no gameplay caller yet.
pub(crate) fn is_within_movement_range(state: &GameState, unit: &UnitInfo) -> bool {
    if unit.speed == 0 {
        return false;
    }
    match unit.path.as_ref() {
        None => true,
        Some(crate::unit::UnitPath::Ground(path)) => {
            let mut x = unit.grid.x;
            let mut y = unit.grid.y;
            let mut cost = 0i32;
            for step in path.steps.iter().skip(path.step_index as usize) {
                if step.x != 0 || step.y != 0 {
                    x += i16::from(step.x);
                    y += i16::from(step.y);
                    let mut step_cost =
                        is_accessible_unit(state, unit, Point::new(x, y), ACCESS_MODIFIER_NONE)
                            as i32;
                    if step.x != 0 && step.y != 0 {
                        step_cost = (step_cost * 3) / 2;
                    }
                    cost += step_cost;
                }
            }
            cost < i32::from(unit.speed) * 4 + i32::from(unit.move_fraction)
        }
        Some(_) => true,
    }
}

/// `Access_GetRepairShopClientCount` (`access.cpp:1300`) — for P3 repair.
pub(crate) fn get_repair_shop_client_count(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
    use crate::enums::unit_flags::ELECTRONIC_UNIT;
    let (list, flags) = match unit_type {
        UnitType::Depot => (
            crate::unit::UnitListKind::MobileLandSea,
            MOBILE_LAND_UNIT | ELECTRONIC_UNIT,
        ),
        UnitType::Hangar => (
            crate::unit::UnitListKind::MobileAir,
            MOBILE_AIR_UNIT | ELECTRONIC_UNIT,
        ),
        UnitType::Dock => (
            crate::unit::UnitListKind::MobileLandSea,
            MOBILE_SEA_UNIT | ELECTRONIC_UNIT,
        ),
        UnitType::Barracks => (crate::unit::UnitListKind::MobileLandSea, MOBILE_LAND_UNIT),
        _ => return 0,
    };

    let mut result = 0;
    for id in state.unit_list(list) {
        let Some(unit) = state.unit(*id) else {
            continue;
        };
        let unit_flags =
            unit.flags & (MOBILE_AIR_UNIT | MOBILE_LAND_UNIT | MOBILE_SEA_UNIT | ELECTRONIC_UNIT);
        if unit.team == team && unit_flags == flags && unit.hits > 0 {
            result += 1;
        }
    }
    result
}

/// A `Task_IsAdjacent`-like check for stealth actions: the target is on a cell
/// next to the unit (diagonals included).
pub fn is_adjacent_to_grid(unit: &UnitInfo, grid: Point) -> bool {
    let dx = (i32::from(unit.grid.x) - i32::from(grid.x)).abs();
    let dy = (i32::from(unit.grid.y) - i32::from(grid.y)).abs();
    dx <= 1 && dy <= 1 && (dx != 0 || dy != 0)
}

/// The target's `storage` attribute value (`ComputeUnitStorage`): used by
/// `AttemptStealthAction` to check whether the transport is full.
pub fn is_transporter_full(unit: &UnitInfo) -> bool {
    i32::from(unit.storage) >= unit.base_values.get_attribute(ATTRIB_STORAGE)
}

/// `Access_DestroyUtilities` (`access.cpp`): removes utilities from a cell —
/// tapes/cones always, road/slabs/rubble/connectors — by flags.
pub(crate) fn destroy_utilities(
    state: &mut GameState,
    grid: Point,
    remove_slabs: bool,
    remove_rubble: bool,
    remove_connectors: bool,
    remove_road: bool,
) {
    let ids = state.units_at(grid).to_vec();

    for id in ids {
        let Some(unit) = state.unit(id) else { continue };
        let destroy = match unit.unit_type {
            UnitType::SmallTape
            | UnitType::LargeTape
            | UnitType::SmallCones
            | UnitType::LargeCones => true,
            UnitType::Road => remove_road,
            UnitType::SmallSlab | UnitType::LargeSlab => remove_slabs,
            // `UnitsManager_RemoveConnections` — P3 complex logic.
            UnitType::Connector => remove_connectors,
            UnitType::SmallRubble | UnitType::LargeRubble => remove_rubble,
            _ => false,
        };

        if destroy {
            state.destroy_unit(id);
        }
    }
}

/// `UnitsManager_BuildClearing(unit, mode)` (`units_manager.cpp:3646-3805`) —
/// destroying a unit while leaving rubble.
///
/// C++:
/// 1. `rubble_type = LRGRUBLE` for `BUILDING`, otherwise `SMLRUBLE`, but only if
///    `mode` (the unit was destroyed, not removed), the type is not `COMMANDO`/`INFANTRY`, there are no
///    `CONNECTOR_UNIT`/`HOVERING` flags and the cell is **entirely on land**
///    (`Access_IsFullyLandCovered`, `access.cpp:1049`);
/// 2. `cargo_amount = GetNormalRateBuildCost() / 2` (+ `storage` for raw materials);
/// 3. `Access_DestroyGroundCovers(unit)` — road/site under the rubble;
/// 4. `UnitsManager_DestroyUnit(unit, mode)`;
/// 5. for buildings — `Access_DestroyUtilities(grid, true, …, false, false)`;
/// 6. `UnitsManager_DeployUnit(rubble_type, team, …, Randomizer_Generate(image_count))`
///    and `rubble->storage = cargo_amount`.
///
/// Mines leave no rubble (`rubble_type = INVALID_ID`, `units_manager.cpp:3720`).
///
/// Returns the id of the created rubble (for tests/log).
pub(crate) fn build_clearing(
    state: &mut GameState,
    unit_id: crate::unit::UnitId,
    mode: bool,
) -> Option<crate::unit::UnitId> {
    let unit = state.unit(unit_id)?.clone();
    let team = unit.team;
    let grid = unit.grid;
    let flags = unit.flags;
    let mut rubble_type = None;
    let mut cargo_amount = 0i32;

    let is_mine = matches!(unit.unit_type, UnitType::LandMine | UnitType::SeaMine);

    if mode
        && !is_mine
        && !matches!(unit.unit_type, UnitType::Infiltrator | UnitType::Infantry)
        && flags & (crate::enums::unit_flags::CONNECTOR_UNIT | crate::enums::unit_flags::HOVERING) == 0
        && is_fully_land_covered(state, grid, flags)
    {
        rubble_type = Some(if unit.is_quad() {
            UnitType::LargeRubble
        } else {
            UnitType::SmallRubble
        });

        cargo_amount = crate::victory::get_normal_rate_build_cost(&unit) / 2;

        if crate::victory::cargo_type_of(unit.unit_type) == crate::victory::CargoType::Raw {
            cargo_amount += i32::from(unit.storage);
        }
    }

    // `Access_DestroyGroundCovers(unit)` — ground/stationary (not HOVERING).
    if (unit.is_air() || unit.is_ground() || unit.is_amphibious() || !unit.is_mobile())
        && flags & crate::enums::unit_flags::HOVERING == 0
    {
        destroy_ground_covers(state, &unit);
    }

    let remove_rubble = !(unit.unit_type == UnitType::Bulldozer && unit.orders == UnitOrder::Clear);

    state.destroy_unit(unit_id);

    // `Access_DestroyUtilities(unit_grid, true, !skip_rubble_removal, false, false)`
    // — slabs/tapes under a building and an unfinished construction. `STANDALONE` — 1×1
    // structures with their own site, so the condition does not reduce to the footprint.
    if unit.is_quad()
        || flags & crate::enums::unit_flags::STANDALONE != 0
        || unit.orders == UnitOrder::Build
    {
        destroy_utilities(state, grid, true, remove_rubble, false, false);
    }

    let rubble_type = rubble_type?;
    let image_count = crate::frames::frame_info(rubble_type).image_count.max(1);
    let angle = state.rng.generate_i32(i32::from(image_count)) as u8;

    let rubble = state.spawn_unit(rubble_type, team, grid, 0)?;

    if let Some(rubble_unit) = state.unit_mut(rubble) {
        rubble_unit.angle = angle;
        rubble_unit.image_index = rubble_unit.image_base + i16::from(angle);
        rubble_unit.storage = cargo_amount as i16;
    }

    Some(rubble)
}

/// `Access_DestroyGroundCovers` (`access.cpp`): removes the road/site/
/// platform/bridge under the unit (a building occupies 2×2).
pub(crate) fn destroy_ground_covers(state: &mut GameState, unit: &UnitInfo) {
    let footprint = unit.footprint();
    let positions = if footprint.is_quad() {
        [
            Point::new(unit.grid.x, unit.grid.y),
            Point::new(unit.grid.x + 1, unit.grid.y),
            Point::new(unit.grid.x, unit.grid.y + 1),
            Point::new(unit.grid.x + 1, unit.grid.y + 1),
        ]
    } else {
        [unit.grid; 4]
    };
    let count = if footprint.is_quad() { 4 } else { 1 };

    for position in positions.iter().take(count) {
        for id in state.units_at(*position).to_vec() {
            let Some(other) = state.unit(id) else {
                continue;
            };
            if other.id != unit.id
                && other.state != UnitOrderState::Destroy
                && matches!(
                    other.unit_type,
                    UnitType::Road
                        | UnitType::LandingPad
                        | UnitType::WaterPlatform
                        | UnitType::Bridge
                )
            {
                state.destroy_unit(id);
            }
        }
    }
}

/// `UnitsManager_IsUnloadSiteTaken` (`units_manager.cpp:4074`) — whether the
/// unload site is taken by another air transport.
///
/// Ported fully: a walk over `UnitsManager_MobileAirUnits` in
/// insertion order.
pub(crate) fn is_unload_site_taken(state: &GameState, unit: &UnitInfo) -> bool {
    for id in state.unit_list(crate::unit::UnitListKind::MobileAir) {
        let Some(other) = state.unit(*id) else {
            continue;
        };
        if other.id != unit.id && other.hits > 0 && other.grid == unit.grid {
            let is_suspended = (other.orders == UnitOrder::Disable
                || other.orders == UnitOrder::Explode)
                && other.prior_orders == UnitOrder::Unload
                && other.prior_state == UnitOrderState::UnloadingInProgress;
            if (other.orders == UnitOrder::Unload
                && other.state == UnitOrderState::UnloadingInProgress)
                || is_suspended
            {
                return true;
            }
        }
    }
    false
}

/// Number of turns for which the saboteur disables the target
/// (`UnitsManager_DisableUnit`, `units_manager.cpp:4395`).
pub fn disable_turns(infiltrator: &UnitInfo, target: &UnitInfo) -> i32 {
    let target_speed = target.base_values.get_attribute(ATTRIB_SPEED);
    let target_turns = target.base_values.get_attribute(ATTRIB_TURNS);
    let experience = i32::from(infiltrator.experience);
    let mut turns;

    if target_turns == 0 {
        // C++ divides by `ATTRIB_TURNS` (risk 15); 0 is unreachable for game types.
        return 1;
    }

    if target_speed != 0 {
        turns = ((target_speed * (experience + 8)) - (target_turns * i32::from(target.speed)))
            / (target_turns * target_speed);
    } else {
        turns = (experience + 8) / target_turns;
    }

    if turns < 1 {
        turns = 1;
    }

    turns
}

#[cfg(test)]
#[path = "access_tests.rs"]
mod tests;
