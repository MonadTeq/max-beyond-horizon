//! Режим выбора места стройки (`ORDER_STATE_SELECT_SITE`) — сквозные тесты
//! через ввод игрока: призрак-лента, валидность клетки, LMB/Esc, движение
//! строителя в центр площадки (`PositionInTape`).
//!
//! Оракул: `buildmenu.cpp:921` (`MobileBuildMenu::Build` → `SELECT_SITE` +
//! `GameManager_TempTape`), `game_manager.cpp:4566` (`PathBuild`),
//! `game_manager.cpp:5829` (`GetBuilderUnitCursor`),
//! `unitinfo.cpp:4414` (`PositionInTape`).
//!
//! Тесты пропускаются, если данных игры нет рядом с репозиторием (как
//! `unit_menu.rs`/`construction_site.rs`).

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::{handle_session_input, App, AppConfig, InputEvent, Transition};
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

fn app_with_assets() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("build_site: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"));
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
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

/// Партия через настоящие фазы старта (мобилизация → высадка → `InGame`).
fn game_app() -> Option<App> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() {
        eprintln!("build_site: пропуск — нет SNOW_1.WRL");
        return None;
    }

    let mut flow = max_app::pregame::PreGameFlow::start(
        max_app::setup::GameSetupForm::default(),
        single_human(),
        &world_dir,
    );
    flow.commit_mobilization();
    flow.commit_landing();

    let config = flow.setup.new_game_config();
    let surface =
        max_app::landing::LandingScreen::load_surface(0, &world_dir).expect("WRL-поверхность");

    let mut app = app_with_assets()?;
    app.new_game_on_surface(&config, &surface);
    app.tick();

    Some(app)
}

/// Конструктор RED в клетке, вокруг которой свободны 2×2 (иначе
/// `UnitsManager_IssueBuildOrder` не даст поставить здание).
fn spawn_builder(app: &mut App) -> (UnitId, Point) {
    let land = max_core::enums::Surface::LAND.bits();
    let free = |app: &App, cell: Point| {
        app.state().units_at(cell).is_empty()
            && app.state().map.surface_at(cell.x, cell.y).bits() & land != 0
    };
    let mut spot = None;

    'search: for radius in 1..14 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(22 + dx, 22 + dy);
                let clear =
                    (0..2).all(|y| (0..2).all(|x| free(app, Point::new(grid.x + x, grid.y + y))));

                if clear {
                    spot = Some(grid);
                    break 'search;
                }
            }
        }
    }

    let grid = spot.expect("свободная площадка 2×2 на карте");
    let flags = max_core::access::unit_type_flags(UnitType::Constructor);
    let id = app
        .state_mut()
        .spawn_unit(UnitType::Constructor, TeamId::RED, grid, flags)
        .expect("конструктор появился");

    app.viewport_mut().center_on_grid(grid.x, grid.y);

    (id, grid)
}

/// Кадровая точка центра клетки (окно 640×480 с хромом).
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

/// Индекс `TrainingHall` в списке конструктора (`BUILDABLE`), здание 2×2.
fn training_hall_index() -> usize {
    max_core::production::buildable_units(UnitType::Constructor)
        .iter()
        .position(|unit_type| *unit_type == UnitType::TrainingHall)
        .expect("Training Hall в списке конструктора")
}

/// Выделяет строителя и открывает для него `BuildMenu`.
fn open_build_menu(app: &mut App, builder: UnitId) {
    app.hud_mut().select(Some(builder));
    assert!(app.open_build_menu(), "BuildMenu открылся");
    assert!(
        app.build_menu_select(training_hall_index()),
        "выбран Training Hall"
    );
}

/// `Done`: для здания у мобильного строителя открывается выбор места.
fn order_site_selection(app: &mut App, builder: UnitId) -> Point {
    open_build_menu(app, builder);
    let line = app.commit_build_menu(false).expect("Done обработан");
    assert!(app.build_site().is_some(), "выбор места открыт ({line})");
    assert!(
        app.state().unit(builder).expect("строитель").orders == UnitOrder::Await,
        "до подтверждения клетки приказ не отдан"
    );

    app.build_site().expect("режим выбора").anchor
}

