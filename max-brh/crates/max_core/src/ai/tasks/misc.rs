//! Экономика, ремонт и очистка: `TaskGetResource`/`TaskGetMaterials`,
//! `TaskRepair`/`TaskReload`/`TaskUpgrade`, `TaskRemoveRubble`/
//! `TaskRemoveMines`, `TaskPlaceMines`, `TaskScavenge`, `TaskClearZone`,
//! `TaskCheckAssaults`.
//!
//! Оракул: `taskgetresource.cpp/.hpp`, `taskgetmaterials.cpp/.hpp`,
//! `taskrepair.cpp/.hpp`, `taskreload.cpp/.hpp`, `taskupgrade.cpp/.hpp`,
//! `taskremoverubble.cpp/.hpp`, `taskremovemines.cpp/.hpp`,
//! `taskplacemines.cpp/.hpp`, `taskscavenge.cpp/.hpp`,
//! `taskclearzone.cpp/.hpp`, `taskcheckassaults.cpp/.hpp`.
//!
//! # Упрощения P6 (P3-экономика и P5-разметка зон)
//!
//! * передача груза (`TaskGetMaterials::DoTransfer`) выполняется приказом
//!   `ORDER_TRANSFER` — сама передача реализуется экономикой комплексов (P3),
//!   поэтому здесь только маршрут и приказ;
//! * ремонт/перезарядка/апгрейд отдают `ORDER_REPAIR`/`ORDER_RELOAD`/
//!   `ORDER_UPGRADE` ремонтному цеху (P3-производство);
//! * `TaskClearZone` обходит заявленные сайты и отдаёт `ORDER_CLEAR`
//!   бульдозеру; полная разметка `Zone` (P5/P7) не портирована;
//! * `TaskCheckAssaults` контролирует штурмовые группы по приказам
//!   `ORDER_MOVE_TO_ATTACK` (полные проверки воздушных штурмов — P7).

use crate::enums::{unit_flags, CautionLevel, UnitOrder, UnitOrderState, UnitType};
use crate::geo::Point;
use crate::unit::{UnitId, UnitListKind};

use super::super::task::{
    task_is_repair_due, TaskHeader, TASK_PRIORITY_AUTO_SURVEY, TASK_PRIORITY_CHECK_ASSAULTS,
    TASK_PRIORITY_EMERGENCY, TASK_PRIORITY_GET_RESOURCE, TASK_PRIORITY_REMOVE_RUBBLE,
    TASK_PRIORITY_REMOVE_RUBBLE_LOW, TASK_PRIORITY_REPAIR, TASK_PRIORITY_SCAVENGE,
};
use super::{TaskCtx, TaskKind};

// ---------------------------------------------------------------------------
// Общие поиски
// ---------------------------------------------------------------------------

/// Расстояние в квадрате.
fn distance_squared(first: Point, second: Point) -> i32 {
    let dx = i32::from(first.x) - i32::from(second.x);
    let dy = i32::from(first.y) - i32::from(second.y);
    dx * dx + dy * dy
}

/// Ближайший юнит команды с указанным типом (`FindClosestBuilding`).
fn find_closest_unit_of_type(
    ctx: &TaskCtx<'_>,
    team: crate::team::TeamId,
    unit_type: UnitType,
    from: Point,
) -> Option<UnitId> {
    let mut best: Option<(i32, UnitId)> = None;

    for kind in [
        UnitListKind::Stationary,
        UnitListKind::MobileLandSea,
        UnitListKind::MobileAir,
        UnitListKind::GroundCover,
    ] {
        for id in ctx.list(kind) {
            let Some(info) = ctx.unit(id) else { continue };

            if info.team != team || info.unit_type != unit_type || info.hits == 0 {
                continue;
            }

            let distance = distance_squared(info.grid, from);

            if best
                .map(|(best_distance, _)| distance < best_distance)
                .unwrap_or(true)
            {
                best = Some((distance, id));
            }
        }
    }

    best.map(|(_, id)| id)
}

