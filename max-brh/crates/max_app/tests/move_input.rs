//! The player's end-to-end mouse path: match start, camera on the base, selecting an own
//! unit with a click, an RMB order (green preview + log `Move: N steps`) and
//! the actual movement after End Turn.
//!
//! The tests use real frames (`Assets`/`.WRL`), require the game data next to the
//! repository — like `unit_sprites.rs` and `tests/app.rs`.

use std::path::{Path, PathBuf};

use max_app::hud::{self, build_move_order};
use max_app::local::{LocalPhase, LocalSetup};
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::setup::GameSetupForm;
use max_app::{
    handle_session_input, App, AppConfig, InputEvent, MenuClient, Screen, Transition, UiContext,
};
use max_core::enums::{Surface, UnitType};
use max_core::geo::{Point, MAP_TILES};
use max_core::map::SurfaceMap;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitListKind;
use max_net::{CreateGameParams, GameInfo};
use max_render::layout::MAIN_MAP;

/// Path inside the repository (`max-brh/crates/max_app/../../..`).
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn land_surface() -> SurfaceMap {
    SurfaceMap::from_tiles([Surface::LAND; MAP_TILES])
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
        eprintln!("move_input: нет данных — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"));
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
}

/// A match through the real start phases (`PreGameFlow`): mobilization → landing
/// (Enter confirms the default point) → `new_game_on_surface`.
fn phased_app() -> Option<(App, max_core::unit::UnitId)> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() {
        eprintln!("move_input: нет данных — нет SNOW_1.WRL");
        return None;
    }

    let mut flow = PreGameFlow::start(GameSetupForm::default(), single_human(), &world_dir);
    assert!(matches!(flow.stage, PreGameStage::Mobilize(_)));
    flow.commit_mobilization();
    assert!(
        matches!(flow.stage, PreGameStage::Landing(_)),
        "после мобилизации — выбор точки высадки"
    );
    flow.commit_landing();

    let config = flow.setup.new_game_config();
    let surface =
        max_app::landing::LandingScreen::load_surface(0, &world_dir).expect("WRL-поверхность");

    let mut app = app_with_assets()?;
    app.new_game_on_surface(&config, &surface);

    let surveyor = app
        .state_ref()
        .unit_list(UnitListKind::MobileLandSea)
        .iter()
        .copied()
        .find(|id| {
            app.state_ref().unit(*id).is_some_and(|unit| {
                unit.team == TeamId::RED && unit.unit_type == UnitType::Surveyor
            })
        })
        .expect("Surveyor высажен");

    Some((app, surveyor))
}

/// Screen point of the cell center in a 640×480 frame with chrome.
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

