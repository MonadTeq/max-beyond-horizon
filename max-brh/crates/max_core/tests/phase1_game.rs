//! Phase 1 integration: the match state machine (`game.rs`).
//!
//! Oracles: `max-cpp-port/src/game_manager.cpp`
//! (`GameManager_TeamTurnTurnBased`, `GameManager_TeamTurnConcurrent`,
//! `GameManager_TeamTurnFinish`, `GameManager_AreTeamsFinishedTurn`),
//! `remote.cpp` (`Remote_WaitBeginTurnAcknowledge`, `Remote_ServerWaitBarrier`)
//! and `max-brh-docs/state-model.md` §3.
//!
//! Scenario shape: given a `GameState` → the team-turn machine
//! ([`GameState::begin_round`]/[`GameState::next_team`]/[`GameState::end_round`])
//! → ticks → packet 06 / barriers → expected event, phase, team flags, counters
//! and digest. The hot-seat harness uses the same machine as
//! `max_app::local` (the application layer only consumes [`TurnEvent`] and never
//! writes simulation fields itself).

use max_core::crc;
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{GameStateType, PlayMode, TeamClan, UnitType};
use max_core::events;
use max_core::game::{GameState, TurnEvent, PLAYER_TEAM_COUNT};
use max_core::geo::Point;
use max_core::input::{Command, Packet00TurnBarrier, Packet45CrcBarrier, Packet52EndTurnBarrier};
use max_core::team::{TeamId, TeamKind, TeamUnits};

const TANK_FLAGS: u32 = MOBILE_LAND_UNIT | SELECTABLE;

/// A two-human duel with one tank per side (turn-based, local semantics).
fn duel() -> GameState {
    let mut state = GameState::new();
    state.network_game = false;
    state.play_mode = PlayMode::TurnBased;

    for team in [TeamId::RED, TeamId::GREEN] {
        let index = team.0 as usize;
        state.teams[index].team_type = TeamKind::Player;
        state.team_units[index] = TeamUnits::with_clan(TeamClan::Random);
    }

    state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), TANK_FLAGS)
        .expect("red tank");
    state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(20, 10),
            TANK_FLAGS,
        )
        .expect("green tank");

    state
}

/// The next team turn selected by the core (`GameManager_TeamTurnTurnBased`):
/// the machine resets `finished_turn`, runs the economy and sets the active team.
fn start_team_turn(state: &mut GameState) -> TeamId {
    match state.next_team() {
        TurnEvent::Human(team) | TurnEvent::Computer(team) => {
            assert_eq!(state.active_turn_team, team, "active team of {team:?}");
            assert!(
                !state.teams[team.0 as usize].finished_turn,
                "the turn start resets finished_turn of {team:?}"
            );
            team
        }
        other => panic!("a team turn was expected, got {other:?}"),
    }
}

/// Ends a team's turn by packet 06 (as `App::local_end_turn` / the AI close
/// path) and forces the end-of-turn phase the round close requires.
fn close_team_turn(state: &mut GameState, team: TeamId) {
    state.enqueue(events::command_finished_turn(team));
    state.tick();
    assert!(
        state.teams[team.0 as usize].finished_turn,
        "packet 06 marks {team:?} finished"
    );

    if state.play_mode == PlayMode::TurnBased {
        // `apply_packet_06`: in `TurnBased` packet 06 closes the team turn.
        assert_eq!(
            state.game_state,
            GameStateType::EndTurn,
            "packet 06 must close a turn-based team turn"
        );
    }

    // `GameManager_RefreshOrders`: the turn is over, the phase is
    // `GAME_STATE_9_END_TURN` (the sequential `SimultaneousMoves` path forces it).
    state.enter_end_turn();
}

/// One team turn: the machine picks the slot, the team ticks and closes the turn.
fn play_team_turn(state: &mut GameState, ticks: usize) -> TeamId {
    let team = start_team_turn(state);

    for _ in 0..ticks {
        state.tick();
    }

    close_team_turn(state, team);
    team
}

/// A full round through the core machine: `begin_round` → every playing team
/// → `end_round` (`GameManager_TeamTurnFinish`). Returns `true` when the match
/// is over.
fn play_round(state: &mut GameState, ticks_per_team: usize) -> bool {
    state.begin_round();

    loop {
        match state.next_team() {
            TurnEvent::Human(team) | TurnEvent::Computer(team) => {
                for _ in 0..ticks_per_team {
                    state.tick();
                }
                close_team_turn(state, team);
            }
            TurnEvent::RoundClosed => return false,
            TurnEvent::GameOver => return true,
        }
    }
}

