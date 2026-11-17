//! Меню команд выделенного юнита (C++ `GameManager_PopupButtons`): триггер
//! повторным кликом, состав пунктов по типу, модальность, `Esc` и вызов
//! существующих обработчиков.
//!
//! Тесты с настоящими кадрами (`Assets`/`.WRL`) пропускаются, если данных игры
//! нет рядом с репозиторием — как `move_input.rs` и `tests/app.rs`.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::setup::GameSetupForm;
use max_app::ui::Key;
use max_app::unitmenu::{self, ItemId};
use max_app::{handle_session_input, App, AppConfig, InputEvent, Transition};
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

/// Одна человеческая команда RED, остальное — ИИ/пусто.
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
        eprintln!("unit_menu: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"));
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
}

/// Партия через настоящие фазы старта: мобилизация → высадка → `InGame`.
fn game_app() -> Option<App> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() {
        eprintln!("unit_menu: пропуск — нет SNOW_1.WRL");
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

/// Свободная клетка рядом с шахтой RED.
fn free_grid(app: &App) -> Point {
    for radius in 1..6 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(22 + dx, 22 + dy);

                if app.state().units_at(grid).is_empty()
                    && app
                        .state()
                        .map
                        .surface_at(grid.x, grid.y)
                        .bits()
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

/// Добавляет юнита RED в свободную клетку и ставит камеру на него.
fn spawn(app: &mut App, unit_type: UnitType) -> UnitId {
    let grid = free_grid(app);
    let flags = max_core::access::unit_type_flags(unit_type);
    let id = app
        .state_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("юнит появился");

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    id
}

/// Экранная точка центра клетки (кадр 640×480 с хромом).
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

fn click(app: &mut App, point: (i32, i32)) -> Transition {
    handle_session_input(app, InputEvent::MouseClick(point.0, point.1), false)
}

fn key(app: &mut App, key: Key) -> Transition {
    handle_session_input(app, InputEvent::Key(key), false)
}

/// Двойной клик по юниту (выделение + повторный клик = меню) через ввод.
///
/// Выделение заранее снимается: после пункта меню юнит остаётся выделенным, и
/// одиночный клик по нему снова открыл бы меню (как в C++).
fn open_menu(app: &mut App, id: UnitId) {
    app.close_unit_menu();
    app.hud_mut().select(None);
    app.hud_mut().cursor_mode = 0;
    app.hud_mut().enter_mode = false;

    let grid = app.state().unit(id).expect("юнит").grid;
    let point = screen_point(app, grid);
    let _ = click(app, point);
    assert_eq!(app.hud().selected, Some(id), "первый клик выделяет юнита");

    let _ = click(app, point);
    assert!(app.unit_menu().is_some(), "повторный клик открывает меню");
}

#[test]
fn repeat_click_on_selected_unit_opens_menu() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = spawn(&mut app, UnitType::Tank);
    let grid = app.state().unit(id).expect("танк").grid;
    let point = screen_point(&app, grid);

    // Первый клик — только выделение (как C++ `GameManager_ClickUnit` с unit1 != unit2).
    let _ = click(&mut app, point);
    assert_eq!(app.hud().selected, Some(id));
    assert!(app.unit_menu().is_none(), "первый клик меню не открывает");

    // Второй клик по тому же юниту — меню команд.
    let _ = click(&mut app, point);
    let menu = app.unit_menu().expect("меню открыто повторным кликом");
    assert_eq!(menu.unit, id);
    assert!(!menu.items.is_empty());
}

#[test]
fn menu_items_follow_unit_type() {
    let Some(mut app) = game_app() else {
        return;
    };

    // Танк: `attack` (не портирован) → `enter` → `sentry` → `Done`.
    let tank = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, tank);

    let menu = app.unit_menu().expect("меню танка");
    let ids: Vec<ItemId> = menu.items.iter().map(|item| item.id).collect();

    assert!(ids.contains(&ItemId::Sentry), "танк умеет sentry");
    assert!(ids.contains(&ItemId::Enter));
    assert!(ids.contains(&ItemId::Done));
    assert!(!ids.contains(&ItemId::Build), "танк не строит");
    assert!(!ids.iter().any(|id| *id == ItemId::Load));

    let _ = key(&mut app, Key::Escape);

    // Строитель: `Build` первым пунктом.
    let builder = spawn(&mut app, UnitType::Constructor);
    open_menu(&mut app, builder);
    let menu = app.unit_menu().expect("меню строителя");
    assert_eq!(menu.items[0].id, ItemId::Build);
    assert!(menu.items[0].enabled);
    let _ = key(&mut app, Key::Escape);

    // Транспорт: загрузка (не портирована — пункт есть, но серый).
    let transport = spawn(&mut app, UnitType::PersonnelCarrier);
    open_menu(&mut app, transport);
    let menu = app.unit_menu().expect("меню транспорта");
    let load = menu
        .index_of(ItemId::Load)
        .map(|index| &menu.items[index])
        .expect("load у транспорта");
    assert!(!load.enabled, "погрузка не портирована");
}

