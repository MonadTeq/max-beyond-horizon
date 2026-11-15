//! Перемещение: `TaskMove`, `TaskMoveHome`, `TaskFindPath`,
//! `TaskSearchDestination`, `TaskRendezvous`, `TaskAssistMove`,
//! `TaskTransport`, `TaskDump`.
//!
//! Оракул: `taskmove.cpp/.hpp`, `taskmovehome.cpp/.hpp`, `taskfindpath.cpp/.hpp`,
//! `tasksearchdestination.cpp/.hpp`, `taskrendezvous.cpp/.hpp`,
//! `taskassistmove.cpp/.hpp`, `tasktransport.cpp/.hpp`, `taskdump.cpp/.hpp`.
//!
//! # Модель движения
//!
//! В C++ `TaskMove` асинхронно запрашивает путь (`TaskPathRequest` →
//! `TaskFindPath`), получает `GroundPath` и отдаёт приказ `ORDER_MOVE` с
//! готовым путём. В max_core поиск пути синхронный и детерминированный
//! (`pathfinding::request_path` → `poll_results`), а машина приказов
//! (`unit_order`) сама ставит запрос и применяет результат. Поэтому `TaskMove`:
//!
//! 1. проверяет дистанцию (`minimum_distance`) — как `TaskMove::Search`;
//! 2. отдаёт `ORDER_MOVE/INIT` с `move_to` (C++ — `ORDER_STATE_IN_PROGRESS` с
//!    готовым путём; в lockstep-модели это эквивалентно, см. `unit_order`);
//! 3. ждёт `RemindMoveFinished` и повторяет приказ (`RetryMove`), пока юнит не
//!    дойдёт или не заблокируется;
//! 4. по завершении уведомляет родителя кодом [`TASKMOVE_RESULT_*`]
//!    (C++ `result_callback`).
//!
//! Транспорт (`AttemptTransport`, `TaskTransport`) и мосты/зоны
//! (`EventZoneCleared`) портированы структурно: их поведение зависит от
//! `AccessMap`/`ZoneWalker`-очистки зон (P5/P7), поэтому `TaskTransport`
//! создаётся, но перевозку не начинает.

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
/// Дошёл ли юнит до точки в пределах `minimum_distance` (квадрат расстояния).
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

/// `TaskMove::Finished(result)` — уведомление родителя и удаление задачи.
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

/// `TaskMove::Search(mode)` — путь или «уже в радиусе».
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

/// `TaskMove::RetryMove` — повторный приказ к той же точке.
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

/// `TaskMove::Execute(unit)` — завершение очередного перегона.
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

    // Атака по пути (C++ `AiAttack_EvaluateAttack`).
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

    // Резерв выстрела (C++ `Task_ShouldReserveShot`) — ждём, не двигаемся.
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

    // `RetryMove` + `RemindMoveFinished` (ждём следующего хода юнита).
    retry_move(kind, ctx);
    ctx.remind_move_finished(unit, true);
    let _ = TASK_PRIORITY_MOVE;

    true
}

/// `TaskMove::RemoveUnit` — отмена.
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

/// Целевое здание для отступления домой (`TaskMoveHome::EndTurn`): ближайшее
/// здание команды с `storage` или `ORDER_AWAIT`.
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
// Диспетчеризация
// ---------------------------------------------------------------------------

/// `Init` для move-классов.
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
            // В синхронной модели путь запрашивает машина приказов; задача
            // завершается сразу и уведомляет родителя.
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

/// `TaskFindPath`: в синхронной модели запрос пути выполняет `unit_order`.
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

/// `BeginTurn` для move-классов.
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

/// `TaskMoveHome::EndTurn` — идти к ближайшему своему зданию.
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

/// `TaskSearchDestination` — поиск места для юнита (структурно).
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

    // Спираль поиска (C++ `SearchNextCircle`) сведена к квадрату радиуса.
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

/// `TaskRendezvous` — свести два юнита (структурно).
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

/// `TaskTransport::EndTurn` — структурно: перевозка не начинается (см. шапку).
fn transport_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Transport { move_tasks, .. } = kind else {
        return;
    };

    if move_tasks.is_empty() {
        ctx.remove_self(header.id);
    }
}

/// `EndTurn` для move-классов.
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

/// `Execute(unit)` для move-классов.
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

/// `AddUnit(unit)` для move-классов.
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

/// `RemoveUnit(unit)` для move-классов.
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

/// `RemoveSelf()` для move-классов.
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

/// `ChildComplete(task)` для move-классов.
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

/// Уведомление о завершении дочернего `TaskMove`.
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

/// `CheckReactions` — не переопределён.
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

/// `EventUnitDestroyed` для move-классов.
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

/// `EventUnitLoaded` — `TaskAssistMove`/`TaskTransport` (структурно).
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

/// `EventUnitUnloaded` — `TaskAssistMove`/`TaskTransport` (структурно).
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

/// `TaskAssistMove::IsUnitUsable` — транспортный юнит команды.
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
            UnitType::SeaTransport | UnitType::AirTransport | UnitType::PersonnelCarrier
        )
        && info.orders != UnitOrder::Idle
}

/// Код результата для внешних наблюдателей (тесты).
pub const MOVE_RESULTS: [i8; 4] = [
    TASKMOVE_RESULT_SUCCESS,
    TASKMOVE_RESULT_ALREADY_IN_RANGE,
    TASKMOVE_RESULT_BLOCKED,
    TASKMOVE_RESULT_CANCELLED,
];
