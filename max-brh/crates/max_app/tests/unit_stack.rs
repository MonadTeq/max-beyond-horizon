//! Stacked units on one cell — an aircraft above a ground unit, several aircraft
//! on the same cell. `MapHash` keeps every unit of a cell in one ordered list,
//! air and ground alike (`MapHash::AddEx`, `hash.cpp:153-193`), so a click has to
//! walk that list.
//!
//! A repeated LMB on the already selected unit moves the focus **down** the cell
//! stack, cyclically: C++ `GameManager_ClickUnit` (`game_manager.cpp:6265-6290`)
//! → `Access_GetSelectableUnit` (`access.cpp:1131-1151`) →
//! `Access_GetActiveUnitWithFlags` (`access.cpp:1492-1531`). The oracle rotates the
//! selected unit to the back of the cell list on every such click; the port does
//! the same walk **without** mutating the simulation `MapHash` (the rotation would
//! enter `snapshot_bytes`/CRC) — see `max_app::hud::next_stacked_unit` and
//! `max_app::hud::unit_at_screen_preferring_selected`.
//!
//! The tests use real frames (`Assets`/`.WRL`), like `unit_menu.rs`: the command
//! menu that a repeated click opens first is part of the 1:1 chrome.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::setup::GameSetupForm;
use max_app::{handle_session_input, App, AppConfig, InputEvent, Transition};
use max_core::enums::{Surface, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_render::layout::MAIN_MAP;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// One human RED team, the rest — AI/empty.
fn single_human() -> LocalSetup {
    let mut setup = LocalSetup::default();
    setup.world = 0;
    setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    setup
}

fn app_with_assets() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("tests/fixtures/assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("unit_stack: нет данных — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"));
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
}

/// A match through the real start phases: mobilization → landing → `InGame`.
fn game_app() -> Option<App> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() {
        eprintln!("unit_stack: нет данных — нет SNOW_1.WRL");
        return None;
    }

    let mut flow = PreGameFlow::start(GameSetupForm::default(), single_human(), &world_dir);
    assert!(matches!(flow.stage, PreGameStage::Mobilize(_)));
    flow.commit_mobilization();
    assert!(matches!(flow.stage, PreGameStage::Landing(_)));
    flow.commit_landing();

    let config = flow.setup.new_game_config();
    let surface =
        max_app::landing::LandingScreen::load_surface(0, &world_dir).expect("WRL-поверхность");

    let mut app = app_with_assets()?;
    app.new_game_on_surface(&config, &surface);
    app.tick();

    Some(app)
}

/// A free LAND cell next to the RED mine (the camera is put on it).
fn free_land_grid(app: &App) -> Point {
    let land = Surface::LAND.bits();

    for radius in 1..8 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(22 + dx, 22 + dy);

                if app.state_ref().units_at(grid).is_empty()
                    && app.state_ref().map().surface_at(grid.x, grid.y).bits() & land != 0
                {
                    return grid;
                }
            }
        }
    }

    Point::new(23, 23)
}

/// A RED unit in `grid` (`UnitInfo::flags` from the `SC_UNITS` table).
fn spawn(app: &mut App, unit_type: UnitType, grid: Point) -> UnitId {
    let flags = max_core::access::unit_type_flags(unit_type);

    app.universe_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("юнит появился")
}

/// Screen point of the cell center (640×480 frame with chrome).
fn cell_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

fn click(app: &mut App, point: (i32, i32)) -> Transition {
    handle_session_input(app, InputEvent::MouseClick(point.0, point.1))
}

/// The click on the selected unit: the popup opens first (C++
/// `GameManager_InitPopupButtons`, `game_manager.cpp:6277-6281`), the next click
/// closes it and walks the stack (`Access_GetSelectableUnit`, `:6283-6288`).
fn open_menu(app: &mut App, point: (i32, i32), unit: UnitId) {
    click(app, point);
    assert_eq!(
        app.unit_menu().map(|menu| menu.unit),
        Some(unit),
        "повторный клик открывает меню выделенного юнита"
    );
}

#[test]
fn three_unit_cell_walks_down_the_stack_and_wraps() {
    let Some(mut app) = game_app() else {
        return;
    };

    let grid = free_land_grid(&app);
    let tank = spawn(&mut app, UnitType::Tank, grid);
    let bomber = spawn(&mut app, UnitType::Bomber, grid);
    let fighter = spawn(&mut app, UnitType::Fighter, grid);
    app.viewport_mut().center_on_grid(grid.x, grid.y);

    // `MapHash::AddEx`: a new unit goes to the front of the cell list
    // (`hash.cpp:170-174`), so the newest unit is the top of the stack.
    assert_eq!(
        app.state_ref().units_at(grid),
        &[fighter, bomber, tank],
        "порядок списка клетки — как в оракуле (новый юнит первым)"
    );

    let point = cell_point(&app, grid);
    let snapshot = app.state_ref().snapshot_bytes();

    // 1. The first click selects the top of the stack.
    click(&mut app, point);
    assert_eq!(app.hud().selected, Some(fighter));
    assert!(app.unit_menu().is_none(), "первый клик только выделяет");

    // 2. The repeated click: the command menu takes the click.
    open_menu(&mut app, point, fighter);

    // 3. The click that closes the popup also moves the focus one step down
    //    (`Access_GetSelectableUnit`).
    click(&mut app, point);
    assert!(app.unit_menu().is_none(), "меню закрыто повторным кликом");
    assert_eq!(app.hud().selected, Some(bomber), "фокус шагнул вниз по стеку");

    open_menu(&mut app, point, bomber);
    click(&mut app, point);
    assert_eq!(app.hud().selected, Some(tank), "дно стека");

    open_menu(&mut app, point, tank);
    // 4. The bottom of the stack wraps around to the top.
    click(&mut app, point);
    assert_eq!(app.hud().selected, Some(fighter), "обход по кругу");
    assert!(app.unit_menu().is_none());

    // Selection is presentation: the walk leaves the snapshot/CRC untouched.
    assert_eq!(
        app.state_ref().snapshot_bytes(),
        snapshot,
        "листание стека — выбор, а не состояние симуляции"
    );
}

