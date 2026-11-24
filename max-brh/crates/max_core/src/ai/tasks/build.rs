//! Construction management: `TaskManageBuildings` and helpers
//! (`TaskPowerAssistant`, `TaskHabitatAssistant`, `TaskRadarAssistant`,
//! `TaskConnectionAssistant`, `TaskDefenseAssistant`,
//! `TaskFrontierAssistant`, `TaskMineAssistant`, `TaskUpdateTerrain`).
//!
//! Oracle: `taskmanagebuildings.cpp/.hpp`, `taskpowerassistant.cpp`,
//! `taskhabitatassistant.cpp`, `taskradarassistant.cpp`,
//! `taskconnectionassistant.cpp`, `taskdefenseassistant.cpp`,
//! `taskfrontierassistant.cpp`, `taskmineassistant.cpp`, `taskupdateterrain.cpp`,
//! `cargo.cpp`.
//!
//! # What the AI builds
//!
//! `TaskManageBuildings` is the "base manager": it keeps the buildings of a base
//! (`units`), its child `TaskCreateBuilding`s (`tasks`) and decides what to build
//! next.
//!
//! Ported decision logic:
//!
//! * `GetUnitCount` (units + planned tasks at or below a priority),
//!   `CreateBuildings` (a demand counted against the census), `CreateBuilding`
//!   (buildability, mission time, builder census, one plan per type, site
//!   search, child `TaskCreateBuilding`);
//! * `PlanNextBuildJob`: light plant → heavy plant → air plant / habitat /
//!   greenhouse / research, then strategy-driven factory expansion
//!   (`DEFENSIVE`/`SCOUT_HORDE` → light plants, `TANK_HORDE`/`COMBINED_ARMS` →
//!   heavy plants, `AIR` → air plants, `SEA` → shipyards, `ESPIONAGE` →
//!   training halls), then repair shops (`DEPOT`/`HANGAR`/`DOCK`/`BARRACKS`),
//!   their capacity upkeep and the heavy-plant → shipyard step;
//! * `UpdateMiningNeeds`: the raw/fuel/gold demand computed from the cargo of
//!   builders and the consumption of buildings/plans (`cargo.cpp` rates), the
//!   mining-station capacity of own and planned mines, the communications tower
//!   call, and the `MININGST` decision of `CheckNeeds`;
//! * `EndTurn`: storage upkeep — `ADUMP`/`FDUMP`/`GOLDSM` when a mine fills the
//!   storage (needs `Survey_GetTotalResourcesInArea` for planned mines);
//! * `CheckPower`/`CheckWorkers` (driven by the power/habitat assistants) and
//!   the radar assistant's "is there still time" guard;
//! * unit production on factories: in C++ the factory branch lives in
//!   `TaskAttackReserve::AddUnit` (`taskattackreserve.cpp:136-164`); the reserve
//!   life cycle is phase A.4, so until then the same decision (filtered weight
//!   table by factory type → `RollUnitType` → `TaskObtainUnits`) is performed by
//!   the manager each turn (`plan_unit_production`).
//!
//! NOT ported (they require the `AccessMap` marking, P5/P7): block maps
//! (`MarkBuildingAreas`, `LimitBlockSize`, `ClearBlocks`), roads/bridges
//! (`BuildBridge`, `MakeConnectors`), `FindSiteForRadar`/`EvaluateNeedForRadar`
//! (radar need is simplified to "the team has no radar") and the defense-site
//! search (`FindDefenseSite`); sites come from
//! [`super::create::find_build_site`].

use crate::enums::{UnitOrder, UnitType, UNIT_END};
use crate::geo::Point;
use crate::team::{Cargo, TeamId};
use crate::unit::{UnitId, UnitListKind};
use crate::unit_values::{ATTRIB_STORAGE, ATTRIB_TURNS};
use crate::victory::{
    cargo_fuel_consumption_rate, cargo_gold_consumption_rate, cargo_life_consumption_rate,
    cargo_power_consumption_rate, cargo_raw_consumption_rate, cargo_type_of, CargoType,
};

use super::super::build_orders::is_buildable;
use super::super::task::{
    task_estimate_turns_till_mission_end, TaskHeader, TASK_PRIORITY_BUILDING_CONNECTOR,
    TASK_PRIORITY_BUILDING_DEFENSE, TASK_PRIORITY_BUILDING_DUMP, TASK_PRIORITY_BUILDING_EXPANSION,
    TASK_PRIORITY_BUILDING_GREENHOUSE, TASK_PRIORITY_BUILDING_HABITAT,
    TASK_PRIORITY_BUILDING_HEAVY_PLANT, TASK_PRIORITY_BUILDING_LIGHT_PLANT,
    TASK_PRIORITY_BUILDING_LOW, TASK_PRIORITY_BUILDING_MEDIUM, TASK_PRIORITY_BUILDING_POWER,
    TASK_PRIORITY_BUILDING_RADAR, TASK_PRIORITY_BUILDING_REPAIR_SHOP,
    TASK_PRIORITY_BUILDING_UPGRADES, TASK_PRIORITY_MANAGE_BUILDINGS,
    TASK_PRIORITY_OFFSET_CENSUS_SLACK,
};
use super::{TaskCtx, TaskKind};

// ---------------------------------------------------------------------------
// Manager state accessors
// ---------------------------------------------------------------------------

/// The buildings assigned to the manager (C++ `units`).
fn manager_units(kind: &TaskKind) -> Vec<UnitId> {
    match kind {
        TaskKind::ManageBuildings { units, .. } => units.clone(),
        _ => Vec::new(),
    }
}

/// The child construction tasks of the manager (C++ `tasks`).
fn manager_tasks(kind: &TaskKind) -> Vec<u32> {
    match kind {
        TaskKind::ManageBuildings { tasks, .. } => tasks.clone(),
        _ => Vec::new(),
    }
}

/// The manager's base site (`building_site`).
fn manager_site(kind: &TaskKind) -> Option<Point> {
    match kind {
        TaskKind::ManageBuildings { building_site, .. } => Some(*building_site),
        _ => None,
    }
}

/// Writes the manager state back into the task (for the assistant tasks).
fn write_manager_kind(ctx: &mut TaskCtx<'_>, manager: u32, kind: TaskKind) {
    if let Some(task) = ctx.tm.task_mut(manager) {
        task.kind = kind;
    }
}

/// Reads the manager state out of the task (for the assistant tasks).
fn read_manager_kind(ctx: &TaskCtx<'_>, manager: u32) -> Option<(TaskKind, TaskHeader)> {
    ctx.tm
        .task(manager)
        .map(|task| (task.kind.clone(), task.header()))
}

// ---------------------------------------------------------------------------
// TaskManageBuildings — census and construction orders
// ---------------------------------------------------------------------------