/// Ремонтный цех для типа юнита (`TaskRepair::GetRepairShopType`).
fn repair_shop_type(unit_type: UnitType) -> UnitType {
    let flags = crate::access::unit_type_flags(unit_type);

    if flags & unit_flags::MOBILE_AIR_UNIT != 0 {
        UnitType::Hangar
    } else if flags & unit_flags::MOBILE_SEA_UNIT != 0 {
        UnitType::Dock
    } else {
        UnitType::Depot
    }
}

/// Склад-источник ресурсов (`TaskGetMaterials::FindBuilding`).
fn find_source_building(ctx: &TaskCtx<'_>, team: crate::team::TeamId) -> Option<UnitId> {
    for kind in [UnitType::ADump, UnitType::Depot, UnitType::MiningStation] {
        for id in ctx.list(UnitListKind::Stationary) {
            let Some(info) = ctx.unit(id) else { continue };
            if info.team == team && info.unit_type == kind && info.orders != UnitOrder::Idle {
                return Some(id);
            }
        }
    }

    None
}

/// Грузовик-снабженец (`TaskGetMaterials::FindTruck`).
fn find_truck(ctx: &TaskCtx<'_>, team: crate::team::TeamId) -> Option<UnitId> {
    for id in ctx.list(UnitListKind::MobileLandSea) {
        let Some(info) = ctx.unit(id) else { continue };
        if info.team == team
            && matches!(
                info.unit_type,
                UnitType::SupplyTruck | UnitType::FuelTruck | UnitType::GoldTruck
            )
            && info.hits > 0
        {
            return Some(id);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// TaskGetMaterials / TaskGetResource
// ---------------------------------------------------------------------------

fn get_materials_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::GetMaterials {
        resource,
        materials_needed,
    } = kind
    else {
        return;
    };

    let Some(requestor) = resource.requestor else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(requestor_info) = ctx.unit(requestor) else {
        ctx.remove_self(header.id);
        return;
    };

    let team = requestor_info.team;

    // `FindTruck` — назначить грузовик.
    if resource.supplier.is_none() {
        match find_truck(ctx, team) {
            Some(truck) => {
                resource.supplier = Some(truck);
                ctx.attach(header.id, truck);
            }
            None => {
                // Нет грузовика: заказать `SPLYTRCK` через `TaskObtainUnits`.
                if !resource.started {
                    resource.started = true;
                    let mut obtain = TaskKind::obtain_units(requestor_info.grid);
                    super::create::obtain_units_add_type(&mut obtain, UnitType::SupplyTruck);
                    ctx.append_task(team, Some(header.id), TASK_PRIORITY_GET_RESOURCE, obtain);
                }

                return;
            }
        }
    }

    let Some(supplier) = resource.supplier else {
        return;
    };

    // `FindBuilding` — источник ресурсов.
    if resource.source.is_none() {
        resource.source = find_source_building(ctx, team);
    }

    let Some(source) = resource.source else {
        // Источника нет — доставить напрямую (структурно).
        return;
    };

    let Some(supplier_info) = ctx.unit(supplier) else {
        resource.supplier = None;
        return;
    };

    let Some(source_info) = ctx.unit(source) else {
        resource.source = None;
        return;
    };

    // У источника: загрузиться (приказ `ORDER_TRANSFER`).
    if distance_squared(supplier_info.grid, source_info.grid) <= 2 {
        if i32::from(supplier_info.storage) < i32::from(*materials_needed) {
            if supplier_info.orders == UnitOrder::Await {
                if let Some(mut supplier_info) = ctx.unit(supplier) {
                    supplier_info.transfer_cargo = *materials_needed as i16;
                    supplier_info.parent_unit = Some(source);
                    ctx.write_unit(supplier, supplier_info);
                }

                ctx.set_new_order(supplier, UnitOrder::Transfer, UnitOrderState::Init);
            }
        } else {
            // Везём к заказчику.
            if supplier_info.orders == UnitOrder::Await {
                let priority = ctx.priority_of(header.id);
                let move_kind = TaskKind::move_to(
                    supplier,
                    requestor_info.grid,
                    2,
                    CautionLevel::AvoidNextTurnsFire,
                );
                ctx.append_task(team, Some(header.id), priority, move_kind);
            }
        }

        return;
    }

    // Двигаться к источнику.
    if supplier_info.orders == UnitOrder::Await || supplier_info.orders == UnitOrder::Sentry {
        let priority = ctx.priority_of(header.id);
        let move_kind = TaskKind::move_to(
            supplier,
            source_info.grid,
            2,
            CautionLevel::AvoidNextTurnsFire,
        );
        ctx.append_task(team, Some(header.id), priority, move_kind);
    }
}

fn get_resource_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    // `TaskGetResource` — база: параметры совпадают с `TaskGetMaterials`.
    if matches!(kind, TaskKind::GetMaterials { .. }) {
        get_materials_end_turn(kind, ctx, header);
    }
}
// ---------------------------------------------------------------------------
// TaskRepair / TaskReload / TaskUpgrade
// ---------------------------------------------------------------------------

/// `TaskRepair::EndTurn` (база для `Reload`/`Upgrade`): доставить юнит в цех.
fn repair_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let (repair, issue_order) = match kind {
        TaskKind::Repair { repair } => (repair, UnitOrder::Repair),
        TaskKind::Reload { repair } => (repair, UnitOrder::Reload),
        TaskKind::Upgrade { repair } => (repair, UnitOrder::Upgrade),
        _ => return,
    };

    let Some(target) = repair.target_unit else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(target_info) = ctx.unit(target) else {
        ctx.remove_self(header.id);
        return;
    };

    let team = target_info.team;
    let shop_type = repair_shop_type(target_info.unit_type);

    // `SelectRepairShop` — цех команды.
    if repair.operator_unit.is_none() {
        match find_closest_unit_of_type(ctx, team, shop_type, target_info.grid) {
            Some(shop) => {
                repair.operator_unit = Some(shop);
            }
            None => {
                // Нет цеха: заказать постройку (структурно, через менеджер).
                ctx.remove_self(header.id);
                return;
            }
        }
    }

    let Some(shop) = repair.operator_unit else {
        return;
    };

    let Some(shop_info) = ctx.unit(shop) else {
        repair.operator_unit = None;
        return;
    };

    // Рядом с цехом — отдать приказ цеху.
    if distance_squared(target_info.grid, shop_info.grid) <= 2 {
        if shop_info.orders == UnitOrder::Await || shop_info.orders == UnitOrder::Sentry {
            if let Some(mut shop_info) = ctx.unit(shop) {
                shop_info.parent_unit = Some(target);
                ctx.write_unit(shop, shop_info);
            }

            ctx.set_new_order(shop, issue_order, UnitOrderState::Init);
        }

        // Цель остаётся в задаче до конца ремонта.
        let finished = match kind {
            TaskKind::Reload { .. } => {
                target_info.ammo as i32
                    >= target_info
                        .base_values
                        .get_attribute(crate::unit_values::ATTRIB_ROUNDS)
            }
            TaskKind::Upgrade { .. } => false,
            _ => !task_is_repair_due(&target_info, false),
        };

        if finished {
            ctx.clear_unit_tasks(shop, false);
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
        }

        return;
    }

    // Двигаться к цеху.
    if target_info.orders == UnitOrder::Await || target_info.orders == UnitOrder::Sentry {
        let priority = ctx.priority_of(header.id);
        let move_kind = TaskKind::move_to(target, shop_info.grid, 2, CautionLevel::AvoidAllDamage);
        ctx.append_task(team, Some(header.id), priority, move_kind);
    }
}
// ---------------------------------------------------------------------------
// TaskRemoveRubble / TaskRemoveMines
// ---------------------------------------------------------------------------

