//! Build site selection mode (`ORDER_STATE_SELECT_SITE`) — end-to-end tests
//! through player input: the ghost tape, cell validity, LMB/Esc, the builder
//! moving to the site center (`PositionInTape`).
//!
//! Oracle: `buildmenu.cpp:921` (`MobileBuildMenu::Build` → `SELECT_SITE` +
//! `GameManager_TempTape`), `game_manager.cpp:4566` (`PathBuild`),
//! `game_manager.cpp:5829` (`GetBuilderUnitCursor`),
//! `unitinfo.cpp:4414` (`PositionInTape`).
//!
//! The tests require the game data next to the repository (like
//! `unit_menu.rs`/`construction_site.rs`); a missing data set fails the test.

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
    let patches = repo_path("tests/fixtures/assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("build_site: нет данных — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"));
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

/// A match through the real start phases (mobilization → landing → `InGame`).
fn game_app() -> Option<App> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() {
        eprintln!("build_site: нет данных — нет SNOW_1.WRL");
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

/// A RED constructor in a cell with a free 2×2 around it (otherwise
/// `UnitsManager_IssueBuildOrder` will not allow placing the building).
fn spawn_builder(app: &mut App) -> (UnitId, Point) {
    let land = max_core::enums::Surface::LAND.bits();
    let free = |app: &App, cell: Point| {
        app.state_ref().units_at(cell).is_empty()
            && app.state_ref().map().surface_at(cell.x, cell.y).bits() & land != 0
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
        .universe_mut()
        .spawn_unit(UnitType::Constructor, TeamId::RED, grid, flags)
        .expect("конструктор появился");

    app.viewport_mut().center_on_grid(grid.x, grid.y);

    (id, grid)
}

/// Frame point of the cell center (640×480 window with chrome).
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

/// The `TrainingHall` index in the constructor's list (`BUILDABLE`), a 2×2 building.
fn training_hall_index() -> usize {
    max_core::production::buildable_units(UnitType::Constructor)
        .iter()
        .position(|unit_type| *unit_type == UnitType::TrainingHall)
        .expect("Training Hall в списке конструктора")
}

/// Selects the builder and opens `BuildMenu` for it.
fn open_build_menu(app: &mut App, builder: UnitId) {
    app.hud_mut().select(Some(builder));
    assert!(app.open_build_menu(), "BuildMenu открылся");
    assert!(
        app.build_menu_select(training_hall_index()),
        "выбран Training Hall"
    );
}

/// `Done`: for a building at a mobile builder the site selection opens.
fn order_site_selection(app: &mut App, builder: UnitId) -> Point {
    open_build_menu(app, builder);
    let line = app.commit_build_menu(max_app::SessionChannel::Local).expect("Done обработан");
    assert!(app.build_site().is_some(), "выбор места открыт ({line})");
    assert!(
        app.state_ref().unit(builder).expect("строитель").orders == UnitOrder::Await,
        "до подтверждения клетки приказ не отдан"
    );

    app.build_site().expect("режим выбора").anchor
}

/// Moves the mouse onto a frame cell (like a player) — `GameManager_GetBuilderUnitCursor`.
fn move_cursor(app: &mut App, grid: Point) -> Transition {
    let (x, y) = screen_point(app, grid);

    handle_session_input(app, InputEvent::MouseMove(x, y))
}

/// A click on a cell (LMB — confirmation, invalid — cancel as in C++).
fn click_cell(app: &mut App, grid: Point) -> Transition {
    let (x, y) = screen_point(app, grid);

    handle_session_input(app, InputEvent::MouseClick(x, y))
}

/// Opens `BuildMenu` with the `B` key — a shortcut (`ui.rs`).
fn open_build_menu_via_key(app: &mut App, builder: UnitId) {
    app.hud_mut().select(Some(builder));

    handle_session_input(app, InputEvent::Key(max_app::ui::Key::Char('b')));

    assert!(app.build_menu().is_some(), "B открывает BuildMenu");
    assert!(app.unit_menu().is_none(), "попап не открывался");
}

/// Opens `BuildMenu` through the unit popup — the player path
/// (`GameManager_ClickUnit` → `GameManager_InitPopupButtons` → the `Build` item).
///
/// A click on the already selected unit opens the popup, then a click on the
/// `Build` (`unitmenu::ItemId::Build`) — `open_build_menu`.
fn open_build_menu_via_popup(app: &mut App, builder: UnitId) {
    app.hud_mut().select(Some(builder));
    let grid = app.state_ref().unit(builder).expect("строитель").grid;
    let (x, y) = screen_point(app, grid);

    let _ = handle_session_input(app, InputEvent::MouseMove(x, y));
    let _ = handle_session_input(app, InputEvent::MouseClick(x, y));

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
        InputEvent::MouseClick(rect.ulx + rect.width / 2, rect.uly + rect.height / 2)
    );

    assert!(app.unit_menu().is_none(), "попап закрылся после Build");
    assert!(
        app.build_menu().is_some(),
        "Build в попапе открывает BuildMenu"
    );
}

/// Site selection through the specified `BuildMenu` opening path.
///
/// `popup == false` — the `B` key; `popup == true` — the popup `Build` item.
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

    let line = app.commit_build_menu(max_app::SessionChannel::Local).expect("Done обработан");
    assert!(app.build_site().is_some(), "выбор места открыт ({line})");
    assert!(
        app.state_ref().unit(builder).expect("строитель").orders == UnitOrder::Await,
        "до подтверждения клетки приказ не отдан"
    );

    app.build_site().expect("режим выбора").anchor
}

/// Runs the body on a thread with a larger stack: in a debug build the `App`
/// scenes and the 640×480 frames need more than the 2 MiB test-thread stack.
fn with_big_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(body)
        .expect("test thread")
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

#[test]
fn ghost_cursor_moves_with_the_mouse_and_marks_validity() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
    let (builder, builder_grid) = spawn_builder(&mut app);

    let anchor = order_site_selection(&mut app, builder);
    assert_eq!(anchor, builder_grid, "первый якорь C++ — клетка строителя");
    assert!(app.build_site().expect("режим").valid(app.state_ref()));

    // The cursor left/above the builder moves the 2×2 ghost to the neighboring anchor.
    let target = Point::new(builder_grid.x - 1, builder_grid.y - 1);
    let _ = move_cursor(&mut app, target);

    let site = app.build_site().expect("режим");
    assert_eq!(site.anchor, target, "призрак следует за курсором");
    assert_eq!(site.cursor, target);
    assert!(site.valid(app.state_ref()), "свободная суша — валидно");
    assert_eq!(
        site.cursor_id(app.state_ref()),
        max_app::cursor::CursorId::Map2,
        "CURSOR_MAP2 над валидной клеткой"
    );

    // Water under the 2×2 — invalid (`CURSOR_UNIT_NO_GO`).
    for dy in 0..2 {
        for dx in 0..2 {
            *app.universe_mut()
                .map_mut()
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
    assert!(!app.build_site().expect("режим").valid(app.state_ref()));
    assert_eq!(
        app.build_site().expect("режим").cursor_id(app.state_ref()),
        max_app::cursor::CursorId::UnitNoGo
    );
}

#[test]
fn ghost_is_drawn_in_the_frame_and_esc_cancels_without_an_order() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
    let (builder, builder_grid) = spawn_builder(&mut app);

    // A frame without the mode — the reference for comparison.
    let before = app.render_frame();
    let anchor = order_site_selection(&mut app, builder);
    let after = app.render_frame();

    assert_eq!(anchor, builder_grid);

    // The ghost is the same `LRGTAPE` decal that C++ spawns in `GameManager_TempTape`:
    // the frame comparison distinguishes its pixels in the anchor cell.
    let (cx, cy) = screen_point(&app, anchor);
    let changed = count_changed_in_rect(&before, &after, cx - 64, cy - 64, 128, 128);
    assert!(
        changed > 100,
        "призрак площадки нарисован в кадре ({changed} px)"
    );

    // Esc — cancel without an order (the order does not go, there is no site in the world).
    let _ = handle_session_input(&mut app, InputEvent::Key(max_app::ui::Key::Escape));

    assert!(app.build_site().is_none(), "режим закрыт");
    assert_eq!(
        app.state_ref().unit(builder).expect("строитель").orders,
        UnitOrder::Await,
        "приказ не отдан"
    );
    assert!(
        app.state_ref().construction_utility( TeamId::RED, anchor).is_none(),
        "площадки в мире нет"
    );
}

#[test]
fn lmb_places_the_site_in_the_chosen_cell_and_the_builder_works_at_its_center() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
    let (builder, builder_grid) = spawn_builder(&mut app);

    place_and_assert_centered(&mut app, builder, builder_grid, false);
}

