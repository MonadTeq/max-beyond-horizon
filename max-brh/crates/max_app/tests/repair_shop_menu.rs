//! `RepairShopMenu` — the slot list that lets the player choose **which**
//! stored unit leaves a transport or a repair-shop building.
//!
//! Manual-run defect: `activate` released the first stored unit automatically,
//! with no way to pick another one. Oracle: `UnitsManager_Popup_InitRepairShops`
//! (`units_manager.cpp:1022-1033`) → `RepairShopMenu_Menu` → the per-slot
//! `RepairShopSlot_OnClick_Activate` (`repairshopmenu.cpp:737-745`) →
//! `RepairShopMenu::Activate` (`:661-690`).
//!
//! The whole chain goes through the player's path: the command menu item, the
//! slot list, the slot button, the selection of the stored unit and the map
//! click that gives the container `ORDER_ACTIVATE`.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::unitmenu::{self, ItemId};
use max_app::{handle_session_input, App, AppConfig, InputEvent};
use max_assets::image::BigImage;
use max_core::enums::{UnitOrder, UnitType};
use max_core::geo::{Point, DIRECTION_OFFSETS};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_render::buffer::Rgba;
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
        eprintln!("repair_shop_menu: нет данных — нет данных игры рядом с репозиторием");
        return None;
    }

    let mut flow = PreGameFlow::start(
        max_app::setup::GameSetupForm::default(),
        single_human(),
        &world_dir,
    );
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
        eprintln!("repair_shop_menu: нет данных — ассеты игры недоступны");
        return None;
    }

    app.new_game_on_surface(&config, &surface);
    app.tick();

    Some(app)
}

/// A free land cell and a free land neighbour next to the RED mine.
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

                for step in DIRECTION_OFFSETS {
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

/// Adds a RED unit to a cell.
fn spawn_at(app: &mut App, unit_type: UnitType, grid: Point) -> UnitId {
    let flags = max_core::access::unit_type_flags(unit_type);
    app.universe_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("юнит появился")
}

/// The hold state `UnitInfo::ProcessLoading` leaves behind: the passenger is
/// `ORDER_IDLE`/`STORE`, its parent is the carrier, and `storage` counts it.
fn store_passenger(app: &mut App, carrier: UnitId, passenger: UnitId) {
    {
        let cargo = app.universe_mut().unit_mut(passenger).expect("пассажир");
        cargo.orders = UnitOrder::Idle;
        cargo.state = max_core::enums::UnitOrderState::Store;
        cargo.parent_unit = Some(carrier);
    }
    let bays = app
        .state_ref()
        .unit_ids_in_order()
        .iter()
        .filter(|id| {
            app.state_ref().unit(**id).is_some_and(|unit| {
                unit.parent_unit == Some(carrier) && unit.orders == UnitOrder::Idle
            })
        })
        .count() as i16;
    app.universe_mut()
        .unit_mut(carrier)
        .expect("транспорт")
        .storage = bays;
}

/// A free 2×2 land area (a building footprint), searched around the RED mine.
fn free_2x2(app: &App) -> Point {
    let land = max_core::enums::Surface::LAND.bits();
    let free = |cell: Point| {
        app.state_ref().units_at(cell).is_empty()
            && app.state_ref().map().surface_at(cell.x, cell.y).bits() & land != 0
    };

    for radius in 1..12 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(22 + dx, 22 + dy);

                if (0..2).all(|y| (0..2).all(|x| free(Point::new(grid.x + x, grid.y + y)))) {
                    return grid;
                }
            }
        }
    }

    Point::new(30, 30)
}

/// Opens the command menu of the container and presses `activate`; the slot
/// list of the hold opens.
fn open_slot_list(app: &mut App, container: UnitId) {
    app.hud_mut().select(Some(container));
    assert!(app.open_unit_menu(), "меню контейнера открывается");

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Activate))
        .expect("activate у контейнера");
    assert!(
        app.unit_menu().expect("меню").items[index].enabled,
        "activate открывает RepairShopMenu"
    );

    let rect = app.unit_menu().expect("меню").button_rect(index);
    click(app, (rect.ulx + 1, rect.uly + 1));

    assert!(app.unit_menu().is_none(), "меню команд закрылось");
    assert!(
        app.repair_shop().is_some(),
        "`RepairShopMenu_Menu` — экран списка хранимых юнитов"
    );
}

