//! Победа/поражение — `winloss.cpp`, `winlosshandler.cpp`, `scripter.cpp` (P7).
//!
//! # Оракул
//!
//! | Что | Где |
//! |---|---|
//! | `WinLoss_GetWorthOfAssets`, `WinLoss_GetTotalWorthOfAssets`, `WinLoss_EvaluateStatus` | `max-cpp-port/src/winloss.cpp` |
//! | общий тест условий (`TestWinLossConditions` → Lua) | `winlosshandler.cpp` |
//! | примитивы-предикаты `max_*` | `scripter.cpp:159-1149` |
//! | `Cargo_*` (расход/добыча/производство) | `cargo.cpp:30-271` |
//! | `GetNormalRateBuildCost` | `unitinfo.cpp:3719` |
//! | значения `MAX_REGISTRY.*` | `maxregistryhandler.cpp:33-163` |
//! | условия победы/поражения миссий (Lua) | `assets/PATCHES.RES` → `SC_L0002`, `SC_T0001..15`, `SC_S0001..24`, `SC_C0001..09` |
//! | generic end-game (`menu_check_end_game_conditions`) | `menu.cpp:548` |
//! | описание правил | `max-port-site/save.md` → *Victory and Loss Conditions* |
//!
//! # Модель
//!
//! 1. [`predicates`] — чистые порты Lua-API (`max_*`): счётчики юнитов,
//!    потери, ресурсы, очки, исследования, ходы, добыча/потребление, состояние
//!    юнитов и три «инфраструктурных» предиката (военная мощь, производство,
//!    строительство).
//! 2. [`mission_win_loss`] — статус команды по правилам миссии. Скрипты миссий
//!    из ресурсов порта переписаны 1:1 (в комментариях к каждой ветке — исходный
//!    Lua), поэтому это **данные-правила**, а не интерпретатор Lua (DESIGN §7,
//!    риск 8). `mission_index == 0` и категории без скрипта дают
//!    [`default_win_loss_conditions`] (`SC_L0002`).
//! 3. [`evaluate_status`] — дословный порт `WinLoss_EvaluateStatus`: статусы,
//!    счётчики команд, сортировка `teams_in_play` (очки, затем «стоимость базы»
//!    [`get_total_worth_of_assets`]) и ранги.
//! 4. [`check_end_game_conditions`] — порт `menu_check_end_game_conditions`
//!    (generic-правила SCORE/DURATION, «последний выживший», «все люди
//!    выбиты»), вызывается из [`crate::game::GameState::finish_turn`].
//!
//! # Осознанные расхождения
//!
//! * `WinLoss_EvaluateStatus(turn_counter)` в C++ **не использует** аргумент
//!   (все предикаты читают `GameManager_TurnCounter`); в Rust так же — параметр
//!   сохранён для соответствия сигнатуре.
//! * `Cargo_GetNetProduction` для фабрик в `ORDER_BUILD` использует
//!   `GetMaxAllowedBuildRate()`; в max_core это `build_rate` без клампа
//!   `BuildMenu_GetMaxPossibleBuildRate` (нужны данные `BuildMenu`, P3) — на
//!   предикаты миссий это не влияет (см. `tests`).
//! * `menu_check_end_game_conditions` в C++ рисует экран итогов; здесь возвращается
//!   только факт конца партии и (для не-сетевой ветки) команда помечается
//!   `TeamKind::Eliminated` — как `menu.cpp:605`.

use crate::enums::{
    unit_flags, FileType, Opponent, ResearchTopic, UnitOrder, UnitOrderState, UnitType,
    VictoryType, WinLossState,
};
use crate::game::{GameState, PLAYER_TEAM_COUNT, TEAM_SLOTS};
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitInfo, UnitListKind};
use crate::unit_values::{
    ATTRIB_AMMO, ATTRIB_HITS, ATTRIB_ROUNDS, ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS,
};

// ---------------------------------------------------------------------------
// Тип груза юнита — Unit::CargoType (unit.hpp), данные SC_UNITS
// ---------------------------------------------------------------------------

/// `Unit::CargoType` (`unit.hpp`), значения — как в C++.
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
    /// `CARGO_TYPE_LAND` (транспорт).
    Land = 4,
    /// `CARGO_TYPE_SEA` (транспорт).
    Sea = 5,
    /// `CARGO_TYPE_AIR` (транспорт).
    Air = 6,
}

/// `ResourceManager_GetUnit(type).GetCargoType()`.
///
/// Таблица — из `assets/PATCHES.RES` → `SC_UNITS` (JSON-описание `units.cpp`),
/// сгруппирована по типам груза. В C++ поле читается из ресурса при загрузке
/// данных юнитов; здесь — const-данные (DESIGN §1.4 п.4).
pub fn cargo_type_of(unit_type: UnitType) -> CargoType {
    match unit_type {
        UnitType::Barracks
        | UnitType::Depot
        | UnitType::PersonnelCarrier
        | UnitType::SeaTransport
        | UnitType::AirTransport => CargoType::Land,
        UnitType::ADump
        | UnitType::MiningStation
        | UnitType::Constructor
        | UnitType::MineLayer
        | UnitType::SupplyTruck
        | UnitType::Engineer
        | UnitType::Bulldozer
        | UnitType::RepairUnit
        | UnitType::SeaMineLayer
        | UnitType::CargoShip => CargoType::Raw,
        UnitType::FDump | UnitType::FuelTruck => CargoType::Fuel,
        UnitType::GoldSmelter | UnitType::GoldTruck => CargoType::Gold,
        UnitType::Hangar | UnitType::LandingPad => CargoType::Air,
        UnitType::Dock => CargoType::Sea,
        _ => CargoType::None,
    }
}

