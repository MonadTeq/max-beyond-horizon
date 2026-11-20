//! Интерактивный путь достройки здания с **реальным** `dt > 0`
//! (`App::local_update(1.0 / 60.0)`) — тот же цикл, что крутит окно
//! (`main.rs`: `local_update(frame_dt)`), а не QA-сценарии с `dt = 0`.
//!
//! Регрессия после A1/A4 (`builder stays on site` + `tape_center_offset` ±32 px):
//! QA `production.qa` выделяет строителя командой `select type Constructor` и
//! поэтому не видит проблемы клика. В живой игре строитель после достройки
//! стоит в **центре ленты**: `PositionInTape` сдвигает спрайт на ±32 px, то есть
//! ровно в угол четырёх клеток 2×2. Игрок кликает по нарисованному строителю —
//! и попадает в соседнюю клетку площадки, где лежат декали (`LRGCONES`/
//! `LRGTAPE`/`LRGSLAB`). Порт `hud::unit_at_screen` брал **любого**
//! не-`ORDER_IDLE` юнита клетки, поэтому выделялись конусы/плита:
//!
//! * маркеры `BLDMRK1..5` не рисуются (`render::build_marker_cells` требует
//!   выделенного строителя в `ORDER_BUILD`/`UNIT_READY`), подсказки
//!   «Finished building …» тоже нет (`builder_status_message`);
//! * правый клик по карте с выделенной декалью отменял приказ («Unit cannot
//!   move (air/stationary)»), поэтому строитель не мог уйти с площадки, а
//!   `UnitsManager_BuildingReady` не раскрывал здание.
//!
//! C++ (`Access_GetTeamUnit`, `access.cpp:1804`) требует `SELECTABLE` и
//! возвращает **родителя** ленты (строителя), поэтому клик по любой клетке
//! площадки — это клик по строителю. Тесты ниже закрепляют оба пункта на
//! кадрах с `dt > 0`.

use std::path::{Path, PathBuf};

use max_app::local::{LocalPhase, LocalSetup};
use max_app::ui::{handle_session_input, InputEvent};
use max_app::{App, AppConfig};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamKind;
use max_core::TeamClan;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// Приложение с настоящими ассетами (нужны спрайты `BLDMRK1..5`).
fn local_app() -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("assets"))
        .with_mode(max_app::AppMode::Ui);

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

fn setup_vs_one_ai() -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: max_core::enums::PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: max_core::enums::VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        team_clans: [TeamClan::Random; 4],
        hot_seat: false,
        seed: 7,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

/// Один кадр интерактивного цикла — `main.rs`: `local_update(frame_dt)`.
const FRAME: f32 = 1.0 / 60.0;

fn run_until_player_turn(app: &mut App, limit: usize) -> bool {
    for _ in 0..limit {
        if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
            return true;
        }
        if app.local().is_none_or(|local| local.finished()) {
            return false;
        }
        app.local_update(FRAME);
    }

    app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn)
}

/// Экранная точка центра клетки (окно 1:1: карта рисуется в `MAIN_MAP`).
fn cell_screen(app: &App, cell: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), cell);

    (
        x + max_render::layout::MAIN_MAP.ulx,
        y + max_render::layout::MAIN_MAP.uly,
    )
}

/// Экранная точка, где **нарисован** строитель: центр ленты 2×2
/// (`PositionInTape`/`tape_center_offset`), а не его `grid`.
fn builder_sprite_screen(app: &App, unit_type: UnitType, anchor: Point) -> (i32, i32) {
    let (world_x, world_y) = max_app::buildsite::site_center_world(unit_type, anchor);
    let (x, y) = app.map_viewport().world_to_screen(world_x, world_y);

    (
        x + max_render::layout::MAIN_MAP.ulx,
        y + max_render::layout::MAIN_MAP.uly,
    )
}

fn site_units(app: &App, team: max_core::TeamId) -> usize {
    app.state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .filter(|unit| {
            unit.team == team
                && matches!(
                    unit.unit_type,
                    UnitType::LargeTape
                        | UnitType::SmallTape
                        | UnitType::LargeCones
                        | UnitType::SmallCones
                )
        })
        .count()
}

fn has_slab(app: &App, team: max_core::TeamId) -> bool {
    app.state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .any(|unit| {
            unit.team == team
                && matches!(unit.unit_type, UnitType::LargeSlab | UnitType::SmallSlab)
        })
}

