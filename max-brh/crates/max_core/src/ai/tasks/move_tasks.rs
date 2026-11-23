//! Movement: `TaskMove`, `TaskMoveHome`, `TaskFindPath`,
//! `TaskSearchDestination`, `TaskRendezvous`, `TaskAssistMove`,
//! `TaskTransport`, `TaskDump`.
//!
//! Oracle: `taskmove.cpp/.hpp`, `taskmovehome.cpp/.hpp`, `taskfindpath.cpp/.hpp`,
//! `tasksearchdestination.cpp/.hpp`, `taskrendezvous.cpp/.hpp`,
//! `taskassistmove.cpp/.hpp`, `tasktransport.cpp/.hpp`, `taskdump.cpp/.hpp`.
//!
//! # Movement model
//!
//! In C++ `TaskMove` asynchronously requests a path (`TaskPathRequest` →
//! `TaskFindPath`), gets a `GroundPath` and issues an `ORDER_MOVE` with
//! the ready path. In max_core pathfinding is synchronous and deterministic
//! (`pathfinding::request_path` → `poll_results`), and the order machine
//! (`unit_order`) places the request and applies the result itself. Therefore `TaskMove`:
//!
//! 1. checks the distance (`minimum_distance`) — like `TaskMove::Search`;
//! 2. issues `ORDER_MOVE/INIT` with `move_to` (C++ — `ORDER_STATE_IN_PROGRESS` with
//!    a ready path; in the lockstep model this is equivalent, see `unit_order`);
//! 3. waits for `RemindMoveFinished` and repeats the order (`RetryMove`) until the unit
//!    arrives or is blocked;
//! 4. on completion notifies the parent with a [`TASKMOVE_RESULT_*`] code
//!    (C++ `result_callback`).
//!
//! # Transport model
//!
//! `TaskMove::AttemptTransport` (`taskmove.cpp:370`) splits a land move at the
//! water (`transport_route`): the passenger walks to the boarding cell
//! (`transport_waypoint`), waits in the transporter's hold and continues from the
//! drop-off cell (`passenger_destination`) after the release. `TaskTransport`
//! requests the transporter (`TaskObtainUnits`), picks the nearest client, drives
//! the transporter to the sea-side cell next to the boarding/drop-off cell, loads
//! (`ORDER_MOVE_TO_UNIT`) and unloads (`ORDER_ACTIVATE` + `move_to`, the
//! `TaskTransport_FinishTransport` path). The C++ `EventUnitLoaded`/
//! `EventUnitUnloaded` notifications do not exist in the port, so the hold state
//! is polled from `parent_unit`/the stored unit and the released passenger is
//! closed by `transport_cleanup_clients`.
//!
//! Bridges/zones (`EventZoneCleared`) stay structural: they depend on
//! `AccessMap`/`ZoneWalker` zone clearing (P5/P7).

use crate::access;
use crate::enums::{CautionLevel, UnitOrder, UnitOrderState, UnitType};
use crate::geo::{PathStep, Point};
use crate::team::TeamId;
use crate::unit::{GroundPath, UnitId, UnitInfo, UnitListKind, UnitPath};
use crate::unit_values::ATTRIB_STORAGE;

use super::super::task::{
    task_remove_movement_tasks, task_retreat_from_danger, task_should_reserve_shot, TaskHeader,
    TASKMOVE_RESULT_ALREADY_IN_RANGE, TASKMOVE_RESULT_BLOCKED, TASKMOVE_RESULT_CANCELLED,
    TASKMOVE_RESULT_SUCCESS, TASK_PRIORITY_MOVE, TASK_TYPE_TRANSPORT,
};
use super::{TaskCtx, TaskKind};

// ---------------------------------------------------------------------------
// TaskMove
// ---------------------------------------------------------------------------
/// Whether the unit reached the point within `minimum_distance` (squared distance).
pub(crate) fn is_at_destination(
    ctx: &TaskCtx<'_>,
    unit: UnitId,
    destination: Point,
    minimum_distance: u16,
) -> bool {
    let Some(info) = ctx.unit(unit) else {
        return false;
    };

    let dx = i32::from(info.grid.x) - i32::from(destination.x);
    let dy = i32::from(info.grid.y) - i32::from(destination.y);

    dx * dx + dy * dy <= i32::from(minimum_distance)
}

/// `TaskMove::Finished(result)` — notify the parent and remove the task.
pub(crate) fn move_finished_self(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    result: i8,
) {
    let TaskKind::Move(state) = kind else {
        return;
    };

    let unit = state.unit;

    if let Some(unit_id) = unit {
        ctx.detach(header.id, unit_id, false);
    }

    if let (Some(parent), Some(unit_id)) = (header.parent, unit) {
        if ctx.tm.has_task(parent) {
            ctx.tm
                .task_move_finished(ctx.world, parent, unit_id, result);
        }
    }

    state.unit = None;
    ctx.set_parent(header.id, None);
    ctx.remove_self(header.id);
}

/// `TaskMove::Search(mode)` — path or "already in range".
pub(crate) fn move_search(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    _mode: bool,
) {
    let TaskKind::Move(state) = kind else {
        return;
    };

    let Some(unit_id) = state.unit else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(info) = ctx.unit(unit_id) else {
        move_finished_self(kind, ctx, header, TASKMOVE_RESULT_CANCELLED);
        return;
    };

    let dx = i32::from(state.destination.x) - i32::from(info.grid.x);
    let dy = i32::from(state.destination.y) - i32::from(info.grid.y);

    if dx * dx + dy * dy <= i32::from(state.minimum_distance) {
        move_finished_self(kind, ctx, header, TASKMOVE_RESULT_ALREADY_IN_RANGE);
        return;
    }

    // `passenger->move_to_grid = destination_waypoint; move_order_issued = true;
    //  passenger->path = path; UnitsManager_SetNewOrder(ORDER_MOVE, …)`.
    state.passenger_waypoint = state.destination;
    state.move_order_issued = true;

    if let Some(mut info) = ctx.unit(unit_id) {
        info.ai_state_bits &= !crate::enums::AI_STATE_NO_RETREAT;
        ctx.write_unit(unit_id, info);
    }

    let mut data = ctx.order_data(unit_id);
    data.move_to = state.destination;
    ctx.set_order_data(unit_id, data);

    if let Some(mut info) = ctx.unit(unit_id) {
        info.target_grid = state.destination;
        ctx.write_unit(unit_id, info);
    }

    ctx.set_new_order(unit_id, UnitOrder::Move, UnitOrderState::Init);
    state.started = true;
}

/// `TaskMove::RetryMove` — repeat the order to the same point.
fn retry_move(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>) {
    let TaskKind::Move(state) = kind else {
        return;
    };

    let Some(unit_id) = state.unit else { return };
    if ctx.unit(unit_id).is_none() {
        return;
    }

    let mut data = ctx.order_data(unit_id);
    data.move_to = state.destination;
    ctx.set_order_data(unit_id, data);

    ctx.set_new_order(unit_id, UnitOrder::Move, UnitOrderState::Init);
}

