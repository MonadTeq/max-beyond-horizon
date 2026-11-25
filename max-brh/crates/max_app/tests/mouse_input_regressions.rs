//! Mouse regressions of the manual playthrough (LMB = movement/selection,
//! RMB = inspection):
//!
//! * **#1** a Scout attacks: its popup is `InitMilitary`, so the `attack` item
//!   exists and the targeting mode can be toggled (`units_manager.cpp:617`),
//!   and an LMB on a visible enemy gives the attack order (08/P3);
//! * **#2** RMB on the **selected** unit opens `UnitStats_Menu` (`STATS`),
//!   `Esc` closes it (`game_manager.cpp:6747-6753`, `unitstats.cpp`);
//! * **#3** RMB on the minimap gives `ORDER_MOVE` to the selected unit
//!   (`game_manager.cpp:6688-6699`).
//!
//! Real frames/assets and the game data next to the repository, like
//! `move_input.rs`.

use std::path::{Path, PathBuf};

use max_app::ui::{handle_session_input, InputEvent, Key};
use max_app::{App, AppConfig, LocalSetup};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};

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
        eprintln!("mouse_input_regressions: game data is missing next to the repository");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"));
    let mut app = App::new(config);
    app.init_assets().expect("game assets");

    Some(app)
}

fn local_game() -> Option<App> {
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
    app.start_local_game(&setup).expect("local match");

    Some(app)
}

fn find_unit(app: &App, team: TeamId, unit_type: UnitType) -> Option<max_core::unit::UnitId> {
    app.state_ref().unit_ids_in_order().into_iter().find(|id| {
        app.state_ref()
            .unit(*id)
            .is_some_and(|unit| unit.team == team && unit.unit_type == unit_type)
    })
}

fn free_point(app: &App, near: Point) -> Point {
    for radius in 0..12 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let point = Point::new(near.x + dx, near.y + dy);
                if app.state_ref().map().index(point.x, point.y).is_some()
                    && app.state_ref().units_at(point).is_empty()
                {
                    return point;
                }
            }
        }
    }
    near
}

fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);
    if app.chrome_active() {
        (
            x + max_render::layout::MAIN_MAP.ulx,
            y + max_render::layout::MAIN_MAP.uly,
        )
    } else {
        (x, y)
    }
}

fn with_big_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(body)
        .expect("test thread")
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

/// #1: the Scout popup has `attack` (`InitMilitary`) and toggles `targeting_mode`.
#[test]
fn scout_popup_has_attack_and_toggles_targeting() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        let point = free_point(&app, center);
        let scout = app
            .universe_mut()
            .spawn_unit(
                UnitType::Scout,
                team,
                point,
                max_core::access::unit_type_flags(UnitType::Scout),
            )
            .expect("scout");

        app.hud_mut().select(Some(scout));
        assert!(app.open_unit_menu(), "the Scout popup opens");

        let index = app
            .unit_menu()
            .and_then(|menu| menu.index_of(max_app::unitmenu::ItemId::Attack))
            .expect("the Scout has the attack item (oracle InitMilitary)");
        assert!(app.unit_menu().expect("menu").items[index].enabled);

        app.activate_unit_menu_item(index);
        assert!(
            app.hud().targeting_mode,
            "attack enables the targeting mode of the Scout"
        );
    });
}

/// #1 (full mouse path): LMB on a visible enemy attacks with the Scout.
#[test]
fn scout_left_click_attacks_a_visible_enemy() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        for dy in -2..=2 {
            for dx in -2..=2 {
                let point = Point::new(center.x + dx, center.y + dy);
                app.universe_mut()
                    .map_mut()
                    .surface
                    .set(point.x, point.y, max_core::Surface::LAND);
            }
        }

        let scout_grid = free_point(&app, center);
        let scout = app
            .universe_mut()
            .spawn_unit(
                UnitType::Scout,
                team,
                scout_grid,
                max_core::access::unit_type_flags(UnitType::Scout),
            )
            .expect("scout");
        let enemy_grid = free_point(&app, Point::new(scout_grid.x + 2, scout_grid.y));
        let enemy = app
            .universe_mut()
            .spawn_unit(
                UnitType::Tank,
                TeamId(1),
                enemy_grid,
                max_core::access::unit_type_flags(UnitType::Tank),
            )
            .expect("tank");
        {
            let unit = app.universe_mut().unit_mut(enemy).expect("tank");
            unit.set_visible_to_team(team, true);
            unit.orders = UnitOrder::Await;
            unit.state = UnitOrderState::ExecutingOrder;
        }

        app.viewport_mut()
            .center_on_grid(scout_grid.x, scout_grid.y);

        let (sx, sy) = screen_point(&app, scout_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(sx, sy));
        assert_eq!(app.hud().selected, Some(scout));
        assert!(
            app.unit_stats().is_none(),
            "LMB does not open the stats window"
        );

        let (ex, ey) = screen_point(&app, enemy_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));

        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Attack:")),
            "LMB on a visible enemy must be an attack, log: {:?}",
            app.hud().log.lines()
        );
        let unit = app.state_ref().unit(scout).expect("scout");
        assert_eq!(unit.orders, UnitOrder::MoveToAttack);
        assert_eq!(unit.enemy_unit, Some(enemy));
    });
}

