//! Stage A: `App` on top of `GameUniverse` (stage-A task
//! `game-universe-stage-a.md` §4).
//!
//! What is proved here:
//!
//! * `App::state_ref` is the same state as [`App::snapshot`] — the facade snapshot
//!   carries the canonical bytes (`StateRef::snapshot_bytes`) and the presentation
//!   read path;
//! * `App::tick` goes through the facade and adds nothing to the simulation state:
//!   a run through [`App::tick`] is byte-identical to a run through
//!   `GameState::tick`;
//! * `App::submit` is a 1:1 facade call: it reaches the same state as the
//!   `GameUniverse::submit` it delegates to (stage C §5 removed the bridge).

use max_app::net::dispatcher::NetworkDispatcher;
use max_app::{App, AppConfig};
use max_core::enums::TeamClan;
use max_core::enums::{PlayMode, UnitOrder, UnitOrderState};
use max_core::events::{command_end_turn_barrier, command_finished_turn, command_move_linear};
use max_core::geo::Point;
use max_core::input::{Command, Packet13Seed};
use max_core::map::Map;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::{GameUniverse, SubmitOutcome};

fn app() -> App {
    let mut app = App::new(AppConfig::default());
    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().teams_mut()[1].team_type = TeamKind::Computer;
    app.universe_mut().team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    app.universe_mut().team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);
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

    // `Q22`: the presentation fields are part of the facade snapshot.
    let id = app
        .universe_mut()
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
}

#[test]
fn app_tick_adds_nothing_to_the_simulation_state() {
    // Two identical applications: one ticks through the facade (`App::tick`), the
    // other through the raw `GameState::tick` — the snapshots must stay identical.
    let mut facade = app();
    let mut direct = app();

    for _ in 0..5 {
        facade.tick();
        direct.universe_mut().tick();
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
    let state = facade.state_ref();
    assert_eq!(state.frame_sync_counter(), [0; max_core::TEAM_SLOTS]);
    assert_eq!(state.turn_indices(), [0; max_core::TEAM_SLOTS]);
    assert_eq!(state.next_turn_indices(), [0; max_core::TEAM_SLOTS]);
}

#[test]
fn app_submit_matches_the_facade_queue() {
    let mut submitted = app();
    let mut direct = app();

    for command in commands() {
        assert_eq!(submitted.submit(command.clone()), SubmitOutcome::Accepted);
        direct.universe_mut().submit(command);
    }

    for _ in 0..4 {
        submitted.tick();
        direct.tick();
    }

    assert_eq!(
        submitted.snapshot(),
        direct.snapshot(),
        "App::submit must feed the same queue as the facade submit"
    );
    assert_eq!(
        submitted.digest().state_digest,
        direct.digest().state_digest
    );
}

#[test]
fn app_order_command_travels_through_the_facade() {
    // A move order via the facade queue: the unit starts moving after the tick
    // that applies it — the documented one-command stage of the single path.
    let mut app = app();
    let red = app
        .universe_mut()
        .spawn_unit(
            max_core::enums::UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            max_core::enums::unit_flags::MOBILE_LAND_UNIT | max_core::enums::unit_flags::SELECTABLE,
        )
        .expect("tank");
    app.universe_mut().set_active_turn_team(TeamId::RED);
    app.universe_mut().set_player_team(TeamId::RED);

    let order = command_move_linear(
        red,
        Point::new(10, 10),
        Point::new(14, 10),
        UnitOrder::Move,
        UnitOrderState::InProgress,
    );
    assert_eq!(app.submit(order), SubmitOutcome::Accepted);
    assert!(!app.state_ref().commands().is_empty(), "the order is queued");

    app.tick();
    assert!(
        app.state_ref().commands().is_empty(),
        "the order is applied at the start of the tick"
    );
}

// ---------------------------------------------------------------------------
// 5. Stage C §3 — the explicit-dispatcher setup constructor (A3)
// ---------------------------------------------------------------------------

#[test]
fn with_setup_and_dispatcher_accepts_the_network_dispatcher() {
    // The network constructor is the same world generator as `with_setup`: only
    // the dispatcher differs, and the dispatcher is facade state (ADR §3), so the
    // canonical bytes and the CRC must not depend on it.
    let team_kinds = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    let map = Map::new();

    let local = GameUniverse::with_setup(map.clone(), team_kinds);
    let network = GameUniverse::with_setup_and_dispatcher(
        map,
        team_kinds,
        NetworkDispatcher::new(TeamId::RED, PlayMode::TurnBased),
    );

    assert_eq!(
        local.snapshot().snapshot_bytes(),
        network.snapshot().snapshot_bytes(),
        "a network setup must be byte-identical to a local setup"
    );
    assert_eq!(
        local.snapshot().state_digest(),
        network.snapshot().state_digest()
    );
    assert_eq!(local.seq(), network.seq());
}