/// `TaskMove::Execute(unit)` — completing the next leg.
pub(crate) fn move_execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let TaskKind::Move(state) = kind else {
        return false;
    };

    if state.unit != Some(unit) {
        return false;
    }

    let Some(info) = ctx.unit(unit) else {
        move_finished_self(kind, ctx, header, TASKMOVE_RESULT_CANCELLED);
        return true;
    };

    // ---- transport phase (`TaskMove::AttemptTransport`) -------------------
    //
    // A land passenger that cannot reach the destination on the ground boards a
    // `TaskTransport` first: it walks to the boarding cell (`transport_waypoint`),
    // waits in the transporter's hold and continues from the drop-off cell
    // (`passenger_destination`) once released. The delivery is detected even while
    // the unit is in the hold and therefore not ready for orders, so this runs
    // before the readiness check.
    if state.transport_needed {
        let loaded = passenger_in_hold(ctx, unit, state);

        if loaded {
            state.transport_loaded = true;
            return false;
        }

        if state.transport_loaded {
            // Carried and released, or already standing on the far shore: the
            // rest of the move is an ordinary ground move.
            state.transport_loaded = false;
            state.transport_needed = false;
            state.transporter_unit_type = None;
            state.transport_task = None;
            state.destination = state.final_destination;
        } else {
            // Waiting at the boarding cell: the transporter picks the unit up.
            if !transport_client_registered(ctx, header.id, state.transport_task) {
                state.transport_task = transport_request(
                    ctx,
                    header,
                    unit,
                    state.transporter_unit_type,
                    state.transport_waypoint,
                );
            }

            if ctx.ready_to_take_orders(unit)
                && !is_at_destination(ctx, unit, state.destination, state.minimum_distance)
            {
                retry_move(kind, ctx);
            }

            // No self-reminder: the transporter reminds the passenger when it is
            // loaded/released and `AiPlayer::BeginTurn` polls every unit with a
            // task, so re-queueing here would spin inside `ExecuteReminders`.
            return true;
        }
    } else if state.transporter_unit_type.is_none()
        && is_land_passenger(&info)
        && ctx.ready_to_take_orders(unit)
    {
        // `TaskMove::AttemptTransport` on a failed full path: split the trip at
        // the water and request a transporter.
        if let Some(route) = transport_route(ctx.world, &info, state.destination) {
            state.final_destination = state.destination;
            state.transport_waypoint = route.pickup;
            state.passenger_destination = route.dropoff;
            state.transporter_unit_type = Some(route.transporter);
            state.transport_needed = true;
            state.transport_loaded = false;
            state.destination = route.pickup;
            state.transport_task =
                transport_request(ctx, header, unit, Some(route.transporter), route.pickup);

            if !is_at_destination(ctx, unit, state.destination, state.minimum_distance) {
                retry_move(kind, ctx);
            }

            return true;
        }
    }

    if !ctx.ready_to_take_orders(unit) {
        return false;
    }

    // Attack along the way (C++ `AiAttack_EvaluateAttack`).
    let is_computer =
        ctx.world.teams[info.team.0 as usize].team_type == crate::team::TeamKind::Computer;

    if is_computer
        && info.ammo > 0
        && crate::ai::attack::evaluate_attack(
            ctx.tm,
            ctx.world,
            unit,
            state.caution_level != CautionLevel::None,
        )
    {
        return true;
    }

    if is_at_destination(ctx, unit, state.destination, state.minimum_distance) {
        move_finished_self(kind, ctx, header, TASKMOVE_RESULT_SUCCESS);
        return true;
    }

    // Shot reserve (C++ `Task_ShouldReserveShot`) — wait, do not move.
    if state.caution_level != CautionLevel::None {
        let grid = ctx.unit(unit).map(|info| info.grid).unwrap_or(Point::ZERO);
        if let Some(info) = ctx.unit(unit) {
            if task_should_reserve_shot(ctx.world, &info, grid) {
                let rounds = info
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
                let unit_speed = info
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_SPEED);

                if rounds > 0 && i32::from(info.speed) <= (unit_speed + rounds - 1) / rounds {
                    return false;
                }
            }
        }
    }

    // `RetryMove` + `RemindMoveFinished` (wait for the unit's next turn).
    retry_move(kind, ctx);
    ctx.remind_move_finished(unit, true);
    let _ = TASK_PRIORITY_MOVE;

    true
}

/// `TaskMove::RemoveUnit` — cancel.
fn move_remove_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    let TaskKind::Move(state) = kind else {
        return;
    };

    if state.unit != Some(unit) {
        return;
    }

    move_finished_self(kind, ctx, header, TASKMOVE_RESULT_CANCELLED);
}

// ---------------------------------------------------------------------------
// TaskMoveHome
// ---------------------------------------------------------------------------

/// Target building for retreating home (`TaskMoveHome::EndTurn`): the nearest
/// team building with `storage` or `ORDER_AWAIT`.
fn home_site(ctx: &TaskCtx<'_>, team: crate::team::TeamId, from: Point) -> Option<Point> {
    let mut best: Option<(i32, Point)> = None;

    for id in ctx.list(UnitListKind::Stationary) {
        let Some(unit) = ctx.unit(id) else { continue };
        if unit.team != team || unit.orders == UnitOrder::Idle {
            continue;
        }

        let dx = i32::from(unit.grid.x) - i32::from(from.x);
        let dy = i32::from(unit.grid.y) - i32::from(from.y);
        let distance = dx * dx + dy * dy;

        if best
            .map(|(best_distance, _)| distance < best_distance)
            .unwrap_or(true)
        {
            best = Some((distance, unit.grid));
        }
    }

    best.map(|(_, grid)| grid)
}

// ---------------------------------------------------------------------------
// Transport (`TaskMove::AttemptTransport`, `TaskTransport`)
// ---------------------------------------------------------------------------

/// A land move split by a water gap — the data `TaskMove::TranscribeTransportPath`
/// (`taskmove.cpp:847`) writes into `transport_waypoint`/`passenger_destination`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransportRoute {
    /// Transporter type the passenger needs (`AttemptTransport`).
    pub transporter: UnitType,
    /// `transport_waypoint` — the last reachable cell before the water.
    pub pickup: Point,
    /// `passenger_destination` — the first reachable cell behind the water.
    pub dropoff: Point,
}

/// In-map check (`Access_IsInsideBounds`).
fn is_inside_map(site: Point) -> bool {
    site.x >= 0 && site.y >= 0 && site.x < crate::geo::MAP_WIDTH && site.y < crate::geo::MAP_HEIGHT
}

/// Types `TaskTransport` operates: `SEATRANS` and the amphibious `CLNTRANS`
/// (`ArmouredPersonnelCarrier`). Air transport is not used by the AI transport
/// flow (documented deviation: C++ `AttemptTransport` may pick `AIRTRANS`).
fn is_transporter_type(unit_type: UnitType) -> bool {
    matches!(
        unit_type,
        UnitType::SeaTransport | UnitType::ArmouredPersonnelCarrier
    )
}

