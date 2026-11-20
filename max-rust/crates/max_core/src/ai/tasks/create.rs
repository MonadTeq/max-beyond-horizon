//! Создание юнитов и зданий: `TaskCreate*`, `TaskObtainUnits`, `TaskActivate`.
//!
//! Оракул: `taskcreate.cpp/.hpp`, `taskcreateunit.cpp/.hpp`,
//! `taskcreatebuilding.cpp/.hpp`, `taskobtainunits.cpp/.hpp`,
//! `taskactivate.cpp/.hpp`, `builder.cpp` (`Builder_GetBuilderType`).
//!
//! # Что портировано дословно
//!
//! * `TaskCreateUnit`: состояния `CREATE_UNIT_STATE_*`, `Init`/`BeginTurn`/
//!   `EndTurn`/`Execute`/`AddUnit`/`RemoveUnit`, `WaitForMaterials`,
//!   `IsUnitStillNeeded`, `IsActivelyBuilding`, `HasCommittedToSite`.
//! * `TaskObtainUnits`: `CountInstancesOfUnitType`, `FindUnit`,
//!   `IsValidCandidate`, `AddUnit`, `Init`/`BeginTurn`/`EndTurn`,
//!   `RequestUnits` (упрощённо, см. ниже), `IsEndTurnNeeded`.
//! * `TaskCreateBuilding`: конструктор, `Init`, `AddUnit`, `Execute`,
//!   `BeginTurn`, `EndTurn`, `RequestBuilder`, `BeginBuilding`,
//!   `FindBuildSite`, `CheckMaterials`, `Activate`, `Finish`, `Abort`,
//!   `IsActivelyBuilding`, `HasCommittedToSite`, `IsEndTurnNeeded`.
//!
//! # Осознанные упрощения (P6)
//!
//! 1. `Builder_GetBuildableUnits`/`BuildMenu_*` — данные производства
//!    (P3-таблица `resourcetable`), поэтому `TaskObtainUnits::RequestUnits`
//!    заказывает по одному `TaskCreateUnit` на недостающий юнит, а не
//!    «2×(заводов)+1 − уже заказано»;
//! 2. `CheckMaterials` считает материалы команды по комплексам
//!    (`TeamUnits::complexes()`), порог — 10 единиц сырья, как для постройки
//!    юнита в C++; если комплексов у команды нет (P3-экономика не
//!    смоделирована), материалы считаются доступными;
//! 3. участки под дороги/мосты/водные платформы (`BuildRoad`,
//!    `BuildBoardwalks`, `BuildBridges`, `RequestWaterPlatform`) не
//!    портированы — они опираются на `AccessMap` и `Zone`-очистку (P5/P7);
//!    соответствующие состояния остаются «блокирующими», как в C++ при
//!    отказе запроса.

use crate::enums::{unit_flags, CautionLevel, UnitOrder, UnitOrderState, UnitType};
use crate::geo::Point;
use crate::team::TeamId;
use crate::unit::UnitId;

use super::super::task::{
    task_estimate_turns_till_mission_end, task_retreat_from_danger, TaskHeader, TASK_PRIORITY_MOVE,
    TASK_PRIORITY_OFFSET_CENSUS_SLACK, TASK_TYPE_CREATE_UNIT, TASK_TYPE_OBTAIN_UNITS,
};
use super::super::task_manager::TaskManager;
use super::{create_building_state, create_unit_state, TaskCtx, TaskKind};

// ---------------------------------------------------------------------------
// Данные производства (замена `Builder_*`/`BuildMenu_*`)
// ---------------------------------------------------------------------------

/// `Builder_GetBuilderType(team, unit_type)` — тип юнита, которым строят
/// (`builder.cpp`).
///
/// P2: сначала точная таблица `max_builder_capability_list` из
/// `default.game_rules.lua` ([`crate::production::builder_type`]) — она задаёт
/// и `LIGHTPLT` для `SCOUT`/`REPAIR`, и `TRAINHAL` для `INFANTRY`/`COMMANDO`,
/// и `SHIPYARD`/`AIRPLT` для флота/авиации. Fallback по классу юнита — для
/// типов вне таблицы (aliens, ракеты, обломки).
pub fn builder_type_for(unit_type: UnitType) -> UnitType {
    if let Some(builder) = crate::production::builder_type(unit_type) {
        return builder;
    }

    let flags = crate::access::unit_type_flags(unit_type);

    if flags & unit_flags::STATIONARY != 0 {
        UnitType::Constructor
    } else if flags & unit_flags::MOBILE_AIR_UNIT != 0 {
        UnitType::AirUnitsPlant
    } else if flags & unit_flags::MOBILE_SEA_UNIT != 0 {
        UnitType::Shipyard
    } else if matches!(unit_type, UnitType::Infantry | UnitType::Commando) {
        UnitType::TrainingHall
    } else if flags & unit_flags::MOBILE_LAND_UNIT != 0 {
        UnitType::HeavyVehiclePlant
    } else {
        UnitType::Constructor
    }
}

/// `BuildMenu_GetTurnsToBuild(unit_type, team)` — ходы на постройку.
///
/// P2: единая формула с `BuildMenu` (`max_core::production::turns_to_build`,
/// включая множитель сложности для компьютера).
pub fn turns_to_build(world: &crate::game::GameState, team: TeamId, unit_type: UnitType) -> i32 {
    crate::production::turns_to_build(world, team, unit_type)
}

/// Материалы команды (сумма по комплексам). `None` — комплексов нет.
pub fn team_materials(world: &crate::game::GameState, team: TeamId) -> Option<i32> {
    let complexes = world.team_units[team.0 as usize].complexes();

    if complexes.is_empty() {
        return None;
    }

    Some(
        complexes
            .iter()
            .map(|complex| i32::from(complex.material))
            .sum(),
    )
}

/// `TaskCreateBuilding::CheckMaterials` — хватает ли материалов на постройку.
pub fn check_materials(world: &crate::game::GameState, team: TeamId, unit_type: UnitType) -> bool {
    const REQUIRED_RAW_MATERIALS: i32 = 10;

    let turns = turns_to_build(world, team, unit_type);
    let _ = turns;

    match team_materials(world, team) {
        None => true,
        Some(materials) => materials >= REQUIRED_RAW_MATERIALS,
    }
}

/// `UnitsManager_NeedToReserveRawMaterials` для команды.
///
/// Порт `TaskManager_NeedToReserveRawMaterials` (`ai.cpp:434`): сырьё,
/// которое везут юниты, минус расход зданий команды.
pub fn need_to_reserve_raw_materials(world: &crate::game::GameState, team: TeamId) -> bool {
    let mut raw_materials: i32 = -10;

    for id in world.unit_list(crate::unit::UnitListKind::MobileLandSea) {
        let Some(unit) = world.unit(*id) else {
            continue;
        };
        if unit.team != team {
            continue;
        }

        let cargo = cargo_type_of(unit.unit_type);
        if cargo == Some(CargoType::Raw) {
            raw_materials += i32::from(unit.storage);

            if unit.unit_type == UnitType::Engineer {
                raw_materials -= 15;
            }

            if unit.unit_type == UnitType::Constructor {
                raw_materials -= 30;
            }
        }
    }

    for id in world.unit_list(crate::unit::UnitListKind::Stationary) {
        let Some(unit) = world.unit(*id) else {
            continue;
        };
        if unit.team != team {
            continue;
        }

        raw_materials -= raw_consumption_rate(unit.unit_type);

        if cargo_type_of(unit.unit_type) == Some(CargoType::Raw) {
            raw_materials += i32::from(unit.storage);

            let storage_max = unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_STORAGE);
            if i32::from(unit.storage) == storage_max {
                return false;
            }
        }
    }

    raw_materials < 0
}

