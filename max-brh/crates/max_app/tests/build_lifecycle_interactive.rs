//! Interactive building completion path with **real** `dt > 0`
//! (`App::local_update(1.0 / 60.0)`) — the same loop the window runs
//! (`main.rs`: `local_update(frame_dt)`), not QA scenarios with `dt = 0`.
//!
//! Regression after A1/A4 (`builder stays on site` + `tape_center_offset` ±32 px):
//! QA `production.qa` selects the builder with `select type Constructor` and
//! therefore does not see the click problem. In a live game the builder after completion
//! stands in the **center of the tape**: `PositionInTape` shifts the sprite by ±32 px, i.e.
//! exactly to the corner of four 2×2 cells. The player clicks the drawn builder —
//! and hits the neighboring site cell with the decals (`LRGCONES`/
//! `LRGTAPE`/`LRGSLAB`). The port's `hud::unit_at_screen` took **any**
//! non-`ORDER_IDLE` unit of the cell, so the cones/slab were selected:
//!
//! * the `BLDMRK1..5` markers are not drawn (`render::build_marker_cells` requires
//!   a selected builder in `ORDER_BUILD`/`UNIT_READY`), and the hint
//!   «Finished building …» is absent too (`builder_status_message`);
//! * a right click on the map with a decal selected canceled the order («Unit cannot
//!   move (air/stationary)»), so the builder could not leave the site,
//!   and `UnitsManager_BuildingReady` did not open the building.
//!
//! C++ (`Access_GetTeamUnit`, `access.cpp:1804`) requires `SELECTABLE` and
//! returns the **parent** of the tape (the builder), so a click on any site
//! cell is a click on the builder. The tests below lock both points on
//! frames with `dt > 0`.

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

/// Application with real assets (`BLDMRK1..5` sprites are needed).
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

/// One interactive loop frame — `main.rs`: `local_update(frame_dt)`.
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

/// Screen point of the cell center (1:1 window: the map is drawn in `MAIN_MAP`).
fn cell_screen(app: &App, cell: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), cell);

    (
        x + max_render::layout::MAIN_MAP.ulx,
        y + max_render::layout::MAIN_MAP.uly,
    )
}

/// Screen point where the builder is **drawn**: the center of the 2×2 tape
/// (`PositionInTape`/`tape_center_offset`), not its `grid`.
fn builder_sprite_screen(app: &App, unit_type: UnitType, anchor: Point) -> (i32, i32) {
    let (world_x, world_y) = max_app::buildsite::site_center_world(unit_type, anchor);
    let (x, y) = app.map_viewport().world_to_screen(world_x, world_y);

    (
        x + max_render::layout::MAIN_MAP.ulx,
        y + max_render::layout::MAIN_MAP.uly,
    )
}

fn site_units(app: &App, team: max_core::TeamId) -> usize {
    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
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
    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .any(|unit| {
            unit.team == team
                && matches!(unit.unit_type, UnitType::LargeSlab | UnitType::SmallSlab)
        })
}

/// How a player takes the builder away: LMB on the "cell with the symbol" (C++ `CURSOR_UNIT_GO`,
/// `game_manager.cpp:5832/6879`) or RMB (the port convention).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ExitClick {
    Left,
    Right,
}