/// A passenger whose type cannot enter water (`TaskMove::AttemptTransport`:
/// `!(GetLandType() & SURFACE_TYPE_WATER)`).
pub(crate) fn is_land_passenger(unit: &UnitInfo) -> bool {
    !unit.is_air() && access::unit_type_land_type(unit.unit_type) & access::SURFACE_TYPE_WATER == 0
}

/// Terrain passability of a cell for a type — the surface rules of
/// `AccessMap::Init` (`accessmap.cpp:372`).
fn can_stand(world: &crate::game::GameState, unit_type: UnitType, site: Point) -> bool {
    if !is_inside_map(site) {
        return false;
    }

    let land_type = access::unit_type_land_type(unit_type);
    let surface = access::modified_surface_type(world, site, false);

    if surface & access::SURFACE_TYPE_WATER != 0 {
        land_type & access::SURFACE_TYPE_WATER != 0
    } else if surface & access::SURFACE_TYPE_COAST != 0 {
        land_type & access::SURFACE_TYPE_COAST != 0
    } else if surface & access::SURFACE_TYPE_LAND != 0 {
        land_type & access::SURFACE_TYPE_LAND != 0
    } else {
        false
    }
}

/// Whether the team owns a unit of the type (`Task_GetReadyUnitsCount`-like;
/// only alive units count).
fn team_has_unit(world: &crate::game::GameState, team: TeamId, unit_type: UnitType) -> bool {
    for kind in UnitListKind::ALL {
        for id in world.unit_list(kind) {
            let Some(unit) = world.unit(*id) else {
                continue;
            };
            if unit.team == team && unit.unit_type == unit_type && unit.hits > 0 {
                return true;
            }
        }
    }

    false
}

/// Whether any enemy heat map covers the cell (`UnitsManager_TeamInfo[team].heat_map`).
fn cell_seen_by_enemies(world: &crate::game::GameState, team: TeamId, site: Point) -> bool {
    for other in 0..crate::game::PLAYER_TEAM_COUNT {
        if crate::team::TeamId(other as u8) == team
            || world.teams[other].team_type == crate::team::TeamKind::None
        {
            continue;
        }

        if world.ai.heat_maps[other].get_complete(i32::from(site.x), i32::from(site.y)) > 0 {
            return true;
        }
    }

    false
}

/// `TaskMove::AttemptTransport` transporter choice (`taskmove.cpp:370`).
///
/// An infantry man/commando boards an `APC` when the team owns one and the
/// starting cell is not watched; otherwise the sea transport is used.
fn choose_transporter(world: &crate::game::GameState, passenger: &UnitInfo) -> UnitType {
    if matches!(
        passenger.unit_type,
        UnitType::Infiltrator | UnitType::Infantry
    ) && team_has_unit(world, passenger.team, UnitType::ArmouredPersonnelCarrier)
        && !cell_seen_by_enemies(world, passenger.team, passenger.grid)
    {
        return UnitType::ArmouredPersonnelCarrier;
    }

    UnitType::SeaTransport
}

/// Splits a land move at the water — the heart of `TaskMove::AttemptTransport`
/// and `TranscribeTransportPath`.
///
/// Returns `None` when the passenger reaches the destination on the ground
/// (`find_ground_path` succeeds) or when no landing cell exists behind the water.
pub(crate) fn transport_route(
    world: &crate::game::GameState,
    unit: &UnitInfo,
    destination: Point,
) -> Option<TransportRoute> {
    let steps = crate::pathfinding::append_linear_steps(unit.grid, destination);
    let mut position = unit.grid;
    let mut pickup: Option<Point> = None;
    let mut dropoff: Option<Point> = None;
    let mut saw_water = false;

    for step in steps {
        let next = Point::new(
            position.x.wrapping_add(i16::from(step.x)),
            position.y.wrapping_add(i16::from(step.y)),
        );

        if !is_inside_map(next) {
            return None;
        }

        let stand = can_stand(world, unit.unit_type, next);

        if pickup.is_none() {
            if !stand {
                pickup = Some(position);
            }
        } else if stand {
            dropoff = Some(next);
            break;
        }

        if !stand
            && access::modified_surface_type(world, next, false) & access::SURFACE_TYPE_WATER != 0
        {
            saw_water = true;
        }

        position = next;
    }

    let (pickup, dropoff) = (pickup?, dropoff?);

    if !saw_water {
        // Blocked by something other than water — a bridge or a zone.
        return None;
    }

    // The C++ `AttemptTransport` runs after a failed full path search: a ground
    // detour around the water makes the transporter unnecessary.
    if crate::pathfinding::find_ground_path(
        world,
        unit,
        destination,
        access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS,
        0,
    )
    .is_some()
    {
        return None;
    }

    Some(TransportRoute {
        transporter: choose_transporter(world, unit),
        pickup,
        dropoff,
    })
}

/// `TaskTransport::WillTransportNewClient` — free capacity of the transporter.
fn transport_has_room(world: &crate::game::GameState, team: TeamId, kind: &TaskKind) -> bool {
    let TaskKind::Transport {
        transporter_unit_type,
        move_tasks,
        ..
    } = kind
    else {
        return false;
    };

    let storage = world.team_units[team.0 as usize]
        .current_unit_values(*transporter_unit_type as u16)
        .get_attribute(ATTRIB_STORAGE)
        .max(1) as usize;

    move_tasks.len() < storage
}

/// The transporter's current cell, or the boarding cell it heads to.
fn transport_position(world: &crate::game::GameState, kind: &TaskKind) -> Option<Point> {
    let TaskKind::Transport {
        unit_transporter,
        task_move,
        ..
    } = kind
    else {
        return None;
    };

    if let Some(transporter) = unit_transporter {
        return world.unit(*transporter).map(|unit| unit.grid);
    }

    task_move.and_then(
        |id| match world.ai.task_manager.task(id).map(|task| &task.kind) {
            Some(TaskKind::Move(state)) => Some(state.transport_waypoint),
            _ => None,
        },
    )
}

/// Joins an existing `TaskTransport` of the team or creates one
/// (`TaskMove::FindTransport`, `taskmove.cpp:1399`).
pub(crate) fn transport_request(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    _unit: UnitId,
    transporter_type: Option<UnitType>,
    pickup: Point,
) -> Option<u32> {
    let transporter_type = transporter_type?;
    let team = header.team;

    let mut best: Option<(u32, i32)> = None;

    for task in ctx.tm.tasks() {
        if task.team != team || task.get_type() != TASK_TYPE_TRANSPORT {
            continue;
        }

        let TaskKind::Transport {
            transporter_unit_type,
            ..
        } = &task.kind
        else {
            continue;
        };

        if *transporter_unit_type != transporter_type
            || !transport_has_room(ctx.world, team, &task.kind)
        {
            continue;
        }

        let site = transport_position(ctx.world, &task.kind).unwrap_or(pickup);
        let distance = access::squared_distance(site, pickup);

        if best
            .map(|(_, best_distance)| distance < best_distance)
            .unwrap_or(true)
        {
            best = Some((task.id, distance));
        }
    }

    if let Some((task_id, _)) = best {
        if let Some(task) = ctx.tm.task_mut(task_id) {
            if let TaskKind::Transport {
                move_tasks,
                task_move,
                ..
            } = &mut task.kind
            {
                if !move_tasks.contains(&header.id) {
                    move_tasks.push(header.id);
                }

                if task_move.is_none() {
                    *task_move = Some(header.id);
                }

                return Some(task_id);
            }
        }

        return None;
    }

    let priority = ctx.priority_of(header.id);
    let kind = TaskKind::transport(header.id, transporter_type);

    Some(ctx.append_task(team, None, priority, kind))
}