/// Как игрок уводит строителя: LMB по «клетке с символом» (C++ `CURSOR_UNIT_GO`,
/// `game_manager.cpp:5832/6879`) или RMB (конвенция порта).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ExitClick {
    Left,
    Right,
}

/// Полный интерактивный цикл: заказ → стройка → достройка → маркеры →
/// уход строителя → появление здания. Все шаги — на кадрах `dt = 1/60`.
fn run_build_cycle(exit: ExitClick) {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия");
    assert!(
        run_until_player_turn(&mut app, 600),
        "не дошли до хода человека"
    );

    let team = app.player_team();
    app.state_mut().team_units[team.0 as usize].set_gold(5000);

    let builder = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::Constructor)
        })
        .expect("у RED есть конструктор");
    let builder_grid = app.state().unit(builder).expect("builder").grid;

    // Заказ здания ровно как в UI: призрак 2×2 привязан к строителю
    // (`GameManager_GetBuilderUnitCursor`), приказ — пакет 08/P4.
    let anchor = max_core::production::initial_build_site(
        app.state(),
        team,
        UnitType::TrainingHall,
        builder_grid,
        &[builder],
    )
    .expect("свободный якорь 2×2");
    let command = max_app::buildmenu::build_order_command(
        app.state(),
        builder,
        anchor,
        &[UnitType::TrainingHall],
        false,
        1,
    )
    .expect("пакет заказа");
    app.apply(command);

    // `PositionInTape` ENTER: строитель въезжает в ленту (четыре шага + доворот).
    for _ in 0..60 {
        app.local_update(FRAME);
        if app
            .state()
            .unit(builder)
            .is_some_and(|unit| unit.state == UnitOrderState::BuildInProgress)
        {
            break;
        }
    }

    assert_eq!(
        app.state().unit(builder).map(|unit| unit.orders),
        Some(UnitOrder::Build),
        "строитель должен стоять в ORDER_BUILD"
    );

    // Ускорение прогресса напрямую через состояние: остался один ход.
    app.state_mut().unit_mut(builder).expect("builder").build_time = 1;

    // Ход закрывается интерактивно: `finish_turn` → `progress_build_state`
    // (`SpawnNewUnit`) на границе раунда, затем ход ИИ и снова ход человека.
    app.local_end_turn();
    assert!(
        run_until_player_turn(&mut app, 8000),
        "не дождались хода человека после достройки"
    );

    let building = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::TrainingHall)
        })
        .expect("Training Hall построен");

    let builder_unit = app.state().unit(builder).expect("builder").clone();
    let building_unit = app.state().unit(building).expect("building").clone();

    assert_eq!(builder_unit.orders, UnitOrder::Build);
    assert_eq!(
        builder_unit.state,
        UnitOrderState::UnitReady,
        "достроивший строитель ждёт приказа (C++ UNIT_READY)"
    );
    assert_eq!(building_unit.parent_unit, Some(builder));
    assert_eq!(builder_unit.parent_unit, Some(building));
    assert_eq!(
        (building_unit.orders, building_unit.state),
        (UnitOrder::Idle, UnitOrderState::BuildingReady),
        "здание ждёт ухода строителя и не рисуется (`IsRelevant`)"
    );
    assert!(
        max_core::production::unit_bounds_contains(&building_unit, builder_unit.grid),
        "строитель стоит в габаритах площадки: {:?} в {:?}",
        builder_unit.grid,
        building_unit.grid
    );
    assert_eq!(site_units(&app, team), 2, "лента и конусы ещё на площадке");

    // --- (1) Подсветка/сообщение о готовом здании ------------------------
    // Игрок кликает по НАРИСОВАННОМУ строителю — он в центре ленты, спрайт
    // стоит в угле четырёх клеток, а не в своей `grid`.
    let (click_x, click_y) = builder_sprite_screen(&app, UnitType::TrainingHall, building_unit.grid);
    handle_session_input(&mut app, InputEvent::MouseClick(click_x, click_y), false);

    assert_eq!(
        app.hud().selected,
        Some(builder),
        "клик по спрайту строителя обязан выделить СТРОИТЕЛЯ, а не декаль ленты"
    );
    let hint = app.hud().hint.clone().unwrap_or_default();
    assert!(
        hint.contains("Finished building Training Hall"),
        "подсказка о готовности: {hint:?}"
    );

    app.local_update(FRAME);
    let (_, stats) = app.render_frame_with_stats();
    assert!(
        stats.build_markers > 0,
        "маркеры BLDMRK1..5 обязаны рисоваться вокруг достроенной площадки"
    );

    // --- (2) Уход строителя → здание появляется --------------------------
    // Клетка кольца `BLDMRK*` вокруг здания, свободная от юнитов.
    let ring = max_app::render::build_marker_cells(app.state(), &builder_unit)
        .into_iter()
        .find(|cell| app.state().units_at(*cell).is_empty())
        .expect("свободная клетка кольца маркеров");
    let (ring_x, ring_y) = cell_screen(&app, ring);

    match exit {
        ExitClick::Left => {
            handle_session_input(&mut app, InputEvent::MouseClick(ring_x, ring_y), false);
        }
        ExitClick::Right => {
            handle_session_input(&mut app, InputEvent::RightClick(ring_x, ring_y), false);
        }
    }

    assert_eq!(
        app.state().unit(builder).map(|unit| unit.orders),
        Some(UnitOrder::Activate),
        "клик по кольцу маркеров отдаёт ORDER_ACTIVATE ({exit:?})"
    );
    assert_eq!(
        app.state().unit(builder).map(|unit| unit.state),
        Some(UnitOrderState::InTransition),
        "и ORDER_STATE_IN_TRANSITION ({exit:?})"
    );

    let mut revealed = false;

    for _ in 0..1200 {
        app.local_update(FRAME);

        if app
            .state()
            .unit(building)
            .is_some_and(|unit| unit.orders != UnitOrder::Idle)
        {
            revealed = true;
            break;
        }
    }

    assert!(
        revealed,
        "UnitsManager_BuildingReady обязан раскрыть здание после ухода строителя ({exit:?})"
    );

    let building_unit = app.state().unit(building).expect("building").clone();
    let builder_unit = app.state().unit(builder).expect("builder").clone();

    assert_eq!(
        building_unit.scaler_adjust, 4,
        "здание раскрывается `ScaleUnit(EXPAND)` (`scaler_adjust = 4`)"
    );
    assert!(
        !max_core::production::unit_bounds_contains(&building_unit, builder_unit.grid),
        "строитель вне габаритов 2×2: {:?} / {:?}",
        builder_unit.grid,
        building_unit.grid
    );
    assert_eq!(site_units(&app, team), 0, "лента и конусы сняты");
    assert!(
        has_slab(&app, team),
        "плита остаётся фундаментом (`remove_slabs = false`)"
    );

    // Маркеры гаснут: строитель больше не ждёт приказа.
    let (_, stats) = app.render_frame_with_stats();
    assert_eq!(
        stats.build_markers, 0,
        "после ухода строителя маркеров стройки нет"
    );
}

