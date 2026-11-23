//! Phase 1 integration: victory, defeat and the end of the match (`victory.rs`).
//!
//! Oracle: `max-cpp-port/src/winloss.cpp` (`WinLoss_EvaluateStatus`),
//! `menu.cpp` (`menu_check_end_game_conditions`), `assets/PATCHES.RES`
//! (`SC_L0002` — the generic custom-game script) and
//! `max-brh-docs/save.md` (*Victory and Loss Conditions*).
//!
//! Scenario shape: created game → rounds (packets 06 + `finish_turn`) → the
//! `Duration`/`Score`/elimination condition → expected result and unaffected
//! phase/counter. Determinism (D) is asserted with the canonical digests.

use max_core::crc;
use max_core::deploy::{deploy_starting_units, DEFAULT_START_GOLD};
use max_core::enums::{GameStateType, Surface, VictoryType, WinLossState};
use max_core::events;
use max_core::game::{GameState, PLAYER_TEAM_COUNT};
use max_core::geo::{Point, MAP_TILES};
use max_core::map::SurfaceMap;
use max_core::setup::{
    generate_map, AlienUnitCosts, GameSetup, GenerationSettings, ResourceLevel, TeamStart,
};
use max_core::team::{TeamId, TeamKind};
use max_core::victory;
use max_core::Randomizer;

const SEED: u32 = 0x00E7_AC01;

/// A generated two-human custom game with both bases landed.
fn new_game() -> GameState {
    let surface = SurfaceMap::from_tiles([Surface::LAND; MAP_TILES]);
    let mut setup = GameSetup {
        planet: max_core::enums::Planet::Snowcrab,
        settings: GenerationSettings::from_menu_levels(
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::LOW,
        ),
        teams: vec![
            TeamStart::new(TeamId::RED, Point::new(20, 20)),
            TeamStart::new(TeamId::GREEN, Point::new(90, 20)),
        ],
        alien_costs: AlienUnitCosts {
            alien_tank: 4,
            juggernaut: 8,
            alien_assault_gun: 5,
            alien_plane: 6,
        },
    };

    let mut rng = Randomizer::new(SEED);
    let output = generate_map(&mut setup, &surface, &mut rng);

    let mut state = GameState::with_setup(
        output.map,
        [
            TeamKind::Player,
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None,
        ],
    );
    state.network_game = false;
    state.rng = Randomizer::new(SEED);
    // `GameManager_InitUnitsAndGameState` (`game_manager.cpp:3035`): the first
    // turn is number 1 (called from `GameManager_GameSetup` before
    // `GameManager_InitGame`, which does not touch the counter).
    state.turn_counter = 1;
    state.mission_category = victory::MissionCategory::Custom;
    state.mission_index = 0;

    deploy_starting_units(
        &mut state,
        DEFAULT_START_GOLD,
        setup.settings.max_resources,
        &setup.teams,
    );

    state
}

/// One round: every playing team gets economy + ticks and sends packet 06; the
/// round is closed by `finish_turn` (`GameManager_TeamTurnFinish`).
fn play_round(state: &mut GameState, ticks_per_team: usize) -> bool {
    for team_index in 0..PLAYER_TEAM_COUNT {
        let kind = state.teams[team_index].team_type;
        if kind == TeamKind::None || kind == TeamKind::Eliminated {
            continue;
        }

        let team = TeamId(team_index as u8);
        state.teams[team_index].finished_turn = false;
        state.manage_economy(team);
        state.active_turn_team = team;
        state.game_state = GameStateType::InGame;

        for _ in 0..ticks_per_team {
            state.tick();
        }

        state.enqueue(events::command_finished_turn(team));
        state.tick();
    }

    state.finish_turn()
}

/// `VICTORY_TYPE_DURATION`: the limit is inclusive (`limit <= turn_counter`);
/// the game ends when the counter reaches the limit and is not advanced further.
#[test]
fn duration_limit_ends_the_game_on_the_limit_turn() {
    fn run() -> GameState {
        let mut state = new_game();
        state.victory_type = VictoryType::Duration;
        state.victory_limit = 2;

        assert_eq!(state.turn_counter, 1);
        assert!(!play_round(&mut state, 1), "turn 1 is below the limit");
        assert_eq!(state.turn_counter, 2);
        assert!(play_round(&mut state, 1), "turn 2 reaches the limit");
        assert_eq!(
            state.turn_counter, 2,
            "a finished game keeps the limit turn"
        );
        assert_eq!(state.game_state, GameStateType::InGame);

        state
    }

    let first = run();
    let second = run();
    assert_eq!(first.turn_counter, 2);
    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(crc::state_digest(&first), crc::state_digest(&second));
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
}

