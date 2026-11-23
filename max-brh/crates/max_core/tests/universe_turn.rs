//! Step 5 of stage A: the team-turn machine inside the facade, gated by policy
//! (stage-A task `game-universe-stage-a.md` §6, ADR
//! `game-universe-facade.md` §1/§2/§3).
//!
//! What is proved here (the mandatory gates):
//!
//! 1. **Gate «06 without a shift»** — [`GameUniverse::end_local_turn`] produces a
//!    byte-identical `snapshot_bytes` to the pre-facade sequence
//!    `apply_command(06) + enter_end_turn + next_team` loop (`local.rs:789-826`
//!    before the move), step by step, for a solo turn-based match and for a
//!    hot-seat round. The `TurnNotice` peek data equals the values the old loop
//!    read before each step.
//! 2. **Gate «path distinguishability»** — `end_local_turn` (immediate `06`) and
//!    `submit(06)` (queue) behave **differently**: the immediate path starts the
//!    next team right away; the queued path does not touch the state until the next
//!    tick and does not close the turn then either.
//! 3. **Gate 4 through the facade** — a single-player cycle
//!    `tick → end_local_turn → tick → end_local_turn → turn_counter + 1` passes
//!    **without** `step_*_barrier` (the counters stay `0`) and its `snapshot_bytes`
//!    is identical to the barrier-free network-style run.
//! 4. `Dispatcher::advances_turns` — `LocalDispatcher` → `true`, the trait default
//!    → `false` (fail-closed).
//! 5. `TurnNotice.status` — `Some` only on the closing step that ends the match
//!    (`GameOver`), `None` for `Human`/`Computer`/`RoundClosed`.
//! 6. [`GameUniverse::begin_local_round`] — the round entry that keeps `next_team`
//!    inside the facade (stage-A task §6 item 4): the same state as `begin_round` +
//!    `next_team` on the raw state.

use max_core::crc;
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{PlayMode, TeamClan, UnitType, VictoryType};
use max_core::events::{apply_command, command_finished_turn};
use max_core::geo::Point;
use max_core::input::Command;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::{
    BarrierFacts, Dispatcher, EventStream, GameEvent, GameUniverse, LocalDispatcher,
    NetworkCommand, RemoteCommand, RemoteHandling, RoutedCommand, SubmitOutcome, TurnNotice,
};
use max_core::victory::{self, WinLossStatus};
use max_core::{GameState, TurnEvent, TEAM_SLOTS};

// ---------------------------------------------------------------------------
// Scenario helpers
// ---------------------------------------------------------------------------

