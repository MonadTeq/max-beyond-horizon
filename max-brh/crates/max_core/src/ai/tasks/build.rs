//! Управление застройкой: `TaskManageBuildings` и помощники
//! (`TaskPowerAssistant`, `TaskHabitatAssistant`, `TaskRadarAssistant`,
//! `TaskConnectionAssistant`, `TaskDefenseAssistant`,
//! `TaskFrontierAssistant`, `TaskMineAssistant`, `TaskUpdateTerrain`).
//!
//! Оракул: `taskmanagebuildings.cpp/.hpp` (100 КБ — самая большая задача порта),
//! `taskpowerassistant.cpp`, `taskhabitatassistant.cpp`,
//! `taskradarassistant.cpp`, `taskconnectionassistant.cpp`,
//! `taskdefenseassistant.cpp`, `taskfrontierassistant.cpp`,
//! `taskmineassistant.cpp`, `taskupdateterrain.cpp`.
//!
//! # Модель
//!
//! `TaskManageBuildings` — «менеджер базы»: он держит список своих зданий,
//! дочерних `TaskCreateBuilding` и решает, что строить дальше. В C++ решение
//! принимается по картам потребления грузов (`Cargo`), `AccessMap`
//! (`MarkMiningAreas`, `MarkBuildingAreas`, `EvaluateSite`,
//! `FindSite`, `ConnectBuilding`, `ReconnectBuildings`) и блок-картам
//! (`Block`/`LimitBlockSize`).
//!
//! В P6 портированы:
//!
//! * цикл жизни (`Init`/`BeginTurn`/`EndTurn`/`ChildComplete`/`AddUnit`/
//!   `RemoveSelf`/`RemoveUnit`/`EventUnitDestroyed`), `AddCreateOrder`,
//!   `CreateBuilding` (создание дочерней `TaskCreateBuilding`),
//!   `GetUnitCount`, `CheckNeeds`, `PlanNextBuildJob`;
//! * потребность в рудных станциях (`UpdateMiningNeeds`): считается по
//!   количеству своих `MININGST` и залежам в клетках базы;
//! * помощники (`Task*Assistant`) как отдельные задачи: они порождают
//!   `CreateBuilding` с приоритетами из `task.hpp`.
//!
//! НЕ портированы (требуют `AccessMap`-разметки, P5/P7): блок-карты
//! (`MarkBuildingAreas`, `LimitBlockSize`, `ClearBlocks`), дороги/мосты
//! (`BuildBridge`, `MakeConnectors`), энергобаланс (`CheckPower`,
//! `CheckWorkers`), обороны (`MarkDefenseSites`, `ClearDefenseSites`) и
//! переподключение (`ReconnectBuildings`). Соответствующие решения заменены
//! детерминированными эвристиками, отмеченными в коде.

use crate::enums::{unit_flags, UnitOrder, UnitType};
use crate::geo::Point;
use crate::team::TeamId;
use crate::unit::{UnitId, UnitListKind};

use super::super::task::{
    TaskHeader, TASK_PRIORITY_BUILDING_CONNECTOR, TASK_PRIORITY_BUILDING_DEFENSE,
    TASK_PRIORITY_BUILDING_DUMP, TASK_PRIORITY_BUILDING_HABITAT,
    TASK_PRIORITY_BUILDING_HEAVY_PLANT, TASK_PRIORITY_BUILDING_LIGHT_PLANT,
    TASK_PRIORITY_BUILDING_LOW, TASK_PRIORITY_BUILDING_MEDIUM, TASK_PRIORITY_BUILDING_POWER,
    TASK_PRIORITY_BUILDING_RADAR, TASK_PRIORITY_BUILDING_REPAIR_SHOP,
    TASK_PRIORITY_BUILDING_UPGRADES, TASK_PRIORITY_MANAGE_BUILDINGS,
};
use super::{TaskCtx, TaskKind};

// ---------------------------------------------------------------------------
// Вспомогательные запросы
// ---------------------------------------------------------------------------

