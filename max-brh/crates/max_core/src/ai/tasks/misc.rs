//! Economy, repair and clearing: `TaskGetResource`/`TaskGetMaterials`,
//! `TaskRepair`/`TaskReload`/`TaskUpgrade`, `TaskRemoveRubble`/
//! `TaskRemoveMines`, `TaskPlaceMines`, `TaskScavenge`, `TaskClearZone`,
//! `TaskCheckAssaults`.
//!
//! Oracle: `taskgetresource.cpp/.hpp`, `taskgetmaterials.cpp/.hpp`,
//! `taskrepair.cpp/.hpp`, `taskreload.cpp/.hpp`, `taskupgrade.cpp/.hpp`,
//! `taskremoverubble.cpp/.hpp`, `taskremovemines.cpp/.hpp`,
//! `taskplacemines.cpp/.hpp`, `taskscavenge.cpp/.hpp`,
//! `taskclearzone.cpp/.hpp`, `taskcheckassaults.cpp/.hpp`.
//!
//! # P6 simplifications (P3 economy and P5 zone marking)
//!
//! * cargo transfer (`TaskGetMaterials::DoTransfer`) is performed by the
//!   `ORDER_TRANSFER` order — the transfer itself is implemented by complex
//!   economy (P3), so here only the route and the order;
//! * repair/reload/upgrade issue `ORDER_REPAIR`/`ORDER_RELOAD`/
//!   `ORDER_UPGRADE` to the repair shop (P3 production);
//! * `TaskClearZone` walks the claimed sites and issues `ORDER_CLEAR` to
//!   the bulldozer; full `Zone` marking (P5/P7) is not ported;
//! * `TaskCheckAssaults` monitors assault groups by `ORDER_MOVE_TO_ATTACK`
//!   orders (full air assault checks — P7).

use crate::classification::UnitTypeClassify;
use crate::enums::{CautionLevel, UnitOrder, UnitOrderState, UnitType};
use crate::geo::Point;
use crate::unit::{UnitId, UnitListKind};

use super::super::task::{
    task_is_repair_due, TaskHeader, TASK_PRIORITY_AUTO_SURVEY, TASK_PRIORITY_CHECK_ASSAULTS,
    TASK_PRIORITY_EMERGENCY, TASK_PRIORITY_GET_RESOURCE, TASK_PRIORITY_REMOVE_RUBBLE,
    TASK_PRIORITY_REMOVE_RUBBLE_LOW, TASK_PRIORITY_REPAIR, TASK_PRIORITY_SCAVENGE,
};
use super::{TaskCtx, TaskKind};

// ---------------------------------------------------------------------------
// Common searches
// ---------------------------------------------------------------------------

/// Squared distance.
fn distance_squared(first: Point, second: Point) -> i32 {
    let dx = i32::from(first.x) - i32::from(second.x);
    let dy = i32::from(first.y) - i32::from(second.y);
    dx * dx + dy * dy
}

/// The nearest team unit with the given type (`FindClosestBuilding`).
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

/// Repair shop for a unit type (`TaskRepair::GetRepairShopType`).
fn repair_shop_type(unit_type: UnitType) -> UnitType {
    if unit_type.is_air() {
        UnitType::Hangar
    } else if unit_type.is_sea() || unit_type.is_amphibious() {
        UnitType::Dock
    } else {
        UnitType::Depot
    }
}

/// Source of raw material — `TaskGetMaterials::FindBuilding`
/// (`taskgetmaterials.cpp:211`): the nearest stationary building of the team
/// that holds raw material (`storage > 0`).
fn find_source_building(
    ctx: &TaskCtx<'_>,
    team: crate::team::TeamId,
    requestor: UnitId,
) -> Option<UnitId> {
    let requestor_grid = ctx.unit(requestor).map(|info| info.grid)?;
    let mut best: Option<(i32, UnitId)> = None;

    for id in ctx.list(UnitListKind::Stationary) {
        let Some(info) = ctx.unit(id) else { continue };

        if info.team != team
            || info.hits == 0
            || info.storage <= 0
            || id == requestor
            || crate::ai::tasks::create::cargo_type_of(info.unit_type)
                != Some(crate::ai::tasks::create::CargoType::Raw)
        {
            continue;
        }

        let distance = distance_squared(info.grid, requestor_grid);

        if best
            .map(|(best_distance, _)| distance < best_distance)
            .unwrap_or(true)
        {
            best = Some((distance, id));
        }
    }

    best.map(|(_, id)| id)
}

