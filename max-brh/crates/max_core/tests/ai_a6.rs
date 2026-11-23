//! Phase A.6 integration tests: `TaskExplore` (map reconnaissance) and
//! `TaskTransport` (ferrying land units across water).
//!
//! Oracles: `taskexplore.cpp`, `tasksearchdestination.cpp`,
//! `tasktransport.cpp`, `taskmove.cpp` (`AttemptTransport`).

use max_core::ai::{TaskKind, TASK_PRIORITY_MOVE, TASK_TYPE_MOVE, TASK_TYPE_TRANSPORT};
use max_core::enums::{
    AiStrategy, CautionLevel, GameStateType, Opponent, Surface, TeamClan, UnitOrder, UnitType,
};
use max_core::geo::Point;
use max_core::map::Map;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::GameUniverse;

/// One team turn: economy → `Ai_BeginTurn` → ticks → end of turn.
fn play_turn(state: &mut GameUniverse, team: TeamId, ticks: usize) {
    state.manage_economy(team);
    state.ai_begin_turn(team);

    for _ in 0..ticks {
        state.tick();
    }

    state.set_game_state(GameStateType::EndTurn);
    let _ = state.finish_turn();
}

/// Land on the west side (`x <= 15`), water in `16..=24`, land from `x = 25`.
fn channel_map() -> Map {
    let mut map = Map::new();

    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            let surface = if (16..=24).contains(&x) {
                Surface::WATER
            } else {
                Surface::LAND
            };
            map.surface.set(x, y, surface);
        }
    }

    map
}

fn computer_state(map: Map) -> GameUniverse {
    let mut state = GameUniverse::with_setup(
        map,
        [
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    state.teams_mut()[TeamId::RED.0 as usize].team_type = TeamKind::Computer;
    state.teams_mut()[TeamId::RED.0 as usize].team_clan = TeamClan::CrimsonPath;
    state.team_units_table_mut()[TeamId::RED.0 as usize] =
        TeamUnits::with_clan(TeamClan::CrimsonPath);
    state.ai_mut().settings.opponent = Opponent::Average as u8;
    state.ai_init();

    state.ai_mut().players[TeamId::RED.0 as usize].strategy = AiStrategy::Sea;

    state
}

/// `TaskExplore` asks for reconnaissance units and drives the scout over the map.
#[test]
fn explore_requests_reconnaissance_and_moves_the_scout() {
    let mut state = computer_state(channel_map());

    let scout = state
        .spawn_unit(UnitType::Scout, TeamId::RED, Point::new(10, 10), 0)
        .expect("scout");

    // The exploration task is created by `AiPlayer::init_tasks`.
    state.ai_begin_turn(TeamId::RED);

    let explore = state
        .snapshot()
        .ai()
        .task_manager
        .tasks()
        .iter()
        .find(|task| matches!(task.kind, TaskKind::Explore { .. }))
        .map(|task| task.id)
        .expect("TaskExplore created by init_tasks");

    let requests = state
        .snapshot()
        .ai()
        .task_manager
        .task(explore)
        .and_then(|task| match &task.kind {
            TaskKind::Explore {
                obtain_requests, ..
            } => Some(obtain_requests.clone()),
            _ => None,
        })
        .expect("explore state");

    assert!(
        requests.contains(&UnitType::Scout),
        "TaskExplore must request a SCOUT, got {requests:?}"
    );
    assert!(
        requests.contains(&UnitType::Escort) || requests.contains(&UnitType::Submarine),
        "TaskExplore must request the fleet reconnaissance types, got {requests:?}"
    );

    // `IsUnitUsable` takes the scout (SCOUT is the first requested type).
    assert!(
        state
            .snapshot().task_is_unit_usable(explore, scout),
        "TaskExplore must accept the idle scout"
    );

    state.set_game_state(GameStateType::EndTurn);
    let _ = state.finish_turn();

    let start = state.snapshot().unit(scout).expect("scout alive").grid;
    let mut moved = false;
    let mut saw_move = false;

    for _ in 0..8 {
        play_turn(&mut state, TeamId::RED, 16);

        if state
            .snapshot()
            .ai()
            .task_manager
            .tasks()
            .iter()
            .any(|task| task.get_type() == TASK_TYPE_MOVE)
        {
            saw_move = true;
        }

        let position = state.snapshot().unit(scout).expect("scout alive").grid;
        if position != start {
            moved = true;
            break;
        }
    }

    assert!(
        saw_move,
        "TaskExplore must issue a TaskMove to a search destination"
    );
    assert!(
        moved,
        "TaskExplore must send the scout to an unexplored area (scout stays at {start:?})"
    );
}

/// The whole `AttemptTransport` → `TaskTransport` chain: the split of the move
/// at the channel, boarding the sea transport and unloading on the far shore.
///
/// The tasks are driven directly (`TaskManager::begin_turn`/`end_turn` +
/// `RemindMoveFinished` + `ExecuteReminders`) instead of `AiPlayer::BeginTurn`:
/// the planner would pull the tank into its own combat/defense tasks before the
/// transport flow can run.
#[test]
fn ai_ferries_a_tank_across_the_channel() {
    let mut state = computer_state(channel_map());

    // No `AiPlayer::BeginTurn`: the planner would own the tank. Only the task
    // manager runs, which is exactly the `TaskMove`/`TaskTransport` subsystem.
    state.ai_mut().players[TeamId::RED.0 as usize].need_init = false;
    state.ai_mut().players[TeamId::RED.0 as usize].tasks_pending = false;

    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("tank");
    let ferry = state
        .spawn_unit(UnitType::SeaTransport, TeamId::RED, Point::new(20, 10), 0)
        .expect("sea transport");

    // A plain move across the water: `TaskMove` has to call `AttemptTransport`.
    let destination = Point::new(32, 10);
    state.ai_append_task(
        TeamId::RED,
        None,
        TASK_PRIORITY_MOVE,
        TaskKind::move_to(tank, destination, 0, CautionLevel::AvoidAllDamage),
    );

    let mut boarded = false;
    let mut landed = false;

    for _ in 0..16 {
        state.manage_economy(TeamId::RED);
        state
            .ai_task_begin_turn(TeamId::RED);
        state.ai_remind_move_finished(tank, true);
        state.ai_remind_move_finished(ferry, true);
        state.execute_reminders();

        for _ in 0..16 {
            state.tick();
            state.execute_reminders();
        }

        state
            .ai_task_end_turn(TeamId::RED);
        state.execute_reminders();

        state.set_game_state(GameStateType::EndTurn);
        let _ = state.finish_turn();

        let tasks = state.snapshot().ai().task_manager.tasks();
        let transport_exists = tasks.iter().any(|task| {
            task.get_type() == TASK_TYPE_TRANSPORT
                && matches!(task.kind, TaskKind::Transport { .. })
        });
        let tank_loaded = state
            .snapshot()
            .unit(tank)
            .map(|unit| unit.orders == UnitOrder::Idle && unit.parent_unit.is_some())
            .unwrap_or(false);

        if transport_exists || tank_loaded {
            boarded = true;
        }

        let tank_grid = state.snapshot().unit(tank).map(|unit| unit.grid);
        if tank_grid.map(|grid| grid.x >= 25).unwrap_or(false) {
            landed = true;
            break;
        }
    }

    assert!(
        boarded,
        "TaskMove must create a TaskTransport and board the sea transport"
    );
    assert!(
        landed,
        "the tank must be unloaded on the far shore, tank at {:?}",
        state.snapshot().unit(tank).map(|unit| unit.grid)
    );
}
