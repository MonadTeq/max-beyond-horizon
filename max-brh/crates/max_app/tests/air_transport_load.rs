//! Air transport loading — the defect «air transport does not load a unit».
//!
//! The whole chain goes through the player's path:
//!
//! 1. the `load` item of the unit menu — cursor mode 7 (`CURSOR_LOAD`,
//!    `UnitsManager_Popup_OnClick_Load`, `game_manager.cpp:6807`);
//! 2. a click on the passenger — the **transport** gets `ORDER_MOVE/INIT` to the
//!    passenger cell with `parent = passenger` (`game_manager.cpp:6814-6818`);
//! 3. the arrival — `Paths_FinishMove` (`airpath.cpp:408-456`): `ORDER_LOAD/INIT`
//!    (`units_manager.cpp:3092`), `ProgressLoading` (`units_manager.cpp:4131`) puts the
//!    passenger into the hold and `ProcessLoading` (`unitinfo.cpp:5161`) ends the order;
//! 4. after boarding, the transport resumes `ORDER_AWAIT` and its `storage` counts the
//!    passenger.
//!
//! Oracle: `max-cpp-port` (`game_manager.cpp`, `airpath.cpp`, `units_manager.cpp`,
//! `unitinfo.cpp`).

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::setup::GameSetupForm;
use max_app::unitmenu::{self, ItemId};
use max_app::{handle_session_input, App, AppConfig, InputEvent};
use max_core::enums::{UnitOrder, UnitType};
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

/// A match through the real start phases: mobilization → landing → `InGame`.
///
/// `PATCHES.RES` may come from the repository fallbacks (`assets::PATCHES_RES_FALLBACKS`),
/// so only a completely missing game data set skips the check.
fn game_app() -> Option<App> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() || !world_dir.join("MAX.RES").is_file() {
        eprintln!("air_transport_load: нет данных — нет данных игры рядом с репозиторием");
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

    let mut app = App::new(
        AppConfig::default()
            .with_data_dir(repo_path("max-original"))
            .with_assets_dir(repo_path("tests/fixtures/assets")),
    );
    if app.init_assets().is_err() {
        eprintln!("air_transport_load: нет данных — ассеты игры недоступны");
        return None;
    }

    app.new_game_on_surface(&config, &surface);
    app.tick();

    Some(app)
}

/// A free land cell next to the RED mine.
fn free_grid(app: &App) -> Point {
    for radius in 1..6 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(22 + dx, 22 + dy);

                if app.state_ref().units_at(grid).is_empty()
                    && app.state_ref().map().surface_at(grid.x, grid.y).bits()
                        & max_core::enums::Surface::LAND.bits()
                        != 0
                {
                    return grid;
                }
            }
        }
    }

    Point::new(23, 23)
}

/// Adds a RED unit to a free cell and puts the camera on it.
fn spawn(app: &mut App, unit_type: UnitType) -> UnitId {
    let grid = free_grid(app);
    let flags = max_core::access::unit_type_flags(unit_type);
    let id = app
        .universe_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("юнит появился");

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    id
}

/// Screen point of the cell center (640×480 frame with chrome).
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

fn click(app: &mut App, point: (i32, i32)) {
    let _ = handle_session_input(app, InputEvent::MouseClick(point.0, point.1));
}

/// Selection + repeated click = command menu (`GameManager_ClickUnit`).
fn open_menu(app: &mut App, id: UnitId) {
    app.close_unit_menu();
    app.hud_mut().select(None);
    app.hud_mut().cursor_mode = 0;
    app.hud_mut().enter_mode = false;

    let grid = app.state_ref().unit(id).expect("юнит").grid;
    let point = screen_point(app, grid);
    click(app, point);
    click(app, point);

    assert!(app.unit_menu().is_some(), "повторный клик открывает меню");
}

/// `CURSOR_LOAD` on an air transport: the transport flies to the passenger and takes it
/// aboard (`airpath.cpp:430-443` + `units_manager.cpp:3092`).
#[test]
fn air_transport_load_click_takes_the_passenger_aboard() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let transport = spawn(&mut app, UnitType::AirTransport);
    let passenger = spawn(&mut app, UnitType::Tank);
    let passenger_grid = app.state_ref().unit(passenger).expect("танк").grid;

    // `load` — cursor mode 7 (`UnitsManager_Popup_OnClick_Load`).
    open_menu(&mut app, transport);
    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Load))
        .expect("load в меню авиатранспорта");
    let rect = app.unit_menu().expect("меню").button_rect(index);
    click(&mut app, (rect.ulx + 1, rect.uly + 1));
    assert_eq!(app.hud().cursor_mode, unitmenu::CURSOR_LOAD);

    // A click on the passenger: the **transport** gets `ORDER_MOVE/INIT`, `parent = tank`.
    let point = screen_point(&app, passenger_grid);
    click(&mut app, point);

    let unit = app.state_ref().unit(transport).expect("авиатранспорт");
    assert_eq!(
        unit.orders,
        UnitOrder::Move,
        "авиатранспорт летит к пассажиру"
    );
    assert_eq!(unit.parent_unit, Some(passenger));

    // Takeoff animation + flight + landing + boarding (`ProcessLoading` ends in `AWAIT`).
    let mut loaded = false;
    for _ in 0..240 {
        app.tick();
        if app
            .state_ref()
            .unit(transport)
            .is_some_and(|unit| unit.storage == 1 && unit.orders == UnitOrder::Await)
        {
            loaded = true;
            break;
        }
    }

    assert!(loaded, "авиатранспорт должен принять пассажира на борт");

    let transport_unit = app.state_ref().unit(transport).expect("авиатранспорт");
    assert_eq!(
        transport_unit.orders,
        UnitOrder::Await,
        "`ProcessLoading` завершает погрузку"
    );
    assert!(transport_unit.parent_unit.is_none());

    let passenger_unit = app.state_ref().unit(passenger).expect("танк");
    assert_eq!(
        passenger_unit.parent_unit,
        Some(transport),
        "пассажир числится в отсеке авиатранспорта"
    );
    assert_eq!(passenger_unit.orders, UnitOrder::Idle);
}