/// `GameManager_ProcessTeamMissionSupplyUnits`: the generator stands in a separate
/// cell from the mine, and the building sprites do not overlap each other.
///
/// This is the "generator overlaps the mine" regression: the coordinates in `deploy.rs`
/// match C++ (the mine — `start_position_mining_station`, the generator —
/// `starting_position_power_generator`), but C++ `UnitInfo::SetPosition`
/// shifts the building anchor (`flags & BUILDING`) by +31 px from the cell center.
/// Without this shift the 2×2 mine sprite was drawn half a cell up-left and
/// intruded into the generator cell.
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
fn generator_and_mining_station_are_on_different_cells_without_sprite_overlap() {
    with_big_stack(|| {
        let (app, _surveyor) = phased_app().expect(concat!("test fixture is missing: ", stringify!(phased_app())));

        let assets = app.assets().expect("ассеты");
        let find = |unit_type| {
            app.state_ref()
                .unit_ids_in_order()
                .into_iter()
                .filter_map(|id| app.state_ref().unit(id))
                .find(|unit| unit.team == TeamId::RED && unit.unit_type == unit_type)
                .unwrap_or_else(|| panic!("{unit_type:?} не высажен"))
        };

        let mining_station = find(UnitType::MiningStation);
        let generator = find(UnitType::PowerGenerator);

        assert_ne!(
            mining_station.grid, generator.grid,
            "генератор ({:?}) наложен на шахту ({:?})",
            generator.grid, mining_station.grid
        );

        let sprite_bounds = |unit: &max_core::unit::UnitInfo| {
            let sprite = assets.sprite(unit.unit_type).expect("спрайт");
            let base = max_app::render::image_base_for(
                unit,
                app.state_ref()
                    .teams()
                    .get(unit.team.0 as usize)
                    .map(|team| team.team_clan),
            );
            let index = max_app::render::unit_frame_index(unit, base, sprite.sheet.len());
            let frame = sprite.sheet.frame(index).expect("кадр");
            let visual = (f32::from(unit.grid.x), f32::from(unit.grid.y));
            let anchor = max_app::render::unit_anchor_world(unit, visual);

            frame.bounds_for_anchor(anchor.0, anchor.1)
        };

        let mine = sprite_bounds(mining_station);
        let power = sprite_bounds(generator);

        assert!(
            !(mine.0 < power.2 && power.0 < mine.2 && mine.1 < power.3 && power.1 < mine.3),
            "спрайты шахты {mine:?} и генератора {power:?} пересекаются"
        );

        // The mine — `BUILDING` 2×2: the anchor +31 px from the cell center, like `unit.x/y`
        // in `UnitInfo::SetPosition`; the generator — 1×1, the anchor in the cell center.
        assert_ne!(
            u32::from(mining_station.flags) & max_core::enums::unit_flags::BUILDING,
            0,
            "MININGST должен иметь флаг BUILDING"
        );
        assert_eq!(
            max_app::render::unit_anchor_world(
                mining_station,
                (
                    f32::from(mining_station.grid.x),
                    f32::from(mining_station.grid.y)
                )
            ),
            (
                i32::from(mining_station.grid.x) * 64 + 32 + 31,
                i32::from(mining_station.grid.y) * 64 + 32 + 31
            ),
            "якорь здания — +31 px (C++ UnitInfo::SetPosition)"
        );
        assert_eq!(
            u32::from(mining_station.x),
            (i32::from(mining_station.grid.x) * 64 + 63) as u32,
            "unit.x шахты уже содержит сдвиг здания"
        );

        // Both buildings have the PowerOn order: the second frame of the pair is drawn (on).
        for (unit, label) in [(mining_station, "шахта"), (generator, "генератор")] {
            assert_eq!(unit.orders, max_core::enums::UnitOrder::PowerOn, "{label}");
            let sprite = assets.sprite(unit.unit_type).expect("спрайт");
            let base = max_app::render::image_base_for(
                unit,
                app.state_ref()
                    .teams()
                    .get(unit.team.0 as usize)
                    .map(|team| team.team_clan),
            );
            let index = max_app::render::unit_frame_index(unit, base, sprite.sheet.len());

            assert_eq!(
                index,
                usize::try_from(base.max(0)).unwrap() + 1,
                "{label}: включённое здание обязано рисовать кадр image_base + 1"
            );
            assert_ne!(
                sprite.sheet.frame(index),
                sprite.sheet.frame(base as usize),
                "{label}: кадры «включено» и «выключено» должны различаться"
            );
        }

    });
}

#[test]
fn camera_centers_on_the_mining_station_like_cpp() {
    let config = single_human().new_game_config();
    let mut app = App::new(AppConfig::default());
    app.new_game_on_surface(&config, &land_surface());

    let base = app
        .state_ref()
        .unit_list(UnitListKind::Stationary)
        .iter()
        .filter_map(|id| app.state_ref().unit(*id))
        .find(|unit| unit.team == TeamId::RED && unit.unit_type == UnitType::MiningStation)
        .expect("шахта высажена")
        .grid;

    assert_eq!(
        app.viewport().center(),
        (i32::from(base.x) * 64 + 32, i32::from(base.y) * 64 + 32),
        "GameManager_GetFirstRelevantUnit: камера на шахте, а не на GROUND_COVER"
    );
}