/// Наводит мышь на клетку кадра (как игрок) — `GameManager_GetBuilderUnitCursor`.
fn move_cursor(app: &mut App, grid: Point) -> Transition {
    let (x, y) = screen_point(app, grid);

    handle_session_input(app, InputEvent::MouseMove(x, y), false)
}

/// Клик по клетке (LMB — подтверждение, невалидная — отмена как в C++).
fn click_cell(app: &mut App, grid: Point) -> Transition {
    let (x, y) = screen_point(app, grid);

    handle_session_input(app, InputEvent::MouseClick(x, y), false)
}

/// Открывает `BuildMenu` клавишей `B` — быстрый путь (`ui.rs`).
fn open_build_menu_via_key(app: &mut App, builder: UnitId) {
    app.hud_mut().select(Some(builder));

    handle_session_input(app, InputEvent::Key(max_app::ui::Key::Char('b')), false);

    assert!(app.build_menu().is_some(), "B открывает BuildMenu");
    assert!(app.unit_menu().is_none(), "попап не открывался");
}

/// Открывает `BuildMenu` через попап юнита — путь игрока
/// (`GameManager_ClickUnit` → `GameManager_InitPopupButtons` → пункт `Build`).
///
/// Клик по уже выделенному юниту открывает попап, затем клик по кнопке
/// `Build` (`unitmenu::ItemId::Build`) — `open_build_menu`.
fn open_build_menu_via_popup(app: &mut App, builder: UnitId) {
    app.hud_mut().select(Some(builder));
    let grid = app.state().unit(builder).expect("строитель").grid;
    let (x, y) = screen_point(app, grid);

    let _ = handle_session_input(app, InputEvent::MouseMove(x, y), false);
    let _ = handle_session_input(app, InputEvent::MouseClick(x, y), false);

    assert!(app.unit_menu().is_some(), "попап юнита открыт");

    let rect = {
        let menu = app.unit_menu().expect("попап");
        let index = menu
            .index_of(max_app::unitmenu::ItemId::Build)
            .expect("пункт Build в попапе строителя");

        menu.button_rect(index)
    };

    let _ = handle_session_input(
        app,
        InputEvent::MouseClick(rect.ulx + rect.width / 2, rect.uly + rect.height / 2),
        false,
    );

    assert!(app.unit_menu().is_none(), "попап закрылся после Build");
    assert!(
        app.build_menu().is_some(),
        "Build в попапе открывает BuildMenu"
    );
}

/// Выбор места через указанный путь открытия `BuildMenu`.
///
/// `popup == false` — клавиша `B`; `popup == true` — пункт `Build` попапа.
fn order_site_selection_via(app: &mut App, builder: UnitId, popup: bool) -> Point {
    if popup {
        open_build_menu_via_popup(app, builder);
    } else {
        open_build_menu_via_key(app, builder);
    }

    assert!(
        app.build_menu_select(training_hall_index()),
        "выбран Training Hall"
    );

    let line = app.commit_build_menu(false).expect("Done обработан");
    assert!(app.build_site().is_some(), "выбор места открыт ({line})");
    assert!(
        app.state().unit(builder).expect("строитель").orders == UnitOrder::Await,
        "до подтверждения клетки приказ не отдан"
    );

    app.build_site().expect("режим выбора").anchor
}