/// Тип юнита-уборщика для цели.
fn clearer_type_for(target: &crate::unit::UnitInfo) -> UnitType {
    if target.unit_type == UnitType::LandMine || target.unit_type == UnitType::SeaMine {
        UnitType::MineLayer
    } else {
        UnitType::Bulldozer
    }
}

fn remove_rubble_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let state = match kind {
        TaskKind::RemoveRubble(state) => state,
        TaskKind::RemoveMines { rubble } => rubble,
        _ => return,
    };

    let Some(target) = state.target else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(target_info) = ctx.unit(target) else {
        ctx.remove_self(header.id);
        return;
    };

    let team = target_info.team;
    let clearer_type = clearer_type_for(&target_info);

    if state.unit.is_none() {
        match find_closest_unit_of_type(ctx, team, clearer_type, target_info.grid) {
            Some(unit) => {
                state.unit = Some(unit);
                ctx.attach(header.id, unit);
            }
            None => {
                if !state.unit_requested {
                    state.unit_requested = true;
                    let mut obtain = TaskKind::obtain_units(target_info.grid);
                    super::create::obtain_units_add_type(&mut obtain, clearer_type);
                    ctx.append_task(team, Some(header.id), TASK_PRIORITY_REMOVE_RUBBLE, obtain);
                }
                return;
            }
        }
    }

    let Some(clearer) = state.unit else { return };
    let Some(clearer_info) = ctx.unit(clearer) else {
        state.unit = None;
        return;
    };

    if distance_squared(clearer_info.grid, target_info.grid) <= 1 {
        if clearer_info.orders == UnitOrder::Await {
            if let Some(mut clearer_info) = ctx.unit(clearer) {
                clearer_info.target_grid = target_info.grid;
                ctx.write_unit(clearer, clearer_info);
            }

            let mut data = ctx.order_data(clearer);
            data.move_to = target_info.grid;
            ctx.set_order_data(clearer, data);

            ctx.set_new_order(clearer, UnitOrder::Clear, UnitOrderState::Init);
        }
    } else if clearer_info.orders == UnitOrder::Await || clearer_info.orders == UnitOrder::Sentry {
        let priority = ctx.priority_of(header.id);
        let move_kind =
            TaskKind::move_to(clearer, target_info.grid, 1, CautionLevel::AvoidAllDamage);
        ctx.append_task(team, Some(header.id), priority, move_kind);
    }
}

