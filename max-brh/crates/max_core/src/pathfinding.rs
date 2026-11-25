//! Pathfinding: ground/air/builder paths, the request queue, PathFill.
//!
//! Oracle: `max-cpp-port/src/` — `paths.cpp` (`Paths_RequestPath`,
//! `Paths_GetAirPath`, `Paths_GetAngle`, `Paths_UpdateAngle`,
//! `Paths_CalculateStep`, `Paths_IsOccupied`, `Paths_IsAccessible` semantics),
//! `groundpath.cpp` (`GroundPath::Execute`, `AppendLinearSteps`,
//! `GetPosition`, `GetMovementCost`), `airpath.cpp` (`AirPath`), `builderpath.cpp`,
//! `searcher.cpp`/`paths_manager.cpp` (`Searcher` — search over the cost map),
//! `pathrequest.cpp` (`PathRequest::Finish`, `AssignGroundPath`),
//! `accessmap.cpp` (`AccessMap::Init` — the cost map),
//! `pathfill.cpp` (`PathFill`), `maxfloodfill.cpp`, DESIGN.md §1.4 item 2
//! (**synchronous, without threads**) and risk 2 (results are applied in
//! request submission order).
//!
//! # Synchronous model
//!
//! C++ puts `PathRequest`s into `PathsManager` and processes them in worker
//! threads. `max_core` does not use threads: [`request_path`] puts a request into
//! the [`crate::game::GameState::path_requests`] queue, [`poll_results`] computes and
//! applies **all** requests in queue order (`PushFront` for humans,
//! `PushBack` for computers — as in `Paths_RequestPath`). The application order
//! therefore matches submission order rather than worker completion
//! order (DESIGN risk 2).
//!
//! # Search
//!
//! C++ `Searcher` is a bidirectional A* over `AccessMap` initialized with
//! a straight route. Here a deterministic A* search over the same
//! cost map is ported: directions are iterated strictly `0..8`
//! (`DIRECTION_OFFSETS`), the queue is by `(cost, insertion sequence number)`,
//! so equally valid paths always resolve identically. A diagonal costs
//! `cost * 3 / 2`, as in `Searcher::ForwardSearch`. Air paths are a straight
//! segment (`Paths_GetAirPath`) without obstacle avoidance.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::access;
use crate::enums::unit_flags::HOVERING;
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::{PathStep, Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use crate::unit::{AirPath, GroundPath, UnitId, UnitInfo, UnitListKind, UnitPath};
use crate::unit_order;
use crate::unit_values::ATTRIB_RANGE;

/// `PathRequest_CautionLevels[0]` — without caution (P5 does not model
/// AI damage maps; danger warnings are considered zero).
pub const CAUTION_LEVEL_NONE: i32 = 0;

/// `SHRT_MAX` — "no limit" in `Paths_GetAirPath`.
const SHRT_MAX: i32 = 32767;

/// A path request — C++ `class PathRequest` (`pathrequest.hpp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRequest {
    /// Client unit (`client`).
    pub unit: UnitId,
    /// `AccessModifier_*` flags (`flags`).
    pub flags: u32,
    /// Destination at submission time (`point`).
    pub destination: Point,
    /// Minimum distance to the target (`minimum_distance`) — the attack radius
    /// squared for `MOVE_TO_ATTACK`.
    pub minimum_distance: i32,
    /// Request to board a transport (`board_transport`).
    pub board_transport: bool,
    /// Cost limit (`max_cost`; C++ defaults to `INT16_MAX`).
    pub max_cost: i32,
}

impl PathRequest {
    /// `PathRequest::PathRequest` (`pathrequest.cpp:34`).
    pub fn new(unit: UnitId, flags: u32, destination: Point) -> Self {
        Self {
            unit,
            flags,
            destination,
            minimum_distance: 0,
            board_transport: false,
            max_cost: i32::from(i16::MAX),
        }
    }
}

