//! Transport `activate` — the defect "a personnel/sea transport cannot set its
//! stored units down on the cell the player points at".
//!
//! The whole chain goes through the player's path:
//!
//! 1. the `activate` item of the transport menu — C++
//!    `UnitsManager_Popup_OnClick_Activate` (`units_manager.cpp:997-1009`) opens
//!    the `RepairShopMenu` **slot list**; its per-slot `Activate` button
//!    (`repairshopmenu.cpp:737-745`) calls `RepairShopMenu::Activate`
//!    (`:661-690`). For a non-air `repairshop` the oracle only checks
//!    `Access_FindReachableSpot`, selects the stored passenger
//!    (`GameManager_EnableMainMenu(target_unit)`) and asks for a cell (`_(755b)`
//!    "Select an open square to place unit.");
//! 2. a left click on a cell next to the carrier — `CURSOR_ACTIVATE`
//!    (`GameManager_GetAirUnitCursor`, `game_manager.cpp:5937-5962`) gives the
//!    **carrier** `ORDER_ACTIVATE`/`EXECUTING_ORDER` with the passenger as its
//!    parent and the clicked cell in `move_to` (`game_manager.cpp:6936-6946`);
//! 3. `UnitsManager_ActivateUnit` (`units_manager.cpp:3826-3913`) sets the
//!    passenger down on that cell, gives one cargo bay back and returns the
//!    carrier to `ORDER_AWAIT`.
//!
//! Oracle: `max-cpp-port` (`units_manager.cpp`, `repairshopmenu.cpp`,
//! `game_manager.cpp`).

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::setup::GameSetupForm;
use max_app::unitmenu::{self, ItemId};
use max_app::{handle_session_input, App, AppConfig, InputEvent};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
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
fn game_app() -> Option<App> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() || !world_dir.join("MAX.RES").is_file() {
        eprintln!("transport_unload: нет данных — нет данных игры рядом с репозиторием");
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
        eprintln!("transport_unload: нет данных — ассеты игры недоступны");
        return None;
    }

    app.new_game_on_surface(&config, &surface);
    app.tick();

    Some(app)
}

/// A free land cell and a free land neighbour (the drop site) next to the RED mine.
fn free_land_pair(app: &App) -> (Point, Point) {
    let land = max_core::enums::Surface::LAND.bits();
    let free = |grid: Point| {
        app.state_ref().units_at(grid).is_empty()
            && app.state_ref().map().surface_at(grid.x, grid.y).bits() & land != 0
    };

    for radius in 1..10 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(22 + dx, 22 + dy);

                if !free(grid) {
                    continue;
                }

                for step in max_core::geo::DIRECTION_OFFSETS {
                    let neighbour = Point::new(grid.x + step.x, grid.y + step.y);

                    if free(neighbour) {
                        return (grid, neighbour);
                    }
                }
            }
        }
    }

    (Point::new(23, 23), Point::new(24, 23))
}

/// Screen point of the cell center (640×480 frame with chrome).
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

fn click(app: &mut App, point: (i32, i32)) {
    let _ = handle_session_input(app, InputEvent::MouseClick(point.0, point.1));
}

/// Adds a RED unit to a cell; the camera stays where the caller put it.
fn spawn_at(app: &mut App, unit_type: UnitType, grid: Point) -> UnitId {
    let flags = max_core::access::unit_type_flags(unit_type);
    app.universe_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("юнит появился")
}

/// The hold state `UnitInfo::ProcessLoading` (`unitinfo.cpp:5161`) leaves behind:
/// the passenger is `ORDER_IDLE`/`STORE`, its parent is the carrier, and the
/// carrier's `storage` counts it.
fn store_passenger(app: &mut App, carrier: UnitId, passenger: UnitId) {
    {
        let cargo = app.universe_mut().unit_mut(passenger).expect("пассажир");
        cargo.orders = UnitOrder::Idle;
        cargo.state = UnitOrderState::Store;
        cargo.parent_unit = Some(carrier);
    }
    app.universe_mut()
        .unit_mut(carrier)
        .expect("транспорт")
        .storage = 1;
}

