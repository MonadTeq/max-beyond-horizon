//! Camera across turn changes (manual-run defect 1): the view must **not** be
//! re-centred when a new turn starts.
//!
//! Oracle: `GameManager_UpdateGui` (`max-cpp-port/src/game_manager.cpp:2407-2492`)
//! does not touch the camera on a normal turn start — it calls
//! `GameManager_UpdatePanelButtons` only for `GAME_STATE_10_LOAD_GAME` (`:2415-2419`).
//! For a human team change the camera comes from the team record
//! (`GameManager_UpdateGuiControl` → `UpdatePanelButtons`, `:2312` →
//! `GameManager_UpdateMainMapView(MAP_VIEW_CENTER, team_info->camera_position)`,
//! `:3750`), which `GameManager_GuiSwitchTeam` (`:5220-5240`) filled when the team
//! handed the turn over (`:492`). The port used to call `focus_player()` on every
//! human turn start, so the view jumped back to the mine each round.

use std::path::{Path, PathBuf};

use max_app::local::{LocalPhase, LocalSetup};
use max_app::{App, AppConfig};
use max_core::enums::PlayMode;
use max_core::team::{TeamId, TeamKind};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn local_app() -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        .with_mode(max_app::AppMode::Ui);

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

/// Match settings; `endturn = 0` closes the AI turn by the oracle counter fallback,
/// so a test round does not have to spin through a full 45 s window.
fn setup(team_types: [TeamKind; 4], hot_seat: bool) -> LocalSetup {
    LocalSetup {
        player_name: "Commander".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: if hot_seat {
            PlayMode::TurnBased
        } else {
            PlayMode::SimultaneousMoves
        },
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: max_core::enums::VictoryType::Duration,
        victory_limit: 50,
        team_types,
        team_clans: [max_core::enums::TeamClan::Random; 4],
        hot_seat,
        seed: 7,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

/// Confirms a hot-seat «Begin turn» window.
fn acknowledge(app: &mut App) {
    if app.local_notice().is_some() {
        assert!(app.local_acknowledge_turn());
    }
}

/// Runs frames until the human turn of `team` starts again.
fn run_until_human_turn(app: &mut App, team: TeamId) {
    for _ in 0..20_000 {
        if matches!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::PlayerTurn)
        ) && app.state_ref().player_team() == team
        {
            return;
        }

        app.local_update(1.0 / 60.0);
    }

    panic!("the human turn of {team:?} did not start within the frame budget");
}

/// One human: the camera stays exactly where the player left it after End Turn,
/// the AI turn and the return of control.
#[test]
fn a_single_human_keeps_the_camera_across_the_ai_turn() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));
    app.start_local_game(&setup(
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        false,
    ))
    .expect("local match");

    // A place far from the mine: any re-centring on `GameManager_GetFirstRelevantUnit`
    // would move the view.
    app.viewport_mut().center_on_grid(40, 30);
    let expected = app.viewport().center();

    app.local_end_turn();
    assert_ne!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn),
        "End Turn closes the human turn"
    );

    run_until_human_turn(&mut app, TeamId::RED);
    acknowledge(&mut app);

    assert_eq!(
        app.viewport().center(),
        expected,
        "the camera must not be reset at the start of a turn"
    );
}

/// Two humans: each one returns to their own view — `CTInfo::camera_position`
/// stored by `GameManager_GuiSwitchTeam` and restored on the next turn.
#[test]
fn hot_seat_restores_each_players_own_camera() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));
    app.start_local_game(&setup(
        [TeamKind::Player, TeamKind::Player, TeamKind::None, TeamKind::None],
        true,
    ))
    .expect("hot-seat match");

    acknowledge(&mut app);

    app.viewport_mut().center_on_grid(30, 25);
    let red_view = app.viewport().center();

    app.local_end_turn();
    acknowledge(&mut app);
    assert_eq!(
        app.state_ref().player_team(),
        TeamId::GREEN,
        "the turn passed to the second human"
    );

    app.viewport_mut().center_on_grid(50, 40);
    let green_view = app.viewport().center();

    app.local_end_turn();
    acknowledge(&mut app);
    assert_eq!(app.state_ref().player_team(), TeamId::RED);
    assert_eq!(
        app.viewport().center(),
        red_view,
        "RED returns to the view it left"
    );

    app.local_end_turn();
    acknowledge(&mut app);
    assert_eq!(app.state_ref().player_team(), TeamId::GREEN);
    assert_eq!(
        app.viewport().center(),
        green_view,
        "GREEN returns to the view it left"
    );
}

/// A human with an AI team in between: the view is stored when the human hands the
/// turn over (before the AI acts), not after the AI window.
#[test]
fn a_human_view_survives_an_ai_turn_in_between() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));
    app.start_local_game(&setup(
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::Player,
            TeamKind::None,
        ],
        true,
    ))
    .expect("hot-seat match with an AI team");

    acknowledge(&mut app);
    app.viewport_mut().center_on_grid(35, 28);
    let red_view = app.viewport().center();

    app.local_end_turn();
    // GREEN (AI) plays itself, then BLUE (human) gets the window.
    run_until_human_turn(&mut app, TeamId::BLUE);
    acknowledge(&mut app);

    app.viewport_mut().center_on_grid(60, 45);
    app.local_end_turn();

    // GREEN again, then RED — whose camera was stored before the AI turn.
    run_until_human_turn(&mut app, TeamId::RED);
    acknowledge(&mut app);

    assert_eq!(
        app.viewport().center(),
        red_view,
        "the RED view is the one stored at the RED turn end"
    );
}
