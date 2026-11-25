use super::*;
use crate::enums::unit_flags::{MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, SELECTABLE};
use crate::enums::{Surface, TeamClan};
use crate::team::{TeamId, TeamKind};

fn state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    state
}

fn tank_at(state: &mut GameState, grid: Point) -> UnitId {
    state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            grid,
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap()
}

fn cumulative(steps: &[PathStep], start: Point) -> Vec<Point> {
    let mut points = vec![start];
    let mut point = start;
    for step in steps {
        point.x += i16::from(step.x);
        point.y += i16::from(step.y);
        points.push(point);
    }
    points
}

#[test]
fn integer_distance_helpers_match_cpp_math() {
    assert_eq!(isqrt(0), 0);
    assert_eq!(isqrt(1), 1);
    assert_eq!(isqrt(24), 4);
    assert_eq!(isqrt(25), 5);
    // sqrt(25)*4 = 20, sqrt(2)*4 ≈ 5.66 → 6.
    assert_eq!(scaled_distance4(3, 4), 20);
    assert_eq!(scaled_distance4(1, 1), 6);
    assert_eq!(scaled_distance4(0, 0), 0);
}

#[test]
fn shortest_path_is_a_straight_line() {
    let mut state = state();
    let tank = tank_at(&mut state, Point::new(5, 5));
    let unit = state.unit(tank).unwrap().clone();

    let path = find_ground_path(&state, &unit, Point::new(5, 8), 0, 0).expect("path");
    assert_eq!((path.end_x, path.end_y), (5, 8));
    assert_eq!(path.steps.len(), 3);
    for step in &path.steps {
        assert_eq!((step.x, step.y), (0, 1));
    }

    // Start == finish — no path (C++ `CompleteRequest(nullptr)`).
    assert!(find_ground_path(&state, &unit, unit.grid, 0, 0).is_none());
}

#[test]
fn path_goes_around_water_wall() {
    let mut state = state();
    // Water as a wall x = 7, y = 0..=10, the only passage is y = 11.
    for y in 0..=10 {
        state.map.surface.set(7, y, Surface::WATER);
    }

    let tank = tank_at(&mut state, Point::new(5, 5));
    let unit = state.unit(tank).unwrap().clone();
    let path = find_ground_path(&state, &unit, Point::new(9, 5), 0, 0).expect("path");

    // The path cannot be shorter than the Manhattan distance and must
    // go around the wall (down to y = 11 and back).
    assert!(path.steps.len() >= 12);
    for point in cumulative(&path.steps, unit.grid) {
        assert_eq!(
            state.map.surface_at(point.x, point.y),
            Surface::LAND,
                "путь зашёл в воду: {point:?}"
        );
    }
    assert_eq!(
        cumulative(&path.steps, unit.grid).last().copied(),
        Some(Point::new(9, 5))
    );
}

#[test]
fn search_is_deterministic() {
    let mut state = state();
    for y in 2..=12 {
        state.map.surface.set(8, y, Surface::WATER);
    }
    let tank = tank_at(&mut state, Point::new(4, 6));
    let unit = state.unit(tank).unwrap().clone();

    let first = find_ground_path(&state, &unit, Point::new(12, 6), 0, 0).expect("path");
    let second = find_ground_path(&state, &unit, Point::new(12, 6), 0, 0).expect("path");
    assert_eq!(first, second);
}

#[test]
fn request_queue_is_applied_in_placement_order() {
    let mut state = state();
    let first = tank_at(&mut state, Point::new(5, 5));
    let second = tank_at(&mut state, Point::new(5, 9));
    for (unit, target) in [(first, Point::new(9, 5)), (second, Point::new(9, 9))] {
        state.set_order_data(
            unit,
            crate::game::OrderData {
                move_to: target,
                fire_on: Point::ZERO,
            },
        );
        state.set_order(unit, UnitOrder::Move, UnitOrderState::Init);
    }

    state.tick();
    // Both requests are submitted in the same tick. Mobile ground units
    // are inserted into the list with `PushFront` (`AddToDrawList`), so the second
    // unit is walked first; `Paths_RequestPath` does
    // `PushFront` for players — the final queue order: first, second.
    assert_eq!(state.path_requests.len(), 2);
    assert_eq!(state.path_requests[0].unit, first);
    assert_eq!(state.path_requests[1].unit, second);

    state.tick();
    assert!(state.path_requests.is_empty());
    for unit in [first, second] {
        let unit = state.unit(unit).unwrap();
        assert!(unit.path.is_some());
        assert_eq!(unit.state, UnitOrderState::InProgress);
    }
}

