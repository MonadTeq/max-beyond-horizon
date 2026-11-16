//! Сквозной путь игрока мышью: старт партии, камера на базе, выделение своего
//! юнита кликом, RMB-приказ (зелёный предпросмотр + журнал `Move: N steps`) и
//! фактическое перемещение после End Turn.
//!
//! Тесты с настоящими кадрами (`Assets`/`.WRL`) пропускаются, если данных игры
//! нет рядом с репозиторием — как `unit_sprites.rs` и `tests/app.rs`.

use std::path::{Path, PathBuf};

use max_app::hud::{self, build_move_order};
use max_app::local::LocalSetup;
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
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED && unit.unit_type == UnitType::Surveyor)
        })
        .expect("Surveyor высажен");

    Some((app, surveyor))
}

/// Экранная точка центра клетки в кадре 640×480 с хромом.
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
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
        (
            i32::from(base.x) * 64 + 32,
            i32::from(base.y) * 64 + 32
        ),
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
    assert_eq!(stats.units_skipped, 0, "свои юниты не должны скрываться FOW");
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