#[test]
fn interactive_build_cycle_with_real_dt_marks_and_reveals_on_left_click() {
    run_build_cycle(ExitClick::Left);
}

#[test]
fn interactive_build_cycle_with_real_dt_marks_and_reveals_on_right_click() {
    run_build_cycle(ExitClick::Right);
}

/// Клик по клетке площадки (даже если там лежат конусы/плита) обязан отдавать
/// **строителя** — `Access_GetTeamUnit` (`access.cpp:1804`): `SELECTABLE`
/// в обоих проходах и `unit = unit->GetParent()` для ленты.
#[test]
fn clicking_the_site_decal_returns_the_builder() {
    use max_core::enums::unit_flags;

    let mut app = App::new(AppConfig::default().with_screen(512, 384));
    app.state_mut().player_team = max_core::TeamId::RED;
    app.state_mut().teams[0].team_type = TeamKind::Player;
    app.state_mut().team_units[0] = max_core::TeamUnits::with_clan(TeamClan::Random);

    let site = Point::new(40, 40);
    let team = max_core::TeamId::RED;
    // Строитель в правой нижней клетке площадки 2×2 (как после `PositionInTape`).
    let builder = app
        .state_mut()
        .spawn_unit(UnitType::Constructor, team, Point::new(41, 41), 0)
        .expect("constructor");
    let building = app
        .state_mut()
        .spawn_unit(UnitType::TrainingHall, team, site, 0)
        .expect("training hall");

    let site_units =
        max_core::production::deploy_construction_site(app.state_mut(), builder, UnitType::TrainingHall, site)
            .expect("стройплощадка");

    {
        let mut unit = app.state().unit(builder).expect("builder").clone();
        unit.parent_unit = Some(building);
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
        app.state_mut().write_unit(builder, unit);
    }
    {
        let mut unit = app.state().unit(building).expect("building").clone();
        unit.parent_unit = Some(builder);
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::BuildingReady;
        app.state_mut().write_unit(building, unit);
    }

    app.viewport_mut().center_on_grid(site.x, site.y);

    // Конусы/плита не `SELECTABLE` — C++ их клик не берёт; лента `SELECTABLE`,
    // но возвращает родителя-строителя.
    for decal in [site_units.cones, site_units.slab, Some(site_units.tape)] {
        let decal = decal.expect("декаль на площадке");
        let unit = app.state().unit(decal).expect("decal");
        assert!(
            matches!(
                unit.unit_type,
                UnitType::LargeTape
                    | UnitType::LargeCones
                    | UnitType::LargeSlab
                    | UnitType::SmallTape
                    | UnitType::SmallCones
                    | UnitType::SmallSlab
            ),
            "декаль площадки: {:?}",
            unit.unit_type
        );
        let cell = unit.grid;
        let (x, y) = cell_screen(&app, cell);
        // Экранная точка центра клетки декали в кадре 512×384 (без хрома).
        let (x, y) = (x - max_render::layout::MAIN_MAP.ulx, y - max_render::layout::MAIN_MAP.uly);
        let picked = max_app::hud::unit_at_screen(app.state(), app.viewport(), team, x, y);

        assert_eq!(
            picked,
            Some(builder),
            "клик по декали {:?} в {cell:?} обязан вернуть строителя",
            unit.unit_type
        );
    }

    // Кольцо вокруг площадки — пусто (там игрок и кликает, чтобы увести
    // строителя: `CURSOR_UNIT_GO`, `IsUnitNextToPosition`).
    let (x, y) = cell_screen(&app, Point::new(38, 40));
    let (x, y) = (x - max_render::layout::MAIN_MAP.ulx, y - max_render::layout::MAIN_MAP.uly);
    assert_eq!(
        max_app::hud::unit_at_screen(app.state(), app.viewport(), team, x, y),
        None,
        "в кольце маркеров юнитов нет"
    );

    // Флаги деки: `GROUND_COVER` есть у всех трёх, `SELECTABLE` — только у ленты.
    assert_ne!(
        app.state().unit(site_units.tape).unwrap().flags & unit_flags::SELECTABLE,
        0,
        "лента `SELECTABLE`, как в C++ (`Access_GetTeamUnit` → `unit->GetParent()`)"
    );
    for decal in [site_units.cones, site_units.slab].into_iter().flatten() {
        assert_eq!(
            app.state().unit(decal).unwrap().flags & unit_flags::SELECTABLE,
            0,
            "конусы/плита не `SELECTABLE`: клик достаётся строителю через ленту"
        );
    }

    let _ = building;
}