/// The same scenario, but `BuildMenu` is opened via the user path — the unit popup
/// `Build` item (`unitmenu`), not the `B` key.
#[test]
fn popup_build_places_the_site_and_centers_the_builder() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
    let (builder, builder_grid) = spawn_builder(&mut app);

    place_and_assert_centered(&mut app, builder, builder_grid, true);
}

/// The ghost is drawn in the frame after `Done` regardless of the `BuildMenu`
/// opening path (the `B` key or the unit popup).
#[test]
fn ghost_is_drawn_after_both_build_menu_paths() {
    for popup in [false, true] {
        let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
        // A 640×480 frame with `FRAMEPIC` loaded goes through
        // `render_with_chrome` — the ghost must be drawn exactly there.
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

/// Common scenario: site selection → LMB → 5 ticks → the builder in the tape center.
fn place_and_assert_centered(app: &mut App, builder: UnitId, builder_grid: Point, popup: bool) {
    order_site_selection_via(app, builder, popup);

    // The player points at a cell left/above the builder: the site is placed
    // THERE (2×2 covering this cell), not at an automatically found place.
    let chosen = Point::new(builder_grid.x - 1, builder_grid.y - 1);
    let _ = move_cursor(app, chosen);
    assert_eq!(app.build_site().expect("режим").anchor, chosen);

    let _ = click_cell(app, chosen);

    assert!(app.build_site().is_none(), "LMB подтвердил клетку");
    assert_eq!(
        app.state_ref().unit(builder).expect("строитель").orders,
        UnitOrder::Build,
        "ORDER_BUILD с клеткой площадки"
    );

    // The site is placed by `UnitInfo::StartBuilding` on the next tick.
    app.tick();
    let site = app.state_ref().construction_utility( TeamId::RED, chosen)
        .expect("лента площадки в выбранной клетке");

    assert_eq!(
        app.state_ref().unit(site).expect("лента").grid,
        chosen,
        "площадка ровно в клетке, куда указал игрок"
    );
    assert_eq!(
        app.state_ref().unit(builder).expect("строитель").grid,
        builder_grid,
        "сетка строителя не меняется (`PositionInTape` двигает только спрайт)"
    );
    assert!(
        max_core::production::site_covers(
            UnitType::TrainingHall,
            chosen,
            app.state_ref().unit(builder).expect("строитель").grid
        ),
        "якорь 2×2 всегда накрывает клетку строителя (C++ GetBuilderUnitCursor)"
    );

    // `PositionInTape` + `OffsetDrawZones`: first the builder turns toward the
    // tape (`Paths_UpdateAngle`), then four steps of 8 px put the sprite in
    // the 2×2 tape center — 32 px (half a cell) from its cell center. The first
    // tick after confirmation — `StartBuilding`
    // (`AWAIT_TAPE_POSITIONING`/`INIT`), then `INIT → ENTER`, the turn and four
    // `moved` steps; on the next tick `moved >= 4` → `RestoreOrders`.
    app.tick();
    app.tick();
    app.tick();
    app.tick();
    app.tick();
    app.tick();

    let unit = app.state_ref().unit(builder).expect("строитель").clone();
    let offset = max_app::buildsite::tape_center_offset(app.state_ref(), &unit);
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
        app.state_ref().unit(builder).expect("строитель").state,
        max_core::enums::UnitOrderState::BuildInProgress,
        "после позиционирования — рабочий кадр"
    );

    // Frame test: the builder sprite pixels stand in the site center.
    // A frame with the builder versus a frame without it (`IsRelevant` skips an
    // idle unit) — the center of the differing pixels must coincide with the
    // 2×2 center.
    let with_builder = app.render_frame();
    let without_builder = frame_without_unit(app, builder);

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

/// Frame without `unit`: the unit is temporarily made idle, which
/// `UnitInfoGroup::IsRelevant` excludes from the sprite loop
/// (`unitinfogroup.cpp:59`). `visible_to_team` no longer hides own units
/// (`presented_visible` keeps own-team units visible), so this is the way to
/// measure exactly the sprite pixels.
fn frame_without_unit(app: &mut App, unit: UnitId) -> max_render::buffer::RgbaBuffer {
    let saved = app
        .universe_mut()
        .unit_mut(unit)
        .expect("юнит для кадра без него")
        .orders;
    app.universe_mut()
        .unit_mut(unit)
        .expect("юнит для кадра без него")
        .orders = UnitOrder::Idle;
    let frame = app.render_frame();
    app.universe_mut()
        .unit_mut(unit)
        .expect("юнит для кадра без него")
        .orders = saved;

    frame
}

/// Center (x,y) of the builder sprite pixels in the frame: a frame with the unit versus a frame
/// with the unit idle. This measures exactly the sprite, without the tape markup.
fn builder_pixel_center(app: &mut App, builder: UnitId) -> (i32, i32) {
    let with_builder = app.render_frame();
    let without_builder = frame_without_unit(app, builder);

    let (x0, y0, x1, y1) = changed_bounds(&with_builder, &without_builder, MAIN_MAP)
        .expect("строитель нарисован на карте");

    ((x0 + x1) / 2, (y0 + y1) / 2)
}

/// Sliding at an angle: C++ turns the builder before driving into the tape
/// (`Paths_UpdateAngle` in `PositionInTape`), and `OffsetDrawZones` moves the sprite
/// along `DIRECTION_OFFSETS[angle] * 8` — diagonally/in the direction it looks, and
/// not "straight". "Before/after" frames: the angle and bbox of the sprite on each tick.
#[test]
fn builder_turns_to_the_tape_and_slides_diagonally_by_eight_pixels_per_tick() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
    let (builder, builder_grid) = spawn_builder(&mut app);

    // The tape anchor is one cell left/above the builder: it stands in the SE corner of the 2×2,
    // `GetTargetUnitAngle` gives NW(7), the turn 0 → 7 in one tick (8 directions).
    let chosen = Point::new(builder_grid.x - 1, builder_grid.y - 1);
    order_site_selection(&mut app, builder);
    let _ = move_cursor(&mut app, chosen);
    let _ = click_cell(&mut app, chosen);
    app.tick(); // `StartBuilding`: the site and `AWAIT_TAPE_POSITIONING`/`INIT`

    let initial_angle = app.state_ref().unit(builder).expect("строитель").angle;
    assert_eq!(initial_angle, 0, "конструктор смотрит на север");

    // The turn tick: `Paths_UpdateAngle` turns in place, `moved` = 0.
    app.tick();
    let unit = app.state_ref().unit(builder).expect("строитель").clone();
    assert_eq!(
        unit.state,
        max_core::enums::UnitOrderState::TapePositioningEnter
    );
    assert_eq!(unit.angle, 7, "строитель повернулся к центру ленты (NW)");
    assert_eq!(unit.moved, 0, "до доворота спрайт не сдвигается");
    assert_eq!(
        max_app::buildsite::tape_center_offset(app.state_ref(), &unit),
        (0, 0)
    );

    let turn_center = builder_pixel_center(&mut app, builder);
    let cell_center = screen_point(&app, builder_grid);

    // From the turn in place the sprite bbox stays in its cell (with a tolerance for
    // sprite/shadow asymmetry).
    assert!(
        (turn_center.0 - cell_center.0).abs() <= 16 && (turn_center.1 - cell_center.1).abs() <= 16,
        "после доворота спрайт ещё в своей клетке: {turn_center:?} vs {cell_center:?}"
    );

    // Four sliding ticks: exactly `DIRECTION_OFFSETS[NW] * 8 * step`.
    let mut centers = vec![turn_center];
    for step in 1..=4u8 {
        app.tick();

        let unit = app.state_ref().unit(builder).expect("строитель").clone();
        let offset = max_app::buildsite::tape_center_offset(app.state_ref(), &unit);
        assert_eq!(
            offset,
            (-8 * i32::from(step), -8 * i32::from(step)),
            "шаг {step}: сдвиг по диагонали NW"
        );
        assert_eq!(unit.grid, builder_grid, "сетка строителя не меняется");
        assert_eq!(unit.angle, 7, "на скольжении угол не меняется");

        centers.push(builder_pixel_center(&mut app, builder));
    }

    // The sprite center goes exactly diagonally: each tick −8 px on both axes
    // (the sprite does not rotate, so the bbox shifts entirely).
    for pair in centers.windows(2) {
        let (dx, dy) = (pair[1].0 - pair[0].0, pair[1].1 - pair[0].1);

        assert!(
            (dx + 8).abs() <= 1 && (dy + 8).abs() <= 1,
            "спрайт должен ехать по диагонали NW (−8,−8) за тик, а не прямо: {dx},{dy}"
        );
    }

    // A frame with completed positioning: the sprite in the center of the 2×2 tape.
    let site_center = max_app::buildsite::site_center_world(UnitType::TrainingHall, chosen);
    let viewport = app.map_viewport();
    let (sx, sy) = viewport.world_to_screen(site_center.0, site_center.1);
    let (sx, sy) = (sx + MAIN_MAP.ulx, sy + MAIN_MAP.uly);
    let (bx, by) = builder_pixel_center(&mut app, builder);

    assert!(
        (sx - bx).abs() <= 8 && (sy - by).abs() <= 8,
        "после четырёх шагов спрайт в центре ленты: ({bx},{by}) vs ({sx},{sy})"
    );

    // The next tick — `RestoreOrders` and a turn `(angle + 1) & 7`: NW(7) → N(0),
    // the shift remains full (the working pose in the tape center).
    app.tick();
    let unit = app.state_ref().unit(builder).expect("строитель").clone();
    assert_eq!(unit.orders, UnitOrder::Build);
    assert_eq!(unit.state, max_core::enums::UnitOrderState::BuildInProgress);
    assert_eq!(unit.angle, 0, "C++: Paths_UpdateAngle((angle + 1) & 7)");
    assert_eq!(
        max_app::buildsite::tape_center_offset(app.state_ref(), &unit),
        (-32, -32)
    );
}

/// Bounds (x0,y0,x1,y1) of the differing pixels of two frames inside `area`.
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
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
    let (builder, builder_grid) = spawn_builder(&mut app);

    order_site_selection(&mut app, builder);

    // We move the ghost onto an occupied/impassable cell and make sure that it
    // is invalid (in C++ — `CURSOR_UNIT_NO_GO`).
    let mut target = Point::new(builder_grid.x + 1, builder_grid.y);
    let mut attempts = 0;

    while app.build_site().expect("режим").valid(app.state_ref()) && attempts < 8 {
        attempts += 1;
        target = Point::new(builder_grid.x + 1, builder_grid.y + attempts);
        let _ = move_cursor(&mut app, target);
    }

    assert!(
        !app.build_site().expect("режим").valid(app.state_ref()),
        "no invalid cell found next to the builder: the scenario cannot check \
         that a click on it cancels the selection"
    );

    let _ = click_cell(&mut app, target);

    // C++ `ManagePlayerAction`: a click on an invalid cell cancels the selection.
    assert!(app.build_site().is_none(), "выбор отменён");
    assert_eq!(
        app.state_ref().unit(builder).expect("строитель").orders,
        UnitOrder::Await,
        "приказ не отдан"
    );
    assert!(
        app.state_ref()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| app.state_ref().unit(id))
            .all(
                |unit| !matches!(unit.unit_type, UnitType::LargeTape | UnitType::SmallTape)
                    || unit.team != TeamId::RED
            ),
        "площадки в мире нет"
    );
}