/// Full interactive cycle: order → construction → completion → markers →
/// builder leaves → building appears. All steps — on frames with `dt = 1/60`.
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
    app.universe_mut().team_units_table_mut()[team.0 as usize].set_gold(5000);

    let builder = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::Constructor)
        })
        .expect("у RED есть конструктор");
    let builder_grid = app.state_ref().unit(builder).expect("builder").grid;

    // A building order exactly as in the UI: the 2×2 ghost is bound to the builder
    // (`GameManager_GetBuilderUnitCursor`), the order — packet 08/P4.
    let anchor = app.state_ref().initial_build_site(
        team,
        UnitType::TrainingHall,
        builder_grid,
        &[builder],
    )
    .expect("свободный якорь 2×2");
    let command = max_app::buildmenu::build_order_command(
        app.state_ref(),
        builder,
        anchor,
        &[UnitType::TrainingHall],
        false,
        1,
    )
    .expect("пакет заказа");
    app.apply(command);

    // `PositionInTape` ENTER: the builder drives into the tape (four steps + a turn).
    for _ in 0..60 {
        app.local_update(FRAME);
        if app
            .state_ref()
            .unit(builder)
            .is_some_and(|unit| unit.state == UnitOrderState::BuildInProgress)
        {
            break;
        }
    }

    assert_eq!(
        app.state_ref().unit(builder).map(|unit| unit.orders),
        Some(UnitOrder::Build),
        "строитель должен стоять в ORDER_BUILD"
    );

    // Accelerate progress directly through the state: one turn left.
    app.universe_mut().unit_mut(builder).expect("builder").build_time = 1;

    // The turn closes interactively: `finish_turn` → `progress_build_state`
    // (`SpawnNewUnit`) at the round boundary, then the AI turn and the human turn again.
    app.local_end_turn();
    assert!(
        run_until_player_turn(&mut app, 8000),
        "не дождались хода человека после достройки"
    );

    let building = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::TrainingHall)
        })
        .expect("Training Hall построен");

    let builder_unit = app.state_ref().unit(builder).expect("builder").clone();
    let building_unit = app.state_ref().unit(building).expect("building").clone();

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

    // --- (1) Highlight/message about the finished building ------------------------
    // The player clicks the DRAWN builder — it is in the tape center, the sprite
    // stands at the corner of four cells, not in its `grid`.
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

    // --- (2) The builder leaves → the building appears --------------------------
    // A cell of the `BLDMRK*` ring around the building, free of units.
    let ring = max_app::render::build_marker_cells(app.state_ref(), &builder_unit)
        .into_iter()
        .find(|cell| app.state_ref().units_at(*cell).is_empty())
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
        app.state_ref().unit(builder).map(|unit| unit.orders),
        Some(UnitOrder::Activate),
        "клик по кольцу маркеров отдаёт ORDER_ACTIVATE ({exit:?})"
    );
    assert_eq!(
        app.state_ref().unit(builder).map(|unit| unit.state),
        Some(UnitOrderState::InTransition),
        "и ORDER_STATE_IN_TRANSITION ({exit:?})"
    );

    let mut revealed = false;

    for _ in 0..1200 {
        app.local_update(FRAME);

        if app
            .state_ref()
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

    let building_unit = app.state_ref().unit(building).expect("building").clone();
    let builder_unit = app.state_ref().unit(builder).expect("builder").clone();

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

    // The markers go out: the builder no longer waits for an order.
    let (_, stats) = app.render_frame_with_stats();
    assert_eq!(
        stats.build_markers, 0,
        "после ухода строителя маркеров стройки нет"
    );
}