#[test]
fn board_transport_path_reaches_receiver_cell() {
    let mut state = state();
    let commando = state
        .spawn_unit(
            UnitType::Infiltrator,
            TeamId::RED,
            Point::new(5, 6),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    let transport = state
        .spawn_unit(
            UnitType::SeaTransport,
            TeamId::RED,
            Point::new(9, 6),
            crate::enums::unit_flags::MOBILE_SEA_UNIT
                | crate::enums::unit_flags::MOBILE_LAND_UNIT,
        )
        .unwrap();
    state.unit_mut(transport).unwrap().orders = UnitOrder::Sentry;
    state.unit_mut(transport).unwrap().state = UnitOrderState::ExecutingOrder;
    state.unit_mut(transport).unwrap().storage = 0;

    state.set_order_data(
        commando,
        crate::game::OrderData {
            move_to: Point::new(9, 6),
            fire_on: Point::ZERO,
        },
    );
    state.set_order(commando, UnitOrder::MoveToUnit, UnitOrderState::Init);
    state.tick();
    state.tick();

    let unit = state.unit(commando).unwrap();
    let Some(UnitPath::Ground(path)) = unit.path.as_ref() else {
        panic!("путь к транспорту не найден: {:?}", unit.state);
    };
    assert_eq!((path.end_x, path.end_y), (9, 6));
}

#[test]
fn move_to_attack_requests_path_to_enemy() {
    let mut state = state();
    let red = tank_at(&mut state, Point::new(5, 5));
    let green = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(12, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    state.unit_mut(green).unwrap().orders = UnitOrder::Sentry;
    state.unit_mut(green).unwrap().state = UnitOrderState::ExecutingOrder;

    {
        let unit = state.unit_mut(red).unwrap();
        unit.enemy_unit = Some(green);
        unit.orders = UnitOrder::MoveToAttack;
        unit.state = UnitOrderState::Init;
    }
    // The path of a unit moving to attack is truncated by the radius (`AssignGroundPath`).
    for _ in 0..12 {
        state.tick();
    }
    let unit = state.unit(red).unwrap();
    assert!(unit.grid.x > 5, "юнит должен приблизиться к цели");
    assert!(unit.grid.x <= 12);
    assert!(state.path_requests.len() <= 1, "очередь путей не растёт");
}

#[test]
fn air_path_is_a_linear_segment() {
    let mut state = state();
    let fighter = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_AIR_UNIT | SELECTABLE,
        )
        .unwrap();
    let mut unit = state.unit(fighter).unwrap().clone();
    let path = paths_get_air_path(&state, &mut unit, Point::new(9, 8)).expect("air path");
    assert_eq!((path.end_x, path.end_y), (9, 8));
    assert_eq!(path.start_x, unit.x as i16);
    assert_eq!(path.start_y, unit.y as i16);
    // The delta direction is the sign of the distance.
    assert!(path.delta_x > 0);
    assert!(path.delta_y > 0);
    assert!(path.length > 0);

    let missile = get_missile_path(&unit, Point::new(8, 5)).expect("missile path");
    assert_eq!((missile.end_x, missile.end_y), (8, 5));
}

#[test]
fn path_fill_marks_only_connected_cells() {
    let mut costs = [4u8; MAP_TILES];
    // A wall at x = 6 (units cannot pass).
    for y in 0..MAP_HEIGHT {
        let index = y as usize * MAP_WIDTH as usize + 6;
        costs[index] = 0;
    }

    let (filled, count) = path_fill(&costs, Point::new(2, 2));
    assert!(count > 0);
    assert!(count < MAP_TILES as u32);
    let start_index = 2 * MAP_WIDTH as usize + 2;
    assert_ne!(filled[start_index] & 0x20, 0);
    // The cell behind the wall is not filled.
    let beyond = 2 * MAP_WIDTH as usize + 8;
    assert_eq!(filled[beyond] & 0x20, 0);
    assert!(is_processed(&costs, Point::new(2, 2)));

    // A repeated run gives the same result (fill determinism).
    let (again, again_count) = path_fill(&costs, Point::new(2, 2));
    assert_eq!(count, again_count);
    assert_eq!(filled, again);
}

#[test]
fn ground_and_air_path_costs_are_ported() {
    let mut state = state();
    let tank = tank_at(&mut state, Point::new(5, 5));
    let mut unit = state.unit(tank).unwrap().clone();
    unit.path = Some(UnitPath::Ground(GroundPath {
        end_x: 7,
        end_y: 5,
        step_index: 0,
        steps: vec![PathStep::new(1, 0), PathStep::new(1, 0)],
    }));
    assert_eq!(ground_path_movement_cost(&state, &unit), 8);
    assert!(ground_path_is_end_step(&unit) == false);

    let mut air = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(6, 6),
            MOBILE_AIR_UNIT,
        )
        .map(|id| state.unit(id).unwrap().clone())
        .unwrap();
    air.max_velocity = 4;
    air.path = Some(UnitPath::Air(AirPath {
        length: 12,
        ..AirPath::default()
    }));
    assert_eq!(air_path_movement_cost(&air), 12);
}