// ---------------------------------------------------------------------------
// TaskScavenge
// ---------------------------------------------------------------------------

fn scavenge_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Scavenge { units, .. } = kind else {
        return;
    };

    units.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));

    // Рекрутинг инженеров/бульдозеров.
    if units.is_empty() {
        for id in ctx.list(UnitListKind::MobileLandSea) {
            let Some(info) = ctx.unit(id) else { continue };
            if info.team == header.team
                && matches!(info.unit_type, UnitType::Engineer | UnitType::Bulldozer)
                && crate::ai::task::task_is_recruitable(
                    ctx.tm,
                    ctx.world,
                    id,
                    header.team,
                    TASK_PRIORITY_SCAVENGE,
                )
            {
                units.push(id);
                ctx.attach(header.id, id);
            }
        }

        if units.is_empty() {
            ctx.remove_self(header.id);
            return;
        }
    }

    // Обломки на карте (`TaskScavenge`).
    let mut site: Option<Point> = None;
    for id in ctx.list(UnitListKind::Stationary) {
        let Some(info) = ctx.unit(id) else { continue };
        if matches!(
            info.unit_type,
            UnitType::LargeRubble | UnitType::SmallRubble
        ) {
            site = Some(info.grid);
            break;
        }
    }

    if let Some(target) = site {
        for unit in units.clone() {
            let Some(info) = ctx.unit(unit) else { continue };

            if info.orders != UnitOrder::Await && info.orders != UnitOrder::Sentry {
                continue;
            }

            let priority = ctx.priority_of(header.id);
            let move_kind = TaskKind::move_to(unit, target, 1, CautionLevel::AvoidNextTurnsFire);
            ctx.append_task(info.team, Some(header.id), priority, move_kind);
        }
    }
}

