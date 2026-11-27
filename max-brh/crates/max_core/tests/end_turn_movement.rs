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
//! # The movement is paced (visual tempo)
//!
//! The port runs that loop **in the tick**: `GameUniverse::end_local_turn` marks
//! the closing team (`GameState::begin_end_turn_movement` → `ClosingTurn`) and
//! returns; every following tick advances the movement by one visual budget window
//! (`max_core::MOVEMENT_CELLS_PER_SECOND` cells per second of a 60 Hz step), with
//! reaction fire resolved between the cells. The same budget paces a route in the
//! **active** turn as well, not only at the turn close
//! (`the_movement_budget_paces_routes_in_the_active_turn` in `game_tests.rs`). The
//! simulation therefore walks a cell exactly when the frame draws it, and the next
//! team starts on the tick the walk ends — the original's tempo, where
//! `UnitInfo::Move` moves the unit a few pixels per tick and the drawing follows the
//! state.
//!
//! These scenarios drive the drain through [`support::close_turn`] (a headless
//! frame loop) unless a test is *about* the pacing itself.

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

    let id = support::spawn(
        &mut universe,
        UnitType::Tank,
        TeamId::RED,
        Point::new(10, 10),
    );
    give_route(&mut universe, id, 4, 2);

    let start = universe.snapshot().unit(id).cloned().expect("unit").grid;
    assert_eq!(start, Point::new(10, 10));

    let over = support::close_turn(&mut universe, TeamId::RED);
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

/// The turn close is **incremental**: `end_local_turn` no longer walks the route,
/// it marks the closing team, and the movement advances one cell per visual budget
/// window (four cells per second of a 60 Hz simulation step).
///
/// This is the property that keeps the simulation and the drawing together: when
/// the drain ran the whole route inside one call, reaction fire was resolved
/// several cells ahead of the sprite. The same route, walked cell by cell, keeps
/// the attack on the cell the player sees.
#[test]
fn the_turn_close_walks_one_cell_per_visual_budget_window() {
    let mut universe = support::game();
    universe.begin_local_round();

    let id = support::spawn(
        &mut universe,
        UnitType::Tank,
        TeamId::RED,
        Point::new(10, 10),
    );
    // Eight cells and enough speed for all of them.
    give_route(&mut universe, id, 8, 8);

    universe.end_local_turn(TeamId::RED);

    // The turn is not closed yet, and no cell was entered inside the call.
    assert!(
        universe.turn_close_pending(),
        "the turn close must wait for the route"
    );
    assert_eq!(
        universe.snapshot().active_turn_team(),
        TeamId::RED,
        "the next team must not start before the movement is finished"
    );
    assert_eq!(
        universe.snapshot().unit(id).expect("unit").grid,
        Point::new(10, 10),
        "`end_local_turn` must not walk the route by itself"
    );

    // Drive the drain and remember the tick every cell was entered on. The hull
    // first turns toward the first route step (`Paths_UpdateAngle`, `paths.cpp:258`):
    // a turning pass moves no pixel, so it costs no budget and keeps the normal pace.
    let mut entries: Vec<usize> = Vec::new();
    let mut last = universe.snapshot().unit(id).expect("unit").grid.x;

    for tick in 1..=400 {
        universe.tick();

        let grid = universe.snapshot().unit(id).expect("unit").grid;
        if grid.x != last {
            entries.push(tick);
            last = grid.x;
        }

        if !universe.turn_close_pending() {
            break;
        }
    }

    assert_eq!(
        entries.len(),
        8,
        "eight movement points walk eight cells: {entries:?}"
    );
    assert!(
        entries[0] <= 9,
        "the hull turns within a few passes, then the first border is crossed: {entries:?}"
    );

    // Steady state: one cell per 15 ticks — four cells per second at 60 Hz.
    for pair in entries.windows(2).skip(1) {
        assert_eq!(
            pair[1] - pair[0],
            15,
            "a cell takes exactly one visual budget window: {entries:?}"
        );
    }

    assert_eq!(
        universe.snapshot().active_turn_team(),
        TeamId::GREEN,
        "the next team starts on the tick the walk ends"
    );
    assert_eq!(
        universe.snapshot().unit(id).expect("unit").grid,
        Point::new(18, 10),
        "the whole route is walked, one cell per window"
    );
}