/// A click on another cell during construction does **not** give a move order.
///
/// Oracle: `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5927-5931`) —
/// for a builder in `BUILD_IN_PROGRESS` the cursor is `CURSOR_UNIT_NO_GO`; cancelling
/// construction — only an explicit popup `stop` (`UnitsManager_Popup_OnClick_BuildStop`
/// → `ORDER_BUILD`/`BUILD_CANCEL` → `UnitInfo::CancelBuilding`). The port does not
/// cancel construction on an attempted move and does not waste the order.
#[test]
fn right_click_while_building_is_no_go_and_keeps_the_site() {
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
    app.universe_mut().team_units_table_mut()[team.0 as usize].set_gold(5000);

    let builder = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.unit_type == UnitType::Constructor)
        })
        .expect("у RED есть конструктор");
    let builder_grid = app.state_ref().unit(builder).expect("builder").grid;

    let anchor = app.state_ref().initial_build_site(
        team,
        UnitType::TrainingHall,
        builder_grid,
        &[builder],
    )
    .expect("свободный якорь 2×2");
    let command = max_app::buildmenu::build_order_command(
        app.state_ref(),
        builder,
        anchor,
        &[UnitType::TrainingHall],
        false,
        1,
    )
    .expect("пакет заказа");
    app.apply(command);

    for _ in 0..60 {
        app.local_update(FRAME);
        if app
            .state_ref()
            .unit(builder)
            .is_some_and(|unit| unit.state == UnitOrderState::BuildInProgress)
        {
            break;
        }
    }

    assert_eq!(
        app.state_ref().unit(builder).map(|unit| unit.orders),
        Some(UnitOrder::Build)
    );
    assert!(
        site_units(&app, team) > 0,
        "площадка стоит на карте до ухода строителя"
    );
    let slab_at_site = |app: &App| {
        app.state_ref().units_at(anchor).iter().any(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| matches!(unit.unit_type, UnitType::LargeSlab | UnitType::SmallSlab))
        })
    };
    assert!(slab_at_site(&app), "плита стоит на площадке");

    // A free reachable cell nearby: a click on it in C++ — `UNIT_NO_GO`.
    let destination = (0..=4)
        .flat_map(|radius| {
            (-radius..=radius).flat_map(move |dy| (-radius..=radius).map(move |dx| (dx, dy)))
        })
        .map(|(dx, dy)| Point::new(builder_grid.x + dx, builder_grid.y + dy))
        .find(|cell| {
            *cell != builder_grid
                && app.state_ref().map().index(cell.x, cell.y).is_some()
                && app.state_ref().units_at(*cell).is_empty()
                && max_app::hud::build_move_order(app.state_ref(), builder, *cell).is_some()
        })
        .expect("достижимая клетка для ухода");

    let (click_x, click_y) = cell_screen(&app, destination);
    app.hud_mut().select(Some(builder));

    // The C++ cursor for `BUILD_IN_PROGRESS` — `CURSOR_UNIT_NO_GO`.
    assert_eq!(
        max_app::cursor::window_cursor(&app, destination),
        max_app::cursor::CursorId::UnitNoGo,
        "курсор над клеткой увода — CURSOR_UNIT_NO_GO"
    );

    handle_session_input(&mut app, InputEvent::RightClick(click_x, click_y), false);

    let unit = app.state_ref().unit(builder).expect("builder alive").clone();
    assert_eq!(
        (unit.orders, unit.state),
        (UnitOrder::Build, UnitOrderState::BuildInProgress),
        "приказ движения не отдан — строитель продолжает стройку"
    );
    assert_eq!(
        unit.build_list,
        vec![UnitType::TrainingHall],
        "очередь стройки не тронута"
    );
    assert!(unit.build_time > 0, "прогресс стройки сохранён");
    assert!(site_units(&app, team) > 0, "лента и конусы на месте");
    assert!(slab_at_site(&app), "плита на месте");
    assert!(
        app.hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("Cannot move while building")),
        "HUD объясняет отказ: {:?}",
        app.hud().log.lines()
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