/// Зона клика `CURSOR_UNIT_GO` — `GameManager_IsUnitNextToPosition`
/// (`game_manager.cpp:3208`): 4×4 вокруг 2×2-здания, границы включительно.
#[test]
fn builder_exit_click_zone_is_a_four_by_four_block_around_the_site() {
    use max_core::enums::unit_flags;

    let mut state = max_core::GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = max_core::TeamUnits::with_clan(TeamClan::Random);
    let team = max_core::TeamId::RED;
    let building = state
        .spawn_unit(UnitType::TrainingHall, team, Point::new(40, 40), 0)
        .expect("building");
    let building_unit = state.unit(building).expect("building").clone();

    assert_ne!(building_unit.flags & unit_flags::BUILDING, 0);

    for cell in [
        Point::new(39, 39),
        Point::new(42, 42),
        Point::new(40, 40),
        Point::new(41, 41),
        Point::new(39, 42),
        Point::new(42, 39),
    ] {
        assert!(
            max_app::hud::builder_exit_zone_contains(&building_unit, cell),
            "клетка {cell:?} входит в 4×4 вокруг 2×2"
        );
    }

    for cell in [
        Point::new(38, 40),
        Point::new(43, 40),
        Point::new(40, 38),
        Point::new(40, 43),
    ] {
        assert!(
            !max_app::hud::builder_exit_zone_contains(&building_unit, cell),
            "клетка {cell:?} уже за пределами 4×4"
        );
    }
}
