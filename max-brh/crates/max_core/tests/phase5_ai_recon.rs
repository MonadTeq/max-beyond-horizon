//! Phase 5 — AI reconnaissance: `TaskExplore` and `TaskSearchDestination`
//! (area A.6).
//!
//! Scenario: an AI base + a passive enemy base and a scout → rounds through the
//! core turn machine → `TaskExplore`, its `TaskObtainUnits` requests, the
//! `TaskMove` to an unexplored site, the scout's travel and the growth of the
//! AI's `info_map`.
//!
//! Oracles: `taskexplore.cpp` (`IsVisited`, `ObtainUnit`),
//! `taskabstractsearch.cpp:79` (`EndTurn`), `tasksearchdestination.cpp`,
//! `taskmove.cpp`.

mod support;

use max_core::enums::{AiStrategy, Opponent, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use support::phase5::{
    count_units, explored_count, green_base, obtain_units_types, play_rounds_observed, red_base,
    spawn, strategy_state, AiObservation, ROUND_TICKS,
};

/// `TaskExplore` asks for reconnaissance units and drives the scout over the
/// map, marking the explored cells in `AiPlayer::info_map`.
#[test]
fn ai_explores_the_map_with_a_scout() {
    let mut state = strategy_state(AiStrategy::CombinedArms, Opponent::Average);
    red_base(&mut state, Point::new(10, 10), 500);
    green_base(&mut state, Point::new(30, 30));

    let scout = spawn(&mut state, UnitType::Scout, TeamId::RED, Point::new(12, 12));

    let start = state.snapshot().unit(scout).expect("scout").grid;
    let explored_before = explored_count(&state, TeamId::RED);

    let mut observation = AiObservation::default();
    let (_, over) = play_rounds_observed(&mut state, 2, ROUND_TICKS, TeamId::RED, &mut observation);
    assert!(!over);

    assert!(
        observation.saw_explore_task,
        "TaskExplore is created by `init_tasks`"
    );
    assert!(
        observation.saw_move_task,
        "TaskExplore must issue a TaskMove to a search destination"
    );

    let position = state.snapshot().unit(scout).expect("scout alive").grid;
    assert_ne!(
        position, start,
        "the scout must travel to an unexplored site (start {start:?})"
    );

    let explored_after = explored_count(&state, TeamId::RED);
    assert!(
        explored_after > explored_before,
        "the scout must mark explored cells: {explored_before} → {explored_after}"
    );
    assert!(
        observation.max_explored >= explored_after,
        "the observation must sample the growth"
    );
}

/// `TaskExplore::ObtainUnit` requests the reconnaissance types: SCOUT for the
/// land a scout, AWAC/FASTBOAT/SUBMARNE when the team can build them
/// (`taskexplore.cpp:36-46`).
#[test]
fn explore_requests_reconnaissance_types() {
    let mut state = strategy_state(AiStrategy::CombinedArms, Opponent::Average);
    red_base(&mut state, Point::new(10, 10), 500);
    green_base(&mut state, Point::new(30, 30));

    let (_, over) = play_rounds_observed(
        &mut state,
        1,
        ROUND_TICKS,
        TeamId::RED,
        &mut AiObservation::default(),
    );
    assert!(!over);

    let requested = obtain_units_types(&state, TeamId::RED);
    for expected in [
        UnitType::Scout,
        UnitType::Awac,
        UnitType::Escort,    // FASTBOAT in C++
        UnitType::Submarine, // SUBMARNE
    ] {
        assert!(
            requested.contains(&expected),
            "TaskExplore must request {expected:?}, requests: {requested:?}"
        );
    }
}

/// A scout that is already in the field is taken by `TaskExplore`
/// (`IsUnitUsable`), so no extra `TaskObtainUnits(SCOUT)` is needed.
#[test]
fn explore_uses_the_available_scout() {
    let mut state = strategy_state(AiStrategy::CombinedArms, Opponent::Average);
    red_base(&mut state, Point::new(10, 10), 500);
    green_base(&mut state, Point::new(30, 30));

    let scout = spawn(&mut state, UnitType::Scout, TeamId::RED, Point::new(12, 12));

    let mut observation = AiObservation::default();
    let (_, over) = play_rounds_observed(&mut state, 1, ROUND_TICKS, TeamId::RED, &mut observation);
    assert!(!over);

    assert_eq!(count_units(&state, TeamId::RED, UnitType::Scout), 1);

    let attached = state
        .snapshot()
        .ai()
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == TeamId::RED)
        .any(|task| {
            matches!(task.kind, max_core::ai::TaskKind::Explore { .. })
                && state
                    .snapshot()
                    .ai()
                    .task_manager
                    .unit_task(scout)
                    .map(|task_id| task_id == task.id)
                    .unwrap_or(false)
        });

    assert!(
        attached || observation.saw_move_task,
        "the scout must be attached to TaskExplore (or already moving)"
    );
    assert!(
        state.snapshot().unit(scout).expect("scout").hits > 0,
        "the scout survives the exploration turn"
    );
}