/// The defect itself: with two stored units the second slot releases exactly
/// the unit that was loaded **first** (the list head is the most recent one),
/// not "the first one" blindly.
#[test]
fn the_slot_button_releases_the_chosen_stored_unit() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let (carrier_grid, drop) = free_land_pair(&app);
    let carrier = spawn_at(&mut app, UnitType::ArmouredPersonnelCarrier, carrier_grid);
    let loaded_first = spawn_at(
        &mut app,
        UnitType::Infantry,
        Point::new(drop.x + 2, drop.y + 2),
    );
    let loaded_second = spawn_at(
        &mut app,
        UnitType::Infantry,
        Point::new(drop.x + 3, drop.y + 2),
    );

    store_passenger(&mut app, carrier, loaded_first);
    store_passenger(&mut app, carrier, loaded_second);

    app.viewport_mut()
        .center_on_grid(carrier_grid.x, carrier_grid.y);

    open_slot_list(&mut app, carrier);

    let order = {
        let menu = app.repair_shop().expect("список слотов");
        assert_eq!(menu.shop, carrier);
        assert_eq!(menu.units.len(), 2, "оба юнита в списке");

        // `Access_IsHeldByUnit` order: the list is `PushFront`-ed, so the unit
        // loaded last is slot 0 and the first loaded one is slot 1.
        assert_eq!(menu.units[0], loaded_second);
        assert_eq!(menu.units[1], loaded_first);
        menu.activate_rect(1)
    };

    // `RepairShopSlot_OnClick_Activate` for the **second** slot.
    click(&mut app, (order.ulx + 1, order.uly + 1));

    assert!(
        app.repair_shop().is_none(),
        "выбор слота закрывает экран (`Deinit` в начале `Activate`)"
    );
    assert_eq!(
        app.hud().selected,
        Some(loaded_first),
        "выбран именно тот юнит, чей слот нажали"
    );
    assert_eq!(
        app.hud().hint.as_deref(),
        Some(unitmenu::SELECT_DROP_SQUARE),
        "`_(755b)` — укажите клетку высадки"
    );

    // A click on a free neighbour gives the carrier `ORDER_ACTIVATE`.
    let point = screen_point(&app, drop);
    click(&mut app, point);

    assert_eq!(
        app.state_ref().unit(carrier).expect("транспорт").orders,
        UnitOrder::Activate,
        "приказ выгрузки получил транспорт"
    );
    assert_eq!(
        app.state_ref()
            .unit(carrier)
            .expect("транспорт")
            .parent_unit,
        Some(loaded_first),
        "выгружается выбранный юнит"
    );

    let mut unloaded = false;
    for _ in 0..16 {
        app.tick();
        unloaded |= app
            .state_ref()
            .unit(loaded_first)
            .is_some_and(|unit| unit.orders == UnitOrder::Await);
        if unloaded {
            break;
        }
    }

    assert!(unloaded, "выбранный юнит покинул трюм");
    assert_eq!(app.state_ref().unit(loaded_first).expect("юнит").grid, drop);
    assert_eq!(
        app.state_ref().unit(carrier).expect("транспорт").storage,
        1,
        "вторая единица груза осталась в трюме"
    );
    assert_eq!(
        app.state_ref()
            .unit(loaded_second)
            .expect("второй юнит")
            .parent_unit,
        Some(carrier),
        "другой хранимый юнит не тронут"
    );
}

/// The screen is modal: a click on the map does not give an order, `Done`/`Esc`
/// closes it and keeps the container selected (`GameManager_EnableMainMenu`).
#[test]
fn the_slot_list_is_modal_and_done_keeps_the_selection() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let (carrier_grid, _drop) = free_land_pair(&app);
    let carrier = spawn_at(&mut app, UnitType::ArmouredPersonnelCarrier, carrier_grid);
    let cargo = spawn_at(
        &mut app,
        UnitType::Infantry,
        Point::new(carrier_grid.x + 3, carrier_grid.y),
    );
    store_passenger(&mut app, carrier, cargo);

    app.viewport_mut()
        .center_on_grid(carrier_grid.x, carrier_grid.y);

    open_slot_list(&mut app, carrier);

    // A click on the map (outside the screen buttons) — no order, the screen stays.
    let (x, y) = screen_point(&app, Point::new(carrier_grid.x + 1, carrier_grid.y + 1));
    click(&mut app, (x, y));

    assert!(
        app.repair_shop().is_some(),
        "клик по карте не закрывает экран"
    );
    assert!(app.hud().pending.is_empty(), "приказов нет");
    assert_eq!(
        app.state_ref().unit(carrier).expect("транспорт").orders,
        UnitOrder::Await
    );

    // `RepairShopMenu_OnClick_Done`: the screen closes, the selection stays.
    let done = max_app::repairshopmenu::DONE_RECT;
    click(&mut app, (done.ulx + 1, done.uly + 1));

    assert!(app.repair_shop().is_none());
    assert_eq!(app.hud().selected, Some(carrier));
    assert!(app.hud().pending.is_empty());
}