/// A round is closed only after the last playing team finished; the turn
/// counter advances once per round and every `finished_turn` flag is reset.
#[test]
fn hot_seat_round_closes_after_both_humans_finish() {
    let mut state = duel();

    let red = play_team_turn(&mut state, 2);
    assert_eq!(red, TeamId::RED, "the round starts at PLAYER_TEAM_RED");
    assert!(state.teams[TeamId::RED.0 as usize].finished_turn);
    assert!(!state.teams[TeamId::GREEN.0 as usize].finished_turn);
    assert!(
        !state.are_teams_finished_turn(),
        "the green human has not finished yet"
    );
    assert_eq!(state.turn_counter, 0, "the round is still open");
    assert_eq!(state.active_turn_team, TeamId::RED);
    assert_eq!(state.round_team, TeamId::GREEN.0 as usize, "cursor after RED");

    // The hot-seat layer moves to the next human team.
    let green = play_team_turn(&mut state, 2);
    assert_eq!(green, TeamId::GREEN);
    assert!(state.are_teams_finished_turn());
    assert_eq!(state.turn_counter, 0, "the turn advances only on close");
    assert_eq!(
        state.round_team,
        TeamId::GREEN.0 as usize + 1,
        "the cursor follows the last started team"
    );

    // `GameManager_TeamTurnFinish` through the machine: the game is not over.
    assert!(!state.end_round(), "the game is not over");
    assert_eq!(state.turn_counter, 1);
    assert_eq!(state.game_state, GameStateType::InGame);
    assert_eq!(state.round_team, 0, "the next round starts at RED");
    for team in 0..PLAYER_TEAM_COUNT {
        assert!(!state.teams[team].finished_turn, "team {team}");
    }
}

/// The machine skips `None`/`Eliminated` slots, starts the computer team with
/// `Ai_BeginTurn` and closes the round through `next_team` after the last one.
#[test]
fn turn_machine_visits_only_playing_teams() {
    let mut state = GameState::new();
    state.play_mode = PlayMode::TurnBased;
    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::None;
    state.teams[TeamId::BLUE.0 as usize].team_type = TeamKind::Computer;
    state.teams[TeamId::GRAY.0 as usize].team_type = TeamKind::Eliminated;
    for team in [TeamId::RED, TeamId::BLUE] {
        state.team_units[team.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
    }
    state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), TANK_FLAGS)
        .expect("red tank");
    state
        .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(30, 10), TANK_FLAGS)
        .expect("blue tank");

    state.begin_round();
    assert_eq!(state.upcoming_team(), Some(TeamId::RED));

    assert_eq!(state.next_team(), TurnEvent::Human(TeamId::RED));
    assert_eq!(state.active_turn_team, TeamId::RED);
    assert_eq!(state.game_state, GameStateType::InGame);
    assert_eq!(state.round_team, TeamId::GREEN.0 as usize);
    close_team_turn(&mut state, TeamId::RED);

    // GREEN is `None`: the cursor moves to the computer BLUE.
    assert_eq!(state.upcoming_team(), Some(TeamId::BLUE));
    assert_eq!(state.next_team(), TurnEvent::Computer(TeamId::BLUE));
    assert_eq!(state.active_turn_team, TeamId::BLUE);
    assert_eq!(state.round_team, TeamId::GRAY.0 as usize);
    close_team_turn(&mut state, TeamId::BLUE);

    // GRAY is eliminated: there is nothing left to play in this round.
    assert_eq!(state.upcoming_team(), None);
    assert_eq!(state.next_team(), TurnEvent::RoundClosed);
    assert_eq!(state.turn_counter, 1);
    assert_eq!(state.round_team, 0, "the next round starts at RED");
}

/// Two identical hot-seat rounds are bit-identical: snapshot, packet-45 CRC,
/// canonical digest and replay digest all agree (D).
#[test]
fn full_round_is_bit_identical_across_runs() {
    fn run() -> GameState {
        let mut state = duel();
        assert!(!play_round(&mut state, 3));
        assert!(!play_round(&mut state, 3));
        state
    }

    let first = run();
    let second = run();

    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(crc::state_digest(&first), crc::state_digest(&second));
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
    assert_eq!(crc::state_checksum(&first), crc::state_checksum(&second));
    assert_eq!(first.turn_counter, 2);
    assert_ne!(crc::state_digest(&first), crc::CRC16_INITIAL);
}

