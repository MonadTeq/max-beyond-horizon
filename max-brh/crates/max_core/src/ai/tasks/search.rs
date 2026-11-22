//! Map search: `TaskAbstractSearch`, `TaskSurvey`, `TaskExplore`,
//! `TaskAutoSurvey`, `TaskFindMines`.
//!
//! Oracle: `taskabstractsearch.cpp/.hpp`, `tasksurvey.cpp/.hpp`,
//! `taskexplore.cpp/.hpp`, `taskautosurvey.cpp/.hpp`, `taskfindmines.cpp/.hpp`.
//!
//! # Model
//!
//! `TaskAbstractSearch` is the base: it holds a point of interest, a unit list and
//! the number of "requesters"; `FindDestination(unit, radius)` searches the nearest
//! cell (`Access_IsAccessible`) and issues `TaskMove`. Subclasses override
//! `IsVisited(unit, point)`, `ObtainUnit()` and `Execute(unit)`:
//!
//! * `TaskSurvey` — terrain survey (`Surveyor`);
//! * `TaskExplore` — map reconnaissance (any mobile units);
//! * `TaskAutoSurvey` — survey around the center in expanding circles;
//! * `TaskFindMines` — sweeping spotted minefields (`AiPlayer::mine_map`).

use crate::enums::{CautionLevel, UnitOrder};
use crate::geo::Point;
use crate::unit::UnitId;

use super::super::task::{
    TaskHeader, TASK_PRIORITY_AUTO_SURVEY, TASK_PRIORITY_EXPLORE, TASK_PRIORITY_FIND_MINES,
};
use super::{TaskCtx, TaskKind};

/// Default search radius of a free cell (`FindDestination`).
pub(crate) const SEARCH_RADIUS: i32 = 6;

/// `TaskAbstractSearch::IsVisited` — the cell was already visited by this unit.
fn is_visited(state: &super::SearchState, unit: UnitId, point: Point) -> bool {
    state
        .visited
        .iter()
        .any(|(visited_unit, visited_point)| *visited_unit == unit && *visited_point == point)
}

/// Mark the cell as visited (with a history size limit).
fn mark_visited(state: &mut super::SearchState, unit: UnitId, point: Point) {
    const MAX_HISTORY: usize = 64;

    if !is_visited(state, unit, point) {
        state.visited.push((unit, point));

        if state.visited.len() > MAX_HISTORY {
            state.visited.remove(0);
        }
    }
}

/// `TaskAbstractSearch::FindDestination(unit, radius)` — the nearest unvisited
/// passable cell around `point`.
fn find_destination(
    ctx: &TaskCtx<'_>,
    state: &super::SearchState,
    unit: UnitId,
    radius: i32,
) -> Option<Point> {
    let info = ctx.unit(unit)?;
    let mut best: Option<(i32, Point)> = None;

    let mut dy = -radius;
    while dy <= radius {
        let mut dx = -radius;
        while dx <= radius {
            let site = Point::new(state.point.x + dx as i16, state.point.y + dy as i16);

            if crate::ai::threat::in_bounds(site)
                && !is_visited(state, unit, site)
                && ctx.accessible(
                    info.unit_type,
                    info.team,
                    site,
                    crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                )
            {
                let ddx = i32::from(info.grid.x) - i32::from(site.x);
                let ddy = i32::from(info.grid.y) - i32::from(site.y);
                let distance = ddx * ddx + ddy * ddy;

                if best
                    .map(|(best_distance, _)| distance < best_distance)
                    .unwrap_or(true)
                {
                    best = Some((distance, site));
                }
            }

            dx += 1;
        }
        dy += 1;
    }

    best.map(|(_, site)| site)
}