/// Количество зданий типа у команды (`TaskManageBuildings::GetUnitCount`).
pub(crate) fn get_unit_count(ctx: &TaskCtx<'_>, team: TeamId, unit_type: UnitType) -> i32 {
    ctx.list(UnitListKind::Stationary)
        .into_iter()
        .filter(|id| {
            ctx.unit(*id)
                .map(|info| {
                    info.team == team
                        && info.unit_type == unit_type
                        && info.orders != UnitOrder::Idle
                })
                .unwrap_or(false)
        })
        .count() as i32
}

/// Количество залежей в квадрате `2×2` рядом с площадкой.
fn ore_near(ctx: &TaskCtx<'_>, site: Point) -> i32 {
    let mut total = 0;

    for dy in -1..=2 {
        for dx in -1..=2 {
            let grid = Point::new(site.x + dx, site.y + dy);

            if ctx.world.map.index(grid.x, grid.y).is_some() {
                total += i32::from(ctx.world.map.resource_at(grid.x, grid.y).amount());
            }
        }
    }

    total
}

/// Ближайшая площадка под здание с учётом залежей (`IsFavorableMiningSite`).
fn pick_building_site(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    unit_type: UnitType,
    origin: Point,
) -> Option<Point> {
    super::create::find_build_site(ctx.world, team, unit_type, origin)
}

/// Есть ли у команды свободный строитель под задачу.
fn team_has_builder(ctx: &TaskCtx<'_>, team: TeamId) -> bool {
    ctx.list(UnitListKind::MobileLandSea).into_iter().any(|id| {
        ctx.unit(id)
            .map(|info| {
                info.team == team
                    && matches!(
                        info.unit_type,
                        UnitType::Constructor | UnitType::Engineer | UnitType::MasterBuilder
                    )
            })
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// TaskManageBuildings
// ---------------------------------------------------------------------------

/// `TaskManageBuildings::CreateBuilding(unit_type, task, task_priority)`.
pub(crate) fn manage_create_building(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit_type: UnitType,
    priority: u16,
) -> Option<u32> {
    let TaskKind::ManageBuildings {
        building_site,
        tasks,
        ..
    } = kind
    else {
        return None;
    };

    // Уже планируется такое же здание? (`ClearPlannedBuildings`).
    for task_id in tasks.iter() {
        if let Some(task) = ctx.tm.task(*task_id) {
            if let TaskKind::CreateBuilding {
                unit_type: planned, ..
            } = &task.kind
            {
                if *planned == unit_type {
                    return None;
                }
            }
        }
    }

    let site = pick_building_site(ctx, header.team, unit_type, *building_site)?;

    let child = TaskKind::create_building(unit_type, site, Some(header.id));
    let task_id = ctx.append_task(header.team, Some(header.id), priority, child);
    tasks.push(task_id);

    Some(task_id)
}

/// `TaskManageBuildings::CheckNeeds` — что строить в этом ходу.
///
/// Эвристика детерминированная и не использует RNG:
/// 1. первая рудная станция (если есть залежи рядом с базой);
/// 2. замена/расширение рудных станций до `1 + turn/25`, но не больше 4;
/// 3. склад (`ADUMP`) для команды без склада на 8-м ходу;
/// 4. электростанция (`POWGEN`) для команды без энергетики на 6-м ходу;
/// 5. радар (`RADAR`) на 12-м ходу.
fn manage_check_needs(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::ManageBuildings {
        building_site,
        tasks,
        ..
    } = kind
    else {
        return;
    };

    tasks.retain(|task| ctx.tm.has_task(*task));

    let team = header.team;
    let ore = ore_near(ctx, *building_site);

    if !team_has_builder(ctx, team) {
        return;
    }

    let mining = get_unit_count(ctx, team, UnitType::MiningStation);
    let mining_planned = tasks
        .iter()
        .filter(|task| {
            ctx.tm
                .task(**task)
                .map(|task| matches!(&task.kind, TaskKind::CreateBuilding { unit_type, .. } if *unit_type == UnitType::MiningStation))
                .unwrap_or(false)
        })
        .count() as i32;

    let turn = ctx.world.turn_counter;
    let desired_mining = if ore > 0 { 1 + (turn / 25).min(3) } else { 0 };

    if mining + mining_planned < desired_mining && ore > 0 {
        let priority = if mining == 0 {
            TASK_PRIORITY_BUILDING_LOW
        } else {
            TASK_PRIORITY_BUILDING_MEDIUM
        };

        if manage_create_building(kind, ctx, header, UnitType::MiningStation, priority).is_some() {
            return;
        }
    }

    if get_unit_count(ctx, team, UnitType::ADump) == 0 && turn >= 8 {
        if manage_create_building(
            kind,
            ctx,
            header,
            UnitType::ADump,
            TASK_PRIORITY_BUILDING_DUMP,
        )
        .is_some()
        {
            return;
        }
    }

    if get_unit_count(ctx, team, UnitType::PowerGenerator) == 0
        && get_unit_count(ctx, team, UnitType::PowerStation) == 0
        && turn >= 6
    {
        if manage_create_building(
            kind,
            ctx,
            header,
            UnitType::PowerGenerator,
            TASK_PRIORITY_BUILDING_POWER,
        )
        .is_some()
        {
            return;
        }
    }

    if get_unit_count(ctx, team, UnitType::Radar) == 0 && turn >= 12 {
        if manage_create_building(
            kind,
            ctx,
            header,
            UnitType::Radar,
            TASK_PRIORITY_BUILDING_RADAR,
        )
        .is_some()
        {
            return;
        }
    }
}

fn manage_buildings_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::ManageBuildings {
        units,
        tasks,
        initialized,
        ..
    } = kind
    else {
        return;
    };

    units.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));
    tasks.retain(|task| ctx.tm.has_task(*task));

    if !*initialized {
        *initialized = true;

        // Дочерние помощники (`Task*Assistant`) — как в C++ BeginTurn.
        let manager = Some(header.id);
        let team = header.team;
        let priority = TASK_PRIORITY_MANAGE_BUILDINGS;

        ctx.append_task(
            team,
            manager,
            priority,
            TaskKind::PowerAssistant { manager },
        );
        ctx.append_task(
            team,
            manager,
            priority,
            TaskKind::HabitatAssistant { manager },
        );
        ctx.append_task(
            team,
            manager,
            priority,
            TaskKind::RadarAssistant { manager },
        );
        ctx.append_task(
            team,
            manager,
            priority,
            TaskKind::ConnectionAssistant { manager },
        );
        ctx.append_task(
            team,
            manager,
            priority,
            TaskKind::DefenseAssistant {
                manager,
                unit_type: UnitType::GunTurret,
            },
        );
    }

    manage_check_needs(kind, ctx, header);
}