/// `TaskManageBuildings::GetUnitCount(unit_type, task_priority)`
/// (`taskmanagebuildings.cpp:926`).
///
/// Buildings assigned to the manager plus planned tasks of the type that are
/// actively building or whose priority is at most `task_priority`.
fn get_unit_count(
    kind: &TaskKind,
    ctx: &TaskCtx<'_>,
    unit_type: UnitType,
    task_priority: u16,
) -> i32 {
    let mut unit_count = 0;

    for unit_id in manager_units(kind) {
        if ctx
            .unit(unit_id)
            .map(|unit| unit.unit_type == unit_type)
            .unwrap_or(false)
        {
            unit_count += 1;
        }
    }

    for task_id in manager_tasks(kind) {
        let Some(task) = ctx.tm.task(task_id) else {
            continue;
        };

        if task.kind.create_unit_type() == Some(unit_type)
            && (task.kind.is_actively_building()
                || task.compare_priority(ctx.world, task_priority) <= 0)
        {
            unit_count += 1;
        }
    }

    unit_count
}

/// `TaskManageBuildings::CreateBuilding(unit_type, task, task_priority)`
/// (`taskmanagebuildings.cpp:2360`).
///
/// Checks, in C++ order: buildability, remaining mission time, the builder
/// census (`builder_count * 2 + 1 > planned units`) and the site. The port adds
/// one planned task per type — the stand-in for `ClearPlannedBuildings`, which
/// otherwise keeps `find_build_site` from selecting the same site twice.
pub(crate) fn manage_create_building(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit_type: UnitType,
    priority: u16,
) -> Option<u32> {
    let site_base = manager_site(kind)?;
    let team = header.team;

    if !is_buildable(unit_type) {
        return None;
    }

    let turns = ctx.world.team_units[team.0 as usize]
        .current_unit_values(unit_type as u16)
        .get_attribute(ATTRIB_TURNS);

    if task_estimate_turns_till_mission_end(ctx.world) < turns {
        return None;
    }

    // Builder census: the planned units this builder may produce.
    let mut unit_counters = vec![0i32; UNIT_END as usize];

    for task_id in manager_tasks(kind) {
        let Some(task) = ctx.tm.task(task_id) else {
            continue;
        };

        if task.compare_priority(
            ctx.world,
            priority.saturating_add(TASK_PRIORITY_OFFSET_CENSUS_SLACK),
        ) <= 0
        {
            if let Some(planned) = task.kind.create_unit_type() {
                unit_counters[planned as usize] += 1;
            }
        }
    }

    let builder_type = crate::production::builder_type(unit_type)?;

    let mut builder_count = 0i64;
    for unit_id in ctx.list(UnitListKind::MobileLandSea) {
        if ctx
            .unit(unit_id)
            .map(|unit| unit.team == team && unit.unit_type == builder_type)
            .unwrap_or(false)
        {
            builder_count += 1;
        }
    }

    let unit_count: i64 = crate::production::buildable_units(builder_type)
        .iter()
        .map(|candidate| i64::from(unit_counters[*candidate as usize]))
        .sum();

    if builder_count * 2 + 1 <= unit_count {
        return None;
    }

    // One planned task per type (the C++ census is priority based; the port keeps
    // one plan per type as the `ClearPlannedBuildings` stand-in).
    for task_id in manager_tasks(kind) {
        if let Some(task) = ctx.tm.task(task_id) {
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

    // The site skips the footprints of the base's other plans
    // (`TaskManageBuildings::ClearPlannedBuildings`, `taskmanagebuildings.cpp:352`)
    // — two plans of one turn must not share cells.
    let planned = super::create::planned_build_footprints(ctx.tm, team, None);
    let site = super::create::find_build_site_for_task(
        ctx.world,
        team,
        unit_type,
        site_base,
        &planned,
        &[],
    )?;

    let child = TaskKind::create_building(unit_type, site, Some(header.id));
    let task_id = ctx.append_task(team, Some(header.id), priority, child);

    if let TaskKind::ManageBuildings { tasks, .. } = kind {
        tasks.push(task_id);
    }

    Some(task_id)
}

/// `TaskManageBuildings::CreateBuildings` (`taskmanagebuildings.cpp:997`).
fn create_buildings(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    building_demand: i32,
    unit_type: UnitType,
    priority: u16,
) -> bool {
    if !is_buildable(unit_type) {
        return false;
    }

    let demand = building_demand - get_unit_count(kind, ctx, unit_type, priority);

    for _ in 0..demand.max(0) {
        if manage_create_building(kind, ctx, header, unit_type, priority).is_some() {
            return true;
        }
    }

    false
}

/// `TaskManageBuildings::IsSupremeTeam` (`taskmanagebuildings.cpp:945`).
///
/// The C++ loop compares the team with **every** active team including itself,
/// so a team with points always fails the `team_points <= other_points` check.
/// The behavior is kept as is (a 1:1 port, not a wish).
fn is_supreme_team(world: &crate::game::GameState, team: TeamId) -> bool {
    let team_points = i64::from(world.teams[team.0 as usize].team_points);

    if team_points <= 0 {
        return false;
    }

    for other in 0..crate::game::PLAYER_TEAM_COUNT {
        if world.teams[other].team_type != crate::team::TeamKind::None
            && team_points <= i64::from(world.teams[other].team_points)
        {
            return false;
        }
    }

    true
}

/// `TaskManageBuildings::GetHighestGreenHouseCount` (`taskmanagebuildings.cpp:967`).
fn get_highest_greenhouse_count(world: &crate::game::GameState, team: TeamId) -> i32 {
    // `PLAYER_TEAM_MAX` slots: ALIEN buildings are counted (C++ indexes the
    // array by team) but never considered for the highest opponent count.
    let mut counts = [0i32; crate::game::TEAM_SLOTS];

    for unit_id in world.unit_list(UnitListKind::Stationary) {
        if let Some(unit) = world.unit(*unit_id) {
            if unit.unit_type == UnitType::EcoSphere {
                counts[unit.team.0 as usize] += 1;
            }
        }
    }

    for unit_id in world.unit_list(UnitListKind::MobileLandSea) {
        if let Some(unit) = world.unit(*unit_id) {
            if unit.unit_type == UnitType::Constructor
                && unit.orders == UnitOrder::Build
                && unit.state == crate::enums::UnitOrderState::BuildInProgress
                && unit.build_list.first().copied() == Some(UnitType::EcoSphere)
            {
                counts[unit.team.0 as usize] += 1;
            }
        }
    }

    let mut highest = 0;
    for (index, count) in counts
        .iter()
        .enumerate()
        .take(crate::game::PLAYER_TEAM_COUNT)
    {
        if index != team.0 as usize {
            highest = highest.max(*count);
        }
    }

    highest
}

/// `TaskManageBuildings::PlanNextBuildJob` (`taskmanagebuildings.cpp:1011`).
///
/// The strategy now drives which factories and support buildings the AI adds;
/// the turn heuristic (mine/storage/generator/radar on fixed turns) is gone.
fn plan_next_build_job(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    if manager_units(kind).is_empty() {
        return false;
    }

    let team = header.team;

    // `LIGHTPLT`: the first factory of every base.
    if get_unit_count(
        kind,
        ctx,
        UnitType::LightVehiclePlant,
        TASK_PRIORITY_BUILDING_LIGHT_PLANT,
    ) == 0
        && manage_create_building(
            kind,
            ctx,
            header,
            UnitType::LightVehiclePlant,
            TASK_PRIORITY_BUILDING_LIGHT_PLANT,
        )
        .is_some()
    {
        return true;
    }

    // `LANDPLT`: the second one.
    if get_unit_count(
        kind,
        ctx,
        UnitType::HeavyVehiclePlant,
        TASK_PRIORITY_BUILDING_HEAVY_PLANT,
    ) == 0
        && manage_create_building(
            kind,
            ctx,
            header,
            UnitType::HeavyVehiclePlant,
            TASK_PRIORITY_BUILDING_HEAVY_PLANT,
        )
        .is_some()
    {
        return true;
    }

    // Both plants exist: the support block.
    {
        let unit_count_lightplant = get_unit_count(kind, ctx, UnitType::LightVehiclePlant, 0x00);
        let unit_count_landplant = get_unit_count(kind, ctx, UnitType::HeavyVehiclePlant, 0x00);
        let unit_count_airplant = get_unit_count(kind, ctx, UnitType::AirUnitsPlant, 0x00);
        let unit_count_shipyard = get_unit_count(kind, ctx, UnitType::Shipyard, 0x00);
        let unit_count_trainhall = get_unit_count(kind, ctx, UnitType::TrainingHall, 0x00);
        let total_unit_count = unit_count_lightplant
            + unit_count_landplant
            + unit_count_airplant
            + unit_count_shipyard
            + unit_count_trainhall;

        if unit_count_lightplant > 0 && unit_count_landplant > 0 {
            if get_unit_count(
                kind,
                ctx,
                UnitType::AirUnitsPlant,
                TASK_PRIORITY_BUILDING_HEAVY_PLANT,
            ) == 0
                && manage_create_building(
                    kind,
                    ctx,
                    header,
                    UnitType::AirUnitsPlant,
                    TASK_PRIORITY_BUILDING_HEAVY_PLANT,
                )
                .is_some()
            {
                return true;
            }

            if get_unit_count(kind, ctx, UnitType::Habitat, TASK_PRIORITY_BUILDING_HABITAT) == 0
                && manage_create_building(
                    kind,
                    ctx,
                    header,
                    UnitType::Habitat,
                    TASK_PRIORITY_BUILDING_HABITAT,
                )
                .is_some()
            {
                return true;
            }

            if get_unit_count(
                kind,
                ctx,
                UnitType::EcoSphere,
                TASK_PRIORITY_BUILDING_GREENHOUSE,
            ) == 0
                && manage_create_building(
                    kind,
                    ctx,
                    header,
                    UnitType::EcoSphere,
                    TASK_PRIORITY_BUILDING_GREENHOUSE,
                )
                .is_some()
            {
                return true;
            }

            if get_unit_count(
                kind,
                ctx,
                UnitType::ResearchCentre,
                TASK_PRIORITY_BUILDING_UPGRADES,
            ) == 0
                && manage_create_building(
                    kind,
                    ctx,
                    header,
                    UnitType::ResearchCentre,
                    TASK_PRIORITY_BUILDING_UPGRADES,
                )
                .is_some()
            {
                return true;
            }

            if unit_count_airplant > 0 {
                // `GetGreenhouseRatio()` is rolled in `Init` before the first
                // turn; the `max(0)` guard only covers a not-yet-initialized
                // player (-1), where C++ would wrap the unsigned ratio.
                let greenhouse_ratio = i32::from(
                    ctx.world.ai.players[team.0 as usize]
                        .greenhouse_ratio
                        .max(0),
                );
                let greenhouse_demand = (greenhouse_ratio * total_unit_count) / 10;

                if create_buildings(
                    kind,
                    ctx,
                    header,
                    greenhouse_demand,
                    UnitType::EcoSphere,
                    TASK_PRIORITY_BUILDING_GREENHOUSE,
                ) {
                    return true;
                }

                let research_demand = get_unit_count(
                    kind,
                    ctx,
                    UnitType::EcoSphere,
                    TASK_PRIORITY_BUILDING_UPGRADES,
                );

                if create_buildings(
                    kind,
                    ctx,
                    header,
                    research_demand,
                    UnitType::ResearchCentre,
                    TASK_PRIORITY_BUILDING_UPGRADES,
                ) {
                    return true;
                }

                if is_supreme_team(ctx.world, team) {
                    let demand = get_highest_greenhouse_count(ctx.world, team) + 1;

                    if create_buildings(
                        kind,
                        ctx,
                        header,
                        demand,
                        UnitType::EcoSphere,
                        TASK_PRIORITY_BUILDING_GREENHOUSE,
                    ) {
                        return true;
                    }
                }
            }
        }

        let total_unit_count = total_unit_count / 3;

        match ctx.world.ai.players[team.0 as usize].strategy {
            crate::enums::AiStrategy::Defensive => {
                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count,
                    UnitType::LightVehiclePlant,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }
            }

            crate::enums::AiStrategy::Missiles => {}

            crate::enums::AiStrategy::Air => {
                if get_unit_count(
                    kind,
                    ctx,
                    UnitType::AirUnitsPlant,
                    TASK_PRIORITY_BUILDING_HEAVY_PLANT,
                ) == 0
                    && manage_create_building(
                        kind,
                        ctx,
                        header,
                        UnitType::AirUnitsPlant,
                        TASK_PRIORITY_BUILDING_HEAVY_PLANT,
                    )
                    .is_some()
                {
                    return true;
                }

                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count,
                    UnitType::AirUnitsPlant,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }
            }

            crate::enums::AiStrategy::Sea => {
                if get_unit_count(
                    kind,
                    ctx,
                    UnitType::Shipyard,
                    TASK_PRIORITY_BUILDING_HEAVY_PLANT,
                ) == 0
                    && manage_create_building(
                        kind,
                        ctx,
                        header,
                        UnitType::Shipyard,
                        TASK_PRIORITY_BUILDING_HEAVY_PLANT,
                    )
                    .is_some()
                {
                    return true;
                }

                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count,
                    UnitType::Shipyard,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }
            }

            crate::enums::AiStrategy::ScoutHorde => {
                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count,
                    UnitType::LightVehiclePlant,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }
            }

            crate::enums::AiStrategy::TankHorde => {
                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count,
                    UnitType::HeavyVehiclePlant,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }
            }

            crate::enums::AiStrategy::FastAttack => {
                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count / 2,
                    UnitType::LightVehiclePlant,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }

                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count / 2,
                    UnitType::HeavyVehiclePlant,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }
            }

            crate::enums::AiStrategy::CombinedArms => {
                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count,
                    UnitType::HeavyVehiclePlant,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }
            }

            crate::enums::AiStrategy::Espionage => {
                if get_unit_count(
                    kind,
                    ctx,
                    UnitType::TrainingHall,
                    TASK_PRIORITY_BUILDING_HEAVY_PLANT,
                ) == 0
                    && manage_create_building(
                        kind,
                        ctx,
                        header,
                        UnitType::TrainingHall,
                        TASK_PRIORITY_BUILDING_HEAVY_PLANT,
                    )
                    .is_some()
                {
                    return true;
                }

                if create_buildings(
                    kind,
                    ctx,
                    header,
                    total_unit_count,
                    UnitType::TrainingHall,
                    TASK_PRIORITY_BUILDING_EXPANSION,
                ) {
                    return true;
                }
            }

            crate::enums::AiStrategy::Random => {}
        }
    }

    // Repair shops and their capacity upkeep.
    if get_unit_count(kind, ctx, UnitType::HeavyVehiclePlant, 0x00) > 0
        && get_unit_count(
            kind,
            ctx,
            UnitType::Depot,
            TASK_PRIORITY_BUILDING_REPAIR_SHOP,
        ) == 0
        && manage_create_building(
            kind,
            ctx,
            header,
            UnitType::Depot,
            TASK_PRIORITY_BUILDING_REPAIR_SHOP,
        )
        .is_some()
    {
        return true;
    }

    if get_unit_count(kind, ctx, UnitType::AirUnitsPlant, 0x00) > 0
        && get_unit_count(
            kind,
            ctx,
            UnitType::Hangar,
            TASK_PRIORITY_BUILDING_REPAIR_SHOP,
        ) == 0
        && manage_create_building(
            kind,
            ctx,
            header,
            UnitType::Hangar,
            TASK_PRIORITY_BUILDING_REPAIR_SHOP,
        )
        .is_some()
    {
        return true;
    }

    if get_unit_count(kind, ctx, UnitType::Shipyard, 0x00) > 0
        && get_unit_count(
            kind,
            ctx,
            UnitType::Dock,
            TASK_PRIORITY_BUILDING_REPAIR_SHOP,
        ) == 0
        && manage_create_building(
            kind,
            ctx,
            header,
            UnitType::Dock,
            TASK_PRIORITY_BUILDING_REPAIR_SHOP,
        )
        .is_some()
    {
        return true;
    }

    if get_unit_count(kind, ctx, UnitType::TrainingHall, 0x00) > 0
        && get_unit_count(
            kind,
            ctx,
            UnitType::Barracks,
            TASK_PRIORITY_MANAGE_BUILDINGS,
        ) == 0
        && manage_create_building(
            kind,
            ctx,
            header,
            UnitType::Barracks,
            TASK_PRIORITY_MANAGE_BUILDINGS,
        )
        .is_some()
    {
        return true;
    }

    if get_unit_count(kind, ctx, UnitType::AirUnitsPlant, 0x00) > 0
        && get_unit_count(
            kind,
            ctx,
            UnitType::Shipyard,
            TASK_PRIORITY_BUILDING_HEAVY_PLANT,
        ) == 0
        && manage_create_building(
            kind,
            ctx,
            header,
            UnitType::Shipyard,
            TASK_PRIORITY_BUILDING_HEAVY_PLANT,
        )
        .is_some()
    {
        return true;
    }

    // `Access_GetRepairShopClientCount`: one shop per `capacity * 2` clients.
    for (shop, priority) in [
        (UnitType::Depot, TASK_PRIORITY_BUILDING_REPAIR_SHOP),
        (UnitType::Hangar, TASK_PRIORITY_BUILDING_REPAIR_SHOP),
        (UnitType::Dock, TASK_PRIORITY_BUILDING_REPAIR_SHOP),
        (UnitType::Barracks, TASK_PRIORITY_MANAGE_BUILDINGS),
    ] {
        let clients = crate::access::get_repair_shop_client_count(ctx.world, team, shop);
        let capacity = ctx.world.team_units[team.0 as usize]
            .base_unit_values(shop as u16)
            .get_attribute(ATTRIB_STORAGE);

        if capacity <= 0 {
            continue;
        }

        if get_unit_count(kind, ctx, shop, priority) < clients / (capacity * 2)
            && manage_create_building(kind, ctx, header, shop, priority).is_some()
        {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Cargo demand — UpdateMiningNeeds
// ---------------------------------------------------------------------------

/// `TaskManageBuildings::UpdateCargoDemand` (`taskmanagebuildings.cpp:1220`).
fn update_cargo_demand(limit: &mut i16, material: &mut i32, max_mining: i32) {
    let mut demand = -*material;

    if demand > 0 {
        demand = demand.min(i32::from(*limit));
        demand = demand.min(max_mining);

        *limit = limit.wrapping_sub(demand as i16);
        *material += demand;
    }
}

/// `TaskManageBuildings::UpdateMiningNeeds` (`taskmanagebuildings.cpp:1237`).
///
/// Returns the demand computed for the team (also stored in the task state, as
/// in C++ `cargo_demand`) and creates the communications tower when the gold
/// balance covers it.
fn update_mining_needs(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> Cargo {
    let team = header.team;
    let mut cargo_demand = Cargo::init();

    for unit_id in ctx.list(UnitListKind::MobileLandSea) {
        let Some(unit) = ctx.unit(unit_id) else {
            continue;
        };

        if unit.team != team {
            continue;
        }

        if matches!(unit.unit_type, UnitType::Engineer | UnitType::Constructor) {
            cargo_demand.raw += i32::from(unit.storage);

            if unit.unit_type == UnitType::Engineer {
                cargo_demand.raw -= 20;
            } else {
                cargo_demand.raw -= 40;
            }
        }
    }

    cargo_demand.raw = (cargo_demand.raw - 50) / 10;
    cargo_demand.fuel -= 5;

    if is_buildable(UnitType::GoldRefinery) {
        cargo_demand.gold -= cargo_gold_consumption_rate(UnitType::GoldRefinery);
    }

    for unit_id in ctx.list(UnitListKind::Stationary) {
        let Some(unit) = ctx.unit(unit_id) else {
            continue;
        };

        if unit.team != team {
            continue;
        }

        cargo_demand.raw -= cargo_raw_consumption_rate(unit.unit_type, 1);
        cargo_demand.fuel -= cargo_fuel_consumption_rate(unit.unit_type);
        cargo_demand.gold -= cargo_gold_consumption_rate(unit.unit_type);

        match cargo_type_of(unit.unit_type) {
            CargoType::Raw => cargo_demand.raw += i32::from(unit.storage) / 10,
            CargoType::Fuel => cargo_demand.fuel += i32::from(unit.storage) / 10,
            CargoType::Gold => cargo_demand.gold += i32::from(unit.storage) / 20,
            _ => {}
        }
    }

    let mut cargo_gold = cargo_demand.gold;

    for task_id in manager_tasks(kind) {
        let Some(task) = ctx.tm.task(task_id) else {
            continue;
        };

        let Some(unit_type) = task.kind.create_unit_type() else {
            continue;
        };

        cargo_gold -= cargo_gold_consumption_rate(unit_type);

        if task.kind.is_actively_building() {
            cargo_demand.raw -= cargo_raw_consumption_rate(unit_type, 1);
            cargo_demand.fuel -= cargo_fuel_consumption_rate(unit_type);
            cargo_demand.gold -= cargo_gold_consumption_rate(unit_type);
        }
    }

    for unit_id in ctx.list(UnitListKind::Stationary) {
        let Some(unit) = ctx.unit(unit_id) else {
            continue;
        };

        if unit.team != team || unit.unit_type != UnitType::MiningStation {
            continue;
        }

        let mut capacity_limit = 16i16;

        update_cargo_demand(
            &mut capacity_limit,
            &mut cargo_demand.fuel,
            i32::from(unit.fuel_mining_max),
        );
        update_cargo_demand(
            &mut capacity_limit,
            &mut cargo_demand.raw,
            i32::from(unit.raw_mining_max),
        );

        cargo_gold += i32::from(capacity_limit).min(i32::from(unit.gold_mining_max));

        update_cargo_demand(
            &mut capacity_limit,
            &mut cargo_demand.gold,
            i32::from(unit.gold_mining_max),
        );
    }

    for task_id in manager_tasks(kind) {
        let Some(task) = ctx.tm.task(task_id) else {
            continue;
        };

        if task.kind.create_unit_type() != Some(UnitType::MiningStation) {
            continue;
        }

        let mut capacity_limit = 16i16;
        let bounds = ctx.tm.task_get_bounds(ctx.world, task_id);

        // `Survey_GetTotalResourcesInArea(bounds.ulx, bounds.uly, 1, …)`.
        let (raw, gold, fuel) = crate::deploy::survey_resources_in_area(
            ctx.world,
            Point::new(bounds.ulx as i16, bounds.uly as i16),
            1,
        );

        update_cargo_demand(&mut capacity_limit, &mut cargo_demand.fuel, fuel);
        update_cargo_demand(&mut capacity_limit, &mut cargo_demand.raw, raw);

        let actively_building = task.kind.is_actively_building();

        if actively_building {
            cargo_gold += i32::from(capacity_limit).min(gold);
        }

        update_cargo_demand(&mut capacity_limit, &mut cargo_demand.gold, gold);
    }

    if let TaskKind::ManageBuildings {
        cargo_demand: stored,
        ..
    } = kind
    {
        *stored = cargo_demand;
    }

    if cargo_gold_consumption_rate(UnitType::GoldRefinery) <= cargo_gold {
        manage_create_building(
            kind,
            ctx,
            header,
            UnitType::GoldRefinery,
            TASK_PRIORITY_BUILDING_UPGRADES,
        );
    }

    cargo_demand
}

/// `TaskManageBuildings::CheckNeeds` (`taskmanagebuildings.cpp:1383`).
///
/// First the strategic build plan, then the mining demand. The C++ retry
/// reminder (`RemindTurnStart`) is unnecessary: the task manager reminds every
/// task at every turn start.
fn check_needs(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    if plan_next_build_job(kind, ctx, header) {
        return true;
    }

    let cargo_demand = update_mining_needs(kind, ctx, header);

    if cargo_demand.raw < 0 || cargo_demand.fuel < 0 || cargo_demand.gold < 0 {
        let team = header.team;
        let has_mining_station = ctx
            .list(UnitListKind::Stationary)
            .into_iter()
            .any(|unit_id| {
                ctx.unit(unit_id)
                    .map(|unit| unit.team == team && unit.unit_type == UnitType::MiningStation)
                    .unwrap_or(false)
            });

        let priority = if has_mining_station {
            TASK_PRIORITY_BUILDING_MEDIUM
        } else {
            TASK_PRIORITY_BUILDING_LOW
        };

        if manage_create_building(kind, ctx, header, UnitType::MiningStation, priority).is_some() {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Unit production on factories
// ---------------------------------------------------------------------------

/// The `TaskAttackReserve::AddUnit` factory branch
/// (`taskattackreserve.cpp:136-164`).
///
/// C++ rolls a unit type per idle factory from
/// `GetFilteredWeightTable(INVALID_ID, 3)`, zeroes the weights of types another
/// factory builds or that no longer fit into the mission time, and orders the
/// rolled type through `TaskObtainUnits`. The reserve life cycle is phase A.4;
/// the manager performs the same roll so that the weight tables drive unit
/// production already at A.3. `flags = 3` disables stationary (factories must
/// not build factories) and regenerating (alien) types.
fn plan_unit_production(kind: &TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let team = header.team;
    let Some(site) = manager_site(kind) else {
        return;
    };

    let factories: Vec<UnitId> = ctx
        .list(UnitListKind::Stationary)
        .into_iter()
        .filter(|unit_id| {
            ctx.unit(*unit_id)
                .map(|unit| {
                    unit.team == team
                        && is_factory(unit.unit_type)
                        && unit.orders != UnitOrder::Idle
                        // A factory already running a production task is not
                        // "available" (C++ `FindTaskForUnit` skips `GetTask()`).
                        && ctx.tm.unit_task(*unit_id).is_none()
                })
                .unwrap_or(false)
        })
        .collect();

    for factory_id in factories {
        let Some(factory) = ctx.unit(factory_id) else {
            continue;
        };

        let Some(complex_id) = factory.complex else {
            continue;
        };

        let (material, fuel) = ctx.world.team_units[team.0 as usize]
            .get_complex(complex_id)
            .map(|complex| (complex.raw, complex.fuel))
            .unwrap_or((0, 0));

        if material <= 10 || fuel <= 10 {
            continue;
        }

        let turns_till_mission_end = task_estimate_turns_till_mission_end(ctx.world);
        let mut table =
            ctx.world.ai.players[team.0 as usize].get_filtered_weight_table(ctx.world, None, 0x03);

        for entry in table.entries_mut() {
            let Some(unit_type) = entry.unit_type else {
                continue;
            };

            let builder_matches = crate::production::builder_type(unit_type)
                .map(|builder| builder == factory.unit_type)
                .unwrap_or(false);

            let turns = ctx.world.team_units[team.0 as usize]
                .current_unit_values(unit_type as u16)
                .get_attribute(ATTRIB_TURNS);

            if !builder_matches || turns > turns_till_mission_end {
                entry.weight = 0;
            }
        }

        let unit_type = table.roll_unit_type(&mut ctx.world.rng);
        let Some(unit_type) = unit_type else {
            continue;
        };

        let mut obtain_kind = TaskKind::obtain_units(site);
        super::create::obtain_units_add_type(&mut obtain_kind, unit_type);

        // `TaskObtainUnits(this, site)` — the C++ parent is the attack reserve
        // (priority `TASK_PRIORITY_ATTACK_RESERVE_POOL`).
        ctx.append_task(
            team,
            Some(header.id),
            super::super::task::TASK_PRIORITY_ATTACK_RESERVE_POOL,
            obtain_kind,
        );
    }
}

/// The C++ factory types (`LANDPLT`/`LIGHTPLT`/`AIRPLT`/`SHIPYARD`/`TRAINHAL`).
fn is_factory(unit_type: UnitType) -> bool {
    matches!(
        unit_type,
        UnitType::HeavyVehiclePlant
            | UnitType::LightVehiclePlant
            | UnitType::AirUnitsPlant
            | UnitType::Shipyard
            | UnitType::TrainingHall
    )
}

// ---------------------------------------------------------------------------
// Assistants — CheckPower / CheckWorkers / radar
// ---------------------------------------------------------------------------

/// `TaskCreateBuilding::EstimateBuildTime` (`taskcreatebuilding.cpp:98`).
fn estimate_build_time(ctx: &TaskCtx<'_>, task_id: u32) -> i32 {
    let Some(task) = ctx.tm.task(task_id) else {
        return 100;
    };

    match &task.kind {
        TaskKind::CreateBuilding {
            builder: Some(builder),
            ..
        } if task.kind.is_actively_building() => ctx
            .unit(*builder)
            .map(|unit| i32::from(unit.build_time))
            .unwrap_or(100),
        _ => 100,
    }
}

/// `TaskManageBuildings::CheckWorkers` (`taskmanagebuildings.cpp:2521`).
fn check_workers(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    let mut total_consumption = 0;

    for unit_id in manager_units(kind) {
        if let Some(unit) = ctx.unit(unit_id) {
            let rate = cargo_life_consumption_rate(unit.unit_type);
            if rate > 0 {
                total_consumption += rate;
            }
        }
    }

    for task_id in manager_tasks(kind) {
        let Some(task) = ctx.tm.task(task_id) else {
            continue;
        };

        if !task.kind.is_actively_building() {
            continue;
        }

        if let Some(unit_type) = task.kind.create_unit_type() {
            let rate = cargo_life_consumption_rate(unit_type);
            if rate > 0 {
                total_consumption += rate;
            }
        }
    }

    let rate = -cargo_life_consumption_rate(UnitType::Habitat);

    if rate <= 0 {
        return false;
    }

    let habitat_demand = (total_consumption + rate - 1) / rate;

    create_buildings(
        kind,
        ctx,
        header,
        habitat_demand,
        UnitType::Habitat,
        TASK_PRIORITY_BUILDING_HABITAT,
    )
}

/// `TaskManageBuildings::CheckPower` (`taskmanagebuildings.cpp:2552`).
fn check_power(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    let team = header.team;
    let mut power_consumption = 0;
    let mut power_generation = 0;
    let mut total_storage = 0;

    for unit_id in manager_units(kind) {
        let Some(unit) = ctx.unit(unit_id) else {
            continue;
        };

        let rate = cargo_power_consumption_rate(unit.unit_type);

        if rate >= 0 {
            power_consumption += rate;
        } else {
            power_generation -= rate;
        }

        if cargo_type_of(unit.unit_type) == CargoType::Air {
            total_storage += i32::from(unit.storage);
        }
    }

    let mut turns_to_build =
        crate::production::turns_to_build(ctx.world, team, UnitType::PowerStation);
    let mut total_power_generation = power_generation;

    for task_id in manager_tasks(kind) {
        let Some(task) = ctx.tm.task(task_id) else {
            continue;
        };

        if task.kind.create_unit_type() == Some(UnitType::PowerGenerator) {
            total_power_generation += 1;
        }

        if task.kind.is_actively_building()
            && task.kind.create_unit_type() == Some(UnitType::PowerStation)
        {
            turns_to_build = turns_to_build.min(estimate_build_time(ctx, task_id));
        }
    }

    for task_id in manager_tasks(kind) {
        let Some(task) = ctx.tm.task(task_id) else {
            continue;
        };

        if !task.kind.is_actively_building() || estimate_build_time(ctx, task_id) >= turns_to_build
        {
            continue;
        }

        if let Some(unit_type) = task.kind.create_unit_type() {
            let rate = cargo_power_consumption_rate(unit_type);
            if rate > 0 {
                power_consumption += rate;
            }
        }
    }

    let generation_rate = -cargo_power_consumption_rate(UnitType::PowerStation);
    let generator_rate = -cargo_power_consumption_rate(UnitType::PowerGenerator);
    let station_fuel = cargo_fuel_consumption_rate(UnitType::PowerStation);
    let generator_fuel = cargo_fuel_consumption_rate(UnitType::PowerGenerator);

    if total_storage < 50 && generation_rate > 0 && generator_fuel > 0 {
        let mut power_station_demand =
            generation_rate - (station_fuel * generator_rate) / generator_fuel;

        power_station_demand = (power_consumption + power_station_demand) / generation_rate;

        if create_buildings(
            kind,
            ctx,
            header,
            power_station_demand,
            UnitType::PowerStation,
            TASK_PRIORITY_BUILDING_POWER,
        ) {
            return true;
        }
    }

    if power_consumption > total_power_generation
        && manage_create_building(
            kind,
            ctx,
            header,
            UnitType::PowerGenerator,
            TASK_PRIORITY_BUILDING_POWER,
        )
        .is_some()
    {
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// TaskManageBuildings — lifecycle
// ---------------------------------------------------------------------------

fn manage_buildings_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let (units, tasks, initialized) = match kind {
        TaskKind::ManageBuildings {
            units,
            tasks,
            initialized,
            ..
        } => (units, tasks, initialized),
        _ => return,
    };

    units.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));
    tasks.retain(|task| ctx.tm.has_task(*task));

    if !*initialized {
        *initialized = true;

        // Child assistants (`Task*Assistant`) — as in C++ `Init`.
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

    check_needs(kind, ctx, header);

    // Unit production from the weight tables (see `plan_unit_production`).
    plan_unit_production(kind, ctx, header);
}

/// `TaskManageBuildings::EndTurn` (`taskmanagebuildings.cpp:2258`) — the storage
/// upkeep: a dump is added when a mine fills it.
fn manage_buildings_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let team = header.team;
    let mut cargo = Cargo::init();
    let mut capacity = Cargo::init();
    let mut raw_mining_max = 0;
    let mut fuel_mining_max = 0;
    let mut gold_mining_max = 0;

    for unit_id in manager_units(kind) {
        let Some(unit) = ctx.unit(unit_id) else {
            continue;
        };

        if unit.unit_type == UnitType::MiningStation {
            raw_mining_max += i32::from(unit.raw_mining_max);
            fuel_mining_max += i32::from(unit.fuel_mining_max);
            gold_mining_max += i32::from(unit.gold_mining_max);
        }

        match cargo_type_of(unit.unit_type) {
            CargoType::Raw => {
                cargo.raw += i32::from(unit.storage);
                capacity.raw += unit.base_values.get_attribute(ATTRIB_STORAGE);
            }
            CargoType::Fuel => {
                cargo.fuel += i32::from(unit.storage);
                capacity.fuel += unit.base_values.get_attribute(ATTRIB_STORAGE);
            }
            CargoType::Gold => {
                cargo.gold += i32::from(unit.storage);
                capacity.gold += unit.base_values.get_attribute(ATTRIB_STORAGE);
            }
            _ => {}
        }
    }

    let team_units = &ctx.world.team_units[team.0 as usize];
    let adump_capacity = team_units
        .base_unit_values(UnitType::MaterialStorage as u16)
        .get_attribute(ATTRIB_STORAGE);

    for task_id in manager_tasks(kind) {
        let Some(task) = ctx.tm.task(task_id) else {
            continue;
        };

        let Some(unit_type) = task.kind.create_unit_type() else {
            continue;
        };

        if unit_type == UnitType::MiningStation && task.kind.is_actively_building() {
            let bounds = ctx.tm.task_get_bounds(ctx.world, task_id);

            // `Survey_GetTotalResourcesInArea(bounds.ulx, bounds.uly, 1, …)`.
            let (raw, gold, fuel) = crate::deploy::survey_resources_in_area(
                ctx.world,
                Point::new(bounds.ulx as i16, bounds.uly as i16),
                1,
            );

            raw_mining_max += raw;
            fuel_mining_max += fuel;
            gold_mining_max += gold;

            // Do not count the mediocre storage capacity of planned mining
            // stations under a given capacity threshold.
            if capacity.raw < 2 * adump_capacity {
                continue;
            }
        }

        let storage = ctx.world.team_units[team.0 as usize]
            .current_unit_values(unit_type as u16)
            .get_attribute(ATTRIB_STORAGE);

        match cargo_type_of(unit_type) {
            CargoType::Raw => capacity.raw += storage,
            CargoType::Fuel => capacity.fuel += storage,
            CargoType::Gold => capacity.gold += storage,
            _ => {}
        }
    }

    if raw_mining_max > 0
        && (cargo.raw >= (capacity.raw * 3) / 4 || capacity.raw < 2 * adump_capacity)
        && capacity.raw < 500
    {
        manage_create_building(
            kind,
            ctx,
            header,
            UnitType::MaterialStorage,
            TASK_PRIORITY_BUILDING_DUMP,
        );
    }

    if fuel_mining_max > 0 && cargo.fuel >= (capacity.fuel * 3) / 4 && capacity.fuel < 500 {
        manage_create_building(
            kind,
            ctx,
            header,
            UnitType::FuelStorage,
            TASK_PRIORITY_BUILDING_DUMP,
        );
    }

    if gold_mining_max > 0
        && cargo.gold >= (capacity.gold * 3) / 4
        && capacity.gold < 500
        && is_buildable(UnitType::GoldVault)
    {
        manage_create_building(
            kind,
            ctx,
            header,
            UnitType::GoldVault,
            TASK_PRIORITY_BUILDING_DUMP,
        );
    }
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

    // `TaskManageBuildings::AddUnit` (`taskmanagebuildings.cpp:2168`): only
    // stationary units join `units`; a mobile builder gets a
    // `TaskCreateBuilding` child in C++ (the port's builders already carry
    // their own construction task).
    let Some(info) = ctx.unit(unit) else {
        return;
    };

    if info.is_mobile() {
        return;
    }

    if !units.contains(&unit) {
        units.push(unit);
    }

    let _ = header;
}

/// `TaskManageBuildings::IsUnitUsable` — a stationary team building.
pub(crate) fn manage_buildings_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    info.team == header.team
        && !info.is_mobile()
        && info.flags & crate::enums::unit_flags::GROUND_COVER == 0
        && info.orders != UnitOrder::Idle
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `TaskPowerAssistant::BeginTurn` — request a power station.
fn power_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::PowerAssistant { manager } = kind else {
        return;
    };

    let Some(manager_id) = *manager else {
        ctx.remove_self(header.id);
        return;
    };

    let Some((mut manager_kind, manager_header)) = read_manager_kind(ctx, manager_id) else {
        ctx.remove_self(header.id);
        return;
    };

    check_power(&mut manager_kind, ctx, manager_header);

    write_manager_kind(ctx, manager_id, manager_kind);
}

/// `TaskHabitatAssistant` — a habitat when workers are short.
fn habitat_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::HabitatAssistant { manager } = kind else {
        return;
    };

    let Some(manager_id) = *manager else {
        ctx.remove_self(header.id);
        return;
    };

    let Some((mut manager_kind, manager_header)) = read_manager_kind(ctx, manager_id) else {
        ctx.remove_self(header.id);
        return;
    };

    check_workers(&mut manager_kind, ctx, manager_header);

    write_manager_kind(ctx, manager_id, manager_kind);
}

/// `TaskRadarAssistant` — a radar when absent.
///
/// Simplification: `FindSiteForRadar`/`EvaluateNeedForRadar` need the
/// `AccessMap` site marking (P7); the port builds a radar when the team has none
/// and the mission still lasts long enough.
fn radar_assistant_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::RadarAssistant { manager } = kind else {
        return;
    };

    let Some(manager_id) = *manager else {
        ctx.remove_self(header.id);
        return;
    };

    let Some((mut manager_kind, manager_header)) = read_manager_kind(ctx, manager_id) else {
        ctx.remove_self(header.id);
        return;
    };

    let team = manager_header.team;
    let turns = ctx.world.team_units[team.0 as usize]
        .current_unit_values(UnitType::Radar as u16)
        .get_attribute(ATTRIB_TURNS);

    if task_estimate_turns_till_mission_end(ctx.world) >= turns
        && get_unit_count(
            &manager_kind,
            ctx,
            UnitType::Radar,
            TASK_PRIORITY_BUILDING_RADAR,
        ) == 0
    {
        manage_create_building(
            &mut manager_kind,
            ctx,
            manager_header,
            UnitType::Radar,
            TASK_PRIORITY_BUILDING_RADAR,
        );
    }

    write_manager_kind(ctx, manager_id, manager_kind);
}

/// `TaskConnectionAssistant` — connectors between buildings.
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

    // `MakeConnectors`/`ConnectBuilding` require the `AccessMap` marking (P7).
    let _ = TASK_PRIORITY_BUILDING_CONNECTOR;
}

/// `TaskDefenseAssistant` — a defensive turret.
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

    // `FindDefenseSite` requires the `AccessMap` defense-site marking (P7).
    let _ = (unit_type, TASK_PRIORITY_BUILDING_DEFENSE);
}