/// Тип груза юнита — `Unit::CargoType` (`unit.hpp`), минимальный набор.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoType {
    /// Сырьё (`CARGO_TYPE_RAW`).
    Raw,
    /// Топливо.
    Fuel,
    /// Золото.
    Gold,
}

/// `ResourceManager_GetUnit(type).GetCargoType()`.
pub fn cargo_type_of(unit_type: UnitType) -> Option<CargoType> {
    match unit_type {
        UnitType::Constructor
        | UnitType::Engineer
        | UnitType::Bulldozer
        | UnitType::Harvester
        | UnitType::SupplyTruck
        | UnitType::GoldTruck
        | UnitType::FuelTruck => Some(CargoType::Raw),
        UnitType::Depot
        | UnitType::ADump
        | UnitType::MiningStation
        | UnitType::Greenhouse
        | UnitType::Barracks => Some(CargoType::Raw),
        _ => None,
    }
}

/// `Cargo_GetRawConsumptionRate(unit_type, 1)` — расход сырья зданием.
///
/// Таблица потребления — данные P3 (`resourcetable`); здесь — те же значения,
/// что у зданий-потребителей по классам.
pub fn raw_consumption_rate(unit_type: UnitType) -> i32 {
    match unit_type {
        UnitType::MiningStation | UnitType::Greenhouse | UnitType::Habitat => 2,
        UnitType::PowerStation | UnitType::PowerGenerator => 1,
        UnitType::ResearchCenter | UnitType::CommTower => 1,
        UnitType::Barracks | UnitType::Depot | UnitType::Hangar | UnitType::Dock => 1,
        UnitType::LightVehiclePlant
        | UnitType::HeavyVehiclePlant
        | UnitType::Shipyard
        | UnitType::AirUnitsPlant
        | UnitType::TrainingHall => 2,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Выбор площадки
// ---------------------------------------------------------------------------

/// Площадка под здание валидна: 2×2 доступны и свободны от юнитов.
fn is_valid_building_site(
    world: &crate::game::GameState,
    team: TeamId,
    unit_type: UnitType,
    site: Point,
) -> bool {
    let building = crate::access::unit_type_flags(unit_type) & unit_flags::BUILDING != 0;
    let size = if building { 2 } else { 1 };

    for dy in 0..size {
        for dx in 0..size {
            let grid = Point::new(site.x + dx, site.y + dy);

            if world.map.index(grid.x, grid.y).is_none() {
                return false;
            }

            if crate::access::is_accessible(
                world,
                unit_type,
                team,
                grid,
                crate::access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS,
            ) == 0
            {
                return false;
            }
        }
    }

    true
}

/// Сырьё в клетке (`ResourceMapEntry`): количество.
fn resource_amount(world: &crate::game::GameState, site: Point) -> i32 {
    if world.map.index(site.x, site.y).is_none() {
        return 0;
    }

    let entry = world.map.resource_at(site.x, site.y);

    i32::from(entry.amount())
}

/// `TaskCreateBuilding::FindBuildSite` — поиск свободной площадки рядом.
///
/// C++ сканирует карту `AccessMap` вокруг заявленного сайта; здесь — квадрат
/// радиуса 8 вокруг сайта в порядке `y`, затем `x` (как `rect`-циклы C++),
/// с предпочтением площадок рядом с сырьём для рудной станции.
pub fn find_build_site(
    world: &crate::game::GameState,
    team: TeamId,
    unit_type: UnitType,
    origin: Point,
) -> Option<Point> {
    if is_valid_building_site(world, team, unit_type, origin) && origin.x >= 0 && origin.y >= 0 {
        return Some(origin);
    }

    const RADIUS: i16 = 8;
    let mut best: Option<(i32, Point)> = None;

    let mut y = origin.y - RADIUS;
    while y <= origin.y + RADIUS {
        let mut x = origin.x - RADIUS;
        while x <= origin.x + RADIUS {
            let site = Point::new(x, y);

            if x >= 0 && y >= 0 && is_valid_building_site(world, team, unit_type, site) {
                let score = if unit_type == UnitType::MiningStation {
                    let mut ore = 0;
                    for dy in -1..=2 {
                        for dx in -1..=2 {
                            ore += resource_amount(world, Point::new(x + dx, y + dy));
                        }
                    }
                    ore
                } else {
                    0
                };

                let dx = i32::from(x) - i32::from(origin.x);
                let dy = i32::from(y) - i32::from(origin.y);
                let distance = dx * dx + dy * dy;

                match best {
                    None => best = Some((score * 1000 - distance, site)),
                    Some((best_score, _)) => {
                        if score * 1000 - distance > best_score {
                            best = Some((score * 1000 - distance, site));
                        }
                    }
                }
            }

            x += 1;
        }
        y += 1;
    }

    best.map(|(_, site)| site)
}

// ---------------------------------------------------------------------------
// TaskObtainUnits
// ---------------------------------------------------------------------------

/// `TaskObtainUnits::CountInstancesOfUnitType`.
pub(crate) fn count_instances(units: &[UnitType], unit_type: UnitType) -> u32 {
    units
        .iter()
        .filter(|candidate| **candidate == unit_type)
        .count() as u32
}

/// `TaskObtainUnits::IsValidCandidate`.
pub(crate) fn is_valid_candidate(
    manager: &TaskManager,
    world: &crate::game::GameState,
    team: TeamId,
    unit_id: UnitId,
    mode: bool,
    request_priority: u16,
) -> bool {
    let Some(unit) = world.unit(unit_id) else {
        return false;
    };

    if unit.team != team || unit.hits == 0 {
        return false;
    }

    let accepts = unit.orders == UnitOrder::Await
        || unit.orders == UnitOrder::Sentry
        || (matches!(
            unit.orders,
            UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
        ) && unit.state == UnitOrderState::ExecutingOrder);

    if !accepts {
        return false;
    }

    match manager.unit_task(unit_id) {
        None => true,
        Some(task_id) => {
            let Some(task) = manager.task(task_id) else {
                return true;
            };

            if mode {
                false
            } else if !task.kind.is_unit_transferable(world, task, unit) {
                false
            } else {
                // `task->ComparePriority(GetPriority()) > 0` — приоритет запроса
                // совпадает с приоритетом родителя задачи.
                task.compare_priority(world, request_priority) > 0
            }
        }
    }
}

/// `TaskObtainUnits::FindUnit` — ближайший подходящий юнит типа.
pub(crate) fn find_unit(
    manager: &TaskManager,
    world: &crate::game::GameState,
    team: TeamId,
    unit_type: UnitType,
    point: Point,
    mode: bool,
    request_priority: u16,
) -> Option<UnitId> {
    let kind = crate::ai::task::task_unit_list_kind(unit_type);

    let mut selected: Option<UnitId> = None;
    let mut best_speed = 0;
    let mut is_unit_available = false;

    for id in world.unit_list(kind) {
        let Some(unit) = world.unit(*id) else {
            continue;
        };
        if unit.unit_type != unit_type {
            continue;
        }

        if unit.orders == UnitOrder::Build
            && unit.flags
                & (unit_flags::MOBILE_AIR_UNIT
                    | unit_flags::MOBILE_SEA_UNIT
                    | unit_flags::MOBILE_LAND_UNIT)
                != 0
        {
            let speed = i32::from(unit.build_time)
                * unit
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_SPEED)
                + crate::access::approximate_distance(
                    i32::from(point.x) - i32::from(unit.grid.x),
                    i32::from(point.y) - i32::from(unit.grid.y),
                );

            if selected.is_none() || best_speed > speed {
                is_unit_available = false;
                selected = Some(*id);
                best_speed = speed;
            }
        } else if is_valid_candidate(manager, world, team, *id, mode, request_priority) {
            let speed = crate::access::approximate_distance(
                i32::from(point.x) - i32::from(unit.grid.x),
                i32::from(point.y) - i32::from(unit.grid.y),
            );

            if selected.is_none() || best_speed > speed {
                is_unit_available = true;
                selected = Some(*id);
                best_speed = speed;
            }
        }
    }

    if is_unit_available {
        selected
    } else {
        None
    }
}

/// `TaskObtainUnits::EndTurn` — найти готовых юнитов и заказать недостающих.
///
/// Отличие от C++ (`taskobtainunits.cpp:225`): поиск готового юнита
/// повторяется на каждом `EndTurn`, пока запрос не удовлетворён. В C++ поиск
/// выполняется один раз на `Init`, потому что `TaskManageBuildings` появляется
/// только при наличии зданий и не конкурирует с разведкой за единственного
/// строителя; в max_core менеджер застройки создаётся и для «голой» базы,
/// поэтому единственный `CONSTRCT` иначе остаётся в `TaskSurvey` и запрос на
/// постройку не выполняется никогда. Остальная структура (`RequestUnits`,
/// `IsValidCandidate`, `FindUnit`, порядок перебора) — как в C++.
pub(crate) fn obtain_units_end_turn(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
) {
    let TaskKind::ObtainUnits {
        units, point, init, ..
    } = kind
    else {
        return;
    };

    let parent_needed = header
        .parent
        .map(|parent| {
            ctx.tm
                .task(parent)
                .map(|task| task.is_needed(ctx.tm))
                .unwrap_or(false)
        })
        .unwrap_or(false);

    if header.parent.is_none() || !parent_needed {
        units.clear();
    }

    {
        let mut index = units.len() as isize - 1;
        while index >= 0 {
            let unit_type = units[index as usize];

            // `TaskObtainUnits::GetPriority` — приоритет родителя.
            let request_priority = header
                .parent
                .and_then(|parent| ctx.tm.task(parent).map(|task| task.get_priority(ctx.world)))
                .unwrap_or(TASK_PRIORITY_MOVE);

            if let Some(unit_id) = find_unit(
                ctx.tm,
                ctx.world,
                header.team,
                unit_type,
                *point,
                false,
                request_priority,
            ) {
                units.remove(index as usize);

                if ctx.tm.unit_task(unit_id).is_some() {
                    ctx.tm.remove_unit_tasks(ctx.world, unit_id);

                    if ctx.world.unit(unit_id).map(|unit| unit.orders) != Some(UnitOrder::Await) {
                        ctx.set_new_order(unit_id, UnitOrder::Await, UnitOrderState::ClearPath);
                    }
                }

                if let Some(parent) = header.parent {
                    ctx.tm.task_add_unit(ctx.world, parent, unit_id);
                }
            }

            index -= 1;
        }
    }

    if *init {
        let mut requested = Vec::new();
        for unit_type in units.iter() {
            let stationary =
                crate::access::unit_type_flags(*unit_type) & unit_flags::STATIONARY != 0;

            if stationary {
                ctx.tm
                    .create_building(ctx.world, *unit_type, header.team, *point, header.id);
            } else if matches!(*unit_type, UnitType::Constructor | UnitType::Engineer) {
                ctx.tm
                    .create_unit(ctx.world, *unit_type, header.team, *point, header.id);
            } else if !requested.contains(unit_type) {
                requested.push(*unit_type);

                let count = count_instances(units, *unit_type);
                request_units(ctx, header, *point, *unit_type, count);
            }
        }

        *init = false;
    }

    // Повторный поиск юнита на следующем конце хода (см. примечание к
    // функции): в C++ это делает `TaskObtainUnits::EndTurn` по расписанию
    // `TaskManager::EndTurn`, здесь — сам себя, потому что менеджер получает
    // `EndTurn` только по событию конца хода ИИ.
    if !units.is_empty() {
        ctx.remind_turn_end(header.id, true);
    }

    if units.is_empty() {
        if let Some(parent) = header.parent {
            ctx.tm.task_child_complete(ctx.world, parent, header.id);
        }

        ctx.set_parent(header.id, None);
        ctx.remove_self(header.id);
    }
}

/// `TaskObtainUnits::RequestUnits` — заказ `TaskCreateUnit` (упрощённо).
fn request_units(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    site: Point,
    unit_type: UnitType,
    requested_amount: u32,
) {
    let turns_limit = task_estimate_turns_till_mission_end(ctx.world);
    let turns = turns_to_build(ctx.world, header.team, unit_type);

    if turns_limit < turns {
        return;
    }

    // Строитель себя не строит (см. `TaskManager::create_unit`).
    if builder_type_for(unit_type) == unit_type {
        return;
    }

    for _ in 0..requested_amount {
        let priority = ctx.priority_of(header.id);
        let kind = TaskKind::create_unit(unit_type, site);
        let _ = TASK_PRIORITY_OFFSET_CENSUS_SLACK;

        ctx.append_task(header.team, Some(header.id), priority, kind);
    }
}

/// `TaskObtainUnits::AddUnit(unit_type)` — добавить требование.
pub(crate) fn obtain_units_add_type(kind: &mut TaskKind, unit_type: UnitType) {
    if let TaskKind::ObtainUnits { units, .. } = kind {
        units.push(unit_type);
    }
}

// ---------------------------------------------------------------------------
// TaskCreateUnit
// ---------------------------------------------------------------------------

fn create_unit_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::CreateUnit {
        unit_type,
        site,
        op_state,
        builder,
    } = kind
    else {
        return;
    };

    if *op_state == create_unit_state::INITIALIZING {
        let builder_type = builder_type_for(*unit_type);

        // Защита от рекурсии: строитель, который «строит сам себя»
        // (`CONSTRCT`/`MASTER_BUILDER`), заводом не производится — задача
        // снимается (в C++ такой запрос отсекает `Builder_GetBuilderType`).
        if builder_type == *unit_type {
            *op_state = create_unit_state::FINISHED;
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            return;
        }

        let priority = ctx.priority_of(header.id);

        let mut obtain = TaskKind::obtain_units(*site);
        obtain_units_add_type(&mut obtain, builder_type);
        ctx.append_task(header.team, Some(header.id), priority, obtain);

        *op_state = create_unit_state::OBTAINING_BUILDER;
    }

    if *op_state == create_unit_state::WAITING_FOR_MATERIALS {
        if let Some(builder_id) = *builder {
            create_unit_wait_for_materials(ctx, header, *unit_type, builder_id, op_state);
        }
    }

    if *op_state == create_unit_state::BUILDING {
        if let Some(builder_id) = *builder {
            let ready = ctx
                .world
                .unit(builder_id)
                .map(|unit| unit.state == UnitOrderState::UnitReady)
                .unwrap_or(false);

            if ready {
                *op_state = create_unit_state::ACTIVATING;
            }
        }
    }

    if *op_state != create_unit_state::FINISHED {
        let _ = create_unit_is_still_needed(kind, ctx, header);
    }
}