/// `TaskScavenge::IsUnitUsable`.
pub(crate) fn scavenge_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    info.team == header.team
        && matches!(info.unit_type, UnitType::Engineer | UnitType::Bulldozer)
        && info.hits > 0
}

// ---------------------------------------------------------------------------
// TaskPlaceMines
// ---------------------------------------------------------------------------

fn place_mines_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::PlaceMines {
        units,
        mine_layer_count,
        sea_mine_layer_count,
    } = kind
    else {
        return;
    };

    units.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));

    let mut layers = 0;
    let mut sea_layers = 0;

    for id in ctx.list(UnitListKind::MobileLandSea) {
        let Some(info) = ctx.unit(id) else { continue };
        if info.team != header.team {
            continue;
        }

        match info.unit_type {
            UnitType::MineLayer => layers += 1,
            UnitType::SeaMineLayer => sea_layers += 1,
            _ => {}
        }
    }

    *mine_layer_count = layers;
    *sea_mine_layer_count = sea_layers;

    // Сайт минирования: первый фронт (`INFO_MAP_FRONTIER`) карты информации.
    let player = &ctx.world.ai.players[header.team.0 as usize];
    let mut frontier: Option<Point> = None;

    for y in 0..crate::geo::MAP_HEIGHT {
        for x in 0..crate::geo::MAP_WIDTH {
            let index = y as usize * crate::geo::MAP_WIDTH as usize + x as usize;
            if player.info_map[index] & crate::enums::INFO_MAP_FRONTIER != 0 {
                frontier = Some(Point::new(x, y));
                break;
            }
        }
        if frontier.is_some() {
            break;
        }
    }

    let Some(site) = frontier else {
        return;
    };

    for unit in units.clone() {
        let Some(info) = ctx.unit(unit) else { continue };

        if info.orders != UnitOrder::Await && info.orders != UnitOrder::Sentry {
            continue;
        }

        let priority = ctx.priority_of(header.id);
        let move_kind = TaskKind::move_to(unit, site, 1, CautionLevel::AvoidNextTurnsFire);
        ctx.append_task(info.team, Some(header.id), priority, move_kind);
    }
}

/// `TaskPlaceMines::IsUnitUsable`.
pub(crate) fn place_mines_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    info.team == header.team
        && matches!(info.unit_type, UnitType::MineLayer | UnitType::SeaMineLayer)
        && info.hits > 0
}

// ---------------------------------------------------------------------------
// TaskClearZone
// ---------------------------------------------------------------------------

/// `CLEARZONE_STATE_*` (`taskclearzone.hpp`).
pub mod clear_zone_state {
    /// `CLEARZONE_STATE_WAITING`.
    pub const WAITING: u8 = 0;
    /// `CLEARZONE_STATE_EXAMINING_ZONES`.
    pub const EXAMINING_ZONES: u8 = 1;
    /// `CLEARZONE_STATE_SEARCHING_MAP`.
    pub const SEARCHING_MAP: u8 = 2;
    /// `CLEARZONE_STATE_WAITING_FOR_PATH`.
    pub const WAITING_FOR_PATH: u8 = 3;
    /// `CLEARZONE_STATE_MOVING_UNIT`.
    pub const MOVING_UNIT: u8 = 4;
}