#[test]
fn same_input_gives_the_same_site_and_digest() {
    with_big_stack(|| {
        let mut first = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
        let mut second = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

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
                app.state_ref().construction_utility( TeamId::RED, chosen),
                app.state_ref()
                    .unit(builder)
                    .map(|unit| (unit.grid, unit.orders, unit.state)),
            )
        };

        let left = run(&mut first);
        let right = run(&mut second);

        assert_eq!(left, right, "та же последовательность — тот же результат");

    });
}

/// `GameManager_PathBuild` (`game_manager.cpp:4566-4598`): the engineer's `Path`
/// order attaches a **linear `GroundPath`** to the clicked cell (packet 38) and
/// starts the first object on its own cell (packet 08/P4 with `move_to` = builder
/// cell) — the queue head then repeats along the route
/// (`UnitsManager_BuildNext`, `units_manager.cpp:3807`).
#[test]
fn engineer_path_attaches_the_route_and_builds_at_its_own_cell() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    // A free horizontal strip of four land cells for the engineer and its route.
    let land = max_core::enums::Surface::LAND.bits();
    let home = Point::new(22, 22);
    let free = |app: &App, cell: Point| {
        app.state_ref().units_at(cell).is_empty()
            && app.state_ref().map().surface_at(cell.x, cell.y).bits() & land != 0
    };
    let mut start = None;

    'search: for dy in -12..=12 {
        for dx in -12..=12 {
            let grid = Point::new(home.x + dx, home.y + dy);
            let clear = (0..4).all(|offset| free(&app, Point::new(grid.x + offset, grid.y)));

            if clear {
                start = Some(grid);
                break 'search;
            }
        }
    }

    let grid = start.expect("свободная полоса суши");
    let engineer = app
        .universe_mut()
        .spawn_unit(
            UnitType::Engineer,
            TeamId::RED,
            grid,
            max_core::access::unit_type_flags(UnitType::Engineer),
        )
        .expect("инженер");
    app.universe_mut().unit_mut(engineer).expect("engineer").storage = 40;

    // `Path` button: `Build()` with `event_click_path_build` (1×1 site selection).
    app.hud_mut().select(Some(engineer));
    assert!(app.open_build_menu(), "BuildMenu открылся");
    let index = max_core::production::buildable_units(UnitType::Engineer)
        .iter()
        .position(|unit_type| *unit_type == UnitType::MaterialStorage)
        .expect("Material Storage в списке инженера");
    assert!(app.build_menu_select(index), "выбран Material Storage");

    let line = app
        .commit_build_menu_path(max_app::SessionChannel::Local)
        .expect("Path обработан");
    assert!(app.build_site().is_some(), "выбор места открыт ({line})");

    let target = Point::new(grid.x + 3, grid.y);
    app.viewport_mut().center_on_grid(target.x, target.y);
    let (x, y) = screen_point(&app, target);
    let _ = handle_session_input(&mut app, InputEvent::MouseMove(x, y));
    assert!(
        app.build_site()
            .expect("режим выбора")
            .valid(app.state_ref()),
        "клетка в строке инженера валидна"
    );

    app.confirm_build_site(max_app::SessionChannel::Local);

    let unit = app.state_ref().unit(engineer).expect("engineer");
    let (end_x, end_y) = unit
        .path
        .as_ref()
        .map(max_core::unit::UnitPath::end)
        .expect("Path задаёт линейный маршрут");
    assert_eq!(
        (end_x, end_y),
        (target.x, target.y),
        "маршрут ведёт до указанной клетки"
    );
    assert_eq!(
        app.state_ref().order_data(engineer).move_to,
        grid,
        "`UnitsManager_StartBuild` сбрасывает move_to инженера на его клетку"
    );
    assert_eq!(
        unit.build_list,
        vec![UnitType::MaterialStorage],
        "очередь Path-постройки сохранена"
    );
    assert_eq!(unit.orders, UnitOrder::Build);
    assert_eq!(
        unit.target_grid, grid,
        "первый объект ставится на клетке инженера"
    );
}

/// How many pixels of the area differ (any channel above the threshold).
fn count_changed_in_rect(    left: &max_render::buffer::RgbaBuffer,
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
