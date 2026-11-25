//! Victory/defeat — `winloss.cpp`, `winlosshandler.cpp`, `scripter.cpp` (P7).
//!
//! # Oracle
//!
//! | What | Where |
//! |---|---|
//! | `WinLoss_GetWorthOfAssets`, `WinLoss_GetTotalWorthOfAssets`, `WinLoss_EvaluateStatus` | `max-cpp-port/src/winloss.cpp` |
//! | common condition test (`TestWinLossConditions` → Lua) | `winlosshandler.cpp` |
//! | `max_*` primitive predicates | `scripter.cpp:159-1149` |
//! | `Cargo_*` (consumption/mining/production) | `cargo.cpp:30-271` |
//! | `GetNormalRateBuildCost` | `unitinfo.cpp:3719` |
//! | `MAX_REGISTRY.*` values | `maxregistryhandler.cpp:33-163` |
//! | mission victory/defeat conditions (Lua) | `assets/PATCHES.RES` → `SC_L0002`, `SC_T0001..15`, `SC_S0001..24`, `SC_C0001..09` |
//! | generic end-game (`menu_check_end_game_conditions`) | `menu.cpp:548` |
//! | rule description | `max-cpp-port-max-brh-docs/save.md` → *Victory and Loss Conditions* |
//!
//! # Model
//!
//! 1. [`predicates`] — pure ports of the Lua API (`max_*`): unit counters,
//!    losses, resources, points, research, turns, mining/consumption, unit
//!    state and three "infrastructure" predicates (military power, production,
//!    construction).
//! 2. [`mission_win_loss`] — team status by mission rules. Mission scripts
//!    from the port's resources are rewritten 1:1 (each branch's comment contains the original
//!    Lua), so this is **rule data**, not a Lua interpreter (DESIGN §7,
//!    risk 8). `mission_index == 0` and categories without a script give
//!    [`default_win_loss_conditions`] (`SC_L0002`).
//! 3. [`evaluate_status`] — a verbatim port of `WinLoss_EvaluateStatus`: statuses,
//!    team counters, `teams_in_play` sorting (points, then "base worth"
//!    [`get_total_worth_of_assets`]) and ranks.
//! 4. [`check_end_game_conditions`] — a port of `menu_check_end_game_conditions`
//!    (generic SCORE/DURATION rules, "last survivor", "all humans"
//!    eliminated"), called from [`crate::game::GameState::finish_turn`].
//!
//! # Intentional divergences
//!
//! * C++ `WinLoss_EvaluateStatus(turn_counter)` **does not use** the argument
//!   (all predicates read `GameManager_TurnCounter`); Rust does the same — the parameter
//!   is kept for signature compatibility.
//! * `Cargo_GetNetProduction` for factories in `ORDER_BUILD` uses
//!   `GetMaxAllowedBuildRate()`; in max_core this is `build_rate` without the
//!   `BuildMenu_GetMaxPossibleBuildRate` clamp (`BuildMenu` data is needed, P3) — this
//!   does not affect mission predicates (see `tests`).
//! * `menu_check_end_game_conditions` in C++ draws the results screen; here only
//!   the fact that the game ended is returned and (for the non-network branch) the team is marked
//!   `TeamKind::Eliminated` — as `menu.cpp:605`.

use crate::classification::UnitTypeClassify;
use crate::enums::{
    unit_flags, Opponent, ResearchTopic, UnitOrder, UnitOrderState, UnitType, VictoryType,
    WinLossState,
};
use crate::game::{GameState, PLAYER_TEAM_COUNT, TEAM_SLOTS};
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitInfo, UnitListKind};
use crate::unit_values::{
    ATTRIB_AMMO, ATTRIB_HITS, ATTRIB_ROUNDS, ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS,
};

// ---------------------------------------------------------------------------
// Unit cargo type — Unit::CargoType (unit.hpp), SC_UNITS data
// ---------------------------------------------------------------------------

/// `Unit::CargoType` (`unit.hpp`), values as in C++.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CargoType {
    /// `CARGO_TYPE_NONE`.
    None = 0,
    /// `CARGO_TYPE_RAW`.
    Raw = 1,
    /// `CARGO_TYPE_FUEL`.
    Fuel = 2,
    /// `CARGO_TYPE_GOLD`.
    Gold = 3,
    /// `CARGO_TYPE_LAND` (transport).
    Land = 4,
    /// `CARGO_TYPE_SEA` (transport).
    Sea = 5,
    /// `CARGO_TYPE_AIR` (transport).
    Air = 6,
}

/// `ResourceManager_GetUnit(type).GetCargoType()`.
///
/// The table — from `assets/PATCHES.RES` → `SC_UNITS` (the JSON description of `units.cpp`),
/// grouped by cargo type. In C++ the field is read from the resource when loading
/// unit data; here — const data (DESIGN §1.4 item 4).
pub fn cargo_type_of(unit_type: UnitType) -> CargoType {
    match unit_type {
        UnitType::Barracks
        | UnitType::Depot
        | UnitType::ArmouredPersonnelCarrier
        | UnitType::SeaTransport
        | UnitType::AirTransport => CargoType::Land,
        UnitType::MaterialStorage
        | UnitType::MiningStation
        | UnitType::Constructor
        | UnitType::MineLayer
        | UnitType::SupplyTruck
        | UnitType::Engineer
        | UnitType::Bulldozer
        | UnitType::RepairUnit
        | UnitType::SeaMineLayer
        | UnitType::CargoShip => CargoType::Raw,
        UnitType::FuelStorage | UnitType::FuelTruck => CargoType::Fuel,
        UnitType::GoldVault | UnitType::GoldTruck => CargoType::Gold,
        UnitType::Hangar | UnitType::LandingPad => CargoType::Air,
        UnitType::Dock => CargoType::Sea,
        _ => CargoType::None,
    }
}