fn clear_zone_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::ClearZone {
        state,
        zones,
        moving_unit,
    } = kind
    else {
        return;
    };

    if zones.is_empty() {
        ctx.remove_self(header.id);
        return;
    }

    *state = clear_zone_state::EXAMINING_ZONES;

    let site = zones[0];
    let occupant = ctx.world.units_at(site).iter().copied().find(|id| {
        ctx.unit(*id)
            .map(|info| {
                info.team != header.team
                    || matches!(
                        info.unit_type,
                        UnitType::LargeRubble | UnitType::SmallRubble | UnitType::LandMine
                    )
            })
            .unwrap_or(false)
    });

    match occupant {
        Some(target) => {
            *state = clear_zone_state::MOVING_UNIT;

            let existing = moving_unit.and_then(|id| ctx.unit(id)).is_some();
            if !existing {
                *moving_unit = None;

                let priority = TASK_PRIORITY_REMOVE_RUBBLE_LOW;
                let kind_task = TaskKind::RemoveRubble(super::RemoveRubbleState {
                    unit: None,
                    target: Some(target),
                    site,
                    unit_requested: false,
                });
                ctx.append_task(header.team, Some(header.id), priority, kind_task);
            }
        }
        None => {
            zones.remove(0);
        }
    }
}

// ---------------------------------------------------------------------------
// TaskCheckAssaults
// ---------------------------------------------------------------------------

fn check_assaults_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::CheckAssaults { active, .. } = kind else {
        return;
    };

    *active = ctx
        .list(UnitListKind::MobileLandSea)
        .into_iter()
        .chain(ctx.list(UnitListKind::MobileAir))
        .any(|id| {
            ctx.unit(id)
                .map(|info| info.team == header.team && info.orders == UnitOrder::MoveToAttack)
                .unwrap_or(false)
        });

    let _ = TASK_PRIORITY_CHECK_ASSAULTS;
}

// ---------------------------------------------------------------------------
// Диспетчеризация
// ---------------------------------------------------------------------------

/// `Init` для misc-классов.
pub fn init(kind: &mut TaskKind, _ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    match kind {
        TaskKind::GetMaterials { .. } => true,
        TaskKind::Repair { .. } | TaskKind::Reload { .. } | TaskKind::Upgrade { .. } => true,
        TaskKind::RemoveRubble(_) | TaskKind::RemoveMines { .. } => true,
        TaskKind::PlaceMines { .. } => true,
        TaskKind::Scavenge { .. } => true,
        TaskKind::ClearZone { .. } => true,
        TaskKind::CheckAssaults { .. } => true,
        _ => false,
    }
}