#[test]
fn enter_confirms_the_landing_site_and_starts_the_game() {
    // The lobby client mock: the landing phase needs no network.
    struct DummyClient;

    impl MenuClient for DummyClient {
        fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
            Ok(Vec::new())
        }

        fn create_game(&mut self, _params: CreateGameParams) -> Result<GameInfo, String> {
            Err("not used".to_string())
        }
    }

    let surface = land_surface();
    let mut setup = single_human();
    setup.team_types = [
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
        TeamKind::None,
    ];

    let landing = max_app::LandingScreen::new(TeamId::RED, 0, surface.clone());
    let cursor = landing.cursor;
    assert!(landing.cursor_valid(), "точка по умолчанию валидна");

    let flow = PreGameFlow {
        setup,
        form: GameSetupForm::default(),
        pending: Vec::new(),
        stage: PreGameStage::Landing(landing),
        surface: Some(surface),
        terrain: None,
        status: String::new(),
    };
    let mut screen = Screen::PreGame(flow);
    let mut ctx = UiContext::new(Box::new(DummyClient), 640, 480);

    let transition = screen.handle_input(&mut ctx, InputEvent::Key(max_app::Key::Enter));

    match transition {
        Transition::StartLocal(started) => {
            assert_eq!(
                started.start_positions[0],
                Some(cursor),
                "Enter подтверждает точку курсора (PreGameInput::LandingConfirm)"
            );
        }
        other => panic!("Enter на экране высадки должен начать партию, получено {other:?}"),
    }
}

#[test]
fn start_units_are_drawn_after_landing() {
    with_big_stack(|| {
        let (app, _surveyor) = phased_app().expect(concat!("test fixture is missing: ", stringify!(phased_app())));

        assert!(app.chrome_active(), "кадр 640×480 с FRAMEPIC");

        let (frame, stats) = app.render_frame_with_stats();

        assert_eq!((frame.width(), frame.height()), (640, 480));
        assert_eq!(
            stats.units_skipped, 0,
            "свои юниты не должны скрываться FOW"
        );
        assert!(
            stats.units() >= 7,
            "на карте должны быть спрайты стартовых юнитов (шахта, генератор, 3 мобильных, 2 плиты), stats={stats:?}"
        );
        assert!(
            stats.units_drawn >= 7,
            "стартовые юниты рисуются спрайтами, а не пустой картой: stats={stats:?}"
        );

    });
}

/// The "unit does not move" regression: the camera must stand on the mine
/// (`GameManager_GetFirstRelevantUnit` → `Access_GetFirstMiningStation`), otherwise
/// clicks computed from the base hit the cell down-left — a
/// stationary mine stands there, RMB answers «Unit cannot move», and the turn moves nothing.
#[test]
fn base_is_centred_and_the_qa_click_lands_on_the_surveyor() {
    with_big_stack(|| {
        let (app, surveyor) = phased_app().expect(concat!("test fixture is missing: ", stringify!(phased_app())));

        let base = app
            .state_ref()
            .unit_list(UnitListKind::Stationary)
            .iter()
            .filter_map(|id| app.state_ref().unit(*id))
            .find(|unit| unit.team == TeamId::RED && unit.unit_type == UnitType::MiningStation)
            .expect("шахта высажена")
            .grid;

        assert_eq!(
            screen_point(&app, base),
            (
                MAIN_MAP.ulx + MAIN_MAP.width / 2,
                MAIN_MAP.uly + MAIN_MAP.height / 2
            ),
            "камера центрирована на шахте, а не на первый GROUND_COVER"
        );

        // `move-input.qa` clicks (468,178): with the camera on the mine this is the Surveyor's
        // cell. If the camera moves, the scenario coordinates must be
        // recomputed — the test will catch it.
        let surveyor_grid = app.state_ref().unit(surveyor).expect("Surveyor").grid;
        assert_eq!(
            surveyor_grid,
            Point::new(base.x + 1, base.y - 1),
            "Surveyor высажен справа-сверху от шахты"
        );

        let (map_x, map_y) = max_app::panels::screen_to_map(468, 178).expect("клик внутри карты");
        assert_eq!(
            app.map_viewport().screen_to_grid(map_x, map_y),
            (surveyor_grid.x, surveyor_grid.y),
            "клик (468,178) сценария попадает в клетку Surveyor'а"
        );

    });
}