/// `fast_movement` ("Double Unit Steps") halves the budget window, as
/// `GameManager_FastMovement` doubles the movement velocity in the original
/// (`unitinfo.cpp:1742`).
#[test]
fn fast_movement_halves_the_budget_window() {
    /// Tick numbers of the cell entries of the closing tank.
    fn entries(fast_movement: bool) -> Vec<usize> {
        let mut universe = support::game();
        universe.begin_local_round();
        universe.set_fast_movement(fast_movement);

        let id = support::spawn(
            &mut universe,
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
        );
        give_route(&mut universe, id, 8, 8);
        universe.end_local_turn(TeamId::RED);

        let mut entries = Vec::new();
        let mut last = universe.snapshot().unit(id).expect("unit").grid.x;

        for tick in 1..=400 {
            universe.tick();

            let grid = universe.snapshot().unit(id).expect("unit").grid;
            if grid.x != last {
                entries.push(tick);
                last = grid.x;
            }

            if !universe.turn_close_pending() {
                break;
            }
        }

        entries
    }

    let normal = entries(false);
    let fast = entries(true);

    assert_eq!(normal.len(), 8);
    assert_eq!(fast.len(), 8, "the setting changes the pace, not the route");

    // The same eight cells: the normal pace needs 4 x 15 ticks for the last four
    // cells, the doubled pace 4 x 7.5 ticks — 8 whole pixels per tick, so the cell
    // border falls every 8 ticks.
    fn span_of_last_four(entries: &[usize]) -> usize {
        entries[entries.len() - 1] - entries[entries.len() - 5]
    }

    assert_eq!(
        span_of_last_four(&normal),
        60,
        "four cells take 4 x 15 ticks at the normal pace: {normal:?}"
    );
    assert_eq!(
        span_of_last_four(&fast),
        32,
        "`fast_movement` doubles the pace: four cells in 32 ticks (8-pixel steps): {fast:?}"
    );
}

/// The pacing does not change the simulation result: the same closing turn walked
/// at either pace ends in byte-identical canonical state (the drain only spaces the
/// passes, exactly like a different frame rate spaces the oracle's `ProgressTurn`).
#[test]
fn the_drain_pace_does_not_change_the_state_bytes() {
    fn close(fast_movement: bool) -> (Vec<u8>, Point, u16, u32) {
        let mut universe = support::game();
        universe.begin_local_round();
        universe.set_fast_movement(fast_movement);

        let id = support::spawn(
            &mut universe,
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
        );
        give_route(&mut universe, id, 6, 3);
        let over = support::close_turn(&mut universe, TeamId::RED);
        assert!(!over, "the match is not over");

        let state = universe.snapshot();
        let unit = state.unit(id).expect("unit");

        (
            state.snapshot_bytes(),
            unit.grid,
            unit.speed,
            universe.closing_turn_passes(),
        )
    }

    let slow = close(false);
    let fast = close(true);

    assert_eq!(
        slow.0, fast.0,
        "the visual pace must not change the canonical state bytes"
    );
    assert_eq!(slow.1, fast.1, "the same cells are walked");
    assert_eq!(slow.1, Point::new(13, 10), "three cells for three points");
    assert_eq!(slow.2, 0, "the remaining speed is spent");
    assert_eq!(
        slow.3, 0,
        "the drain state is gone once the turn close is over"
    );
}

/// Without movement points the route is not walked: the drain is bounded by
/// `speed`, as in `GroundPath::Execute` (`speed == 0` → `MoveFinished`, the path
/// stays for the next turn).
#[test]
fn a_route_without_speed_does_not_advance_at_the_turn_end() {
    let mut universe = support::game();
    universe.begin_local_round();

    let id = support::spawn(
        &mut universe,
        UnitType::Tank,
        TeamId::RED,
        Point::new(10, 10),
    );
    give_route(&mut universe, id, 4, 0);

    let before = universe.snapshot().unit(id).cloned().expect("unit").grid;
    support::close_turn(&mut universe, TeamId::RED);

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
        let id = support::spawn(
            &mut universe,
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
        );
        give_route(&mut universe, id, 6, 3);
        support::close_turn(&mut universe, TeamId::RED);
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

/// The drain state itself is **not** part of the snapshot: a state captured while
/// the turn close is waiting for the route carries no extra field, and the bytes
/// move only when the simulation does.
#[test]
fn the_closing_turn_is_not_part_of_the_snapshot() {
    let mut universe = support::game();
    universe.begin_local_round();
    let id = support::spawn(
        &mut universe,
        UnitType::Tank,
        TeamId::RED,
        Point::new(10, 10),
    );
    give_route(&mut universe, id, 4, 4);
    universe.end_local_turn(TeamId::RED);

    assert!(universe.turn_close_pending(), "the route is still pending");
    let pending = universe.snapshot().snapshot_bytes();

    // The first pass is a real state change (one cell entered)...
    universe.tick();
    assert_ne!(
        pending,
        universe.snapshot().snapshot_bytes(),
        "a walked cell changes the canonical bytes"
    );
    assert!(
        universe.turn_close_pending(),
        "the route is not finished after one cell"
    );

    // ...and the byte length is the same before and after: the drain adds no field
    // to the digest, it only spaces the passes.
    assert_eq!(
        pending.len(),
        universe.snapshot().snapshot_bytes().len(),
        "the closing drain must not extend the snapshot"
    );
}