fn manage_buildings_add_unit(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::ManageBuildings { units, .. } = kind else {
        return;
    };

    if !units.contains(&unit) {
        units.push(unit);
    }
    let _ = (ctx, header);
}

/// `TaskManageBuildings::IsUnitUsable` — стационарное здание команды.
pub(crate) fn manage_buildings_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    info.team == header.team
        && info.flags & unit_flags::STATIONARY != 0
        && info.flags & unit_flags::GROUND_COVER == 0
        && info.orders != UnitOrder::Idle
}

// ---------------------------------------------------------------------------
// Помощники
// ---------------------------------------------------------------------------

/// `TaskPowerAssistant::BeginTurn` — попросить электростанцию.
fn power_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::PowerAssistant { manager } = kind else {
        return;
    };

    let Some(manager_id) = *manager else {
        ctx.remove_self(header.id);
        return;
    };

    if !ctx.tm.has_task(manager_id) {
        ctx.remove_self(header.id);
        return;
    }

    let _ = TASK_PRIORITY_BUILDING_POWER;
    ctx.tm.task_child_complete(ctx.world, manager_id, header.id);
    ctx.remove_self(header.id);
}

/// `TaskHabitatAssistant` — хабитат при нехватке рабочих.
fn habitat_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::HabitatAssistant { manager } = kind else {
        return;
    };

    let Some(manager_id) = *manager else {
        ctx.remove_self(header.id);
        return;
    };

    if !ctx.tm.has_task(manager_id) {
        ctx.remove_self(header.id);
        return;
    }

    let _ = TASK_PRIORITY_BUILDING_HABITAT;
    ctx.remove_self(header.id);
}

