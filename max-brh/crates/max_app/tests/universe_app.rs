//! Stage A: `App` on top of `GameUniverse` (ТЗ `game-universe-stage-a.md` §4).
//!
//! What is proved here:
//!
//! * `App::state_ref` is the same state as [`App::snapshot`] — the facade snapshot
//!   carries the canonical bytes (`StateRef::snapshot_bytes`) and the presentation
//!   read path;
//! * `App::tick` goes through the facade and adds nothing to the simulation state:
//!   a run through [`App::tick`] is byte-identical to a run through
//!   `GameState::tick`;
//! * `App::submit` reaches the same state as [`App::enqueue`] (the single command
//!   path of stage A).

use max_app::{App, AppConfig};
use max_core::enums::TeamClan;
use max_core::enums::{UnitOrder, UnitOrderState};
use max_core::events::{command_end_turn_barrier, command_finished_turn, command_move_linear};
use max_core::geo::Point;
use max_core::input::{Command, Packet13Seed};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::SubmitOutcome;

fn app() -> App {
    let mut app = App::new(AppConfig::default());
    app.state_mut().teams[0].team_type = TeamKind::Player;
    app.state_mut().teams[1].team_type = TeamKind::Computer;
    app.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    app.state_mut().team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    app
}

fn commands() -> Vec<Command> {
    vec![
        Command::Seed(Packet13Seed { seed: 0xDEAD_BEEF }),
        command_finished_turn(TeamId::RED),
        command_end_turn_barrier(TeamId::RED, 1),
    ]
}

#[test]
fn state_ref_is_the_canonical_state_of_the_app() {
    let mut app = app();
    for _ in 0..3 {
        app.tick();
    }

    assert_eq!(
        app.state_ref().snapshot_bytes(),
        app.snapshot(),
        "the facade snapshot must carry the canonical state bytes"
    );
    assert_eq!(app.state_ref().turn_counter(), app.state().turn_counter);
    assert_eq!(
        app.state_ref().active_turn_team(),
        app.state().active_turn_team
    );

    // `Q22`: the presentation fields are part of the facade snapshot.
    let id = app
        .state_mut()
        .spawn_unit(
            max_core::enums::UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            max_core::enums::unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("unit");
    assert!(
        app.state_ref().presentation(id).is_some(),
        "UnitPresentation must be readable through StateRef"
    );
    assert_eq!(
        app.state()
            .presentation(id)
            .map(|presentation| presentation.clone()),
        app.state_ref()
            .presentation(id)
            .map(|presentation| presentation.clone()),
        "StateRef::presentation is the same field as GameState::presentation"
    );
}

#[test]
fn app_tick_adds_nothing_to_the_simulation_state() {
    // Two identical applications: one ticks through the facade (`App::tick`), the
    // other through the raw `GameState::tick` — the snapshots must stay identical.
    let mut facade = app();
    let mut direct = app();

    for _ in 0..5 {
        facade.tick();
        direct.state_mut().tick();
    }

    assert_eq!(
        facade.snapshot(),
        direct.snapshot(),
        "App::tick must be the same simulation step as GameState::tick"
    );
    assert_eq!(
        facade.digest().state_digest,
        direct.digest().state_digest,
        "the digests must match"
    );

    // Single player never steps the barrier counters (gate 4, owner item 4).
    let state = facade.state();
    assert_eq!(state.frame_sync_counter, [0; max_core::TEAM_SLOTS]);
    assert_eq!(state.turn_indices, [0; max_core::TEAM_SLOTS]);
    assert_eq!(state.next_turn_indices, [0; max_core::TEAM_SLOTS]);
}

#[test]
fn app_submit_and_enqueue_reach_the_same_state() {
    let mut submitted = app();
    let mut enqueued = app();

    for command in commands() {
        assert_eq!(submitted.submit(command.clone()), SubmitOutcome::Accepted);
        enqueued.enqueue(command);
    }

    for _ in 0..4 {
        submitted.tick();
        enqueued.tick();
    }

    assert_eq!(
        submitted.snapshot(),
        enqueued.snapshot(),
        "App::submit must feed the same queue as App::enqueue"
    );
    assert_eq!(
        submitted.digest().state_digest,
        enqueued.digest().state_digest
    );
}

#[test]
fn app_order_command_travels_through_the_facade() {
    // A move order via the facade queue: the unit starts moving after the tick
    // that applies it — the documented one-command stage of the single path.
    let mut app = app();
    let red = app
        .state_mut()
        .spawn_unit(
            max_core::enums::UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            max_core::enums::unit_flags::MOBILE_LAND_UNIT | max_core::enums::unit_flags::SELECTABLE,
        )
        .expect("tank");
    app.state_mut().active_turn_team = TeamId::RED;
    app.state_mut().player_team = TeamId::RED;

    let order = command_move_linear(
        red,
        Point::new(10, 10),
        Point::new(14, 10),
        UnitOrder::Move,
        UnitOrderState::InProgress,
    );
    assert_eq!(app.submit(order), SubmitOutcome::Accepted);
    assert!(!app.state().commands.is_empty(), "the order is queued");

    app.tick();
    assert!(
        app.state().commands.is_empty(),
        "the order is applied at the start of the tick"
    );
}
