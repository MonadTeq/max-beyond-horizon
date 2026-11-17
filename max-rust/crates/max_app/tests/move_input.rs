//! Сквозной путь игрока мышью: старт партии, камера на базе, выделение своего
//! юнита кликом, RMB-приказ (зелёный предпросмотр + журнал `Move: N steps`) и
//! фактическое перемещение после End Turn.
//!
//! Тесты с настоящими кадрами (`Assets`/`.WRL`) пропускаются, если данных игры
//! нет рядом с репозиторием — как `unit_sprites.rs` и `tests/app.rs`.

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

/// Путь внутри репозитория (`max-rust/crates/max_app/../../..`).
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn land_surface() -> SurfaceMap {
    SurfaceMap::from_tiles([Surface::LAND; MAP_TILES])
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
        eprintln!("move_input: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"));
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
}

/// Партия через настоящие фазы старта (`PreGameFlow`): мобилизация → высадка
/// (Enter подтверждает точку по умолчанию) → `new_game_on_surface`.
fn phased_app() -> Option<(App, max_core::unit::UnitId)> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() {
        eprintln!("move_input: пропуск — нет SNOW_1.WRL");
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
        .state()
        .unit_list(UnitListKind::MobileLandSea)
        .iter()
        .copied()
        .find(|id| {
            app.state().unit(*id).is_some_and(|unit| {
                unit.team == TeamId::RED && unit.unit_type == UnitType::Surveyor
            })
        })
        .expect("Surveyor высажен");

    Some((app, surveyor))
}

/// Экранная точка центра клетки в кадре 640×480 с хромом.
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