/// The move order reaches the simulation: after the ticks the unit stands in the target
/// cell (packet 38 executes `process_order_move`).
#[test]
fn move_order_moves_the_unit_on_following_ticks() {
    with_big_stack(|| {
        let (mut app, surveyor) = phased_app().expect(concat!("test fixture is missing: ", stringify!(phased_app())));

        let grid = app.state_ref().unit(surveyor).expect("Surveyor").grid;
        let destination = Point::new(grid.x + 1, grid.y);
        let order = build_move_order(app.state_ref(), surveyor, destination).expect(concat!("test fixture is missing: ", stringify!(build_move_order(app.state_ref(), surveyor, destination))));

        app.hud_mut().pending.push(order.command);
        app.flush_orders();

        let mut ticks = 0;
        while app.state_ref().unit(surveyor).map(|unit| unit.grid) != Some(destination) && ticks < 120 {
            app.tick();
            ticks += 1;
        }

        assert_eq!(
            app.state_ref().unit(surveyor).map(|unit| unit.grid),
            Some(destination),
            "юнит должен дойти до (24,21) за {ticks} тиков, а не стоять на месте"
        );

    });
}

#[test]
fn click_selects_own_unit_and_rmb_shows_path_preview() {
    with_big_stack(|| {
        let (mut app, surveyor) = phased_app().expect(concat!("test fixture is missing: ", stringify!(phased_app())));

        let grid = app.state_ref().unit(surveyor).expect("Surveyor").grid;
        let (x, y) = screen_point(&app, grid);

        handle_session_input(&mut app, InputEvent::MouseClick(x, y));
        assert_eq!(
            app.hud().selected,
            Some(surveyor),
            "клик по клетке своего юнита выделяет его (портрет/панель заполняются)"
        );

        // An adjacent cell without units, reachable by a ground route.
        let mut destination = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }

                let point = Point::new(grid.x + dx, grid.y + dy);
                if !app.state_ref().units_at(point).is_empty() {
                    continue;
                }

                if build_move_order(app.state_ref(), surveyor, point).is_some() {
                    destination = Some(point);
                    break;
                }
            }

            if destination.is_some() {
                break;
            }
        }

        let destination = destination.expect("рядом есть проходимая клетка");
        let (rx, ry) = screen_point(&app, destination);

        handle_session_input(&mut app, InputEvent::RightClick(rx, ry));

        let preview = app.hud().preview.as_ref().expect("RMB строит предпросмотр");
        assert!(!preview.is_empty(), "маршрут не пуст");
        assert_eq!(preview.to, destination);
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Move:")),
            "журнал HUD: «Move: N steps», получено {:?}",
            app.hud().log.lines()
        );

        // The green route must get into the frame in windowed mode too (chrome):
        // `render_hud` is not called there, the preview is drawn into the map buffer.
        let (frame, _) = app.render_frame_with_stats();
        let green = hud::PREVIEW_COLOR.to_rgba();
        let painted = frame
            .pixels()
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] == green[..3])
            .count();

        assert!(
            painted >= 8,
            "зелёный предпросмотр пути должен быть нарисован, пикселей: {painted}"
        );

    });
}

/// A match through `App::start_local_game` — the same interactive loop as in the window
/// (`local_update`, `local_end_turn`, HUD orders).
fn local_game_app() -> Option<App> {
    let mut app = app_with_assets()?;
    let mut setup = LocalSetup::default();
    setup.world = 0;
    setup.timer = 0;
    setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    app.start_local_game(&setup).expect("локальная партия");

    Some(app)
}