/// The common `TaskAbstractSearch::BeginTurn`/`EndTurn` loop.
pub(crate) fn abstract_search_begin_turn(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    radius: i32,
) {
    let state = match kind {
        TaskKind::Survey { search } => search,
        TaskKind::Explore { search, .. } => search,
        TaskKind::FindMines { search } => search,
        _ => return,
    };

    state
        .units
        .retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));

    if state.units.is_empty() {
        ctx.remove_self(header.id);
        return;
    }

    for unit in state.units.clone() {
        let Some(info) = ctx.unit(unit) else { continue };

        // The unit is busy — do not touch it.
        if info.orders != UnitOrder::Await && info.orders != UnitOrder::Sentry {
            continue;
        }

        if ctx.tm.unit_task(unit) != Some(header.id) {
            continue;
        }

        if let Some(destination) = find_destination(ctx, state, unit, radius) {
            mark_visited(state, unit, destination);

            let priority = ctx.priority_of(header.id);
            let move_kind =
                TaskKind::move_to(unit, destination, 0, CautionLevel::AvoidNextTurnsFire);
            ctx.append_task(info.team, Some(header.id), priority, move_kind);
        }
    }
}

/// `TaskAutoSurvey::EndTurn` — circles around the center.
fn auto_survey_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::AutoSurvey {
        central_site,
        unit,
        radius,
        position,
        destination,
        continue_search,
        location_found,
        current_radius,
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

    if info.orders != UnitOrder::Await && info.orders != UnitOrder::Sentry {
        return;
    }

    if !*location_found {
        // Spiral: expand the radius until a free cell is found.
        let mut site = Point::new(central_site.x, central_site.y);

        let max_radius = (*radius as i16).max(1);
        let mut found = false;
        let mut r: i16 = *current_radius as i16;

        while r <= max_radius && !found {
            let mut dy = -r;
            while dy <= r && !found {
                let mut dx = -r;
                while dx <= r && !found {
                    let candidate = Point::new(central_site.x + dx, central_site.y + dy);

                    if crate::ai::threat::in_bounds(candidate)
                        && ctx.accessible(
                            info.unit_type,
                            info.team,
                            candidate,
                            crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                        )
                    {
                        site = candidate;
                        found = true;
                    }

                    dx += 1;
                }
                dy += 1;
            }
            r += 1;
        }

        *position = site;
        *destination = site;
        *location_found = found;

        if found {
            *current_radius = r as u16;
            let priority = ctx.priority_of(header.id);
            let move_kind = TaskKind::move_to(unit_id, site, 0, CautionLevel::AvoidNextTurnsFire);
            ctx.append_task(info.team, Some(header.id), priority, move_kind);
        } else {
            *continue_search = false;
            ctx.remove_self(header.id);
        }

        return;
    }

    // Arrived — start a new circle.
    let dx = i32::from(info.grid.x) - i32::from(destination.x);
    let dy = i32::from(info.grid.y) - i32::from(destination.y);

    if dx * dx + dy * dy <= 1 {
        *location_found = false;
        *current_radius = current_radius.saturating_add(1);
    }
}