/// `TaskFrontierAssistant` — frontier: mining/mine laying.
fn frontier_assistant_begin_turn(_kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    // The frontier site is chosen by `TaskUpdateTerrain`/`PlanMinefields` (P7).
    ctx.remove_self(header.id);
}

/// `TaskMineAssistant` — the minefield manager (`TaskMineAssistant`).
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

/// `TaskUpdateTerrain` — updating the distance field (`AiPlayer_TerrainDistanceField`).
fn update_terrain_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::UpdateTerrain { location } = kind else {
        return;
    };

    *location = Point::ZERO;
    ctx.remove_self(header.id);
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// `Init` for build classes.
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

/// `BeginTurn` for build classes.
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

/// `EndTurn` for build classes.
pub fn end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::ManageBuildings { .. } => {
            manage_buildings_end_turn(kind, ctx, header);
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

/// `Execute(unit)` — assistants do not execute orders.
pub fn execute(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `AddUnit(unit)` for build classes.
pub fn add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::ManageBuildings { .. } => manage_buildings_add_unit(kind, ctx, header, unit),
        _ => {}
    }
}

/// `RemoveUnit(unit)` for build classes.
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

/// `RemoveSelf()` for build classes.
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

/// `ChildComplete(task)` — the manager removes a completed task.
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