/// `Paths_RequestPath` (`paths.cpp:46`).
///
/// Returns `true` if the request was enqueued. With an invalid
/// destination the unit receives `BlockedOnPathRequest(false)` and the function
/// returns `false` (as in C++).
pub(crate) fn request_path(state: &mut GameState, id: UnitId, mode: u32) -> bool {
    let Some(unit) = state.unit(id).cloned() else {
        return false;
    };

    let mut data = state.order_data(id);
    let destination = if unit.orders == UnitOrder::MoveToAttack {
        match unit
            .enemy_unit
            .and_then(|enemy| state.unit(enemy).map(|enemy| enemy.grid))
        {
            Some(enemy_grid) => {
                data.move_to = enemy_grid;
                state.set_order_data(id, data);
                enemy_grid
            }
            None => data.move_to,
        }
    } else {
        data.move_to
    };

    if destination.x < 0
        || destination.y < 0
        || destination.x >= MAP_WIDTH
        || destination.y >= MAP_HEIGHT
    {
        if let Some(mut unit) = state.unit(id).cloned() {
            unit_order::blocked_on_path_request(&mut unit, false, state.disable_fire);
            state.write_unit(id, unit);
        }
        return false;
    }

    let mut request = PathRequest::new(id, mode, destination);
    request.board_transport = unit.orders == UnitOrder::MoveToUnit;

    if mode & access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0 {
        request.minimum_distance = 3;
    }

    if unit.orders == UnitOrder::MoveToAttack {
        let range = unit.base_values.get_attribute(ATTRIB_RANGE);
        request.minimum_distance = range * range;
    }

    if unit.state == UnitOrderState::NewOrder {
        state.path_requests.retain(|queued| queued.unit != id);
    }

    // Identical requests of one unit are not duplicated (otherwise `PursueEnemy`
    // would enqueue a request every tick until the path is ready).
    if state
        .path_requests
        .iter()
        .any(|queued| queued.unit == id && queued.destination == destination)
    {
        return true;
    }

    if state.teams[unit.team.0 as usize].team_type == crate::team::TeamKind::Computer {
        state.path_requests.push_back(request);
    } else {
        state.path_requests.push_front(request);
    }

    true
}

/// `PathsManager::DispatchJobs` — in the synchronous model the work is done in
/// [`poll_results`]; the call site is kept for tick order (DESIGN §5.1).
pub(crate) fn dispatch_jobs(_state: &mut GameState) {}

/// `PathsManager::PollResults` — computes and applies all requests in
/// submission order.
pub(crate) fn poll_results(state: &mut GameState) {
    let mut requests: Vec<PathRequest> = Vec::new();
    while let Some(request) = state.path_requests.pop_front() {
        requests.push(request);
    }

    for request in requests {
        apply_request(state, &request);
    }
}

/// Computes the path for a request and applies it via `PathRequest::Finish`.
fn apply_request(state: &mut GameState, request: &PathRequest) {
    let Some(unit) = state.unit(request.unit).cloned() else {
        return;
    };

    if unit.hits == 0 {
        return;
    }

    let path = find_ground_path_inner(
        state,
        &unit,
        request.destination,
        request.flags,
        request.minimum_distance,
        request.board_transport,
    );
    finish_path(state, request, path);
}

/// `PathRequest::Finish` (`pathrequest.cpp:145`).
fn finish_path(state: &mut GameState, request: &PathRequest, path: Option<GroundPath>) {
    let Some(unit) = state.unit(request.unit).cloned() else {
        return;
    };

    let order = unit.orders;
    let order_state_accepts = matches!(
        unit.state,
        UnitOrderState::NewOrder | UnitOrderState::MoveGettingPath
    ) || (order == UnitOrder::MoveToAttack
        && unit.state == UnitOrderState::ExecutingOrder);

    if unit.hits == 0
        || !matches!(
            order,
            UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::Build | UnitOrder::MoveToAttack
        )
        || !order_state_accepts
    {
        return;
    }

    let previous_state = unit.state;
    let mut unit = unit;
    let path_steps = path.as_ref().map(|path| path.steps.clone());
    unit.path = path.map(UnitPath::Ground);

    if order == UnitOrder::MoveToAttack {
        if let Some(steps) = path_steps {
            assign_ground_path(&mut unit, &steps, request.destination);
        }
    }

    if unit.path.is_none() {
        if order == UnitOrder::MoveToAttack {
            let range = unit.base_values.get_attribute(ATTRIB_RANGE);
            if access::squared_distance(unit.grid, request.destination) <= range * range {
                unit_order::set_new_order_int(
                    &mut unit,
                    UnitOrder::MoveToAttack,
                    UnitOrderState::ExecutingOrder,
                );
                state.write_unit(request.unit, unit);
                return;
            }
        }

        unit_order::blocked_on_path_request(&mut unit, false, state.disable_fire);

        if previous_state == UnitOrderState::NewOrder {
            access::clear_unit_group(state, request.unit);
        }
    } else if order == UnitOrder::Build {
        // `UnitsManager_StartBuild` — P3 site logic; the state does not change.
        unit.state = UnitOrderState::InProgress;
    } else if previous_state == UnitOrderState::MoveGettingPath {
        if order == UnitOrder::MoveToAttack {
            unit.state = UnitOrderState::ExecutingOrder;
        } else {
            unit_order::restore_orders(&mut unit);
        }
    } else {
        unit.state = UnitOrderState::InProgress;
    }

    state.write_unit(request.unit, unit);

    if state.group_members(request.unit).is_some() {
        access::process_new_group_order(state, request.unit);
    }
}

