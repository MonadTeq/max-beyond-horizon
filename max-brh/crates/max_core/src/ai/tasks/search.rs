//! Map search: `TaskAbstractSearch`, `TaskSurvey`, `TaskExplore`,
//! `TaskAutoSurvey`, `TaskFindMines`.
//!
//! Oracle: `taskabstractsearch.cpp/.hpp`, `tasksearchdestination.cpp/.hpp`,
//! `tasksurvey.cpp/.hpp`, `taskexplore.cpp/.hpp`, `taskautosurvey.cpp/.hpp`,
//! `taskfindmines.cpp/.hpp`.
//!
//! # Model
//!
//! C++ `TaskAbstractSearch` holds the point of interest, a unit list and the
//! number of "requesters". Its `EndTurn` reminds every ready unit about the
//! finished move, sends damaged units to `TaskRepair` and — when the unit list is
//! empty and no request is pending — creates a `TaskObtainUnits` child. The owner
//! unit's `Execute` then calls `FindDestination`, which hands the unit over to
//! `TaskSearchDestination`: a spiral around the unit that stops at the nearest
//! cell the subclass does not consider `IsVisited`, then extends the site away
//! from the task point by the search radius and issues `TaskMove`.
//!
//! Here the same search runs synchronously inside the owner's `Execute`
//! (`find_search_destination`): the nearest ring around the unit is scanned first,
//! cells are filtered by [`SearchClass::is_visited`], accessibility
//! (`Access_IsAccessible` with `AccessModifier_EnemySameClassBlocks`) and the
//! damage-potential map, and the found cell is extended in the direction away from
//! the task point. The ring scan replaces the C++ spiral cursors (the candidate
//! set and the nearest-radius order are identical), see the deviation note in
//! `max-brh-docs/analysis/ai-port-status.md`.
//!
//! Subclasses:
//!
//! * `TaskSurvey` — resource survey (`IsVisited` — the team's cargo map bit);
//! * `TaskExplore` — map reconnaissance (`IsVisited` — surface type and
//!   `AiPlayer::info_map` `INFO_MAP_EXPLORED`, exactly `taskexplore.cpp:108`);
//!   its constructor flags request `SCOUT`/`AWAC`/`FASTBOAT`/`SUBMARNE`/`COMMANDO`;
//! * `TaskFindMines` — sweeping spotted minefields (`AiPlayer::mine_map`);
//! * `TaskAutoSurvey` — survey around the center in expanding circles.

use crate::access::{surface_type, SURFACE_TYPE_LAND, SURFACE_TYPE_WATER};
use crate::enums::{CautionLevel, Opponent, UnitType};
use crate::geo::{Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_WIDTH};
use crate::team::TeamId;
use crate::unit::{UnitId, UnitInfo};
use crate::unit_values::{ATTRIB_HITS, ATTRIB_SCAN};

use super::super::task::{
    task_retreat_from_danger, TaskHeader, TASK_PRIORITY_AUTO_SURVEY, TASK_PRIORITY_EXPLORE,
    TASK_PRIORITY_FIND_MINES, TASK_PRIORITY_REPAIR, TASK_TYPE_OBTAIN_UNITS,
};
use super::{TaskCtx, TaskKind};

/// Default search radius of a free cell (`FindDestination`) for the searches that
/// pass a literal radius in C++ (`TaskSurvey`/`TaskFindMines`: 1).
pub(crate) const SEARCH_RADIUS: i32 = 1;

/// `INFO_MAP_EXPLORED` — the reconnaissance map cell was seen by the team.
const INFO_MAP_EXPLORED: u8 = crate::enums::INFO_MAP_EXPLORED;

/// Which `TaskAbstractSearch` subclass runs the destination search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchClass {
    /// `TaskSurvey`.
    Survey,
    /// `TaskExplore`.
    Explore,
    /// `TaskFindMines`.
    FindMines,
}