/// The "generator on the mine cell" regression for the **interactive** flow.
///
/// The point comes from the landing phase (`PreGameFlow::commit_landing` →
/// `LocalSetup::start_positions` → `TeamDeployment`) and goes into
/// `deploy_starting_units_with`, and not into the direct `deploy_starting_units`.
///
/// On the landing cell there really stand **two** units — the `MININGST` mine and its
/// `LRGSLAB` slab (`UnitsManager_DeployUnit(LRGSLAB, …, mining_station_location)`
/// in `GameManager_ProcessTeamMissionSupplyUnits`). The generator is placed
/// `GameManager_GetStartingPositionPowerGenerator`: `point.x -= 1; point.y += 2`,
/// then the first available candidate to the north, i.e. `(mining.x - 1, mining.y + 1)`
/// — a **different** cell. The `[tick]` trace with types `17`/`40` on one cell —
/// is the slab and the mine, not the generator (`2`).
#[test]
fn interactive_landing_keeps_the_generator_off_the_mining_station_cell() {
    let surface = land_surface();
    let mut landing = max_app::LandingScreen::new(TeamId::RED, 0, surface.clone());
    landing.cursor = Point::new(20, 20);

    let mut flow = PreGameFlow {
        setup: single_human(),
        form: GameSetupForm::default(),
        pending: Vec::new(),
        stage: PreGameStage::Landing(landing),
        surface: Some(surface.clone()),
        terrain: None,
        status: String::new(),
    };
    flow.commit_landing();

    let config = flow.setup.new_game_config();
    assert_eq!(
        config.deployments.len(),
        2,
        "запись на каждую играющую команду"
    );
    assert_eq!(config.deployments[0].position, Point::new(20, 20));

    let mut app = App::new(AppConfig::default());
    app.new_game_on_surface(&config, &surface);

    let find = |unit_type| {
        app.state_ref()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| app.state_ref().unit(id))
            .find(|unit| unit.team == TeamId::RED && unit.unit_type == unit_type)
            .unwrap_or_else(|| panic!("{unit_type:?} не высажен"))
    };

    let mining = find(UnitType::MiningStation);
    let generator = find(UnitType::PowerGenerator);
    let large_slab = find(UnitType::LargeSlab);
    let small_slab = find(UnitType::SmallSlab);

    // The mine remained at the chosen point (it is valid), the generator — per C++.
    assert_eq!(mining.grid, Point::new(20, 20));
    assert_eq!(
        generator.grid,
        Point::new(mining.grid.x - 1, mining.grid.y + 1),
        "GameManager_GetStartingPositionPowerGenerator: (x-1, y+2) и первый \
         доступный кандидат на север"
    );
    assert_ne!(
        generator.grid, mining.grid,
        "генератор не должен занимать клетку шахты"
    );

    // Slabs — by the cells of their buildings, like `UnitsManager_DeployUnit`.
    assert_eq!(large_slab.grid, mining.grid);
    assert_eq!(small_slab.grid, generator.grid);

    // On the landing cell there is exactly the mine and the slab; the generator is not there.
    let types_at_base: Vec<UnitType> = app
        .state_ref()
        .units_at(mining.grid)
        .iter()
        .filter_map(|id| app.state_ref().unit(*id))
        .map(|unit| unit.unit_type)
        .collect();

    assert!(types_at_base.contains(&UnitType::MiningStation));
    assert!(
        types_at_base.contains(&UnitType::LargeSlab),
        "LRGSLAB ставится в клетку шахты: {types_at_base:?}"
    );
    assert!(
        !types_at_base.contains(&UnitType::PowerGenerator),
        "генератор не делит клетку с шахтой: {types_at_base:?}"
    );
}

