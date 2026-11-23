//! Phase 1 integration: the match state machine (`game.rs`).
//!
//! Oracles: `max-cpp-port/src/game_manager.cpp`
//! (`GameManager_TeamTurnTurnBased`, `GameManager_TeamTurnFinish`,
//! `GameManager_AreTeamsFinishedTurn`), `remote.cpp`
//! (`Remote_WaitBeginTurnAcknowledge`, `Remote_ServerWaitBarrier`) and
//! `max-brh-docs/state-model.md` §3.
//!
//! Scenario shape: given a `GameState` → team turn (economy + orders) → ticks →
//! packet 06 / barriers → expected phase, team flags, counters and digest.
//! The hot-seat harness mirrors `max_app::local::LocalGame::local_next_team` /
//! `local_end_turn` (the layer that owns team switching; `max_core` only marks
//! `finished_turn` and the phase).

use max_core::crc;
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{GameStateType, PlayMode, TeamClan, UnitType};
use max_core::events;
use max_core::game::{GameState, PLAYER_TEAM_COUNT};
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

/// One team turn: `GameManager_TeamTurnTurnBased` (`manage_economy` → active
/// team → ticks) and the human end of turn (packet 06 through the command
/// queue, as `LocalGame::local_end_turn`).
fn play_team_turn(state: &mut GameState, team: TeamId, ticks: usize) {
    let index = team.0 as usize;
    state.teams[index].finished_turn = false;
    state.manage_economy(team);
    state.active_turn_team = team;
    state.game_state = GameStateType::InGame;

    for _ in 0..ticks {
        state.tick();
    }

    state.enqueue(events::command_finished_turn(team));
    state.tick();
    assert_eq!(
        state.game_state,
        GameStateType::EndTurn,
        "packet 06 must close a turn-based team turn"
    );
}

/// A full hot-seat round: every playing team acts, then the round is closed by
/// `GameManager_TeamTurnFinish` (`finish_turn`).
fn play_round(state: &mut GameState, ticks_per_team: usize) -> bool {
    for team_index in 0..PLAYER_TEAM_COUNT {
        let kind = state.teams[team_index].team_type;
        if kind == TeamKind::None || kind == TeamKind::Eliminated {
            continue;
        }
        play_team_turn(state, TeamId(team_index as u8), ticks_per_team);
    }

    state.finish_turn()
}

/// A round is closed only after the last playing team finished; the turn
/// counter advances once per round and every `finished_turn` flag is reset.
#[test]
fn hot_seat_round_closes_after_both_humans_finish() {
    let mut state = duel();

    play_team_turn(&mut state, TeamId::RED, 2);
    assert!(state.teams[TeamId::RED.0 as usize].finished_turn);
    assert!(!state.teams[TeamId::GREEN.0 as usize].finished_turn);
    assert!(
        !state.are_teams_finished_turn(),
        "the green human has not finished yet"
    );
    assert_eq!(state.turn_counter, 0, "the round is still open");
    assert_eq!(state.active_turn_team, TeamId::RED);

    // The hot-seat layer moves to the next human team.
    play_team_turn(&mut state, TeamId::GREEN, 2);
    assert!(state.are_teams_finished_turn());
    assert_eq!(state.turn_counter, 0, "the turn advances only on close");

    assert!(!state.finish_turn(), "the game is not over");
    assert_eq!(state.turn_counter, 1);
    assert_eq!(state.game_state, GameStateType::InGame);
    for team in 0..PLAYER_TEAM_COUNT {
        assert!(!state.teams[team].finished_turn, "team {team}");
    }
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

    state.enqueue(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::BLUE,
        counter: 1,
    }));
    state.tick();
    assert_eq!(state.begin_turn_barrier(), Some(1));

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