/// The subclass of a search task (or `None` for any other task).
fn search_class(kind: &TaskKind) -> Option<SearchClass> {
    match kind {
        TaskKind::Survey { .. } => Some(SearchClass::Survey),
        TaskKind::Explore { .. } => Some(SearchClass::Explore),
        TaskKind::FindMines { .. } => Some(SearchClass::FindMines),
        _ => None,
    }
}

/// The `TaskAbstractSearch::units` list of a search task.
fn search_units(kind: &TaskKind) -> Option<&Vec<UnitId>> {
    match kind {
        TaskKind::Survey { search } => Some(&search.units),
        TaskKind::Explore { search, .. } => Some(&search.units),
        TaskKind::FindMines { search } => Some(&search.units),
        _ => None,
    }
}

/// Mutable access to the `TaskAbstractSearch` part of a search task.
fn search_units_mut(kind: &mut TaskKind) -> Option<&mut Vec<UnitId>> {
    match kind {
        TaskKind::Survey { search } => Some(&mut search.units),
        TaskKind::Explore { search, .. } => Some(&mut search.units),
        TaskKind::FindMines { search } => Some(&mut search.units),
        _ => None,
    }
}

/// `TaskAbstractSearch::point` (`GetPoint`).
fn search_point(kind: &TaskKind) -> Option<Point> {
    match kind {
        TaskKind::Survey { search } => Some(search.point),
        TaskKind::Explore { search, .. } => Some(search.point),
        TaskKind::FindMines { search } => Some(search.point),
        _ => None,
    }
}

/// The search radius C++ passes to `FindDestination`.
fn search_radius(class: SearchClass, unit: &UnitInfo) -> i32 {
    match class {
        // `TaskExplore::Execute` (`taskexplore.cpp:91`):
        // `unit.GetBaseValues()->GetAttribute(ATTRIB_SCAN)`.
        SearchClass::Explore => unit
            .base_values
            .get_attribute(ATTRIB_SCAN)
            .max(SEARCH_RADIUS),
        // `TaskSurvey::MoveFinishedCallback`/`TaskFindMines::Execute`:
        // `FindDestination(unit, 1)`.
        SearchClass::Survey | SearchClass::FindMines => SEARCH_RADIUS,
    }
}

/// In-map check (`Access_IsInsideBounds`).
fn is_inside_map(site: Point) -> bool {
    site.x >= 0 && site.y >= 0 && site.x < MAP_WIDTH && site.y < MAP_HEIGHT
}

/// `Task::IsReadyForOrders(this)` — ready *for this task*.
fn is_ready_for_task(ctx: &TaskCtx<'_>, header: TaskHeader, unit: UnitId) -> bool {
    ctx.tm.unit_task(unit) == Some(header.id) && ctx.ready_to_take_orders(unit)
}

// ---------------------------------------------------------------------------
// `IsVisited` implementations
// ---------------------------------------------------------------------------

/// `TaskExplore::IsVisited` (`taskexplore.cpp:108`).
///
/// Water units (`FASTBOAT`/`SUBMARNE`, i.e. `Escort`/`Submarine`) skip non-water
/// cells, every other unit skips non-land cells; after the surface check the
/// cell is visited only if the team's `info_map` has `INFO_MAP_EXPLORED`.
fn explore_is_visited(
    world: &crate::game::GameState,
    team: TeamId,
    unit: &UnitInfo,
    point: Point,
) -> bool {
    let surface = surface_type(world, point);

    if matches!(unit.unit_type, UnitType::Escort | UnitType::Submarine) {
        if surface != SURFACE_TYPE_WATER {
            return true;
        }
    } else if surface != SURFACE_TYPE_LAND {
        return true;
    }

    let player = &world.ai.players[team.0 as usize];
    let index = point.y as usize * MAP_WIDTH as usize + point.x as usize;

    player.info_map.get(index).copied().unwrap_or(0) & INFO_MAP_EXPLORED != 0
}