/// A peaceful round changes the turn/phase fields (digest) but no unit field,
/// so the packet-45 unit-list CRC stays stable (D). The CRC is then reported by
/// both teams through packet 45 and forms the network barrier.
#[test]
fn peaceful_round_keeps_the_unit_list_checksum() {
    let mut state = duel();
    state.network_game = true;

    let checksum_before = crc::state_checksum(&state);
    let digest_before = crc::state_digest(&state);

    assert!(!play_round(&mut state, 2));

    assert_eq!(
        crc::state_checksum(&state),
        checksum_before,
        "no unit was moved, damaged or produced"
    );
    assert_ne!(
        crc::state_digest(&state),
        digest_before,
        "the turn counter is part of the canonical digest"
    );

    // Both teams report the same CRC and next-turn index: the 45 barrier closes.
    for team in [TeamId::RED, TeamId::GREEN] {
        state.enqueue(Command::CrcBarrier(Packet45CrcBarrier {
            team,
            next_turn_index: 1,
            crc16: checksum_before,
        }));
    }
    state.tick();

    assert_eq!(state.crc_barrier(), Some(1));
    for team in [TeamId::RED, TeamId::GREEN] {
        assert_eq!(
            state.team_data_crc16[team.0 as usize], checksum_before,
            "team {team:?} reports a diverged CRC"
        );
    }
}

/// In `SimultaneousMoves` the round closes through the concurrent barrier
/// (`GameManager_AreTeamsFinishedTurn`): with a playing team still unfinished the
/// machine re-enters it instead of closing the round.
#[test]
fn simultaneous_round_waits_for_every_team() {
    let mut state = duel();
    state.play_mode = PlayMode::SimultaneousMoves;

    state.begin_round();
    let team = start_team_turn(&mut state);
    assert_eq!(team, TeamId::RED);
    close_team_turn(&mut state, team);

    // GREEN is still playing, but the cursor already ran past the last slot
    // (e.g. the round was interrupted): the barrier, not the cursor, decides.
    state.round_team = PLAYER_TEAM_COUNT;
    assert!(!state.are_teams_finished_turn());
    assert_eq!(state.next_team(), TurnEvent::Human(TeamId::GREEN));
    assert_eq!(state.round_team, TeamId::GREEN.0 as usize + 1);
    close_team_turn(&mut state, TeamId::GREEN);

    assert!(state.are_teams_finished_turn());
    assert_eq!(state.next_team(), TurnEvent::RoundClosed);
    assert_eq!(state.turn_counter, 1);
}

/// A network round is gated by packet 00 (begin turn) and 52 (end turn): the
/// barrier value is available only when every playing team reports it.
#[test]
fn network_round_gates_on_begin_and_end_barriers() {
    let mut state = GameState::new();
    state.network_game = true;
    state.play_mode = PlayMode::TurnBased;

    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::Remote;
    state.teams[TeamId::BLUE.0 as usize].team_type = TeamKind::Remote;
    state.team_units[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Random);

    // Only two of the three teams acknowledged the round.
    state.enqueue(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::RED,
        counter: 1,
    }));
    state.enqueue(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::GREEN,
        counter: 1,
    }));
    state.tick();
    assert_eq!(state.begin_turn_barrier(), None, "blue is still missing");
    assert!(
        !state.begin_turn_barrier_reached(1),
        "blue has not reported the 00 counter yet"
    );

    state.enqueue(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::BLUE,
        counter: 1,
    }));
    state.tick();
    assert_eq!(state.begin_turn_barrier(), Some(1));
    assert!(
        state.begin_turn_barrier_reached(1),
        "every remote team reported the 00 counter"
    );

    // End-of-turn barrier: a diverged counter never completes.
    state.enqueue(events::command_end_turn_barrier(TeamId::RED, 2));
    state.enqueue(events::command_end_turn_barrier(TeamId::GREEN, 2));
    state.tick();
    assert_eq!(state.end_turn_barrier(), None);

    state.enqueue(Command::EndTurnBarrier(Packet52EndTurnBarrier {
        team: TeamId::BLUE,
        counter: 2,
    }));
    state.tick();
    assert_eq!(state.end_turn_barrier(), Some(2));
    assert!(state.end_turn_barrier_reached(2));
}