/// `BARRACKS`/`DEPOT`/`HANGAR`/`DOCK` use the same slot list
/// (`UnitsManager_Popup_InitRepairShops`), and a `HANGAR` has four slots.
#[test]
fn hangar_lists_its_stored_aircraft_in_four_slots() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    // A 2×2 building needs a free area.
    let grid = free_2x2(&app);
    let hangar = spawn_at(&mut app, UnitType::Hangar, grid);
    let fighter = spawn_at(
        &mut app,
        UnitType::Fighter,
        Point::new(grid.x + 3, grid.y + 3),
    );
    let bomber = spawn_at(
        &mut app,
        UnitType::Bomber,
        Point::new(grid.x + 4, grid.y + 3),
    );

    store_passenger(&mut app, hangar, fighter);
    store_passenger(&mut app, hangar, bomber);

    app.viewport_mut().center_on_grid(grid.x, grid.y);

    open_slot_list(&mut app, hangar);

    let menu = app.repair_shop().expect("список слотов ангара");
    assert!(menu.is_hangar);
    assert_eq!(
        menu.slots_per_screen,
        max_app::repairshopmenu::HANGAR_SLOTS_PER_SCREEN
    );
    assert_eq!(menu.units.len(), 2);
    // The second slot (index 1) is the first loaded aircraft (Bomber).
    assert_eq!(menu.units[1], bomber);

    let rect = menu.activate_rect(1);
    click(&mut app, (rect.ulx + 1, rect.uly + 1));

    assert!(app.repair_shop().is_none());
    assert_eq!(
        app.hud().selected,
        Some(bomber),
        "выбран самолёт из нажатого слота"
    );
}

/// Manual-run defect 3: the slots showed only the unit name instead of its picture.
///
/// `RepairShopSlot::Draw` (`repairshopmenu.cpp:243-245`) fills the slot picture
/// area with the armory portrait `A_*` (`Unit::GetArmoryPortrait`). The port draws
/// big images with their own palette (`DEPOTFRM`/`HANGRFRM` do the same), so the
/// pixels below the name text must be exactly the decoded `A_*` picture.
#[test]
fn slots_draw_the_armory_portrait_of_the_stored_unit() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let (carrier_grid, _drop) = free_land_pair(&app);
    let carrier = spawn_at(&mut app, UnitType::ArmouredPersonnelCarrier, carrier_grid);
    let passenger = spawn_at(
        &mut app,
        UnitType::Infantry,
        Point::new(carrier_grid.x + 3, carrier_grid.y + 2),
    );
    store_passenger(&mut app, carrier, passenger);

    app.viewport_mut()
        .center_on_grid(carrier_grid.x, carrier_grid.y);
    open_slot_list(&mut app, carrier);

    let rect = app
        .repair_shop()
        .expect("список слотов")
        .slot_image_rect(0);

    let frame = app.render_frame();
    let assets = app.assets().expect("assets games");
    let bytes = assets
        .max_res
        .read("A_INFANT")
        .expect("A_INFANT в MAX.RES");
    let portrait = BigImage::decode(bytes).expect("A_INFANT декодируется");
    assert_eq!((portrait.width, portrait.height), (128, 128));

    // Below the name label (`Text_TextBox(..., 10, 3, w-20, ...)`), the slot's
    // statistics/`Activate` start at `+140`: rows `40..128` are pure picture.
    let mut compared = 0;

    for y in 40..128u32 {
        for x in 0..128u32 {
            let index = portrait.pixel(x, y).expect("pixel portrait");
            let expected = Rgba::from_rgb(portrait.color(index));
            let actual = frame
                .get(rect.ulx + x as i32, rect.uly + y as i32)
                .expect("pixel frame");

            assert_eq!(
                actual, expected,
                "слот 0: пиксель ({x},{y}) — портрет юнита, а не текст"
            );
            compared += 1;
        }
    }

    assert_eq!(compared, 128 * 88);
}
