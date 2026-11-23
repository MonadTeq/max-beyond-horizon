//! X-FER (`TransferMenu`) in the live application: the menu item is enabled, the window
//! opens by clicking the target in cursor mode, `Done` gives `ORDER_TRANSFER`
//! (08/P3), the stocks change, `Esc` changes nothing, the frame draws the window.
//!
//! Tests with real assets are skipped if there is no game data next to
//! the repository (like `alloc_menu.rs`/`unit_menu.rs`).

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::setup::GameSetupForm;
use max_app::ui::Key;
use max_app::unitmenu::ItemId;
use max_app::{handle_session_input, App, AppConfig, InputEvent, Transition};
use max_core::enums::{UnitOrder, UnitType};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_render::layout::MAIN_MAP;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

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
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("transfer: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"));
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
}

/// A match through the real start phases: mobilization → landing → `InGame`.
fn game_app() -> Option<App> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() {
        eprintln!("transfer: пропуск — нет SNOW_1.WRL");
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

fn unit_of_type(app: &App, unit_type: UnitType) -> UnitId {
    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref().unit(*id).is_some_and(|unit| {
                unit.team == TeamId::RED && unit.unit_type == unit_type
            })
        })
        .unwrap_or_else(|| panic!("на старте есть {unit_type:?} RED"))
}

fn click(app: &mut App, point: (i32, i32)) -> Transition {
    handle_session_input(app, InputEvent::MouseClick(point.0, point.1), false)
}

fn key(app: &mut App, key: Key) -> Transition {
    handle_session_input(app, InputEvent::Key(key), false)
}

fn frame_point(app: &mut App, id: UnitId) -> (i32, i32) {
    let grid = app.state_ref().unit(id).expect("unit").grid;
    app.viewport_mut().center_on_grid(grid.x, grid.y);
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

/// Selects the unit and opens the command menu with a repeated click.
fn select_and_open_menu(app: &mut App, id: UnitId) {
    let point = frame_point(app, id);
    app.close_unit_menu();
    app.hud_mut().select(None);
    let _ = click(app, point);
    assert_eq!(app.hud().selected, Some(id), "первый клик выделяет");
    let _ = click(app, point);
    assert!(app.unit_menu().is_some(), "повторный клик открывает меню");
}

/// Prepares a mine → constructor transfer with known stocks:
/// `source` (mine) 10 of 25, target (constructor) 40 of 60.
fn prepare_transfer(app: &mut App) -> (UnitId, UnitId) {
    let mine = unit_of_type(app, UnitType::MiningStation);
    let constructor = unit_of_type(app, UnitType::Constructor);

    let complex = app
        .state_ref()
        .unit(mine)
        .and_then(|unit| unit.complex)
        .expect("у шахты есть комплекс");

    app.universe.unit_mut(mine).expect("mine").storage = 10;
    app.universe
        .unit_mut(constructor)
        .expect("constructor")
        .storage = 40;
    if let Some(target) = app
        .universe
        .complex_mut(TeamId::RED, complex.0 as u16)
    {
        target.raw = 10;
    }

    (mine, constructor)
}

/// Opens the X-FER window as a player: menu → x-fer → click on the target.
fn open_transfer(app: &mut App, source: UnitId, target: UnitId) -> UnitId {
    select_and_open_menu(app, source);

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Transfer))
        .expect("x-fer в меню");
    let rect = app.unit_menu().expect("меню").button_rect(index);
    let _ = click(app, (rect.ulx + rect.width / 2, rect.uly + rect.height / 2));

    assert_eq!(
        app.hud().cursor_mode,
        max_app::unitmenu::CURSOR_TRANSFER,
        "x-fer включает курсорный режим 3"
    );
    assert!(app.unit_menu().is_none(), "меню команд закрылось");

    let point = frame_point(app, target);
    let _ = click(app, point);

    assert!(app.transfer_menu().is_some(), "клик по цели открывает X-FER");
    assert_eq!(app.transfer_menu().expect("меню").unit, source);
    assert_eq!(app.transfer_menu().expect("меню").target, target);

    source
}

#[test]
fn xfer_item_is_enabled_for_a_mining_station() {
    let Some(mut app) = game_app() else {
        return;
    };

    let mine = unit_of_type(&app, UnitType::MiningStation);
    select_and_open_menu(&mut app, mine);

    let menu = app.unit_menu().expect("меню шахты");
    let transfer = menu
        .index_of(ItemId::Transfer)
        .map(|index| &menu.items[index])
        .expect("x-fer в меню шахты");

    assert!(transfer.enabled, "x-fer больше не отключён");
    assert!(transfer.note.is_empty());
    assert_eq!(transfer.hotkey, '3');
}

