//! Проходимость, занятость клеток и групповые приказы.
//!
//! Оракул: `max-cpp-port/src/access.cpp` (`Access_IsAccessible`,
//! `Access_GetModifiedSurfaceType`, `Access_GetAttackTarget`,
//! `Access_GetAttackTarget2`, `Access_GetTeamUnit`,
//! `Access_GetActiveUnitWithFlags`, `Access_GetReceiverUnit`,
//! `Access_CanReceiveUnits`, `Access_GetStoredUnitCount`,
//! `Access_IsWithinAttackRange`, `Access_GetSquaredDistance`,
//! `Access_GetApproximateDistance`, `Access_GetValidAttackTargetTypes`,
//! `Access_IsValidAttackTarget*`, `Access_GetAttackPosition` — последняя живёт в
//! `units_manager.cpp`, но по смыслу относится к доступу), `Access_SetUnitDestination`,
//! `Access_TriggerEnemyMine`, `Access_ProcessNewGroupOrder`,
//! `Access_ProcessGroupAirPath`; `paths.cpp` (`Paths_IsOccupied`); `unitinfo.cpp`
//! (`UnitInfo::AttemptSideStep`, `UnitInfo::AreTherePins`,
//! `UnitsManager_IsEngaged`/`IsEngagedByInfiltrator` из `units_manager.cpp`),
//! `units.cpp` (`SC_UNITS`: флаги и `land_type` 93 типов), DESIGN.md §2.
//!
//! # Таблица типов
//!
//! C++-порт читает флаги и `land_type` юнита из ресурса `SC_UNITS`
//! (`Units::LoadResource`, `assets/PATCHES.RES`). В `max_core` нет файлового I/O,
//! поэтому таблица [`UNIT_TYPE_PROPERTIES`] сгенерирована из этого же ресурса и
//! закоммичена (значения — `UNIT_FLAGS`/`SURFACE_TYPE_*` C++). Порядок строк —
//! порядок `UnitType` (0x00–0x5C), он же порядок `ResourceID`.
//!
//! # Занятость и группы
//!
//! `GameState` ведёт `MapHash` (`hash.rs`) — точный аналог `Hash_MapHash` C++
//! (клетка → юниты в порядке вставки; у зданий — 2×2 клетки). Групповые списки
//! `UnitInfoGroup` C++ моделируются параллельными таблицами `GameState`
//! (`unit_group`/`groups`): списки не сериализуются в V70, поэтому отдельного
//! поля в [`crate::unit::UnitInfo`] нет.

use crate::enums::unit_flags::{
    BUILDING, GROUND_COVER, HOVERING, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, MOBILE_SEA_UNIT,
    SELECTABLE, STATIONARY,
};
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::{Point, DIRECTION_OFFSETS};
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo};
use crate::unit_order;
use crate::unit_values::{ATTRIB_RANGE, ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS};

// ---------------------------------------------------------------------------
// Константы C++ (access.cpp / enums.hpp)
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

// ---------------------------------------------------------------------------
// Таблица свойств типов юнитов (SC_UNITS)
// ---------------------------------------------------------------------------

/// Флаги и проходимость типа юнита — поля `Unit` C++ (`units.json`/`SC_UNITS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitTypeProperties {
    /// `Unit::m_flags` (`unit_flags::*`).
    pub flags: u32,
    /// `Unit::m_land_type` (`SURFACE_TYPE_*`).
    pub land_type: u8,
}

impl UnitTypeProperties {
    const fn new(flags: u32, land_type: u8) -> Self {
        Self { flags, land_type }
    }
}

/// Свойства всех 93 типов — значения из `assets/PATCHES.RES` (`SC_UNITS`),
/// порядок совпадает с [`UnitType`].
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

/// Свойства типа юнита (`ResourceManager_GetUnit(type).GetFlags()/GetLandType()`).
#[inline]
pub fn unit_type_properties(unit_type: UnitType) -> UnitTypeProperties {
    UNIT_TYPE_PROPERTIES[unit_type as usize]
}

/// Флаги типа юнита (`ResourceManager_GetUnit(type).GetFlags()`).
#[inline]
pub fn unit_type_flags(unit_type: UnitType) -> u32 {
    unit_type_properties(unit_type).flags
}

/// Проходимость типа юнита (`ResourceManager_GetUnit(type).GetLandType()`).
#[inline]
pub fn unit_type_land_type(unit_type: UnitType) -> u8 {
    unit_type_properties(unit_type).land_type
}

// ---------------------------------------------------------------------------
// Поверхность
// ---------------------------------------------------------------------------

/// `Access_GetSurfaceType` (`access.cpp:936`) — сырая поверхность клетки.
#[inline]
pub fn surface_type(state: &GameState, grid: Point) -> u8 {
    state.map.surface_at(grid.x, grid.y).bits()
}