/// `GetRelevantUnits(unit_type)` (`scripter.cpp:140`): the unit list in which
/// the predicates search for units of this type.
///
/// Check order — as in C++ (`STATIONARY` first, then `MOBILE_AIR`, otherwise
/// `MOBILE_LAND_SEA`). Therefore `GROUND_COVER` types without `STATIONARY` (e.g.
/// `TORPEDO`) land in `MobileLandSea`, not in `GroundCover`.
pub fn relevant_list(unit_type: UnitType) -> UnitListKind {
    let flags = crate::access::unit_type_flags(unit_type);

    if !unit_type.is_mobile() {
        if flags & unit_flags::GROUND_COVER != 0 {
            UnitListKind::GroundCover
        } else {
            UnitListKind::Stationary
        }
    } else if unit_type.is_air() {
        UnitListKind::MobileAir
    } else {
        UnitListKind::MobileLandSea
    }
}

/// `MAX_UNIT_CLASS` in the `max_has_attack_power` Lua predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitClass {
    /// `MOBILE_LAND_UNIT | MOBILE_SEA_UNIT`.
    MobileLandSea,
    /// `MOBILE_AIR_UNIT`.
    MobileAir,
    /// `STATIONARY`.
    Stationary,
}

// ---------------------------------------------------------------------------
// Primitive predicates (scripter.cpp → Lua API)
// ---------------------------------------------------------------------------

/// `HasMaterials()` (`scripter.cpp:159`): the sum of `storage` of the team's units with
/// the given cargo type over the `Stationary` and `MobileLandSea` lists.
pub(crate) fn has_materials(state: &GameState, team: TeamId, cargo: CargoType) -> i32 {
    let mut cargo_sum = 0i32;

    for kind in [UnitListKind::Stationary, UnitListKind::MobileLandSea] {
        for &id in state.unit_list(kind) {
            let Some(unit) = state.unit(id) else { continue };
            if unit.team == team && cargo_type_of(unit.unit_type) == cargo {
                cargo_sum += i32::from(unit.storage);
            }
        }
    }

    cargo_sum
}

/// `CountReadyUnits()` (`scripter.cpp:204`): units of the type that are **ready** (state
/// is not `ORDER_STATE_BUILDING_READY`).
pub(crate) fn count_ready_units(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
    let mut ready_units = 0i32;

    for &id in state.unit_list(relevant_list(unit_type)) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.unit_type == unit_type
            && unit.state != UnitOrderState::BuildingReady
        {
            ready_units += 1;
        }
    }

    ready_units
}

/// `HasAttackPower()` (`scripter.cpp:244`): there is a unit of the class with `ammo > 0`,
/// not `SUBMARNE` and not `COMMANDO`.
pub(crate) fn has_attack_power(state: &GameState, team: TeamId, class: UnitClass) -> bool {
    let kind = match class {
        UnitClass::MobileLandSea => UnitListKind::MobileLandSea,
        UnitClass::MobileAir => UnitListKind::MobileAir,
        UnitClass::Stationary => UnitListKind::Stationary,
    };

    for &id in state.unit_list(kind) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.ammo > 0
            && unit.unit_type != UnitType::Submarine
            && unit.unit_type != UnitType::Infiltrator
        {
            return true;
        }
    }

    false
}

/// `HasProductionInfrastructure()` (`scripter.cpp:296`): a factory + fuel ≥ 8 and
/// materials ≥ 24, otherwise a mine; it covers the power demand (`power_demand`
/// 1 or 2).
pub(crate) fn has_production_infrastructure(state: &GameState, team: TeamId) -> bool {
    let has_plant = state.unit_list(UnitListKind::Stationary).iter().any(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.team == team
                && matches!(
                    unit.unit_type,
                    UnitType::Shipyard
                        | UnitType::LightVehiclePlant
                        | UnitType::HeavyVehiclePlant
                        | UnitType::AirUnitsPlant
                        | UnitType::TrainingHall
                )
        })
    });

    if !has_plant {
        return false;
    }

    let sum_cargo_fuel = has_materials(state, team, CargoType::Fuel);
    let sum_cargo_materials = has_materials(state, team, CargoType::Raw);
    let mut power_demand = 1i32;

    if sum_cargo_fuel < 8 || sum_cargo_materials < 24 {
        let has_mining_station = state.unit_list(UnitListKind::Stationary).iter().any(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::MiningStation)
        });

        if has_mining_station {
            power_demand = 2;
        } else {
            return false;
        }
    }

    let mut sum_cargo_generated_power = 0i32;
    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team {
            let rate = cargo_power_consumption_rate(unit.unit_type);
            if rate < 0 {
                sum_cargo_generated_power -= rate;
            }
        }
    }

    sum_cargo_generated_power >= power_demand
}

/// `HasConstructionInfrastructure()` (`scripter.cpp:372`): constructor +
/// engineer + enough materials (38, 8 with a mine, −8 with own
/// generation).
pub(crate) fn has_construction_infrastructure(state: &GameState, team: TeamId) -> bool {
    let has_constructor = state
        .unit_list(UnitListKind::MobileLandSea)
        .iter()
        .any(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::Constructor)
        });

    if !has_constructor {
        return false;
    }

    let has_engineer = state
        .unit_list(UnitListKind::MobileLandSea)
        .iter()
        .any(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::Engineer)
        });

    if !has_engineer {
        return false;
    }

    let mut resources_needed = 38i32;

    if state.unit_list(UnitListKind::Stationary).iter().any(|id| {
        state
            .unit(*id)
            .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::MiningStation)
    }) {
        resources_needed = 8;
    }

    if state.unit_list(UnitListKind::Stationary).iter().any(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.team == team && cargo_power_consumption_rate(unit.unit_type) < 0
        })
    }) {
        resources_needed -= 8;
    }

    resources_needed == 0 || resources_needed <= has_materials(state, team, CargoType::Raw)
}

