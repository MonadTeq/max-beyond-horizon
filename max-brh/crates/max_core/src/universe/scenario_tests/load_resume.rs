//! Gate test of the **load path** (ADR `game-universe-facade.md` §1 item 6):
//! `save → load → resume` must not apply `manage_economy` / `progress_build_state`
//! of the saved active team a second time.
//!
//! Oracle (`max-cpp-port/src/game_manager.cpp`):
//!
//! * `GameManager_TeamTurnTurnBased` starts its loop at
//!   `GameManager_ActiveTurnTeam` when the phase is `GAME_STATE_10_LOAD_GAME`
//!   (`:458`) — the slot is restored by `SaveLoad_Load` (`saveload.cpp:905-910`);
//! * `GameManager_ManageEconomy` is skipped for that phase (`:462-464`), and
//!   `GameManager_ProgressBuildState` is skipped in `GameManager_UpdateGui`
//!   (`:2450/2468/2484`): both were applied before the save;
//! * `finished_turn` of the started team is reset (`:472`), the active team and the
//!   phase are set (`:2411`, `:2417`), and exactly one team is started — the load
//!   branch breaks out of the loop (`:476-480`).
//!
//! The gates proved here:
//!
//! 1. **one economy pass** — resuming a human-active save leaves the loaded state
//!    byte-identical (a second `manage_economy` would charge the builder's raw
//!    storage again, so the control below is sensitive);
//! 2. **saved active team** — a save taken during GREEN's turn resumes GREEN, not
//!    `PLAYER_TEAM_RED`, and GREEN's economy is not re-applied either;
//! 3. **byte-stable save → load → load** and run-to-run determinism;
//! 4. **facade** — `GameUniverse::resume_local_round` starts one team, emits one
//!    `TurnNotice` with the pre-step `previous_player_team`, bumps `seq` once, and
//!    reports no team when the saved slot has no playing slot at/after it.
//!
//! The marker: a `CONSTRCT` with a `Build` order and raw storage. Every
//! `GameManager_ManageEconomy` pass runs `GameManager_UpdateProductions` →
//! `UnitInfo::UpdateProduction`, which charges `Cargo_GetRawConsumptionRate` from
//! the builder's storage (`unitinfo.cpp:2904-2915`, `game.rs:2086`), so the
//! storage value records how many economy passes the team went through.

use crate::enums::{
    unit_flags::{MOBILE_LAND_UNIT, SELECTABLE},
    GameStateType, PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType,
};
use crate::events::{apply_command, command_finished_turn};
use crate::geo::Point;
use crate::save::SaveFile;
use crate::team::{TeamId, TeamKind, TeamUnits};
use crate::universe::{EventStream, GameEvent, GameUniverse, LocalDispatcher, TurnNotice};
use crate::game::GameState;
use crate::{crc, TurnEvent};

/// Raw storage of a freshly created builder before any economy pass.
const BUILDER_STORAGE: i16 = 40;
/// `Cargo_GetRawConsumptionRate(CONSTRCT, build_rate = 1)` per turn
/// (`cargo.cpp`, `production::total_cost(Constructor, 10) == 20`).
const BUILDER_RAW_PER_TURN: i16 = 2;

/// Compares two byte blobs without dumping them on failure (a V70 file or a
/// snapshot is hundreds of kilobytes).
fn assert_same_bytes(actual: &[u8], expected: &[u8], what: &str) {
    if actual == expected {
        return;
    }

    let first = actual
        .iter()
        .zip(expected.iter())
        .position(|(a, b)| a != b)
        .map(|index| index.to_string())
        .unwrap_or_else(|| "no common prefix".to_string());

    panic!(
        "{what}: blobs differ (len {} vs {}, crc16 0x{:04X} vs 0x{:04X}, \
         first differing byte {first})",
        actual.len(),
        expected.len(),
        crc::crc16(actual),
        crc::crc16(expected)
    );
}