/// `Access_GetModifiedSurfaceType` (`access.cpp:950`): мост/платформа меняют
/// поверхность клетки для путей. `mode == true` — точка зрения морского юнита
/// (мост не скрывает воду), иначе наземного (и мост, и платформа = земля).
pub fn modified_surface_type(state: &GameState, grid: Point, mode: bool) -> u8 {
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
// Доступность клетки
// ---------------------------------------------------------------------------

/// `Access_IsAccessible` (`access.cpp:107`) — стоимость входа в клетку
/// (`0` — непроходима). `unit_type` — тип входящего юнита, `team` — его команда.
///
/// Это точный порт цикла по `Hash_MapHash[Point(grid)]`: порядок юнитов в
/// клетке — порядок вставки в ведёрко [`crate::hash::MapHash`].
pub fn is_accessible(
    state: &GameState,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
    flags: u32,
) -> u32 {
    let props = unit_type_properties(unit_type);
    let unit_flags = props.flags;

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
            && unit_flags & MOBILE_LAND_UNIT != 0
            && unit_type != UnitType::Surveyor
        {
            result = 4 + 8;
        }

        for id in state.units_at(grid).iter().copied() {
            let Some(other) = state.unit(id) else { continue };
            let other_flags = other.flags;

            if !(other.is_visible_to_team(team)
                || flags & ACCESS_MODIFIER_IGNORE_VISIBILITY != 0
                || (other.is_detected_by_team(team) && other_flags & STATIONARY != 0))
            {
                continue;
            }

            if other.orders == UnitOrder::Idle && other_flags & STATIONARY == 0 {
                continue;
            }

            if unit_flags & MOBILE_AIR_UNIT != 0 {
                if (flags & ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0
                    || (flags & ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0 && other.team != team))
                    && other_flags & MOBILE_AIR_UNIT != 0
                {
                    return 0;
                }
            } else {
                if flags & ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0
                    || (flags & ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0 && other.team != team)
                {
                    if other_flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT) != 0 {
                        return 0;
                    }

                    // Приземлившийся самолёт (не в полёте) блокирует наземный путь.
                    if other_flags & MOBILE_AIR_UNIT != 0 && other_flags & HOVERING == 0 {
                        return 0;
                    }
                }

                if unit_flags & STATIONARY != 0
                    && unit_type != UnitType::Connector
                    && (other.unit_type == UnitType::Bridge
                        || (other.unit_type as u16 >= UnitType::LargeRubble as u16
                            && other.unit_type as u16 <= UnitType::SmallCones as u16)
                        || other.unit_type == UnitType::LandMine
                        || other.unit_type == UnitType::SeaMine)
                {
                    return 0;
                }

                if other_flags & STATIONARY != 0 {
                    match other.unit_type {
                        UnitType::Road => {
                            if unit_type == UnitType::Road {
                                return 0;
                            }
                            result = 2;
                        }
                        UnitType::Bridge => {
                            result = 4;
                            if unit_flags & MOBILE_LAND_UNIT != 0 {
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
                            } else if unit_flags & STATIONARY != 0
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

/// Доступность для конкретного юнита (тип и команда берутся из него).
#[inline]
pub fn is_accessible_unit(state: &GameState, unit: &UnitInfo, grid: Point, flags: u32) -> u32 {
    is_accessible(state, unit.unit_type, unit.team, grid, flags)
}

/// `UnitInfo::IsBridgeElevated` (`unitinfo.cpp:3030`).
#[inline]
pub fn is_bridge_elevated(unit: &UnitInfo) -> bool {
    unit.unit_type == UnitType::Bridge && unit.image_index != unit.image_base
}

// ---------------------------------------------------------------------------
// Геометрия и радиус атаки
// ---------------------------------------------------------------------------

/// `Access_GetSquaredDistance` (`access.cpp`).
#[inline]
pub fn squared_distance(first: Point, second: Point) -> i32 {
    let dx = i32::from(first.x) - i32::from(second.x);
    let dy = i32::from(first.y) - i32::from(second.y);
    dx * dx + dy * dy
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
/// Для подлодок и корветов дополнительно проверяется, что линия огня не
/// пересекает сушу: C++ идёт по пиксельной линии и смотрит `Access_GetSurfaceType`.
pub fn is_within_attack_range(state: &GameState, unit: &UnitInfo, grid: Point, range: i32) -> bool {
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
// Цели атаки
// ---------------------------------------------------------------------------

/// `Access_GetValidAttackTargetTypes` (`access.cpp:1841`).
pub fn valid_attack_target_types(unit_type: UnitType) -> u32 {
    use UnitType::*;
    match unit_type {
        Scout | Commando | Infantry | Corvette | Battleship | Submarine | MissileBoat | Bomber
        | Juggernaut | AlienTank | AlienAssaultGun | Tank | AssaultGun | RocketLauncher
        | MissileCrawler | GunTurret | ArtilleryTurret | AntiMissile => {
            MOBILE_SEA_UNIT | MOBILE_LAND_UNIT | STATIONARY
        }
        FastBoat | Fighter | MobileAntiAir | AntiAir => MOBILE_AIR_UNIT,
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
pub fn is_valid_attack_target_ex(
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
pub fn is_valid_attack_target_point(
    state: &GameState,
    attacker: &UnitInfo,
    target: &UnitInfo,
    point: Point,
) -> bool {
    let mut target_flags = target.flags;
    let mut target_type = target.unit_type;

    if attacker.flags & MOBILE_AIR_UNIT != 0 && attacker.flags & HOVERING == 0 {
        return false;
    }

    if target_flags & MOBILE_AIR_UNIT != 0 && target_flags & HOVERING == 0 {
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
pub fn is_valid_attack_target(
    state: &GameState,
    attacker: &UnitInfo,
    target: &UnitInfo,
) -> bool {
    is_valid_attack_target_point(state, attacker, target, target.grid)
}

/// `Access_IsUnitBusyAtLocation` (`access.cpp:1949`).
pub fn is_unit_busy_at_location(state: &GameState, unit: &UnitInfo) -> bool {
    for id in state.units_at(unit.grid).iter().copied() {
        let Some(other) = state.unit(id) else { continue };
        if other.flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT | STATIONARY) != 0
            && other.flags & GROUND_COVER == 0
            && other.orders != UnitOrder::Idle
        {
            return true;
        }
    }
    false
}

/// `Access_GetAttackTarget` (`access.cpp:1999`).
pub fn get_attack_target(
    state: &GameState,
    unit: &UnitInfo,
    grid: Point,
    mode: bool,
) -> Option<UnitId> {
    let mut normal_unit = false;
    let mut result = None;

    if grid.x < 0 || grid.y < 0 || grid.x >= crate::geo::MAP_WIDTH || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else { continue };

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
                // `mode == true`: SpotByTeam выполняется вызывающим слоем
                // (C++ делает это здесь; в Rust нужна изменяемая ссылка).
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

/// `Access_GetAttackTarget2` (`access.cpp:2077`) — цель, ещё не видимая
/// атакующему (обнаружение при выстреле).
pub fn get_attack_target2(state: &GameState, unit: &UnitInfo, grid: Point) -> Option<UnitId> {
    if grid.x < 0 || grid.y < 0 || grid.x >= crate::geo::MAP_WIDTH || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else { continue };
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
pub fn get_team_unit(
    state: &GameState,
    grid: Point,
    team: TeamId,
    flags: u32,
) -> Option<UnitId> {
    if grid.x < 0 || grid.y < 0 || grid.x >= crate::geo::MAP_WIDTH || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    let mut result = None;

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else { continue };
        if other.team == team
            && other.flags & flags != 0
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
            let Some(other) = state.unit(id) else { continue };
            if other.team == team
                && other.flags & flags != 0
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
pub fn get_active_unit_with_flags(
    state: &GameState,
    grid: Point,
    flags: u32,
) -> Option<UnitId> {
    if grid.x < 0 || grid.y < 0 || grid.x >= crate::geo::MAP_WIDTH || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    for id in state.units_at(grid).iter().copied() {
        let Some(unit) = state.unit(id) else { continue };
        if unit.flags & flags != 0 && unit.flags & GROUND_COVER == 0 && unit.orders != UnitOrder::Idle
        {
            return Some(id);
        }
    }

    None
}

/// `Access_GetEnemyMineOnSentry` (`access.cpp:2050`).
pub fn get_enemy_mine_on_sentry(state: &GameState, team: TeamId, grid: Point) -> Option<UnitId> {
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

/// `Access_TriggerEnemyMine` (`access.cpp:2068`): обнаруженная мина взрывается.
pub fn trigger_enemy_mine(state: &mut GameState, team: TeamId, grid: Point) {
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
// Приём юнитов (транспорт/здания)
// ---------------------------------------------------------------------------

/// `Access_CanReceiveUnits` (`access.cpp:2150`).
#[inline]
pub fn can_receive_units(receiver: &UnitInfo) -> bool {
    receiver.hits > 0 && receiver.state != UnitOrderState::Destroy
}

/// `Access_GetReceiverUnit` (`access.cpp:2100`).
pub fn get_receiver_unit(state: &GameState, unit: &UnitInfo, grid: Point) -> Option<UnitId> {
    if grid.x < 0 || grid.y < 0 || grid.x >= crate::geo::MAP_WIDTH || grid.y >= crate::geo::MAP_HEIGHT
    {
        return None;
    }

    let team = unit.team;
    let flags = unit.flags;

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else { continue };
        if other.team != team || other.orders == UnitOrder::Idle || !can_receive_units(other) {
            continue;
        }

        if flags & MOBILE_LAND_UNIT != 0 {
            if (other.unit_type == UnitType::Barracks
                || other.unit_type == UnitType::PersonnelCarrier
                || other.unit_type == UnitType::SeaTransport)
                && (unit.unit_type == UnitType::Commando || unit.unit_type == UnitType::Infantry)
            {
                return Some(id);
            } else if (other.unit_type == UnitType::Depot
                || other.unit_type == UnitType::SeaTransport)
                && unit.unit_type != UnitType::Commando
                && unit.unit_type != UnitType::Infantry
            {
                return Some(id);
            }
        }

        if flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT) == MOBILE_SEA_UNIT
            && other.unit_type == UnitType::Dock
        {
            return Some(id);
        }

        if flags & MOBILE_AIR_UNIT != 0
            && (other.unit_type == UnitType::LandingPad || other.unit_type == UnitType::Hangar)
        {
            return Some(id);
        }
    }

    None
}

/// `Access_GetStoredUnitCount` (`access.cpp:1291`).
pub fn get_stored_unit_count(state: &GameState, receiver: &UnitInfo) -> i32 {
    let list = if receiver.unit_type == UnitType::Hangar {
        crate::unit::UnitListKind::MobileAir
    } else {
        crate::unit::UnitListKind::MobileLandSea
    };

    let mut result = 0;
    for id in state.unit_list(list) {
        let Some(unit) = state.unit(*id) else { continue };
        if unit.orders == UnitOrder::Idle && unit.parent_unit == Some(receiver.id) {
            result += 1;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Engaged / pins / stealth
// ---------------------------------------------------------------------------

/// `UnitInfo::AreTherePins` (`unitinfo.cpp`): цель держится живой атакой.
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

/// `UnitsManager_IsEngagedByInfiltrator` (`units_manager.cpp:1408`): диверсант
/// держит цель с момента приказа до разрешения действия.
pub fn is_engaged_by_infiltrator(state: &GameState, unit: &UnitInfo) -> bool {
    for id in state.unit_list(crate::unit::UnitListKind::MobileLandSea) {
        let Some(other) = state.unit(*id) else { continue };
        if other.unit_type == UnitType::Commando
            && other.parent_unit == Some(unit.id)
            && is_stealth_action_in_progress(other)
        {
            return true;
        }
    }
    false
}

/// `UnitsManager_IsEngaged` (`units_manager.cpp:1422`).
pub fn is_engaged(state: &GameState, unit: &UnitInfo) -> bool {
    are_there_pins(unit) || is_engaged_by_infiltrator(state, unit)
}

/// `UnitsManager_GetAttackPosition` (`units_manager.cpp:4847`).
pub fn get_attack_position(state: &GameState, attacker: &UnitInfo, target: &UnitInfo) -> Point {
    if target.unit_type == UnitType::Constructor && target.orders == UnitOrder::Build {
        // `Access_GetConstructionUtility` — P5-упрощение: сама стройплощадка.
    }

    let mut position = target.grid;

    if target.flags & BUILDING != 0 {
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

/// `Paths_IsOccupied` (`paths.cpp:439`): скрытный юнит (подлодка/диверсант/
/// транспорт) в клетке пути пытается отойти в сторону; иначе он раскрывается и
/// блокирует движение. Возвращает `true`, если клетка считается занятой.
pub fn paths_is_occupied(state: &mut GameState, grid: Point, angle: i32, team: TeamId) -> bool {
    let Some(ids) = state.units_at_opt(grid).map(|units| units.to_vec()) else {
        return false;
    };

    for id in ids {
        let Some(unit) = state.unit(id) else { continue };
        if (unit.unit_type == UnitType::Submarine
            || unit.unit_type == UnitType::Commando
            || unit.unit_type == UnitType::PersonnelCarrier)
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

/// `UnitInfo::AttemptSideStep` (`unitinfo.cpp:3243`) — сокращённый порт:
/// проверяются состояния/engagement/скорость, затем выбирается соседняя
/// клетка по тому же правилу (максимальный угол к направлению движения,
/// минимальная стоимость; для `CLNTRANS` приоритет — вода).
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
        // C++ проверяет целевую клетку, временно сняв юнита с `Hash_MapHash`, и
        // при успехе лишь останавливает его (`BlockedOnPathRequest(true)`):
        // фактическое смещение выполняет его собственный приказ.
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
        && (unit.state == UnitOrderState::InProgress
            || unit.state == UnitOrderState::InTransition)
    {
        return true;
    }

    let is_clntrans = unit.unit_type == UnitType::PersonnelCarrier;
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
            let mut step_cost = is_accessible_unit(
                state,
                &unit,
                position,
                ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) as i32;
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

            let candidate_is_water = is_clntrans
                && modified_surface_type(state, position, false) == SURFACE_TYPE_WATER;

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
/// Возвращает `true`, если клетка «уступит» (юнит уедет или есть мост), и
/// `false`, если путь надо искать заново.
pub fn set_unit_destination(
    state: &mut GameState,
    _unit: &UnitInfo,
    from: Point,
    to: Point,
    mode: bool,
) -> bool {
    let mut bridge = None;

    for id in state.units_at(to).to_vec() {
        let Some(other) = state.unit(id) else { continue };
        if other.flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT) != 0 {
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

/// `GroundPath::IsInPath` (`groundpath.cpp:146`) для относительной цели.
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
// Групповые приказы (`UnitInfoGroup`)
// ---------------------------------------------------------------------------

/// `Access_ProcessGroupAirPath` (`access.cpp`): воздушный путь члена группы.
pub fn process_group_air_path(state: &mut GameState, id: UnitId) -> bool {
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
/// В C++ группа — `SmartList<UnitInfo>`; здесь — `GameState::groups`.
/// Возвращает `false`, если путь ещё строится (лидер остаётся в
/// `PendingAirGroupLeader`), `true` — если групповой приказ разрешён.
pub fn process_new_group_order(state: &mut GameState, leader: UnitId) -> bool {
    let Some(group) = state.group_members(leader) else {
        return false;
    };

    for id in group.iter().copied() {
        let Some(unit) = state.unit(id) else { continue };
        if unit.state == UnitOrderState::NewOrder && unit.path.is_none() {
            if unit.hits > 0 {
                if unit.flags & MOBILE_AIR_UNIT != 0 && unit.orders != UnitOrder::MoveToAttack {
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
            let result = crate::pathfinding::request_path(state, id, ACCESS_MODIFIER_SAME_CLASS_BLOCKS);
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

/// Приписать юнитов группе с лидером `leader` (`AllocateUnitList` +
/// `AssignUnitList`). Возвращает номер группы.
pub fn assign_unit_group(state: &mut GameState, leader: UnitId, members: &[UnitId]) -> u32 {
    clear_unit_group(state, leader);
    let group_id = state.create_group_with_leader(leader);
    for id in members {
        state.assign_group_member(*id, group_id);
    }
    group_id
}

/// `UnitInfo::ClearUnitList` — снять юнита с группового списка.
pub fn clear_unit_group(state: &mut GameState, id: UnitId) {
    state.clear_group_member(id);
}

/// `Access_GroupAttackOrder` (`access.cpp`): групповой приказ атаки. В C++
/// обходит список группы; здесь — только маршруты членов (без `Ai_*`).
pub fn group_attack_order(state: &mut GameState, leader: UnitId, mode: bool) {
    let Some(group) = state.group_members(leader) else {
        return;
    };
    for id in group {
        let Some(unit) = state.unit(id) else { continue };
        if unit.flags & MOBILE_AIR_UNIT != 0 && unit.orders != UnitOrder::MoveToAttack {
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

/// Группа юнита (копия списка).
#[inline]
pub fn group_members_of(state: &GameState, id: UnitId) -> Option<Vec<UnitId>> {
    state.group_members(id)
}

// ---------------------------------------------------------------------------
// Вспомогательные величины C++ (`Access_*`)
// ---------------------------------------------------------------------------

/// `Access_IsTeamInUnitLists` (`access.cpp`) — есть ли у команды юнит в списках.
pub fn is_team_in_unit_lists(state: &GameState, team: TeamId) -> bool {
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

/// `Access_GetUnit2`-подобный поиск: первый живой юнит клетки.
#[inline]
pub fn first_unit_at(state: &GameState, grid: Point) -> Option<UnitId> {
    state.units_at(grid).first().copied()
}

/// `Access_IsAnyLandPresent` (`access.cpp:1030`) — есть ли земля в 2×2.
pub fn is_any_land_present(state: &GameState, grid: Point, flags: u32) -> bool {
    let is_land = |x: i16, y: i16| {
        surface_type(state, Point::new(x, y)) == SURFACE_TYPE_LAND
    };

    if flags & BUILDING != 0 {
        is_land(grid.x + 1, grid.y)
            || is_land(grid.x, grid.y + 1)
            || is_land(grid.x + 1, grid.y + 1)
            || is_land(grid.x, grid.y)
    } else {
        is_land(grid.x, grid.y)
    }
}

/// `Access_IsFullyLandCovered` (`access.cpp:1050`).
pub fn is_fully_land_covered(state: &GameState, grid: Point, flags: u32) -> bool {
    let is_land = |x: i16, y: i16| {
        surface_type(state, Point::new(x, y)) == SURFACE_TYPE_LAND
    };

    if flags & BUILDING == 0
        || (is_land(grid.x + 1, grid.y)
            && is_land(grid.x, grid.y + 1)
            && is_land(grid.x + 1, grid.y + 1))
    {
        is_land(grid.x, grid.y)
    } else {
        false
    }
}

/// `UnitInfo::GetAttackRange` (`unitinfo.cpp:3888`) — максимальная дистанция
/// атаки с учётом оставшегося хода.
pub fn get_attack_range(unit: &UnitInfo) -> i32 {
    let rounds = unit.base_values.get_attribute(crate::unit_values::ATTRIB_ROUNDS);
    if rounds > 0 {
        if unit.base_values.get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE) != 0 {
            unit.base_values.get_attribute(ATTRIB_RANGE) + unit.base_values.get_attribute(ATTRIB_SPEED) / 2
        } else {
            ((rounds - 1) * unit.base_values.get_attribute(ATTRIB_SPEED)) / rounds
                + unit.base_values.get_attribute(ATTRIB_RANGE)
        }
    } else {
        0
    }
}

/// `UnitsManager_IsAccessible` (`units_manager.cpp:4987`): площадка 2×2 для
/// здания (все четыре клетки проходимы для его типа).
pub fn is_accessible_for_building(
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

    is_accessible(state, unit_type, team, grid, ACCESS_MODIFIER_SAME_CLASS_BLOCKS) != 0
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

/// `Access_IsWithinMovementRange` (`access.cpp`): хватит ли оставшейся скорости
/// на текущий путь.
pub fn is_within_movement_range(state: &GameState, unit: &UnitInfo) -> bool {
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
                    let mut step_cost = is_accessible_unit(
                        state,
                        unit,
                        Point::new(x, y),
                        ACCESS_MODIFIER_NONE,
                    ) as i32;
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

/// `Access_GetRepairShopClientCount` (`access.cpp:1300`) — для P3-ремонта.
pub fn get_repair_shop_client_count(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
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
        let Some(unit) = state.unit(*id) else { continue };
        let unit_flags =
            unit.flags & (MOBILE_AIR_UNIT | MOBILE_LAND_UNIT | MOBILE_SEA_UNIT | ELECTRONIC_UNIT);
        if unit.team == team && unit_flags == flags && unit.hits > 0 {
            result += 1;
        }
    }
    result
}

/// `Task_IsAdjacent`-подобная проверка для стелс-действий: цель в клетке
/// по соседству (включая диагонали).
pub fn is_adjacent_to_grid(unit: &UnitInfo, grid: Point) -> bool {
    let dx = (i32::from(unit.grid.x) - i32::from(grid.x)).abs();
    let dy = (i32::from(unit.grid.y) - i32::from(grid.y)).abs();
    dx <= 1 && dy <= 1 && (dx != 0 || dy != 0)
}

/// Значение атрибута `storage` цели (`ComputeUnitStorage`): используется
/// `AttemptStealthAction` для проверки заполненности транспорта.
pub fn is_transporter_full(unit: &UnitInfo) -> bool {
    i32::from(unit.storage) >= unit.base_values.get_attribute(ATTRIB_STORAGE)
}

/// `Access_DestroyUtilities` (`access.cpp`): снимает с клетки утилиты —
/// ленты/конусы всегда, дорогу/плиты/обломки/коннекторы — по флагам.
pub fn destroy_utilities(
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
            UnitType::SmallTape | UnitType::LargeTape | UnitType::SmallCones | UnitType::LargeCones => true,
            UnitType::Road => remove_road,
            UnitType::SmallSlab | UnitType::LargeSlab => remove_slabs,
            // `UnitsManager_RemoveConnections` — P3-логика комплексов.
            UnitType::Connector => remove_connectors,
            UnitType::SmallRubble | UnitType::LargeRubble => remove_rubble,
            _ => false,
        };

        if destroy {
            state.destroy_unit(id);
        }
    }
}

/// `Access_DestroyGroundCovers` (`access.cpp`): убирает дорогу/площадку/
/// платформу/мост под юнитом (здание занимает 2×2).
pub fn destroy_ground_covers(state: &mut GameState, unit: &UnitInfo) {
    let positions = if unit.flags & BUILDING != 0 {
        [
            Point::new(unit.grid.x, unit.grid.y),
            Point::new(unit.grid.x + 1, unit.grid.y),
            Point::new(unit.grid.x, unit.grid.y + 1),
            Point::new(unit.grid.x + 1, unit.grid.y + 1),
        ]
    } else {
        [unit.grid; 4]
    };
    let count = if unit.flags & BUILDING != 0 { 4 } else { 1 };

    for position in positions.iter().take(count) {
        for id in state.units_at(*position).to_vec() {
            let Some(other) = state.unit(id) else { continue };
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

/// `UnitsManager_IsUnloadSiteTaken` (`units_manager.cpp:4074`) — занято ли
/// место высадки другим воздушным транспортом.
///
/// Портировано полностью: обход `UnitsManager_MobileAirUnits` в порядке
/// вставки.
pub fn is_unload_site_taken(state: &GameState, unit: &UnitInfo) -> bool {
    for id in state.unit_list(crate::unit::UnitListKind::MobileAir) {
        let Some(other) = state.unit(*id) else { continue };
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

/// Число ходов, на которое диверсант отключает цель
/// (`UnitsManager_DisableUnit`, `units_manager.cpp:4395`).
pub fn disable_turns(infiltrator: &UnitInfo, target: &UnitInfo) -> i32 {
    let target_speed = target.base_values.get_attribute(ATTRIB_SPEED);
    let target_turns = target.base_values.get_attribute(ATTRIB_TURNS);
    let experience = i32::from(infiltrator.experience);
    let mut turns;

    if target_turns == 0 {
        // C++ делит на `ATTRIB_TURNS` (риск 15); у игровых типов 0 недостижим.
        return 1;
    }

    if target_speed != 0 {
        turns = ((target_speed * (experience + 8))
            - (target_turns * i32::from(target.speed)))
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
mod tests {
    use super::*;
    use crate::enums::unit_flags::{
        ELECTRONIC_UNIT, GROUND_COVER, MOBILE_LAND_UNIT, SELECTABLE,
    };
    use crate::enums::{Surface, TeamClan};
    use crate::team::TeamKind;

    fn state() -> GameState {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Player;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        state.team_units[1] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        state
    }

    fn spawn(
        state: &mut GameState,
        unit_type: UnitType,
        team: TeamId,
        grid: Point,
        flags: u32,
    ) -> UnitId {
        state.spawn_unit(unit_type, team, grid, flags).expect("spawn")
    }

    #[test]
    fn unit_type_table_matches_resource_units() {
        assert_eq!(UNIT_TYPE_PROPERTIES.len(), 93);
        assert_eq!(unit_type_flags(UnitType::CommTower), 0x0160_4210);
        assert_eq!(unit_type_land_type(UnitType::CommTower), SURFACE_TYPE_LAND);
        assert_eq!(unit_type_land_type(UnitType::Fighter), SURFACE_TYPE_ALL);
        assert_eq!(unit_type_land_type(UnitType::PersonnelCarrier), 0x7);
        assert_eq!(unit_type_flags(UnitType::Tank) & MOBILE_LAND_UNIT, MOBILE_LAND_UNIT);
        assert_eq!(unit_type_flags(UnitType::CommTower) & ELECTRONIC_UNIT, ELECTRONIC_UNIT);
        assert_eq!(unit_type_flags(UnitType::Rocket) & crate::enums::unit_flags::MISSILE_UNIT, crate::enums::unit_flags::MISSILE_UNIT);
    }

    #[test]
    fn surface_rules_follow_land_type() {
        let mut state = state();
        let same = ACCESS_MODIFIER_SAME_CLASS_BLOCKS;

        // Земля: наземный юнит проходит.
        assert_eq!(is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(5, 5), same), 4);

        // Вода: танк — нет, surveyor (LAND|WATER|COAST) — да.
        state.map.surface.set(6, 5, Surface::WATER);
        assert_eq!(is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(6, 5), same), 0);
        assert_eq!(is_accessible(&state, UnitType::Surveyor, TeamId::RED, Point::new(6, 5), same), 4);
        // Воздушный юнит (land_type = ALL) проходит над водой.
        assert_eq!(is_accessible(&state, UnitType::Fighter, TeamId::RED, Point::new(6, 5), same), 4);
        // Корабль на земле — нет, амфибия CLNTRANS (0x7) — да.
        assert_eq!(is_accessible(&state, UnitType::Corvette, TeamId::RED, Point::new(5, 6), same), 0);
        assert_eq!(is_accessible(&state, UnitType::PersonnelCarrier, TeamId::RED, Point::new(5, 6), same), 4);

        // Вне карты — 0.
        assert_eq!(is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(-1, 0), same), 0);
        assert_eq!(is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(112, 0), same), 0);
    }

    #[test]
    fn stationary_units_change_cost_and_block() {
        let mut state = state();
        let same = ACCESS_MODIFIER_SAME_CLASS_BLOCKS;

        // Плита: проезд по ней дешевле (2).
        spawn(
            &mut state,
            UnitType::SmallSlab,
            TeamId::RED,
            Point::new(7, 5),
            STATIONARY | GROUND_COVER,
        );
        assert_eq!(is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(7, 5), same), 2);
        // Дорога: колёсному ходу дешевле, второй дороге — нельзя.
        spawn(
            &mut state,
            UnitType::Road,
            TeamId::RED,
            Point::new(8, 5),
            STATIONARY | GROUND_COVER,
        );
        assert_eq!(is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(8, 5), same), 2);
        assert_eq!(is_accessible(&state, UnitType::Road, TeamId::RED, Point::new(8, 5), same), 0);

        // Обнаруженная мина врага блокирует клетку.
        let mine = spawn(
            &mut state,
            UnitType::LandMine,
            TeamId::GREEN,
            Point::new(9, 5),
            crate::enums::unit_flags::GROUND_COVER | STATIONARY,
        );
        {
            let unit = state.unit_mut(mine).unwrap();
            unit.spot_by_team(TeamId::RED);
        }
        assert_eq!(is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(9, 5), same), 0);
        // Своя мина не блокирует.
        assert_ne!(is_accessible(&state, UnitType::Tank, TeamId::GREEN, Point::new(9, 5), same), 0);
    }

    #[test]
    fn bridge_and_platform_modify_surface() {
        let mut state = state();
        let same = ACCESS_MODIFIER_SAME_CLASS_BLOCKS;
        state.map.surface.set(10, 5, Surface::WATER);
        spawn(
            &mut state,
            UnitType::Bridge,
            TeamId::RED,
            Point::new(10, 5),
            STATIONARY | GROUND_COVER,
        );

        // Наземный: по мосту (2), поверхность — земля.
        assert_eq!(is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(10, 5), same), 2);
        assert_eq!(
            modified_surface_type(&state, Point::new(10, 5), false),
            SURFACE_TYPE_LAND
        );
        // Морской: мост — вода, стоимость 4.
        assert_eq!(is_accessible(&state, UnitType::Corvette, TeamId::RED, Point::new(10, 5), same), 4);
        assert_eq!(
            modified_surface_type(&state, Point::new(10, 5), true),
            SURFACE_TYPE_WATER
        );
    }

    #[test]
    fn pins_and_infiltrator_make_unit_engaged() {
        let mut state = state();
        let tank = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        );
        let mut unit = state.unit(tank).unwrap().clone();
        assert!(!is_engaged(&state, &unit));

        unit.pin_count = 1;
        assert!(is_engaged(&state, &unit));
        assert!(are_there_pins(&unit));

        unit.pin_count = 0;
        unit.orders = UnitOrder::Await;
        state.write_unit(tank, unit);

        // Диверсант, «держащий» цель, тоже блокирует её.
        let commando = spawn(
            &mut state,
            UnitType::Commando,
            TeamId::GREEN,
            Point::new(5, 6),
            MOBILE_LAND_UNIT | SELECTABLE,
        );
        {
            let unit = state.unit_mut(commando).unwrap();
            unit.parent_unit = Some(tank);
            unit.orders = UnitOrder::AwaitStealUnit;
            unit.state = UnitOrderState::InProgress;
        }
        let tank_unit = state.unit(tank).unwrap().clone();
        assert!(is_engaged(&state, &tank_unit));
        assert!(is_engaged_by_infiltrator(&state, &tank_unit));
    }

    #[test]
    fn receiver_unit_accepts_commando() {
        let mut state = state();
        let barracks = spawn(
            &mut state,
            UnitType::Barracks,
            TeamId::RED,
            Point::new(5, 5),
            STATIONARY | crate::enums::unit_flags::BUILDING,
        );
        {
            let unit = state.unit_mut(barracks).unwrap();
            unit.orders = UnitOrder::PowerOn;
            unit.state = UnitOrderState::Init;
        }
        let commando = state
            .spawn_unit(
                UnitType::Commando,
                TeamId::RED,
                Point::new(5, 6),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        let unit = state.unit(commando).unwrap().clone();
        assert_eq!(
            get_receiver_unit(&state, &unit, Point::new(5, 5)),
            Some(barracks)
        );
        assert!(can_receive_units(state.unit(barracks).unwrap()));

        // Танк в казарму не входит.
        let tank = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::RED,
                Point::new(5, 7),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        let tank_unit = state.unit(tank).unwrap().clone();
        assert_eq!(get_receiver_unit(&state, &tank_unit, Point::new(5, 5)), None);
    }

    #[test]
    fn attack_target_selection_respects_state_and_flags() {
        let mut state = state();
        let attacker = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        );
        let defender = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(5, 6),
            MOBILE_LAND_UNIT | SELECTABLE,
        );
        {
            let unit = state.unit_mut(defender).unwrap();
            unit.orders = UnitOrder::Sentry;
            unit.state = UnitOrderState::ExecutingOrder;
        }
        let attacker_unit = state.unit(attacker).unwrap().clone();
        assert_eq!(
            get_attack_target(&state, &attacker_unit, Point::new(5, 6), false),
            Some(defender)
        );

        // IDLE-юнит не является целью.
        state.unit_mut(defender).unwrap().orders = UnitOrder::Idle;
        assert_eq!(
            get_attack_target(&state, &attacker_unit, Point::new(5, 6), false),
            None
        );

        // Пустая/внекартная клетка.
        assert_eq!(
            get_attack_target(&state, &attacker_unit, Point::new(9, 9), false),
            None
        );
        assert_eq!(
            get_attack_target(&state, &attacker_unit, Point::new(-1, 5), false),
            None
        );
    }

    #[test]
    fn group_order_is_dissolved_when_members_are_cleared() {
        let mut state = state();
        let leader = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        );
        let wing = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::RED,
            Point::new(6, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        );
        assign_unit_group(&mut state, leader, &[wing]);
        assert_eq!(state.group_members(leader), Some(vec![leader, wing]));

        clear_unit_group(&mut state, wing);
        assert_eq!(state.group_members(leader), Some(vec![leader]));
        clear_unit_group(&mut state, leader);
        assert_eq!(state.group_members(leader), None);
    }
}
