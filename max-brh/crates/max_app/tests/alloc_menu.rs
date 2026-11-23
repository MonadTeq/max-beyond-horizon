//! Mine extraction allocation dialog (C++ `AllocMenu`, window `ALLOCFRM`):
//! opening from the unit menu, changing the value, saving it in the unit fields,
//! the effect on the complex economy, determinism and the frame.
//!
//! Tests with real frames (`Assets`/`.WRL`) are skipped if there is no game data
//! next to the repository — like `move_input.rs` and `unit_menu.rs`.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::setup::GameSetupForm;
use max_app::ui::Key;
use max_app::unitmenu::ItemId;
use max_app::{handle_session_input, App, AppConfig, InputEvent, Transition};
use max_core::alloc::MiningKind;
use max_core::enums::{UnitOrder, UnitType};
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
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("alloc_menu: пропуск — нет данных игры рядом с репозиторием");
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
        eprintln!("alloc_menu: пропуск — нет SNOW_1.WRL");
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

/// RED mine (the only one at the start).
fn mining_station(app: &App) -> UnitId {
    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED && unit.unit_type == UnitType::MiningStation)
        })
        .expect("на старте есть шахта RED")
}

fn click(app: &mut App, point: (i32, i32)) -> Transition {
    handle_session_input(app, InputEvent::MouseClick(point.0, point.1), false)
}

fn key(app: &mut App, key: Key) -> Transition {
    handle_session_input(app, InputEvent::Key(key), false)
}

/// Selects the mine and opens the command menu with a repeated click.
fn select_mine(app: &mut App) -> UnitId {
    let id = mining_station(app);
    let grid = app.state_ref().unit(id).expect("шахта").grid;

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);
    let point = (x + MAIN_MAP.ulx, y + MAIN_MAP.uly);

    // Clear the previous selection so that the first click only selects.
    app.close_unit_menu();
    app.hud_mut().select(None);
    let _ = click(app, point);
    assert_eq!(app.hud().selected, Some(id), "первый клик выделяет шахту");

    let _ = click(app, point);
    assert!(app.unit_menu().is_some(), "повторный клик открывает меню");

    id
}

/// Opens the dialog via the command menu `allocate` item (like a player).
fn open_alloc(app: &mut App) -> UnitId {
    let id = select_mine(app);
    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Allocate))
        .expect("пункт allocate есть у шахты");
    let rect = app.unit_menu().expect("меню").button_rect(index);

    let _ = click(app, (rect.ulx + rect.width / 2, rect.uly + rect.height / 2));
    assert!(app.alloc_menu().is_some(), "allocate открывает диалог");
    assert!(app.unit_menu().is_none(), "меню команд закрылось");

    id
}

#[test]
fn allocation_item_is_enabled_for_a_mining_station() {
    let Some(mut app) = game_app() else {
        return;
    };

    select_mine(&mut app);

    let menu = app.unit_menu().expect("меню шахты");
    let allocate = menu
        .index_of(ItemId::Allocate)
        .map(|index| &menu.items[index])
        .expect("allocate в меню");

    assert!(allocate.enabled, "пункт allocate больше не отключён");
    assert!(allocate.note.is_empty(), "у включённого пункта нет причины");
    assert_eq!(allocate.hotkey, '1');
}

#[test]
fn allocate_opens_the_dialog_and_matches_the_cpp_layout() {
    let Some(mut app) = game_app() else {
        return;
    };

    open_alloc(&mut app);

    let menu = app.alloc_menu().expect("диалог");
    assert_eq!(
        max_app::allocmenu::WINDOW_RECT,
        max_render::buffer::PixelRect::new(0, 0, 640, 480)
    );
    // Complex extraction = the mine's `raw_mining`/`fuel_mining`/`gold_mining`.
    let unit = app.state_ref().unit(menu.unit).expect("шахта");
    assert_eq!(menu.production.raw, i32::from(unit.raw_mining));
    assert_eq!(menu.production.fuel, i32::from(unit.fuel_mining));
    assert_eq!(menu.production.gold, i32::from(unit.gold_mining));
    assert!(menu.free_capacity() >= 0);
}