#[test]
fn escape_closes_menu_and_does_not_leave_session() {
    let Some(mut app) = game_app() else {
        return;
    };

    let tank = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, tank);

    let transition = key(&mut app, Key::Escape);
    assert_eq!(transition, Transition::None, "первый Esc закрывает только меню");
    assert!(app.unit_menu().is_none());
    assert_eq!(app.hud().selected, Some(tank), "выделение сохраняется");

    // Второй Esc — обычное поведение сессии (снять выделение).
    let _ = key(&mut app, Key::Escape);
    assert_eq!(app.hud().selected, None);
}

#[test]
fn menu_is_modal_for_the_map() {
    let Some(mut app) = game_app() else {
        return;
    };

    let tank = spawn(&mut app, UnitType::Tank);
    let other = spawn(&mut app, UnitType::Scout);
    open_menu(&mut app, tank);

    // Клик по другому своему юниту: меню закрывается, выделение не меняется,
    // приказов не появилось (C++: попап модален для карты).
    let point = screen_point(&app, app.state().unit(other).expect("scout").grid);
    let _ = click(&mut app, point);

    assert!(app.unit_menu().is_none(), "клик вне меню закрывает его");
    assert_eq!(app.hud().selected, Some(tank), "карта приказ не получила");
    assert!(app.hud().pending.is_empty(), "приказов нет");

    // И повторный клик по танку снова открывает меню (не перевыделяет).
    open_menu(&mut app, tank);
    assert!(app.unit_menu().is_some());
}

#[test]
fn hotkey_and_click_activate_sentry() {
    let Some(mut app) = game_app() else {
        return;
    };

    let tank = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, tank);

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Sentry))
        .expect("sentry в меню");
    let rect = app.unit_menu().expect("меню").button_rect(index);

    // Клик по кнопке пункта — как настоящая мышь.
    let _ = click(&mut app, (rect.ulx + rect.width / 2, rect.uly + rect.height / 2));

    assert!(app.unit_menu().is_none(), "пункт закрывает меню");
    let unit = app.state().unit(tank).expect("танк");
    assert_eq!(unit.orders, UnitOrder::Sentry);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);

    // Повторное открытие: пункт `sentry` нажат (`state[i]` в C++).
    open_menu(&mut app, tank);
    let menu = app.unit_menu().expect("меню");
    let sentry = &menu.items[menu.index_of(ItemId::Sentry).expect("sentry")];
    assert!(sentry.pressed, "sentry уже включён — нажатый вид");

    // Горячая клавиша '8' снимает sentry (`ORDER_AWAIT`).
    let _ = key(&mut app, Key::Char('8'));
    assert_eq!(
        app.state().unit(tank).expect("танк").orders,
        UnitOrder::Await
    );
}

#[test]
fn build_item_opens_build_menu() {
    let Some(mut app) = game_app() else {
        return;
    };

    let builder = spawn(&mut app, UnitType::Constructor);
    open_menu(&mut app, builder);

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Build))
        .expect("Build в меню");
    let rect = app.unit_menu().expect("меню").button_rect(index);
    let _ = click(&mut app, (rect.ulx + 1, rect.uly + 1));

    assert!(app.unit_menu().is_none(), "меню закрылось");
    assert!(app.build_menu().is_some(), "Build открыл BuildMenu");
}

#[test]
fn disabled_item_does_not_act() {
    let Some(mut app) = game_app() else {
        return;
    };

    let tank = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, tank);

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Attack))
        .expect("attack в меню");
    let rect = app.unit_menu().expect("меню").button_rect(index);
    let _ = click(&mut app, (rect.ulx + 1, rect.uly + 1));

    assert!(
        app.unit_menu().is_some(),
        "отключённый пункт не закрывает меню и не срабатывает"
    );
    assert!(app.hud().pending.is_empty());
}

#[test]
fn frame_shows_menu_over_map() {
    let Some(mut app) = game_app() else {
        return;
    };

    let tank = spawn(&mut app, UnitType::Tank);
    let before = app.render_frame();
    open_menu(&mut app, tank);

    let rect = app.unit_menu().expect("меню").rect;
    let after = app.render_frame();
    let mut changed = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            if before.get(x, y) != after.get(x, y) {
                changed += 1;
            }
        }
    }

    assert!(
        changed > rect.width * rect.height / 4,
        "меню занимает заметную часть своего прямоугольника (изменилось {changed} пикселей)"
    );
}