/// `TaskCreateUnit::WaitForMaterials`.
fn create_unit_wait_for_materials(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit_type: UnitType,
    builder: UnitId,
    op_state: &mut u8,
) {
    if !ctx.world.is_active_turn(header.team) {
        return;
    }

    if !check_materials(ctx.world, header.team, unit_type) {
        return;
    }

    // Материалов достаточно: строим.
    if matches!(unit_type, UnitType::Engineer | UnitType::Constructor)
        && crate::ai::task::task_get_ready_units_count(ctx.world, header.team, unit_type) > 0
        && need_to_reserve_raw_materials(ctx.world, header.team)
    {
        return;
    }

    *op_state = create_unit_state::BUILDING;

    let build_rate = if team_materials(ctx.world, header.team).unwrap_or(0) > 100
        && ctx.world.ai.settings.opponent >= crate::enums::Opponent::Average as u8
    {
        2
    } else {
        1
    };

    // `builder->move_to_grid = 0; BuildOrder();`
    if let Some(mut unit) = ctx.unit(builder) {
        unit.target_grid = Point::ZERO;
        let turns = turns_to_build(ctx.world, header.team, unit_type);
        unit.build_time = turns as u8;
        unit.build_rate = build_rate;
        unit.build_list.clear();
        unit.build_list.push(unit_type);
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
        ctx.write_unit(builder, unit);
    }

    let mut data = ctx.order_data(builder);
    data.move_to = Point::ZERO;
    ctx.set_order_data(builder, data);
    ctx.set_new_order(builder, UnitOrder::Build, UnitOrderState::Init);
}