/// Whether the move task is still a client of its `TaskTransport`.
fn transport_client_registered(
    ctx: &TaskCtx<'_>,
    client: u32,
    transport_task: Option<u32>,
) -> bool {
    match transport_task.and_then(|id| ctx.tm.task(id)) {
        Some(task) => {
            matches!(&task.kind, TaskKind::Transport { move_tasks, .. } if move_tasks.contains(&client))
        }
        None => false,
    }
}

/// `ORDER_IDLE` with a transporter parent — the passenger is in the hold.
fn passenger_in_hold(ctx: &TaskCtx<'_>, passenger: UnitId, state: &super::MoveState) -> bool {
    let Some(info) = ctx.unit(passenger) else {
        return false;
    };

    if info.orders != UnitOrder::Idle {
        return false;
    }

    let Some(parent) = info.parent_unit else {
        return false;
    };

    if let Some(task) = state.transport_task.and_then(|id| ctx.tm.task(id)) {
        if let TaskKind::Transport {
            unit_transporter, ..
        } = &task.kind
        {
            return *unit_transporter == Some(parent);
        }
    }

    ctx.unit(parent)
        .map(|unit| is_transporter_type(unit.unit_type))
        .unwrap_or(false)
}

/// The current client of a transport task.
#[derive(Debug, Clone, Copy)]
struct ClientPlan {
    passenger: UnitId,
    pickup: Point,
    dropoff: Point,
}

/// Reads the transport data of a client `TaskMove`.
fn client_plan(ctx: &TaskCtx<'_>, client: Option<u32>) -> Option<ClientPlan> {
    let task = ctx.tm.task(client?)?;

    let TaskKind::Move(state) = &task.kind else {
        return None;
    };

    if !state.transport_needed {
        return None;
    }

    Some(ClientPlan {
        passenger: state.unit?,
        pickup: state.transport_waypoint,
        dropoff: state.passenger_destination,
    })
}

/// Clears the transport phase of a delivered client and lets its `TaskMove`
/// continue to the real destination.
fn clear_client_transport(ctx: &mut TaskCtx<'_>, client: u32) {
    let Some(passenger) = client_plan(ctx, Some(client)).map(|plan| plan.passenger) else {
        return;
    };

    if let Some(task) = ctx.tm.task_mut(client) {
        if let TaskKind::Move(state) = &mut task.kind {
            state.transport_needed = false;
            state.transport_loaded = false;
            state.transporter_unit_type = None;
            state.transport_task = None;
            state.destination = state.final_destination;
        }
    }

    ctx.remind_move_finished(passenger, true);
}

/// Drops stale clients and closes the clients delivered on the far shore
/// (C++ `TaskTransport::ChildComplete`/`EventUnitUnloaded` without the events).
fn transport_cleanup_clients(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>) {
    let TaskKind::Transport {
        move_tasks,
        task_move,
        unload_site,
        ..
    } = kind
    else {
        return;
    };

    let mut keep = Vec::new();

    for client in move_tasks.iter().copied() {
        let Some(plan) = client_plan(ctx, Some(client)) else {
            // The client finished or no longer needs a transporter.
            continue;
        };

        let Some(info) = ctx.unit(plan.passenger) else {
            continue;
        };

        if info.hits == 0 {
            continue;
        }

        let in_hold = info.orders == UnitOrder::Idle && info.parent_unit.is_some();

        // Released by `UnitsManager_ActivateUnit` on the cell the unload order set.
        if !in_hold && *unload_site == Some(info.grid) {
            *unload_site = None;
            clear_client_transport(ctx, client);
            continue;
        }

        keep.push(client);
    }

    *move_tasks = keep;

    if task_move
        .map(|id| !move_tasks.contains(&id))
        .unwrap_or(false)
    {
        *task_move = None;
    }

    if task_move.is_none() {
        *task_move = move_tasks.first().copied();
    }
}

/// The nearest client (`TaskTransport::ChooseNewTask`).
fn transport_choose_client(ctx: &TaskCtx<'_>, transporter: UnitId, clients: &[u32]) -> Option<u32> {
    let position = ctx.unit(transporter)?.grid;
    let mut best: Option<(u32, i32)> = None;

    for client in clients {
        let Some(plan) = client_plan(ctx, Some(*client)) else {
            continue;
        };

        let target = ctx
            .unit(plan.passenger)
            .map(|unit| unit.grid)
            .unwrap_or(plan.pickup);
        let distance = access::squared_distance(position, target);

        if best
            .map(|(_, best_distance)| distance < best_distance)
            .unwrap_or(true)
        {
            best = Some((*client, distance));
        }
    }

    best.map(|(client, _)| client)
}

/// A cell the transporter can stand on near `near` — the sea-side stop for a
/// boarding/drop-off cell (C++ uses a `TransporterMap` for this; the port takes
/// the nearest water/coast cell reachable by the transporter).
fn transporter_stop_site(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    transporter_type: UnitType,
    near: Point,
) -> Option<Point> {
    for radius in 0..=3i16 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if radius > 0 && dx.abs() != radius && dy.abs() != radius {
                    continue;
                }

                let site = Point::new(near.x.wrapping_add(dx), near.y.wrapping_add(dy));

                if !is_inside_map(site) || !can_stand(ctx.world, transporter_type, site) {
                    continue;
                }

                if access::is_accessible(
                    ctx.world,
                    transporter_type,
                    team,
                    site,
                    access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                ) == 0
                {
                    continue;
                }

                return Some(site);
            }
        }
    }

    None
}

/// A free cell for the passenger around the planned drop-off
/// (`TaskTransport_FinishTransport`: "unit in the way" handling reduced to the
/// nearest free cell instead of `AiPlayer::ClearZone`).
fn find_dropoff(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    passenger_type: UnitType,
    site: Point,
) -> Option<Point> {
    for radius in 0..=2i16 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if radius > 0 && dx.abs() != radius && dy.abs() != radius {
                    continue;
                }

                let candidate = Point::new(site.x.wrapping_add(dx), site.y.wrapping_add(dy));

                if !is_inside_map(candidate) {
                    continue;
                }

                // `is_accessible` blames occupied cells as well, so a free and
                // enterable cell is the `TaskTransport_FinishTransport` condition.
                if access::is_accessible(
                    ctx.world,
                    passenger_type,
                    team,
                    candidate,
                    access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                ) == 0
                {
                    continue;
                }

                return Some(candidate);
            }
        }
    }

    None
}