// -----------------------------------------------------------------------
// Search boundaries: unreachable cells, map edges, cost map classes
// -----------------------------------------------------------------------

/// A target enclosed by water is unreachable for a land unit; the search
/// returns `None` (C++ `CompleteRequest(nullptr)` → `BlockedOnPathRequest`).
#[test]
fn unreachable_target_returns_no_path() {
    let mut state = state();
    // Ring around (10,10): the cell itself stays land.
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            state.map.surface.set(10 + dx, 10 + dy, Surface::WATER);
        }
    }
    let tank = tank_at(&mut state, Point::new(5, 5));
    let unit = state.unit(tank).unwrap().clone();

    assert!(find_ground_path(&state, &unit, Point::new(10, 10), 0, 0).is_none());

    // A request to the same cell is completed as blocked: the unit waits.
    state.set_order_data(
        tank,
        crate::game::OrderData {
            move_to: Point::new(10, 10),
            fire_on: Point::ZERO,
        },
    );
    state.set_order(tank, UnitOrder::Move, UnitOrderState::NewOrder);
    assert!(request_path(&mut state, tank, access::ACCESS_MODIFIER_NONE));
    poll_results(&mut state);
    let unit = state.unit(tank).unwrap();
    assert!(unit.path.is_none());
    assert_eq!(unit.orders, UnitOrder::Await);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert_eq!(unit.delayed_reaction, 1);
}

/// Destinations outside `0..MAP_WIDTH/HEIGHT` are rejected without a queue
/// entry and block the order (`Paths_RequestPath`, `paths.cpp:46`).
#[test]
fn out_of_bounds_destination_is_rejected() {
    let mut state = state();
    let tank = tank_at(&mut state, Point::new(5, 5));
    let unit = state.unit(tank).unwrap().clone();

    for destination in [
        Point::new(-1, 5),
        Point::new(5, -1),
        Point::new(MAP_WIDTH, 5),
        Point::new(5, MAP_HEIGHT),
    ] {
        assert!(
            find_ground_path(&state, &unit, destination, 0, 0).is_none(),
                "{destination:?}"
        );
    }

    for destination in [Point::new(-1, 5), Point::new(5, MAP_HEIGHT)] {
        state.set_order_data(
            tank,
            crate::game::OrderData {
                move_to: destination,
                fire_on: Point::ZERO,
            },
        );
        state.set_order(tank, UnitOrder::Move, UnitOrderState::Init);
        assert!(
            !request_path(&mut state, tank, access::ACCESS_MODIFIER_NONE),
                "{destination:?}"
        );
        assert!(state.path_requests.is_empty());
        let unit = state.unit(tank).unwrap();
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert_eq!(unit.delayed_reaction, 1);
    }
}

