//! End-of-turn auto-movement along an assigned route.
//!
//! Oracle: `max-cpp-port/src/game_manager.cpp` — `GameManager_TeamTurnTurnBased`
//! and `GameManager_TeamTurnConcurrent` close a team turn with
//!
//! ```c
//! while (Access_AreTaskEventsPending()) {
//!     GameManager_ProgressTurn();
//! }
//! ```
//!
//! (`:488-490`, `:549-558`, "Waiting for units to finish moving"). A unit whose
//! `ORDER_MOVE`/`MOVE_TO_UNIT`/`MOVE_TO_ATTACK` order is not in
//! `ORDER_STATE_EXECUTING_ORDER` still has a pending route step
//! (`Access_AreTaskEventsPending`, `access.cpp:2270`); each pass is one
//! `ProcessTick` → `UnitsManager_ProcessOrders` → `UnitInfo::ExecuteMove` →
//! `GroundPath::Execute` (`groundpath.cpp:210`), which spends the cell cost from
//! `speed` (`Paths_CalculateStep`, `paths.cpp:292`).
//!
//! The port runs that drain at the turn close
//! (`GameUniverse::end_local_turn` → `GameState::finish_end_turn_movement`):
//! a unit with a path and movement points left walks its route before the next
//! team starts.

mod support;

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{PathStep, Point};
use max_core::team::TeamId;
use max_core::unit::{GroundPath, UnitPath};
use max_core::universe::GameUniverse;
use max_core::UnitId;

/// Gives the unit a ready route east `steps` cells long and `speed` movement
/// points, as the path manager leaves it after `PathRequest::Finish`
/// (`pathrequest.cpp:145`: `state = ORDER_STATE_IN_PROGRESS`).
fn give_route(universe: &mut GameUniverse, id: UnitId, steps: usize, speed: u16) {
    let end = {
        let unit = universe.unit_mut(id).expect("unit");
        let mut end = unit.grid;

        for _ in 0..steps {
            end.x += 1;
        }

        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::InProgress;
        unit.speed = speed;
        unit.path = Some(UnitPath::Ground(GroundPath {
            end_x: end.x,
            end_y: end.y,
            step_index: 0,
            steps: vec![PathStep::new(1, 0); steps],
        }));

        end
    };

    universe.set_order_data(
        id,
        max_core::game::OrderData {
            move_to: end,
            fire_on: Point::ZERO,
        },
    );
}

/// A unit of the closing team with a path and `speed > 0` walks its route at the
/// turn end, spending one movement point per cell, and stops with the rest of the
/// route kept for the next turn (`GroundPath::Execute`, `speed == 0` branch).
#[test]
fn unit_with_path_and_speed_advances_at_the_turn_end() {
    let mut universe = support::game();
    // Enter the round, so the turn cursor is past RED and the close starts GREEN:
    // RED's economy has already run, its speed is not reset again for this turn.
    universe.begin_local_round();
    assert_eq!(universe.snapshot().active_turn_team(), TeamId::RED);

    let id = support::spawn(&mut universe, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    give_route(&mut universe, id, 4, 2);

    let start = universe.snapshot().unit(id).cloned().expect("unit").grid;
    assert_eq!(start, Point::new(10, 10));

    let over = universe.end_local_turn(TeamId::RED);
    assert!(!over, "the match is not over");

    let unit = universe.snapshot().unit(id).cloned();
    // The facade borrows the state; take the snapshot into a local binding first.
    let unit = unit.expect("unit survives the turn close");
    assert_eq!(
        unit.grid,
        Point::new(12, 10),
        "two cells walked at the turn close: the route step costs one movement point"
    );
    assert_eq!(unit.speed, 0, "the remaining speed is spent");
    assert_eq!(unit.orders, UnitOrder::Move);
    assert_eq!(
        unit.state,
        UnitOrderState::ExecutingOrder,
        "out of speed: `GroundPath::Execute` stops the order state"
    );
    assert!(
        unit.path.is_some(),
        "the rest of the route is kept until the speed is restored"
    );
    assert_eq!(
        unit.path.as_ref().map(UnitPath::remaining_steps),
        Some(2),
        "two of the four route cells are left"
    );

    assert_eq!(
        universe.snapshot().active_turn_team(),
        TeamId::GREEN,
        "the next team starts only after the closing team finished walking"
    );
}

/// Without movement points the route is not walked: the drain is bounded by
/// `speed`, as in `GroundPath::Execute` (`speed == 0` → `MoveFinished`, the path
/// stays for the next turn).
#[test]
fn a_route_without_speed_does_not_advance_at_the_turn_end() {
    let mut universe = support::game();
    universe.begin_local_round();

    let id = support::spawn(&mut universe, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    give_route(&mut universe, id, 4, 0);

    let before = universe.snapshot().unit(id).cloned().expect("unit").grid;
    universe.end_local_turn(TeamId::RED);

    let unit = universe.snapshot().unit(id).cloned().expect("unit");
    assert_eq!(unit.grid, before, "no speed — no cell is entered");
    assert_eq!(unit.path.as_ref().map(UnitPath::remaining_steps), Some(4));
}

/// The drain is deterministic: the same closing turn produces the same canonical
/// state bytes, including the walked cells and the spent speed.
#[test]
fn the_end_of_turn_walk_is_deterministic() {
    fn close_turn() -> (Vec<u8>, Point, u16) {
        let mut universe = support::game();
        universe.begin_local_round();
        let id = support::spawn(&mut universe, UnitType::Tank, TeamId::RED, Point::new(10, 10));
        give_route(&mut universe, id, 6, 3);
        universe.end_local_turn(TeamId::RED);
        let state = universe.snapshot();
        let unit = state.unit(id).expect("unit");
        (state.snapshot_bytes(), unit.grid, unit.speed)
    }

    let first = close_turn();
    let second = close_turn();

    assert_eq!(first, second, "the turn close is byte-identical");
    assert_eq!(first.1, Point::new(13, 10), "three cells for three points");
    assert_eq!(first.2, 0);
}
