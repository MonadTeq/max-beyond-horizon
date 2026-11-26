//! Inspection during the computer turn (manual-run defect 2).
//!
//! Oracle: the active team gates **orders**, not information.
//! `GameManager_UpdateSelection` (`max-cpp-port/src/game_manager.cpp:6292-6348`)
//! resolves a foreign unit through `GameManager_UnitSelectOther` (`:6295-6298`)
//! before the `TEAM_TYPE_PLAYER` check (`:6300-6302`), and an own unit still goes
//! through `GameManager_UnitSelect` (`:6339-6342`) — no command menu opens while the
//! computer moves (`:6253-6258`). The right button has no active-team gate at all
//! (`:6747-6776`): `UnitStats_Menu` on the selected unit, otherwise select/inspect.
//! The port blocked every map click while `LocalGame::input_blocked()` was true,
//! including these read-only actions.

use std::path::{Path, PathBuf};

use max_app::hud;
use max_app::local::{LocalPhase, LocalSetup};
use max_app::{handle_session_input, App, AppConfig, InputEvent, Key};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::UnitType;
use max_render::layout::MAIN_MAP;

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

fn setup() -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 45,
        play_mode: max_core::enums::PlayMode::SimultaneousMoves,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: max_core::enums::VictoryType::Duration,
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

/// Screen point of a cell in the 640×480 window (`MAIN_MAP` offset included).
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

fn click(app: &mut App, point: (i32, i32)) {
    let _ = handle_session_input(app, InputEvent::MouseClick(point.0, point.1));
}

fn rclick(app: &mut App, point: (i32, i32)) {
    let _ = handle_session_input(app, InputEvent::RightClick(point.0, point.1));
}

/// Starts a match and hands the turn to the computer.
fn ai_turn_app() -> Option<App> {
    let mut app = local_app()?;
    app.start_local_game(&setup()).expect("local match");
    app.local_update(0.0);
    app.local_end_turn();
    app.local_update(1.0 / 60.0);

    assert!(
        matches!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::AiTurn(TeamId::GREEN))
        ),
        "the computer turn is active"
    );

    Some(app)
}

/// A unit of `team` of the given type.
fn find_unit(app: &App, team: TeamId, unit_type: UnitType) -> Option<UnitId> {
    app.state_ref().unit_ids_in_order().into_iter().find(|id| {
        app.state_ref()
            .unit(*id)
            .is_some_and(|unit| unit.team == team && unit.unit_type == unit_type)
    })
}

/// The whole information surface stays available during the AI turn: selecting
/// own units and buildings, opening `UnitStats_Menu` with the right button and
/// inspecting a spotted enemy — while no order is issued.
#[test]
fn the_player_can_inspect_units_and_buildings_during_the_ai_turn() {
    let mut app = ai_turn_app().expect(concat!("test fixture is missing: ", stringify!(ai_turn_app())));
    let team = app.player_team();

    let surveyor = find_unit(&app, team, UnitType::Surveyor).expect("Surveyor");
    let surveyor_grid = app.state_ref().unit(surveyor).expect("Surveyor").grid;
    let mine = find_unit(&app, team, UnitType::MiningStation).expect("mine");
    let mine_grid = app.state_ref().unit(mine).expect("mine").grid;

    let camera = app.viewport().center();
    let orders_before = app.state_ref().unit(surveyor).expect("Surveyor").orders;

    // LMB on an own unit: selected, its status is shown, no order is pending.
    let surveyor_point = screen_point(&app, surveyor_grid);
    click(&mut app, surveyor_point);
    assert_eq!(app.hud().selected, Some(surveyor), "own unit is selected");
    assert!(
        app.hud().pending.is_empty(),
        "inspection must not queue orders"
    );
    assert_eq!(
        app.state_ref().unit(surveyor).expect("Surveyor").orders,
        orders_before,
        "the unit order did not change"
    );
    assert_eq!(app.viewport().center(), camera, "the camera did not move");

    // RMB on the selected unit: `UnitStats_Menu` (the oracle opens it at any time).
    rclick(&mut app, surveyor_point);
    assert!(
        app.unit_stats().is_some(),
        "right click opens the full statistics window"
    );
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Escape));
    assert!(app.unit_stats().is_none());
    assert_eq!(app.hud().selected, Some(surveyor));

    // LMB on an own building: the same inspection path.
    let mine_point = screen_point(&app, mine_grid);
    click(&mut app, mine_point);
    assert_eq!(app.hud().selected, Some(mine), "own building is selected");
    assert!(app.hud().pending.is_empty());

    rclick(&mut app, mine_point);
    assert!(
        app.unit_stats().is_some(),
        "building statistics open with the right button"
    );
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Escape));

    // A left click on an empty cell does not give the selected unit an order.
    click(&mut app, surveyor_point);
    let empty_point = screen_point(&app, Point::new(surveyor_grid.x + 1, surveyor_grid.y));
    click(&mut app, empty_point);
    assert!(
        app.hud().pending.is_empty(),
        "a map order click is ignored during the AI turn"
    );
    assert_eq!(
        app.state_ref().unit(surveyor).expect("Surveyor").orders,
        orders_before,
        "no order reached the simulation"
    );

    // End Turn is disabled while the computer moves (`GameManager_MenuClickEndTurnButton`).
    click(&mut app, (426, 12));
    assert!(
        matches!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::AiTurn(TeamId::GREEN))
        ),
        "the AI turn is still running"
    );
}

/// A spotted enemy unit is selected and inspected during the AI turn
/// (`GameManager_UnitSelectOther`, `game_manager.cpp:6295-6298`), and no order is
/// given to it (a selected enemy is an inspection target, not an order source).
#[test]
fn a_spotted_enemy_is_inspected_during_the_ai_turn() {
    let mut app = ai_turn_app().expect(concat!("test fixture is missing: ", stringify!(ai_turn_app())));

    // Make the whole map visible (`AiPlayer::all_visible`) and place an enemy tank
    // two cells from the RED Surveyor.
    app.universe_mut().ai_mut().all_visible = true;
    app.universe_mut().update_visibility_status(false);

    let surveyor = find_unit(&app, app.player_team(), UnitType::Surveyor).expect("Surveyor");
    let anchor = app.state_ref().unit(surveyor).expect("Surveyor").grid;
    let flags = max_core::access::unit_type_flags(UnitType::Tank);
    let enemy = app
        .universe_mut()
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(anchor.x + 2, anchor.y),
            flags,
        )
        .expect("enemy tank");

    app.viewport_mut().center_on_grid(anchor.x + 1, anchor.y);
    let point = screen_point(&app, Point::new(anchor.x + 2, anchor.y));

    click(&mut app, point);
    assert_eq!(
        app.hud().selected,
        Some(enemy),
        "the enemy is selected for inspection"
    );
    assert!(app.hud().pending.is_empty(), "no order to a foreign unit");

    rclick(&mut app, point);
    assert!(
        app.unit_stats().is_some(),
        "the enemy statistics window opens"
    );
}