#[test]
fn aircraft_above_a_ground_unit_swaps_the_focus() {
    let Some(mut app) = game_app() else {
        return;
    };

    let grid = free_land_grid(&app);
    let tank = spawn(&mut app, UnitType::Tank, grid);
    let fighter = spawn(&mut app, UnitType::Fighter, grid);
    app.viewport_mut().center_on_grid(grid.x, grid.y);

    let point = cell_point(&app, grid);

    // The aircraft is the newest unit of the cell — the click takes it first.
    click(&mut app, point);
    assert_eq!(app.hud().selected, Some(fighter));

    open_menu(&mut app, point, fighter);
    click(&mut app, point);
    assert_eq!(
        app.hud().selected,
        Some(tank),
        "фокус уходит с самолёта на юнит под ним"
    );

    open_menu(&mut app, point, tank);
    click(&mut app, point);
    assert_eq!(app.hud().selected, Some(fighter), "и возвращается на самолёт");
}

#[test]
fn ground_cover_unit_is_skipped_by_the_walk() {
    let Some(mut app) = game_app() else {
        return;
    };

    let grid = free_land_grid(&app);
    let tank = spawn(&mut app, UnitType::Tank, grid);
    // `GROUND_COVER` and not `SELECTABLE`: `Access_GetActiveUnitWithFlags` skips it
    // (`access.cpp:1504`).
    let road = spawn(&mut app, UnitType::Road, grid);
    let fighter = spawn(&mut app, UnitType::Fighter, grid);
    app.viewport_mut().center_on_grid(grid.x, grid.y);

    assert_eq!(app.state_ref().units_at(grid), &[fighter, road, tank]);

    let point = cell_point(&app, grid);
    click(&mut app, point);
    assert_eq!(app.hud().selected, Some(fighter));

    open_menu(&mut app, point, fighter);
    click(&mut app, point);
    assert_eq!(
        app.hud().selected,
        Some(tank),
        "дорожное покрытие пропускается, фокус идёт к танку"
    );
}

#[test]
fn a_lone_unit_keeps_the_focus_when_the_menu_closes() {
    let Some(mut app) = game_app() else {
        return;
    };

    let grid = free_land_grid(&app);
    let tank = spawn(&mut app, UnitType::Tank, grid);
    app.viewport_mut().center_on_grid(grid.x, grid.y);

    let point = cell_point(&app, grid);
    click(&mut app, point);
    assert_eq!(app.hud().selected, Some(tank));

    // No other selectable unit at the cell: `Access_GetSelectableUnit` returns the
    // unit itself, the focus stays (`access.cpp:1144-1147`).
    open_menu(&mut app, point, tank);
    click(&mut app, point);
    assert!(app.unit_menu().is_none());
    assert_eq!(app.hud().selected, Some(tank), "выделение не сбрасывается");
}

/// The walk itself, without the UI: the cyclic ring of the cell list.
#[test]
fn next_stacked_unit_follows_the_cell_list_cyclically() {
    let mut universe = max_core::universe::GameUniverse::default();
    let cell = Point::new(10, 10);
    let flags = max_core::enums::unit_flags::MOBILE_LAND_UNIT
        | max_core::enums::unit_flags::SELECTABLE;

    let first = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, cell, flags)
        .expect("first");
    let second = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, cell, flags)
        .expect("second");
    let third = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, cell, flags)
        .expect("third");

    // Newest first: `[third, second, first]`.
    let state = universe.snapshot();
    assert_eq!(state.units_at(cell), &[third, second, first]);

    assert_eq!(
        max_app::hud::next_stacked_unit(state, third, cell),
        Some(second)
    );
    assert_eq!(
        max_app::hud::next_stacked_unit(state, second, cell),
        Some(first)
    );
    assert_eq!(
        max_app::hud::next_stacked_unit(state, first, cell),
        Some(third),
        "после дна стека — снова верх"
    );

    // A lone unit has no next one.
    let lone = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 10), flags)
        .expect("lone");
    let state = universe.snapshot();
    assert_eq!(
        max_app::hud::next_stacked_unit(state, lone, Point::new(20, 10)),
        None
    );
}