/// The end-to-end interactive path after RMB: the order is applied, the frame passes
/// the route (visible intermediate positions), the unit ends up in the target cell,
/// and **End Turn** (the real panel button) does not lose the order and does not roll back
/// the position.
#[test]
fn rmb_then_end_turn_moves_the_unit_through_the_path_in_the_local_loop() {
    with_big_stack(|| {
        let mut app = local_game_app().expect(concat!("test fixture is missing: ", stringify!(local_game_app())));

        assert_eq!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::PlayerTurn),
            "новая локальная партия начинается с хода человека"
        );

        let surveyor = app
            .state_ref()
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                app.state_ref().unit(*id).is_some_and(|unit| {
                    unit.team == TeamId::RED && unit.unit_type == UnitType::Surveyor
                })
            })
            .expect("Surveyor высажен");
        let start = app.state_ref().unit(surveyor).expect("Surveyor").grid;

        // LMB on the Surveyor cell: the camera is already on the mine (`focus_player`), the click
        // arrives in the same coordinate system as the window.
        let (x, y) = screen_point(&app, start);
        handle_session_input(&mut app, InputEvent::MouseClick(x, y));
        assert_eq!(app.hud().selected, Some(surveyor));

        // An adjacent passable cell without units — the RMB target.
        let mut destination = None;
        'search: for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }

                let point = Point::new(start.x + dx, start.y + dy);
                if !app.state_ref().units_at(point).is_empty() {
                    continue;
                }

                if build_move_order(app.state_ref(), surveyor, point).is_some() {
                    destination = Some(point);
                    break 'search;
                }
            }
        }
        let destination = destination.expect("рядом есть проходимая клетка");

        let (rx, ry) = screen_point(&app, destination);
        handle_session_input(&mut app, InputEvent::RightClick(rx, ry));

        let preview = app.hud().preview.as_ref().expect("RMB строит предпросмотр");
        assert_eq!(preview.to, destination);
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Move:")),
            "журнал HUD: «Move: N steps», получено {:?}",
            app.hud().log.lines()
        );

        // Window frame: the green path is drawn in chrome mode (like C++
        // `Paths_DrawMarker` COLOR_GREEN in `GameManager_DrawMainMap`).
        let (frame, _) = app.render_frame_with_stats();
        let green = hud::PREVIEW_COLOR.to_rgba();
        let painted = frame
            .pixels()
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] == green[..3])
            .count();
        assert!(
            painted >= 8,
            "зелёный предпросмотр должен быть в кадре окна, пикселей: {painted}"
        );

        // Frames of the interactive loop (dt > 0): the simulation puts the unit on the target, and
        // the visual route passes the intermediate positions and ends exactly
        // in the target cell.
        app.update_visual_units(0.0);
        let mut seen: Vec<(f32, f32)> = Vec::new();

        for _ in 0..900 {
            app.local_update(1.0 / 60.0);

            let position = app.visual_position(surveyor).expect("Surveyor жив");
            if seen.last() != Some(&position) {
                seen.push(position);
            }

            let grid = app.state_ref().unit(surveyor).map(|unit| unit.grid);
            let target = (f32::from(destination.x), f32::from(destination.y));

            if grid == Some(destination) && position == target {
                break;
            }
        }

        assert_eq!(
            app.state_ref().unit(surveyor).map(|unit| unit.grid),
            Some(destination),
            "после приказа юнит обязан дойти до ({}, {})",
            destination.x,
            destination.y
        );
        assert!(
            seen.len() >= 3,
            "кадр должен пройти маршрут, а не телепортироваться: {seen:?}"
        );
        assert_eq!(
            seen.last(),
            Some(&(f32::from(destination.x), f32::from(destination.y))),
            "в конце маршрута визуальная позиция — целевая клетка: {seen:?}"
        );

        // End Turn with the real panel button (`WINDOW_ENDTURN`, (390,2)-(462,23)).
        let end_turn = max_render::layout::END_TURN_BUTTON;
        let button = (
            end_turn.ulx + end_turn.width / 2,
            end_turn.uly + end_turn.height / 2,
        );
        handle_session_input(&mut app, InputEvent::MouseClick(button.0, button.1));

        assert_ne!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::PlayerTurn),
            "End Turn закрывает ход человека"
        );

        // The turn is closed, but the order is already executed: after the AI ticks the unit remains on the target,
        // and the visual position catches up with it.
        for _ in 0..600 {
            app.local_update(1.0 / 60.0);
        }

        assert_eq!(
            app.state_ref().unit(surveyor).map(|unit| unit.grid),
            Some(destination),
            "End Turn не должен откатывать или терять приказ"
        );

        let mut frames = 0;
        while frames < 40_000 && app.local().map(|local| local.phase) != Some(LocalPhase::PlayerTurn) {
            app.local_update(1.0 / 60.0);
            frames += 1;
        }
        assert_eq!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::PlayerTurn),
            "после хода ИИ снова ход человека"
        );
        assert_eq!(
            app.state_ref().unit(surveyor).map(|unit| unit.grid),
            Some(destination)
        );
        assert_eq!(
            app.visual_position(surveyor),
            Some((f32::from(destination.x), f32::from(destination.y))),
            "визуальный маршрут доигран после смены хода"
        );

        // The unit really stands on the target: LMB on this cell selects it.
        let (x, y) = screen_point(&app, destination);
        handle_session_input(&mut app, InputEvent::MouseClick(x, y));
        assert_eq!(app.hud().selected, Some(surveyor));

    });
}