#[test]
fn ghost_cursor_moves_with_the_mouse_and_marks_validity() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (builder, builder_grid) = spawn_builder(&mut app);

    let anchor = order_site_selection(&mut app, builder);
    assert_eq!(anchor, builder_grid, "первый якорь C++ — клетка строителя");
    assert!(app.build_site().expect("режим").valid(app.state()));

    // Курсор левее/выше строителя уводит призрак 2×2 на соседний якорь.
    let target = Point::new(builder_grid.x - 1, builder_grid.y - 1);
    let _ = move_cursor(&mut app, target);

    let site = app.build_site().expect("режим");
    assert_eq!(site.anchor, target, "призрак следует за курсором");
    assert_eq!(site.cursor, target);
    assert!(site.valid(app.state()), "свободная суша — валидно");
    assert_eq!(
        site.cursor_id(app.state()),
        max_app::cursor::CursorId::Map2,
        "CURSOR_MAP2 над валидной клеткой"
    );

    // Вода под 2×2 — невалидно (`CURSOR_UNIT_NO_GO`).
    for dy in 0..2 {
        for dx in 0..2 {
            *app.state_mut()
                .map
                .surface
                .tiles_mut()
                .get_mut(
                    (i32::from(target.y) + dy) as usize
                        * usize::try_from(max_core::geo::MAP_WIDTH).unwrap()
                        + (i32::from(target.x) + dx) as usize,
                )
                .unwrap() = max_core::enums::Surface::WATER;
        }
    }
    assert!(!app.build_site().expect("режим").valid(app.state()));
    assert_eq!(
        app.build_site().expect("режим").cursor_id(app.state()),
        max_app::cursor::CursorId::UnitNoGo
    );
}

#[test]
fn ghost_is_drawn_in_the_frame_and_esc_cancels_without_an_order() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (builder, builder_grid) = spawn_builder(&mut app);

    // Кадр без режима — эталон для сравнения.
    let before = app.render_frame();
    let anchor = order_site_selection(&mut app, builder);
    let after = app.render_frame();

    assert_eq!(anchor, builder_grid);

    // Призрак — та же декаль `LRGTAPE`, что C++ спавнит в `GameManager_TempTape`:
    // сравнение кадров различает её пиксели в клетке якоря.
    let (cx, cy) = screen_point(&app, anchor);
    let changed = count_changed_in_rect(&before, &after, cx - 64, cy - 64, 128, 128);
    assert!(
        changed > 100,
        "призрак площадки нарисован в кадре ({changed} px)"
    );

    // Esc — отмена без заказа (приказ не уходит, площадки в мире нет).
    let _ = handle_session_input(&mut app, InputEvent::Key(max_app::ui::Key::Escape), false);

    assert!(app.build_site().is_none(), "режим закрыт");
    assert_eq!(
        app.state().unit(builder).expect("строитель").orders,
        UnitOrder::Await,
        "приказ не отдан"
    );
    assert!(
        max_core::production::construction_utility(app.state(), TeamId::RED, anchor).is_none(),
        "площадки в мире нет"
    );
}

#[test]
fn lmb_places_the_site_in_the_chosen_cell_and_the_builder_works_at_its_center() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (builder, builder_grid) = spawn_builder(&mut app);

    place_and_assert_centered(&mut app, builder, builder_grid, false);
}

/// Тот же сценарий, но `BuildMenu` открыт пользовательским путём — пунктом
/// `Build` попапа юнита (`unitmenu`), а не клавишей `B`.
#[test]
fn popup_build_places_the_site_and_centers_the_builder() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (builder, builder_grid) = spawn_builder(&mut app);

    place_and_assert_centered(&mut app, builder, builder_grid, true);
}

/// Призрак нарисован в кадре после `Done` независимо от пути открытия
/// `BuildMenu` (клавиша `B` или попап юнита).
#[test]
fn ghost_is_drawn_after_both_build_menu_paths() {
    for popup in [false, true] {
        let Some(mut app) = game_app() else {
            return;
        };
        // Кадр 640×480 с загруженным `FRAMEPIC` идёт через
        // `render_with_chrome` — призрак обязан рисоваться именно там.
        assert!(app.chrome_active(), "кадр идёт через render_with_chrome");
        let (builder, _builder_grid) = spawn_builder(&mut app);
        let before = app.render_frame();

        let anchor = order_site_selection_via(&mut app, builder, popup);
        let after = app.render_frame();

        let (cx, cy) = screen_point(&app, anchor);
        let changed = count_changed_in_rect(&before, &after, cx - 64, cy - 64, 128, 128);

        assert!(
            changed > 100,
            "призрак площадки нарисован в кадре (путь {}, {changed} px)",
            if popup { "попап" } else { "B" }
        );
    }
}