/// `CountTotalMining()` (`scripter.cpp:454`): the total **maximums** of mining
/// of enabled mines (`ORDER_POWER_ON`) — `(raw, fuel, gold)`.
pub(crate) fn count_total_mining(state: &GameState, team: TeamId) -> (i32, i32, i32) {
    let mut raw = 0i32;
    let mut fuel = 0i32;
    let mut gold = 0i32;

    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.unit_type == UnitType::MiningStation
            && unit.orders == UnitOrder::PowerOn
        {
            raw += i32::from(unit.raw_mining_max);
            fuel += i32::from(unit.fuel_mining_max);
            gold += i32::from(unit.gold_mining_max);
        }
    }

    (raw, fuel, gold)
}

/// `GetTotalUnitsBeingConstructed()` (`scripter.cpp:498`).
///
/// A curious C++ detail: if the **constructed** type is stationary, the list of
/// mobile ground/naval units is iterated, and vice versa (the list contains
/// "producers").
pub(crate) fn get_total_units_being_constructed(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
) -> i32 {
    let kind = if !unit_type.is_mobile() {
        UnitListKind::MobileLandSea
    } else {
        UnitListKind::Stationary
    };

    let mut result = 0i32;

    for &id in state.unit_list(kind) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.orders == UnitOrder::Build
            && unit.build_time != 0
            && unit.build_list.first() == Some(&unit_type)
        {
            result += 1;
        }
    }

    result
}

/// `GetTotalPowerConsumption()` (`scripter.cpp:547`) — the number of stationary units
/// of the type in state `POWER_ON`/`BUILD`.
pub(crate) fn get_total_power_consumption(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
    let mut result = 0i32;

    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.unit_type == unit_type
            && (unit.orders == UnitOrder::PowerOn || unit.orders == UnitOrder::Build)
        {
            result += 1;
        }
    }

    result
}

/// `GetTotalMining()` (`scripter.cpp:588`) — the actual mining of enabled mines.
pub(crate) fn get_total_mining(state: &GameState, team: TeamId, cargo: CargoType) -> i32 {
    let mut result = 0i32;

    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.unit_type == UnitType::MiningStation
            && unit.orders == UnitOrder::PowerOn
        {
            result += match cargo {
                CargoType::Raw => i32::from(unit.raw_mining),
                CargoType::Fuel => i32::from(unit.fuel_mining),
                CargoType::Gold => i32::from(unit.gold_mining),
                _ => 0,
            };
        }
    }

    result
}

/// `GetTotalConsumption()` (`scripter.cpp:638`) — the total consumption
/// (negative components of `Cargo_GetNetProduction`).
pub(crate) fn get_total_consumption(state: &GameState, team: TeamId, cargo: CargoType) -> i32 {
    let mut result = 0i32;

    for &id in state.unit_list(UnitListKind::Stationary) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team != team {
            continue;
        }

        let net = cargo_get_net_production(state, unit, false);
        match cargo {
            CargoType::Raw if net.raw < 0 => result -= net.raw,
            CargoType::Fuel if net.fuel < 0 => result -= net.fuel,
            CargoType::Gold if net.gold < 0 => result -= net.gold,
            _ => {}
        }
    }

    result
}

/// `HasUnitAboveMarkLevel()` (`scripter.cpp:699`) — the unit version (mark level)
/// is above the given one; **only** the `MobileLandSea` list is checked (as in C++).
pub(crate) fn has_unit_above_mark_level(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    mark_level: i32,
) -> bool {
    for &id in state.unit_list(UnitListKind::MobileLandSea) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.unit_type == unit_type
            && i32::from(unit.base_values.version) > mark_level
        {
            return true;
        }
    }

    false
}

/// `HasUnitExperience()` (`scripter.cpp:741`): there is a unit of the type with `storage > 0`
/// (for `COMMANDO`/`REGENERATING_UNIT` `storage` is experience).
pub(crate) fn has_unit_experience(state: &GameState, team: TeamId, unit_type: UnitType) -> bool {
    for &id in state.unit_list(relevant_list(unit_type)) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team && unit.unit_type == unit_type && unit.storage > 0 {
            return true;
        }
    }

    false
}

/// `CountCasualties()` (`scripter.cpp:780`).
pub(crate) fn count_casualties(state: &GameState, team: TeamId, unit_type: UnitType) -> u32 {
    state
        .teams
        .get(team.0 as usize)
        .map(|info| info.casualties_for(unit_type as u16))
        .unwrap_or(0)
}

/// `CountDamangedUnits()` (`scripter.cpp:809`) — units with `hits != base hits`.
pub(crate) fn count_damaged_units(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
    let mut result = 0i32;

    for &id in state.unit_list(relevant_list(unit_type)) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.unit_type == unit_type
            && i32::from(unit.hits) != unit.base_values.get_attribute(ATTRIB_HITS)
        {
            result += 1;
        }
    }

    result
}

/// `CountUnitsAboveAttribLevel()` (`scripter.cpp:850`).
#[allow(dead_code)] // Oracle trace: C++ parity, no gameplay caller yet.
pub(crate) fn count_units_above_attrib_level(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    attribute: u8,
    level: i32,
) -> i32 {
    let mut result = 0i32;

    for &id in state.unit_list(relevant_list(unit_type)) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team
            && unit.unit_type == unit_type
            && unit.base_values.get_attribute(attribute) > level
        {
            result += 1;
        }
    }

    result
}

/// `CountUnitsWithReducedAttrib()` (`scripter.cpp:900`): a unit attribute is below
/// the base (`ammo`, `speed`, `shots`/`rounds`, `hits`, `storage`).
pub(crate) fn count_units_with_reduced_attrib(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    attribute: u8,
) -> i32 {
    let mut result = 0i32;

    for &id in state.unit_list(relevant_list(unit_type)) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team != team || unit.unit_type != unit_type {
            continue;
        }

        let is_qualified = match attribute {
            ATTRIB_AMMO => i32::from(unit.ammo) != unit.base_values.get_attribute(ATTRIB_AMMO),
            ATTRIB_SPEED => i32::from(unit.speed) != unit.base_values.get_attribute(ATTRIB_SPEED),
            ATTRIB_ROUNDS => i32::from(unit.shots) != unit.base_values.get_attribute(ATTRIB_ROUNDS),
            ATTRIB_HITS => i32::from(unit.hits) != unit.base_values.get_attribute(ATTRIB_HITS),
            ATTRIB_STORAGE => {
                i32::from(unit.storage) != unit.base_values.get_attribute(ATTRIB_STORAGE)
            }
            _ => false,
        };

        if is_qualified {
            result += 1;
        }
    }

    result
}