/// `TaskTransport::LoadUnit` (`tasktransport.cpp:758`).
///
/// The passenger walks onto the transporter's cell: C++ builds a one-step
/// `GroundPath` and issues `ORDER_MOVE_TO_UNIT`/`ORDER_STATE_IN_PROGRESS`, the
/// order machine stores it in the hold (`ProcessOrderMove`/`ProcessLoading`).
fn transport_load(ctx: &mut TaskCtx<'_>, transporter: UnitId, passenger: UnitId) -> bool {
    let Some(transporter_info) = ctx.unit(transporter) else {
        return false;
    };
    let Some(passenger_info) = ctx.unit(passenger) else {
        return false;
    };

    if passenger_info.speed == 0 {
        return false;
    }

    // Already boarding or carried by another transporter.
    if passenger_info.orders == UnitOrder::MoveToUnit
        || (passenger_info.parent_unit.is_some() && passenger_info.parent_unit != Some(transporter))
    {
        return false;
    }

    // `LoadUnit`: a sea transport/APC takes a passenger standing no farther
    // than `Access_GetApproximateDistance <= 3`.
    if access::approximate_distance(
        i32::from(transporter_info.grid.x) - i32::from(passenger_info.grid.x),
        i32::from(transporter_info.grid.y) - i32::from(passenger_info.grid.y),
    ) > 3
    {
        return false;
    }

    let dx = transporter_info.grid.x.wrapping_sub(passenger_info.grid.x);
    let dy = transporter_info.grid.y.wrapping_sub(passenger_info.grid.y);

    let mut info = passenger_info;
    info.target_grid = transporter_info.grid;
    info.path = Some(UnitPath::Ground(GroundPath {
        end_x: transporter_info.grid.x,
        end_y: transporter_info.grid.y,
        step_index: 0,
        steps: vec![PathStep::new(dx as i8, dy as i8)],
    }));
    info.orders = UnitOrder::MoveToUnit;
    info.state = UnitOrderState::InProgress;
    ctx.write_unit(passenger, info);

    let mut data = ctx.order_data(passenger);
    data.move_to = transporter_info.grid;
    ctx.set_order_data(passenger, data);

    true
}

/// `TaskTransport::UnloadUnit` + `TaskTransport_FinishTransport`
/// (`tasktransport.cpp:718`): hand the passenger over with
/// `ORDER_ACTIVATE`/`ORDER_STATE_EXECUTING_ORDER`; `UnitsManager_ActivateUnit`
/// places it on the cell and releases the hold.
fn transport_unload(
    ctx: &mut TaskCtx<'_>,
    transporter: UnitId,
    passenger: UnitId,
    site: Point,
) -> bool {
    let Some(passenger_info) = ctx.unit(passenger) else {
        return false;
    };
    let Some(mut transporter_info) = ctx.unit(transporter) else {
        return false;
    };

    if passenger_info.orders != UnitOrder::Idle {
        return false;
    }

    transporter_info.parent_unit = Some(passenger);
    transporter_info.target_grid = site;
    ctx.write_unit(transporter, transporter_info);

    let mut data = ctx.order_data(transporter);
    data.move_to = site;
    ctx.set_order_data(transporter, data);

    ctx.set_new_order(
        transporter,
        UnitOrder::Activate,
        UnitOrderState::ExecutingOrder,
    );

    true
}

/// `TaskTransport::Init` (`tasktransport.cpp:433`).
fn transport_init(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    let TaskKind::Transport {
        transporter_unit_type,
        unit_transporter,
        task_obtain_units,
        task_move,
        ..
    } = kind
    else {
        return false;
    };

    if let Some(transporter) = *unit_transporter {
        ctx.attach(header.id, transporter);
        ctx.remind_move_finished(transporter, true);
        true
    } else {
        let point = client_plan(ctx, *task_move)
            .map(|plan| plan.pickup)
            .unwrap_or(Point::ZERO);

        let mut obtain_kind = TaskKind::obtain_units(point);
        super::create::obtain_units_add_type(&mut obtain_kind, *transporter_unit_type);

        let priority = ctx.priority_of(header.id);
        let id = ctx.append_task(header.team, Some(header.id), priority, obtain_kind);
        *task_obtain_units = Some(id);

        true
    }
}

/// `TaskTransport::AddUnit` (`tasktransport.cpp:421`).
fn transport_add_unit(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::Transport {
        transporter_unit_type,
        unit_transporter,
        ..
    } = kind
    else {
        return;
    };

    if unit_transporter.is_some() {
        return;
    }

    let Some(info) = ctx.unit(unit) else {
        return;
    };

    if info.unit_type != *transporter_unit_type {
        return;
    }

    *unit_transporter = Some(unit);
    ctx.attach(header.id, unit);
    ctx.remind_move_finished(unit, true);
}

/// `TaskTransport::EndTurn`/`Execute` (`tasktransport.cpp:485-586`).
fn transport_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    transport_cleanup_clients(kind, ctx);

    let TaskKind::Transport {
        unit_transporter, ..
    } = kind
    else {
        return false;
    };

    let Some(transporter) = *unit_transporter else {
        // Waiting for `TaskObtainUnits`.
        return true;
    };

    if ctx.tm.unit_task(transporter) != Some(header.id) || !ctx.ready_to_take_orders(transporter) {
        return true;
    }

    if task_retreat_from_danger(
        ctx.tm,
        ctx.world,
        header.id,
        transporter,
        CautionLevel::AvoidAllDamage,
    ) {
        return true;
    }

    transport_execute(kind, ctx, header)
}

/// `TaskTransport::Execute(unit)` (`tasktransport.cpp:496`).
fn transport_execute(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    let TaskKind::Transport {
        unit_transporter,
        task_move,
        move_tasks,
        transporter_move,
        task_obtain_units,
        unload_site,
        ..
    } = kind
    else {
        return false;
    };

    let Some(transporter_id) = *unit_transporter else {
        return false;
    };

    let Some(transporter) = ctx.unit(transporter_id) else {
        *unit_transporter = None;
        return true;
    };

    if task_move.is_none() {
        *task_move = transport_choose_client(ctx, transporter_id, move_tasks);
        *unload_site = None;
    }

    let Some(client_id) = *task_move else {
        // `ChooseNewTask`: nothing left to transport — free the transporter and
        // remove the task (C++ `TaskManager::ClearUnitTasksAndRemindAvailable`,
        // the obtain request, `TaskManager::RemoveTask(*this)`).
        ctx.clear_unit_tasks(transporter_id, true);

        if let Some(obtain) = *task_obtain_units {
            ctx.remove_self(obtain);
            *task_obtain_units = None;
        }

        *unit_transporter = None;
        ctx.set_parent(header.id, None);
        ctx.remove_self(header.id);
        return true;
    };

    let Some(plan) = client_plan(ctx, Some(client_id)) else {
        *task_move = None;
        return true;
    };

    let Some(passenger) = ctx.unit(plan.passenger) else {
        *task_move = None;
        return true;
    };

    let reach = if transporter.unit_type == UnitType::AirTransport {
        0
    } else {
        3
    };

    let in_hold =
        passenger.orders == UnitOrder::Idle && passenger.parent_unit == Some(transporter_id);

    if in_hold {
        // `Execute`: `Access_GetSquaredDistance(transporter, destination) <= distance`.
        if access::squared_distance(transporter.grid, plan.dropoff) <= reach {
            if let Some(site) = find_dropoff(ctx, header.team, passenger.unit_type, plan.dropoff) {
                // The hold is released by `ORDER_ACTIVATE` on the next tick; the
                // client is closed by `transport_cleanup_clients` once it stands
                // on the far shore (the `EventUnitUnloaded` notification does not
                // exist in this port).
                if transport_unload(ctx, transporter_id, plan.passenger, site) {
                    *unload_site = Some(site);
                }
            }

            *transporter_move = None;
        } else if transporter_move.is_none() {
            if let Some(site) =
                transporter_stop_site(ctx, header.team, transporter.unit_type, plan.dropoff)
            {
                let priority = ctx.priority_of(header.id);
                let move_kind =
                    TaskKind::move_to(transporter_id, site, 0, CautionLevel::AvoidAllDamage);
                *transporter_move =
                    Some(ctx.append_task(header.team, Some(header.id), priority, move_kind));
            }
        }

        return true;
    }

    if passenger.hits == 0 {
        *task_move = None;
        return true;
    }

    // `TaskTransport::LoadUnit`: the sea transport stops within
    // `Access_GetApproximateDistance <= 3` of the passenger.
    if access::approximate_distance_points(transporter.grid, plan.pickup) <= reach {
        transport_load(ctx, transporter_id, plan.passenger);
    } else if transporter_move.is_none() {
        if let Some(site) =
            transporter_stop_site(ctx, header.team, transporter.unit_type, plan.pickup)
        {
            let priority = ctx.priority_of(header.id);
            let move_kind =
                TaskKind::move_to(transporter_id, site, 0, CautionLevel::AvoidAllDamage);
            *transporter_move =
                Some(ctx.append_task(header.team, Some(header.id), priority, move_kind));
        }
    }

    true
}