/// Общий сценарий: выбор места → LMB → 5 тиков → строитель в центре ленты.
fn place_and_assert_centered(app: &mut App, builder: UnitId, builder_grid: Point, popup: bool) {
    order_site_selection_via(app, builder, popup);

    // Игрок указывает на клетку левее/выше строителя: площадка ставится
    // ТУДА (2×2, накрывающий эту клетку), а не в автоматически найденное место.
    let chosen = Point::new(builder_grid.x - 1, builder_grid.y - 1);
    let _ = move_cursor(app, chosen);
    assert_eq!(app.build_site().expect("режим").anchor, chosen);

    let _ = click_cell(app, chosen);

    assert!(app.build_site().is_none(), "LMB подтвердил клетку");
    assert_eq!(
        app.state().unit(builder).expect("строитель").orders,
        UnitOrder::Build,
        "ORDER_BUILD с клеткой площадки"
    );

    // Площадку ставит `UnitInfo::StartBuilding` на следующем тике.
    app.tick();
    let site = max_core::production::construction_utility(app.state(), TeamId::RED, chosen)
        .expect("лента площадки в выбранной клетке");

    assert_eq!(
        app.state().unit(site).expect("лента").grid,
        chosen,
        "площадка ровно в клетке, куда указал игрок"
    );
    assert_eq!(
        app.state().unit(builder).expect("строитель").grid,
        builder_grid,
        "сетка строителя не меняется (`PositionInTape` двигает только спрайт)"
    );
    assert!(
        max_core::production::site_covers(
            UnitType::TrainingHall,
            chosen,
            app.state().unit(builder).expect("строитель").grid
        ),
        "якорь 2×2 всегда накрывает клетку строителя (C++ GetBuilderUnitCursor)"
    );

    // `PositionInTape` + `OffsetDrawZones`: после 4 тиков спрайт стоит в центре
    // ленты 2×2 — 32 px (половина клетки) от центра своей клетки. Первый тик
    // после подтверждения — `StartBuilding` (`AWAIT_TAPE_POSITIONING`/`INIT`),
    // затем `INIT → ENTER` и четыре шага `moved`.
    app.tick();
    app.tick();
    app.tick();
    app.tick();
    app.tick();

    let unit = app.state().unit(builder).expect("строитель").clone();
    let offset = max_app::buildsite::tape_center_offset(app.state(), &unit);
    let center = max_app::buildsite::site_center_world(UnitType::TrainingHall, chosen);

    assert_eq!(offset, (-32, -32), "сдвиг 8 px × 4 тика к центру ленты");
    assert_eq!(
        (
            i32::from(unit.grid.x) * 64 + 32 + offset.0,
            i32::from(unit.grid.y) * 64 + 32 + offset.1
        ),
        center,
        "спрайт строителя — ровно в центре площадки"
    );
    assert_eq!(
        app.state().unit(builder).expect("строитель").state,
        max_core::enums::UnitOrderState::BuildInProgress,
        "после позиционирования — рабочий кадр"
    );

    // Кадровый тест: пиксели спрайта строителя стоят в центре площадки.
    // Кадр со строителем против кадра без него (видимость команде снята) —
    // центр различающихся пикселей должен совпасть с центром 2×2.
    let with_builder = app.render_frame();
    app.state_mut()
        .unit_mut(builder)
        .expect("строитель")
        .set_visible_to_team(TeamId::RED, false);
    let without_builder = app.render_frame();
    app.state_mut()
        .unit_mut(builder)
        .expect("строитель")
        .set_visible_to_team(TeamId::RED, true);

    let bounds = changed_bounds(&with_builder, &without_builder, MAIN_MAP)
        .expect("строитель нарисован на карте");
    let viewport = app.map_viewport();
    let (cx, cy) = viewport.world_to_screen(center.0, center.1);
    let (cx, cy) = (cx + MAIN_MAP.ulx, cy + MAIN_MAP.uly);
    let (bx, by) = ((bounds.0 + bounds.2) / 2, (bounds.1 + bounds.3) / 2);

    assert!(
        (cx - bx).abs() <= 8 && (cy - by).abs() <= 8,
        "центр спрайта строителя ({bx},{by}) должен быть в центре ленты ({cx},{cy})"
    );
}