/// #2: RMB on the selected unit opens the full-screen statistics window.
#[test]
fn right_click_on_selected_unit_opens_the_stats_window() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let surveyor = find_unit(&app, team, UnitType::Surveyor).expect("surveyor");
        let grid = app.state_ref().unit(surveyor).expect("surveyor").grid;
        app.viewport_mut().center_on_grid(grid.x, grid.y);

        let (x, y) = screen_point(&app, grid);
        handle_session_input(&mut app, InputEvent::MouseClick(x, y));
        assert_eq!(app.hud().selected, Some(surveyor));

        // The first RMB on the *selected* unit is `UnitStats_Menu`.
        handle_session_input(&mut app, InputEvent::RightClick(x, y));
        let menu = app.unit_stats().expect("statistics window");
        assert_eq!(menu.unit, surveyor);
        assert!(
            menu.rows.iter().any(|row| row.label == "Hits"),
            "the window has the full characteristics table"
        );
        assert!(!menu.name.is_empty());
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Unit stats")),
            "log: {:?}",
            app.hud().log.lines()
        );

        // The window is drawn over the frame (the `STATS` background covers it).
        let (frame, _) = app.render_frame_with_stats();
        assert_eq!((frame.width(), frame.height()), (640, 480));

        // `STATS` covers the map: the window differs from the game frame and the
        // portrait area is painted (the `P_*` picture, not the background).
        app.close_unit_stats();
        let (without, _) = app.render_frame_with_stats();
        app.open_unit_stats(surveyor);
        let different = frame
            .pixels()
            .chunks_exact(4)
            .zip(without.pixels().chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            different > 640 * 480 / 2,
            "the statistics window must cover the frame, differing pixels: {different}"
        );
        let portrait_colors: std::collections::BTreeSet<[u8; 3]> = (11..11 + 300)
            .flat_map(|x| (13..13 + 240).map(move |y| (x, y)))
            .filter_map(|(x, y)| frame.get(x, y).map(|pixel| [pixel.r, pixel.g, pixel.b]))
            .collect();
        assert!(
            portrait_colors.len() > 8,
            "the unit portrait must be drawn (colors: {})",
            portrait_colors.len()
        );

        // `Esc` closes it and returns to the game (`UnitStatsMenu::Run`).
        handle_session_input(&mut app, InputEvent::Key(Key::Escape));
        assert!(app.unit_stats().is_none());
        assert_eq!(app.hud().selected, Some(surveyor));
    });
}

/// #2 (oracle): RMB on another own unit **selects** it, it does not open the
/// statistics window (`GameManager_SelectedUnit != unit`).
#[test]
fn right_click_on_another_unit_only_selects() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let surveyor = find_unit(&app, team, UnitType::Surveyor).expect("surveyor");
        let other = app
            .state_ref()
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                app.state_ref().unit(*id).is_some_and(|unit| {
                    // A cell with exactly one unit: the mining station shares its
                    // cell with the slab, and the click picks the first one.
                    unit.team == team
                        && *id != surveyor
                        && unit.is_visible_to_team(team)
                        && app.state_ref().units_at(unit.grid).len() == 1
                })
            })
            .expect("another own unit");
        let grid = app.state_ref().unit(other).expect("unit").grid;
        app.viewport_mut().center_on_grid(grid.x, grid.y);

        app.hud_mut().select(Some(surveyor));
        let (x, y) = screen_point(&app, grid);
        handle_session_input(&mut app, InputEvent::RightClick(x, y));

        assert_eq!(app.hud().selected, Some(other), "RMB selects the own unit");
        assert!(
            app.unit_stats().is_none(),
            "statistics open only for the selected unit"
        );
    });
}