/// `GetRelevantUnits(unit_type)` (`scripter.cpp:140`): список юнитов, в котором
/// предикаты ищут юнитов данного типа.
///
/// Порядок проверок — как в C++ (`STATIONARY` первым, затем `MOBILE_AIR`, иначе
/// `MOBILE_LAND_SEA`). Поэтому `GROUND_COVER`-типы без `STATIONARY` (например
/// `TORPEDO`) попадают в `MobileLandSea`, а не в `GroundCover`.
pub fn relevant_list(unit_type: UnitType) -> UnitListKind {
    let flags = crate::access::unit_type_flags(unit_type);

    if flags & unit_flags::STATIONARY != 0 {
        if flags & unit_flags::GROUND_COVER != 0 {
            UnitListKind::GroundCover
        } else {
            UnitListKind::Stationary
        }
    } else if flags & unit_flags::MOBILE_AIR_UNIT != 0 {
        UnitListKind::MobileAir
    } else {
        UnitListKind::MobileLandSea
    }
}

/// `MAX_UNIT_CLASS` в Lua-предикатах `max_has_attack_power`.
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
// Примитивы-предикаты (scripter.cpp → Lua API)
// ---------------------------------------------------------------------------

/// `HasMaterials()` (`scripter.cpp:159`): сумма `storage` юнитов команды с
/// заданным типом груза по спискам `Stationary` и `MobileLandSea`.
pub fn has_materials(state: &GameState, team: TeamId, cargo: CargoType) -> i32 {
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

/// `CountReadyUnits()` (`scripter.cpp:204`): юниты типа, **готовые** (состояние
/// не `ORDER_STATE_BUILDING_READY`).
pub fn count_ready_units(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
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

/// `HasAttackPower()` (`scripter.cpp:244`): есть юнит класса с `ammo > 0`,
/// не `SUBMARNE` и не `COMMANDO`.
pub fn has_attack_power(state: &GameState, team: TeamId, class: UnitClass) -> bool {
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
            && unit.unit_type != UnitType::Commando
        {
            return true;
        }
    }

    false
}

/// `HasProductionInfrastructure()` (`scripter.cpp:296`): фабрика + топливо ≥ 8 и
/// материалы ≥ 24, иначе шахта; покрывает потребность в энергии (`power_demand`
/// 1 или 2).
pub fn has_production_infrastructure(state: &GameState, team: TeamId) -> bool {
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

/// `HasConstructionInfrastructure()` (`scripter.cpp:372`): конструктор +
/// инженер + достаточно материалов (38, 8 при наличии шахты, −8 при своей
/// генерации).
pub fn has_construction_infrastructure(state: &GameState, team: TeamId) -> bool {
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

/// `CountTotalMining()` (`scripter.cpp:454`): суммарные **максимумы** добычи
/// включённых шахт (`ORDER_POWER_ON`) — `(raw, fuel, gold)`.
pub fn count_total_mining(state: &GameState, team: TeamId) -> (i32, i32, i32) {
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
/// Забавная деталь C++: если **строящийся** тип стационарный, перебирается
/// список мобильных наземных/морских юнитов, и наоборот (список содержит
/// «производителей»).
pub fn get_total_units_being_constructed(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
) -> i32 {
    let kind = if crate::access::unit_type_flags(unit_type) & unit_flags::STATIONARY != 0 {
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

/// `GetTotalPowerConsumption()` (`scripter.cpp:547`) — число стационарных юнитов
/// типа в состоянии `POWER_ON`/`BUILD`.
pub fn get_total_power_consumption(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
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

/// `GetTotalMining()` (`scripter.cpp:588`) — фактическая добыча включённых шахт.
pub fn get_total_mining(state: &GameState, team: TeamId, cargo: CargoType) -> i32 {
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

/// `GetTotalConsumption()` (`scripter.cpp:638`) — суммарное потребление
/// (отрицательные компоненты `Cargo_GetNetProduction`).
pub fn get_total_consumption(state: &GameState, team: TeamId, cargo: CargoType) -> i32 {
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

/// `HasUnitAboveMarkLevel()` (`scripter.cpp:699`) — версия юнита (mark level)
/// выше заданной; проверяется **только** список `MobileLandSea` (как в C++).
pub fn has_unit_above_mark_level(
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

/// `HasUnitExperience()` (`scripter.cpp:741`): есть юнит типа с `storage > 0`
/// (у `COMMANDO`/`REGENERATING_UNIT` `storage` — это опыт).
pub fn has_unit_experience(state: &GameState, team: TeamId, unit_type: UnitType) -> bool {
    for &id in state.unit_list(relevant_list(unit_type)) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team && unit.unit_type == unit_type && unit.storage > 0 {
            return true;
        }
    }

    false
}

/// `CountCasualties()` (`scripter.cpp:780`).
pub fn count_casualties(state: &GameState, team: TeamId, unit_type: UnitType) -> u32 {
    state
        .teams
        .get(team.0 as usize)
        .map(|info| info.casualties_for(unit_type as u16))
        .unwrap_or(0)
}

/// `CountDamangedUnits()` (`scripter.cpp:809`) — юниты с `hits != base hits`.
pub fn count_damaged_units(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
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
pub fn count_units_above_attrib_level(
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

/// `CountUnitsWithReducedAttrib()` (`scripter.cpp:900`): атрибут юнита ниже
/// базового (`ammo`, `speed`, `shots`/`rounds`, `hits`, `storage`).
pub fn count_units_with_reduced_attrib(
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
pub fn get_research_level(state: &GameState, team: TeamId, topic: ResearchTopic) -> u32 {
    state
        .teams
        .get(team.0 as usize)
        .map(|info| info.research_topics[topic as usize].research_level)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Cargo_* — cargo.cpp
// ---------------------------------------------------------------------------

/// `Cargo` в объёме `Cargo_GetNetProduction` (C++ `Cargo` — cargo.hpp, поля
/// `int16_t`; здесь i32 до сужения, как в формулах C++).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetProduction {
    /// Золото.
    pub gold: i32,
    /// Материалы.
    pub raw: i32,
    /// Топливо.
    pub fuel: i32,
    /// Энергия.
    pub power: i32,
    /// Жизнеобеспечение.
    pub life: i32,
    /// Свободная вместимость комплекса.
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
        | UnitType::Greenhouse
        | UnitType::ResearchCenter
        | UnitType::CommTower
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
        UnitType::TrainingHall | UnitType::Greenhouse | UnitType::ResearchCenter => 1,
        _ => 0,
    }
}

/// `Cargo_GetGoldConsumptionRate` (`cargo.cpp:257`).
pub fn cargo_gold_consumption_rate(unit_type: UnitType) -> i32 {
    match unit_type {
        UnitType::CommTower => 5,
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
/// Для мобильных наземных — `build_rate`; для зданий C++ берёт
/// `min(build_rate, BuildMenu_GetMaxPossibleBuildRate(...))`. Последняя зависит
/// от данных `BuildMenu` (доступных материалов/времени) и в max_core не
/// портирована — используется `build_rate` (см. «Осознанные расхождения»).
fn max_allowed_build_rate(unit: &UnitInfo) -> i32 {
    i32::from(unit.build_rate)
}

/// `Cargo_GetNetProduction` (`cargo.cpp:70`).
pub fn cargo_get_net_production(
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
            if orders == UnitOrder::Build && unit.state != UnitOrderState::UnitReady {
                cargo_apply_unit_consumption(
                    unit.unit_type,
                    max_allowed_build_rate(unit),
                    &mut cargo,
                );
            }
        }
        UnitType::CommTower
        | UnitType::PowerStation
        | UnitType::PowerGenerator
        | UnitType::Habitat
        | UnitType::ResearchCenter
        | UnitType::Greenhouse => {
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

// ---------------------------------------------------------------------------
// Стоимость юнита/базы — unitinfo.cpp:3719, winloss.cpp
// ---------------------------------------------------------------------------

/// `UnitInfo::GetNormalRateBuildCost()` (`unitinfo.cpp:3719`): стоимость
/// постройки при скорости 1× в единицах сырья.
pub fn get_normal_rate_build_cost(unit: &UnitInfo) -> i32 {
    let turns = unit.base_values.get_attribute(ATTRIB_TURNS);

    if unit.flags & unit_flags::STATIONARY != 0 {
        cargo_raw_consumption_rate(UnitType::Constructor, 1) * turns
    } else if unit.unit_type == UnitType::Commando || unit.unit_type == UnitType::Infantry {
        cargo_raw_consumption_rate(UnitType::TrainingHall, 1) * turns
    } else {
        cargo_raw_consumption_rate(UnitType::HeavyVehiclePlant, 1) * turns
    }
}

/// `WinLoss_GetWorthOfAssets` (`winloss.cpp:33`) — суммарная стоимость юнитов
/// команды в одном списке.
pub fn get_worth_of_assets(state: &GameState, team: TeamId, kind: UnitListKind) -> i32 {
    let mut sum = 0i32;

    for &id in state.unit_list(kind) {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team {
            sum += get_normal_rate_build_cost(unit);
        }
    }

    sum
}

/// `WinLoss_GetTotalWorthOfAssets` (`winloss.cpp:47`): мобильные наземные/морские
/// + воздушные + стационарные (ground cover не входит).
pub fn get_total_worth_of_assets(state: &GameState, team: TeamId) -> i32 {
    get_worth_of_assets(state, team, UnitListKind::MobileLandSea)
        + get_worth_of_assets(state, team, UnitListKind::MobileAir)
        + get_worth_of_assets(state, team, UnitListKind::Stationary)
}

// ---------------------------------------------------------------------------
// Категории миссий
// ---------------------------------------------------------------------------

/// `MissionCategory` (`mission.hpp:31`) — категория миссии (не путать с
/// `FileType` заголовка сейва; соответствие — [`MissionCategory::from_file_type`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MissionCategory {
    /// `MISSION_CATEGORY_CUSTOM`.
    Custom = 0,
    /// `MISSION_CATEGORY_TRAINING`.
    Training = 1,
    /// `MISSION_CATEGORY_CAMPAIGN`.
    Campaign = 2,
    /// `MISSION_CATEGORY_HOT_SEAT`.
    HotSeat = 3,
    /// `MISSION_CATEGORY_MULTI`.
    Multiplayer = 4,
    /// `MISSION_CATEGORY_DEMO`.
    Demo = 5,
    /// `MISSION_CATEGORY_SCENARIO`.
    Scenario = 6,
    /// `MISSION_CATEGORY_MULTI_PLAYER_SCENARIO`.
    MultiPlayerScenario = 7,
}

impl MissionCategory {
    /// Соответствие `FileType` заголовка сейва категории миссии
    /// (`saveload.cpp` передаёт категорию в `SaveLoad_LoadFormatV70`).
    pub fn from_file_type(file_type: FileType) -> Self {
        match file_type {
            FileType::Custom | FileType::Debug | FileType::Text => Self::Custom,
            FileType::Tutorial => Self::Training,
            FileType::Campaign => Self::Campaign,
            FileType::HotSeat => Self::HotSeat,
            FileType::Multiplayer => Self::Multiplayer,
            FileType::Demo => Self::Demo,
            FileType::Scenario => Self::Scenario,
            FileType::MultiScenario => Self::MultiPlayerScenario,
        }
    }
}

// ---------------------------------------------------------------------------
// WinLoss_Status и generic-правила
// ---------------------------------------------------------------------------

/// `WinLoss_Status` (`winloss.hpp:29`) — результат оценки победы/поражения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinLossStatus {
    /// `mission_type` — категория миссии ([`MissionCategory`] как `i32`).
    pub mission_type: i32,
    /// `team_type[PLAYER_TEAM_MAX]` — типы команд как `i32`.
    pub team_type: [i32; TEAM_SLOTS],
    /// `team_status[PLAYER_TEAM_MAX]`.
    pub team_status: [WinLossState; TEAM_SLOTS],
    /// `team_rank[PLAYER_TEAM_MAX]` — 1..N среди играющих.
    pub team_rank: [i32; TEAM_SLOTS],
    /// `teams_total` — число команд с `team_type != NONE`.
    pub teams_total: i32,
    /// `teams_in_play`.
    pub teams_in_play: i32,
    /// `teams_humans_in_play`.
    pub teams_humans_in_play: i32,
    /// `teams_non_computers`.
    pub teams_non_computers: i32,
}

/// База скрипта миссии: `if alive then <alive_state> else LOST`.
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

/// Скрипт по умолчанию — `SC_L0002` (`LoadDefaultWinLossConditions`):
///
/// ```lua
/// if max_has_attack_power(a,MOBILE_LAND_SEA) or max_has_attack_power(a,MOBILE_AIR)
///    or max_has_attack_power(a,STATIONARY) or max_has_production_infrastructure(a)
///    or max_has_construction_infrastructure(a)
/// then b=GENERIC else b=LOST end; return b
/// ```
///
/// `GENERIC` означает: дальше действуют generic-правила
/// ([`check_end_game_conditions`]); `LOST` — команда выбита из игры.
pub fn default_win_loss_conditions(state: &GameState, team: TeamId) -> WinLossState {
    survival_status(state, team, WinLossState::Generic)
}

/// `MAX_REGISTRY.TeamTurnValue` — `GameManager_TurnCounter`
/// (`maxregistryhandler.cpp:66`).
fn team_turn_value(state: &GameState) -> i64 {
    i64::from(state.turn_counter)
}

/// `MAX_REGISTRY.TeamTurnVictoryLimit` (`maxregistryhandler.cpp:59`):
/// `victory_limit` для `VICTORY_TYPE_DURATION`, иначе `-1`.
fn team_turn_victory_limit(state: &GameState) -> i64 {
    if state.victory_type == VictoryType::Duration {
        i64::from(state.victory_limit)
    } else {
        -1
    }
}

/// Статус команды по правилам миссии (`WinLossHandler::TestWinLossConditions`).
///
/// `None` — неизвестная миссия (в C++ `SDL_assert(0)` в
/// `WinLoss_EvaluateStatus`); вызывающий подставляет `GENERIC`, как это делает
/// `winloss.cpp:77`.
///
/// Каждая ветка — дословный перевод Lua-скрипта из `assets/PATCHES.RES`
/// (код ресурса указан в комментарии).
pub fn mission_win_loss(
    state: &GameState,
    category: MissionCategory,
    mission_index: u16,
    team: TeamId,
) -> Option<WinLossState> {
    use WinLossState::{Generic, Lost, Pending, Won};

    let red = team == TeamId::RED;
    // Общий пролог скриптов: `b` — «команда жива» (PENDING или GENERIC).
    let base = |alive_state| survival_status(state, team, alive_state);

    let status = match category {
        // --- Training missions (SC_T0001..SC_T0015, SAVE1..15.TRA) ---
        MissionCategory::Training => match mission_index {
            // SC_T0001
            1 => {
                let a;
                if has_materials(state, TeamId::RED, CargoType::Raw) < 1
                    || has_materials(state, TeamId::RED, CargoType::Fuel) < 1
                    || count_ready_units(state, TeamId::RED, UnitType::ADump) <= 0
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
                        if has_unit_experience(state, team, UnitType::Commando) {
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
                        } else if count_ready_units(state, TeamId::RED, UnitType::ResearchCenter)
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
            // SC_S0009: сценарий со сравнением очков команд.
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
            // SC_S0018: у зелёных 0 центров исследований → победа красных.
            18 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::ResearchCenter) > 0 {
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
            // SC_S0024: у зелёных 0 экосфер → победа красных.
            24 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::Greenhouse) > 0 {
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
            // SC_C0001: зелёные потеряли центр исследований → поражение красных;
            // 0 экосфер у зелёных → победа.
            1 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::ResearchCenter) == 0 {
                            b = Lost;
                        } else if count_ready_units(state, TeamId::GREEN, UnitType::Greenhouse) > 0
                        {
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
            // SC_C0002: потеря центра исследований красных → поражение; лимит
            // ходов → победа.
            2 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_casualties(state, TeamId::RED, UnitType::ResearchCenter) > 0 {
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
            // SC_C0003: снабжение и диверсант.
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
                        } else if count_ready_units(state, TeamId::RED, UnitType::Commando) > 0 {
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
            // SC_C0004 / SC_C0005 / SC_C0009: 0 экосфер у зелёных → победа.
            4 | 5 | 9 => {
                let mut b = base(Pending);
                if red {
                    if b != Lost {
                        if count_ready_units(state, TeamId::GREEN, UnitType::Greenhouse) > 0 {
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
            // SC_C0008: инопланетные юниты, лимит ходов.
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
            // SC_C0006 / SC_C0007 и прочие — generic-скрипт.
            _ => default_win_loss_conditions(state, team),
        },

        // --- Custom / Multiplayer / Hot-seat / Demo / MPS — SC_L0002 ---
        _ => default_win_loss_conditions(state, team),
    };

    Some(status)
}

/// `WinLoss_EvaluateStatus` (`winloss.cpp:53`) — дословный порт.
///
/// `turn_counter` сохранён для соответствия сигнатуре C++; предикаты читают
/// [`GameState::turn_counter`] (как `MAX_REGISTRY.TeamTurnValue`).
pub fn evaluate_status(state: &GameState, turn_counter: i32) -> WinLossStatus {
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

        // C++: у `PLAYER_TEAM_ALIEN` `team_type = TEAM_TYPE_NONE` (`ResourceManager_InitTeamInfo`),
        // поэтому служебная команда обломков не попадает в `teams_total`/
        // `teams_in_play` и не участвует в условиях победы. В max_core слот
        // `ALIEN` помечен `Computer`, поэтому пропускаем его явно.
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

    // Пузырьковая сортировка C++ (`winloss.cpp:98`): по убыванию очков, при
    // равенстве — по «стоимости базы» (больше стоимость — выше).
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

/// `menu_check_end_game_conditions` (`menu.cpp:548`) в части правил generic
/// партии (без UI).
///
/// Условия конца партии (проверяются для каждой команды, как в C++):
/// 1. статус локального игрока `WON`/`LOST`;
/// 2. остался один играющий (`teams_in_play == 1`, `teams_total > 1`) и он не
///    `PENDING`;
/// 3. не осталось играющих (`teams_in_play == 0`);
/// 4. выбиты все люди, но не все были компьютерами;
/// 5. `VICTORY_TYPE_DURATION` и лимит ходов достигнут;
/// 6. `VICTORY_TYPE_SCORE` и очки команды ≥ лимита.
///
/// Дополнительно (ветка не-сетевой игры, `menu.cpp:593-609`): если у
/// локального игрока статус `GENERIC`, а команда-человек проиграла — она
/// помечается `TeamKind::Eliminated`.
///
/// Возвращает `true`, если партия завершена (`menu_wrap_up_game`).
pub fn check_end_game_conditions(state: &mut GameState) -> bool {
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
            // Сетевая игра в C++ возвращает true; локальная — помечает команду
            // выбитой и продолжает. max_core — lockstep, поэтому берём
            // не-сетевую ветку.
            if state.network_game {
                return true;
            }

            state.teams[team].team_type = TeamKind::Eliminated;
        }
    }

    false
}

/// Число команд без `ALIEN` — вспомогательная константа для тестов.
pub const PLAYING_TEAMS: usize = PLAYER_TEAM_COUNT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::TeamClan;
    use crate::game::GameState;
    use crate::geo::Point;
    use crate::team::TeamUnits;
    use crate::unit::UnitId;

    fn empty_match() -> GameState {
        let mut state = GameState::new();
        state.game_state = crate::enums::GameStateType::InGame;
        state.mission_category = MissionCategory::Custom;
        state.mission_index = 0;
        state.victory_type = VictoryType::Duration;
        state.victory_limit = 50;
        // Дуэль: красные — человек, зелёные — компьютер, синие/серые не играют.
        // Сетевая ветка выключена, чтобы проверять локальную семантику
        // `menu_check_end_game_conditions` (помечивание Eliminated).
        state.player_team = TeamId::RED;
        state.network_game = false;

        for team in 0..PLAYER_TEAM_COUNT {
            state.teams[team].team_type = if team == 0 {
                TeamKind::Player
            } else if team == 1 {
                TeamKind::Computer
            } else {
                TeamKind::None
            };
            state.team_units[team] = TeamUnits::with_clan(TeamClan::Random);
        }

        state
    }

    fn spawn(state: &mut GameState, unit_type: UnitType, team: TeamId, x: i16, y: i16) {
        let flags = crate::access::unit_type_flags(unit_type);
        state
            .spawn_unit(unit_type, team, Point::new(x, y), flags)
            .expect("unit spawns");
    }

    #[test]
    fn team_without_units_is_lost() {
        let state = empty_match();
        assert_eq!(
            default_win_loss_conditions(&state, TeamId::RED),
            WinLossState::Lost
        );
    }

    #[test]
    fn army_with_ammo_is_alive_and_gets_generic() {
        let mut state = empty_match();
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        assert_eq!(
            default_win_loss_conditions(&state, TeamId::RED),
            WinLossState::Generic
        );
        // Подлодка и диверсант не считаются военной мощью (save.md, corner case).
        let mut stealth = empty_match();
        spawn(&mut stealth, UnitType::Submarine, TeamId::RED, 20, 20);
        spawn(&mut stealth, UnitType::Commando, TeamId::RED, 21, 20);
        assert_eq!(
            default_win_loss_conditions(&stealth, TeamId::RED),
            WinLossState::Lost
        );
    }

    #[test]
    fn production_and_construction_infrastructure_keep_team_alive() {
        let mut state = empty_match();
        spawn(&mut state, UnitType::LightVehiclePlant, TeamId::RED, 20, 20);
        // Фабрика без топлива/материалов и без шахты — не производство.
        assert!(!has_production_infrastructure(&state, TeamId::RED));

        spawn(&mut state, UnitType::MiningStation, TeamId::RED, 22, 20);
        assert!(!has_production_infrastructure(&state, TeamId::RED));
        // Генератор даёт энергию (power_demand = 2 для шахты).
        spawn(&mut state, UnitType::PowerGenerator, TeamId::RED, 24, 20);
        assert!(!has_production_infrastructure(&state, TeamId::RED));
        // Энергии хватает (POWGEN даёт 1) — нет, demand 2 → нужен ещё один.
        spawn(&mut state, UnitType::PowerGenerator, TeamId::RED, 26, 20);
        assert!(has_production_infrastructure(&state, TeamId::RED));
        assert_eq!(
            default_win_loss_conditions(&state, TeamId::RED),
            WinLossState::Generic
        );

        let mut construction = empty_match();
        spawn(
            &mut construction,
            UnitType::Constructor,
            TeamId::RED,
            20,
            20,
        );
        spawn(&mut construction, UnitType::Engineer, TeamId::RED, 21, 20);
        // 38 материалов (без шахты и без своей генерации) — нет ресурсов.
        // Спавн даёт `storage` из таблицы атрибутов, поэтому обнуляем его явно.
        let ids = construction.unit_ids_in_order();
        for id in ids {
            if let Some(mut unit) = construction.unit(id).cloned() {
                unit.storage = 0;
                construction.write_unit(id, unit);
            }
        }
        assert_eq!(has_materials(&construction, TeamId::RED, CargoType::Raw), 0);
        assert!(!has_construction_infrastructure(&construction, TeamId::RED));
        let first = construction.unit_ids_in_order()[0];
        if let Some(mut unit) = construction.unit(first).cloned() {
            unit.storage = 38;
            construction.write_unit(first, unit);
        }
        assert!(has_construction_infrastructure(&construction, TeamId::RED));
    }

    #[test]
    fn last_team_standing_wins_generic_game() {
        let mut state = empty_match();
        state.player_team = TeamId::RED;
        // Красные живы танком, зелёные пусты (и потому LOST).
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        let status = evaluate_status(&state, state.turn_counter);
        assert_eq!(status.teams_total, 2);
        assert_eq!(status.teams_in_play, 1);
        assert_eq!(
            status.team_status[TeamId::RED.0 as usize],
            WinLossState::Generic
        );
        assert_eq!(
            status.team_status[TeamId::GREEN.0 as usize],
            WinLossState::Lost
        );
        assert_eq!(status.team_rank[TeamId::RED.0 as usize], 1);
        assert!(check_end_game_conditions(&mut state));
    }

    #[test]
    fn score_limit_ends_the_game() {
        let mut state = empty_match();
        state.player_team = TeamId::RED;
        state.victory_type = VictoryType::Score;
        state.victory_limit = 25;
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
        assert!(!check_end_game_conditions(&mut state));

        state.teams[TeamId::GREEN.0 as usize].team_points = 25;
        assert!(check_end_game_conditions(&mut state));
    }

    #[test]
    fn duration_limit_ends_the_game() {
        let mut state = empty_match();
        state.player_team = TeamId::RED;
        state.victory_type = VictoryType::Duration;
        state.victory_limit = 30;
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
        state.turn_counter = 29;
        assert!(!check_end_game_conditions(&mut state));
        state.turn_counter = 30;
        assert!(check_end_game_conditions(&mut state));
    }

    #[test]
    fn ranks_break_ties_by_team_index_then_assets() {
        let mut state = empty_match();
        state.player_team = TeamId::RED;
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
        let status = evaluate_status(&state, 0);
        // Равные очки и равные базы → меньший индекс выше (RED=0).
        assert_eq!(status.team_rank[TeamId::RED.0 as usize], 1);
        assert_eq!(status.team_rank[TeamId::GREEN.0 as usize], 2);

        // Больше очков у зелёных → зелёные выше.
        state.teams[TeamId::GREEN.0 as usize].team_points = 10;
        let status = evaluate_status(&state, 0);
        assert_eq!(status.team_rank[TeamId::GREEN.0 as usize], 1);
        assert_eq!(status.team_rank[TeamId::RED.0 as usize], 2);
    }

    #[test]
    fn mission_18_wins_when_green_has_no_research_center() {
        let mut state = empty_match();
        state.player_team = TeamId::RED;
        state.mission_category = MissionCategory::Scenario;
        state.mission_index = 18;
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
        spawn(&mut state, UnitType::ResearchCenter, TeamId::GREEN, 42, 40);

        let status = evaluate_status(&state, 0);
        assert_eq!(
            status.team_status[TeamId::RED.0 as usize],
            WinLossState::Pending
        );
        assert_eq!(
            status.team_status[TeamId::GREEN.0 as usize],
            WinLossState::Generic
        );

        // Уничтожаем центр исследований зелёных.
        let research = state
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                state
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::ResearchCenter)
            })
            .expect("green research center");
        state.destroy_unit(research);

        let status = evaluate_status(&state, 0);
        assert_eq!(
            status.team_status[TeamId::RED.0 as usize],
            WinLossState::Won
        );
        assert_eq!(
            status.team_status[TeamId::GREEN.0 as usize],
            WinLossState::Generic
        );
    }

    #[test]
    fn campaign_1_loses_when_green_loses_research_center() {
        let mut state = empty_match();
        state.player_team = TeamId::RED;
        state.mission_category = MissionCategory::Campaign;
        state.mission_index = 1;
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
        spawn(&mut state, UnitType::ResearchCenter, TeamId::GREEN, 42, 40);
        spawn(&mut state, UnitType::Greenhouse, TeamId::GREEN, 44, 40);

        assert_eq!(
            mission_win_loss(&state, MissionCategory::Campaign, 1, TeamId::RED),
            Some(WinLossState::Pending)
        );

        let research = state
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                state
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::ResearchCenter)
            })
            .expect("green research center");
        state.destroy_unit(research);

        assert_eq!(
            mission_win_loss(&state, MissionCategory::Campaign, 1, TeamId::RED),
            Some(WinLossState::Lost)
        );

        // Оставшийся без центра исследований зелёный тоже получает GENERIC
        // (он не RED и жив).
        assert_eq!(
            mission_win_loss(&state, MissionCategory::Campaign, 1, TeamId::GREEN),
            Some(WinLossState::Generic)
        );
    }

    #[test]
    fn training_15_wins_on_armor_research() {
        let mut state = empty_match();
        state.mission_category = MissionCategory::Training;
        state.mission_index = 15;
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        spawn(&mut state, UnitType::ResearchCenter, TeamId::RED, 22, 20);

        assert_eq!(
            mission_win_loss(&state, MissionCategory::Training, 15, TeamId::RED),
            Some(WinLossState::Pending)
        );

        state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Armor as usize]
            .research_level = 1;
        assert_eq!(
            mission_win_loss(&state, MissionCategory::Training, 15, TeamId::RED),
            Some(WinLossState::Won)
        );
    }

    #[test]
    fn mission_categories_follow_file_types() {
        assert_eq!(
            MissionCategory::from_file_type(FileType::Custom),
            MissionCategory::Custom
        );
        assert_eq!(
            MissionCategory::from_file_type(FileType::Tutorial),
            MissionCategory::Training
        );
        assert_eq!(
            MissionCategory::from_file_type(FileType::Campaign),
            MissionCategory::Campaign
        );
        assert_eq!(
            MissionCategory::from_file_type(FileType::Scenario),
            MissionCategory::Scenario
        );
        assert_eq!(
            MissionCategory::from_file_type(FileType::MultiScenario),
            MissionCategory::MultiPlayerScenario
        );
        assert_eq!(MissionCategory::Training as i32, 1);
    }

    #[test]
    fn cargo_types_match_sc_units_table() {
        assert_eq!(cargo_type_of(UnitType::MiningStation), CargoType::Raw);
        assert_eq!(cargo_type_of(UnitType::FDump), CargoType::Fuel);
        assert_eq!(cargo_type_of(UnitType::GoldTruck), CargoType::Gold);
        assert_eq!(cargo_type_of(UnitType::Barracks), CargoType::Land);
        assert_eq!(cargo_type_of(UnitType::Hangar), CargoType::Air);
        assert_eq!(cargo_type_of(UnitType::Dock), CargoType::Sea);
        assert_eq!(cargo_type_of(UnitType::Tank), CargoType::None);
    }

    #[test]
    fn build_cost_and_consumption_rates_match_cpp() {
        let mut unit = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId::NO_ID);
        unit.base_values.turns = 5;
        // Мобильный юнит (не STATIONARY): ставка LANDPLT = 3 × turns.
        assert_eq!(get_normal_rate_build_cost(&unit), 15);

        let mut stationary = UnitInfo::new(UnitType::MiningStation, TeamId::RED, UnitId::NO_ID);
        stationary.base_values.turns = 6;
        stationary.flags |= unit_flags::STATIONARY;
        assert_eq!(get_normal_rate_build_cost(&stationary), 12);

        let mut infantry = UnitInfo::new(UnitType::Infantry, TeamId::RED, UnitId::NO_ID);
        infantry.base_values.turns = 4;
        assert_eq!(get_normal_rate_build_cost(&infantry), 4);

        assert_eq!(cargo_power_consumption_rate(UnitType::PowerStation), -6);
        assert_eq!(cargo_power_consumption_rate(UnitType::MiningStation), 1);
        assert_eq!(
            cargo_raw_consumption_rate(UnitType::LightVehiclePlant, 2),
            12
        );
        assert_eq!(cargo_gold_consumption_rate(UnitType::CommTower), 5);
    }

    #[test]
    fn every_mission_index_has_a_rule_and_returns_a_status() {
        // Скрипты покрывают training 1..15, scenario 1..24, campaign 1..9;
        // прочие номера и категории падают в generic-скрипт `SC_L0002`.
        let mut state = empty_match();
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);

        for category in [
            MissionCategory::Custom,
            MissionCategory::Training,
            MissionCategory::Campaign,
            MissionCategory::HotSeat,
            MissionCategory::Multiplayer,
            MissionCategory::Demo,
            MissionCategory::Scenario,
            MissionCategory::MultiPlayerScenario,
        ] {
            for mission_index in 0..=25u16 {
                for team in 0..TEAM_SLOTS {
                    let status =
                        mission_win_loss(&state, category, mission_index, TeamId(team as u8));
                    assert!(status.is_some(), "{category:?}/{mission_index}/{team}");
                }
            }
        }
    }

    #[test]
    fn training_predicates_read_the_right_unit_fields() {
        let mut state = empty_match();
        state.mission_category = MissionCategory::Training;

        // Шахта с включённым питанием: максимумы и факт добычи.
        let mining = state
            .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), 0)
            .expect("mining station");
        {
            let unit = state.unit_mut(mining).expect("unit");
            unit.orders = UnitOrder::PowerOn;
            unit.raw_mining_max = 12;
            unit.fuel_mining_max = 3;
            unit.gold_mining_max = 1;
            unit.raw_mining = 5;
            unit.fuel_mining = 2;
            unit.gold_mining = 1;
            unit.total_mining = 8;
        }

        assert_eq!(count_total_mining(&state, TeamId::RED), (12, 3, 1));
        assert_eq!(get_total_mining(&state, TeamId::RED, CargoType::Raw), 5);
        assert_eq!(get_total_mining(&state, TeamId::RED, CargoType::Fuel), 2);
        assert_eq!(get_total_mining(&state, TeamId::RED, CargoType::Gold), 1);

        // Выключенная шахта не даёт ни максимумов, ни добычи.
        {
            let unit = state.unit_mut(mining).expect("unit");
            unit.orders = UnitOrder::PowerOff;
        }
        assert_eq!(count_total_mining(&state, TeamId::RED), (0, 0, 0));
        assert_eq!(get_total_mining(&state, TeamId::RED, CargoType::Raw), 0);

        // `Cargo_GetNetProduction` шахты: gold + raw + fuel и потребление.
        {
            let unit = state.unit_mut(mining).expect("unit");
            unit.orders = UnitOrder::PowerOn;
        }
        let net = cargo_get_net_production(&state, state.unit(mining).unwrap(), false);
        assert_eq!(net.raw, 5);
        assert_eq!(net.fuel, 2);
        assert_eq!(net.gold, 1);
        assert_eq!(net.power, -1);
        assert_eq!(net.free_capacity, 16 - 8);

        // Фабрика в ORDER_BUILD потребляет материалы по `build_rate`.
        let plant = state
            .spawn_unit(
                UnitType::LightVehiclePlant,
                TeamId::RED,
                Point::new(24, 20),
                0,
            )
            .expect("plant");
        {
            let unit = state.unit_mut(plant).expect("unit");
            unit.orders = UnitOrder::Build;
            unit.state = UnitOrderState::BuildInProgress;
            unit.build_time = 3;
            unit.build_rate = 1;
            unit.build_list = vec![UnitType::Tank];
        }
        assert_eq!(
            get_total_units_being_constructed(&state, TeamId::RED, UnitType::Tank),
            1
        );
        assert_eq!(
            get_total_power_consumption(&state, TeamId::RED, UnitType::LightVehiclePlant),
            1
        );
        let net = cargo_get_net_production(&state, state.unit(plant).unwrap(), false);
        assert_eq!(net.raw, -3); // LIGHTPLT: 3 × build_rate
        assert_eq!(net.power, -1);
    }

    #[test]
    fn unit_state_predicates_match_cpp() {
        let mut state = empty_match();
        let tank = state
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
            .expect("tank");

        // Повреждённый юнит (hits ниже базовых).
        {
            let unit = state.unit_mut(tank).expect("unit");
            unit.hits = unit.base_values.get_attribute(ATTRIB_HITS) as u16 - 1;
        }
        assert_eq!(count_damaged_units(&state, TeamId::RED, UnitType::Tank), 1);

        // Уменьшенный боезапас.
        {
            let unit = state.unit_mut(tank).expect("unit");
            unit.ammo = 0;
        }
        assert_eq!(
            count_units_with_reduced_attrib(&state, TeamId::RED, UnitType::Tank, ATTRIB_AMMO),
            1
        );
        assert_eq!(
            count_units_with_reduced_attrib(&state, TeamId::RED, UnitType::Tank, ATTRIB_HITS),
            1
        );
        assert_eq!(
            count_units_with_reduced_attrib(&state, TeamId::RED, UnitType::Tank, ATTRIB_STORAGE),
            0
        );

        // Опыт (`storage` у COMMANDO) и версия (mark level).
        let commando = state
            .spawn_unit(UnitType::Commando, TeamId::RED, Point::new(22, 20), 0)
            .expect("commando");
        {
            let unit = state.unit_mut(commando).expect("unit");
            unit.storage = 1;
        }
        assert!(has_unit_experience(&state, TeamId::RED, UnitType::Commando));

        {
            let unit = state.unit_mut(tank).expect("unit");
            unit.base_values.version = 2;
        }
        assert!(has_unit_above_mark_level(
            &state,
            TeamId::RED,
            UnitType::Tank,
            1
        ));

        // Потери и уровень исследований.
        state.teams[TeamId::RED.0 as usize].casualties[UnitType::Tank as usize] = 3;
        assert_eq!(count_casualties(&state, TeamId::RED, UnitType::Tank), 3);

        state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Armor as usize]
            .research_level = 4;
        assert_eq!(
            get_research_level(&state, TeamId::RED, ResearchTopic::Armor),
            4
        );
    }

    #[test]
    fn worth_of_assets_counts_three_lists_only() {
        let mut state = empty_match();
        spawn(&mut state, UnitType::Tank, TeamId::RED, 10, 10);
        spawn(&mut state, UnitType::Engineer, TeamId::RED, 12, 10);
        // Ground cover (плита) в стоимость базы не входит (`winloss.cpp:47`).
        spawn(&mut state, UnitType::LargeSlab, TeamId::RED, 14, 10);

        let tank_cost = state
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                state
                    .unit(*id)
                    .is_some_and(|u| u.unit_type == UnitType::Tank)
            })
            .map(|id| get_normal_rate_build_cost(state.unit(id).unwrap()))
            .expect("tank");
        let engineer_cost = state
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                state
                    .unit(*id)
                    .is_some_and(|u| u.unit_type == UnitType::Engineer)
            })
            .map(|id| get_normal_rate_build_cost(state.unit(id).unwrap()))
            .expect("engineer");

        assert_eq!(
            get_total_worth_of_assets(&state, TeamId::RED),
            tank_cost + engineer_cost
        );
    }
}
