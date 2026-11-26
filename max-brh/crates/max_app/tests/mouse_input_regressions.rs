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
use max_core::enums::{PlayMode, UnitOrder, UnitOrderState, UnitType};
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
    local_game_with_play_mode(PlayMode::TurnBased)
}

/// The same local match in a chosen play mode. `SimultaneousMoves` is the only
/// mode where the `Manual` item is registered
/// (`UnitsManager_Popup_InitCommons`, `units_manager.cpp:544-548`).
fn local_game_with_play_mode(play_mode: PlayMode) -> Option<App> {
    let mut app = app_with_assets()?;
    let mut setup = LocalSetup::default();
    setup.world = 0;
    setup.timer = 0;
    setup.play_mode = play_mode;
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

/// #4: with a **non-combat** aircraft in focus (`ATTRIB_ATTACK == 0`,
/// `UnitInfo::weapon == 0`), LMB on a cell holding a **foreign** ground unit is
/// a move — the aircraft hovers above the unit (an air/ground stack).
///
/// Oracle `GameManager_GetWindowCursor` (`game_manager.cpp:4434-4465`): with no
/// own `SELECTABLE` unit in the cell (`team_unit == nullptr`) the
/// `MOBILE_AIR_UNIT` branch returns `CURSOR_UNIT_GO`, and LMB gives
/// `ORDER_MOVE`/`ORDER_STATE_INIT` (`:6892-6898`). `Access_IsAccessible` blocks
/// an aircraft only with another **air** unit (`access.cpp:131-137`: the
/// `SameClassBlocks` check compares air vs air), so a ground/sea unit never
/// blocks the destination cell. An **armed** aircraft attacks instead (the
/// owner rule, see `combat_aircraft_left_click_attacks_a_foreign_unit`);
/// `Manual` (`disabled_reaction_fire`) restores this hover move.
#[test]
fn aircraft_left_click_hovers_over_a_foreign_ground_unit() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        for dy in -3..=3 {
            for dx in -3..=3 {
                let point = Point::new(center.x + dx, center.y + dy);
                app.universe_mut()
                    .map_mut()
                    .surface
                    .set(point.x, point.y, max_core::Surface::LAND);
            }
        }

        let aircraft_grid = free_point(&app, center);
        let aircraft = app
            .universe_mut()
            .spawn_unit(
                UnitType::AirTransport,
                team,
                aircraft_grid,
                max_core::access::unit_type_flags(UnitType::AirTransport),
            )
            .expect("air transport");
        {
            // A flying aircraft — the reported symptom is a plane above units.
            let unit = app
                .universe_mut()
                .unit_mut(aircraft)
                .expect("air transport");
            unit.flags |= max_core::enums::unit_flags::HOVERING;
        }

        let ground_grid = free_point(&app, Point::new(aircraft_grid.x + 3, aircraft_grid.y));
        let ground = app
            .universe_mut()
            .spawn_unit(
                UnitType::Tank,
                TeamId(1),
                ground_grid,
                max_core::access::unit_type_flags(UnitType::Tank),
            )
            .expect("foreign tank");
        {
            let unit = app.universe_mut().unit_mut(ground).expect("tank");
            unit.set_visible_to_team(team, true);
            unit.orders = UnitOrder::Await;
            unit.state = UnitOrderState::ExecutingOrder;
        }

        app.viewport_mut()
            .center_on_grid(aircraft_grid.x, aircraft_grid.y);

        // Focus the aircraft with a click on its own cell.
        let (ax, ay) = screen_point(&app, aircraft_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ax, ay));
        assert_eq!(app.hud().selected, Some(aircraft));

        // LMB on the foreign tank's cell: the aircraft flies there, the focus
        // stays on the aircraft (no inspection selection).
        let (gx, gy) = screen_point(&app, ground_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(gx, gy));

        assert_eq!(
            app.hud().selected,
            Some(aircraft),
            "the click must not move the focus to the foreign unit"
        );
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Move (air)")),
            "LMB on a cell with a foreign ground unit must be an air move, log: {:?}",
            app.hud().log.lines()
        );

        let unit = app.state_ref().unit(aircraft).expect("air transport");
        assert_eq!(unit.orders, UnitOrder::Move, "ORDER_MOVE was given");
        assert_eq!(
            app.state_ref().order_data(aircraft).move_to,
            ground_grid,
            "move_to is the foreign unit's cell"
        );

        // The air path is a straight segment: the aircraft reaches the cell and
        // shares it with the ground unit (`MapHash` keeps both, air above ground).
        for _ in 0..240 {
            if app.state_ref().unit(aircraft).map(|unit| unit.grid) == Some(ground_grid) {
                break;
            }
            app.tick();
        }
        assert_eq!(
            app.state_ref().unit(aircraft).map(|unit| unit.grid),
            Some(ground_grid),
            "the aircraft must hover above the ground unit"
        );
        let cell = app.state_ref().units_at(ground_grid);
        assert!(
            cell.contains(&aircraft) && cell.contains(&ground),
            "the cell must hold both the aircraft and the ground unit, got {cell:?}"
        );
    });
}