#[test]
fn values_change_and_survive_closing_the_dialog() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = open_alloc(&mut app);

    if app.alloc_menu().expect("диалог").free_capacity() == 0 {
        // The mine starts fully loaded (16 units) — we free the raw material.
        app.alloc_menu_set(MiningKind::Raw, 0);
    }

    app.alloc_menu_set(MiningKind::Raw, 3);
    assert_eq!(
        app.alloc_menu().map(|menu| menu.production_of(MiningKind::Raw)),
        Some(3)
    );
    assert_eq!(app.state_ref().unit(id).expect("шахта").raw_mining, 3);

    // The second resource — the one that is actually present in the mine cell.
    let other = MiningKind::ALL
        .into_iter()
        .find(|kind| {
            *kind != MiningKind::Raw
                && app
                    .alloc_menu()
                    .is_some_and(|menu| menu.capacity_of(*kind) > 0)
        })
        .unwrap_or(MiningKind::Fuel);
    app.alloc_menu_set(other, 1);
    assert_eq!(app.alloc_menu().map(|menu| menu.production_of(other)), Some(1));

    // `Done`: close + `ORDER_NEW_ALLOCATE` (08/P5).
    let _ = key(&mut app, Key::Enter);
    app.flush_orders();
    assert!(app.alloc_menu().is_none(), "Enter закрывает диалог");
    assert_eq!(
        app.state_ref().unit(id).expect("шахта").orders,
        UnitOrder::NewAllocate,
        "C++ рассылает `ORDER_NEW_ALLOCATE` шахтам комплекса"
    );

    // The unit fields remain: the `allocate` task is to change extraction, not the order.
    assert_eq!(app.state_ref().unit(id).expect("шахта").raw_mining, 3);
    let total = app.state_ref().unit(id).expect("шахта").total_mining;
    assert!(total <= 16, "сумма добычи не больше 16: {total}");

    // Reopening shows the saved values.
    app.hud_mut().select(None);
    open_alloc(&mut app);
    assert_eq!(
        app.alloc_menu().map(|menu| menu.production_of(MiningKind::Raw)),
        Some(3)
    );
}

#[test]
fn arrows_bump_one_unit_like_the_cpp_buttons() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = open_alloc(&mut app);

    if app.alloc_menu().expect("диалог").free_capacity() == 0 {
        app.alloc_menu_set(MiningKind::Raw, 0);
    }

    let before = app.state_ref().unit(id).expect("шахта").raw_mining;
    let rect = max_app::allocmenu::RIGHT_ARROW_X;
    let y = max_app::allocmenu::SECTION_TOP[0] + 2;
    let _ = click(&mut app, (rect + 2, y));

    assert_eq!(
        app.state_ref().unit(id).expect("шахта").raw_mining,
        before + 1,
        "правая стрелка `RTARO_OF` добавляет единицу"
    );

    let _ = click(&mut app, (max_app::allocmenu::LEFT_ARROW_X + 2, y));
    assert_eq!(
        app.state_ref().unit(id).expect("шахта").raw_mining,
        before,
        "левая стрелка `LFARO_OF` снимает единицу"
    );
}

#[test]
fn dialog_is_modal_for_the_map() {
    let Some(mut app) = game_app() else {
        return;
    };

    open_alloc(&mut app);
    let pending = app.hud().pending.len();

    // A click on the map outside the dialog: no order, the dialog is open.
    let _ = click(&mut app, (500, 300));

    assert!(app.alloc_menu().is_some(), "клик по карте не закрывает диалог");
    assert_eq!(app.hud().pending.len(), pending, "приказов не появилось");
}