/// Truck for the material — `TaskGetMaterials::FindTruck`
/// (`taskgetmaterials.cpp:239`): any mobile land/sea unit of the team that
/// carries raw material and waits for orders (`ORDER_AWAIT`, or a stopped
/// `ORDER_MOVE`), other than the requestor itself.
fn find_truck(ctx: &TaskCtx<'_>, team: crate::team::TeamId, requestor: UnitId) -> Option<UnitId> {
    let requestor_grid = ctx.unit(requestor).map(|info| info.grid)?;
    let mut best: Option<(i32, UnitId)> = None;

    for id in ctx.list(UnitListKind::MobileLandSea) {
        let Some(info) = ctx.unit(id) else { continue };

        if info.team != team
            || id == requestor
            || info.hits == 0
            || info.storage <= 0
            || crate::ai::tasks::create::cargo_type_of(info.unit_type)
                != Some(crate::ai::tasks::create::CargoType::Raw)
            || !(info.orders == UnitOrder::Await
                || (info.orders == UnitOrder::Move && info.speed == 0))
        {
            continue;
        }

        let distance = distance_squared(info.grid, requestor_grid);

        if best
            .map(|(best_distance, _)| distance < best_distance)
            .unwrap_or(true)
        {
            best = Some((distance, id));
        }
    }

    best.map(|(_, id)| id)
}

// ---------------------------------------------------------------------------
// TaskGetMaterials / TaskGetResource
// ---------------------------------------------------------------------------