/// Owner rule: an **armed** aircraft (`ATTRIB_ATTACK > 0` → `UnitInfo::weapon == 2`,
/// `unitinfo.cpp:304-308`) attacks a visible foreign unit by default. Even a
/// target outside the weapon's valid set is an attack here (the Fighter cannot
/// damage a Tank, but the click must be `ORDER_MOVE_TO_ATTACK`, not a hover
/// move). The raw C++ air arm (`game_manager.cpp:4441-4464`) would yield
/// `CURSOR_UNIT_GO` when no valid attack target exists; this divergence is the
/// owner's decision (the Manual toggle below is the hover escape).
#[test]
fn combat_aircraft_left_click_attacks_a_foreign_unit() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        for dy in -3..=3 {
            for dx in -3..=3 {
                let point = Point::new(center.x + dx, center.y + dy);
                app.universe_mut()
                    .map_mut()
                    .surface
                    .set(point.x, point.y, max_core::Surface::LAND);
            }
        }

        let aircraft_grid = free_point(&app, center);
        let aircraft = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                team,
                aircraft_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("fighter");
        {
            let unit = app.universe_mut().unit_mut(aircraft).expect("fighter");
            unit.flags |= max_core::enums::unit_flags::HOVERING;
        }

        let enemy_grid = free_point(&app, Point::new(aircraft_grid.x + 3, aircraft_grid.y));
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
            .center_on_grid(aircraft_grid.x, aircraft_grid.y);

        let (ax, ay) = screen_point(&app, aircraft_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ax, ay));
        assert_eq!(app.hud().selected, Some(aircraft));

        let (ex, ey) = screen_point(&app, enemy_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));

        assert_eq!(
            app.hud().selected,
            Some(aircraft),
            "the click must not move the focus to the foreign unit"
        );
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Attack:")),
            "LMB on a foreign unit with an armed aircraft must be an attack, log: {:?}",
            app.hud().log.lines()
        );

        let unit = app.state_ref().unit(aircraft).expect("fighter");
        assert_eq!(
            unit.orders,
            UnitOrder::MoveToAttack,
            "ORDER_MOVE_TO_ATTACK was given"
        );
        assert_eq!(
            app.state_ref().unit(aircraft).expect("fighter").enemy_unit,
            Some(enemy),
            "the attack order carries the enemy"
        );
        assert_eq!(
            app.state_ref().order_data(aircraft).move_to,
            enemy_grid,
            "the attack order targets the enemy's cell"
        );
    });
}