/// `TaskSurvey::IsVisited` (`tasksurvey.cpp:131`) — the team knows the tile's
/// resources (`ResourceManager_CargoMap & hash_team_id`).
fn survey_is_visited(world: &crate::game::GameState, team: TeamId, point: Point) -> bool {
    world
        .map
        .resources
        .get(point.x, point.y)
        .is_visible_to(team.0)
}

/// `TaskFindMines::IsVisited` (`taskfindmines.cpp:92`) — `AiPlayer::GetMineMapEntry(site) <= 0`.
fn find_mines_visited_for_team(world: &crate::game::GameState, team: TeamId, point: Point) -> bool {
    let player = &world.ai.players[team.0 as usize];
    let index = point.y as usize * MAP_WIDTH as usize + point.x as usize;

    player.mine_map.get(index).copied().unwrap_or(0) <= 0
}

/// The subclass predicate of `TaskAbstractSearch::IsVisited` (`sub_3DFCF`).
fn is_visited(
    world: &crate::game::GameState,
    team: TeamId,
    class: SearchClass,
    unit: &UnitInfo,
    point: Point,
) -> bool {
    match class {
        SearchClass::Survey => survey_is_visited(world, team, point),
        SearchClass::Explore => explore_is_visited(world, team, unit, point),
        SearchClass::FindMines => find_mines_visited_for_team(world, team, point),
    }
}

// ---------------------------------------------------------------------------
// Damage potential (`Search`/`SearchTrySite`)
// ---------------------------------------------------------------------------

/// `AiPlayer::GetDamagePotentialMap(unit, CAUTION_LEVEL_AVOID_ALL_DAMAGE)`.
fn damage_map(
    world: &crate::game::GameState,
    team: TeamId,
    unit: &UnitInfo,
) -> Option<crate::ai::ThreatMap> {
    super::super::strategy::get_damage_potential_map(
        world,
        team,
        unit,
        CautionLevel::AvoidAllDamage,
    )
}

/// `damage_potential_map[x][y] < 1` — the cell is free of known threats.
fn is_safe(map: Option<&crate::ai::ThreatMap>, site: Point) -> bool {
    match map {
        Some(map) => map.damage_at(i32::from(site.x), i32::from(site.y)) < 1,
        // Without an AI player (non-computer team) no threat is known.
        None => true,
    }
}

// ---------------------------------------------------------------------------
// `FindDestination` (TaskSearchDestination)
// ---------------------------------------------------------------------------

/// `TaskSearchDestination::sub_3DFCF` — not visited and enterable.
fn is_search_candidate(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    class: SearchClass,
    unit: &UnitInfo,
    map: Option<&crate::ai::ThreatMap>,
    site: Point,
) -> bool {
    if !is_inside_map(site) || is_visited(ctx.world, team, class, unit, site) || !is_safe(map, site)
    {
        return false;
    }

    ctx.accessible(
        unit.unit_type,
        unit.team,
        site,
        crate::access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS,
    )
}