/// Opens the transport menu with real clicks, presses `activate`, and then the
/// `Activate` button of the **first slot** of the opened `RepairShopMenu`
/// (`repairshopmenu.cpp:737-745`).
fn press_activate(app: &mut App, carrier: UnitId) {
    app.hud_mut().select(Some(carrier));
    assert!(app.open_unit_menu(), "меню транспорта открывается");

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Activate))
        .expect("activate у транспорта с грузом");
    assert!(
        app.unit_menu().expect("меню").items[index].enabled,
        "activate включён, пока трюм не пуст"
    );

    let rect = app.unit_menu().expect("меню").button_rect(index);
    click(app, (rect.ulx + 1, rect.uly + 1));

    assert!(app.unit_menu().is_none(), "пункт закрывает меню");

    // `UnitsManager_Popup_OnClick_Activate` → `RepairShopMenu_Menu`: the slot
    // list of the hold opens; the player picks the stored unit there.
    let menu = app.repair_shop().expect("RepairShopMenu открыт");
    assert!(!menu.units.is_empty(), "в списке есть хранимый юнит");

    let slot = menu.activate_rect(0);
    click(app, (slot.ulx + 1, slot.uly + 1));

    assert!(
        app.repair_shop().is_none(),
        "выбор слота закрывает экран (`Deinit` в начале `Activate`)"
    );
}

/// The CLNTRANS (personnel carrier) end-to-end: `activate` selects the stored
/// infantryman, the click on the pointed cell drops it there, and the frame shows
/// the released unit on that cell.
#[test]
fn personnel_carrier_activate_drops_the_unit_on_the_clicked_cell() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let (carrier_grid, drop) = free_land_pair(&app);
    let carrier = spawn_at(&mut app, UnitType::ArmouredPersonnelCarrier, carrier_grid);
    let passenger = spawn_at(
        &mut app,
        UnitType::Infantry,
        Point::new(drop.x + 2, drop.y + 2),
    );

    store_passenger(&mut app, carrier, passenger);

    app.viewport_mut()
        .center_on_grid(carrier_grid.x, carrier_grid.y);

    // `RepairShopMenu::Activate`, non-air branch: the passenger becomes the
    // selected unit and `_(755b)` asks for a cell — no order yet.
    press_activate(&mut app, carrier);

    assert_eq!(
        app.hud().selected,
        Some(passenger),
        "C++ `GameManager_EnableMainMenu(target_unit)` selects the stored unit"
    );
    assert_eq!(
        app.hud().hint.as_deref(),
        Some(unitmenu::SELECT_DROP_SQUARE),
        "the message box asks to select an open square"
    );
    assert!(
        app.hud().pending.is_empty(),
        "activate itself gives no order — the player still has to point at a cell"
    );

    // A frame with the unit still in the hold (the drop cell is empty ground).
    let before = app.render_frame();

    // `CURSOR_ACTIVATE`: the click on the cell next to the carrier.
    let point = screen_point(&app, drop);
    click(&mut app, point);

    let carrier_now = app.state_ref().unit(carrier).expect("транспорт");
    assert_eq!(
        carrier_now.orders,
        UnitOrder::Activate,
        "the order goes to the carrier (the applied packet 08/P3)"
    );
    assert_eq!(carrier_now.state, UnitOrderState::ExecutingOrder);
    assert_eq!(carrier_now.parent_unit, Some(passenger));
    assert_eq!(
        app.state_ref().order_data(carrier).move_to,
        drop,
        "the pointed cell travels in `move_to`"
    );
    assert!(
        app.hud().hint.is_none(),
        "a map click closes the message box (`MessageManager_ClearMessageBox`)"
    );

    // `UnitsManager_ActivateUnit` — the passenger is set down on the drop cell.
    let mut unloaded = false;
    for _ in 0..16 {
        app.tick();
        unloaded |= app
            .state_ref()
            .unit(passenger)
            .is_some_and(|unit| unit.orders == UnitOrder::Await);
        if unloaded {
            break;
        }
    }

    assert!(unloaded, "the passenger must leave the hold");
    let cargo = app.state_ref().unit(passenger).expect("пассажир");
    assert_eq!(cargo.grid, drop, "the unit stands on the pointed cell");
    assert!(cargo.parent_unit.is_none());
    assert!(app.state_ref().units_at(drop).contains(&passenger));

    let carrier_now = app.state_ref().unit(carrier).expect("транспорт");
    assert_eq!(carrier_now.storage, 0, "one cargo bay was given back");
    assert!(carrier_now.parent_unit.is_none());

    // The released unit is drawn on the drop cell (the "frame" regression: the
    // stored unit used to stay invisible and out of `Hash_MapHash`).
    let after = app.render_frame();
    let rect = app.map_viewport().tile_rect(drop.x, drop.y);
    let mut changed = 0;

    for y in (rect.uly + MAIN_MAP.uly)..(rect.lry() + MAIN_MAP.uly) {
        for x in (rect.ulx + MAIN_MAP.ulx)..(rect.lrx() + MAIN_MAP.ulx) {
            if before.get(x, y) != after.get(x, y) {
                changed += 1;
            }
        }
    }

    assert!(
        changed > 32,
        "the released unit must be visible on the drop cell (changed {changed} pixels)"
    );
}