/// RED (human) + GREEN (computer), one tank and one builder on each side.
///
/// The builders are not needed for the match itself: they make the number of
/// economy passes observable in the state (`UnitInfo::storage`).
fn economy_state() -> GameState {
    let mut state = GameState::new();
    state.play_mode = PlayMode::TurnBased;
    state.network_game = false;
    state.game_state = GameStateType::InGame;
    state.turn_counter = 1;
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Computer;
    state.teams[2].team_type = TeamKind::None;
    state.teams[3].team_type = TeamKind::None;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[0].set_gold(100);
    state.team_units[1].set_gold(100);
    state.active_turn_team = TeamId::RED;
    state.player_team = TeamId::RED;

    for (team, tank_x, builder_x) in [(TeamId::RED, 10, 12), (TeamId::GREEN, 20, 22)] {
        state
            .spawn_unit(
                UnitType::Tank,
                team,
                Point::new(tank_x, 10),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .expect("tank spawns");
        let builder = state
            .spawn_unit(UnitType::Constructor, team, Point::new(builder_x, 10), 0)
            .expect("builder spawns");

        // A builder with a `Build` order and raw storage: `ManageEconomy` charges
        // `BUILDER_RAW_PER_TURN` per pass (`UnitInfo::UpdateProduction`).
        let mut unit = state.unit(builder).expect("builder").clone();
        unit.storage = BUILDER_STORAGE;
        unit.build_list = vec![UnitType::TrainingHall];
        unit.build_time = 10;
        unit.build_rate = 1;
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::ExecutingOrder;
        state.write_unit(builder, unit);
    }

    state
}

/// The builder slot of `team` (the only `CONSTRCT` of the scenario).
fn builder_of(state: &GameState, team: TeamId) -> crate::UnitId {
    state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::Constructor)
        })
        .expect("the scenario has one builder per team")
}

fn builder_storage(state: &GameState, team: TeamId) -> i16 {
    state
        .unit(builder_of(state, team))
        .expect("builder")
        .storage
}

/// Writes the state as a real V70 file and parses it back (the save/load path of
/// the application, without files).
fn save_and_load(state: &GameState) -> (Vec<u8>, GameState) {
    let bytes = state.to_save_bytes().expect("the state writes as V70");
    let save = SaveFile::parse(&bytes).expect("our V70 parses");
    (bytes, GameState::from_save(&save))
}

/// Starts RED (human) in a fresh round: one economy pass is applied.
fn started_state() -> GameState {
    let mut state = economy_state();
    state.begin_round();
    assert_eq!(state.next_team(), TurnEvent::Human(TeamId::RED));
    assert_eq!(
        builder_storage(&state, TeamId::RED),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN,
        "the normal round entry applies exactly one economy pass"
    );
    state
}

/// Closes RED's turn and starts GREEN (`06` + `EndTurn` + one machine step).
fn started_green_turn() -> GameState {
    let mut state = started_state();
    apply_command(&mut state, &command_finished_turn(TeamId::RED));
    state.enter_end_turn();
    assert_eq!(state.next_team(), TurnEvent::Computer(TeamId::GREEN));
    assert_eq!(
        builder_storage(&state, TeamId::GREEN),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN,
        "GREEN's turn starts with its economy pass"
    );
    state
}

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
// 1. Save → load → resume: the economy of the saved team is not re-applied
// ---------------------------------------------------------------------------