/// Owner rule: `Manual` — the `disabled_reaction_fire` toggle of
/// `UnitsManager_Popup_OnClick_Manual` (`units_manager.cpp:532-541`; status
/// `_(c35e)` "Reaction Fire Off", `game_manager.cpp:7634-7636`) — turns the
/// armed aircraft's click on a visible enemy back into the hover move. A valid
/// attack target (air for the Fighter) is used to show that the toggle, not the
/// target type, decides.
#[test]
fn manual_aircraft_left_click_hovers_over_a_foreign_unit() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        for dy in -3..=3 {
            for dx in -3..=3 {
                let point = Point::new(center.x + dx, center.y + dy);
                app.universe_mut()
                    .map_mut()
                    .surface
                    .set(point.x, point.y, max_core::Surface::LAND);
            }
        }

        let aircraft_grid = free_point(&app, center);
        let aircraft = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                team,
                aircraft_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("fighter");
        {
            let unit = app.universe_mut().unit_mut(aircraft).expect("fighter");
            unit.flags |= max_core::enums::unit_flags::HOVERING;
            // The Manual toggle (`UnitsManager_Popup_OnClick_Manual`).
            unit.disabled_reaction_fire = true;
        }

        let enemy_grid = free_point(&app, Point::new(aircraft_grid.x + 3, aircraft_grid.y));
        let enemy = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                TeamId(1),
                enemy_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("enemy fighter");
        {
            let unit = app.universe_mut().unit_mut(enemy).expect("fighter");
            unit.flags |= max_core::enums::unit_flags::HOVERING;
            unit.set_visible_to_team(team, true);
            unit.orders = UnitOrder::Await;
            unit.state = UnitOrderState::ExecutingOrder;
        }

        app.viewport_mut()
            .center_on_grid(aircraft_grid.x, aircraft_grid.y);

        let (ax, ay) = screen_point(&app, aircraft_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ax, ay));
        assert_eq!(app.hud().selected, Some(aircraft));

        let (ex, ey) = screen_point(&app, enemy_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));

        assert_eq!(
            app.hud().selected,
            Some(aircraft),
            "the click must not move the focus to the foreign unit"
        );
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Move (air)")),
            "Manual must turn the enemy cell into a hover move, log: {:?}",
            app.hud().log.lines()
        );
        assert_eq!(
            app.state_ref().unit(aircraft).expect("fighter").orders,
            UnitOrder::Move,
            "ORDER_MOVE was given"
        );
        assert_eq!(
            app.state_ref().order_data(aircraft).move_to,
            enemy_grid,
            "move_to is the enemy's cell"
        );
    });
}