#[test]
fn allocation_changes_the_complex_income_next_turn() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = open_alloc(&mut app);
    let _ = id;

    // Move all extraction from raw to the other resource of the cell (fuel/gold).
    let other = MiningKind::ALL
        .into_iter()
        .find(|kind| {
            *kind != MiningKind::Raw
                && app
                    .alloc_menu()
                    .is_some_and(|menu| menu.capacity_of(*kind) > 0)
        })
        .expect("в клетке шахты есть ещё один ресурс");

    let target = app.alloc_menu().expect("диалог").capacity_of(other).min(5);
    app.alloc_menu_set(MiningKind::Raw, 0);
    app.alloc_menu_set(other, target);
    assert_eq!(
        app.state_ref().unit(id).expect("шахта").raw_mining,
        0,
        "сырьё больше не добывается"
    );

    let _ = key(&mut app, Key::Escape);
    app.flush_orders();

    // `manage_economy` → `OptimizeComplexes` → `Access_UpdateResourcesTotal`.
    app.universe.manage_economy(TeamId::RED);

    let complex_id = app.economy_complex().expect("комплекс шахты");
    let complex = app
        .state_ref()
        .complex(TeamId::RED, complex_id)
        .expect("комплекс шахты");

    assert_eq!(complex.raw, 0, "доля сырья 0 — добычи сырья нет");
    let income = match other {
        MiningKind::Fuel => complex.fuel,
        MiningKind::Gold => complex.gold,
        MiningKind::Raw => unreachable!(),
    };
    // `POWGEN` in the same complex (it stands next to the mine, their connectors
    // merged) consumes fuel itself: `Cargo_GetFuelConsumptionRate(POWGEN) = 2`
    // (`cargo.cpp:183-200`), so the net income = extraction − 2. Before the complexes
    // merged the generator was not in the mine's pool and the test saw the full 5.
    let has_generator = app
        .state_ref()
        .unit_list(max_core::unit::UnitListKind::Stationary)
        .iter()
        .filter_map(|id| app.state_ref().unit(*id))
        .any(|unit| {
            unit.complex.map(|complex| complex.0) == Some(complex_id as i16)
                && unit.unit_type == UnitType::PowerGenerator
        });
    let generator_fuel_draw = if matches!(other, MiningKind::Fuel) && has_generator {
        2
    } else {
        0
    };
    assert!(
        i32::from(income) >= target - generator_fuel_draw,
        "чистая добыча {other:?} попала в запас комплекса: {income} \
         (ждали ≥ {} = {target} − {generator_fuel_draw})",
        target - generator_fuel_draw
    );
    assert!(
        i32::from(income) > 0,
        "добыча {other:?} положительна даже с расходом генератора: {income}"
    );
}

#[test]
fn two_identical_dialogs_stay_deterministic() {
    let Some(mut a) = game_app() else {
        return;
    };
    let Some(mut b) = game_app() else {
        return;
    };

    for app in [&mut a, &mut b] {
        open_alloc(app);
        app.alloc_menu_set(MiningKind::Raw, 2);
        app.alloc_menu_set(MiningKind::Fuel, 1);
        let _ = key(app, Key::Enter);
        app.flush_orders();
    }

    assert_eq!(
        a.digest().state_checksum,
        b.digest().state_checksum,
        "CRC пакета 45 совпадает: добыча входит в снапшот полями юнита"
    );
    assert_eq!(a.state_ref().snapshot_bytes(), b.state_ref().snapshot_bytes());
    assert_eq!(a.digest().state_digest, b.digest().state_digest);
}

#[test]
fn frame_shows_the_full_screen_dialog() {
    let Some(mut app) = game_app() else {
        return;
    };

    let before = app.render_frame();
    open_alloc(&mut app);
    let after = app.render_frame();

    // `ALLOCFRM` — 640×480: the frame changes almost entirely (the map band,
    // panels and units are covered by the dialog).
    let mut changed = 0;

    for y in 0..480 {
        for x in 0..640 {
            if before.get(x, y) != after.get(x, y) {
                changed += 1;
            }
        }
    }

    assert!(
        changed > 640 * 480 / 2,
        "диалог занимает весь кадр (изменилось {changed} пикселей)"
    );
}

#[test]
fn allocate_is_an_ordered_p5_packet() {
    use max_core::enums::UnitOrder;
    use max_core::input::PayloadLevel;

    // `Remote_Init`: `ORDER_NEW_ALLOCATE` → processor P5 (carries the extraction levels).
    assert_eq!(
        PayloadLevel::for_order(UnitOrder::NewAllocate),
        PayloadLevel::P5
    );
}