/// `TaskGetMaterials::EndTurn` + `TaskGetResource::ChooseSource`/`EndTurn`
/// (`taskgetmaterials.cpp:99`, `taskgetresource.cpp:24/103`).
///
/// The requestor is the unit that needs raw material (a mobile builder of a
/// `TaskCreateBuilding`); `source` is a stationary raw storage of the team
/// (`FindBuilding`) and `supplier` is the unit that carries the material — a
/// truck (`FindTruck`) or, when the team has none, the source building itself
/// (`ChooseSource`: `if (!supplier) supplier = unit`). An adjacent building
/// transfers straight into the requestor (`DoTransfer`), otherwise the
/// requestor walks to it (`TaskRendezvous`).
///
/// This is the branch the AI needs to keep a builder going once the team's raw
/// material sits in a dump instead of the builder's own cargo: without it a
/// builder with an empty `storage` waited forever in
/// `CREATE_BUILDING_STATE_GETTING_MATERIALS` while the base's complex was full.
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
    let needed = i32::from(*materials_needed);

    // `TaskGetMaterials::EndTurn`: already on hand — the task is done
    // (`EventCargoTransfer` → `requestor->RemoveTask(this)`).
    if i32::from(requestor_info.storage) >= needed {
        ctx.set_parent(header.id, None);
        ctx.remove_self(header.id);
        return;
    }

    // `ChooseSource` → `FindBuilding` + `FindTruck`.
    if resource.source.is_none() {
        resource.source = find_source_building(ctx, team, requestor);
    }

    let Some(source) = resource.source else {
        return;
    };

    if resource.supplier.is_none() {
        resource.supplier = find_truck(ctx, team, requestor).or(Some(source));
    }

    let Some(supplier) = resource.supplier else {
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

    // The building itself delivers (`supplier == source`): wait for adjacency,
    // then transfer the missing amount straight into the requestor.
    if supplier == source {
        if distance_squared(requestor_info.grid, source_info.grid) <= 2 {
            if ctx.world.is_active_turn(team) {
                do_transfer(ctx, source, requestor, needed);
            }
        } else {
            let priority = ctx.priority_of(header.id);
            let rendezvous = TaskKind::Rendezvous {
                unit1: Some(source),
                unit2: Some(requestor),
                mode: super::RendezvousMode::Failed,
            };
            ctx.append_task(team, Some(header.id), priority, rendezvous);
        }

        return;
    }

    // At the source: load (`ORDER_TRANSFER` order).
    if distance_squared(supplier_info.grid, source_info.grid) <= 2 {
        if i32::from(supplier_info.storage) < needed {
            if supplier_info.orders == UnitOrder::Await {
                if let Some(mut supplier_info) = ctx.unit(supplier) {
                    supplier_info.transfer_cargo = *materials_needed as i16;
                    supplier_info.parent_unit = Some(source);
                    ctx.write_unit(supplier, supplier_info);
                }

                ctx.set_new_order(supplier, UnitOrder::Transfer, UnitOrderState::Init);
            }
        } else {
            // Transport to the requestor.
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

    // Move to the source.
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

/// `TaskGetMaterials::DoTransfer` (`taskgetmaterials.cpp:158`): the source hands
/// the missing material to the requestor.
///
/// A stationary source pays from its **complex stock** (`GetComplex()->GetCargoInfo`),
/// a mobile one from its own `storage`; the amount is capped by the requestor's
/// free `ATTRIB_STORAGE`. The transfer itself is the regular `ORDER_TRANSFER`
/// order of the source with the requestor as its parent.
fn do_transfer(
    ctx: &mut TaskCtx<'_>,
    source: UnitId,
    requestor: UnitId,
    materials_needed: i32,
) {
    let Some(source_info) = ctx.unit(source) else {
        return;
    };

    let Some(requestor_info) = ctx.unit(requestor) else {
        return;
    };

    if source_info.orders == UnitOrder::Transfer {
        return;
    }

    let capacity = i32::from(
        requestor_info
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_STORAGE),
    );
    let storage = i32::from(requestor_info.storage);

    let available = match source_info.complex {
        Some(complex) => ctx
            .world
            .complex(source_info.team, complex.0 as u16)
            .map(|complex| i32::from(complex.raw))
            .unwrap_or(0),
        None => i32::from(source_info.storage),
    };

    let amount = (materials_needed - storage)
        .min(capacity - storage)
        .min(available);

    if amount <= 0 {
        return;
    }

    if let Some(mut source_info) = ctx.unit(source) {
        source_info.transfer_cargo = amount as i16;
        source_info.parent_unit = Some(requestor);
        ctx.write_unit(source, source_info);
    }

    ctx.set_new_order(source, UnitOrder::Transfer, UnitOrderState::Init);
}

fn get_resource_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    // `TaskGetResource` — base: parameters match `TaskGetMaterials`.
    if matches!(kind, TaskKind::GetMaterials { .. }) {
        get_materials_end_turn(kind, ctx, header);
    }
}
// ---------------------------------------------------------------------------
// TaskRepair / TaskReload / TaskUpgrade
// ---------------------------------------------------------------------------

/// `TaskRepair::EndTurn` (base for `Reload`/`Upgrade`): deliver the unit to the shop.
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

    // `SelectRepairShop` — the team's shop.
    if repair.operator_unit.is_none() {
        match find_closest_unit_of_type(ctx, team, shop_type, target_info.grid) {
            Some(shop) => {
                repair.operator_unit = Some(shop);
            }
            None => {
                // No shop: order a build (structurally, via the manager).
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

    // Next to the shop — issue the order to the shop.
    if distance_squared(target_info.grid, shop_info.grid) <= 2 {
        if shop_info.orders == UnitOrder::Await || shop_info.orders == UnitOrder::Sentry {
            if let Some(mut shop_info) = ctx.unit(shop) {
                shop_info.parent_unit = Some(target);
                ctx.write_unit(shop, shop_info);
            }

            ctx.set_new_order(shop, issue_order, UnitOrderState::Init);
        }

        // The target stays in the task until the repair ends.
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

    // Move to the shop.
    if target_info.orders == UnitOrder::Await || target_info.orders == UnitOrder::Sentry {
        let priority = ctx.priority_of(header.id);
        let move_kind = TaskKind::move_to(target, shop_info.grid, 2, CautionLevel::AvoidAllDamage);
        ctx.append_task(team, Some(header.id), priority, move_kind);
    }
}
// ---------------------------------------------------------------------------
// TaskRemoveRubble / TaskRemoveMines
// ---------------------------------------------------------------------------

/// The cleaner unit type for the target.
///
/// `TaskRemoveMines::ObtainUnit` (`taskremovemines.cpp:101`) picks the layer by
/// the modified surface at the mine cell: `MINELAYR` on land, `SEAMNLYR` on
/// water (the sea variant exists exactly for this), while rubble is removed by
/// a `BULLDOZR` (`taskremoverubble.cpp:256`).
fn clearer_type_for(ctx: &TaskCtx<'_>, target: &crate::unit::UnitInfo) -> UnitType {
    if target.unit_type == UnitType::LandMine || target.unit_type == UnitType::SeaMine {
        if crate::access::modified_surface_type(ctx.world, target.grid, false)
            == crate::access::SURFACE_TYPE_LAND
        {
            UnitType::MineLayer
        } else {
            UnitType::SeaMineLayer
        }
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
    let clearer_type = clearer_type_for(ctx, &target_info);

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

    // Recruiting engineers/bulldozers.
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

    // Rubble on the map (`TaskScavenge`).
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

    // Minelaying site: the first frontier (`INFO_MAP_FRONTIER`) of the info map.
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
// Dispatch
// ---------------------------------------------------------------------------

/// `Init` for misc classes.
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

/// `BeginTurn` for misc classes.
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

/// `EndTurn` for misc classes.
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

/// `Execute(unit)` for misc classes.
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

/// `AddUnit(unit)` for misc classes.
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

/// `RemoveUnit(unit)` for misc classes.
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

/// `RemoveSelf()` for misc classes.
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

/// `ChildComplete(task)` for misc classes.
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

/// Notification about a child `TaskMove` completion.
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

/// `CheckReactions` — not overridden.
pub fn check_reactions(_kind: &mut TaskKind, _ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    false
}

/// `ExchangeOperator` — `TaskRepair` (swapping the shop).
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

/// `EventUnitDestroyed(unit)` for misc classes.
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

/// `EventEnemyUnitSpotted` — not overridden.
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

/// Priorities used by the module (exported for tests).
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