/// The real `Manual` path: in `SimultaneousMoves` the unit menu registers the
/// `Manual` item (`UnitsManager_Popup_InitCommons`, `units_manager.cpp:544-548`,
/// caption `_(b0b9)`), and activating it toggles `disabled_reaction_fire`
/// (`UnitsManager_Popup_OnClick_Manual`, `:532-541`) through the P1 order. After
/// the toggle an armed aircraft's click on a visible enemy is the hover move,
/// not the attack.
#[test]
fn manual_menu_item_turns_reaction_fire_off_and_allows_the_hover_click() {
    with_big_stack(|| {
        let Some(mut app) = local_game_with_play_mode(PlayMode::SimultaneousMoves) else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        for dy in -3..=3 {
            for dx in -3..=3 {
                let point = Point::new(center.x + dx, center.y + dy);
                app.universe_mut()
                    .map_mut()
                    .surface
                    .set(point.x, point.y, max_core::Surface::LAND);
            }
        }

        let aircraft_grid = free_point(&app, center);
        let aircraft = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                team,
                aircraft_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("fighter");
        {
            let unit = app.universe_mut().unit_mut(aircraft).expect("fighter");
            unit.flags |= max_core::enums::unit_flags::HOVERING;
        }

        let enemy_grid = free_point(&app, Point::new(aircraft_grid.x + 3, aircraft_grid.y));
        let enemy = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                TeamId(1),
                enemy_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("enemy fighter");
        {
            let unit = app.universe_mut().unit_mut(enemy).expect("fighter");
            unit.flags |= max_core::enums::unit_flags::HOVERING;
            unit.set_visible_to_team(team, true);
            unit.orders = UnitOrder::Await;
            unit.state = UnitOrderState::ExecutingOrder;
        }

        let toggle = |app: &mut App| {
            app.hud_mut().select(Some(aircraft));
            assert!(app.open_unit_menu(), "the unit menu opens");
            let index = app
                .unit_menu()
                .and_then(|menu| menu.index_of(max_app::unitmenu::ItemId::Manual))
                .expect("the Manual item is registered in SimMoves");
            assert!(app.unit_menu().expect("menu").items[index].enabled);
            app.activate_unit_menu_item(index);
            app.flush_orders();
            app.state_ref()
                .unit(aircraft)
                .expect("fighter")
                .disabled_reaction_fire
        };

        assert!(toggle(&mut app), "Manual turns reaction fire off");
        assert!(!toggle(&mut app), "the second Manual turns it back on");
        assert!(toggle(&mut app), "Manual turns reaction fire off again");

        app.viewport_mut()
            .center_on_grid(aircraft_grid.x, aircraft_grid.y);
        app.hud_mut().select(Some(aircraft));

        let (ex, ey) = screen_point(&app, enemy_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));

        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Move (air)")),
            "Manual must keep the enemy cell a hover move, log: {:?}",
            app.hud().log.lines()
        );
        assert_eq!(
            app.state_ref().unit(aircraft).expect("fighter").orders,
            UnitOrder::Move,
            "ORDER_MOVE was given"
        );
    });
}

/// The own-unit rule confirmed by the run owner: with an aircraft in focus, LMB
/// on a cell holding an **own** ground unit is a move — the aircraft hovers above
/// it.
///
/// The Interplay manual allows aircraft to "fly to any square … park in any
/// square" (`max-brh-assets/text/manual.txt:3365-3367`), and `Access_IsAccessible`
/// blocks an aircraft's destination only with another **air** unit
/// (`access.cpp:131-137`). C++ `GameManager_GetWindowCursor` ends the click on
/// `CURSOR_FRIEND` (`game_manager.cpp:4456-4457`) and selects the own unit
/// (`:6844-6852`), but that routing contradicts the manual — the simulation
/// already accepts the air/ground stack, so LMB must give
/// `ORDER_MOVE`/`ORDER_STATE_INIT` (`:6892-6898`).
#[test]
fn aircraft_left_click_hovers_over_an_own_ground_unit() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        for dy in -3..=3 {
            for dx in -3..=3 {
                let point = Point::new(center.x + dx, center.y + dy);
                app.universe_mut()
                    .map_mut()
                    .surface
                    .set(point.x, point.y, max_core::Surface::LAND);
            }
        }

        let aircraft_grid = free_point(&app, center);
        let aircraft = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                team,
                aircraft_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("fighter");
        {
            let unit = app.universe_mut().unit_mut(aircraft).expect("fighter");
            unit.flags |= max_core::enums::unit_flags::HOVERING;
        }

        let own_grid = free_point(&app, Point::new(aircraft_grid.x + 3, aircraft_grid.y));
        let own = app
            .universe_mut()
            .spawn_unit(
                UnitType::Tank,
                team,
                own_grid,
                max_core::access::unit_type_flags(UnitType::Tank),
            )
            .expect("own tank");

        app.viewport_mut()
            .center_on_grid(aircraft_grid.x, aircraft_grid.y);

        // Focus the aircraft with a click on its own cell.
        let (ax, ay) = screen_point(&app, aircraft_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ax, ay));
        assert_eq!(app.hud().selected, Some(aircraft));

        // LMB on the own tank's cell: the aircraft flies there and hovers above
        // it; the focus stays on the aircraft.
        let (ox, oy) = screen_point(&app, own_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ox, oy));

        assert_eq!(
            app.hud().selected,
            Some(aircraft),
            "the click must not move the focus to the own unit"
        );
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Move (air)")),
            "LMB on a cell with an own ground unit must be an air move, log: {:?}",
            app.hud().log.lines()
        );

        let unit = app.state_ref().unit(aircraft).expect("fighter");
        assert_eq!(unit.orders, UnitOrder::Move, "ORDER_MOVE was given");
        assert_eq!(
            app.state_ref().order_data(aircraft).move_to,
            own_grid,
            "move_to is the own unit's cell"
        );

        for _ in 0..240 {
            if app.state_ref().unit(aircraft).map(|unit| unit.grid) == Some(own_grid) {
                break;
            }
            app.tick();
        }
        assert_eq!(
            app.state_ref().unit(aircraft).map(|unit| unit.grid),
            Some(own_grid),
            "the aircraft must hover above the own ground unit"
        );
        let cell = app.state_ref().units_at(own_grid);
        assert!(
            cell.contains(&aircraft) && cell.contains(&own),
            "the cell must hold both the aircraft and the own ground unit, got {cell:?}"
        );
    });
}

