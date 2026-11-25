//! Campaign mission load: the AI opponent keeps its economy and acts.
//!
//! Regression for the mission blocker: `App::start_local_game_from_save`
//! (`GAME_STATE_10_LOAD_GAME`, `game_manager.cpp:1929-1961`) used to run
//! `Ai_Init` + `Ai_SetupStrategy` after the V70 load. `Ai_SetupStrategy`
//! (`ai.cpp:116`) is a *new game* step (`GameManager_InitGame`,
//! `game_manager.cpp:3098`): it re-rolls the team clan and calls
//! `ResourceManager_InitClanUnitValues` (`aiplayer.cpp:1186`), which clears the
//! loaded team complexes (`resource_manager.cpp:1291`). The mission's AI then had
//! no building managers/resources: its builders did not progress and no units
//! were produced.
//!
//! C++ order on a load is `SaveLoad_Load` → `Ai_FileLoad` (`saveload.cpp:1046`):
//! `Ai_Init` empties the task queue (task lists are not serialized) and leaves
//! `need_init` set, the saved `AiPlayer` fields and `TeamUnits` stay, and the
//! first `Ai_BeginTurn` rebuilds the tasks (`aiplayer.cpp:2242`).
//!
//! Gates:
//! 1. after the load the computer teams keep the file's complexes and clan, and
//!    every unit's complex reference resolves;
//! 2. the loaded team has `need_init` set and an empty task queue;
//! 3. the first computer turn rebuilds the tasks (`need_init` cleared), moves the
//!    AI's units and keeps the complexes;
//! 4. with its economy intact the AI produces units within a few turns — the
//!    old path stalled at the loaded unit count.
//!
//! Fixture: `max_core/tests/data/save2_v70.cam` — campaign 2 ("Heart of the
//! Matter"), RED human, GREEN and BLUE computer, each with a saved complex.

use std::path::{Path, PathBuf};

use max_app::local::{LocalPhase, LocalSetup};
use max_app::{App, AppConfig};
use max_core::enums::{PlayMode, VictoryType};
use max_core::geo::Point;
use max_core::save::SaveFile;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;

/// `max-original/SAVE2.CAM`.
const SAVE2: &[u8] = include_bytes!("../../max_core/tests/data/save2_v70.cam");

/// Computer teams of the fixture (`save.header.team_type`).
fn computer_teams() -> Vec<TeamId> {
    let save = SaveFile::parse(SAVE2).expect("save2 parses as V70");
    (0..4)
        .filter(|team| save.teams[*team].team_type == TeamKind::Computer)
        .map(|team| TeamId(team as u8))
        .collect()
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn mission_setup() -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
        world: 8,
        opponent: 3,
        timer: 0,
        endturn: 0,
        play_mode: PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        team_clans: [max_core::enums::TeamClan::Random; 4],
        hot_seat: false,
        seed: 7,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

/// Application with real assets: the mission load needs the game data (as the
/// `local_sp`/`mission_start` suites do). `None` — the assets are not checked
/// out (the test is skipped instead of reporting a false failure).
fn mission_app() -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        eprintln!("skipping: max-original/MAX.RES is not available");
        return None;
    }

    let mut app = App::new(AppConfig::default());
    let config = AppConfig::default()
        .with_data_dir(data)
        .with_assets_dir(repo_path("assets"));
    app.config_mut().max_res = config.max_res;
    app.config_mut().patches_res = config.patches_res;

    Some(app)
}

fn start_mission(app: &mut App) {
    app.start_local_game_from_save(&mission_setup(), SAVE2)
        .expect("SAVE2.CAM loads as a local match");
}

/// The loaded match is turn-active and the human (RED) owns the current turn.
fn assert_mission_started(app: &App) {
    assert!(app.local_active(), "no local match after the mission load");
    assert_eq!(
        app.state_ref().player_team(),
        TeamId::RED,
        "the mission's human team"
    );
    assert_eq!(
        app.local().expect("local match").phase,
        LocalPhase::PlayerTurn,
        "the loaded match opens on the human turn"
    );
}

fn run_until_player_turn(app: &mut App, limit: usize) -> bool {
    for _ in 0..limit {
        match app.local().map(|local| local.phase) {
            Some(LocalPhase::PlayerTurn) => return true,
            Some(LocalPhase::Finished) | None => return false,
            _ => {}
        }

        app.local_update(0.0);
    }

    app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn)
}

fn ai_unit_positions(app: &App) -> Vec<(UnitId, Point)> {
    let teams = computer_teams();

    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| {
            let unit = app.state_ref().unit(id)?;
            teams.contains(&unit.team).then_some((id, unit.grid))
        })
        .collect()
}

fn ai_unit_count(app: &App) -> usize {
    ai_unit_positions(app).len()
}

// ---------------------------------------------------------------------------
// 1. The load keeps the AI economy and leaves the tasks to the first turn
// ---------------------------------------------------------------------------