/// `PathRequest::AssignGroundPath` (`pathrequest.cpp:115`): the path is truncated
/// at the point from which the target is already within attack range.
fn assign_ground_path(unit: &mut UnitInfo, steps: &[PathStep], destination: Point) {
    let range = unit.base_values.get_attribute(ATTRIB_RANGE);
    let range = range * range;
    let mut position = unit.grid;
    let mut used = 0usize;

    while used < steps.len() && access::squared_distance(position, destination) > range {
        position.x += i16::from(steps[used].x);
        position.y += i16::from(steps[used].y);
        used += 1;
    }

    if used == 0 {
        unit.path = None;
        return;
    }

    let mut path = GroundPath {
        end_x: position.x,
        end_y: position.y,
        step_index: 0,
        steps: steps[..used].to_vec(),
    };

    if unit.is_air() {
        // C++: `SetEndXY(position)` + `Paths_GetAirPath(unit)`.
        path.end_x = position.x;
        path.end_y = position.y;
    }

    unit.path = Some(UnitPath::Ground(path));
}

// ---------------------------------------------------------------------------
// Cost map (AccessMap)
// ---------------------------------------------------------------------------

/// Builds the entry-cost map for a unit — a port of `AccessMap::Init`
/// (`accessmap.cpp:372`). `0` — impassable.
pub(crate) fn build_access_costs(state: &GameState, unit: &UnitInfo, flags: u32) -> [u8; MAP_TILES] {
    let mut costs = [0u8; MAP_TILES];
    let is_air = unit.is_air();

    if is_air {
        costs.fill(4);
        process_mobile_air_units(state, unit, flags, &mut costs);
        return costs;
    }

    let land_type = access::unit_type_land_type(unit.unit_type);
    let water_value =
        if land_type & access::SURFACE_TYPE_LAND != 0 && unit.unit_type != UnitType::Surveyor {
            8
        } else {
            4
        };

    for index in 0..MAP_TILES {
        let x = (index % MAP_WIDTH as usize) as i16;
        let y = (index / MAP_WIDTH as usize) as i16;
        let surface = state.map.surface_at(x, y).bits();
        costs[index] = if surface == access::SURFACE_TYPE_LAND {
            if land_type & access::SURFACE_TYPE_LAND != 0 {
                4
            } else {
                0
            }
        } else if surface == access::SURFACE_TYPE_COAST {
            if land_type & access::SURFACE_TYPE_COAST != 0 {
                4
            } else {
                0
            }
        } else if surface == access::SURFACE_TYPE_WATER {
            if land_type & access::SURFACE_TYPE_WATER != 0 {
                water_value
            } else {
                0
            }
        } else {
            0
        };
    }

    process_ground_cover(state, unit, land_type, &mut costs);
    process_mobile_land_sea_units(state, unit, flags, &mut costs);
    process_mobile_air_units(state, unit, flags, &mut costs);
    process_stationary_units(state, unit, &mut costs);

    costs
}

fn tile_index(grid: Point) -> Option<usize> {
    if grid.x < 0 || grid.y < 0 || grid.x >= MAP_WIDTH || grid.y >= MAP_HEIGHT {
        None
    } else {
        Some(grid.y as usize * MAP_WIDTH as usize + grid.x as usize)
    }
}

fn block_cell(costs: &mut [u8; MAP_TILES], grid: Point, value: u8) {
    if let Some(index) = tile_index(grid) {
        costs[index] = value;
    }
}

fn block_building(costs: &mut [u8; MAP_TILES], grid: Point, value: u8) {
    block_cell(costs, grid, value);
    block_cell(costs, Point::new(grid.x + 1, grid.y), value);
    block_cell(costs, Point::new(grid.x, grid.y + 1), value);
    block_cell(costs, Point::new(grid.x + 1, grid.y + 1), value);
}

fn is_visible_or_detected(unit: &UnitInfo, team: crate::team::TeamId) -> bool {
    unit.is_visible_to_team(team) || unit.is_detected_by_team(team)
}