/// A two-team turn-based match with one tank per side.
///
/// `hot_seat` — RED and GREEN are human, otherwise GREEN is a computer.
/// The state is configured but the round is **not** entered yet (the callers do
/// that identically on both paths).
fn two_team_state(hot_seat: bool) -> GameState {
    let mut state = GameState::new();
    state.play_mode = PlayMode::TurnBased;
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = if hot_seat {
        TeamKind::Player
    } else {
        TeamKind::Computer
    };
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    state.active_turn_team = TeamId::RED;
    state.player_team = TeamId::RED;

    for (team, position) in [
        (TeamId::RED, Point::new(10, 10)),
        (TeamId::GREEN, Point::new(20, 10)),
    ] {
        let _ = state
            .spawn_unit(
                UnitType::Tank,
                team,
                position,
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .expect("tank");
    }

    state
}

/// The same match in the middle of RED's turn: the round is entered and RED acts
/// (the state `local_next_team` left behind after the first entry).
fn started_state(hot_seat: bool) -> GameState {
    let mut state = two_team_state(hot_seat);
    state.begin_round();
    let event = state.next_team();
    assert_eq!(event, TurnEvent::Human(TeamId::RED));
    state
}

/// One step of the turn machine as the application saw it before the move.
type LegacyStep = (TurnEvent, TeamId, Option<WinLossStatus>);

/// The pre-facade close (`local.rs:1032-1055` before the move): immediate `06`,
/// `enter_end_turn`, then the `local.rs:789-826` loop over the raw state.
///
/// Returns the trace of every step: the event and the two peek values.
fn legacy_close(state: &mut GameState, team: TeamId) -> Vec<LegacyStep> {
    apply_command(state, &command_finished_turn(team));
    state.enter_end_turn();

    let mut trace = Vec::new();

    loop {
        let previous_player_team = state.player_team;
        let closing = state.upcoming_team().is_none();
        let status = closing.then(|| victory::evaluate_status(state, state.turn_counter));

        let event = state.next_team();

        trace.push((
            event,
            previous_player_team,
            if event == TurnEvent::GameOver {
                status
            } else {
                None
            },
        ));

        match event {
            TurnEvent::Human(_) | TurnEvent::Computer(_) | TurnEvent::GameOver => break,
            TurnEvent::RoundClosed => {}
        }
    }

    trace
}

/// Drains the turn notices published by the facade since the previous call.
fn drain_turn_notices(
    universe: &mut GameUniverse<LocalDispatcher>,
    stream: &mut EventStream,
) -> Vec<TurnNotice> {
    stream
        .drain(universe)
        .into_iter()
        .filter_map(|event| match event {
            GameEvent::Turn(notice) => Some(notice),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. Gate «06 without a shift»
// ---------------------------------------------------------------------------

/// Closes `rounds` turns on both paths and compares the state and the peek trace
/// after every step.
fn check_06_without_a_shift(hot_seat: bool, closes: usize) {
    let mut legacy = started_state(hot_seat);
    let mut universe = GameUniverse::new(started_state(hot_seat), LocalDispatcher);
    let mut stream = universe.subscribe();

    assert_eq!(
        universe.state().snapshot_bytes(),
        legacy.snapshot_bytes(),
        "the two runs must start from the same state"
    );

    for step in 0..closes {
        let team = legacy.active_turn_team;
        assert_eq!(
            universe.state().active_turn_team,
            team,
            "step {step}: both paths must act for the same team"
        );

        let trace = legacy_close(&mut legacy, team);
        universe.end_local_turn(team);

        assert_eq!(
            universe.state().snapshot_bytes(),
            legacy.snapshot_bytes(),
            "step {step} (team {team:?}): end_local_turn ≠ apply(06)+enter_end_turn+next_team"
        );
        assert_eq!(universe.state().turn_counter, legacy.turn_counter);
        assert_eq!(universe.state().player_team, legacy.player_team);
        assert_eq!(universe.state().round_team, legacy.round_team);
        assert_eq!(universe.state().active_turn_team, legacy.active_turn_team);

        // The peek data is exactly what the old application loop read before
        // every step.
        let notices = drain_turn_notices(&mut universe, &mut stream);
        assert_eq!(
            notices.len(),
            trace.len(),
            "step {step}: one notice per step"
        );

        for (index, (notice, (event, previous, status))) in
            notices.iter().zip(trace.iter()).enumerate()
        {
            assert_eq!(notice.event, *event, "step {step}.{index}: event");
            assert_eq!(
                notice.previous_player_team, *previous,
                "step {step}.{index}: previous_player_team must be peeked before the switch"
            );
            assert_eq!(notice.status, *status, "step {step}.{index}: status");
        }

        // Gate 4 part: the local machine never steps the barrier counters.
        let state = universe.state();
        assert_eq!(state.frame_sync_counter, [0; TEAM_SLOTS]);
        assert_eq!(state.turn_indices, [0; TEAM_SLOTS]);
        assert_eq!(state.next_turn_indices, [0; TEAM_SLOTS]);
        assert_eq!(state.team_data_crc16, [0; TEAM_SLOTS]);
    }
}

#[test]
fn end_local_turn_is_byte_identical_to_the_legacy_close_solo() {
    // RED (human) → GREEN (computer) → round close → RED again.
    check_06_without_a_shift(false, 3);
    check_06_without_a_shift(false, 8);
}

#[test]
fn end_local_turn_is_byte_identical_to_the_legacy_close_hot_seat() {
    // RED (human) → GREEN (human) → round close → RED again, with the
    // «Begin turn» peek data (`previous_player_team`) switching every team.
    check_06_without_a_shift(true, 4);
    check_06_without_a_shift(true, 9);
}

// ---------------------------------------------------------------------------
// 2. Gate «path distinguishability»
// ---------------------------------------------------------------------------

#[test]
fn end_local_turn_and_submit_06_are_distinguishable() {
    let mut queued = GameUniverse::new(started_state(false), LocalDispatcher);
    let mut immediate = GameUniverse::new(started_state(false), LocalDispatcher);
    let mut stream = queued.subscribe();

    let before = queued.state().snapshot_bytes();

    // Path 1: `submit(06)` — only the queue. Nothing is applied, the turn stays
    // open and no turn notice is produced.
    assert_eq!(
        queued.submit(command_finished_turn(TeamId::RED)),
        SubmitOutcome::Accepted
    );
    assert_eq!(queued.state().commands.len(), 1, "the command is queued");
    assert_eq!(
        queued.state().snapshot_bytes(),
        before,
        "submit must not mutate the state"
    );
    assert!(!queued.state().teams[0].finished_turn);
    assert_eq!(queued.state().active_turn_team, TeamId::RED);
    assert!(
        drain_turn_notices(&mut queued, &mut stream).is_empty(),
        "the queue path must not run the turn machine"
    );

    // Path 2: `end_local_turn(06)` — immediate apply + machine: the next team is
    // already acting before any tick.
    immediate.end_local_turn(TeamId::RED);
    assert!(
        immediate.state().teams[0].finished_turn,
        "06 is applied immediately"
    );
    assert_eq!(
        immediate.state().active_turn_team,
        TeamId::GREEN,
        "the next team is already acting"
    );
    assert_eq!(immediate.state().round_team, 2, "the round cursor moved");
    assert_ne!(
        immediate.state().snapshot_bytes(),
        before,
        "the immediate path changes the state"
    );

    // The queued `06` is applied at the start of the next tick — and even then it
    // does not close the turn by itself (the machine is a facade operation, not a
    // tick phase).
    queued.tick();
    assert!(queued.state().teams[0].finished_turn);
    assert_eq!(
        queued.state().active_turn_team,
        TeamId::RED,
        "a queued 06 does not start the next team"
    );
    assert_eq!(
        queued.state().round_team,
        1,
        "a queued 06 does not move the round cursor"
    );

    // The two paths are distinguishable in the state and the snapshots differ.
    assert_ne!(
        immediate.state().snapshot_bytes(),
        queued.state().snapshot_bytes(),
        "the two 06 paths must not collapse into one behaviour"
    );
}

// ---------------------------------------------------------------------------
// 3. Gate 4 through the facade — a full single-player cycle, no barrier steppers
// ---------------------------------------------------------------------------

#[test]
fn single_player_cycle_through_the_facade_keeps_barriers_at_zero() {
    let mut universe = GameUniverse::new(started_state(false), LocalDispatcher);
    let turn_before = universe.state().turn_counter;

    // Human RED: a simulation tick, then End Turn → the computer GREEN.
    universe.tick();
    universe.end_local_turn(TeamId::RED);
    assert_eq!(
        universe.state().active_turn_team,
        TeamId::GREEN,
        "the computer turn started"
    );

    // Computer GREEN: a simulation tick, then its End Turn → the round closes and
    // the turn counter grows.
    universe.tick();
    universe.end_local_turn(TeamId::GREEN);

    assert_eq!(
        universe.state().turn_counter,
        turn_before + 1,
        "the round advanced the turn counter exactly once"
    );

    let single = universe.state().snapshot_bytes();

    // No barrier stepper was called: every barrier counter is still zero.
    let state = universe.state();
    assert_eq!(state.frame_sync_counter, [0; TEAM_SLOTS]);
    assert_eq!(state.turn_indices, [0; TEAM_SLOTS]);
    assert_eq!(state.next_turn_indices, [0; TEAM_SLOTS]);
    assert_eq!(state.team_data_crc16, [0; TEAM_SLOTS]);
    assert_eq!(state.begin_turn_barrier(), Some(0));
    assert_eq!(state.end_turn_barrier(), Some(0));
    assert_eq!(state.crc_barrier(), Some(0));

    // The barrier-free network-style run: the same commands through the raw state
    // and the same transition, without 00/52/45 and without the steppers.
    let mut network = started_state(false);
    network.tick();
    let _ = legacy_close(&mut network, TeamId::RED);
    network.tick();
    let _ = legacy_close(&mut network, TeamId::GREEN);

    assert_eq!(
        single,
        network.snapshot_bytes(),
        "single player and the barrier-free network run must be byte-identical"
    );
    assert_eq!(
        crc::state_digest(universe.state()),
        crc::state_digest(&network)
    );

    // Positive control: the barrier steppers do change the CRC-relevant counters,
    // so the equality above is not vacuous.
    let mut with_barriers = started_state(false);
    let _ = with_barriers.step_begin_turn_barrier(TeamId::RED);
    let _ = with_barriers.step_end_turn_barrier(TeamId::RED);
    let _ = with_barriers.step_crc_barrier(TeamId::RED);
    assert_ne!(
        with_barriers.snapshot_bytes(),
        single,
        "stepped barriers must be visible in the snapshot"
    );
}

// ---------------------------------------------------------------------------
// 4. Dispatcher::advances_turns — the policy is fail-closed
// ---------------------------------------------------------------------------

/// A dispatcher that declares nothing about the turn machine.
struct SilentDispatcher;

impl Dispatcher for SilentDispatcher {
    fn route(&mut self, cmd: Command) -> RoutedCommand {
        RoutedCommand::Local(cmd)
    }

    fn receive(&mut self, _cmd: RemoteCommand) -> RemoteHandling {
        RemoteHandling::Ignore
    }

    fn plan(&mut self, _facts: &BarrierFacts) -> Vec<NetworkCommand> {
        Vec::new()
    }
}

#[test]
fn advances_turns_is_a_fail_closed_policy() {
    assert!(
        LocalDispatcher.advances_turns(),
        "a local match owns the sequential machine"
    );
    assert!(
        !SilentDispatcher.advances_turns(),
        "the default must not start the machine"
    );
}

// ---------------------------------------------------------------------------
// 5. TurnNotice.status — only on the closing step that ends the match
// ---------------------------------------------------------------------------

#[test]
fn turn_notice_status_is_reported_only_on_game_over() {
    // `Duration` victory with the limit already reached: the closing step ends the
    // match (`check_end_game_conditions`), so `finish_turn` reports `GameOver`.
    let mut state = started_state(false);
    state.victory_type = VictoryType::Duration;
    state.victory_limit = 1;
    state.turn_counter = 1;
    assert_eq!(state.turn_counter, 1);

    let mut universe = GameUniverse::new(state, LocalDispatcher);
    let mut stream = universe.subscribe();

    universe.end_local_turn(TeamId::RED);
    let first = drain_turn_notices(&mut universe, &mut stream);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].event, TurnEvent::Computer(TeamId::GREEN));
    assert_eq!(
        first[0].status, None,
        "a non-closing step carries no status"
    );

    universe.end_local_turn(TeamId::GREEN);
    let closing = drain_turn_notices(&mut universe, &mut stream);

    assert_eq!(closing.len(), 1, "the closing step ends the match");
    assert_eq!(closing[0].event, TurnEvent::GameOver);
    let status = closing[0]
        .status
        .expect("the closing step carries the pre-finish_turn status");

    // `finish_turn` does not advance the counter and (for the duration limit) does
    // not mutate the teams before returning, so the same evaluation is repeatable.
    assert_eq!(
        status,
        victory::evaluate_status(universe.state(), universe.state().turn_counter),
        "the notice status must be the status evaluated before finish_turn"
    );
    assert_eq!(
        universe.state().turn_counter,
        1,
        "a finished match does not advance the counter"
    );
}

// ---------------------------------------------------------------------------
// 6. begin_local_round — the round entry with next_team inside the facade
// ---------------------------------------------------------------------------

#[test]
fn begin_local_round_matches_the_legacy_round_entry() {
    // The legacy entry: `begin_round` then the first `next_team`.
    let mut legacy = two_team_state(false);
    legacy.begin_round();
    let event = legacy.next_team();
    assert_eq!(event, TurnEvent::Human(TeamId::RED));

    let mut universe = GameUniverse::new(two_team_state(false), LocalDispatcher);
    let mut stream = universe.subscribe();

    universe.begin_local_round();

    assert_eq!(
        universe.state().snapshot_bytes(),
        legacy.snapshot_bytes(),
        "begin_local_round must be the same state as begin_round + next_team"
    );

    let notices = drain_turn_notices(&mut universe, &mut stream);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].event, TurnEvent::Human(TeamId::RED));
    assert_eq!(
        notices[0].previous_player_team,
        TeamId::RED,
        "the peek is the team of the state before the first step"
    );
    assert_eq!(notices[0].status, None);
}