/// `TaskTransport::RemoveUnit` — the transporter is gone; the clients re-request
/// a transporter on their next `TaskMove::Execute`.
fn transport_remove_unit(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::Transport {
        unit_transporter,
        move_tasks,
        task_move,
        ..
    } = kind
    else {
        return;
    };

    if *unit_transporter != Some(unit) {
        return;
    }

    move_tasks.clear();
    *task_move = None;
    *unit_transporter = None;
    ctx.set_parent(header.id, None);
    ctx.remove_self(header.id);
}

/// `TaskTransport::RemoveSelf` (`tasktransport.cpp:588`).
fn transport_remove_self(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Transport {
        unit_transporter,
        task_obtain_units,
        ..
    } = kind
    else {
        return;
    };

    if let Some(transporter) = *unit_transporter {
        ctx.clear_unit_tasks(transporter, true);
    }

    if let Some(obtain) = *task_obtain_units {
        ctx.remove_self(obtain);
    }

    *unit_transporter = None;
    *task_obtain_units = None;
    ctx.set_parent(header.id, None);
    ctx.remove_self(header.id);
}

/// `TaskTransport::ChildComplete`/`MoveFinishedCallback` for the transporter's
/// own `TaskMove`.
fn transport_move_finished(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    unit: UnitId,
    _result: i8,
) -> bool {
    let TaskKind::Transport {
        unit_transporter,
        transporter_move,
        ..
    } = kind
    else {
        return false;
    };

    if *unit_transporter == Some(unit) {
        *transporter_move = None;
        ctx.remind_move_finished(unit, true);
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// `Init` for move classes.
pub fn init(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Move(state) => {
            let Some(unit_id) = state.unit else {
                ctx.remove_self(header.id);
                return true;
            };

            if ctx.unit(unit_id).is_none() {
                ctx.remove_self(header.id);
                return true;
            }

            // `Task_RemoveMovementTasks(passenger); passenger->AddTask(this);`
            task_remove_movement_tasks(ctx.tm, ctx.world, unit_id);
            ctx.attach(header.id, unit_id);

            let is_idle = ctx
                .world
                .unit(unit_id)
                .map(|unit| unit.orders == UnitOrder::Idle)
                .unwrap_or(true);

            if is_idle {
            } else {
                move_search(kind, ctx, header, true);
            }

            true
        }
        TaskKind::MoveHome { unit, .. } => {
            if let Some(unit_id) = *unit {
                ctx.attach(header.id, unit_id);
            }
            true
        }
        TaskKind::FindPath { .. } => {
            // In the synchronous model the path is requested by the order machine; the task
            // completes immediately and notifies the parent.
            find_path_run(kind, ctx, header);
            true
        }
        TaskKind::Rendezvous { .. } => true,
        TaskKind::AssistMove { .. } => true,
        TaskKind::Transport { .. } => transport_init(kind, ctx, header),
        TaskKind::Dump { .. } => true,
        TaskKind::SearchDestination { .. } => true,
        _ => false,
    }
}

/// `TaskFindPath`: in the synchronous model the path request is performed by `unit_order`.
fn find_path_run(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::FindPath {
        unit,
        destination,
        parent_move,
        finished,
    } = kind
    else {
        return;
    };

    if *finished {
        return;
    }

    if let Some(unit_id) = *unit {
        if ctx.unit(unit_id).is_some()
            && ctx
                .world
                .is_active_turn(crate::ai::task::team_of(ctx.world, unit_id))
        {
            let mut data = ctx.order_data(unit_id);
            data.move_to = *destination;
            ctx.set_order_data(unit_id, data);
            ctx.set_new_order(unit_id, UnitOrder::Move, UnitOrderState::Init);
        }
    }

    *finished = true;

    if let Some(parent) = *parent_move {
        if ctx.tm.has_task(parent) {
            if let Some(unit_id) = *unit {
                ctx.tm
                    .task_move_finished(ctx.world, parent, unit_id, TASKMOVE_RESULT_SUCCESS | 0);
            }
        }
    }

    ctx.remove_self(header.id);
}

/// `BeginTurn` for move classes.
pub fn begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Move(_) => {
            move_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::MoveHome { .. } => {
            move_home_end_turn(kind, ctx, header);
            true
        }
        TaskKind::SearchDestination { .. } => {
            search_destination_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Rendezvous { .. } => {
            rendezvous_end_turn(kind, ctx, header);
            true
        }
        TaskKind::FindPath { .. } => {
            find_path_run(kind, ctx, header);
            true
        }
        TaskKind::Transport { .. } => {
            transport_end_turn(kind, ctx, header);
            true
        }
        TaskKind::AssistMove { .. } | TaskKind::Dump { .. } => true,
        _ => false,
    }
}

/// `TaskMove::BeginTurn`.
fn move_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Move(state) = kind else {
        return;
    };

    let Some(unit_id) = state.unit else { return };

    if ctx.tm.unit_task(unit_id) != Some(header.id) {
        return;
    }

    if !ctx.ready_to_take_orders(unit_id) {
        return;
    }

    let speed = ctx.unit(unit_id).map(|unit| unit.speed).unwrap_or(0);

    if speed > 0 {
        ctx.remind_move_finished(unit_id, true);
    } else if state.finish_on_arrival {
        move_finished_self(kind, ctx, header, TASKMOVE_RESULT_SUCCESS);
    }
}