/// `AccessMap::ProcessGroundCover` (`accessmap.cpp:196`).
fn process_ground_cover(
    state: &GameState,
    unit: &UnitInfo,
    land_type: u8,
    costs: &mut [u8; MAP_TILES],
) {
    let team = unit.team;
    let process_paved = land_type & access::SURFACE_TYPE_LAND != 0
        && unit.laying_state != 2
        && unit.laying_state != 1;

    for id in state.unit_list(UnitListKind::GroundCover) {
        let Some(cover) = state.unit(*id) else {
            continue;
        };
        if !is_visible_or_detected(cover, team) {
            continue;
        }

        match cover.unit_type {
            UnitType::Bridge => {
                if land_type & (access::SURFACE_TYPE_LAND | access::SURFACE_TYPE_WATER) != 0 {
                    block_cell(costs, cover.grid, 4);
                }
            }
            UnitType::WaterPlatform => {
                if land_type & access::SURFACE_TYPE_LAND != 0 {
                    block_cell(costs, cover.grid, 4);
                } else {
                    block_cell(costs, cover.grid, 0);
                }
            }
            UnitType::Road | UnitType::SmallSlab | UnitType::LargeSlab => {
                if process_paved {
                    block_cell(costs, cover.grid, 2);
                    if cover.is_quad() {
                        block_building(costs, cover.grid, 2);
                    }
                }
            }
            UnitType::LargeTape | UnitType::SmallTape => {
                block_cell(costs, cover.grid, 0);
                if cover.is_quad() {
                    block_building(costs, cover.grid, 0);
                }
            }
            UnitType::LandMine | UnitType::SeaMine => {
                if cover.team != team && cover.is_detected_by_team(team) {
                    block_cell(costs, cover.grid, 0);
                }
            }
            _ => {}
        }
    }
}

/// `AccessMap::ProcessMobileUnits` for ground/naval units.
fn process_mobile_land_sea_units(
    state: &GameState,
    unit: &UnitInfo,
    flags: u32,
    costs: &mut [u8; MAP_TILES],
) {
    let team = unit.team;

    for id in state.unit_list(UnitListKind::MobileLandSea) {
        let Some(other) = state.unit(*id) else {
            continue;
        };
        if other.orders != UnitOrder::Idle && other.is_visible_to_team(team) {
            if flags & access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0
                || (flags & access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0
                    && other.team != team)
            {
                block_cell(costs, other.grid, 0);

                if other.path.is_some()
                    && other.state != UnitOrderState::ExecutingOrder
                    && other.id != unit.id
                {
                    let position = ground_path_position(state, other);
                    block_cell(costs, position, 0);
                }
            }
        }
    }
}

/// `AccessMap::ProcessMobileUnits` for air (and blocking of landed aircraft).
fn process_mobile_air_units(
    state: &GameState,
    unit: &UnitInfo,
    flags: u32,
    costs: &mut [u8; MAP_TILES],
) {
    let team = unit.team;
    let is_air_pathfinder = unit.is_air();

    for id in state.unit_list(UnitListKind::MobileAir) {
        let Some(other) = state.unit(*id) else {
            continue;
        };

        if other.orders != UnitOrder::Idle && other.is_visible_to_team(team) {
            if flags & access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0
                || (flags & access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0
                    && other.team != team)
            {
                if other.is_air() && !is_air_pathfinder && other.flags & HOVERING != 0 {
                    continue;
                }

                block_cell(costs, other.grid, 0);

                if other.path.is_some()
                    && other.state != UnitOrderState::ExecutingOrder
                    && other.id != unit.id
                {
                    if let Some(UnitPath::Air(path)) = other.path.as_ref() {
                        block_cell(costs, air_path_position(other, path), 0);
                    }
                }
            }
        }

        if !is_air_pathfinder
            && other.is_air()
            && other.flags & HOVERING == 0
            && other.is_visible_to_team(team)
            && (other.orders != UnitOrder::Idle || !other.is_mobile())
        {
            block_cell(costs, other.grid, 0);
        }
    }
}

/// `AccessMap::ProcessStationaryUnits` (`accessmap.cpp:62`).
fn process_stationary_units(state: &GameState, unit: &UnitInfo, costs: &mut [u8; MAP_TILES]) {
    let team = unit.team;

    for id in state.unit_list(UnitListKind::Stationary) {
        let Some(other) = state.unit(*id) else {
            continue;
        };
        if other.unit_type != UnitType::Connector && is_visible_or_detected(other, team) {
            block_cell(costs, other.grid, 0);
            if other.is_quad() {
                block_building(costs, other.grid, 0);
            }
        }
    }
}