/// A click on a site cell (even if the cones/slab lie there) must give
/// **the builder** — `Access_GetTeamUnit` (`access.cpp:1804`): `SELECTABLE`
/// in both passes and `unit = unit->GetParent()` for the tape.
#[test]
fn clicking_the_site_decal_returns_the_builder() {
    use max_core::enums::unit_flags;

    let mut app = App::new(AppConfig::default().with_screen(512, 384));
    app.universe_mut().set_player_team(max_core::TeamId::RED);
    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().team_units_table_mut()[0] = max_core::TeamUnits::with_clan(TeamClan::Random);

    let site = Point::new(40, 40);
    let team = max_core::TeamId::RED;
    // The builder in the bottom-right cell of the 2×2 site (as after `PositionInTape`).
    let builder = app
        .universe_mut()
        .spawn_unit(UnitType::Constructor, team, Point::new(41, 41), 0)
        .expect("constructor");
    let building = app
        .universe_mut()
        .spawn_unit(UnitType::TrainingHall, team, site, 0)
        .expect("training hall");

    let site_units =
        app.universe_mut().deploy_construction_site( builder, UnitType::TrainingHall, site)
            .expect("стройплощадка");

    {
        let mut unit = app.state_ref().unit(builder).expect("builder").clone();
        unit.parent_unit = Some(building);
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
        app.universe_mut().write_unit(builder, unit);
    }
    {
        let mut unit = app.state_ref().unit(building).expect("building").clone();
        unit.parent_unit = Some(builder);
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::BuildingReady;
        app.universe_mut().write_unit(building, unit);
    }

    app.viewport_mut().center_on_grid(site.x, site.y);

    // The cones/slab are not `SELECTABLE` — C++ does not take their click; the tape is `SELECTABLE`,
    // but returns the builder parent.
    for decal in [site_units.cones, site_units.slab, Some(site_units.tape)] {
        let decal = decal.expect("декаль на площадке");
        let unit = app.state_ref().unit(decal).expect("decal");
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
        // Screen point of the decal cell center in a 512×384 frame (without chrome).
        let (x, y) = (x - max_render::layout::MAIN_MAP.ulx, y - max_render::layout::MAIN_MAP.uly);
        let picked = max_app::hud::unit_at_screen(app.state_ref(), app.viewport(), team, x, y);

        assert_eq!(
            picked,
            Some(builder),
            "клик по декали {:?} в {cell:?} обязан вернуть строителя",
            unit.unit_type
        );
    }

    // The ring around the site is empty (the player clicks there to take the
    // builder away: `CURSOR_UNIT_GO`, `IsUnitNextToPosition`).
    let (x, y) = cell_screen(&app, Point::new(38, 40));
    let (x, y) = (x - max_render::layout::MAIN_MAP.ulx, y - max_render::layout::MAIN_MAP.uly);
    assert_eq!(
        max_app::hud::unit_at_screen(app.state_ref(), app.viewport(), team, x, y),
        None,
        "в кольце маркеров юнитов нет"
    );

    // Decal flags: `GROUND_COVER` is present on all three, `SELECTABLE` — only on the tape.
    assert_ne!(
        app.state_ref().unit(site_units.tape).unwrap().flags & unit_flags::SELECTABLE,
        0,
        "лента `SELECTABLE`, как в C++ (`Access_GetTeamUnit` → `unit->GetParent()`)"
    );
    for decal in [site_units.cones, site_units.slab].into_iter().flatten() {
        assert_eq!(
            app.state_ref().unit(decal).unwrap().flags & unit_flags::SELECTABLE,
            0,
            "конусы/плита не `SELECTABLE`: клик достаётся строителю через ленту"
        );
    }

    let _ = building;
}

/// `CURSOR_UNIT_GO` click zone — `GameManager_IsUnitNextToPosition`
/// (`game_manager.cpp:3208`): 4×4 around a 2×2 building, bounds inclusive.
#[test]
fn builder_exit_click_zone_is_a_four_by_four_block_around_the_site() {
    use max_core::enums::unit_flags;

    let mut state = max_core::universe::GameUniverse::default();
    state
        .team_mut(max_core::TeamId::RED)
        .expect("RED slot")
        .team_type = TeamKind::Player;
    *state
        .team_units_mut(max_core::TeamId::RED)
        .expect("RED economy") = max_core::TeamUnits::with_clan(TeamClan::Random);
    let team = max_core::TeamId::RED;
    let building = state
        .spawn_unit(UnitType::TrainingHall, team, Point::new(40, 40), 0)
        .expect("building");
    let building_unit = state.snapshot().unit(building).expect("building").clone();

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