/// The barrier steppers are the own-team pre-increment of the C++
/// `++Remote_*[team]`: the first `step_*` report is 1, the counters live in the
/// state (and in the canonical digest), and `*_barrier_reached` waits only for
/// `TEAM_TYPE_REMOTE` peers.
#[test]
fn barrier_steppers_advance_the_own_team_counters() {
    let mut state = GameState::new();
    state.network_game = true;
    state.play_mode = PlayMode::TurnBased;
    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::Remote;
    state.teams[TeamId::BLUE.0 as usize].team_type = TeamKind::Remote;
    state.teams[TeamId::GRAY.0 as usize].team_type = TeamKind::None;

    // 00 — `Remote_WaitBeginTurnAcknowledge`.
    assert_eq!(state.step_begin_turn_barrier(TeamId::RED), 1);
    assert_eq!(state.step_begin_turn_barrier(TeamId::RED), 2);
    assert_eq!(state.frame_sync_counter[TeamId::RED.0 as usize], 2);
    assert!(
        !state.begin_turn_barrier_reached(2),
        "green and blue have not reported the 00 counter yet"
    );
    state.frame_sync_counter[TeamId::GREEN.0 as usize] = 2;
    assert!(!state.begin_turn_barrier_reached(2), "blue is still missing");
    state.frame_sync_counter[TeamId::BLUE.0 as usize] = 2;
    assert!(
        state.begin_turn_barrier_reached(2),
        "a `None` team does not take part in the barrier"
    );

    // 52 — `Remote_WaitEndTurnAcknowledge`.
    assert_eq!(state.step_end_turn_barrier(TeamId::RED), 1);
    assert_eq!(state.turn_indices[TeamId::RED.0 as usize], 1);
    assert!(!state.end_turn_barrier_reached(1));
    state.turn_indices[TeamId::GREEN.0 as usize] = 1;
    state.turn_indices[TeamId::BLUE.0 as usize] = 1;
    assert!(state.end_turn_barrier_reached(1));

    // 45 — `Remote_CheckDesync`.
    assert_eq!(state.step_crc_barrier(TeamId::RED), 1);
    assert_eq!(state.next_turn_indices[TeamId::RED.0 as usize], 1);
    assert!(!state.crc_barrier_reached(1));
    state.next_turn_indices[TeamId::GREEN.0 as usize] = 1;
    state.next_turn_indices[TeamId::BLUE.0 as usize] = 1;
    assert!(state.crc_barrier_reached(1));

    // The counters wrap like the C++ `uint8_t` and stay in the digest.
    state.turn_indices[TeamId::RED.0 as usize] = u8::MAX;
    assert_eq!(state.step_end_turn_barrier(TeamId::RED), 0);
    assert_eq!(
        state.next_turn_indices[TeamId::RED.0 as usize],
        1,
        "the steppers touch their own counter only"
    );
}

/// `end_round` is `GameManager_TeamTurnFinish`: when the match is over the turn
/// counter is not advanced and no next round is entered.
#[test]
fn end_round_reports_the_end_of_the_match() {
    let mut state = duel();
    // `VICTORY_TYPE_DURATION`: the limit is reached already at turn 0.
    state.victory_limit = 0;

    state.begin_round();
    for _ in 0..2 {
        let team = start_team_turn(&mut state);
        close_team_turn(&mut state, team);
    }

    assert_eq!(
        state.next_team(),
        TurnEvent::GameOver,
        "the closing step reports the end of the match"
    );
    assert_eq!(
        state.turn_counter, 0,
        "the counter is not advanced when the match is over"
    );
    assert_ne!(state.round_team, 0, "the next round is not entered");
}

/// `finish_turn` is guarded by the phase: a live duel in `InGame` (or any other
/// phase) keeps its counter; only `EndTurn` closes the round.
#[test]
fn finish_turn_is_phase_guarded_in_a_live_duel() {
    let mut state = duel();
    let units_before = state.unit_ids_in_order();

    state.game_state = GameStateType::InGame;
    assert!(!state.finish_turn());
    state.game_state = GameStateType::TurnActive;
    assert!(!state.finish_turn());

    assert_eq!(state.turn_counter, 0);
    assert_eq!(state.unit_ids_in_order(), units_before);

    state.game_state = GameStateType::EndTurn;
    assert!(!state.finish_turn());
    assert_eq!(state.turn_counter, 1);
    assert_eq!(state.unit_ids_in_order(), units_before);
}