/// Границы (x0,y0,x1,y1) различающихся пикселей двух кадров внутри `area`.
fn changed_bounds(
    left: &max_render::buffer::RgbaBuffer,
    right: &max_render::buffer::RgbaBuffer,
    area: max_render::buffer::PixelRect,
) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;

    for y in area.uly..area.lry() {
        for x in area.ulx..area.lrx() {
            let (Some(a), Some(b)) = (left.get(x, y), right.get(x, y)) else {
                continue;
            };

            if a.r.abs_diff(b.r) > 30 || a.g.abs_diff(b.g) > 30 || a.b.abs_diff(b.b) > 30 {
                bounds = Some(match bounds {
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    None => (x, y, x, y),
                });
            }
        }
    }

    bounds
}

#[test]
fn lmb_on_an_invalid_cell_cancels_the_selection() {
    let Some(mut app) = game_app() else {
        return;
    };
    let (builder, builder_grid) = spawn_builder(&mut app);

    order_site_selection(&mut app, builder);

    // Уводим призрак на занятую/непроходимую клетку и убеждаемся, что она
    // невалидна (в C++ — `CURSOR_UNIT_NO_GO`).
    let mut target = Point::new(builder_grid.x + 1, builder_grid.y);
    let mut attempts = 0;

    while app.build_site().expect("режим").valid(app.state()) && attempts < 8 {
        attempts += 1;
        target = Point::new(builder_grid.x + 1, builder_grid.y + attempts);
        let _ = move_cursor(&mut app, target);
    }

    if app.build_site().expect("режим").valid(app.state()) {
        // Вся округа свободна — сценарий нечего проверять (карта генератора).
        return;
    }

    let _ = click_cell(&mut app, target);

    // C++ `ManagePlayerAction`: клик по невалидной клетке отменяет выбор.
    assert!(app.build_site().is_none(), "выбор отменён");
    assert_eq!(
        app.state().unit(builder).expect("строитель").orders,
        UnitOrder::Await,
        "приказ не отдан"
    );
    assert!(
        app.state()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| app.state().unit(id))
            .all(
                |unit| !matches!(unit.unit_type, UnitType::LargeTape | UnitType::SmallTape)
                    || unit.team != TeamId::RED
            ),
        "площадки в мире нет"
    );
}

#[test]
fn same_input_gives_the_same_site_and_digest() {
    let Some(mut first) = game_app() else {
        return;
    };
    let Some(mut second) = game_app() else {
        return;
    };

    let run = |app: &mut App| {
        let (builder, builder_grid) = spawn_builder(app);
        order_site_selection(app, builder);

        let chosen = Point::new(builder_grid.x - 1, builder_grid.y - 1);
        let _ = move_cursor(app, chosen);
        let _ = click_cell(app, chosen);

        app.tick();

        (
            chosen,
            app.digest().state_digest,
            max_core::production::construction_utility(app.state(), TeamId::RED, chosen),
            app.state()
                .unit(builder)
                .map(|unit| (unit.grid, unit.orders, unit.state)),
        )
    };

    let left = run(&mut first);
    let right = run(&mut second);

    assert_eq!(left, right, "та же последовательность — тот же результат");
}

/// Сколько пикселей области различаются (любой канал больше порога).
fn count_changed_in_rect(
    left: &max_render::buffer::RgbaBuffer,
    right: &max_render::buffer::RgbaBuffer,
    x0: i32,
    y0: i32,
    width: i32,
    height: i32,
) -> u32 {
    let mut changed = 0;

    for y in y0..(y0 + height) {
        for x in x0..(x0 + width) {
            let a = left.get(x, y);
            let b = right.get(x, y);

            if let (Some(a), Some(b)) = (a, b) {
                if a.r.abs_diff(b.r) > 30 || a.g.abs_diff(b.g) > 30 || a.b.abs_diff(b.b) > 30 {
                    changed += 1;
                }
            }
        }
    }

    changed
}