/// `GetResearchLevel()` (`scripter.cpp:976`).
pub(crate) fn get_research_level(state: &GameState, team: TeamId, topic: ResearchTopic) -> u32 {
    state
        .teams
        .get(team.0 as usize)
        .map(|info| info.research_topics[topic as usize].research_level)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Cargo_* — cargo.cpp
// ---------------------------------------------------------------------------

/// `Cargo` in the `Cargo_GetNetProduction` scope (C++ `Cargo` — cargo.hpp, fields
/// are `int16_t`; here i32 until narrowing, as in the C++ formulas).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetProduction {
    /// Gold.
    pub gold: i32,
    /// Materials.
    pub raw: i32,
    /// Fuel.
    pub fuel: i32,
    /// Power.
    pub power: i32,
    /// Life support.
    pub life: i32,
    /// Free complex capacity.
    pub free_capacity: i32,
}

/// `Cargo_GetRawConsumptionRate(unit_type, speed_multiplier)` (`cargo.cpp:140`).
pub fn cargo_raw_consumption_rate(unit_type: UnitType, speed_multiplier: i32) -> i32 {
    let multiplier = match speed_multiplier {
        2 => 4,
        4 => 12,
        other => other,
    };

    match unit_type {
        UnitType::TrainingHall => multiplier,
        UnitType::Engineer | UnitType::Constructor => multiplier * 2,
        UnitType::Shipyard
        | UnitType::HeavyVehiclePlant
        | UnitType::AirUnitsPlant
        | UnitType::LightVehiclePlant => multiplier * 3,
        _ => 0,
    }
}

/// `Cargo_GetFuelConsumptionRate` (`cargo.cpp:183`).
pub fn cargo_fuel_consumption_rate(unit_type: UnitType) -> i32 {
    match unit_type {
        UnitType::PowerStation => 6,
        UnitType::PowerGenerator => 2,
        _ => 0,
    }
}

/// `Cargo_GetPowerConsumptionRate` (`cargo.cpp:203`).
pub fn cargo_power_consumption_rate(unit_type: UnitType) -> i32 {
    match unit_type {
        UnitType::Shipyard
        | UnitType::HeavyVehiclePlant
        | UnitType::AirUnitsPlant
        | UnitType::LightVehiclePlant
        | UnitType::TrainingHall
        | UnitType::EcoSphere
        | UnitType::ResearchCentre
        | UnitType::GoldRefinery
        | UnitType::MiningStation => 1,
        UnitType::PowerStation => -6,
        UnitType::PowerGenerator => -1,
        _ => 0,
    }
}

/// `Cargo_GetLifeConsumptionRate` (`cargo.cpp:235`).
pub fn cargo_life_consumption_rate(unit_type: UnitType) -> i32 {
    match unit_type {
        UnitType::Habitat => -3,
        UnitType::TrainingHall | UnitType::EcoSphere | UnitType::ResearchCentre => 1,
        _ => 0,
    }
}

/// `Cargo_GetGoldConsumptionRate` (`cargo.cpp:257`).
pub fn cargo_gold_consumption_rate(unit_type: UnitType) -> i32 {
    match unit_type {
        UnitType::GoldRefinery => 5,
        _ => 0,
    }
}

/// `Cargo_ApplyUnitConsumption` (`cargo.cpp:132`).
fn cargo_apply_unit_consumption(
    unit_type: UnitType,
    build_speed_multiplier: i32,
    cargo: &mut NetProduction,
) {
    cargo.raw -= cargo_raw_consumption_rate(unit_type, build_speed_multiplier);
    cargo.fuel -= cargo_fuel_consumption_rate(unit_type);
    cargo.power -= cargo_power_consumption_rate(unit_type);
    cargo.life -= cargo_life_consumption_rate(unit_type);
    cargo.gold -= cargo_gold_consumption_rate(unit_type);
}

/// `UnitInfo::GetMaxAllowedBuildRate()` (`unitinfo.cpp:3208`).
///
/// For mobile ground units — `build_rate`; for buildings C++ takes
/// `min(build_rate, BuildMenu_GetMaxPossibleBuildRate(...))`. The latter depends
/// on `BuildMenu` data (available materials/time) and is not
/// ported in max_core — `build_rate` is used (see "Intentional divergences").
fn max_allowed_build_rate(unit: &UnitInfo) -> i32 {
    i32::from(unit.build_rate)
}