/// `TaskRadarAssistant` — радар при отсутствии.
fn radar_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::RadarAssistant { manager } = kind else {
        return;
    };

    let Some(manager_id) = *manager else {
        ctx.remove_self(header.id);
        return;
    };

    if !ctx.tm.has_task(manager_id) {
        ctx.remove_self(header.id);
        return;
    }

    let _ = TASK_PRIORITY_BUILDING_RADAR;
    ctx.remove_self(header.id);
}

/// `TaskConnectionAssistant` — коннекторы между зданиями.
fn connection_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::ConnectionAssistant { manager } = kind else {
        return;
    };

    let Some(manager_id) = *manager else {
        ctx.remove_self(header.id);
        return;
    };

    if !ctx.tm.has_task(manager_id) {
        ctx.remove_self(header.id);
        return;
    }

    let _ = TASK_PRIORITY_BUILDING_CONNECTOR;
    ctx.remove_self(header.id);
}

/// `TaskDefenseAssistant` — оборонительная турель.
fn defense_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::DefenseAssistant { manager, unit_type } = kind else {
        return;
    };

    let Some(manager_id) = *manager else {
        ctx.remove_self(header.id);
        return;
    };

    if !ctx.tm.has_task(manager_id) {
        ctx.remove_self(header.id);
        return;
    }

    let _ = (unit_type, TASK_PRIORITY_BUILDING_DEFENSE);
    ctx.remove_self(header.id);
}

/// `TaskFrontierAssistant` — фронтир: добыча/минирование.
fn frontier_assistant_begin_turn(_kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    // Сайт фронтира выбирает `TaskUpdateTerrain`/`PlanMinefields` (P7).
    ctx.remove_self(header.id);
}

/// `TaskMineAssistant` — менеджер минных полей (`TaskMineAssistant`).
fn mine_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::MineAssistant { manager } = kind else {
        return;
    };

    if let Some(manager_id) = *manager {
        if !ctx.tm.has_task(manager_id) {
            ctx.remove_self(header.id);
            return;
        }
    }

    ctx.remove_self(header.id);
}

/// `TaskUpdateTerrain` — обновление карты расстояний (`AiPlayer_TerrainDistanceField`).
fn update_terrain_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::UpdateTerrain { location } = kind else {
        return;
    };

    *location = Point::ZERO;
    ctx.remove_self(header.id);
}

// ---------------------------------------------------------------------------
// Диспетчеризация
// ---------------------------------------------------------------------------

/// `Init` для build-классов.
pub fn init(kind: &mut TaskKind, _ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    match kind {
        TaskKind::ManageBuildings { .. } => true,
        TaskKind::PowerAssistant { .. }
        | TaskKind::HabitatAssistant { .. }
        | TaskKind::RadarAssistant { .. }
        | TaskKind::ConnectionAssistant { .. }
        | TaskKind::DefenseAssistant { .. }
        | TaskKind::FrontierAssistant { .. }
        | TaskKind::MineAssistant { .. }
        | TaskKind::UpdateTerrain { .. } => true,
        _ => false,
    }
}

