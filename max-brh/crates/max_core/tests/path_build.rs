//! Engineer **Path** building — 1:1 by the C++ oracle.
//!
//! Oracle:
//! * `GameManager_PathBuild` (`game_manager.cpp:4566-4598`): an `ENGINEER` pointed
//!   at a cell outside its own gets a linear `GroundPath` to that cell
//!   (`GroundPath` + `AppendLinearSteps`) and `UnitsManager_StartBuild`, which
//!   resets its `move_to` to its own cell — the first object is built where it
//!   stands;
//! * `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3536-3538`): the mobile builder
//!   clears its queue only `if (!path)` — a Path order keeps the head;
//! * `UnitsManager_ProcessOrderBuild`/`UNIT_READY` (`units_manager.cpp:2891`):
//!   `if (unit->path) UnitsManager_BuildNext(unit);`;
//! * `UnitsManager_BuildNext` (`units_manager.cpp:3807-3824`): continue while
//!   `storage >= turns * Cargo_GetRawConsumptionRate(type, 1)` and the path end
//!   is not the current cell; otherwise `ClearBuildListAndPath`;
//! * `UnitInfo::Move` (`unitinfo.cpp:1835-1844`): on finishing a cell of an
//!   `ORDER_BUILD` the builder starts the same type on the new cell.
//!
//! The defect of the manual run: the engineer built one object and stopped —
//! the queue and the path were cleared unconditionally in `SpawnNewUnit`, and
//! `BuildNext`/`Build` were TODOs.

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit::{GroundPath, UnitPath};
use max_core::universe::GameUniverse;
use max_core::{TeamClan, UnitId};

fn path_state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

fn engineer(state: &mut GameUniverse, storage: i16) -> UnitId {
    let id = state
        .spawn_unit(UnitType::Engineer, TeamId::RED, Point::new(10, 10), 0)
        .expect("engineer");

    let unit = state.unit_mut(id).expect("engineer");
    unit.storage = storage;
    unit.build_list = vec![UnitType::Road];
    unit.build_time = 1;
    unit.orders = UnitOrder::Build;
    unit.state = UnitOrderState::Init;
    unit.target_grid = Point::new(10, 10);
    id
}

/// Attaches the linear `GroundPath` that `GameManager_PathBuild` creates for an
/// engineer pointed at `target` (same row/column, as `GetBuilderUnitCursor`
/// allows for `ENGINEER`).
fn path_to(state: &mut GameUniverse, id: UnitId, target: Point) {
    let unit = state.unit_mut(id).expect("engineer");
    let grid = unit.grid;
    let steps = max_core::pathfinding::append_linear_steps(grid, target);

    unit.path = Some(UnitPath::Ground(GroundPath {
        end_x: target.x,
        end_y: target.y,
        step_index: 0,
        steps,
    }));
    state.set_order_data(
        id,
        max_core::game::OrderData {
            move_to: grid,
            fire_on: Point::ZERO,
        },
    );
}

fn count_roads(state: &GameUniverse) -> usize {
    state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
                .snapshot()
                .unit(*id)
                .map(|unit| unit.unit_type == UnitType::Road)
                .unwrap_or(false)
        })
        .count()
}

/// A "turn" of the economy, C++ order: `ManageEconomy` (charging the mobile
/// builder from its storage) → `ProgressBuildState` (`SpawnNewUnits`), then the
/// movement ticks of the builder along the path.
fn turn(state: &mut GameUniverse, ticks: usize) {
    state.manage_economy(TeamId::RED);
    let _ = state.progress_build_state(TeamId::RED);

    for _ in 0..ticks {
        state.tick();
    }
}

#[test]
fn engineer_builds_the_whole_path_while_material_lasts() {
    let mut state = path_state();
    let engineer = engineer(&mut state, 40);
    path_to(&mut state, engineer, Point::new(14, 10));

    // `ProcessOrderBuild`/INIT → `StartBuilding`: the first site is the
    // engineer's own cell (`UnitsManager_StartBuild` resets `move_to` for ENGINEER).
    state.tick();
    assert!(
        state.snapshot().unit(engineer).expect("engineer").build_list == vec![UnitType::Road],
        "the queue head is kept by the path order"
    );
    assert!(
        state
            .snapshot()
            .unit(engineer)
            .expect("engineer")
            .path
            .is_some(),
        "the Path order keeps the route"
    );

    // Five cells (10..=14): each one takes one production turn; the hull walks a
    // cell in one movement-budget window (`MOVEMENT_CELLS_PER_SECOND`, 15 ticks),
    // so a turn gives it enough ticks to complete the next cell.
    for _ in 0..6 {
        turn(&mut state, 20);
    }

    assert_eq!(
        count_roads(&state),
        5,
        "the engineer builds one Road on every cell of the path"
    );

    let unit = state.snapshot().unit(engineer).expect("engineer");
    assert!(unit.path.is_none(), "the route is finished");
    assert!(
        unit.build_list.is_empty(),
        "`ClearBuildListAndPath` cleared the queue"
    );
    assert_eq!(
        unit.orders,
        UnitOrder::Build,
        "the builder waits for the exit order on the last site"
    );
    assert_eq!(unit.state, UnitOrderState::UnitReady);
    assert_eq!(unit.storage, 40 - 5 * 2, "2 raw per Road at 1× rate");
}

#[test]
fn path_build_stops_when_material_runs_out() {
    let mut state = path_state();
    let engineer = engineer(&mut state, 3);
    path_to(&mut state, engineer, Point::new(14, 10));

    state.tick();

    for _ in 0..6 {
        turn(&mut state, 6);
    }

    // 3 raw pays for the first Road (2); 1 left is less than the next rate 2 —
    // `UnitsManager_BuildNext` clears the queue and the path (`ClearBuildListAndPath`).
    assert_eq!(count_roads(&state), 1, "only the first object is paid for");

    let unit = state.snapshot().unit(engineer).expect("engineer");
    assert_eq!(unit.storage, 1, "no debt: the unpaid turn charged nothing");
    assert!(unit.path.is_none(), "the path is cleared on the shortage");
    assert!(unit.build_list.is_empty(), "the queue is cleared on the shortage");
}

#[test]
fn build_next_clears_the_queue_when_the_route_end_is_reached() {
    let mut state = path_state();
    let engineer = engineer(&mut state, 40);
    // One cell: the engineer already stands on the path end.
    path_to(&mut state, engineer, Point::new(10, 10));

    state.tick();
    turn(&mut state, 6);

    assert_eq!(count_roads(&state), 1, "the single site is built");

    turn(&mut state, 6);

    let unit = state.snapshot().unit(engineer).expect("engineer");
    assert!(unit.path.is_none(), "path end reached — the route is cleared");
    assert!(unit.build_list.is_empty(), "no repeated construction");
    assert_eq!(count_roads(&state), 1);
}
