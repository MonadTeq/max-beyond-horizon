//! Phase 2 integration: `pathfinding.rs` — A* routes, obstacles, water and
//! unreachable goals over the tick loop.
//!
//! Oracle: `max-cpp-port/src/paths.cpp` (`Paths_RequestPath`, cost map),
//! `groundpath.cpp` (`GroundPath::Execute`), `pathrequest.cpp`
//! (`AssignGroundPath`).

mod support;

use max_core::access;
use max_core::enums::{Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{Point, MAP_WIDTH};
use max_core::team::TeamId;
use max_core::universe::GameUniverse;

use support::{game, order_move, spawn, start_turn, ticks};

fn wall(state: &mut GameUniverse, x: i16, from_y: i16, to_y: i16) {
    for y in from_y..=to_y {
        state.map_mut().surface.set(x, y, Surface::WATER);
    }
}

/// Given a water wall, the tank routes around it and arrives at the goal; every
/// traversed cell is land. Two runs are bit-identical (D).
#[test]
fn tank_routes_around_a_water_wall() {
    fn run() -> (GameUniverse, usize) {
        let mut state = game();
        wall(&mut state, 10, 0, 15);
        let unit = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
        let goal = Point::new(15, 5);
        order_move(&mut state, unit, goal);

        let mut previous = Point::new(5, 5);
        let mut steps = 0usize;
        let mut arrived = false;

        // The detour is longer than one turn's speed allowance; a player re-issues
        // the move each turn (the order stops in `EXECUTING_ORDER` when out of speed).
        'turn: for _ in 0..8 {
            start_turn(&mut state, TeamId::RED);
            order_move(&mut state, unit, goal);
            // One cell per movement-budget window (`MOVEMENT_CELLS_PER_SECOND`).
            for _ in 0..240 {
                state.tick();
                let tank = state.snapshot().unit(unit).expect("unit");
                if tank.grid != previous {
                    assert_eq!(
                        state.snapshot().map().surface_at(tank.grid.x, tank.grid.y),
                        Surface::LAND,
                        "the tank entered water at {:?}",
                        tank.grid
                    );
                    previous = tank.grid;
                    steps += 1;
                }
                if previous == goal && !tank.has_move_order() {
                    arrived = true;
                    break 'turn;
                }
            }
        }

        let unit = state.snapshot().unit(unit).expect("unit");
        assert!(arrived, "the tank did not reach the goal: {:?}", unit.grid);
        assert_eq!(unit.orders, UnitOrder::Await);
        assert!(unit.path.is_none(), "the route is dropped on arrival");
        // The shortest straight route is 10 cells; the wall detour is longer.
        assert!(steps > 10, "the tank did not detour: {steps} steps");
        (state, steps)
    }

    let (first, first_steps) = run();
    let (second, second_steps) = run();
    assert_eq!(first_steps, second_steps);
    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes()
    );
    assert_eq!(
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
    assert_eq!(
        first.snapshot().replay_digest_bytes(),
        second.snapshot().replay_digest_bytes()
    );
}

/// A goal enclosed by water is unreachable: the request completes as blocked,
/// the tank keeps its cell and `AWAIT`, and no request stays queued.
#[test]
fn unreachable_goal_leaves_the_tank_waiting() {
    fn run() -> GameUniverse {
        let mut state = game();
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                state
                    .map_mut()
                    .surface
                    .set(15 + dx, 15 + dy, Surface::WATER);
            }
        }

        let unit = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
        order_move(&mut state, unit, Point::new(15, 15));
        ticks(&mut state, 8);

        let unit_ref = state.snapshot().unit(unit).expect("unit");
        assert_eq!(unit_ref.grid, Point::new(5, 5), "no movement possible");
        assert_eq!(unit_ref.orders, UnitOrder::Await);
        assert_eq!(unit_ref.state, UnitOrderState::ExecutingOrder);
        assert!(unit_ref.path.is_none());
        assert!(
            state.snapshot().path_requests().is_empty(),
            "the queue is drained"
        );
        assert!(state.snapshot().path_requests().is_empty());
        state
    }

    let first = run();
    let second = run();
    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes()
    );
    assert_eq!(
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
}