#[test]
fn load_resume_does_not_apply_the_economy_a_second_time() {
    let started = started_state();
    let (bytes, mut loaded) = save_and_load(&started);

    // The save was written after the single economy pass: the file carries the
    // already charged storage.
    assert_eq!(
        builder_storage(&loaded, TeamId::RED),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN,
        "the saved state already contains the economy pass"
    );

    let before_resume = loaded.snapshot_bytes();
    assert_eq!(
        loaded.resume_round(),
        Some(TurnEvent::Human(TeamId::RED)),
        "the load entry starts the saved active team"
    );

    // Gate: the resume is exactly the C++ turn start without the start-of-turn
    // economy — no byte of the loaded state changes.
    assert_same_bytes(
        &loaded.snapshot_bytes(),
        &before_resume,
        "resume changed the loaded state: manage_economy/progress_build_state ran again",
    );
    assert_eq!(
        builder_storage(&loaded, TeamId::RED),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN,
        "the builder paid raw material a second time"
    );

    // Positive control: the marker is sensitive — one more economy pass charges
    // the builder and changes the state (that is the regression this gate covers).
    let mut control = loaded.clone();
    control.manage_economy(TeamId::RED);
    assert_eq!(
        builder_storage(&control, TeamId::RED),
        BUILDER_STORAGE - 2 * BUILDER_RAW_PER_TURN,
        "a repeated manage_economy must be visible in the builder storage"
    );
    assert!(
        control.snapshot_bytes() != before_resume,
        "a repeated manage_economy must change the snapshot"
    );

    // `load_state` itself stays the only normalisation of the load path.
    let parsed = SaveFile::parse(&bytes).expect("parses");
    assert_same_bytes(
        &GameState::from_save(&parsed).snapshot_bytes(),
        &before_resume,
        "load_state must produce the saved state",
    );
}

// ---------------------------------------------------------------------------
// 2. The resume slot is the saved active team, not PLAYER_TEAM_RED
// ---------------------------------------------------------------------------

#[test]
fn load_resume_starts_the_saved_active_team() {
    let started = started_green_turn();
    let (bytes, mut loaded) = save_and_load(&started);
    assert_eq!(loaded.active_turn_team, TeamId::GREEN);
    assert_eq!(loaded.player_team, TeamId::RED);

    // Both teams carry their single economy pass from before the save.
    assert_eq!(
        builder_storage(&loaded, TeamId::RED),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN
    );
    assert_eq!(
        builder_storage(&loaded, TeamId::GREEN),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN
    );

    assert_eq!(
        loaded.resume_round(),
        Some(TurnEvent::Computer(TeamId::GREEN)),
        "the loaded match resumes at GameManager_ActiveTurnTeam, not at PLAYER_TEAM_RED"
    );
    assert_eq!(loaded.active_turn_team, TeamId::GREEN);
    assert_eq!(
        loaded.round_team,
        usize::from(TeamId::GREEN.0) + 1,
        "the round cursor continues after the resumed slot"
    );
    assert!(!loaded.teams[TeamId::GREEN.0 as usize].finished_turn);
    assert_eq!(
        builder_storage(&loaded, TeamId::GREEN),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN,
        "GREEN's economy must not run a second time on resume"
    );
    assert_eq!(
        builder_storage(&loaded, TeamId::RED),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN,
        "RED's economy must not run again either"
    );

    // The only C++-mandated mutation of the computer resume is the turn-start
    // step itself: `finished_turn = false`, the active team and `Ai_BeginTurn`
    // (`game_manager.cpp:472`, `:2411`, `:2478`) — no economy, no build progress.
    let mut reference = GameState::from_save(&SaveFile::parse(&bytes).expect("parses"));
    reference.teams[TeamId::GREEN.0 as usize].finished_turn = false;
    reference.active_turn_team = TeamId::GREEN;
    reference.round_team = usize::from(TeamId::GREEN.0) + 1;
    reference.ai_begin_turn(TeamId::GREEN);
    assert_same_bytes(
        &loaded.snapshot_bytes(),
        &reference.snapshot_bytes(),
        "resume must be the C++ turn-start step only",
    );
}

// ---------------------------------------------------------------------------
// 3. Save → load → load is byte-stable; the resume is deterministic
// ---------------------------------------------------------------------------