/// `TaskMove::EndTurn`.
fn move_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Move(state) = kind else {
        return;
    };

    let Some(unit_id) = state.unit else { return };

    if ctx.tm.unit_task(unit_id) != Some(header.id) {
        return;
    }

    if !ctx.ready_to_take_orders(unit_id) {
        return;
    }

    let speed = ctx.unit(unit_id).map(|unit| unit.speed).unwrap_or(0);

    if state.finish_on_arrival && speed == 0 {
        move_finished_self(kind, ctx, header, TASKMOVE_RESULT_SUCCESS);
    } else if speed > 0 {
        move_execute(kind, ctx, header, unit_id);
    }
}

/// `TaskMoveHome::EndTurn` — go to the nearest own building.
fn move_home_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::MoveHome { unit, site } = kind else {
        return;
    };

    let Some(unit_id) = *unit else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(info) = ctx.unit(unit_id) else {
        ctx.remove_self(header.id);
        return;
    };

    if info.orders != UnitOrder::Await && info.orders != UnitOrder::Sentry {
        return;
    }

    if let Some(home) = home_site(ctx, info.team, info.grid) {
        *site = home;

        let priority = ctx.priority_of(header.id);
        let move_kind = TaskKind::move_to(unit_id, home, 0, CautionLevel::AvoidAllDamage);
        ctx.append_task(info.team, Some(header.id), priority, move_kind);
    }

    ctx.remove_self(header.id);
}

/// `TaskSearchDestination` — finding a place for the unit (structurally).
fn search_destination_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::SearchDestination {
        unit,
        search_radius,
        destination,
        ..
    } = kind
    else {
        return;
    };

    let Some(unit_id) = *unit else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(info) = ctx.unit(unit_id) else {
        ctx.remove_self(header.id);
        return;
    };

    if info.orders != UnitOrder::Await {
        return;
    }

    // The search spiral (C++ `SearchNextCircle`) is reduced to a square of the radius.
    let radius = i32::from(*search_radius);
    let mut found = None;

    let mut dy = -radius;
    while dy <= radius {
        let mut dx = -radius;
        while dx <= radius {
            let site = Point::new(info.grid.x + dx as i16, info.grid.y + dy as i16);

            if site.x >= 0
                && site.y >= 0
                && ctx.accessible(
                    info.unit_type,
                    info.team,
                    site,
                    crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                )
            {
                found = Some(site);
                break;
            }

            dx += 1;
        }
        if found.is_some() {
            break;
        }
        dy += 1;
    }

    match found {
        Some(site) => {
            *destination = site;
            let priority = ctx.priority_of(header.id);
            let move_kind = TaskKind::move_to(unit_id, site, 0, CautionLevel::AvoidAllDamage);
            ctx.append_task(info.team, Some(header.id), priority, move_kind);
        }
        None => {}
    }

    ctx.remove_self(header.id);
}

/// `TaskRendezvous` — bring two units together (structurally).
fn rendezvous_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Rendezvous { unit1, unit2, .. } = kind else {
        return;
    };

    let (Some(first), Some(second)) = (*unit1, *unit2) else {
        ctx.remove_self(header.id);
        return;
    };

    let (Some(a), Some(b)) = (ctx.unit(first), ctx.unit(second)) else {
        ctx.remove_self(header.id);
        return;
    };

    let distance = crate::access::squared_distance(a.grid, b.grid);

    if distance <= 2 {
        if let Some(parent) = header.parent {
            ctx.tm.task_child_complete(ctx.world, parent, header.id);
        }
        ctx.remove_self(header.id);
        return;
    }

    let priority = ctx.priority_of(header.id);
    let kind_move = TaskKind::move_to(second, a.grid, 0, CautionLevel::AvoidNextTurnsFire);
    ctx.append_task(header.team, Some(header.id), priority, kind_move);
}

/// `EndTurn` for move classes.
pub fn end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Move(_) => {
            move_end_turn(kind, ctx, header);
            true
        }
        TaskKind::MoveHome { .. } => {
            move_home_end_turn(kind, ctx, header);
            true
        }
        TaskKind::SearchDestination { .. } => {
            search_destination_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Rendezvous { .. } => {
            rendezvous_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Transport { .. } => {
            transport_end_turn(kind, ctx, header);
            true
        }
        TaskKind::FindPath { .. } => {
            find_path_run(kind, ctx, header);
            true
        }
        TaskKind::AssistMove { .. } | TaskKind::Dump { .. } => true,
        _ => false,
    }
}

/// `Execute(unit)` for move classes.
pub fn execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::Move(_) => move_execute(kind, ctx, header, unit),
        TaskKind::MoveHome { .. } => {
            move_home_end_turn(kind, ctx, header);
            true
        }
        // `TaskTransport::Execute(unit)` — only for the transporter itself.
        TaskKind::Transport {
            unit_transporter, ..
        } => {
            if *unit_transporter != Some(unit) {
                return false;
            }

            transport_execute(kind, ctx, header)
        }
        _ => false,
    }
}

/// `AddUnit(unit)` for move classes.
pub fn add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::Move(state) => {
            if state.unit.is_none() {
                state.unit = Some(unit);
            }

            if state.unit == Some(unit) && ctx.tm.unit_task(unit) == Some(header.id) {
                ctx.remind_move_finished(unit, true);
            }
        }
        TaskKind::MoveHome { unit: slot, .. } => {
            if slot.is_none() {
                *slot = Some(unit);
            }
        }
        TaskKind::Rendezvous { unit2, .. } => {
            if unit2.is_none() {
                *unit2 = Some(unit);
            }
        }
        TaskKind::Transport { .. } => {
            transport_add_unit(kind, ctx, header, unit);
        }
        _ => {}
    }
}

/// `RemoveUnit(unit)` for move classes.
pub fn remove_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::Move(_) => move_remove_unit(kind, ctx, header, unit),
        TaskKind::MoveHome { unit: slot, .. } => {
            if *slot == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::Rendezvous { unit1, unit2, .. } => {
            if *unit1 == Some(unit) || *unit2 == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::Transport { .. } => {
            transport_remove_unit(kind, ctx, header, unit);
        }
        _ => {}
    }
}