/// `AccessMap::Init`: per-surface entry costs by `land_type`; air is free
/// terrain; the surveyor and amphibians use the special water cost.
#[test]
fn access_cost_map_surface_classes() {
    let mut state = state();
    state.map.surface.set(10, 10, Surface::WATER);
    state.map.surface.set(11, 10, Surface::COAST);

    let index = |x: i16, y: i16| y as usize * MAP_WIDTH as usize + x as usize;

    let tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    let costs = build_access_costs(&state, &tank, access::ACCESS_MODIFIER_NONE);
    assert_eq!(costs[index(5, 5)], 4, "land");
    assert_eq!(costs[index(10, 10)], 0, "tank cannot enter water");
    assert_eq!(costs[index(11, 10)], 0, "tank cannot enter coast");

    let ship = UnitInfo::new(UnitType::Corvette, TeamId::RED, UnitId(2));
    let costs = build_access_costs(&state, &ship, access::ACCESS_MODIFIER_NONE);
    assert_eq!(costs[index(5, 5)], 0, "ship cannot enter land");
    assert_eq!(costs[index(10, 10)], 4, "water");
    assert_eq!(costs[index(11, 10)], 0, "CORVETTE has no coast land type");

    let mut air_unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(3));
    air_unit.flags = access::unit_type_flags(UnitType::Fighter);
    let costs = build_access_costs(&state, &air_unit, access::ACCESS_MODIFIER_NONE);
    assert!(costs.iter().all(|cost| *cost == 4), "air ignores terrain");

    // Amphibians get the expensive water cost (`AccessMap::Init`, `water_value = 8`)
    // and the normal coastal cost.
    let amphibious = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(4));
    let costs = build_access_costs(&state, &amphibious, access::ACCESS_MODIFIER_NONE);
    assert_eq!(costs[index(10, 10)], 8);
    assert_eq!(costs[index(11, 10)], 4, "coast");

    // The surveyor is the documented exception: water costs 4, not 8.
    let surveyor = UnitInfo::new(UnitType::Surveyor, TeamId::RED, UnitId(5));
    let costs = build_access_costs(&state, &surveyor, access::ACCESS_MODIFIER_NONE);
    assert_eq!(costs[index(10, 10)], 4);
}

/// `PathFill` boundaries: blocked/out-of-map/already-marked starts do
/// nothing, connectivity is orthogonal (4-neighbour), and a uniform open
/// map is filled completely.
#[test]
fn path_fill_boundaries_and_connectivity() {
    let open = [4u8; MAP_TILES];

    let (filled, count) = path_fill(&open, Point::new(0, 0));
    assert_eq!(count, MAP_TILES as u32);
    assert!(filled.iter().all(|cell| cell & 0x20 != 0));

    // Blocked start.
    let mut blocked = open;
    blocked[0] = 0;
    let (filled, count) = path_fill(&blocked, Point::new(0, 0));
    assert_eq!(count, 0);
    assert_eq!(filled, blocked);

    // Already processed start.
    let mut marked = open;
    marked[0] |= 0x20;
    let (filled, count) = path_fill(&marked, Point::new(0, 0));
    assert_eq!(count, 0);
    assert_eq!(filled, marked);

    // Outside the map.
    for start in [
        Point::new(-1, 0),
        Point::new(0, -1),
        Point::new(MAP_WIDTH, 0),
    ] {
        let (_, count) = path_fill(&open, start);
        assert_eq!(count, 0, "{start:?}");
    }

    // Diagonal-only neighbour is not connected.
    let mut diagonal = [0u8; MAP_TILES];
    diagonal[0] = 4;
    let diagonal_index = MAP_WIDTH as usize + 1;
    diagonal[diagonal_index] = 4;
    let (filled, count) = path_fill(&diagonal, Point::new(0, 0));
    assert_eq!(count, 1);
    assert_eq!(filled[diagonal_index] & 0x20, 0);
}

/// `AccessMap::IsProcessed`: bounds, zero cost and the `0x80` "processed"
/// bit all report `false`; a passable unmarked cell reports `true`.
#[test]
fn is_processed_boundaries() {
    let mut costs = [4u8; MAP_TILES];
    assert!(is_processed(&costs, Point::new(5, 5)));
    assert!(!is_processed(&costs, Point::new(-1, 5)));
    assert!(!is_processed(&costs, Point::new(5, -1)));
    assert!(!is_processed(&costs, Point::new(MAP_WIDTH, 5)));
    assert!(!is_processed(&costs, Point::new(5, MAP_HEIGHT)));

    let index = 5 * MAP_WIDTH as usize + 5;
    costs[index] = 0;
    assert!(!is_processed(&costs, Point::new(5, 5)));

    costs[index] = 4 | 0x80;
    assert!(!is_processed(&costs, Point::new(5, 5)));
}

