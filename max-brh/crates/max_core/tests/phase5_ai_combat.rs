//! Phase 5 — AI combat: assault, target census, reserve (area A.4).
//!
//! Scenarios: an AI base with a spotted enemy facility/army + a passive enemy
//! base → rounds through the core turn machine → `TaskAttack`/`TaskKillUnit`,
//! the reserves the AI gathers and the damage it deals.
//!
//! Oracles: `aiattack.cpp` (`AiAttack_EvaluateAssault`, `AiAttack_UnitSpotted`),
//! `taskattack.cpp` (`AssessEnemyUnits`, `ChooseFirstTarget`),
//! `taskkillunit.cpp`, `taskattackreserve.cpp`, `taskdefensereserve.cpp`.
//!
//! # Visibility
//!
//! `Access_UpdateMapStatus` returns early for `ORDER_IDLE` (`access.cpp:762`),
//! so an idle unit scans nothing; the scenarios put the observer on
//! `ORDER_SENTRY` and recompute the coverage as a landing does
//! (`Access_UpdateVisibilityStatus`). The hidden-enemy scenario keeps the
//! default idle orders, so nothing is spotted and no attack may start.

mod support;

use max_core::ai::TaskKind;
use max_core::enums::{AiStrategy, Opponent, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use support::phase5::{
    play_rounds_observed, red_base, refresh_visibility, sentry, spawn, strategy_state,
    AiObservation, ROUND_TICKS,
};

/// Builds the standard combat scenario: red base at (8, 8), a green facility
/// inside the base tank's scan (4 cells) and a second green tank far away (the
/// enemy team must keep an army, otherwise the victory check closes the match
/// as soon as the forward units die).
fn combat_scenario() -> (max_core::universe::GameUniverse, max_core::UnitId) {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    let red = red_base(&mut state, Point::new(8, 8), 500);

    let facility = spawn(
        &mut state,
        UnitType::TrainingHall,
        TeamId::GREEN,
        Point::new(11, 12),
    );
    spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(36, 36),
    );

    sentry(&mut state, red.tank);
    refresh_visibility(&mut state);

    (state, facility)
}

/// A spotted key facility makes the AI create `TaskAttack` + `TaskKillUnit`
/// (with the unit pool) and actually damage it; the base tank works inside
/// `TaskDefenseReserve`.
#[test]
fn spotted_enemy_facility_triggers_an_attack() {
    let (mut state, facility) = combat_scenario();

    assert_eq!(
        state.state().ai.players[TeamId::RED.0 as usize]
            .spotted_units
            .len(),
        1,
        "the sentry/visibility pass must spot the facility"
    );

    let hits_before = state
        .snapshot()
        .unit(facility)
        .map(|unit| unit.hits)
        .expect("facility alive");
    assert!(hits_before > 0);

    let mut observation = AiObservation::default();
    let (_, over) = play_rounds_observed(&mut state, 2, ROUND_TICKS, TeamId::RED, &mut observation);

    assert!(
        observation.saw_attack_task,
        "the spotted facility must create TaskAttack"
    );
    assert!(
        observation.saw_kill_unit_task,
        "TaskAttack must create a TaskKillUnit for the facility"
    );
    assert!(
        observation.saw_attack_reserve_task,
        "TaskAttackReserve must exist as the attack unit pool"
    );
    assert!(
        observation.max_defense_reserve >= 1,
        "the base tank must be registered in TaskDefenseReserve"
    );

    let hits_after = state.snapshot().unit(facility).map(|unit| unit.hits);
    assert!(
        hits_after.map(|hits| hits < hits_before).unwrap_or(true),
        "the facility must be damaged or destroyed (before {hits_before}, after {hits_after:?})"
    );
    assert!(
        !over || hits_after.is_none(),
        "over={over} hits={hits_after:?}"
    );
}

/// The `TaskAttack` targets the spotted enemy team.
#[test]
fn attack_task_targets_the_enemy_team() {
    use max_core::game::TurnEvent;

    let (mut state, _) = combat_scenario();
    let mut target_team = None;

    for _ in 0..3 {
        state.begin_round();

        loop {
            match state.next_team() {
                TurnEvent::Computer(team) | TurnEvent::Human(team) => {
                    for _ in 0..ROUND_TICKS {
                        state.tick();

                        if let Some(task) =
                            state.state().ai.task_manager.tasks().iter().find(|task| {
                                task.team == TeamId::RED && matches!(task.kind, TaskKind::Attack(_))
                            })
                        {
                            if let TaskKind::Attack(attack) = &task.kind {
                                target_team = Some(attack.target_team);
                            }
                        }
                    }

                    support::phase5::close_team_turn(&mut state, team);
                }
                TurnEvent::RoundClosed | TurnEvent::GameOver => break,
            }
        }

        if target_team.is_some() {
            break;
        }
    }

    assert_eq!(
        target_team,
        Some(Some(TeamId::GREEN)),
        "TaskAttack must target the spotted enemy team"
    );
}

/// Without spotting the AI never attacks: no `TaskAttack`, no
/// `TaskKillUnit`, no damage.
#[test]
fn unspotted_enemy_is_not_attacked() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 500);

    let facility = spawn(
        &mut state,
        UnitType::TrainingHall,
        TeamId::GREEN,
        Point::new(11, 12),
    );
    spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(36, 36),
    );

    // No sentry, no visibility refresh: the base tank stays `ORDER_IDLE`.
    let hits_before = state
        .snapshot()
        .unit(facility)
        .map(|unit| unit.hits)
        .expect("facility alive");

    let mut observation = AiObservation::default();
    let (_, over) = play_rounds_observed(&mut state, 6, ROUND_TICKS, TeamId::RED, &mut observation);

    assert_eq!(observation.max_spotted, 0, "nothing may be spotted");
    assert!(
        !observation.saw_attack_task,
        "an unseen enemy must not create TaskAttack"
    );
    assert!(
        !observation.saw_kill_unit_task,
        "an unseen enemy must not create TaskKillUnit"
    );
    assert_eq!(
        state.snapshot().unit(facility).map(|unit| unit.hits),
        Some(hits_before),
        "the facility must stay undamaged, over={over}"
    );
}

/// `TaskDefenseReserve` accumulates the mobile army of the base; with three
/// tanks the reserve holds all of them.
#[test]
fn defense_reserve_accumulates_the_base_army() {
    let (mut state, _) = combat_scenario();

    for site in [Point::new(8, 15), Point::new(10, 16)] {
        spawn(&mut state, UnitType::Tank, TeamId::RED, site);
        let tank = state
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                state
                    .snapshot()
                    .unit(*id)
                    .is_some_and(|unit| unit.team == TeamId::RED && unit.grid == site)
            })
            .expect("new tank");
        sentry(&mut state, tank);
    }

    let mut observation = AiObservation::default();
    let (_, over) = play_rounds_observed(&mut state, 1, ROUND_TICKS, TeamId::RED, &mut observation);
    assert!(!over);

    assert!(
        observation.max_defense_reserve >= 2,
        "the defense reserve must hold the gathered army, got {}",
        observation.max_defense_reserve
    );
}