/// Notification about a child `TaskMove` completion.
pub fn move_finished(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
    _result: i8,
) -> bool {
    false
}

/// `CheckReactions` — the construction manager does not react.
pub fn check_reactions(_kind: &mut TaskKind, _ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    false
}

/// `ExchangeOperator` — not overridden.
pub fn exchange_operator(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventUnitDestroyed(unit)` — the manager forgets the building.
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

/// `EventEnemyUnitSpotted` — not overridden.
pub fn event_enemy_unit_spotted(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventCargoTransfer` — not overridden.
pub fn event_cargo_transfer(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventUnitLoaded` — not overridden.
pub fn event_unit_loaded(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit1: UnitId,
    _unit2: UnitId,
) -> bool {
    false
}

/// `EventUnitUnloaded` — not overridden.
pub fn event_unit_unloaded(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit1: UnitId,
    _unit2: UnitId,
) -> bool {
    false
}

/// Assistant priorities (exported for `task.hpp` conformance tests).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{Surface, TeamClan};
    use crate::map::Map;
    use crate::team::TeamKind;
    use crate::game::GameState;

    fn land_map() -> Map {
        let mut map = Map::new();

        for y in 0..crate::geo::MAP_HEIGHT {
            for x in 0..crate::geo::MAP_WIDTH {
                map.surface.set(x, y, Surface::LAND);
            }
        }

        map
    }

    fn computer_state() -> GameState {
        let mut state = GameState::with_setup(
            land_map(),
            [
                TeamKind::Computer,
                TeamKind::Computer,
                TeamKind::None,
                TeamKind::None,
            ],
        );

        for team in 0..2 {
            state.teams[team].team_type = TeamKind::Computer;
            state.teams[team].team_clan = TeamClan::Musashi;
        }
        crate::ai::ai_init(&mut state);
        state.ai.settings.opponent = crate::enums::Opponent::Expert as u8;

        state
    }

    fn manager_task(state: &mut GameState, site: Point) -> u32 {
        state.ai_append_task(
            TeamId::RED,
            None,
            TASK_PRIORITY_MANAGE_BUILDINGS,
            TaskKind::manage_buildings(site),
        )
    }

    fn with_manager<R>(
        state: &mut GameState,
        task_id: u32,
        f: impl FnOnce(&mut TaskKind, &mut TaskCtx<'_>, TaskHeader) -> R,
    ) -> R {
        state.with_task_manager(|task_manager, world| {
            let header = task_manager.task(task_id).expect("manager task").header();
            let mut kind = task_manager
                .task(task_id)
                .expect("manager task")
                .kind
                .clone();

            let result = {
                let mut ctx = TaskCtx::new(task_manager, world);
                f(&mut kind, &mut ctx, header)
            };

            task_manager.task_mut(task_id).expect("manager task").kind = kind;

            result
        })
    }

    #[test]
    fn plan_next_build_job_starts_with_the_light_plant() {
        let mut state = computer_state();
        let site = Point::new(10, 10);

        // A base with one building: the C++ `if (units.GetCount())` guard.
        let mine = state
            .spawn_unit(UnitType::MiningStation, TeamId::RED, site, 0)
            .expect("mining station");
        let manager = manager_task(&mut state, site);

        let planned = with_manager(&mut state, manager, |kind, ctx, header| {
            if let TaskKind::ManageBuildings { units, .. } = kind {
                units.push(mine);
            }

            plan_next_build_job(kind, ctx, header)
        });

        assert!(planned, "the light plant must be the first factory");

        let create_types: Vec<UnitType> = state
            .ai
            .task_manager
            .tasks()
            .iter()
            .filter_map(|task| match &task.kind {
                TaskKind::CreateBuilding { unit_type, .. } => Some(*unit_type),
                _ => None,
            })
            .collect();

        assert_eq!(create_types, vec![UnitType::LightVehiclePlant]);
    }

    #[test]
    fn plan_unit_production_rolls_a_factory_type_from_the_weight_table() {
        let mut state = computer_state();
        let site = Point::new(10, 10);

        let factory = state
            .spawn_unit(
                UnitType::LightVehiclePlant,
                TeamId::RED,
                Point::new(8, 8),
                0,
            )
            .expect("light plant");
        let mut unit = state.unit(factory).expect("light plant").clone();
        unit.orders = UnitOrder::PowerOn;
        state.write_unit(factory, unit);

        let complex = state
            .unit(factory)
            .and_then(|unit| unit.complex)
            .expect("factory complex");
        let complex = state.team_units[TeamId::RED.0 as usize]
            .get_complex_mut(complex)
            .expect("factory complex");
        complex.raw = 100;
        complex.fuel = 100;

        let manager = manager_task(&mut state, site);

        with_manager(&mut state, manager, |kind, ctx, header| {
            plan_unit_production(kind, ctx, header);
        });

        let obtain: Vec<UnitType> = state
            .ai
            .task_manager
            .tasks()
            .iter()
            .filter_map(|task| match &task.kind {
                TaskKind::ObtainUnits { units, .. } => units.first().copied(),
                _ => None,
            })
            .collect();

        assert_eq!(obtain.len(), 1, "one unit request per factory");
        let requested = obtain[0];
        assert_eq!(
            crate::production::builder_type(requested),
            Some(UnitType::LightVehiclePlant),
            "the rolled type must be buildable by the idle factory"
        );
        assert_eq!(
            crate::access::unit_type_flags(requested) & crate::enums::unit_flags::STATIONARY,
            0,
            "stationary types are filtered out (flags = 3)"
        );
    }

    #[test]
    fn check_needs_orders_a_mining_station_for_a_negative_cargo_demand() {
        let mut state = computer_state();
        let site = Point::new(10, 10);
        let manager = manager_task(&mut state, site);

        // An empty manager: `PlanNextBuildJob` bails out on `units.GetCount()`,
        // and the cargo demand (no builders, no mines) is negative.
        let planned = with_manager(&mut state, manager, |kind, ctx, header| {
            check_needs(kind, ctx, header)
        });

        assert!(planned, "a negative demand must order a mining station");

        let create_types: Vec<UnitType> = state
            .ai
            .task_manager
            .tasks()
            .iter()
            .filter_map(|task| match &task.kind {
                TaskKind::CreateBuilding { unit_type, .. } => Some(*unit_type),
                _ => None,
            })
            .collect();

        assert_eq!(create_types, vec![UnitType::MiningStation]);
    }

    #[test]
    fn plan_unit_production_needs_materials_and_fuel() {
        let mut state = computer_state();
        let site = Point::new(10, 10);

        let factory = state
            .spawn_unit(
                UnitType::LightVehiclePlant,
                TeamId::RED,
                Point::new(8, 8),
                0,
            )
            .expect("light plant");
        let mut unit = state.unit(factory).expect("light plant").clone();
        unit.orders = UnitOrder::PowerOn;
        state.write_unit(factory, unit);

        let complex = state
            .unit(factory)
            .and_then(|unit| unit.complex)
            .expect("factory complex");
        let complex = state.team_units[TeamId::RED.0 as usize]
            .get_complex_mut(complex)
            .expect("factory complex");
        complex.raw = 10;
        complex.fuel = 10;

        let manager = manager_task(&mut state, site);

        with_manager(&mut state, manager, |kind, ctx, header| {
            plan_unit_production(kind, ctx, header);
        });

        assert!(
            !state
                .ai
                .task_manager
                .tasks()
                .iter()
                .any(|task| matches!(task.kind, TaskKind::ObtainUnits { .. })),
            "the C++ threshold is material > 10 and fuel > 10"
        );
    }
}