/// `Cargo_GetNetProduction` (`cargo.cpp:70`).
pub(crate) fn cargo_get_net_production(
    state: &GameState,
    unit: &UnitInfo,
    current_order: bool,
) -> NetProduction {
    let mut cargo = NetProduction::default();

    let mut orders = unit.orders;
    if unit.state == UnitOrderState::Init && !current_order {
        orders = unit.prior_orders;
    }

    match unit.unit_type {
        UnitType::Shipyard
        | UnitType::LightVehiclePlant
        | UnitType::HeavyVehiclePlant
        | UnitType::TrainingHall
        | UnitType::AirUnitsPlant => {
            if orders == UnitOrder::Build
                && unit.state != UnitOrderState::UnitReady
                && !unit.build_paused
            {
                cargo_apply_unit_consumption(
                    unit.unit_type,
                    max_allowed_build_rate(unit),
                    &mut cargo,
                );
            }
        }
        UnitType::GoldRefinery
        | UnitType::PowerStation
        | UnitType::PowerGenerator
        | UnitType::Habitat
        | UnitType::ResearchCentre
        | UnitType::EcoSphere => {
            if orders == UnitOrder::PowerOn {
                cargo_apply_unit_consumption(unit.unit_type, 1, &mut cargo);
            }
        }
        UnitType::MiningStation => {
            if orders == UnitOrder::PowerOn || orders == UnitOrder::NewAllocate {
                let mut difficulty_factor = 4;

                if state
                    .teams
                    .get(unit.team.0 as usize)
                    .is_some_and(|info| info.team_type == TeamKind::Computer)
                {
                    let opponent = state.ai.settings.opponent;
                    if opponent == Opponent::Master as u8 {
                        difficulty_factor = 5;
                    } else if opponent == Opponent::God as u8 {
                        difficulty_factor = 6;
                    }
                }

                cargo.gold += (i32::from(unit.gold_mining) * difficulty_factor) / 4;
                cargo.raw += (i32::from(unit.raw_mining) * difficulty_factor) / 4;
                cargo.fuel += (i32::from(unit.fuel_mining) * difficulty_factor) / 4;
                cargo.free_capacity += 16 - i32::from(unit.total_mining);

                cargo_apply_unit_consumption(unit.unit_type, 1, &mut cargo);
            }
        }
        _ => {}
    }

    cargo
}

/// `Cargo_GetNetProduction` for recomputing the complex stock
/// (`Access_UpdateResourcesTotal`) with one port caveat.
///
/// C++ charges the turn of a factory under construction **here** — inside
/// `Cargo_GetNetProduction` (`cargo.cpp:88-95`), and `Access_UpdateResourcesTotal`
/// (`access.cpp:912`) puts this charge into `Complex::material`. The port, per
/// the QA requirement, does not allow negative `storage`/`complex.raw`,
/// so the actual charging with a pause is done by
/// [`crate::production::ProductionManager`] in the
/// `GameManager_ProgressBuildState` phase (like `StartBuilding`/`ValidateIndustry`).
/// So that the rate is not charged twice, **only
/// `raw`** of the factory under construction is returned from the complex cache; `power`/`life`/`fuel`/`gold` remain
/// C++-exact and are checked by [`crate::power::optimize_complex`] (see the
/// `production` header). A pause (`build_paused`) excludes the consumption entirely.
pub(crate) fn cargo_get_net_production_for_total(
    state: &GameState,
    unit: &UnitInfo,
    current_order: bool,
) -> NetProduction {
    let mut cargo = cargo_get_net_production(state, unit, current_order);

    let mut orders = unit.orders;
    if unit.state == UnitOrderState::Init && !current_order {
        orders = unit.prior_orders;
    }

    if matches!(
        unit.unit_type,
        UnitType::Shipyard
            | UnitType::LightVehiclePlant
            | UnitType::HeavyVehiclePlant
            | UnitType::TrainingHall
            | UnitType::AirUnitsPlant
    ) && orders == UnitOrder::Build
        && unit.state != UnitOrderState::UnitReady
        && !unit.build_paused
    {
        cargo.raw += cargo_raw_consumption_rate(unit.unit_type, max_allowed_build_rate(unit));
    }

    cargo
}

// ---------------------------------------------------------------------------
// Unit/base worth — unitinfo.cpp:3719, winloss.cpp
// ---------------------------------------------------------------------------

/// `UnitInfo::GetNormalRateBuildCost()` (`unitinfo.cpp:3719`): the construction
/// cost at 1× rate in raw material units.
pub fn get_normal_rate_build_cost(unit: &UnitInfo) -> i32 {
    let turns = unit.base_values.get_attribute(ATTRIB_TURNS);

    if !unit.is_mobile() {
        cargo_raw_consumption_rate(UnitType::Constructor, 1) * turns
    } else if unit.unit_type == UnitType::Infiltrator || unit.unit_type == UnitType::Infantry {
        cargo_raw_consumption_rate(UnitType::TrainingHall, 1) * turns
    } else {
        cargo_raw_consumption_rate(UnitType::HeavyVehiclePlant, 1) * turns
    }
}

/// `WinLoss_GetWorthOfAssets` (`winloss.cpp:33`) — the total worth of the team's
/// units in one list.
pub(crate) fn get_worth_of_assets(state: &GameState, team: TeamId, kind: UnitListKind) -> i32 {
    let mut sum = 0i32;

    for &id in state.unit_list(kind) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team {
            sum += get_normal_rate_build_cost(unit);
        }
    }

    sum
}

/// `WinLoss_GetTotalWorthOfAssets` (`winloss.cpp:47`): mobile ground/naval
/// + air + stationary (ground cover is not included).
pub(crate) fn get_total_worth_of_assets(state: &GameState, team: TeamId) -> i32 {
    get_worth_of_assets(state, team, UnitListKind::MobileLandSea)
        + get_worth_of_assets(state, team, UnitListKind::MobileAir)
        + get_worth_of_assets(state, team, UnitListKind::Stationary)
}

// ---------------------------------------------------------------------------
// Mission categories
// ---------------------------------------------------------------------------

// `MissionCategory` is a pure lower-layer enum (`max_types`, F5.2);
// re-exported here so that the path `max_core::victory::MissionCategory` does not change.
pub use max_types::victory::MissionCategory;

// ---------------------------------------------------------------------------
// WinLoss_Status and generic rules
// ---------------------------------------------------------------------------