/// `GameManager_ProcessTeamMissionSupplyUnits`: генератор стоит в отдельной
/// клетке от шахты, а спрайты зданий не накладываются друг на друга.
///
/// Это регресс «генератор пересекается с шахтой»: координаты в `deploy.rs`
/// совпадают с C++ (шахта — `start_position_mining_station`, генератор —
/// `starting_position_power_generator`), но C++ `UnitInfo::SetPosition`
/// сдвигает якорь здания (`flags & BUILDING`) на +31 px от центра клетки.
/// Без этого сдвига 2×2-спрайт шахты рисовался на полклетки вверх-влево и
/// заходил на клетку генератора.
#[test]
fn generator_and_mining_station_are_on_different_cells_without_sprite_overlap() {
    let Some((app, _surveyor)) = phased_app() else {
        return;
    };

    let assets = app.assets().expect("ассеты");
    let find = |unit_type| {
        app.state()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| app.state().unit(id))
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
            app.state()
                .teams
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

    // Шахта — `BUILDING` 2×2: якорь на +31 px от центра клетки, как `unit.x/y`
    // в `UnitInfo::SetPosition`; генератор — 1×1, якорь в центре клетки.
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

    // У обоих зданий приказ PowerOn: рисуется второй кадр пары (включено).
    for (unit, label) in [(mining_station, "шахта"), (generator, "генератор")] {
        assert_eq!(unit.orders, max_core::enums::UnitOrder::PowerOn, "{label}");
        let sprite = assets.sprite(unit.unit_type).expect("спрайт");
        let base = max_app::render::image_base_for(
            unit,
            app.state()
                .teams
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
}

#[test]
fn camera_centers_on_the_mining_station_like_cpp() {
    let config = single_human().new_game_config();
    let mut app = App::new(AppConfig::default());
    app.new_game_on_surface(&config, &land_surface());

    let base = app
        .state()
        .unit_list(UnitListKind::Stationary)
        .iter()
        .filter_map(|id| app.state().unit(*id))
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
    // Мок клиента лобби: фазе высадки сеть не нужна.
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
    let Some((app, _surveyor)) = phased_app() else {
        return;
    };

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
}

/// Регресс «юнит не двигается»: камера обязана стоять на шахте
/// (`GameManager_GetFirstRelevantUnit` → `Access_GetFirstMiningStation`), иначе
/// клики, рассчитанные от базы, попадают на клетку вниз-влево — там стоит
/// стационарная шахта, RMB отвечает «Unit cannot move», ход ничего не двигает.
#[test]
fn base_is_centred_and_the_qa_click_lands_on_the_surveyor() {
    let Some((app, surveyor)) = phased_app() else {
        return;
    };

    let base = app
        .state()
        .unit_list(UnitListKind::Stationary)
        .iter()
        .filter_map(|id| app.state().unit(*id))
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

    // `move-input.qa` кликает по (468,178): при камере на шахте это клетка
    // Surveyor'а. Если камера уедет, координаты сценария обязаны быть
    // пересчитаны — тест это поймает.
    let surveyor_grid = app.state().unit(surveyor).expect("Surveyor").grid;
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
}

/// Приказ движения доходит до симуляции: после тиков юнит стоит в целевой
/// клетке (пакет 38 исполняет `process_order_move`).
#[test]
fn move_order_moves_the_unit_on_following_ticks() {
    let Some((mut app, surveyor)) = phased_app() else {
        return;
    };

    let grid = app.state().unit(surveyor).expect("Surveyor").grid;
    let destination = Point::new(grid.x + 1, grid.y);
    let Some(order) = build_move_order(app.state(), surveyor, destination) else {
        return;
    };

    app.hud_mut().pending.push(order.command);
    app.flush_orders();

    let mut ticks = 0;
    while app.state().unit(surveyor).map(|unit| unit.grid) != Some(destination) && ticks < 120 {
        app.tick();
        ticks += 1;
    }

    assert_eq!(
        app.state().unit(surveyor).map(|unit| unit.grid),
        Some(destination),
        "юнит должен дойти до (24,21) за {ticks} тиков, а не стоять на месте"
    );
}

#[test]
fn click_selects_own_unit_and_rmb_shows_path_preview() {
    let Some((mut app, surveyor)) = phased_app() else {
        return;
    };

    let grid = app.state().unit(surveyor).expect("Surveyor").grid;
    let (x, y) = screen_point(&app, grid);

    handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);
    assert_eq!(
        app.hud().selected,
        Some(surveyor),
        "клик по клетке своего юнита выделяет его (портрет/панель заполняются)"
    );

    // Соседняя клетка без юнитов, в которую есть наземный маршрут.
    let mut destination = None;
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }

            let point = Point::new(grid.x + dx, grid.y + dy);
            if !app.state().units_at(point).is_empty() {
                continue;
            }

            if build_move_order(app.state(), surveyor, point).is_some() {
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

    handle_session_input(&mut app, InputEvent::RightClick(rx, ry), false);

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

    // Зелёный маршрут должен попасть в кадр и в оконном режиме (хром):
    // `render_hud` там не вызывается, предпросмотр рисуется в буфер карты.
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
}

/// Партия через `App::start_local_game` — тот же интерактивный цикл, что в окне
/// (`local_update`, `local_end_turn`, HUD-приказы).
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

/// Регресс «генератор на клетке шахты» для **интерактивного** потока.
///
/// Точка приходит из фазы высадки (`PreGameFlow::commit_landing` →
/// `LocalSetup::start_positions` → `TeamDeployment`) и уходит в
/// `deploy_starting_units_with`, а не в прямой `deploy_starting_units`.
///
/// На клетке высадки действительно стоят **два** юнита — шахта `MININGST` и её
/// плита `LRGSLAB` (`UnitsManager_DeployUnit(LRGSLAB, …, mining_station_location)`
/// в `GameManager_ProcessTeamMissionSupplyUnits`). Генератор же ставится
/// `GameManager_GetStartingPositionPowerGenerator`: `point.x -= 1; point.y += 2`,
/// затем первый доступный кандидат на север, т.е. `(mining.x - 1, mining.y + 1)`
/// — **другая** клетка. Трасса `[tick]` с типами `17`/`40` на одной клетке —
/// это плита и шахта, а не генератор (`2`).
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
        app.state()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| app.state().unit(id))
            .find(|unit| unit.team == TeamId::RED && unit.unit_type == unit_type)
            .unwrap_or_else(|| panic!("{unit_type:?} не высажен"))
    };

    let mining = find(UnitType::MiningStation);
    let generator = find(UnitType::PowerGenerator);
    let large_slab = find(UnitType::LargeSlab);
    let small_slab = find(UnitType::SmallSlab);

    // Шахта осталась в выбранной точке (она валидна), генератор — по C++.
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

    // Плиты — по клеткам своих зданий, как `UnitsManager_DeployUnit`.
    assert_eq!(large_slab.grid, mining.grid);
    assert_eq!(small_slab.grid, generator.grid);

    // На клетке высадки ровно шахта и плита; генератора там нет.
    let types_at_base: Vec<UnitType> = app
        .state()
        .units_at(mining.grid)
        .iter()
        .filter_map(|id| app.state().unit(*id))
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