/// `TaskCreateUnit::IsUnitStillNeeded`.
fn create_unit_is_still_needed(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
) -> bool {
    let TaskKind::CreateUnit {
        unit_type,
        op_state,
        builder,
        ..
    } = kind
    else {
        return false;
    };

    if *op_state > create_unit_state::BUILDING {
        return true;
    }

    if !ctx.world.is_active_turn(header.team) {
        return true;
    }

    let turns_limit = task_estimate_turns_till_mission_end(ctx.world);
    let turns = turns_to_build(ctx.world, header.team, *unit_type);
    let parent_needed = header
        .parent
        .map(|parent| {
            ctx.tm
                .task(parent)
                .map(|task| task.is_needed(ctx.tm))
                .unwrap_or(false)
        })
        .unwrap_or(false);

    if turns_limit > turns && parent_needed {
        return true;
    }

    if let Some(builder_id) = *builder {
        if *op_state == create_unit_state::BUILDING {
            let state_ok = ctx
                .world
                .unit(builder_id)
                .map(|unit| unit.state == UnitOrderState::ExecutingOrder)
                .unwrap_or(false);

            if !state_ok {
                return true;
            }
        }
    }

    if *op_state == create_unit_state::BUILDING {
        if let Some(builder_id) = *builder {
            ctx.halt_building(builder_id);
        }
    }

    *op_state = create_unit_state::FINISHED;

    if let Some(builder_id) = *builder {
        ctx.clear_unit_tasks(builder_id, false);
    }

    *builder = None;
    ctx.set_parent(header.id, None);
    ctx.remove_self(header.id);

    false
}

fn create_unit_add_unit(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::CreateUnit {
        unit_type,
        op_state,
        builder,
        ..
    } = kind
    else {
        return;
    };

    let Some(unit_info) = ctx.unit(unit) else {
        return;
    };

    if *op_state == create_unit_state::OBTAINING_BUILDER
        && unit_info.flags & unit_flags::STATIONARY != 0
    {
        *op_state = create_unit_state::WAITING_FOR_MATERIALS;
        *builder = Some(unit);

        if create_unit_is_still_needed(kind, ctx, header) {
            ctx.attach(header.id, unit);
            if let TaskKind::CreateUnit {
                op_state,
                builder: Some(builder_id),
                unit_type,
                ..
            } = kind
            {
                let builder_id = *builder_id;
                let unit_type = *unit_type;
                create_unit_wait_for_materials(ctx, header, unit_type, builder_id, op_state);
            }
        }

        return;
    }

    if *op_state == create_unit_state::FINISHED && unit_info.flags & unit_flags::STATIONARY != 0 {
        ctx.clear_unit_tasks(unit, false);
        return;
    }

    if *op_state != create_unit_state::FINISHED && unit_info.unit_type == *unit_type {
        *op_state = create_unit_state::FINISHED;

        ctx.clear_unit_tasks(unit, false);

        if let Some(builder_id) = *builder {
            ctx.clear_unit_tasks(builder_id, false);
        }

        ctx.set_parent(header.id, None);
        ctx.remove_self(header.id);
    }
}

fn create_unit_remove_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, unit: UnitId) {
    let _ = ctx;
    let TaskKind::CreateUnit {
        op_state, builder, ..
    } = kind
    else {
        return;
    };

    if *builder == Some(unit) {
        if *op_state <= create_unit_state::BUILDING {
            *op_state = create_unit_state::INITIALIZING;
        }

        *builder = None;
    }
}

// ---------------------------------------------------------------------------
// TaskCreateBuilding
// ---------------------------------------------------------------------------

fn create_building_request_builder(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::CreateBuilding {
        unit_type,
        site,
        op_state,
        ..
    } = kind
    else {
        return;
    };

    let builder_type = builder_type_for(*unit_type);
    *op_state = create_building_state::GETTING_BUILDER;

    let priority = ctx.priority_of(header.id);
    let mut obtain = TaskKind::obtain_units(*site);
    obtain_units_add_type(&mut obtain, builder_type);
    ctx.append_task(header.team, Some(header.id), priority, obtain);
}