/// `TaskSearchDestination::Search` + `SearchTrySite`
/// (`tasksearchdestination.cpp:171-325`).
///
/// `task_point` is `TaskAbstractSearch::GetPoint()`: the site the search is
/// oriented at. `SearchTrySite` walks from the found cell away from that point by
/// up to `radius` cells while the cells stay valid, and moves the unit there.
fn find_search_destination(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    class: SearchClass,
    task_point: Point,
    unit: &UnitInfo,
    radius: i32,
) -> Option<Point> {
    let map = damage_map(ctx.world, team, unit);
    let center = unit.grid;
    let max_radius = i32::from(MAP_WIDTH) + i32::from(MAP_HEIGHT);

    let mut found: Option<Point> = None;

    // The spiral of `TaskSearchDestination::SearchNextCircle` visits every cell
    // in order of the Chebyshev radius around the unit; the ring scan keeps that
    // radius order (top-left to bottom-right inside a ring).
    'rings: for current in 1..=max_radius {
        for dy in -current..=current {
            for dx in -current..=current {
                if dx.abs() != current && dy.abs() != current {
                    continue;
                }

                let site = Point::new(center.x + dx as i16, center.y + dy as i16);

                if is_search_candidate(ctx, team, class, unit, map.as_ref(), site) {
                    found = Some(site);
                    break 'rings;
                }
            }
        }
    }

    let site = found?;

    // `SearchTrySite`: `direction = UnitsManager_GetTargetAngle(site - point)`.
    let direction = crate::combat::get_target_angle(
        i32::from(site.x) - i32::from(task_point.x),
        i32::from(site.y) - i32::from(task_point.y),
    ) & 0x7;
    let offset = DIRECTION_OFFSETS[direction as usize];

    let mut best_site = site;
    let mut cursor = site;

    let mut i = radius;
    while i > 0 {
        cursor = Point::new(cursor.x + offset.x, cursor.y + offset.y);

        if !is_inside_map(cursor)
            || is_visited(ctx.world, team, class, unit, cursor)
            || !is_safe(map.as_ref(), cursor)
            || !ctx.accessible(
                unit.unit_type,
                unit.team,
                cursor,
                crate::access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS,
            )
        {
            break;
        }

        best_site = cursor;
        i -= 1;
    }

    Some(best_site)
}

// ---------------------------------------------------------------------------
// `TaskAbstractSearch` lifecycle
// ---------------------------------------------------------------------------

/// `TaskAbstractSearch::EndTurn` (`taskabstractsearch.cpp:79`).
///
/// Reminds ready units about the finished move, hands damaged units to
/// `TaskRepair` (apprentice opponents and above) and asks for a unit when the list
/// is empty. `BeginTurn` is `EndTurn()` in C++.
pub(crate) fn abstract_search_end_turn(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
) {
    let units = match search_units(kind) {
        Some(units) => units.clone(),
        None => return,
    };

    let opponent = ctx.world.ai.settings.opponent;

    for unit in units {
        let Some(info) = ctx.unit(unit) else { continue };

        if info.speed == 0 || !is_ready_for_task(ctx, header, unit) {
            continue;
        }

        let base_hits = info.base_values.get_attribute(ATTRIB_HITS);

        if i32::from(info.hits) <= base_hits / 2 && opponent >= Opponent::Apprentice as u8 {
            ctx.tm.remove_unit_tasks(ctx.world, unit);

            // `TaskAbstractSearch::RemoveUnit` removes the unit from the list; the
            // re-entrant handler wrote a stale copy back, so repeat it locally.
            if let Some(units) = search_units_mut(kind) {
                units.retain(|id| *id != unit);
            }

            let repair_kind = TaskKind::repair(unit);
            ctx.append_task(info.team, None, TASK_PRIORITY_REPAIR, repair_kind);
            continue;
        }

        ctx.remind_move_finished(unit, true);

        if !is_ready_for_task(ctx, header, unit) {
            break;
        }
    }

    let empty = search_units(kind)
        .map(|units| units.is_empty())
        .unwrap_or(false);
    let requestors = match kind {
        TaskKind::Survey { search } => Some(&mut search.requestors),
        TaskKind::Explore { search, .. } => Some(&mut search.requestors),
        TaskKind::FindMines { search } => Some(&mut search.requestors),
        _ => None,
    };

    if empty {
        if let Some(requestors) = requestors {
            if *requestors == 0 {
                *requestors = 1;
                obtain_unit(kind, ctx, header);
            }
        }
    }
}