/// `RemoveSelf()` for move classes.
pub fn remove_self(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Move(state) => {
            if let Some(unit_id) = state.unit {
                ctx.detach(header.id, unit_id, true);
            }
            state.unit = None;
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::MoveHome { .. }
        | TaskKind::FindPath { .. }
        | TaskKind::Rendezvous { .. }
        | TaskKind::AssistMove { .. }
        | TaskKind::Dump { .. }
        | TaskKind::SearchDestination { .. } => {
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::Transport { .. } => {
            transport_remove_self(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `ChildComplete(task)` for move classes.
pub fn child_complete(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    child: u32,
) -> bool {
    match kind {
        TaskKind::Transport {
            move_tasks,
            task_move,
            transporter_move,
            task_obtain_units,
            unload_site,
            ..
        } => {
            // `TaskTransport::ChildComplete`: a finished `TaskObtainUnits` clears
            // the pending request, a finished client leaves the dispatch list.
            if ctx
                .tm
                .task(child)
                .map(|task| task.get_type())
                .is_some_and(|task_type| task_type == super::super::task::TASK_TYPE_OBTAIN_UNITS)
            {
                *task_obtain_units = None;
            }

            move_tasks.retain(|task| *task != child);

            if *task_move == Some(child) {
                *task_move = None;
                *unload_site = None;
            }

            if *transporter_move == Some(child) {
                *transporter_move = None;
            }

            let _ = header;
            true
        }
        TaskKind::AssistMove { .. } | TaskKind::Rendezvous { .. } | TaskKind::Dump { .. } => {
            let _ = (ctx, header, child);
            true
        }
        _ => false,
    }
}

/// Notification about a child `TaskMove` completion.
pub fn move_finished(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
    result: i8,
) -> bool {
    match kind {
        TaskKind::MoveHome { .. } => {
            let _ = (unit, result);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::SearchDestination { .. } | TaskKind::Rendezvous { .. } => {
            let _ = (unit, result);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::Dump { .. } => {
            let _ = (unit, result);
            true
        }
        TaskKind::Transport { .. } => transport_move_finished(kind, ctx, header, unit, result),
        _ => false,
    }
}

/// `CheckReactions` — not overridden.
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

/// `EventUnitDestroyed` for move classes.
pub fn event_unit_destroyed(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::Move(state) => {
            if state.unit == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::MoveHome { unit: slot, .. } => {
            if *slot == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::Rendezvous { unit1, unit2, .. } => {
            if *unit1 == Some(unit) || *unit2 == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::Transport {
            unit_transporter, ..
        } => {
            if *unit_transporter == Some(unit) {
                transport_remove_unit(kind, ctx, header, unit);
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

/// `EventCargoTransfer` — not overridden.
pub fn event_cargo_transfer(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventUnitLoaded` — `TaskAssistMove`/`TaskTransport` (structurally).
pub fn event_unit_loaded(
    kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit1: UnitId,
    _unit2: UnitId,
) -> bool {
    matches!(
        kind,
        TaskKind::AssistMove { .. } | TaskKind::Transport { .. }
    )
}

/// `EventUnitUnloaded` — `TaskAssistMove`/`TaskTransport` (structurally).
pub fn event_unit_unloaded(
    kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit1: UnitId,
    _unit2: UnitId,
) -> bool {
    matches!(
        kind,
        TaskKind::AssistMove { .. } | TaskKind::Transport { .. }
    )
}

/// `TaskAssistMove::IsUnitUsable` — a transport unit of the team.
pub fn assist_move_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    info.team == header.team
        && info.hits > 0
        && matches!(
            info.unit_type,
            UnitType::SeaTransport | UnitType::AirTransport | UnitType::ArmouredPersonnelCarrier
        )
        && info.orders != UnitOrder::Idle
}

/// Result code for external observers (tests).
pub const MOVE_RESULTS: [i8; 4] = [
    TASKMOVE_RESULT_SUCCESS,
    TASKMOVE_RESULT_ALREADY_IN_RANGE,
    TASKMOVE_RESULT_BLOCKED,
    TASKMOVE_RESULT_CANCELLED,
];

#[cfg(test)]
mod transport_tests {
    use super::*;
    use crate::enums::Surface;
    use crate::enums::TeamClan;
    use crate::game::GameState;
    use crate::geo::{MAP_HEIGHT, MAP_WIDTH};
    use crate::team::TeamUnits;

    /// Land on the west side (`x <= 15`), water in `16..=24`, land from `x = 25`.
    fn channel_state() -> GameState {
        let mut state = GameState::new();
        state.team_units[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Random);

        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                let surface = if (16..=24).contains(&x) {
                    Surface::WATER
                } else {
                    Surface::LAND
                };
                state.map.surface.set(x, y, surface);
            }
        }

        state
    }

    fn unit_at(state: &mut GameState, unit_type: UnitType, site: Point) -> UnitInfo {
        let id = state
            .spawn_unit(unit_type, TeamId::RED, site, 0)
            .expect("unit spawned");
        state.unit(id).expect("unit").clone()
    }

    /// `TranscribeTransportPath`: the boarding cell is the last land cell in
    /// front of the water, the drop-off cell the first land cell behind it.
    #[test]
    fn transport_route_splits_a_land_move_at_the_channel() {
        let mut state = channel_state();
        let tank = unit_at(&mut state, UnitType::Tank, Point::new(10, 10));

        let route = transport_route(&state, &tank, Point::new(32, 10)).expect("transport route");

        assert_eq!(route.pickup, Point::new(15, 10));
        assert_eq!(route.dropoff, Point::new(25, 10));
        assert_eq!(route.transporter, UnitType::SeaTransport);
    }

    /// An infantry man boards the APC when the team owns one and nobody watches.
    #[test]
    fn transport_route_uses_the_apc_for_infantry_when_it_is_available() {
        let mut state = channel_state();
        let _apc = unit_at(
            &mut state,
            UnitType::ArmouredPersonnelCarrier,
            Point::new(13, 10),
        );
        let infantry = unit_at(&mut state, UnitType::Infantry, Point::new(10, 10));

        let route =
            transport_route(&state, &infantry, Point::new(32, 10)).expect("transport route");

        assert_eq!(route.transporter, UnitType::ArmouredPersonnelCarrier);
    }

    /// A ground detour around the water makes the transporter unnecessary
    /// (`AttemptTransport` runs only after a failed full path search).
    #[test]
    fn transport_route_is_none_when_land_connects_the_points() {
        let mut state = channel_state();

        // Close the channel above and below the straight route: the tank walks
        // around the pond.
        for y in 0..MAP_HEIGHT {
            for x in 16..=24 {
                if !(9..=11).contains(&y) {
                    state.map.surface.set(x, y, Surface::LAND);
                }
            }
        }

        let tank = unit_at(&mut state, UnitType::Tank, Point::new(10, 10));

        assert_eq!(transport_route(&state, &tank, Point::new(32, 10)), None);
    }

    /// Units that can enter water themselves and aircraft never request a sea
    /// transporter (`AttemptTransport`).
    #[test]
    fn transport_route_is_none_without_a_water_gap() {
        let mut state = channel_state();
        let tank = unit_at(&mut state, UnitType::Tank, Point::new(10, 10));

        assert_eq!(transport_route(&state, &tank, Point::new(14, 10)), None);
    }

    /// `TaskMove::AttemptTransport`: `GetLandType() & SURFACE_TYPE_WATER`.
    #[test]
    fn only_land_bound_units_are_passengers() {
        let mut state = channel_state();

        let tank = unit_at(&mut state, UnitType::Tank, Point::new(10, 10));
        assert!(is_land_passenger(&tank));

        let ferry = unit_at(&mut state, UnitType::SeaTransport, Point::new(20, 10));
        assert!(!is_land_passenger(&ferry));

        let apc = unit_at(
            &mut state,
            UnitType::ArmouredPersonnelCarrier,
            Point::new(12, 12),
        );
        assert!(!is_land_passenger(&apc));

        let plane = unit_at(&mut state, UnitType::Fighter, Point::new(13, 13));
        assert!(!is_land_passenger(&plane));
    }
}