fn create_building_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    // Снимок полей: дальнейшие вызовы берут `kind` целиком, поэтому
    // заимствование полей не должно переживать чтение.
    let (unit_type, state_now, builder, manager, site, tasks_empty) = match kind {
        TaskKind::CreateBuilding {
            unit_type,
            op_state,
            builder,
            tasks,
            manager,
            site,
            ..
        } => (
            *unit_type,
            *op_state,
            *builder,
            *manager,
            *site,
            tasks.is_empty(),
        ),
        _ => return,
    };

    if let Some(builder_id) = builder {
        match state_now {
            create_building_state::EVALUATING_SITE | create_building_state::SITE_BLOCKED => {
                if state_now == create_building_state::SITE_BLOCKED && !header.needs_processing {
                    return;
                }

                match find_build_site(ctx.world, header.team, unit_type, site) {
                    Some(new_site) => {
                        if let TaskKind::CreateBuilding { site, op_state, .. } = kind {
                            *site = new_site;
                            *op_state = create_building_state::MOVING_TO_SITE;
                        }
                    }
                    None => {
                        if let Some(manager_id) = manager {
                            // `manager->ChangeSite` — попросить менеджера другой сайт.
                            ctx.tm.task_child_complete(ctx.world, manager_id, header.id);
                            ctx.remove_self(header.id);
                        } else if let TaskKind::CreateBuilding { op_state, .. } = kind {
                            *op_state = create_building_state::SITE_BLOCKED;
                            ctx.set_processing_needed(header.id, false);
                        }

                        return;
                    }
                }

                create_building_execute(kind, ctx, header, builder_id);
            }
            create_building_state::MOVING_TO_SITE => {
                let await_order = ctx
                    .world
                    .unit(builder_id)
                    .map(|unit| {
                        unit.orders == UnitOrder::Await
                            && ctx.tm.unit_task(builder_id) == Some(header.id)
                    })
                    .unwrap_or(false);

                if await_order {
                    create_building_execute(kind, ctx, header, builder_id);
                }
            }
            create_building_state::BUILDING => {
                let unit = ctx.unit(builder_id);
                let is_top = ctx.tm.unit_task(builder_id) == Some(header.id);

                if is_top {
                    match unit {
                        Some(unit) if unit.orders == UnitOrder::Await => {
                            if let TaskKind::CreateBuilding { op_state, .. } = kind {
                                *op_state = create_building_state::MOVING_TO_SITE;
                            }
                        }
                        Some(unit) if unit.state == UnitOrderState::UnitReady => {
                            create_building_activate(kind, ctx, header);
                        }
                        Some(unit)
                            if unit.orders == UnitOrder::Build
                                && unit.state == UnitOrderState::BuildInProgress
                                && crate::ai::ai_is_dangerous_location(
                                    ctx.world,
                                    &unit,
                                    site,
                                    CautionLevel::AvoidNextTurnsFire,
                                ) =>
                        {
                            // `AbandonSite`.
                            if ctx.world.is_active_turn(header.team) {
                                ctx.halt_building(builder_id);
                                if let TaskKind::CreateBuilding { op_state, .. } = kind {
                                    *op_state = create_building_state::MOVING_TO_SITE;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            create_building_state::FINISHED | create_building_state::MOVING_OFF_SITE => {
                let await_order = ctx
                    .world
                    .unit(builder_id)
                    .map(|unit| {
                        unit.orders == UnitOrder::Await
                            && ctx.tm.unit_task(builder_id) == Some(header.id)
                    })
                    .unwrap_or(false);

                if await_order {
                    create_building_finish(kind, ctx, header);
                }
            }
            _ => {
                create_building_execute(kind, ctx, header, builder_id);
            }
        }

        return;
    }

    let should_request = unit_type != UnitType::CommTower
        && state_now != create_building_state::GETTING_BUILDER
        && state_now < create_building_state::BUILDING
        && tasks_empty;

    if should_request {
        create_building_request_builder(kind, ctx, header);
    }
}

fn create_building_add_unit(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::CreateBuilding {
        unit_type,
        site,
        op_state,
        builder,
        building,
        manager,
        tasks,
        ..
    } = kind
    else {
        return;
    };

    let Some(unit_info) = ctx.unit(unit) else {
        return;
    };

    // Постройка готова: юнит стоит на площадке.
    if unit_info.unit_type == *unit_type
        && unit_info.grid == *site
        && unit_info.orders != UnitOrder::Idle
    {
        *op_state = create_building_state::FINISHED;
        *building = Some(unit);

        ctx.tm.remove_unit_tasks(ctx.world, unit);

        if let Some(manager_id) = *manager {
            ctx.tm.task_child_complete(ctx.world, manager_id, header.id);
            ctx.tm.task_add_unit(ctx.world, manager_id, unit);
        }

        let is_platform = matches!(*unit_type, UnitType::WaterPlatform | UnitType::Bridge);

        if !is_platform {
            ctx.clear_unit_tasks(unit, false);
        }

        if let Some(parent) = header.parent {
            if Some(parent) != *manager {
                ctx.tm.task_child_complete(ctx.world, parent, header.id);
            }
        }

        *manager = None;
        *building = None;
        tasks.clear();

        create_building_finish(kind, ctx, header);

        return;
    }

    if unit_info.unit_type == builder_type_for(*unit_type) {
        if builder.is_none() {
            if *op_state < create_building_state::BUILDING {
                *op_state = create_building_state::GETTING_MATERIALS;
                *builder = Some(unit);
                ctx.attach(header.id, unit);
                ctx.remind_move_finished(unit, false);
            }
        }
    }
}

fn create_building_execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let TaskKind::CreateBuilding {
        unit_type,
        site,
        op_state,
        builder,
        manager,
        tasks,
        ..
    } = kind
    else {
        return false;
    };

    if *builder != Some(unit) || !ctx.ready_to_take_orders(unit) {
        return false;
    }

    if task_retreat_from_danger(
        ctx.tm,
        ctx.world,
        header.id,
        unit,
        CautionLevel::AvoidAllDamage,
    ) {
        return true;
    }

    match *op_state {
        create_building_state::MOVING_TO_SITE => {
            if !check_materials(ctx.world, header.team, *unit_type) {
                return true;
            }

            let unit_grid = ctx
                .world
                .unit(unit)
                .map(|unit| unit.grid)
                .unwrap_or(Point::ZERO);

            if unit_grid == *site {
                create_building_begin(kind, ctx, header, unit);
            } else {
                let is_connector = *unit_type == UnitType::Connector;
                let minimum_distance = if !tasks.is_empty() && !is_connector {
                    8u16
                } else {
                    0
                };

                *op_state = create_building_state::MOVING_TO_SITE;

                let dx = i32::from(unit_grid.x) - i32::from(site.x);
                let dy = i32::from(unit_grid.y) - i32::from(site.y);

                if dx * dx + dy * dy > i32::from(minimum_distance) {
                    let priority = ctx.priority_of(header.id);
                    let kind_move = TaskKind::move_to(
                        unit,
                        *site,
                        minimum_distance,
                        CautionLevel::AvoidAllDamage,
                    );
                    ctx.append_task(header.team, Some(header.id), priority, kind_move);
                }
            }

            true
        }
        create_building_state::GETTING_MATERIALS => {
            if !check_materials(ctx.world, header.team, *unit_type) {
                return true;
            }

            if manager.is_some() {
                if let Some(new_site) = find_build_site(ctx.world, header.team, *unit_type, *site) {
                    *site = new_site;
                }
            }

            *op_state = create_building_state::MOVING_TO_SITE;
            create_building_execute(kind, ctx, header, unit);

            true
        }
        create_building_state::MOVING_OFF_SITE | create_building_state::FINISHED => {
            let await_order = ctx
                .world
                .unit(unit)
                .map(|unit| unit.orders == UnitOrder::Await)
                .unwrap_or(true);

            if await_order {
                create_building_finish(kind, ctx, header);
            }

            true
        }
        _ => true,
    }
}

/// `TaskCreateBuilding::BeginBuilding`.
fn create_building_begin(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::CreateBuilding {
        unit_type,
        site,
        op_state,
        builder,
        manager,
        tasks,
        ..
    } = kind
    else {
        return;
    };

    if !tasks.is_empty() {
        return;
    }

    *op_state = create_building_state::BUILDING;

    let turns_left = task_estimate_turns_till_mission_end(ctx.world);
    let needed_turns = turns_to_build(ctx.world, header.team, *unit_type);

    if turns_left < needed_turns {
        create_building_abort(kind, ctx, header);
        return;
    }

    if !ctx.accessible(
        *unit_type,
        header.team,
        *site,
        crate::access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS,
    ) {
        create_building_abort(kind, ctx, header);
        return;
    }

    let parent_needed = header
        .parent
        .map(|parent| {
            ctx.tm
                .task(parent)
                .map(|task| task.is_needed(ctx.tm))
                .unwrap_or(false)
        })
        .unwrap_or(true);

    if !parent_needed {
        create_building_abort(kind, ctx, header);
        return;
    }

    if ctx.tm.unit_task(unit) != Some(header.id) {
        *op_state = create_building_state::MOVING_TO_SITE;
        return;
    }

    let dangerous = ctx
        .unit(unit)
        .map(|info| {
            crate::ai::ai_is_dangerous_location(
                ctx.world,
                &info,
                *site,
                CautionLevel::AvoidNextTurnsFire,
            )
        })
        .unwrap_or(false);

    if dangerous {
        task_retreat_from_danger(
            ctx.tm,
            ctx.world,
            header.id,
            unit,
            CautionLevel::AvoidNextTurnsFire,
        );
        *op_state = create_building_state::MOVING_TO_SITE;
        return;
    }

    if !check_materials(ctx.world, header.team, *unit_type) {
        return;
    }

    if !ctx.world.is_active_turn(header.team) {
        return;
    }

    let build_rate = if ctx.world.ai.settings.opponent >= crate::enums::Opponent::Average as u8
        && (*unit_type == UnitType::MiningStation
            || (ctx
                .world
                .unit(unit)
                .map(|info| {
                    let storage = info
                        .base_values
                        .get_attribute(crate::unit_values::ATTRIB_STORAGE);
                    i32::from(info.storage) >= storage - 2
                })
                .unwrap_or(false)
                && !need_to_reserve_raw_materials(ctx.world, header.team)))
    {
        2
    } else {
        1
    };

    ctx.build_order(unit, *unit_type, *site, build_rate);

    let _ = (manager, builder);
}

/// `TaskCreateBuilding::Finish`.
fn create_building_finish(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::CreateBuilding {
        op_state,
        tasks,
        builder,
        ..
    } = kind
    else {
        return;
    };

    *op_state = create_building_state::FINISHED;

    if let Some(builder_id) = *builder {
        ctx.clear_unit_tasks(builder_id, false);
    }

    for task in tasks.iter() {
        ctx.tm.task_remove_self(ctx.world, *task);
    }
    tasks.clear();

    *builder = None;

    if let Some(parent) = header.parent {
        ctx.tm.task_child_complete(ctx.world, parent, header.id);
    }

    ctx.set_parent(header.id, None);
    ctx.remove_self(header.id);
}

/// `TaskCreateBuilding::Abort`.
fn create_building_abort(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::CreateBuilding {
        op_state,
        tasks,
        builder,
        manager,
        ..
    } = kind
    else {
        return;
    };

    *op_state = create_building_state::FINISHED;

    if let Some(manager_id) = *manager {
        ctx.tm.task_child_complete(ctx.world, manager_id, header.id);
    }

    if let Some(parent) = header.parent {
        if Some(parent) != *manager {
            ctx.tm.task_child_complete(ctx.world, parent, header.id);
        }
    }

    ctx.set_parent(header.id, None);
    *manager = None;

    for task in tasks.iter() {
        ctx.tm.task_remove_self(ctx.world, *task);
    }
    tasks.clear();

    if let Some(builder_id) = *builder {
        ctx.clear_unit_tasks(builder_id, false);
    }
    *builder = None;

    let _ = op_state;
    ctx.remove_self(header.id);
}

/// `TaskCreateBuilding::Activate` (`taskcreatebuilding.cpp:912`) —
/// **уход строителя с площадки** через `TaskActivate` (`taskactivate.cpp:104`).
///
/// C++: `Activate` ставит `op_state = CREATE_BUILDING_STATE_MOVING_OFF_SITE` и
/// вешает `TaskActivate(builder, this, building)`; сама `TaskActivate::Activate`
/// ищет первую проходимую клетку вокруг здания (`Access_IsAccessible`) и отдаёт
/// строителю `ORDER_ACTIVATE`/`ORDER_STATE_IN_TRANSITION` (у игрока то же самое
/// делает клик, `game_manager.cpp:6879`). Задача завершается позже — в
/// `EndTurn`/`Execute` при `builder->GetOrder() == ORDER_AWAIT`
/// (`taskcreatebuilding.cpp:847-854`), то есть после того, как здание раскрылось
/// (`UnitsManager_BuildingReady` ждёт ухода строителя).
fn create_building_activate(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::CreateBuilding {
        unit_type,
        site,
        op_state,
        builder,
        manager,
        ..
    } = kind
    else {
        return;
    };

    let Some(builder_id) = *builder else {
        return;
    };

    // Строитель ещё на площадке (`ORDER_BUILD`/`UNIT_READY`, здание
    // `ORDER_IDLE`/`BUILDING_READY`) — отдаём приказ на уход один раз; дальше
    // ждём, пока он сойдёт с габаритов 2×2 (`TaskActivate_IsComplete`).
    if crate::production::builder_awaiting_exit(ctx.world, builder_id).is_some() {
        *op_state = create_building_state::MOVING_OFF_SITE;

        if crate::unit_order::issue_builder_exit(ctx.world, builder_id) {
            return;
        }

        // C++ просит очистить зону (`TaskActivate`: `AiPlayer_Teams[...].ClearZone`)
        // и повторяет попытку на следующем ходу — в порте зон нет (P5/P7).
        ctx.remind_turn_end(header.id, true);

        return;
    }

    let building_id = ctx.world.units_at(*site).iter().copied().find(|id| {
        ctx.world
            .unit(*id)
            .map(|unit| unit.unit_type == *unit_type && unit.team == header.team)
            .unwrap_or(false)
    });

    let _ = building_id;

    if let Some(manager_id) = *manager {
        ctx.tm.task_child_complete(ctx.world, manager_id, header.id);
    }

    if let Some(parent) = header.parent {
        ctx.tm.task_child_complete(ctx.world, parent, header.id);
    }

    ctx.clear_unit_tasks(builder_id, false);
    ctx.remove_self(header.id);
}

// ---------------------------------------------------------------------------
// TaskActivate
// ---------------------------------------------------------------------------

/// `TaskActivate::Activate` (`taskactivate.cpp`): `ORDER_ACTIVATE` контейнеру.
fn activate_run(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Activate {
        unit_to_activate,
        unit_parent,
        activate_issued,
        ..
    } = kind
    else {
        return;
    };

    let (Some(payload), Some(container)) = (*unit_to_activate, *unit_parent) else {
        return;
    };

    if !ctx.world.is_active_turn(header.team) {
        return;
    }

    if *activate_issued {
        return;
    }

    let payload_info = ctx.unit(payload);
    let container_info = ctx.unit(container);

    if let (Some(payload_info), Some(container_info)) = (payload_info, container_info) {
        if payload_info.grid == container_info.grid || payload_info.parent_unit == Some(container) {
            // `UnitsManager_SetNewOrder(container, ORDER_ACTIVATE, INIT)`.
            let mut info = container_info;
            info.parent_unit = Some(payload);
            ctx.write_unit(container, info);
            ctx.set_new_order(container, UnitOrder::Activate, UnitOrderState::Init);
            *activate_issued = true;
        }
    }
}

/// `TaskActivate_IsComplete` (`taskactivate.cpp`): груз выгружен на карту.
pub fn task_activate_is_complete(
    world: &crate::game::GameState,
    payload: UnitId,
    container: UnitId,
) -> bool {
    let Some(payload_info) = world.unit(payload) else {
        return true;
    };
    let Some(container_info) = world.unit(container) else {
        return true;
    };

    payload_info.grid != container_info.grid && payload_info.parent_unit != Some(container)
}

// ---------------------------------------------------------------------------
// Диспетчеризация жизненного цикла
// ---------------------------------------------------------------------------

/// `Init` для create-классов.
pub fn init(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::CreateUnit { builder, .. } => {
            if let Some(builder_id) = *builder {
                ctx.attach(header.id, builder_id);
            }
            // `TaskCreateUnit::Init`: `RemindTurnStart(true)`.
            ctx.remind_turn_start(header.id, true);
            true
        }
        TaskKind::CreateBuilding { .. } => {
            let builder = match kind {
                TaskKind::CreateBuilding { builder, .. } => *builder,
                _ => None,
            };

            if let Some(builder_id) = builder {
                ctx.attach(header.id, builder_id);

                let ready = ctx
                    .world
                    .unit(builder_id)
                    .map(|unit| unit.state == UnitOrderState::UnitReady)
                    .unwrap_or(false);

                if ready {
                    create_building_activate(kind, ctx, header);
                }
            } else {
                // `TaskCreateBuilding::Init`: без строителя — `RemindTurnStart(true)`.
                ctx.remind_turn_start(header.id, true);
            }

            true
        }
        TaskKind::ObtainUnits { .. } => {
            // `TaskObtainUnits::Init`: `RemindTurnEnd(true)` один раз.
            if let TaskKind::ObtainUnits {
                reinit,
                init: init_flag,
                ..
            } = kind
            {
                if *reinit {
                    ctx.remind_turn_end(header.id, true);
                    *init_flag = true;
                    *reinit = false;
                }
            }
            true
        }
        TaskKind::Activate {
            unit_to_activate,
            unit_parent,
            ..
        } => {
            if let Some(parent) = *unit_parent {
                ctx.attach(header.id, parent);
            }
            if unit_to_activate.is_some() && unit_parent.is_some() {
                ctx.remind_turn_start(header.id, true);
            } else {
                ctx.remove_self(header.id);
            }
            true
        }
        _ => false,
    }
}

/// `BeginTurn` для create-классов.
pub fn begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::CreateUnit { .. } => {
            create_unit_end_turn(kind, ctx, header);
            true
        }
        TaskKind::CreateBuilding { op_state, .. } => {
            if *op_state == create_building_state::SITE_BLOCKED {
                *op_state = create_building_state::EVALUATING_SITE;
            }

            let parent_needed = header
                .parent
                .map(|parent| {
                    ctx.tm
                        .task(parent)
                        .map(|task| task.is_needed(ctx.tm))
                        .unwrap_or(true)
                })
                .unwrap_or(true);

            if *op_state < create_building_state::BUILDING && !parent_needed {
                create_building_abort(kind, ctx, header);
            } else {
                create_building_end_turn(kind, ctx, header);
            }

            true
        }
        TaskKind::ObtainUnits { .. } => {
            if let TaskKind::ObtainUnits { reinit, .. } = kind {
                *reinit = true;
            }
            obtain_units_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Activate { .. } => {
            activate_run(kind, ctx, header);
            true
        }
        TaskKind::Escort { .. }
        | TaskKind::DefenseAssistant { .. }
        | TaskKind::PowerAssistant { .. } => false,
        _ => false,
    }
}

/// `EndTurn` для create-классов.
pub fn end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::CreateUnit { .. } => {
            create_unit_end_turn(kind, ctx, header);
            true
        }
        TaskKind::CreateBuilding { .. } => {
            create_building_end_turn(kind, ctx, header);
            true
        }
        TaskKind::ObtainUnits { .. } => {
            obtain_units_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Activate { .. } => {
            activate_run(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `Execute(unit)` для create-классов.
pub fn execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::CreateUnit { .. } => {
            let is_target_unit = ctx
                .world
                .unit(unit)
                .map(|info| {
                    matches!(kind, TaskKind::CreateUnit { unit_type, .. } if info.unit_type == *unit_type)
                })
                .unwrap_or(false);

            if is_target_unit {
                create_unit_add_unit(kind, ctx, header, unit);
            }

            let is_builder = matches!(kind, TaskKind::CreateUnit { builder: Some(builder), .. } if *builder == unit);

            if is_builder {
                begin_turn(kind, ctx, header);
            }

            false
        }
        TaskKind::CreateBuilding { builder, .. } => {
            let is_builder = *builder == Some(unit);
            if is_builder {
                create_building_execute(kind, ctx, header, unit)
            } else {
                false
            }
        }
        TaskKind::Activate {
            unit_to_activate, ..
        } => {
            if *unit_to_activate == Some(unit) {
                activate_run(kind, ctx, header);
            }
            false
        }
        _ => false,
    }
}

/// `AddUnit(unit)` для create-классов.
pub fn add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::CreateUnit { .. } => create_unit_add_unit(kind, ctx, header, unit),
        TaskKind::CreateBuilding { .. } => create_building_add_unit(kind, ctx, header, unit),
        TaskKind::ObtainUnits { units, .. } => {
            let Some(unit_info) = ctx.unit(unit) else {
                return;
            };

            if count_instances(units, unit_info.unit_type) > 0 {
                if let Some(index) = units
                    .iter()
                    .position(|candidate| *candidate == unit_info.unit_type)
                {
                    units.remove(index);
                }

                if let Some(parent) = header.parent {
                    ctx.tm.task_add_unit(ctx.world, parent, unit);
                }

                if units.is_empty() {
                    if let Some(parent) = header.parent {
                        ctx.tm.task_child_complete(ctx.world, parent, header.id);
                    }

                    ctx.set_parent(header.id, None);
                    ctx.remove_self(header.id);
                }
            }
        }
        TaskKind::Activate {
            unit_to_activate,
            unit_parent,
            ..
        } => {
            if *unit_to_activate == Some(unit) || *unit_parent == Some(unit) {
                activate_run(kind, ctx, header);
            }
        }
        _ => {}
    }
}

/// `RemoveUnit(unit)` для create-классов.
pub fn remove_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    let _ = header;
    match kind {
        TaskKind::CreateUnit { .. } => create_unit_remove_unit(kind, ctx, unit),
        TaskKind::CreateBuilding { builder, .. } => {
            if *builder == Some(unit) {
                *builder = None;
            }
        }
        TaskKind::ObtainUnits { .. } => {}
        TaskKind::Activate {
            unit_to_activate,
            unit_parent,
            ..
        } => {
            if *unit_to_activate == Some(unit) || *unit_parent == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        _ => {}
    }
}

/// `RemoveSelf()` для create-классов.
pub fn remove_self(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::CreateUnit { builder, .. } => {
            if let Some(builder_id) = *builder {
                ctx.clear_unit_tasks(builder_id, false);
            }
            *builder = None;
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::CreateBuilding { .. } => {
            // `TaskCreateBuilding::RemoveSelf` → `Abort()`.
            create_building_abort(kind, ctx, header);
            true
        }
        TaskKind::ObtainUnits { units, .. } => {
            units.clear();
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::Activate { .. } => {
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        _ => false,
    }
}

/// `ChildComplete(task)` для create-классов.
pub fn child_complete(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    child: u32,
) -> bool {
    match kind {
        TaskKind::CreateBuilding { tasks, .. } => {
            tasks.retain(|task| *task != child);
            true
        }
        TaskKind::CreateUnit { .. } => {
            let _ = (ctx, header, child);
            true
        }
        TaskKind::Activate { .. } => {
            let _ = (ctx, header, child);
            true
        }
        _ => false,
    }
}

/// Уведомление о завершении `TaskMove`.
pub fn move_finished(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
    result: i8,
) -> bool {
    match kind {
        TaskKind::CreateBuilding { builder, .. } => {
            if *builder == Some(unit) {
                create_building_execute(kind, ctx, header, unit);
            }
            let _ = result;
            true
        }
        TaskKind::CreateUnit { builder, .. } => {
            if *builder == Some(unit) {
                begin_turn(kind, ctx, header);
            }
            true
        }
        _ => false,
    }
}

/// `ExchangeOperator` для create-классов.
pub fn exchange_operator(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let TaskKind::CreateBuilding {
        builder,
        op_state,
        site,
        ..
    } = kind
    else {
        return false;
    };

    let Some(builder_id) = *builder else {
        return false;
    };

    let Some(builder_info) = ctx.unit(builder_id) else {
        return false;
    };
    let Some(candidate) = ctx.unit(unit) else {
        return false;
    };

    if builder_info.unit_type != candidate.unit_type || *op_state >= create_building_state::BUILDING
    {
        return false;
    }

    let builder_distance = crate::access::squared_distance(builder_info.grid, *site);
    let candidate_distance = crate::access::squared_distance(candidate.grid, *site);

    if builder_distance <= candidate_distance {
        return false;
    }

    ctx.clear_unit_tasks(builder_id, false);

    if builder.is_none() {
        create_building_add_unit(kind, ctx, header, unit);
    }

    true
}

/// `EventUnitDestroyed` для create-классов.
pub fn event_unit_destroyed(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::CreateBuilding { builder, .. } => {
            if *builder == Some(unit) {
                *builder = None;
            }
            let _ = (ctx, header);
            true
        }
        TaskKind::CreateUnit { builder, .. } => {
            if *builder == Some(unit) {
                *builder = None;
            }
            true
        }
        TaskKind::ObtainUnits { .. } | TaskKind::Activate { .. } => {
            if unit == UnitId::NO_ID {
                ctx.remove_self(header.id);
            }
            true
        }
        _ => false,
    }
}

/// `IsUnitUsable` для `TaskObtainUnits`.
pub fn obtain_units_is_unit_usable(
    world: &crate::game::GameState,
    _header: TaskHeader,
    kind: &TaskKind,
    unit: UnitId,
) -> bool {
    let TaskKind::ObtainUnits { units, .. } = kind else {
        return false;
    };

    let Some(info) = world.unit(unit) else {
        return false;
    };

    units.iter().any(|unit_type| *unit_type == info.unit_type)
}

/// `TaskObtainUnits` для тестов/менеджера.
pub fn obtain_units(kind: &TaskKind) -> Option<(&[UnitType], Point)> {
    match kind {
        TaskKind::ObtainUnits { units, point, .. } => Some((units, *point)),
        _ => None,
    }
}

/// `TaskObtainUnits::IsNeeded`.
pub fn is_needed(world: &crate::game::GameState, header: TaskHeader) -> bool {
    let Some(parent) = header.parent else {
        return false;
    };

    world
        .ai
        .task_manager
        .task(parent)
        .map(|task| task.is_needed(&world.ai.task_manager))
        .unwrap_or(false)
}

/// `TaskCreateUnit::GetUnitType` (`TaskCreate::GetUnitType`) — для переписи.
pub fn create_kind_type(kind: &TaskKind) -> Option<UnitType> {
    match kind {
        TaskKind::CreateUnit { unit_type, .. } | TaskKind::CreateBuilding { unit_type, .. } => {
            Some(*unit_type)
        }
        _ => None,
    }
}

/// Прочие обработчики, которые create-классы не переопределяют.
pub fn check_reactions(_kind: &mut TaskKind, _ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    false
}

/// `EventEnemyUnitSpotted` — не переопределён.
pub fn event_enemy_unit_spotted(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventCargoTransfer` — не переопределён.
pub fn event_cargo_transfer(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventUnitLoaded` — не переопределён.
pub fn event_unit_loaded(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit1: UnitId,
    _unit2: UnitId,
) -> bool {
    false
}

/// `EventUnitUnloaded` — не переопределён.
pub fn event_unit_unloaded(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit1: UnitId,
    _unit2: UnitId,
) -> bool {
    false
}

/// Неиспользуемый маркер, чтобы `TASK_TYPE_CREATE_UNIT`/`TASK_TYPE_OBTAIN_UNITS`
/// оставались видимыми в этом модуле (соответствие типам задач).
pub const CREATE_MODULE_TASK_TYPES: [u8; 2] = [TASK_TYPE_CREATE_UNIT, TASK_TYPE_OBTAIN_UNITS];