/// `TaskAbstractSearch::ObtainUnit` — the subclass request: `TaskObtainUnits`
/// with the types the subclass wants.
fn obtain_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let Some(point) = search_point(kind) else {
        return;
    };

    let mut obtain_kind = TaskKind::obtain_units(point);

    match kind {
        // `TaskSurvey::ObtainUnit` (`tasksurvey.cpp:136`) — a SURVEYOR.
        TaskKind::Survey { .. } => {
            super::create::obtain_units_add_type(&mut obtain_kind, UnitType::Surveyor);
        }
        // `TaskFindMines::ObtainUnit` (`taskfindmines.cpp:94`) — a SURVEYOR.
        TaskKind::FindMines { .. } => {
            super::create::obtain_units_add_type(&mut obtain_kind, UnitType::Surveyor);
        }
        // `TaskExplore::ObtainUnit` (`taskexplore.cpp:124`) — the requested
        // unit types in the C++ order: SCOUT, AWAC, FASTBOAT, SUBMARNE, COMMANDO.
        TaskKind::Explore {
            obtain_requests, ..
        } => {
            for unit_type in [
                UnitType::Scout,
                UnitType::Awac,
                UnitType::Escort,
                UnitType::Submarine,
                UnitType::Infiltrator,
            ] {
                if obtain_requests.contains(&unit_type) {
                    super::create::obtain_units_add_type(&mut obtain_kind, unit_type);
                }
            }
        }
        _ => {}
    }

    let priority = ctx.priority_of(header.id);
    ctx.append_task(header.team, Some(header.id), priority, obtain_kind);
}

/// `TaskExplore::TaskExplore` (`taskexplore.cpp:36`) — the reconnaissance type
/// requests.
///
/// `SCOUT` is always requested; `AWAC`, `SUBMARNE` and `COMMANDO` only when the
/// type is buildable (and the opponent is `AVERAGE`/`EXPERT`). `FASTBOAT` is read
/// by `TaskExplore::ObtainUnit` but never initialized in the C++ constructor
/// (`taskexplore.cpp:40-45`); the port sets it when the type is buildable so the
/// sea strategy's naval reconnaissance units are actually requested (documented
/// deviation, phase A.6).
pub(crate) fn explore_init_obtain_requests(
    world: &crate::game::GameState,
    obtain_requests: &mut Vec<UnitType>,
) {
    obtain_requests.clear();

    let buildable = |unit_type: UnitType| super::super::build_orders::is_buildable(unit_type);
    let opponent = world.ai.settings.opponent;

    obtain_requests.push(UnitType::Scout);

    if buildable(UnitType::Awac) {
        obtain_requests.push(UnitType::Awac);
    }

    if buildable(UnitType::Escort) {
        obtain_requests.push(UnitType::Escort);
    }

    if buildable(UnitType::Submarine) && opponent >= Opponent::Average as u8 {
        obtain_requests.push(UnitType::Submarine);
    }

    if buildable(UnitType::Infiltrator) && opponent >= Opponent::Expert as u8 {
        obtain_requests.push(UnitType::Infiltrator);
    }
}

/// `TaskExplore::IsUnitUsable` (`taskexplore.cpp:50`).
///
/// Only a type the task currently requests is usable; a `SCOUT` or a `FASTBOAT`
/// is taken up to three times, every other type only once.
pub(crate) fn explore_is_unit_usable(
    world: &crate::game::GameState,
    _header: TaskHeader,
    kind: &TaskKind,
    unit: UnitId,
) -> bool {
    let TaskKind::Explore {
        search,
        obtain_requests,
    } = kind
    else {
        return false;
    };

    let Some(info) = world.unit(unit) else {
        return false;
    };

    if !obtain_requests.contains(&info.unit_type) {
        return false;
    }

    let count = search
        .units
        .iter()
        .filter(|id| {
            world
                .unit(**id)
                .map(|other| other.unit_type == info.unit_type)
                .unwrap_or(false)
        })
        .count();

    if matches!(info.unit_type, UnitType::Scout | UnitType::Escort) {
        count < 3
    } else {
        count == 0
    }
}

/// `TaskAbstractSearch::AddUnit` (`taskabstractsearch.cpp:55`).
fn search_add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    let Some(units) = search_units_mut(kind) else {
        return;
    };

    if !units.contains(&unit) {
        units.push(unit);
        ctx.attach(header.id, unit);
        ctx.remind_move_finished(unit, false);
    }
}