/// `Paths_RequestPath`: identical requests are de-duplicated, a `NEW_ORDER`
/// drops the unit's queued requests, and `MOVE_TO_ATTACK` targets the enemy
/// cell rather than the stale `move_to`.
#[test]
fn request_path_dedup_and_move_to_attack_target() {
    let mut state = state();
    let tank = tank_at(&mut state, Point::new(5, 5));
    state.set_order_data(
        tank,
        crate::game::OrderData {
            move_to: Point::new(9, 5),
            fire_on: Point::ZERO,
        },
    );
    state.set_order(tank, UnitOrder::Move, UnitOrderState::Init);

    assert!(request_path(&mut state, tank, access::ACCESS_MODIFIER_NONE));
    assert!(request_path(&mut state, tank, access::ACCESS_MODIFIER_NONE));
    assert_eq!(state.path_requests.len(), 1, "duplicate suppressed");

    // A new order replaces the queued request even for another destination.
    state.unit_mut(tank).unwrap().state = UnitOrderState::NewOrder;
    state.set_order_data(
        tank,
        crate::game::OrderData {
            move_to: Point::new(12, 9),
            fire_on: Point::ZERO,
        },
    );
    assert!(request_path(&mut state, tank, access::ACCESS_MODIFIER_NONE));
    assert_eq!(state.path_requests.len(), 1);
    assert_eq!(state.path_requests[0].destination, Point::new(12, 9));

    // MOVE_TO_ATTACK: the destination is the enemy's cell.
    state.path_requests.clear();
    let enemy = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(20, 7),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    {
        let unit = state.unit_mut(tank).unwrap();
        unit.orders = UnitOrder::MoveToAttack;
        unit.state = UnitOrderState::Init;
        unit.enemy_unit = Some(enemy);
    }
    state.set_order_data(
        tank,
        crate::game::OrderData {
            move_to: Point::new(0, 0),
            fire_on: Point::ZERO,
        },
    );
    assert!(request_path(&mut state, tank, access::ACCESS_MODIFIER_NONE));
    let request = state
        .path_requests
        .iter()
        .find(|request| request.unit == tank)
        .expect("request");
    assert_eq!(request.destination, Point::new(20, 7));
    assert_eq!(request.minimum_distance, 16, "range 4 squared");
    assert_eq!(state.order_data(tank).move_to, Point::new(20, 7));
}

/// A `minimum_distance` (attack range) truncates the route before the target
/// cell; distance 0 keeps the exact destination.
#[test]
fn minimum_distance_truncates_the_route() {
    let mut state = state();
    let tank = tank_at(&mut state, Point::new(5, 5));
    let unit = state.unit(tank).unwrap().clone();
    let destination = Point::new(15, 5);

    let path = find_ground_path(&state, &unit, destination, 0, 0).expect("path");
    assert_eq!(path.steps.len(), 10);
    assert_eq!(path.end_x, destination.x);

    let truncated = find_ground_path(&state, &unit, destination, 0, 16).expect("path");
    let mut point = unit.grid;
    for step in &truncated.steps {
        point.x += i16::from(step.x);
        point.y += i16::from(step.y);
    }
    assert!(truncated.steps.len() < 10, "route stops earlier");
    assert!(access::squared_distance(point, destination) <= 16);
    assert!(access::squared_distance(point, destination) > 0);
}

/// Integer helpers: `isqrt`/`scaled_distance4` are exact at the boundaries,
/// `append_linear_steps` handles the empty and the longest routes.
#[test]
fn integer_math_and_linear_steps_boundaries() {
    assert_eq!(isqrt(0), 0);
    assert_eq!(isqrt(1), 1);
    assert_eq!(isqrt(2), 1);
    assert_eq!(isqrt(3), 1);
    assert_eq!(isqrt(4), 2);
    assert_eq!(isqrt(u32::MAX - 1), 65_535);

    assert_eq!(scaled_distance4(0, 0), 0);
    assert_eq!(scaled_distance4(1, 0), 4);
    assert_eq!(scaled_distance4(0, -1), 4);
    assert_eq!(scaled_distance4(3, 4), 20);
    assert_eq!(scaled_distance4(-3, -4), 20);
    assert_eq!(scaled_distance4(-3, 4), 20);
    assert!(scaled_distance4(30_000, -30_000) > 0);

    assert!(append_linear_steps(Point::new(4, 4), Point::new(4, 4)).is_empty());
    assert_eq!(
        append_linear_steps(Point::new(2, 3), Point::new(2, 5)),
        vec![PathStep::new(0, 1), PathStep::new(0, 1)]
    );
    assert_eq!(
        append_linear_steps(Point::new(0, 0), Point::new(2, 2)),
        vec![PathStep::new(1, 1), PathStep::new(1, 1)]
    );
    assert_eq!(
        append_linear_steps(Point::new(0, 0), Point::new(-3, 2)).len(),
        3
    );
}