#[test]
fn save_load_load_is_byte_stable_and_deterministic() {
    let started = started_state();
    let (bytes, loaded) = save_and_load(&started);

    // Load of the written file regenerates the same file.
    assert_same_bytes(
        &loaded.to_save_bytes().expect("writes"),
        &bytes,
        "save → load → save must be byte-identical",
    );

    // Resume, then write again: the resumed match has the same bytes.
    let mut first = loaded.clone();
    first.resume_round();
    let bytes_after_resume = first.to_save_bytes().expect("writes after resume");
    assert_same_bytes(
        &bytes_after_resume,
        &bytes,
        "the resume must not change what is saved",
    );

    // Load → resume is deterministic: a second run produces the same state.
    let second_save = SaveFile::parse(&bytes_after_resume).expect("parses");
    let mut second = GameState::from_save(&second_save);
    second.resume_round();
    assert_same_bytes(
        &first.snapshot_bytes(),
        &second.snapshot_bytes(),
        "two load → resume runs must produce the same state",
    );
    assert_eq!(
        crc::state_digest(&first),
        crc::state_digest(&second),
        "the resumed state must stay deterministic"
    );

    // And the loaded file itself is stable when loaded twice.
    let third = GameState::from_save(&SaveFile::parse(&bytes).expect("parses"));
    assert_same_bytes(
        &third.snapshot_bytes(),
        &loaded.snapshot_bytes(),
        "loading the same file twice must be deterministic",
    );
}

// ---------------------------------------------------------------------------
// 4. Facade: one step, one notice, one version bump
// ---------------------------------------------------------------------------

#[test]
fn facade_resume_local_round_is_a_single_turn_start() {
    let started = started_state();
    let (_bytes, loaded) = save_and_load(&started);
    let loaded_snapshot = loaded.snapshot_bytes();

    let mut universe = GameUniverse::new(loaded, LocalDispatcher);
    let mut stream = universe.subscribe();
    let seq_before = universe.seq();

    assert!(
        universe.resume_local_round(),
        "the facade reports the started team"
    );
    assert_eq!(
        universe.seq(),
        seq_before + 1,
        "one atomic facade operation — one version bump"
    );
    assert_eq!((&universe.state).active_turn_team, TeamId::RED);
    assert_eq!(
        builder_storage(&universe.state, TeamId::RED),
        BUILDER_STORAGE - BUILDER_RAW_PER_TURN,
        "the facade path does not re-apply the economy either"
    );
    assert_same_bytes(
        &(&universe.state).snapshot_bytes(),
        &loaded_snapshot,
        "the facade resume must leave the loaded state untouched",
    );

    let notices = drain_turn_notices(&mut universe, &mut stream);
    assert_eq!(notices.len(), 1, "exactly one team is started");
    assert_eq!(notices[0].event, TurnEvent::Human(TeamId::RED));
    assert_eq!(
        notices[0].previous_player_team,
        TeamId::RED,
        "GameManager_PlayerTeam is peeked before the step"
    );
    assert_eq!(
        notices[0].status, None,
        "the load entry never closes a round, so there is no finish_turn status"
    );

    // No round was closed and the machine is not run further: the next turn step
    // would be GREEN (as in C++, which breaks out after the first team).
    assert_eq!((&universe.state).turn_counter, 1);
    assert!(!(&universe.state).teams[1].finished_turn);
}

#[test]
fn facade_resume_returns_false_when_no_playing_team_is_left() {
    // The saved active slot is GRAY, which is `TEAM_TYPE_NONE`; the cursor has no
    // playing team at or after it. C++ never enters the loop body then.
    let mut state = economy_state();
    state.active_turn_team = TeamId::GRAY;
    let (_bytes, loaded) = save_and_load(&state);

    let mut universe = GameUniverse::new(loaded, LocalDispatcher);
    let seq_before = universe.seq();
    assert!(
        !universe.resume_local_round(),
        "no team left at or after the saved active slot"
    );
    assert_eq!(
        universe.seq(),
        seq_before,
        "nothing changed — no version bump"
    );
    assert_eq!((&universe.state).active_turn_team, TeamId::GRAY);
}