/// `WinLoss_Status` (`winloss.hpp:29`) — the victory/defeat evaluation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinLossStatus {
    /// `mission_type` — mission category ([`MissionCategory`] as `i32`).
    pub mission_type: i32,
    /// `team_type[PLAYER_TEAM_MAX]` — team types as `i32`.
    pub team_type: [i32; TEAM_SLOTS],
    /// `team_status[PLAYER_TEAM_MAX]`.
    pub team_status: [WinLossState; TEAM_SLOTS],
    /// `team_rank[PLAYER_TEAM_MAX]` — 1..N among playing teams.
    pub team_rank: [i32; TEAM_SLOTS],
    /// `teams_total` — the number of teams with `team_type != NONE`.
    pub teams_total: i32,
    /// `teams_in_play`.
    pub teams_in_play: i32,
    /// `teams_humans_in_play`.
    pub teams_humans_in_play: i32,
    /// `teams_non_computers`.
    pub teams_non_computers: i32,
}

/// Mission script base: `if alive then <alive_state> else LOST`.
fn survival_status(state: &GameState, team: TeamId, alive_state: WinLossState) -> WinLossState {
    let alive = has_attack_power(state, team, UnitClass::MobileLandSea)
        || has_attack_power(state, team, UnitClass::MobileAir)
        || has_attack_power(state, team, UnitClass::Stationary)
        || has_production_infrastructure(state, team)
        || has_construction_infrastructure(state, team);

    if alive {
        alive_state
    } else {
        WinLossState::Lost
    }
}

/// The default script — `SC_L0002` (`LoadDefaultWinLossConditions`):
///
/// ```lua
/// if max_has_attack_power(a,MOBILE_LAND_SEA) or max_has_attack_power(a,MOBILE_AIR)
///    or max_has_attack_power(a,STATIONARY) or max_has_production_infrastructure(a)
///    or max_has_construction_infrastructure(a)
/// then b=GENERIC else b=LOST end; return b
/// ```
///
/// `GENERIC` means: the generic rules apply from now on
/// ([`check_end_game_conditions`]); `LOST` — the team is out of the game.
pub(crate) fn default_win_loss_conditions(state: &GameState, team: TeamId) -> WinLossState {
    survival_status(state, team, WinLossState::Generic)
}

/// `MAX_REGISTRY.TeamTurnValue` — `GameManager_TurnCounter`
/// (`maxregistryhandler.cpp:66`).
fn team_turn_value(state: &GameState) -> i64 {
    i64::from(state.turn_counter)
}

/// `MAX_REGISTRY.TeamTurnVictoryLimit` (`maxregistryhandler.cpp:59`):
/// `victory_limit` for `VICTORY_TYPE_DURATION`, otherwise `-1`.
fn team_turn_victory_limit(state: &GameState) -> i64 {
    if state.victory_type == VictoryType::Duration {
        i64::from(state.victory_limit)
    } else {
        -1
    }
}