/// `GroundPath::GetPosition` (`groundpath.cpp:94`) — predicted position of a moving
/// unit for the current turn (to block its "future" cell).
pub(crate) fn ground_path_position(state: &GameState, unit: &UnitInfo) -> Point {
    let mut position = unit.grid;
    let speed = i32::from(unit.speed) * 4;

    let Some(UnitPath::Ground(path)) = unit.path.as_ref() else {
        return position;
    };

    if unit.orders == UnitOrder::Build || unit.state != UnitOrderState::ExecutingOrder || speed <= 0
    {
        return position;
    }

    let mut move_fraction = i32::from(unit.move_fraction);

    for step in path.steps.iter().skip(path.step_index as usize) {
        if step.x == 0 && step.y == 0 {
            continue;
        }

        let point = Point::new(
            position.x + i16::from(step.x),
            position.y + i16::from(step.y),
        );
        if point.x < 0 || point.y < 0 || point.x >= MAP_WIDTH || point.y >= MAP_HEIGHT {
            continue;
        }

        let mut step_cost =
            access::is_accessible_unit(state, unit, point, access::ACCESS_MODIFIER_NONE) as i32;

        if step.x != 0 && step.y != 0 {
            step_cost = (step_cost * 3) / 2;
        }

        if move_fraction > step_cost {
            move_fraction = step_cost;
        }

        step_cost -= move_fraction;
        move_fraction = 0;

        if step_cost <= speed {
            position = point;
        } else {
            break;
        }
    }

    position
}

// ---------------------------------------------------------------------------
// Ground path search
// ---------------------------------------------------------------------------

/// Finds a path `unit.grid → destination` over the cost map.
///
/// `minimum_distance` — the squared radius: the search stops at any cell
/// from which the target is already reachable (C++ `BuildAccessMap` + `IsProcessed`).
pub(crate) fn find_ground_path(
    state: &GameState,
    unit: &UnitInfo,
    destination: Point,
    flags: u32,
    minimum_distance: i32,
) -> Option<GroundPath> {
    find_ground_path_inner(state, unit, destination, flags, minimum_distance, false)
}

/// The same with the transport boarding flag (`PathRequest::board_transport`): the
/// receiver's cell is marked passable, as `PathsManager::BuildAccessMap`.
fn find_ground_path_inner(
    state: &GameState,
    unit: &UnitInfo,
    destination: Point,
    flags: u32,
    minimum_distance: i32,
    board_transport: bool,
) -> Option<GroundPath> {
    if destination.x < 0
        || destination.y < 0
        || destination.x >= MAP_WIDTH
        || destination.y >= MAP_HEIGHT
    {
        return None;
    }

    let start = unit.grid;
    if start == destination {
        return None;
    }

    let start_index = tile_index(start)?;
    let destination_index = tile_index(destination)?;
    let mut costs = build_access_costs(state, unit, flags);

    if board_transport {
        if let Some(receiver) = access::get_receiver_unit(state, unit, destination) {
            if let Some(receiver_unit) = state.unit(receiver) {
                if receiver_unit.is_quad() {
                    block_building(&mut costs, receiver_unit.grid, 2);
                } else {
                    block_cell(&mut costs, receiver_unit.grid, 2);
                }
            }
        }
    }

    // Trivial case: an adjacent cell (`paths_manager.cpp:415`).
    if access::squared_distance(start, destination) <= 2 {
        let receiver_reachable =
            board_transport && access::get_receiver_unit(state, unit, destination).is_some();
        let viable = receiver_reachable
            || (if flags & access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0 {
                let mut costs = costs;
                costs[start_index] = 2;
                costs[destination_index] != 0
            } else {
                access::is_accessible(state, unit.unit_type, unit.team, destination, flags) != 0
            });

        return viable.then(|| GroundPath {
            end_x: destination.x,
            end_y: destination.y,
            step_index: 0,
            steps: vec![PathStep::new(
                (destination.x - start.x) as i8,
                (destination.y - start.y) as i8,
            )],
        });
    }

    let mut cost_map = [u32::MAX; MAP_TILES];
    let mut direction_map = [i8::MIN; MAP_TILES];
    let mut explored = [false; MAP_TILES];
    let mut heap: BinaryHeap<Reverse<(u32, u32, usize)>> = BinaryHeap::new();
    let mut sequence = 0u32;

    cost_map[start_index] = 0;
    heap.push(Reverse((0, sequence, start_index)));

    let in_range = |index: usize| -> bool {
        if minimum_distance <= 0 {
            return index == destination_index;
        }
        let x = (index % MAP_WIDTH as usize) as i16;
        let y = (index / MAP_WIDTH as usize) as i16;
        access::squared_distance(Point::new(x, y), destination) <= minimum_distance
    };

    let mut reached = None;

    while let Some(Reverse((cost, _, index))) = heap.pop() {
        if explored[index] {
            continue;
        }
        explored[index] = true;

        if index != start_index && in_range(index) {
            reached = Some(index);
            break;
        }

        let x = (index % MAP_WIDTH as usize) as i16;
        let y = (index / MAP_WIDTH as usize) as i16;

        for direction in 0..8usize {
            let offset = DIRECTION_OFFSETS[direction];
            let next = Point::new(x + offset.x, y + offset.y);
            let Some(next_index) = tile_index(next) else {
                continue;
            };
            if explored[next_index] {
                continue;
            }

            let step_cost = costs[next_index];

            if step_cost == 0 {
                continue;
            }

            let mut step_cost = u32::from(step_cost);
            if direction & 1 != 0 {
                step_cost = (step_cost * 3) / 2;
            }

            let next_cost = cost.saturating_add(step_cost);
            if next_cost < cost_map[next_index] {
                cost_map[next_index] = next_cost;
                direction_map[next_index] = direction as i8;
                sequence = sequence.wrapping_add(1);
                heap.push(Reverse((next_cost, sequence, next_index)));
            }
        }
    }

    let reached = reached?;

    let mut reversed = Vec::new();
    let mut position = reached;
    while position != start_index {
        let direction = direction_map[position];
        if direction < 0 {
            return None;
        }
        let offset = DIRECTION_OFFSETS[(direction as usize) & 7];
        reversed.push(PathStep::new(offset.x as i8, offset.y as i8));
        position = tile_index(Point::new(
            (position % MAP_WIDTH as usize) as i16 - offset.x,
            (position / MAP_WIDTH as usize) as i16 - offset.y,
        ))?;
    }
    reversed.reverse();

    if reversed.is_empty() {
        return None;
    }

    Some(GroundPath {
        end_x: destination.x,
        end_y: destination.y,
        step_index: 0,
        steps: reversed,
    })
}