/// The SEATRANS end-to-end: the tank leaves the hold onto the pointed shore cell
/// while the carrier stays in the water; a water cell is refused (no order).
#[test]
fn sea_transport_activate_sets_the_unit_down_on_the_shore() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let (water, shore) = free_land_pair(&app);
    let sea = Point::new(water.x, water.y);

    // The carrier floats, the pointed cell is a beach.
    app.universe_mut()
        .map_mut()
        .surface
        .set(sea.x, sea.y, max_core::enums::Surface::WATER);
    app.universe_mut()
        .map_mut()
        .surface
        .set(shore.x, shore.y, max_core::enums::Surface::LAND);

    let carrier = spawn_at(&mut app, UnitType::SeaTransport, sea);
    let passenger = spawn_at(
        &mut app,
        UnitType::Tank,
        Point::new(shore.x + 2, shore.y + 2),
    );
    store_passenger(&mut app, carrier, passenger);

    app.viewport_mut().center_on_grid(sea.x, sea.y);
    press_activate(&mut app, carrier);
    assert_eq!(app.hud().selected, Some(passenger));

    // A water cell is impassable for the tank: `CURSOR_UNIT_NO_GO`, no order —
    // the guard also covers `ORDER_IDLE` passengers (they must not move out of
    // the hold on their own).
    let other_water = max_core::geo::DIRECTION_OFFSETS
        .iter()
        .map(|step| Point::new(sea.x + step.x, sea.y + step.y))
        .find(|cell| *cell != shore && app.state_ref().units_at(*cell).is_empty())
        .expect("a free cell next to the carrier");
    app.universe_mut().map_mut().surface.set(
        other_water.x,
        other_water.y,
        max_core::enums::Surface::WATER,
    );

    let point = screen_point(&app, other_water);
    click(&mut app, point);

    assert!(
        app.hud().pending.is_empty(),
        "water is not a drop cell for a land unit"
    );
    assert_eq!(
        app.hud().selected,
        Some(passenger),
        "a no-go click keeps the stored unit selected"
    );
    assert_eq!(
        app.state_ref().unit(carrier).expect("транспорт").orders,
        UnitOrder::Await,
        "the carrier received no order"
    );

    // The shore cell: the tank is set down there.
    let point = screen_point(&app, shore);
    click(&mut app, point);

    let mut unloaded = false;
    for _ in 0..16 {
        app.tick();
        unloaded |= app
            .state_ref()
            .unit(passenger)
            .is_some_and(|unit| unit.orders == UnitOrder::Await);
        if unloaded {
            break;
        }
    }

    assert!(unloaded, "the tank must leave the sea transport");
    let cargo = app.state_ref().unit(passenger).expect("танк");
    assert_eq!(cargo.grid, shore, "the tank stands on the shore cell");
    assert!(app.state_ref().units_at(shore).contains(&passenger));
    assert_eq!(app.state_ref().unit(carrier).expect("транспорт").storage, 0);
}