#[test]
fn mission_load_keeps_the_ai_complexes_and_clan() {
    let Some(mut app) = mission_app() else {
        return;
    };
    let save = SaveFile::parse(SAVE2).expect("save2 parses as V70");

    start_mission(&mut app);
    assert_mission_started(&app);

    let teams = computer_teams();
    assert_eq!(teams.len(), 2, "the fixture has two computer teams");

    for team in teams {
        assert!(
            app.state_ref().ai().players[team.0 as usize].need_init,
            "{team:?}: task lists are not serialized — the first `Ai_BeginTurn` \
             rebuilds them (`Ai_FileLoad` → `Ai_Init`)"
        );
        assert!(
            app.state_ref().ai().players[team.0 as usize].tasks_pending,
            "{team:?}: `Ai_Init` marks the team as having work"
        );

        let saved_complexes = save.team_units[team.0 as usize].complexes.len();
        assert!(saved_complexes > 0, "{team:?}: fixture complex");
        assert_eq!(
            app.state_ref().team_units()[team.0 as usize]
                .complexes()
                .len(),
            saved_complexes,
            "{team:?}: `Ai_SetupStrategy` must not run on a load — it would clear \
             the loaded complexes via `ResourceManager_InitClanUnitValues`"
        );
        assert_eq!(
            app.state_ref().teams()[team.0 as usize].team_clan,
            save.teams[team.0 as usize].team_clan,
            "{team:?}: the clan comes from the save header"
        );
    }

    // No dangling complex references: every AI unit's `complex` exists in its
    // team's list (the old path cleared the lists and left the units pointing at
    // ids that no longer existed).
    for id in app.state_ref().unit_ids_in_order() {
        let Some(unit) = app.state_ref().unit(id) else {
            continue;
        };

        if unit.team == TeamId::ALIEN {
            continue;
        }

        let Some(complex) = unit.complex else {
            continue;
        };

        assert!(
            app.state_ref().team_units()[unit.team.0 as usize]
                .complexes()
                .iter()
                .any(|entry| entry.id == complex.0 as i16),
            "{id:?} references complex {complex:?} missing from team {:?}",
            unit.team
        );
    }

    assert_eq!(
        app.state_ref().ai().task_manager.task_count(),
        0,
        "the task queue is not part of V70: it starts empty"
    );
}

// ---------------------------------------------------------------------------
// 2. The first computer turn rebuilds the tasks and moves the AI
// ---------------------------------------------------------------------------

#[test]
fn mission_load_ai_turn_rebuilds_tasks_and_moves_units() {
    let Some(mut app) = mission_app() else {
        return;
    };

    start_mission(&mut app);
    assert_mission_started(&app);

    let before = ai_unit_positions(&app);
    assert!(!before.is_empty(), "the fixture has AI units");

    app.local_end_turn();
    assert!(
        run_until_player_turn(&mut app, 40_000),
        "the computer turn must finish and hand the turn back"
    );

    let teams = computer_teams();
    for team in &teams {
        assert!(
            !app.state_ref().ai().players[team.0 as usize].need_init,
            "{team:?}: `Ai_BeginTurn` must have run its `need_init` block"
        );
    }

    assert!(
        app.state_ref().ai().task_manager.task_count() > 0,
        "the first AI turn creates the mission's tasks \
         (`TaskCheckAssaults`/`TaskManageBuildings`/`TaskSurvey`/…) — an empty \
         queue is the 'AI does nothing' state"
    );

    let moved = before
        .iter()
        .filter(|(id, grid)| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.grid != *grid)
        })
        .count();
    assert!(
        moved > 0,
        "the AI must move its units on the first loaded turn (moved {moved} of {})",
        before.len()
    );

    // The complexes survive the AI turn (economy is not rebuilt/cleared).
    for team in teams {
        assert!(
            !app.state_ref().team_units()[team.0 as usize]
                .complexes()
                .is_empty(),
            "{team:?}: the complexes must stay after the loaded AI turn"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. The loaded AI produces units — the broken path stalled forever
// ---------------------------------------------------------------------------

#[test]
fn mission_load_ai_produces_units_after_load() {
    let Some(mut app) = mission_app() else {
        return;
    };

    start_mission(&mut app);
    assert_mission_started(&app);

    let units_before = ai_unit_count(&app);
    assert!(units_before > 0, "the fixture has AI units");

    // The first computer turn runs the mission's initialised AI (surveys,
    // construction managers, production). With the old load path the builders
    // had no complexes and the army count never grew.
    for round in 0..4 {
        assert!(
            app.local().expect("local match").phase == LocalPhase::PlayerTurn,
            "round {round}: the human turn did not return"
        );

        app.local_end_turn();
        assert!(
            run_until_player_turn(&mut app, 40_000),
            "round {round}: the computer turn must finish"
        );
    }

    let units_after = ai_unit_count(&app);
    assert!(
        units_after > units_before,
        "the loaded mission's AI must produce units with its restored economy: \
         {units_before} AI units before, {units_after} after four turns"
    );
}
