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
//! Transport (`AttemptTransport`, `TaskTransport`) and bridges/zones
//! (`EventZoneCleared`) are ported structurally: their behavior depends on
//! `AccessMap`/`ZoneWalker` zone clearing (P5/P7), so `TaskTransport`
//! is created but does not start transporting.

use crate::enums::{CautionLevel, UnitOrder, UnitOrderState, UnitType};
use crate::geo::Point;
use crate::unit::{UnitId, UnitListKind};

use super::super::task::{
    task_remove_movement_tasks, task_should_reserve_shot, TaskHeader,
    TASKMOVE_RESULT_ALREADY_IN_RANGE, TASKMOVE_RESULT_BLOCKED, TASKMOVE_RESULT_CANCELLED,
    TASKMOVE_RESULT_SUCCESS, TASK_PRIORITY_MOVE,
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
        TaskKind::Transport { .. } => true,
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

/// `TaskTransport::EndTurn` — structurally: transportation does not start (see the header).
fn transport_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Transport { move_tasks, .. } = kind else {
        return;
    };

    if move_tasks.is_empty() {
        ctx.remove_self(header.id);
    }
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
        TaskKind::Transport {
            unit_transporter, ..
        } => {
            if unit_transporter.is_none() {
                *unit_transporter = Some(unit);
            }
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
        TaskKind::Transport {
            unit_transporter, ..
        } => {
            if *unit_transporter == Some(unit) {
                ctx.remove_self(header.id);
            }
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
        | TaskKind::Transport { .. }
        | TaskKind::Dump { .. }
        | TaskKind::SearchDestination { .. } => {
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
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
        TaskKind::Transport { move_tasks, .. } => {
            move_tasks.retain(|task| *task != child);
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
                ctx.remove_self(header.id);
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