/// `VICTORY_TYPE_SCORE`: any team reaching the limit ends the game; one point
/// below the limit does not (`team_points >= victory_limit`).
#[test]
fn score_limit_is_inclusive_per_team() {
    let mut state = new_game();
    state.victory_type = VictoryType::Score;
    state.victory_limit = 10;

    assert!(!play_round(&mut state, 1), "no points yet");

    state.teams[TeamId::RED.0 as usize].team_points = 9;
    assert!(!play_round(&mut state, 1), "9 points are below the limit");

    // The condition is evaluated for every team, not only for the local player.
    state.teams[TeamId::GREEN.0 as usize].team_points = 10;
    assert!(play_round(&mut state, 1), "10 points reach the limit");

    let status = victory::evaluate_status(&state, state.turn_counter);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Generic
    );
    assert_eq!(
        status.team_status[TeamId::GREEN.0 as usize],
        WinLossState::Generic
    );
}

/// `VICTORY_TYPE_SCORE` is evaluated for every team, not only for the local
/// player: an AI team reaching the limit ends the match while the local human
/// is alive and below the limit.
#[test]
fn score_limit_ends_the_game_for_a_non_local_ai_team() {
    let mut state = new_game();
    state.player_team = TeamId::RED;
    state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::Computer;
    state.victory_type = VictoryType::Score;
    state.victory_limit = 10;

    assert!(!play_round(&mut state, 1), "no points yet");

    state.teams[TeamId::RED.0 as usize].team_points = 3;
    assert!(
        !play_round(&mut state, 1),
        "3 points of the local player are below the limit"
    );

    let turn = state.turn_counter;
    state.teams[TeamId::GREEN.0 as usize].team_points = 10;
    assert!(
        play_round(&mut state, 1),
        "an AI team at the limit ends the match"
    );
    assert_eq!(
        state.turn_counter, turn,
        "a finished game keeps the last turn"
    );

    let status = victory::evaluate_status(&state, state.turn_counter);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Generic,
        "the local player has not won or lost"
    );
    assert_eq!(
        status.team_status[TeamId::GREEN.0 as usize],
        WinLossState::Generic
    );
    assert_eq!(
        status.teams_in_play, 2,
        "the local human is still in play when the AI ends the match"
    );
}

/// Regression: with every slot `TeamKind::None` the C++ loop in
/// `menu_check_end_game_conditions` never enters its body, so the match is not
/// over and `finish_turn` advances the turn counter normally.
#[test]
fn finish_turn_without_playing_teams_advances_the_counter() {
    let mut state = new_game();
    for team in 0..PLAYER_TEAM_COUNT {
        state.teams[team].team_type = TeamKind::None;
    }
    state.game_state = GameStateType::EndTurn;
    let turn = state.turn_counter;

    assert!(!state.finish_turn(), "no playing team can end the match");
    assert_eq!(state.turn_counter, turn + 1);
    assert_eq!(state.game_state, GameStateType::InGame);
    assert!(!victory::check_end_game_conditions(&mut state));
}

/// Destroying every opponent unit ends the match through the generic script
/// (`teams_in_play == 1`); the surviving team ranks first.
#[test]
fn losing_all_units_loses_the_generic_match() {
    let mut state = new_game();
    let green_units: Vec<_> = state
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::GREEN)
        })
        .collect();
    assert!(!green_units.is_empty());

    for id in green_units {
        state.destroy_unit(id);
    }

    let status = victory::evaluate_status(&state, state.turn_counter);
    assert_eq!(status.teams_in_play, 1);
    assert_eq!(
        status.team_status[TeamId::GREEN.0 as usize],
        WinLossState::Lost
    );
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Generic
    );
    assert_eq!(status.team_rank[TeamId::RED.0 as usize], 1);

    assert!(victory::check_end_game_conditions(&mut state));
    // `finish_turn` also reports the end and does not advance the turn counter.
    state.game_state = GameStateType::EndTurn;
    let turn = state.turn_counter;
    assert!(state.finish_turn());
    assert_eq!(state.turn_counter, turn);
}

/// Both teams without units: `teams_in_play == 0` ends the game even though
/// nobody survived.
#[test]
fn no_team_in_play_ends_the_match() {
    let mut state = new_game();

    for id in state.unit_ids_in_order() {
        state.destroy_unit(id);
    }
    assert_eq!(state.unit_count(), 0);

    let status = victory::evaluate_status(&state, state.turn_counter);
    assert_eq!(status.teams_total, 2);
    assert_eq!(status.teams_in_play, 0);
    assert!(victory::check_end_game_conditions(&mut state));
}

/// While the game continues, `check_end_game_conditions` is a pure observation:
/// the canonical state (and the packet-45 CRC) are unchanged (D).
#[test]
fn victory_check_does_not_change_a_running_game() {
    let mut state = new_game();
    state.victory_type = VictoryType::Duration;
    state.victory_limit = 50;
    play_round(&mut state, 1);

    let snapshot = state.snapshot_bytes();
    let checksum = crc::state_checksum(&state);

    for _ in 0..3 {
        assert!(!victory::check_end_game_conditions(&mut state));
        assert_eq!(state.snapshot_bytes(), snapshot);
        assert_eq!(crc::state_checksum(&state), checksum);
    }
}