/// The same rule for a **building**: `MININGST`/`BARRACKS` are `STATIONARY`
/// (`0x200`) and `SELECTABLE` (`0x400000`) in `SC_UNITS`, and
/// `Access_IsAccessible` does not let a building block an aircraft
/// (`access.cpp:131-137` only compares air with air), so LMB on an own building
/// is an air move and the aircraft hovers over it.
#[test]
fn aircraft_left_click_hovers_over_an_own_building() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        for dy in -3..=3 {
            for dx in -3..=3 {
                let point = Point::new(center.x + dx, center.y + dy);
                app.universe_mut()
                    .map_mut()
                    .surface
                    .set(point.x, point.y, max_core::Surface::LAND);
            }
        }

        let aircraft_grid = free_point(&app, center);
        let aircraft = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                team,
                aircraft_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("fighter");
        {
            let unit = app.universe_mut().unit_mut(aircraft).expect("fighter");
            unit.flags |= max_core::enums::unit_flags::HOVERING;
        }

        // A 2×2 own building; its anchor cell is the click target
        // (`MapHash` keeps the building in all four cells).
        let building_grid = free_point(&app, Point::new(aircraft_grid.x + 3, aircraft_grid.y));
        let building = app
            .universe_mut()
            .spawn_unit(
                UnitType::Barracks,
                team,
                building_grid,
                max_core::access::unit_type_flags(UnitType::Barracks),
            )
            .expect("own barracks");
        assert_ne!(building, aircraft);

        app.viewport_mut()
            .center_on_grid(aircraft_grid.x, aircraft_grid.y);
        let (ax, ay) = screen_point(&app, aircraft_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ax, ay));
        assert_eq!(app.hud().selected, Some(aircraft));

        let (bx, by) = screen_point(&app, building_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(bx, by));

        assert_eq!(
            app.hud().selected,
            Some(aircraft),
            "the click must not select the own building"
        );
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line.contains("Move (air)")),
            "LMB on an own building must be an air move, log: {:?}",
            app.hud().log.lines()
        );
        assert_eq!(
            app.state_ref().unit(aircraft).map(|unit| unit.orders),
            Some(UnitOrder::Move),
            "ORDER_MOVE was given to the aircraft"
        );
        assert_eq!(
            app.state_ref().order_data(aircraft).move_to,
            building_grid,
            "move_to is the own building's cell"
        );

        for _ in 0..240 {
            if app.state_ref().unit(aircraft).map(|unit| unit.grid) == Some(building_grid) {
                break;
            }
            app.tick();
        }
        assert_eq!(
            app.state_ref().unit(aircraft).map(|unit| unit.grid),
            Some(building_grid),
            "the aircraft must hover above the own building"
        );
        let cell = app.state_ref().units_at(building_grid);
        assert!(
            cell.contains(&aircraft) && cell.contains(&building),
            "the cell must hold both the aircraft and the own building, got {cell:?}"
        );
    });
}