#[test]
fn unit_menu_actions_are_orders_on_the_wire() {
    // Приказы меню идут тем же путём, что остальные (пакет 08): проверяем
    // уровни процессоров C++ (`Remote_Init`).
    use max_core::enums::{UnitOrder, UnitOrderState};
    use max_core::input::PayloadLevel;

    assert_eq!(
        PayloadLevel::for_order(UnitOrder::Sentry),
        PayloadLevel::P1,
        "sentry — процессор P1"
    );
    assert_eq!(
        PayloadLevel::for_order(UnitOrder::Await),
        PayloadLevel::P3,
        "stop/done — ORDER_AWAIT, P3"
    );
    assert_eq!(
        PayloadLevel::for_order(UnitOrder::Upgrade),
        PayloadLevel::P2,
        "upgrade — P2 (родитель = сам юнит)"
    );
    assert_eq!(
        PayloadLevel::for_order(UnitOrder::Repair),
        PayloadLevel::P2,
        "repair — P2 (родитель = цель)"
    );
    assert_eq!(
        PayloadLevel::for_order(UnitOrder::Reload),
        PayloadLevel::P2
    );
    assert_eq!(
        PayloadLevel::for_order(UnitOrder::MoveToUnit),
        PayloadLevel::P3,
        "enter — MOVE_TO_UNIT, P3"
    );
    assert_eq!(
        PayloadLevel::for_order(UnitOrder::LayMine),
        PayloadLevel::P1
    );
    assert_eq!(
        PayloadLevel::for_order(UnitOrder::Transfer),
        PayloadLevel::P3,
        "x-fer в протоколе есть (P3.transfer_cargo), но UI переноса не портирован"
    );
    let _ = UnitOrderState::Init;
}

#[test]
fn cursor_mode_repairs_own_unit() {
    let Some(mut app) = game_app() else {
        return;
    };

    let repair = spawn(&mut app, UnitType::RepairUnit);
    app.state_mut().unit_mut(repair).expect("repair").storage = 2;
    let tank = spawn(&mut app, UnitType::Tank);

    open_menu(&mut app, repair);
    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Repair))
        .expect("repair в меню");
    let rect = app.unit_menu().expect("меню").button_rect(index);
    let _ = click(&mut app, (rect.ulx + 1, rect.uly + 1));

    assert_eq!(app.hud().cursor_mode, unitmenu::CURSOR_REPAIR);

    // Клик по своему танку — приказ `ORDER_REPAIR` с родителем-танком.
    let point = screen_point(&app, app.state().unit(tank).expect("танк").grid);
    let _ = click(&mut app, point);

    let unit = app.state().unit(repair).expect("repair");
    assert_eq!(unit.orders, UnitOrder::Repair);
    assert_eq!(unit.parent_unit, Some(tank));
    // C++ не сбрасывает `unit->cursor` после клика — режим остаётся, пока
    // игрок не выключит его пунктом меню (или не сменит выделение).
    assert_eq!(app.hud().cursor_mode, unitmenu::CURSOR_REPAIR);

    // Пункт меню выключает режим (C++ `cursor = 0`): юнит всё ещё выделен,
    // поэтому одиночный клик по нему снова открывает меню.
    let repair_grid = app.state().unit(repair).expect("repair").grid;
    let point = screen_point(&app, repair_grid);
    let _ = click(&mut app, point);
    assert!(app.unit_menu().is_some(), "меню открылось повторным кликом");

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Repair))
        .expect("repair в меню");
    let rect = app.unit_menu().expect("меню").button_rect(index);
    let _ = click(&mut app, (rect.ulx + 1, rect.uly + 1));
    assert_eq!(app.hud().cursor_mode, 0, "повторный пункт выключает режим");
}

#[test]
fn enter_mode_moves_to_unit() {
    let Some(mut app) = game_app() else {
        return;
    };

    let tank = spawn(&mut app, UnitType::Tank);
    let transport = spawn(&mut app, UnitType::PersonnelCarrier);

    open_menu(&mut app, tank);
    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Enter))
        .expect("enter в меню");
    let rect = app.unit_menu().expect("меню").button_rect(index);
    let _ = click(&mut app, (rect.ulx + 1, rect.uly + 1));

    assert!(app.hud().enter_mode, "enter включил режим посадки");

    let transport_grid = app.state().unit(transport).expect("транспорт").grid;
    let point = screen_point(&app, transport_grid);
    let _ = click(&mut app, point);

    let unit = app.state().unit(tank).expect("танк");
    assert_eq!(unit.orders, UnitOrder::MoveToUnit);
    assert_eq!(app.state().order_data(tank).move_to, transport_grid);
    assert!(!app.hud().enter_mode, "C++ сбрасывает enter_mode после клика");
}

#[test]
fn done_item_deselects_and_keeps_game_running() {
    let Some(mut app) = game_app() else {
        return;
    };

    let tank = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, tank);

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(ItemId::Done))
        .expect("Done в меню");
    let rect = app.unit_menu().expect("меню").button_rect(index);
    let transition = click(&mut app, (rect.ulx + 1, rect.uly + 1));

    assert_eq!(transition, Transition::None);
    assert!(app.unit_menu().is_none(), "пункт закрывает меню");
    assert_eq!(
        app.hud().selected,
        None,
        "C++ `UnitsManager_PerformAction` снимает выделение (`auto_select`)"
    );
}