/// `BeginTurn` для misc-классов.
pub fn begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::GetMaterials { .. } => {
            get_resource_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Repair { .. } | TaskKind::Reload { .. } | TaskKind::Upgrade { .. } => {
            repair_end_turn(kind, ctx, header);
            true
        }
        TaskKind::RemoveRubble(_) | TaskKind::RemoveMines { .. } => {
            remove_rubble_end_turn(kind, ctx, header);
            true
        }
        TaskKind::PlaceMines { .. } => {
            place_mines_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Scavenge { .. } => {
            scavenge_end_turn(kind, ctx, header);
            true
        }
        TaskKind::ClearZone { .. } => {
            clear_zone_end_turn(kind, ctx, header);
            true
        }
        TaskKind::CheckAssaults { .. } => {
            check_assaults_begin_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `EndTurn` для misc-классов.
pub fn end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::GetMaterials { .. } => {
            get_resource_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Repair { .. } | TaskKind::Reload { .. } | TaskKind::Upgrade { .. } => {
            repair_end_turn(kind, ctx, header);
            true
        }
        TaskKind::RemoveRubble(_) | TaskKind::RemoveMines { .. } => {
            remove_rubble_end_turn(kind, ctx, header);
            true
        }
        TaskKind::PlaceMines { .. } => {
            place_mines_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Scavenge { .. } => {
            scavenge_end_turn(kind, ctx, header);
            true
        }
        TaskKind::ClearZone { .. } => {
            clear_zone_end_turn(kind, ctx, header);
            true
        }
        TaskKind::CheckAssaults { .. } => {
            check_assaults_begin_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `Execute(unit)` для misc-классов.
pub fn execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::GetMaterials { .. } => {
            get_resource_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Repair { .. } | TaskKind::Reload { .. } | TaskKind::Upgrade { .. } => {
            let _ = unit;
            repair_end_turn(kind, ctx, header);
            true
        }
        TaskKind::RemoveRubble(_) | TaskKind::RemoveMines { .. } => {
            let _ = unit;
            remove_rubble_end_turn(kind, ctx, header);
            true
        }
        TaskKind::ClearZone { .. } => {
            let _ = unit;
            clear_zone_end_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `AddUnit(unit)` для misc-классов.
pub fn add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::GetMaterials { resource, .. } => {
            if resource.supplier.is_none() {
                resource.supplier = Some(unit);
                ctx.attach(header.id, unit);
            }
        }
        TaskKind::Repair { repair }
        | TaskKind::Reload { repair }
        | TaskKind::Upgrade { repair } => {
            if repair.operator_unit.is_none() {
                repair.operator_unit = Some(unit);
                ctx.attach(header.id, unit);
            }
        }
        TaskKind::RemoveRubble(state) => {
            if state.unit.is_none() {
                state.unit = Some(unit);
                ctx.attach(header.id, unit);
            }
        }
        TaskKind::RemoveMines { rubble } => {
            if rubble.unit.is_none() {
                rubble.unit = Some(unit);
                ctx.attach(header.id, unit);
            }
        }
        TaskKind::PlaceMines { units, .. } | TaskKind::Scavenge { units, .. } => {
            if !units.contains(&unit) {
                units.push(unit);
                ctx.attach(header.id, unit);
            }
        }
        _ => {}
    }
}

/// `RemoveUnit(unit)` для misc-классов.
pub fn remove_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::GetMaterials { resource, .. } => {
            if resource.supplier == Some(unit) {
                resource.supplier = None;
            }
        }
        TaskKind::Repair { repair }
        | TaskKind::Reload { repair }
        | TaskKind::Upgrade { repair } => {
            if repair.target_unit == Some(unit) {
                ctx.remove_self(header.id);
            } else if repair.operator_unit == Some(unit) {
                repair.operator_unit = None;
            }
        }
        TaskKind::RemoveRubble(state) => {
            if state.unit == Some(unit) {
                state.unit = None;
            }
            if state.target == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::RemoveMines { rubble } => {
            if rubble.unit == Some(unit) {
                rubble.unit = None;
            }
            if rubble.target == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::PlaceMines { units, .. } | TaskKind::Scavenge { units, .. } => {
            units.retain(|id| *id != unit);
        }
        TaskKind::ClearZone { moving_unit, .. } => {
            if *moving_unit == Some(unit) {
                *moving_unit = None;
            }
        }
        _ => {}
    }
}

/// `RemoveSelf()` для misc-классов.
pub fn remove_self(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::GetMaterials { .. }
        | TaskKind::Repair { .. }
        | TaskKind::Reload { .. }
        | TaskKind::Upgrade { .. }
        | TaskKind::RemoveRubble(_)
        | TaskKind::RemoveMines { .. }
        | TaskKind::PlaceMines { .. }
        | TaskKind::Scavenge { .. }
        | TaskKind::ClearZone { .. }
        | TaskKind::CheckAssaults { .. } => {
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        _ => false,
    }
}

/// `ChildComplete(task)` для misc-классов.
pub fn child_complete(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    child: u32,
) -> bool {
    match kind {
        TaskKind::GetMaterials { .. }
        | TaskKind::Repair { .. }
        | TaskKind::Reload { .. }
        | TaskKind::Upgrade { .. }
        | TaskKind::RemoveRubble(_)
        | TaskKind::RemoveMines { .. }
        | TaskKind::PlaceMines { .. }
        | TaskKind::Scavenge { .. }
        | TaskKind::ClearZone { .. }
        | TaskKind::CheckAssaults { .. } => {
            let _ = (ctx, header, child);
            true
        }
        _ => false,
    }
}

/// Уведомление о завершении дочернего `TaskMove`.
pub fn move_finished(
    kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    unit: UnitId,
    result: i8,
) -> bool {
    match kind {
        TaskKind::GetMaterials { .. }
        | TaskKind::Repair { .. }
        | TaskKind::Reload { .. }
        | TaskKind::Upgrade { .. }
        | TaskKind::RemoveRubble(_)
        | TaskKind::RemoveMines { .. }
        | TaskKind::PlaceMines { .. }
        | TaskKind::Scavenge { .. } => {
            let _ = (unit, result);
            true
        }
        TaskKind::ClearZone { .. } => {
            let _ = (unit, result);
            true
        }
        _ => false,
    }
}

/// `CheckReactions` — не переопределён.
pub fn check_reactions(_kind: &mut TaskKind, _ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    false
}

/// `ExchangeOperator` — `TaskRepair` (обмен цеха).
pub fn exchange_operator(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let repair = match kind {
        TaskKind::Repair { repair }
        | TaskKind::Reload { repair }
        | TaskKind::Upgrade { repair } => repair,
        _ => return false,
    };

    let Some(current) = repair.operator_unit else {
        return false;
    };

    let Some(candidate) = ctx.unit(unit) else {
        return false;
    };

    let Some(current_info) = ctx.unit(current) else {
        repair.operator_unit = Some(unit);
        return true;
    };

    if candidate.unit_type != current_info.unit_type {
        return false;
    }

    let target_grid = repair
        .target_unit
        .and_then(|target| ctx.unit(target))
        .map(|info| info.grid)
        .unwrap_or(Point::ZERO);

    if distance_squared(candidate.grid, target_grid)
        >= distance_squared(current_info.grid, target_grid)
    {
        return false;
    }

    ctx.clear_unit_tasks(current, false);
    repair.operator_unit = Some(unit);
    ctx.attach(header.id, unit);

    true
}

/// `EventUnitDestroyed(unit)` для misc-классов.
pub fn event_unit_destroyed(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::GetMaterials { resource, .. } => {
            if resource.requestor == Some(unit) || resource.supplier == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::Repair { repair }
        | TaskKind::Reload { repair }
        | TaskKind::Upgrade { repair } => {
            if repair.target_unit == Some(unit) || repair.operator_unit == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::RemoveRubble(state) => {
            if state.target == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::RemoveMines { rubble } => {
            if rubble.target == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::Scavenge { units, .. } | TaskKind::PlaceMines { units, .. } => {
            units.retain(|id| *id != unit);
            true
        }
        TaskKind::ClearZone { moving_unit, .. } => {
            if *moving_unit == Some(unit) {
                *moving_unit = None;
            }
            true
        }
        _ => false,
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

/// `EventCargoTransfer(unit)` — `TaskGetMaterials::EventCargoTransfer`.
pub fn event_cargo_transfer(
    kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::GetMaterials { resource, .. } => {
            if resource.supplier == Some(unit) || resource.requestor == Some(unit) {}
            true
        }
        _ => false,
    }
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

/// Приоритеты, использованные модулем (экспорт для тестов).
pub const MISC_PRIORITIES: [u16; 8] = [
    TASK_PRIORITY_EMERGENCY,
    TASK_PRIORITY_REMOVE_RUBBLE,
    TASK_PRIORITY_REMOVE_RUBBLE_LOW,
    TASK_PRIORITY_REPAIR,
    TASK_PRIORITY_AUTO_SURVEY,
    TASK_PRIORITY_GET_RESOURCE,
    TASK_PRIORITY_SCAVENGE,
    TASK_PRIORITY_CHECK_ASSAULTS,
];