#[test]
fn xfer_opens_on_target_click_and_moves_cargo_on_done() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (mine, constructor) = prepare_transfer(&mut app);

    open_transfer(&mut app, mine, constructor);

    {
        let menu = app.transfer_menu().expect("окно");
        assert_eq!(
            (menu.source_materials, menu.source_capacity),
            (10, 25),
            "запас/ёмкость комплекса шахты"
        );
        assert_eq!(
            (menu.target_materials, menu.target_capacity),
            (40, 60),
            "storage/ATTRIB_STORAGE конструктора"
        );
        assert_eq!(menu.give_max, 10, "min(60-40, 10)");
        assert_eq!(menu.take_max, -15, "-min(25-10, 40)");
        assert_eq!(menu.amount, 10, "по умолчанию заполняем цель");
    }

    assert!(app.transfer_set_amount(6));
    assert_eq!(app.transfer_menu().expect("окно").amount, 6);
    let _ = key(&mut app, Key::Enter);
    assert!(app.transfer_menu().is_none(), "Done закрывает окно");

    // The order is applied (offline — immediately), but `ProcessTransfer` runs on the tick.
    assert_eq!(
        app.state_ref().unit(mine).expect("mine").orders,
        UnitOrder::Transfer
    );
    app.tick();

    assert_eq!(
        app.state_ref().unit(mine).expect("mine").storage,
        4,
        "шахта отдала 6"
    );
    assert_eq!(
        app.state_ref().unit(constructor).expect("constructor").storage,
        46,
        "конструктор принял 6"
    );
    let complex = app
        .state_ref()
        .unit(mine)
        .and_then(|unit| unit.complex)
        .expect("комплекс");
    assert_eq!(
        app.state_ref()
            .complex(TeamId::RED, complex.0 as u16)
            .expect("комплекс")
            .raw,
        4,
        "запас комплекса уменьшился на 6"
    );
    assert_eq!(
        app.state_ref().unit(mine).expect("mine").orders,
        UnitOrder::PowerOn,
        "RestoreOrders вернул PowerOn"
    );
}

#[test]
fn xfer_cancel_and_zero_amount_do_not_change_storage() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (mine, constructor) = prepare_transfer(&mut app);

    open_transfer(&mut app, mine, constructor);
    assert!(app.transfer_set_amount(7));
    let _ = key(&mut app, Key::Escape);
    assert!(app.transfer_menu().is_none(), "Esc закрывает окно");

    app.tick();

    assert_eq!(app.state_ref().unit(mine).expect("mine").storage, 10);
    assert_eq!(
        app.state_ref().unit(constructor).expect("constructor").storage,
        40
    );
    assert_eq!(
        app.state_ref().unit(mine).expect("mine").orders,
        UnitOrder::PowerOn
    );

    // Zero amount: `Done` closes the window without an order.
    open_transfer(&mut app, mine, constructor);
    assert!(app.transfer_set_amount(0));
    let _ = key(&mut app, Key::Enter);
    assert!(app.transfer_menu().is_none());
    app.tick();
    assert_eq!(app.state_ref().unit(mine).expect("mine").storage, 10);
}

#[test]
fn xfer_help_opens_transfer_setup() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (mine, constructor) = prepare_transfer(&mut app);
    open_transfer(&mut app, mine, constructor);

    // `?` — `TransferMenu::ProcessKey`: the C++ help mode (`HelpMenu_Menu`).
    let _ = key(&mut app, Key::Char('?'));
    assert!(
        app.help_screen().is_none(),
        "? включает режим помощи, окна ещё нет"
    );
    assert_eq!(
        app.help_mode().map(|mode| mode.section),
        Some(max_app::helpmenu::HelpSection::Transfer)
    );

    // A click on the `TRANSFERER` element (335..371, 177..213) opens the entry.
    let _ = max_app::ui::handle_session_input(
        &mut app,
        max_app::ui::InputEvent::MouseClick(353, 195),
        false,
    );
    assert_eq!(
        app.help_screen().and_then(|help| help.entry_name()),
        Some("TRANSFERER")
    );

    // `Esc` closes the window, a second `Esc` — the mode; the X-FER window remains under it
    // (C++ `HelpMenu::Run` returns to `TransferMenu`).
    let _ = key(&mut app, Key::Escape);
    assert!(app.help_screen().is_none(), "Esc закрывает окно помощи");
    assert!(app.help_mode().is_some(), "режим помощи ещё активен");
    let _ = key(&mut app, Key::Escape);
    assert!(app.help_mode().is_none(), "второй Esc выключает режим");
    assert!(
        app.transfer_menu().is_some(),
        "окно X-FER осталось под помощью (C++ HelpMenu возвращается в меню)"
    );
}

#[test]
fn xfer_frame_draws_the_window_over_the_map() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (mine, constructor) = prepare_transfer(&mut app);

    let before = app.render_frame();
    open_transfer(&mut app, mine, constructor);
    let after = app.render_frame();

    let window = app.transfer_menu().expect("окно").window_rect();
    let mut changed = 0;

    for y in window.uly..window.lry() {
        for x in window.ulx..window.lrx() {
            let a = before.get(x, y);
            let b = after.get(x, y);

            if a != b {
                changed += 1;
            }
        }
    }

    assert!(
        changed > 1000,
        "окно XFERPIC 308×162 нарисовано поверх карты (изменилось {changed} px)"
    );

    // The `SMBRRAW` bar is visible in its rectangle (bar pixels differ
    // from the XFERPIC background) — the window is drawn with assets, not a frame.
    let frame_rect = max_app::transfermenu::TransferMenu::layout(MAIN_MAP);
    let slider = max_app::transfermenu::SLIDER_RECT;
    let any_bar_pixel = (slider.uly..slider.lry()).any(|y| {
        (slider.ulx..slider.lrx()).any(|x| {
            after.get(frame_rect.ulx + x, frame_rect.uly + y)
                != before.get(frame_rect.ulx + x, frame_rect.uly + y)
        })
    });
    assert!(any_bar_pixel, "полоса ресурса нарисована");
}