/// Public `isqrt` boundary: `u32::MAX` is `65535² + …`. The initial guess
/// must not overflow at this input (`value + 1` would wrap to zero).
#[test]
fn isqrt_max_value_is_exact() {
    assert_eq!(isqrt(u32::MAX), 65_535);
}

/// `Paths_GetAirPath`: no movement for an equal cell; speed 0 is an unlimited
/// one-turn segment; the target stays the end point.
#[test]
fn air_path_zero_and_unlimited_boundaries() {
    let mut state = state();
    let fighter = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_AIR_UNIT | SELECTABLE,
        )
        .unwrap();

    let mut unit = state.unit(fighter).unwrap().clone();
    let grid = unit.grid;
    assert!(paths_get_air_path(&state, &mut unit, grid).is_none());

    let path = paths_get_air_path(&state, &mut unit, Point::new(6, 5)).expect("air path");
    assert_eq!((path.end_x, path.end_y), (6, 5));
    assert_eq!(path.length, 4, "one cell at max_velocity 4");
    assert!(path.delta_x > 0);
    assert_eq!(path.delta_y, 0);
    assert!(unit.move_fraction < 4);

    // Zero speed is `SHRT_MAX` distance — the whole segment in one turn.
    unit.speed = 0;
    let unlimited = paths_get_air_path(&state, &mut unit, Point::new(15, 5)).expect("air path");
    assert_eq!((unlimited.end_x, unlimited.end_y), (15, 5));
    assert!(unlimited.length > 0);

    // Group speed limits the segment to `(group_speed - 1) * 4`.
    unit.group_speed = 3;
    unit.speed = 10;
    unit.path = None;
    let limited = paths_get_air_path(&state, &mut unit, Point::new(5, 15)).expect("air path");
    assert_eq!((limited.end_x, limited.end_y), (5, 15));
    assert!(limited.length > 0);
    assert!(unit.move_fraction <= 3);
}

/// `GroundPath::GetPosition`: without a path, without speed, for a build
/// order or outside `EXECUTING_ORDER` the predicted cell is the current one;
/// a diagonal step that exceeds the speed stops the prediction.
#[test]
fn ground_path_position_boundaries() {
    let mut state = state();
    let tank = tank_at(&mut state, Point::new(5, 5));
    let mut unit = state.unit(tank).unwrap().clone();
    let path = || {
        Some(UnitPath::Ground(GroundPath {
            end_x: 8,
            end_y: 5,
            step_index: 0,
            steps: vec![
                PathStep::new(1, 0),
                PathStep::new(1, -1),
                PathStep::new(1, 0),
            ],
        }))
    };

    assert_eq!(ground_path_position(&state, &unit), Point::new(5, 5));

    unit.path = path();
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.speed = 0;
    assert_eq!(ground_path_position(&state, &unit), Point::new(5, 5));

    // Speed 1 (=4 points): the straight step fits, the diagonal (6) does not.
    unit.speed = 1;
    assert_eq!(ground_path_position(&state, &unit), Point::new(6, 5));

    // Build orders never predict movement.
    unit.orders = UnitOrder::Build;
    unit.speed = 10;
    assert_eq!(ground_path_position(&state, &unit), Point::new(5, 5));

    // Non-executing states never predict movement.
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::InProgress;
    assert_eq!(ground_path_position(&state, &unit), Point::new(5, 5));

    // Position in the middle of a step uses `move_fraction`.
    unit.state = UnitOrderState::ExecutingOrder;
    unit.speed = 1;
    unit.move_fraction = 4;
    assert_eq!(ground_path_position(&state, &unit), Point::new(6, 5));
}