/// #2 (oracle): RMB on the selected **enemy** opens its statistics window too
/// (`GameManager_SelectedUnit == unit2`).
#[test]
fn right_click_on_selected_enemy_opens_its_stats() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        let enemy_grid = free_point(&app, Point::new(center.x + 4, center.y));
        let enemy = app
            .universe_mut()
            .spawn_unit(
                UnitType::Tank,
                TeamId(1),
                enemy_grid,
                max_core::access::unit_type_flags(UnitType::Tank),
            )
            .expect("enemy tank");
        {
            let unit = app.universe_mut().unit_mut(enemy).expect("tank");
            unit.set_visible_to_team(team, true);
            unit.orders = UnitOrder::Await;
            unit.state = UnitOrderState::ExecutingOrder;
        }
        app.viewport_mut()
            .center_on_grid(enemy_grid.x, enemy_grid.y);

        let (x, y) = screen_point(&app, enemy_grid);
        // The first RMB selects the inspected enemy (`GameManager_UnitSelectOther`).
        handle_session_input(&mut app, InputEvent::RightClick(x, y));
        assert_eq!(app.hud().selected, Some(enemy));
        assert!(app.unit_stats().is_none());

        // The second RMB — `UnitStats_Menu` of the selected enemy.
        handle_session_input(&mut app, InputEvent::RightClick(x, y));
        let menu = app.unit_stats().expect("enemy statistics window");
        assert_eq!(menu.unit, enemy);
        assert_eq!(menu.team, TeamId(1));
    });
}

/// #3: RMB on the minimap gives the selected unit a move order.
#[test]
fn minimap_right_click_orders_the_selected_unit_to_move() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let surveyor = find_unit(&app, team, UnitType::Surveyor).expect("surveyor");
        let start = app.state_ref().unit(surveyor).expect("surveyor").grid;
        app.hud_mut().select(Some(surveyor));

        // A reachable target a few cells away (a path must exist).
        let mut target = None;
        'search: for radius in 1..=4 {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let point = Point::new(start.x + dx, start.y + dy);
                    if point == start || !app.state_ref().units_at(point).is_empty() {
                        continue;
                    }
                    if max_app::hud::build_move_order(app.state_ref(), surveyor, point).is_some() {
                        target = Some(point);
                        break 'search;
                    }
                }
            }
        }
        let target = target.expect("a reachable cell near the surveyor");

        // The minimap pixel that `GameManager_GetMinimapPosition` maps to the cell:
        // invert the same conversion the left click uses.
        let map_width = i32::from(app.state_ref().map().size.width);
        let map_height = i32::from(app.state_ref().map().size.height);
        let minimap = max_render::layout::MINIMAP;
        let mut pixel = None;
        for py in minimap.uly..minimap.uly + minimap.height {
            for px in minimap.ulx..minimap.ulx + minimap.width {
                if max_app::panels::minimap_grid_at(
                    px,
                    py,
                    map_width,
                    map_height,
                    app.viewport(),
                    app.hud().panels.minimap_2x,
                ) == Some((target.x, target.y))
                {
                    pixel = Some((px, py));
                    break;
                }
            }
            if pixel.is_some() {
                break;
            }
        }
        let (px, py) = pixel.expect("minimap pixel of the target cell");

        handle_session_input(&mut app, InputEvent::RightClick(px, py));

        let preview = app.hud().preview.as_ref().expect("minimap order preview");
        assert_eq!(preview.to, target, "the minimap order targets the cell");
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Move:")),
            "minimap RMB must give a move order, log: {:?}",
            app.hud().log.lines()
        );

        // The order reaches the simulation: the surveyor walks to the target.
        for _ in 0..240 {
            if app.state_ref().unit(surveyor).map(|unit| unit.grid) == Some(target) {
                break;
            }
            app.tick();
        }
        assert_eq!(
            app.state_ref().unit(surveyor).map(|unit| unit.grid),
            Some(target),
            "the minimap order must move the unit"
        );
    });
}