/// A naval unit routes along the only water lane; a land unit cannot use it.
#[test]
fn sea_unit_routes_along_the_water_lane() {
    let mut state = game();
    // Only the row y = 5 is water; everything else is land.
    for x in 0..MAP_WIDTH {
        state.map_mut().surface.set(x, 5, Surface::WATER);
    }

    let ship = spawn(
        &mut state,
        UnitType::Corvette,
        TeamId::RED,
        Point::new(5, 5),
    );
    let goal = Point::new(15, 5);
    order_move(&mut state, ship, goal);

    let mut last = Point::new(5, 5);
    let mut moves = 0;
    'turn: for _ in 0..4 {
        start_turn(&mut state, TeamId::RED);
        order_move(&mut state, ship, goal);
        // One cell per movement-budget window (`MOVEMENT_CELLS_PER_SECOND`).
        for _ in 0..200 {
            state.tick();
            let unit = state.snapshot().unit(ship).expect("ship");
            if unit.grid != last {
                assert_eq!(
                    unit.grid.y, 5,
                    "the ship left the water lane: {:?}",
                    unit.grid
                );
                assert_eq!(
                    state.snapshot().map().surface_at(unit.grid.x, unit.grid.y),
                    Surface::WATER,
                    "ship on non-water {:?}",
                    unit.grid
                );
                last = unit.grid;
                moves += 1;
            }
            if last == goal && !unit.has_move_order() {
                break 'turn;
            }
        }
    }

    assert_eq!(last, Point::new(15, 5), "ship did not reach the goal");
    assert_eq!(
        moves, 10,
        "one cell per movement-budget window along the lane"
    );
    assert_eq!(
        state.snapshot().unit(ship).unwrap().orders,
        UnitOrder::Await
    );

    // A tank cannot enter the water lane, so the same destination is unreachable
    // from a land cell (the only route would cross water or leave the map edge).
    let mut land_state = game();
    for x in 0..MAP_WIDTH {
        land_state.map_mut().surface.set(x, 5, Surface::WATER);
    }
    let tank = spawn(
        &mut land_state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(5, 6),
    );
    order_move(&mut land_state, tank, Point::new(15, 5));
    ticks(&mut land_state, 8);
    assert_eq!(
        land_state.snapshot().unit(tank).unwrap().grid,
        Point::new(5, 6)
    );
    assert_eq!(
        land_state.snapshot().unit(tank).unwrap().orders,
        UnitOrder::Await
    );
}

/// `MOVE_TO_ATTACK` path assignment stops in attack range: the tank never steps
/// onto the enemy cell and ends within `range²` (`AssignGroundPath`).
#[test]
fn move_to_attack_stops_within_range() {
    let mut state = game();
    let red = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
    let green = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(20, 5));
    {
        let unit = state.unit_mut(green).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
        // No return fire: the scenario checks the approach geometry only.
        unit.ammo = 0;
        unit.shots = 0;
    }
    {
        let unit = state.unit_mut(red).unwrap();
        unit.enemy_unit = Some(green);
    }
    state.set_order_data(
        red,
        max_core::game::OrderData {
            move_to: Point::new(20, 5),
            fire_on: Point::new(20, 5),
        },
    );
    state.set_order(red, UnitOrder::MoveToAttack, UnitOrderState::Init);

    let range = state
        .snapshot()
        .unit(red)
        .unwrap()
        .base_values
        .get_attribute(max_core::ATTRIB_RANGE);
    assert_eq!(range, 4);

    let mut closest = i32::MAX;
    let mut stopped = false;
    'turn: for _ in 0..8 {
        start_turn(&mut state, TeamId::RED);
        // A player re-issues the attack move when the previous turn ran out of speed.
        state.set_order_data(
            red,
            max_core::game::OrderData {
                move_to: Point::new(20, 5),
                fire_on: Point::new(20, 5),
            },
        );
        state.set_order(red, UnitOrder::MoveToAttack, UnitOrderState::Init);
        // One cell per movement-budget window (`MOVEMENT_CELLS_PER_SECOND`).
        for _ in 0..200 {
            state.tick();
            let red_unit = state.snapshot().unit(red).expect("red unit");
            closest = closest.min(access::squared_distance(red_unit.grid, Point::new(20, 5)));
            assert_ne!(
                red_unit.grid,
                Point::new(20, 5),
                "the attacker must not enter the target cell"
            );
            if closest <= range * range && !red_unit.has_move_order() {
                stopped = true;
                break 'turn;
            }
        }
    }

    assert!(
        stopped,
        "the attacker stopped outside its range: closest {closest} > {}",
        range * range
    );
}

/// Two tanks requesting routes in the same tick are applied in submission order
/// and the resulting state is deterministic (D).
#[test]
fn queued_path_requests_are_deterministic() {
    fn run() -> GameUniverse {
        let mut state = game();
        let first = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
        let second = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 9));
        order_move(&mut state, first, Point::new(9, 5));
        order_move(&mut state, second, Point::new(9, 9));

        ticks(&mut state, 1);
        assert_eq!(state.snapshot().path_requests().len(), 2);
        assert_eq!(state.snapshot().path_requests()[0].unit, first);
        assert_eq!(state.snapshot().path_requests()[1].unit, second);

        ticks(&mut state, 9);
        assert!(state.snapshot().path_requests().is_empty());
        for unit in [first, second] {
            assert!(
                state.snapshot().unit(unit).unwrap().path.is_some()
                    || state.snapshot().unit(unit).unwrap().grid.x > 5
            );
        }
        state
    }

    let first = run();
    let second = run();
    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes()
    );
    assert_eq!(
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
}