/// `GroundPath::AppendLinearSteps` (`groundpath.cpp:582`) — a straight
/// route (a P4-compatible helper; used in the simulation as a fallback).
pub fn append_linear_steps(from: Point, to: Point) -> Vec<PathStep> {
    let mut steps = Vec::new();
    let mut x = i32::from(to.x) - i32::from(from.x);
    let mut y = i32::from(to.y) - i32::from(from.y);

    while x != 0 || y != 0 {
        let step_x = x.signum() as i8;
        let step_y = y.signum() as i8;
        steps.push(PathStep::new(step_x, step_y));
        x -= i32::from(step_x);
        y -= i32::from(step_y);
    }

    steps
}

// ---------------------------------------------------------------------------
// Air paths and missiles
// ---------------------------------------------------------------------------

/// Integer square root (`isqrt`), without `f64` (DESIGN §1.4 item 2).
pub fn isqrt(value: u32) -> u32 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    // `(value + 1) / 2`, but without the overflow at `value == u32::MAX`.
    let mut y = (value >> 1) + (value & 1);
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

/// Integer square root for `u64`.
fn isqrt_u64(value: u64) -> u64 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

/// `sqrt(dx² + dy²) * 4 + 0.5` in integer arithmetic (`paths.cpp`).
pub fn scaled_distance4(distance_x: i32, distance_y: i32) -> i32 {
    let squared = i64::from(distance_x) * i64::from(distance_x)
        + i64::from(distance_y) * i64::from(distance_y);
    let squared = squared as u64;
    let root = isqrt_u64(squared * 16);
    // floor(4*sqrt(d) + 0.5): 4*sqrt(d) >= root + 0.5
    //   ⇔ 64*d >= 4*root² + 4*root + 1.
    let round_up = u128::from(squared) * 64
        >= 4 * u128::from(root) * u128::from(root) + 4 * u128::from(root) + 1;
    (root + u64::from(round_up)) as i32
}