/// Control: the hover rule is only for an **aircraft in focus** and only for an
/// own **non-air** unit. An own aircraft in the destination cell is air-over-air,
/// which `AccessModifier_SameClassBlocks` blocks (`access.cpp:131-137`), so the
/// click keeps the C++ `CURSOR_FRIEND` behavior and selects the own aircraft
/// (`game_manager.cpp:4456-4457`, `:6844-6852`).
#[test]
fn aircraft_left_click_on_an_own_aircraft_still_selects_it() {
    with_big_stack(|| {
        let Some(mut app) = local_game() else {
            return;
        };
        let team = app.player_team();
        let center = Point::new(
            app.state_ref().map().size.width / 2,
            app.state_ref().map().size.height / 2,
        );
        for dy in -3..=3 {
            for dx in -3..=3 {
                let point = Point::new(center.x + dx, center.y + dy);
                app.universe_mut()
                    .map_mut()
                    .surface
                    .set(point.x, point.y, max_core::Surface::LAND);
            }
        }

        let aircraft_grid = free_point(&app, center);
        let aircraft = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                team,
                aircraft_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("fighter");
        let own_air_grid = free_point(&app, Point::new(aircraft_grid.x + 2, aircraft_grid.y));
        let own_air = app
            .universe_mut()
            .spawn_unit(
                UnitType::Fighter,
                team,
                own_air_grid,
                max_core::access::unit_type_flags(UnitType::Fighter),
            )
            .expect("own fighter");

        app.viewport_mut()
            .center_on_grid(aircraft_grid.x, aircraft_grid.y);
        let (ax, ay) = screen_point(&app, aircraft_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ax, ay));
        assert_eq!(app.hud().selected, Some(aircraft));

        let (ox, oy) = screen_point(&app, own_air_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(ox, oy));

        assert_eq!(
            app.hud().selected,
            Some(own_air),
            "air over air is blocked: CURSOR_FRIEND selects the own aircraft"
        );
        assert_eq!(
            app.state_ref().unit(aircraft).map(|unit| unit.orders),
            Some(UnitOrder::Await),
            "the aircraft in focus was not given a move order"
        );
    });
}

/// Control: without an aircraft in focus the click on an own unit stays a plain
/// selection (`GameManager_ClickUnit`, `game_manager.cpp:6265-6268`); the new
/// air rule must not leak into the ground/sea cursor.
#[test]
fn ground_selection_of_an_own_unit_is_unchanged() {
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

        let first_grid = free_point(&app, center);
        let first = app
            .universe_mut()
            .spawn_unit(
                UnitType::Tank,
                team,
                first_grid,
                max_core::access::unit_type_flags(UnitType::Tank),
            )
            .expect("own tank");
        let second_grid = free_point(&app, Point::new(first_grid.x + 2, first_grid.y));
        let second = app
            .universe_mut()
            .spawn_unit(
                UnitType::Tank,
                team,
                second_grid,
                max_core::access::unit_type_flags(UnitType::Tank),
            )
            .expect("own tank");

        app.viewport_mut()
            .center_on_grid(first_grid.x, first_grid.y);
        let (fx, fy) = screen_point(&app, first_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(fx, fy));
        assert_eq!(app.hud().selected, Some(first));

        let (sx, sy) = screen_point(&app, second_grid);
        handle_session_input(&mut app, InputEvent::MouseClick(sx, sy));

        assert_eq!(
            app.hud().selected,
            Some(second),
            "a click on an own unit with a ground unit in focus selects it"
        );
        assert_eq!(
            app.state_ref().unit(second).map(|unit| unit.orders),
            Some(UnitOrder::Await),
            "no move order is given by a selection click"
        );
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