/// `TaskFindMines::BeginTurn` — whether there are spotted mines.
fn find_mines_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::FindMines { search } = kind else {
        return;
    };

    // `AiPlayer::mine_map > 0` — the team's mine map.
    let player = &ctx.world.ai.players[header.team.0 as usize];
    let mut site: Option<Point> = None;

    for y in 0..crate::geo::MAP_HEIGHT {
        for x in 0..crate::geo::MAP_WIDTH {
            let index = y as usize * crate::geo::MAP_WIDTH as usize + x as usize;
            if player.mine_map[index] > 0 {
                site = Some(Point::new(x, y));
                break;
            }
        }
        if site.is_some() {
            break;
        }
    }

    match site {
        Some(point) => {
            search.point = point;
            abstract_search_begin_turn(kind, ctx, header, SEARCH_RADIUS);
        }
        None => ctx.remove_self(header.id),
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// `Init` for search classes.
pub fn init(kind: &mut TaskKind, _ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    match kind {
        TaskKind::Survey { .. } | TaskKind::Explore { .. } | TaskKind::FindMines { .. } => true,
        TaskKind::AutoSurvey { .. } => true,
        _ => false,
    }
}

/// `BeginTurn` for search classes.
pub fn begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Survey { .. } | TaskKind::Explore { .. } => {
            abstract_search_begin_turn(kind, ctx, header, SEARCH_RADIUS);
            true
        }
        TaskKind::FindMines { .. } => {
            find_mines_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::AutoSurvey { .. } => {
            auto_survey_end_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `EndTurn` for search classes.
pub fn end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Survey { .. } | TaskKind::Explore { .. } => {
            abstract_search_begin_turn(kind, ctx, header, SEARCH_RADIUS);
            true
        }
        TaskKind::FindMines { .. } => {
            find_mines_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::AutoSurvey { .. } => {
            auto_survey_end_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `Execute(unit)` for search classes — inspecting the cell.
pub fn execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::Survey { search }
        | TaskKind::Explore { search, .. }
        | TaskKind::FindMines { search } => {
            if !search.units.contains(&unit) {
                return false;
            }

            mark_visited(
                search,
                unit,
                ctx.unit(unit).map(|info| info.grid).unwrap_or(Point::ZERO),
            );
            true
        }
        TaskKind::AutoSurvey { .. } => {
            auto_survey_end_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `AddUnit(unit)` for search classes.
pub fn add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::Survey { search }
        | TaskKind::Explore { search, .. }
        | TaskKind::FindMines { search } => {
            if !search.units.contains(&unit) {
                search.units.push(unit);
                ctx.attach(header.id, unit);
            }
        }
        TaskKind::AutoSurvey { unit: slot, .. } => {
            if slot.is_none() {
                *slot = Some(unit);
                ctx.attach(header.id, unit);
            }
        }
        _ => {}
    }
}

/// `RemoveUnit(unit)` for search classes.
pub fn remove_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::Survey { search }
        | TaskKind::Explore { search, .. }
        | TaskKind::FindMines { search } => {
            search.units.retain(|id| *id != unit);

            if search.units.is_empty() {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::AutoSurvey { unit: slot, .. } => {
            if *slot == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        _ => {}
    }
}

/// `RemoveSelf()` for search classes.
pub fn remove_self(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Survey { search }
        | TaskKind::Explore { search, .. }
        | TaskKind::FindMines { search } => {
            search.units.clear();
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::AutoSurvey { .. } => {
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        _ => false,
    }
}

/// `ChildComplete(task)` for search classes.
pub fn child_complete(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    child: u32,
) -> bool {
    match kind {
        TaskKind::Survey { .. }
        | TaskKind::Explore { .. }
        | TaskKind::FindMines { .. }
        | TaskKind::AutoSurvey { .. } => {
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
        TaskKind::Survey { .. } | TaskKind::Explore { .. } | TaskKind::FindMines { .. } => {
            let _ = (unit, result);
            true
        }
        TaskKind::AutoSurvey { .. } => {
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

/// `EventUnitDestroyed(unit)` — remove the unit from the search.
pub fn event_unit_destroyed(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::Survey { search }
        | TaskKind::Explore { search, .. }
        | TaskKind::FindMines { search } => {
            search.units.retain(|id| *id != unit);

            if search.units.is_empty() {
                ctx.remove_self(header.id);
            }

            true
        }
        TaskKind::AutoSurvey { unit: slot, .. } => {
            if *slot == Some(unit) {
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

/// `TaskExplore::IsUnitUsable` (`taskexplore.cpp`).
pub fn explore_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    info.team == header.team
        && info.hits > 0
        && (info.is_ground() || info.is_amphibious())
        && info.orders != UnitOrder::Idle
}

/// Search task priorities (matching `task.hpp`).
pub const SEARCH_PRIORITIES: [u16; 3] = [
    TASK_PRIORITY_FIND_MINES,
    TASK_PRIORITY_AUTO_SURVEY,
    TASK_PRIORITY_EXPLORE,
];