/// `TaskExplore::OnUnitReleased` (`taskexplore.cpp:102`) — the search is over for
/// this unit type: stop requesting it and free the unit.
fn release_search_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, unit: UnitId) {
    let Some(info) = ctx.unit(unit) else {
        return;
    };

    if let TaskKind::Explore {
        obtain_requests,
        search,
    } = kind
    {
        obtain_requests.retain(|unit_type| *unit_type != info.unit_type);
        search.units.retain(|id| *id != unit);
    }

    // `TaskManager.ClearUnitTasksAndRemindAvailable(&unit)`.
    ctx.clear_unit_tasks(unit, false);
}

/// The per-unit destination search (`Execute`).
fn search_execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(class) = search_class(kind) else {
        return false;
    };

    let Some(info) = ctx.unit(unit) else {
        return false;
    };

    if info.speed == 0 || !is_ready_for_task(ctx, header, unit) {
        return false;
    }

    // `TaskAbstractSearch::FindDestination`: retreat first, then one destination
    // per turn (`IsProcessingNeeded`), see `TaskManager::BeginTurn`.
    if task_retreat_from_danger(
        ctx.tm,
        ctx.world,
        header.id,
        unit,
        CautionLevel::AvoidAllDamage,
    ) {
        return true;
    }

    let processing = ctx
        .tm
        .task(header.id)
        .map(|task| task.needs_processing)
        .unwrap_or(false);

    if !processing {
        return true;
    }

    ctx.set_processing_needed(header.id, false);

    let radius = search_radius(class, &info);
    let task_point = search_point(kind).unwrap_or(info.grid);

    match find_search_destination(ctx, header.team, class, task_point, &info, radius) {
        Some(site) => {
            let priority = ctx.priority_of(header.id);
            let move_kind = TaskKind::move_to(unit, site, 0, CautionLevel::AvoidAllDamage);
            ctx.append_task(header.team, Some(header.id), priority, move_kind);
            true
        }
        None => {
            release_search_unit(kind, ctx, unit);
            true
        }
    }
}