/// `Paths_GetAirPath` (`paths.cpp:97`) — a straight air segment.
///
/// Mutates the unit's `speed`/`move_fraction`/`max_velocity`, as C++ does.
pub(crate) fn paths_get_air_path(
    state: &GameState,
    unit: &mut UnitInfo,
    target: Point,
) -> Option<AirPath> {
    let mut end_x = target.x;
    let mut end_y = target.y;

    if unit.path.is_some() {
        let (x, y) = unit
            .path
            .as_ref()
            .map(UnitPath::end)
            .unwrap_or((end_x, end_y));
        end_x = x;
        end_y = y;
        unit.path = None;
    }

    let distance_x = i32::from(end_x) - i32::from(unit.grid.x);
    let distance_y = i32::from(end_y) - i32::from(unit.grid.y);
    // `AirPath::AirPath`: an air unit gets the angle to the target, the rest —
    // the current angle (a missile flies where `PrepareFire` pointed it).
    let angle = if unit.is_air() {
        crate::combat::get_target_angle(distance_x, distance_y)
    } else {
        i32::from(unit.angle)
    };
    let mut distance = scaled_distance4(distance_x, distance_y);

    if distance == 0 {
        return None;
    }

    distance -= i32::from(unit.move_fraction);
    if distance < 2 {
        distance = 2;
    }

    let max_distance;
    let mut steps_distance;
    let mut distance_x = distance_x;
    let mut distance_y = distance_y;

    if state.real_time {
        max_distance = distance;
    } else {
        let mut value = i32::from(unit.speed);
        if unit.group_speed != 0 {
            value = i32::from(unit.group_speed) - 1;
        }
        max_distance = if value != 0 { value * 4 } else { SHRT_MAX };
    }

    if max_distance < distance {
        let target_grid_x = (distance_x * max_distance) / distance + i32::from(unit.grid.x);
        let target_grid_y = (distance_y * max_distance) / distance + i32::from(unit.grid.y);

        distance_x = target_grid_x - i32::from(unit.grid.x);
        distance_y = target_grid_y - i32::from(unit.grid.y);
        distance = scaled_distance4(distance_x, distance_y);

        if distance > max_distance {
            distance = max_distance;
        }

        steps_distance = max_distance / 4;
        unit.move_fraction = (max_distance - distance).clamp(0, 3) as u8;

        if steps_distance == 0 || distance == 0 {
            unit.speed = 0;
            if unit.group_speed > 1 {
                unit.group_speed = 1;
            }

            distance_x = i32::from(end_x) - i32::from(unit.grid.x);
            distance_y = i32::from(end_y) - i32::from(unit.grid.y);
            distance = scaled_distance4(distance_x, distance_y);
            steps_distance = (distance + 3) / 4;
        }
    } else {
        steps_distance = (distance + 3) / 4;
        if max_distance != SHRT_MAX {
            unit.move_fraction = (distance % 4) as u8;
        }
    }

    unit.max_velocity = 4;
    let euclidean_distance = steps_distance * i32::from(unit.max_velocity);

    if euclidean_distance <= 0 {
        return None;
    }

    Some(AirPath {
        length: euclidean_distance as i16,
        angle: angle as i8,
        start_x: unit.x as i16,
        start_y: unit.y as i16,
        end_x,
        end_y,
        step_x: 0,
        step_y: 0,
        delta_x: (distance_x << 22) / euclidean_distance,
        delta_y: (distance_y << 22) / euclidean_distance,
    })
}

/// `UnitsManager_GetMissilePath` (`units_manager.cpp:4759`).
pub fn get_missile_path(unit: &UnitInfo, fire_on: Point) -> Option<AirPath> {
    let distance_x = i32::from(fire_on.x) - i32::from(unit.grid.x);
    let distance_y = i32::from(fire_on.y) - i32::from(unit.grid.y);
    let distance = scaled_distance4(distance_x, distance_y);

    if distance <= 0 {
        return None;
    }

    let length = distance / 4;
    if length <= 0 {
        return None;
    }

    Some(AirPath {
        length: length as i16,
        angle: unit.angle as i8,
        start_x: unit.x as i16,
        start_y: unit.y as i16,
        end_x: fire_on.x,
        end_y: fire_on.y,
        step_x: 0,
        step_y: 0,
        delta_x: (distance_x << 22) / length,
        delta_y: (distance_y << 22) / length,
    })
}

// ---------------------------------------------------------------------------
// PathFill / MAXFloodFill
// ---------------------------------------------------------------------------

/// `PathFill` (`pathfill.cpp`) — flood-fill of a connected area over the cost map.
///
/// `mark` — the `0x20` bit "cell processed by the fill"; `0x1F` — cost,
/// `0x80` — "impassable/do not process". Returns the number of marked cells.
pub fn path_fill(costs: &[u8; MAP_TILES], start: Point) -> ([u8; MAP_TILES], u32) {
    let mut map = *costs;
    let Some(start_index) = tile_index(start) else {
        return (map, 0);
    };

    if map[start_index] & 0x1F == 0 || map[start_index] & 0x20 != 0 {
        return (map, 0);
    }

    let mut stack: VecDeque<usize> = VecDeque::new();
    stack.push_back(start_index);
    let mut count = 0;

    while let Some(index) = stack.pop_front() {
        if map[index] & 0x1F == 0 || map[index] & 0x20 != 0 {
            continue;
        }
        map[index] |= 0x20;
        count += 1;

        let x = (index % MAP_WIDTH as usize) as i16;
        let y = (index / MAP_WIDTH as usize) as i16;

        // Traversal order: 4 orthogonal directions `0,2,4,6`, as in
        // `MAXFloodFill` (N, E, S, W).
        for direction in [0usize, 2, 4, 6] {
            let offset = DIRECTION_OFFSETS[direction];
            let neighbor = Point::new(x + offset.x, y + offset.y);
            if let Some(neighbor_index) = tile_index(neighbor) {
                if map[neighbor_index] & 0x1F != 0 && map[neighbor_index] & 0x20 == 0 {
                    stack.push_back(neighbor_index);
                }
            }
        }
    }

    (map, count)
}