#[test]
fn hot_seat_begin_turn_peek_survives_the_move_into_the_facade() {
    // Hot-seat: the first human is shown the window because the player team in the
    // configured state (RED here) differs from the started one only after a switch;
    // the facade reports `previous_player_team` so the application keeps comparing
    // exactly as before.
    let mut legacy = two_team_state(true);
    legacy.player_team = TeamId::ALIEN;
    legacy.begin_round();
    let _ = legacy.next_team();

    let mut state = two_team_state(true);
    state.player_team = TeamId::ALIEN;
    let mut universe = GameUniverse::new(state, LocalDispatcher);
    let mut stream = universe.subscribe();
    universe.begin_local_round();

    let notices = drain_turn_notices(&mut universe, &mut stream);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].event, TurnEvent::Human(TeamId::RED));
    assert_eq!(
        notices[0].previous_player_team,
        TeamId::ALIEN,
        "the hot-seat window decision reads the team before the switch"
    );

    // RED closes: the next human's notice carries RED as the previous team.
    universe.end_local_turn(TeamId::RED);
    let notices = drain_turn_notices(&mut universe, &mut stream);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].event, TurnEvent::Human(TeamId::GREEN));
    assert_eq!(notices[0].previous_player_team, TeamId::RED);

    assert_eq!(
        universe.state().snapshot_bytes(),
        {
            let _ = legacy_close(&mut legacy, TeamId::RED);
            legacy.snapshot_bytes()
        },
        "the hot-seat round entry and the first close stay byte-identical"
    );
}