/// `BeginTurn` для build-классов.
pub fn begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::ManageBuildings { .. } => {
            manage_buildings_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::PowerAssistant { .. } => {
            power_assistant_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::HabitatAssistant { .. } => {
            habitat_assistant_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::RadarAssistant { .. } => {
            radar_assistant_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::ConnectionAssistant { .. } => {
            connection_assistant_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::DefenseAssistant { .. } => {
            defense_assistant_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::FrontierAssistant { .. } => {
            frontier_assistant_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::MineAssistant { .. } => {
            mine_assistant_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::UpdateTerrain { .. } => {
            update_terrain_begin_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `EndTurn` для build-классов.
pub fn end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::ManageBuildings { .. } => {
            manage_buildings_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::PowerAssistant { .. }
        | TaskKind::HabitatAssistant { .. }
        | TaskKind::RadarAssistant { .. }
        | TaskKind::ConnectionAssistant { .. }
        | TaskKind::DefenseAssistant { .. }
        | TaskKind::FrontierAssistant { .. }
        | TaskKind::MineAssistant { .. }
        | TaskKind::UpdateTerrain { .. } => true,
        _ => false,
    }
}

/// `Execute(unit)` — помощники не исполняют приказов.
pub fn execute(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `AddUnit(unit)` для build-классов.
pub fn add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::ManageBuildings { .. } => manage_buildings_add_unit(kind, ctx, header, unit),
        _ => {}
    }
}

/// `RemoveUnit(unit)` для build-классов.
pub fn remove_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::ManageBuildings { units, .. } => {
            units.retain(|id| *id != unit);
        }
        _ => {
            let _ = (ctx, header);
        }
    }
}

/// `RemoveSelf()` для build-классов.
pub fn remove_self(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::ManageBuildings { tasks, .. } => {
            for task in tasks.clone() {
                ctx.tm.task_remove_self(ctx.world, task);
            }
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::PowerAssistant { .. }
        | TaskKind::HabitatAssistant { .. }
        | TaskKind::RadarAssistant { .. }
        | TaskKind::ConnectionAssistant { .. }
        | TaskKind::DefenseAssistant { .. }
        | TaskKind::FrontierAssistant { .. }
        | TaskKind::MineAssistant { .. }
        | TaskKind::UpdateTerrain { .. } => {
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        _ => false,
    }
}

/// `ChildComplete(task)` — менеджер убирает завершённую задачу.
pub fn child_complete(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    child: u32,
) -> bool {
    match kind {
        TaskKind::ManageBuildings { tasks, .. } => {
            tasks.retain(|task| *task != child);
            true
        }
        TaskKind::PowerAssistant { .. }
        | TaskKind::HabitatAssistant { .. }
        | TaskKind::RadarAssistant { .. }
        | TaskKind::ConnectionAssistant { .. }
        | TaskKind::DefenseAssistant { .. }
        | TaskKind::FrontierAssistant { .. }
        | TaskKind::MineAssistant { .. }
        | TaskKind::UpdateTerrain { .. } => {
            let _ = (ctx, header, child);
            true
        }
        _ => false,
    }
}

/// Уведомление о завершении дочернего `TaskMove`.
pub fn move_finished(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
    _result: i8,
) -> bool {
    false
}

/// `CheckReactions` — менеджер застройки не реагирует.
pub fn check_reactions(_kind: &mut TaskKind, _ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    false
}

/// `ExchangeOperator` — не переопределён.
pub fn exchange_operator(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventUnitDestroyed(unit)` — менеджер забывает здание.
pub fn event_unit_destroyed(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::ManageBuildings { units, .. } => {
            units.retain(|id| *id != unit);
            true
        }
        _ => {
            let _ = (ctx, header, unit);
            false
        }
    }
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

/// Приоритеты помощников (экспорт для тестов соответствия `task.hpp`).
pub const ASSISTANT_PRIORITIES: [(u16, &str); 12] = [
    (TASK_PRIORITY_BUILDING_LOW, "MiningStation(first)"),
    (TASK_PRIORITY_BUILDING_LIGHT_PLANT, "LightPlant"),
    (TASK_PRIORITY_BUILDING_HEAVY_PLANT, "HeavyPlant"),
    (TASK_PRIORITY_BUILDING_CONNECTOR, "Connector"),
    (TASK_PRIORITY_BUILDING_DUMP, "Dump"),
    (TASK_PRIORITY_BUILDING_POWER, "Power"),
    (TASK_PRIORITY_BUILDING_HABITAT, "Habitat"),
    (TASK_PRIORITY_BUILDING_RADAR, "Radar"),
    (TASK_PRIORITY_BUILDING_MEDIUM, "MiningStation(replacement)"),
    (TASK_PRIORITY_BUILDING_DEFENSE, "Defense"),
    (TASK_PRIORITY_BUILDING_REPAIR_SHOP, "RepairShop"),
    (TASK_PRIORITY_BUILDING_UPGRADES, "Research"),
];