/// Team status by mission rules (`WinLossHandler::TestWinLossConditions`).
///
/// `None` — an unknown mission (in C++ `SDL_assert(0)` in
/// `WinLoss_EvaluateStatus`); the caller substitutes `GENERIC`, as
/// `winloss.cpp:77`.
///
/// Each branch is a verbatim translation of a Lua script from `assets/PATCHES.RES`
/// (the resource code is given in the comment).
pub(crate) fn mission_win_loss(
    state: &GameState,
    category: MissionCategory,
    mission_index: u16,
    team: TeamId,
) -> Option<WinLossState> {
    use WinLossState::{Generic, Lost, Pending, Won};

    let red = team == TeamId::RED;
    // Common script prologue: `b` — "team is alive" (PENDING or GENERIC).
    let base = |alive_state| survival_status(state, team, alive_state);

    let status = match category {
        // --- Training missions (SC_T0001..SC_T0015, SAVE1..15.TRA) ---
        MissionCategory::Training => match mission_index {
            // SC_T0001
            1 => {
                let a;
                if has_materials(state, TeamId::RED, CargoType::Raw) < 1
                    || has_materials(state, TeamId::RED, CargoType::Fuel) < 1
                    || count_ready_units(state, TeamId::RED, UnitType::MaterialStorage) <= 0
                {
                    if count_ready_units(state, TeamId::RED, UnitType::MiningStation) > 0
                        && count_ready_units(state, TeamId::RED, UnitType::Engineer) > 0
                    {
                        a = Pending;
                    } else {
                        a = Lost;
                    }
                } else {
                    a = Won;
                }
                a
            }
            // SC_T0002
            2 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, team, UnitType::LightVehiclePlant) > 0
                            && count_ready_units(state, team, UnitType::GunTurret) > 0
                        {
                            b = Won;
                        } else if (count_ready_units(state, team, UnitType::Engineer) > 0
                            || count_ready_units(state, team, UnitType::GunTurret) > 0)
                            && (count_ready_units(state, team, UnitType::Constructor) > 0
                                || count_ready_units(state, team, UnitType::LightVehiclePlant) > 0)
                        {
                            b = Pending;
                        } else {
                            b = Lost;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0003
            3 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, team, UnitType::HeavyVehiclePlant) > 0
                            && count_ready_units(state, team, UnitType::Scout) > 2
                        {
                            b = Won;
                        } else if (count_ready_units(state, team, UnitType::LightVehiclePlant) > 0
                            || count_ready_units(state, team, UnitType::Scout) > 2)
                            && count_ready_units(state, team, UnitType::MiningStation) > 0
                            && (count_ready_units(state, team, UnitType::Constructor) > 0
                                || count_ready_units(state, team, UnitType::HeavyVehiclePlant) > 0)
                        {
                            b = Pending;
                        } else {
                            b = Lost;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0004
            4 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        let (raw_mining_max, _fuel, _gold) = count_total_mining(state, team);
                        if raw_mining_max < 22 {
                            if count_ready_units(state, team, UnitType::Constructor) > 0
                                && count_ready_units(state, team, UnitType::MiningStation) > 0
                            {
                                b = Pending;
                            } else {
                                b = Lost;
                            }
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0005
            5 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::MiningStation) > 0 {
                            b = Pending;
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0006
            6 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if get_total_units_being_constructed(state, team, UnitType::GunTurret) > 0 {
                            b = Won;
                        } else {
                            b = Pending;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0007
            7 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if get_total_power_consumption(state, team, UnitType::LightVehiclePlant)
                            + get_total_power_consumption(state, team, UnitType::HeavyVehiclePlant)
                            > 1
                            && get_total_mining(state, team, CargoType::Fuel)
                                - get_total_consumption(state, team, CargoType::Fuel)
                                > 0
                        {
                            b = Won;
                        } else if count_ready_units(state, team, UnitType::LightVehiclePlant)
                            + count_ready_units(state, team, UnitType::HeavyVehiclePlant)
                            > 1
                            && count_ready_units(state, team, UnitType::MiningStation) > 0
                        {
                            b = Pending;
                        } else {
                            b = Lost;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0008
            8 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if has_materials(state, team, CargoType::Gold) > 0 {
                            b = Won;
                        } else {
                            b = Pending;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0009
            9 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if has_unit_above_mark_level(state, team, UnitType::Tank, 1) {
                            b = Won;
                        } else {
                            b = Pending;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0010
            10 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if state.teams[team.0 as usize].team_points > 0 {
                            b = Won;
                        } else {
                            b = Pending;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0011
            11 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if has_unit_experience(state, team, UnitType::Infiltrator) {
                            b = Won;
                        } else if count_ready_units(state, TeamId::RED, UnitType::TrainingHall) > 0
                            && count_ready_units(state, TeamId::RED, UnitType::Habitat) > 0
                            && count_ready_units(state, TeamId::RED, UnitType::MiningStation) > 0
                        {
                            b = Pending;
                        } else {
                            b = Lost;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0012
            12 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::RED, UnitType::LandMine) > 0
                            && count_casualties(state, TeamId::GREEN, UnitType::LandMine) > 0
                        {
                            b = Won;
                        } else {
                            b = Pending;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0013
            13 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::RED, UnitType::Tank) < 3
                            || count_damaged_units(state, TeamId::RED, UnitType::Tank) > 0
                        {
                            if count_casualties(state, TeamId::RED, UnitType::Tank) > 0 {
                                b = Lost;
                            } else {
                                b = Pending;
                            }
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0014
            14 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::RED, UnitType::Tank) < 3
                            || count_units_with_reduced_attrib(
                                state,
                                TeamId::RED,
                                UnitType::Tank,
                                ATTRIB_AMMO,
                            ) > 0
                        {
                            if count_casualties(state, TeamId::RED, UnitType::Tank) > 0 {
                                b = Lost;
                            } else {
                                b = Pending;
                            }
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_T0015
            15 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if get_research_level(state, TeamId::RED, ResearchTopic::Armor) > 0 {
                            b = Won;
                        } else if count_ready_units(state, TeamId::RED, UnitType::ResearchCentre)
                            > 0
                        {
                            b = Pending;
                        } else {
                            b = Lost;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            _ => default_win_loss_conditions(state, team),
        },

        // --- Single player scenarios (SC_S0001..SC_S0024, SAVE1..24.SCE) ---
        MissionCategory::Scenario => match mission_index {
            // SC_S0009: a scenario comparing team points.
            9 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if i64::from(state.teams[TeamId::RED.0 as usize].team_points)
                            > i64::from(state.teams[TeamId::GREEN.0 as usize].team_points)
                        {
                            b = Won;
                        } else {
                            b = Pending;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_S0018: green has 0 research centers → red wins.
            18 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::ResearchCentre) > 0 {
                            b = Pending;
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_S0024: green has 0 greenhouses → red wins.
            24 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::EcoSphere) > 0 {
                            b = Pending;
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            _ => default_win_loss_conditions(state, team),
        },

        // --- Campaign (SC_C0001..SC_C0009, SAVE1..9.CAM) ---
        MissionCategory::Campaign => match mission_index {
            // SC_C0001: green lost its research center → red loses;
            // 0 greenhouses at green → victory.
            1 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::ResearchCentre) == 0 {
                            b = Lost;
                        } else if count_ready_units(state, TeamId::GREEN, UnitType::EcoSphere) > 0 {
                            b = Pending;
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_C0002: losing red's research center → defeat; the turn
            // limit → victory.
            2 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_casualties(state, TeamId::RED, UnitType::ResearchCentre) > 0 {
                            b = Lost;
                        } else if team_turn_value(state) < team_turn_victory_limit(state) {
                            b = Pending;
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_C0003: supplies and the saboteur.
            3 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::RED, UnitType::SupplyTruck)
                            + count_ready_units(state, TeamId::GREEN, UnitType::SupplyTruck)
                            < 2
                        {
                            b = Lost;
                        } else if count_ready_units(state, TeamId::RED, UnitType::SupplyTruck) > 1 {
                            b = Won;
                        } else if count_ready_units(state, TeamId::RED, UnitType::Infiltrator) > 0 {
                            b = Pending;
                        } else {
                            b = Lost;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_C0004 / SC_C0005 / SC_C0009: 0 greenhouses at green → victory.
            4 | 5 | 9 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::EcoSphere) > 0 {
                            b = Pending;
                        } else {
                            b = Won;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_C0008: alien units, turn limit.
            8 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        let alien = |team: TeamId| {
                            count_ready_units(state, team, UnitType::AlienPlane)
                                + count_ready_units(state, team, UnitType::AlienTank)
                                + count_ready_units(state, team, UnitType::AlienAssaultGun)
                                + count_ready_units(state, team, UnitType::Juggernaut)
                        };
                        let c = alien(TeamId::RED);
                        let d = alien(TeamId::GREEN);

                        if c + d < 4 {
                            b = Lost;
                        } else if c > 3 {
                            b = Generic;
                        } else if team_turn_value(state) >= team_turn_victory_limit(state) {
                            b = Lost;
                        } else {
                            b = Pending;
                        }
                    }
                } else if b != Lost {
                    b = Generic;
                }
                b
            }
            // SC_C0006 / SC_C0007 and others — the generic script.
            _ => default_win_loss_conditions(state, team),
        },

        // --- Custom / Multiplayer / Hot-seat / Demo / MPS — SC_L0002 ---
        _ => default_win_loss_conditions(state, team),
    };

    Some(status)
}

/// `WinLoss_EvaluateStatus` (`winloss.cpp:53`) — a verbatim port.
///
/// `turn_counter` is kept for C++ signature compatibility; the predicates read
/// [`GameState::turn_counter`] (like `MAX_REGISTRY.TeamTurnValue`).
pub(crate) fn evaluate_status(state: &GameState, turn_counter: i32) -> WinLossStatus {
    let _ = turn_counter;

    let mut status = WinLossStatus {
        mission_type: state.mission_category as i32,
        team_type: [0; TEAM_SLOTS],
        team_status: [WinLossState::Generic; TEAM_SLOTS],
        team_rank: [0; TEAM_SLOTS],
        teams_total: 0,
        teams_in_play: 0,
        teams_humans_in_play: 0,
        teams_non_computers: 0,
    };
    let mut teams_in_play = [0u16; TEAM_SLOTS];

    for team in 0..TEAM_SLOTS - 1 {
        status.team_type[team] = state.teams[team].team_type as i32;
    }

    for team in 0..TEAM_SLOTS {
        let team_id = TeamId(team as u8);
        let kind = state.teams[team].team_type;

        // C++: `PLAYER_TEAM_ALIEN` has `team_type = TEAM_TYPE_NONE` (`ResourceManager_InitTeamInfo`),
        // so the service derelict team does not enter `teams_total`/
        // `teams_in_play` and does not participate in victory conditions. In max_core the
        // `ALIEN` slot is marked `Computer`, so we skip it explicitly.
        if team == TeamId::ALIEN.0 as usize || kind == TeamKind::None {
            continue;
        }

        status.team_status[team] =
            mission_win_loss(state, state.mission_category, state.mission_index, team_id)
                .unwrap_or(WinLossState::Generic);

        status.teams_total += 1;

        if kind != TeamKind::Computer {
            status.teams_non_computers += 1;
        }

        if status.team_status[team] != WinLossState::Lost && kind != TeamKind::Eliminated {
            if kind != TeamKind::Computer {
                status.teams_humans_in_play += 1;
            }

            teams_in_play[status.teams_in_play as usize] = team as u16;
            status.teams_in_play += 1;
        }
    }

    // C++ bubble sort (`winloss.cpp:98`): by descending points, and on
    // equality by "base worth" (higher worth — higher).
    for i in 0..(status.teams_in_play as usize).saturating_sub(1) {
        for j in (i + 1)..(status.teams_in_play as usize) {
            let left = teams_in_play[i] as usize;
            let right = teams_in_play[j] as usize;

            if state.teams[left].team_points < state.teams[right].team_points {
                teams_in_play.swap(i, j);
            } else if state.teams[right].team_points == state.teams[left].team_points {
                let difference = get_total_worth_of_assets(state, TeamId(left as u8))
                    - get_total_worth_of_assets(state, TeamId(right as u8));

                if difference < 0 {
                    teams_in_play.swap(i, j);
                }
            }
        }
    }

    for i in 0..status.teams_in_play as usize {
        status.team_rank[teams_in_play[i] as usize] = i as i32 + 1;
    }

    status
}

/// `menu_check_end_game_conditions` (`menu.cpp:548`) for the generic
/// game rules (without UI).
///
/// End-of-game conditions (checked for each team, as in C++):
/// 1. the local player's status is `WON`/`LOST`;
/// 2. one player remains (`teams_in_play == 1`, `teams_total > 1`) and it is not
///    `PENDING`;
/// 3. no players remain (`teams_in_play == 0`);
/// 4. all humans are eliminated, but not all were computers;
/// 5. `VICTORY_TYPE_DURATION` and the turn limit is reached;
/// 6. `VICTORY_TYPE_SCORE` and the team points ≥ the limit.
///
/// Additionally (non-network branch, `menu.cpp:593-609`): if the
/// local player's status is `GENERIC` and a human team lost, it is
/// marked `TeamKind::Eliminated`.
///
/// Returns `true` if the game is over (`menu_wrap_up_game`).
pub(crate) fn check_end_game_conditions(state: &mut GameState) -> bool {
    let status = evaluate_status(state, state.turn_counter);
    let player_team = state.player_team.0 as usize;

    for team in 0..TEAM_SLOTS {
        let kind = state.teams[team].team_type;

        if team == TeamId::ALIEN.0 as usize || kind == TeamKind::None {
            continue;
        }

        let player_status = status.team_status[player_team];
        let turn_limit_reached = state.victory_type == VictoryType::Duration
            && state.victory_limit <= state.turn_counter;
        let score_limit_reached = state.victory_type == VictoryType::Score
            && kind != TeamKind::None
            && state.teams[team].team_points as i64 >= i64::from(state.victory_limit);

        let end = player_status == WinLossState::Won
            || player_status == WinLossState::Lost
            || (player_status != WinLossState::Pending
                && status.teams_in_play == 1
                && status.teams_total > 1)
            || status.teams_in_play == 0
            || (status.teams_humans_in_play == 0 && status.teams_non_computers > 0)
            || turn_limit_reached
            || score_limit_reached;

        if end {
            return true;
        }

        if player_status == WinLossState::Generic
            && kind == TeamKind::Player
            && status.team_status[team] == WinLossState::Lost
        {
            // A network game in C++ returns true; the local one marks the team
            // eliminated and continues. max_core is lockstep, so we take
            // the non-network branch.
            if state.network_game {
                return true;
            }

            state.teams[team].team_type = TeamKind::Eliminated;
        }
    }

    false
}

/// Number of teams without `ALIEN` — a helper constant for tests.
pub const PLAYING_TEAMS: usize = PLAYER_TEAM_COUNT;

#[cfg(test)]
#[path = "victory_tests.rs"]
mod tests;