// ---------------------------------------------------------------------------
// `TaskAutoSurvey`
// ---------------------------------------------------------------------------

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

    if info.orders != crate::enums::UnitOrder::Await
        && info.orders != crate::enums::UnitOrder::Sentry
    {
        return;
    }

    if !*location_found {
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

                    if is_inside_map(candidate)
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

    let dx = i32::from(info.grid.x) - i32::from(destination.x);
    let dy = i32::from(info.grid.y) - i32::from(destination.y);

    if dx * dx + dy * dy <= 1 {
        *location_found = false;
        *current_radius = current_radius.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// `TaskFindMines`
// ---------------------------------------------------------------------------

/// `TaskFindMines::BeginTurn` (`taskfindmines.cpp:40`) — request enough sweepers
/// for the spotted mines that are not covered by threats.
fn find_mines_request_sweepers(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::FindMines { search } = kind else {
        return;
    };

    let Some(first) = search.units.first().copied() else {
        return;
    };

    if search.requestors != 0 {
        return;
    }

    let Some(info) = ctx.unit(first) else {
        return;
    };

    let map = damage_map(ctx.world, header.team, &info);
    let player = &ctx.world.ai.players[header.team.0 as usize];

    let mut valuable_sites: u32 = 0;

    for index in 0..crate::geo::MAP_TILES {
        let site = Point::new(
            (index % MAP_WIDTH as usize) as i16,
            (index / MAP_WIDTH as usize) as i16,
        );

        if player.mine_map.get(index).copied().unwrap_or(0) > 0 && is_safe(map.as_ref(), site) {
            valuable_sites += 1;
        }
    }

    // `valuable_sites = std::sqrt(valuable_sites) * 0.25` — integer square root
    // keeps the comparison deterministic.
    let valuable = crate::pathfinding::isqrt(valuable_sites) / 4;

    if (search.units.len() as u32) < valuable {
        search.requestors += 1;
        obtain_unit(kind, ctx, header);
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// `Init` for search classes.
pub fn init(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, _header: TaskHeader) -> bool {
    match kind {
        TaskKind::Explore {
            obtain_requests, ..
        } => {
            // `TaskExplore::TaskExplore` — the request flags are computed in the
            // constructor; `init` is the first handler that owns the game state.
            explore_init_obtain_requests(ctx.world, obtain_requests);
            true
        }
        TaskKind::Survey { .. } | TaskKind::FindMines { .. } => true,
        TaskKind::AutoSurvey { .. } => true,
        _ => false,
    }
}

/// `BeginTurn` for search classes.
pub fn begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::FindMines { .. } => {
            find_mines_request_sweepers(kind, ctx, header);
            abstract_search_end_turn(kind, ctx, header);
            true
        }
        TaskKind::Survey { .. } | TaskKind::Explore { .. } => {
            // `TaskAbstractSearch::BeginTurn` — `EndTurn()`.
            abstract_search_end_turn(kind, ctx, header);
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
        TaskKind::Survey { .. } | TaskKind::Explore { .. } | TaskKind::FindMines { .. } => {
            abstract_search_end_turn(kind, ctx, header);
            true
        }
        TaskKind::AutoSurvey { .. } => {
            auto_survey_end_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `Execute(unit)` for search classes.
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

            search_execute(kind, ctx, header, unit)
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
        TaskKind::Survey { .. } | TaskKind::Explore { .. } | TaskKind::FindMines { .. } => {
            search_add_unit(kind, ctx, header, unit);
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
        TaskKind::Survey { .. } | TaskKind::FindMines { .. } => {
            let empty = {
                let Some(units) = search_units_mut(kind) else {
                    return;
                };
                units.retain(|id| *id != unit);
                units.is_empty()
            };

            if empty {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::Explore { search, .. } => {
            // C++ `TaskAbstractSearch::RemoveUnit` only drops the unit; the task
            // stays alive to request the remaining reconnaissance types.
            search.units.retain(|id| *id != unit);
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
    if search_class(kind).is_none() {
        return matches!(kind, TaskKind::AutoSurvey { .. });
    }

    let _ = header;

    // `TaskAbstractSearch::ChildComplete` (`taskabstractsearch.cpp:65`): an
    // obtain request finished, open the next one.
    if ctx.tm.task(child).map(|task| task.get_type()) == Some(TASK_TYPE_OBTAIN_UNITS) {
        let requestors = match kind {
            TaskKind::Survey { search } => Some(&mut search.requestors),
            TaskKind::Explore { search, .. } => Some(&mut search.requestors),
            TaskKind::FindMines { search } => Some(&mut search.requestors),
            _ => None,
        };

        if let Some(requestors) = requestors {
            *requestors -= 1;

            if *requestors < 0 {
                *requestors = 0;
            }
        }
    }

    true
}

/// Notification about a child `TaskMove` completion.
pub fn move_finished(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
    _result: i8,
) -> bool {
    // `TaskExplore` passes no callback; `TaskSurvey` re-runs `FindDestination` on
    // a blocked move, which the next `Execute` covers as well.
    false
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
        TaskKind::Survey { .. } | TaskKind::FindMines { .. } => {
            let empty = {
                let Some(units) = search_units_mut(kind) else {
                    return false;
                };
                units.retain(|id| *id != unit);
                units.is_empty()
            };

            if empty {
                ctx.remove_self(header.id);
            }

            true
        }
        TaskKind::Explore { search, .. } => {
            search.units.retain(|id| *id != unit);
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

/// Search task priorities (matching `task.hpp`).
pub const SEARCH_PRIORITIES: [u16; 3] = [
    TASK_PRIORITY_FIND_MINES,
    TASK_PRIORITY_AUTO_SURVEY,
    TASK_PRIORITY_EXPLORE,
];