/// `AccessMap::IsProcessed` (`accessmap.cpp:52`).
pub fn is_processed(costs: &[u8; MAP_TILES], grid: Point) -> bool {
    match tile_index(grid) {
        Some(index) => costs[index] != 0 && costs[index] & 0x80 == 0,
        None => false,
    }
}

/// `Paths_CalculateStep` (see [`crate::unit_order::paths_calculate_step`]) —
/// re-exported for path-calling modules.
pub use crate::unit_order::paths_calculate_step;

// ---------------------------------------------------------------------------
// BuilderPath (P5: minimal port)
// ---------------------------------------------------------------------------

/// `BuilderPath::Execute` (`builderpath.cpp`) — the builder's path: movement to
/// the target along one axis (direction `(1,1)` by default). Returns the step to
/// the target if it is passable.
pub(crate) fn builder_path_step(state: &GameState, unit: &UnitInfo, target: Point) -> Option<PathStep> {
    let dx = i32::from(target.x) - i32::from(unit.grid.x);
    let dy = i32::from(target.y) - i32::from(unit.grid.y);

    if dx == 0 && dy == 0 {
        return None;
    }

    let step = PathStep::new(dx.signum() as i8, dy.signum() as i8);
    let next = Point::new(
        unit.grid.x + i16::from(step.x),
        unit.grid.y + i16::from(step.y),
    );

    (access::is_accessible_unit(state, unit, next, access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS) != 0)
        .then_some(step)
}

/// Path cost `GroundPath::GetMovementCost` (`groundpath.cpp:178`).
#[cfg(test)]
pub(crate) fn ground_path_movement_cost(state: &GameState, unit: &UnitInfo) -> i32 {
    let Some(UnitPath::Ground(path)) = unit.path.as_ref() else {
        return 0;
    };

    let mut x = unit.grid.x;
    let mut y = unit.grid.y;
    let mut result = 0;

    for step in path.steps.iter().skip(path.step_index as usize) {
        if step.x != 0 || step.y != 0 {
            x += i16::from(step.x);
            y += i16::from(step.y);
            let mut step_cost = access::is_accessible_unit(
                state,
                unit,
                Point::new(x, y),
                access::ACCESS_MODIFIER_NONE,
            ) as i32;
            if step.x != 0 && step.y != 0 {
                step_cost = (step_cost * 3) / 2;
            }
            result += step_cost;
        }
    }

    result
}

/// `GroundPath::IsEndStep` (`groundpath.cpp:561`).
pub fn ground_path_is_end_step(unit: &UnitInfo) -> bool {
    match unit.path.as_ref() {
        Some(UnitPath::Ground(path)) => path.step_index as usize + 1 == path.steps.len(),
        _ => false,
    }
}

/// `AirPath::GetMovementCost` (`airpath.cpp:200`).
pub fn air_path_movement_cost(unit: &UnitInfo) -> i32 {
    match unit.path.as_ref() {
        Some(UnitPath::Air(path)) if unit.max_velocity != 0 => {
            (i32::from(path.length) / i32::from(unit.max_velocity)) * 4
        }
        _ => 0,
    }
}

/// The `UnitsManager_InitUnitPath` part for a missile: the path to `fire_on`.
pub fn missile_path(unit: &UnitInfo, fire_on: Point) -> Option<UnitPath> {
    get_missile_path(unit, fire_on).map(UnitPath::Air)
}

/// `AirPath::GetPosition` (`airpath.cpp:104`) — the predicted cell of an air
/// unit (for the cost map).
pub fn air_path_position(unit: &UnitInfo, path: &AirPath) -> Point {
    if !unit.is_air() {
        return unit.grid;
    }

    let mut pixel_x = i32::from(path.length) * path.delta_x + path.step_x;
    pixel_x >>= 16;
    pixel_x += i32::from(path.start_x);
    pixel_x >>= 6;

    let mut pixel_y = i32::from(path.length) * path.delta_y + path.step_y;
    pixel_y >>= 16;
    pixel_y += i32::from(path.start_y);
    pixel_y >>= 6;

    Point::new(pixel_x as i16, pixel_y as i16)
}

#[cfg(test)]
#[path = "pathfinding_tests.rs"]
mod tests;