/// Сквозной интерактивный путь после RMB: приказ применяется, кадр проходит
/// маршрут (видимые промежуточные позиции), юнит оказывается в целевой клетке,
/// а **End Turn** (настоящая кнопка панели) не теряет приказ и не откатывает
/// позицию.
#[test]
fn rmb_then_end_turn_moves_the_unit_through_the_path_in_the_local_loop() {
    let Some(mut app) = local_game_app() else {
        return;
    };

    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn),
        "новая локальная партия начинается с хода человека"
    );

    let surveyor = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state().unit(*id).is_some_and(|unit| {
                unit.team == TeamId::RED && unit.unit_type == UnitType::Surveyor
            })
        })
        .expect("Surveyor высажен");
    let start = app.state().unit(surveyor).expect("Surveyor").grid;

    // LMB по клетке Surveyor'а: камера уже на шахте (`focus_player`), клик
    // приходит в той же системе координат, что и окно.
    let (x, y) = screen_point(&app, start);
    handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);
    assert_eq!(app.hud().selected, Some(surveyor));

    // Соседняя проходимая клетка без юнитов — цель RMB.
    let mut destination = None;
    'search: for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }

            let point = Point::new(start.x + dx, start.y + dy);
            if !app.state().units_at(point).is_empty() {
                continue;
            }

            if build_move_order(app.state(), surveyor, point).is_some() {
                destination = Some(point);
                break 'search;
            }
        }
    }
    let destination = destination.expect("рядом есть проходимая клетка");

    let (rx, ry) = screen_point(&app, destination);
    handle_session_input(&mut app, InputEvent::RightClick(rx, ry), false);

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

    // Оконный кадр: зелёный путь нарисован в chrome-режиме (как C++
    // `Paths_DrawMarker` COLOR_GREEN в `GameManager_DrawMainMap`).
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

    // Кадры интерактивного цикла (dt > 0): симуляция ставит юнита в цель, а
    // визуальный маршрут проходит промежуточные позиции и заканчивается ровно
    // в целевой клетке.
    app.update_visual_units(0.0);
    let mut seen: Vec<(f32, f32)> = Vec::new();

    for _ in 0..900 {
        app.local_update(1.0 / 60.0);

        let position = app.visual_position(surveyor).expect("Surveyor жив");
        if seen.last() != Some(&position) {
            seen.push(position);
        }

        let grid = app.state().unit(surveyor).map(|unit| unit.grid);
        let target = (f32::from(destination.x), f32::from(destination.y));

        if grid == Some(destination) && position == target {
            break;
        }
    }

    assert_eq!(
        app.state().unit(surveyor).map(|unit| unit.grid),
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

    // End Turn настоящей кнопкой панели (`WINDOW_ENDTURN`, (390,2)-(462,23)).
    let end_turn = max_render::layout::END_TURN_BUTTON;
    let button = (
        end_turn.ulx + end_turn.width / 2,
        end_turn.uly + end_turn.height / 2,
    );
    handle_session_input(&mut app, InputEvent::MouseClick(button.0, button.1), false);

    assert_ne!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn),
        "End Turn закрывает ход человека"
    );

    // Ход закрыт, но приказ уже исполнен: после тиков ИИ юнит остаётся в цели,
    // а визуальная позиция догоняет её.
    for _ in 0..600 {
        app.local_update(1.0 / 60.0);
    }

    assert_eq!(
        app.state().unit(surveyor).map(|unit| unit.grid),
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
        app.state().unit(surveyor).map(|unit| unit.grid),
        Some(destination)
    );
    assert_eq!(
        app.visual_position(surveyor),
        Some((f32::from(destination.x), f32::from(destination.y))),
        "визуальный маршрут доигран после смены хода"
    );

    // Юнит действительно стоит в цели: LMB по этой клетке выделяет его.
    let (x, y) = screen_point(&app, destination);
    handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);
    assert_eq!(app.hud().selected, Some(surveyor));
}
