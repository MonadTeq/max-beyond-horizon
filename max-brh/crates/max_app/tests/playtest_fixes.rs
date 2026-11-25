//! Playtest defects of the in-game UI (2026-11-26):
//!
//! * the unit route preview (`GameManager_RenderScanRangeIndicators` в†’
//!   `GroundPath::Draw`) вЂ” green markers are reachable this turn, blue ones in the
//!   following turns, and the unit walks the queued path when the turn is closed;
//! * scrolling/zooming the map closes the open command popup
//!   (`GameManager_UpdateMainMapView` в†’ `GameManager_DeinitPopupButtons`);
//! * the mouse cursor over UI windows: the hand over an actionable element, the
//!   "no" sign (`CURSOR_UNIT_NO_GO`) over an element that cannot be activated.
//!
//! The tests use real frames (`Assets`/`.WRL`), require the game data next to the
//! repository вЂ” like `unit_menu.rs` and `move_input.rs`.

use std::path::{Path, PathBuf};

use max_app::cursor::{session_cursor, CursorId};
use max_app::local::LocalSetup;
use max_app::{App, AppConfig};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{PathStep, Point};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{GroundPath, UnitId, UnitPath};
use max_render::layout::MAIN_MAP;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// One human RED team, the rest вЂ” AI/empty.
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
        eprintln!("playtest_fixes: no game data next to the repository");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"));
    let mut app = App::new(config);
    app.init_assets().expect("game assets");

    Some(app)
}

/// A local match through `App::start_local_game` вЂ” the same interactive loop as in
/// the window (`local_update`, `local_end_turn`, HUD orders).
fn game_app() -> Option<App> {
    let mut app = app_with_assets()?;
    let mut setup = single_human();
    setup.timer = 0;
    app.start_local_game(&setup).expect("local match");

    Some(app)
}

/// A free land cell next to the RED mine.
fn free_grid(app: &App) -> Point {
    for radius in 1..8 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(22 + dx, 22 + dy);

                if app.state_ref().units_at(grid).is_empty()
                    && app.state_ref().map().surface_at(grid.x, grid.y).bits()
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

/// Adds a RED unit to a free cell and puts the camera on it.
fn spawn(app: &mut App, unit_type: UnitType) -> UnitId {
    let grid = free_grid(app);
    let flags = max_core::access::unit_type_flags(unit_type);
    let id = app
        .universe_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("unit spawned");

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    app.select_unit(Some(id));
    id
}

/// Gives the unit a ready movement order along `steps` with an explicit speed,
/// so that the route spans several turns.
fn give_route(app: &mut App, id: UnitId, steps: Vec<PathStep>, speed: u16) {
    let mut unit = app.state_ref().unit(id).cloned().expect("unit");
    let mut end = unit.grid;

    for step in &steps {
        end = Point::new(end.x + i16::from(step.x), end.y + i16::from(step.y));
    }

    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.speed = speed;
    unit.path = Some(UnitPath::Ground(GroundPath {
        end_x: end.x,
        end_y: end.y,
        step_index: 0,
        steps,
    }));

    app.universe_mut().write_unit(id, unit);
}

fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

/// `GameManager_InitPopupButtons` for the selected unit (`game_manager.cpp:3568`).
fn open_menu(app: &mut App, id: UnitId) {
    app.close_unit_menu();
    app.select_unit(Some(id));
    assert!(app.open_unit_menu(), "the command menu opens");
}

// ---------------------------------------------------------------------------
// Defect 2: route arrows and the end-of-turn movement
// ---------------------------------------------------------------------------

/// `GroundPath::Draw` (`groundpath.cpp:436-556`): one marker per remaining route
/// cell, green while the current movement points last (`COLOR_GREEN`), blue for the
/// cells of the following turns (`COLOR_BLUE`); the destination is always green.
#[test]
fn route_arrows_split_this_turn_from_later_turns() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = spawn(&mut app, UnitType::Tank);
    // 12 cells east; a tank with 2 movement points reaches ~2 cells per turn.
    give_route(&mut app, id, vec![PathStep::new(1, 0); 12], 2);

    let unit = app.state_ref().unit(id).cloned().expect("unit");
    let markers = app.state_ref().path_markers(&unit);

    assert_eq!(markers.len(), 13, "12 steps + the destination marker");

    let green = markers.iter().filter(|marker| marker.this_turn).count();
    let blue = markers.iter().filter(|marker| !marker.this_turn).count();

    assert!(green >= 1, "near cells are green (this turn): {markers:?}");
    assert!(
        blue >= 1,
        "far cells are blue (the following turns): {markers:?}"
    );
    assert!(
        markers.last().is_some_and(|marker| marker.this_turn),
        "the destination marker is always green (`groundpath.cpp:496`)"
    );

    // The marker geometry points along the route: east = direction 2
    // (`Paths_8DirPointsArrayMarker*`).
    assert!(
        markers.iter().all(|marker| marker.angle == 2),
        "an eastbound route вЂ” every arrow points right: {markers:?}"
    );

    // The frame paints the arrows (`GameManager_RenderScanRangeIndicators`).
    let (frame, stats) = app.render_frame_with_stats();
    assert!(
        stats.route_markers >= 1,
        "the arrows are drawn into the frame"
    );

    let palette = max_app::render::window_palette(app.assets().expect("assets"));
    let green_rgb = &palette[usize::from(max_app::render::PATH_MARKER_GREEN_INDEX)][..3];
    let blue_rgb = &palette[usize::from(max_app::render::PATH_MARKER_BLUE_INDEX)][..3];
    let pixels = frame.pixels();
    let has = |rgb: &[u8]| pixels.chunks_exact(4).any(|pixel| &pixel[..3] == rgb);

    assert!(has(green_rgb), "the frame holds COLOR_GREEN (2)");
    assert!(has(blue_rgb), "the frame holds COLOR_BLUE (3)");
}

/// The ordered route: the arrows of the selected unit show the queued path, and the
/// order is not lost by closing the turn вЂ” the unit walks the route through the turn
/// end (`GameManager_ProcessTick` в†’ `UnitsManager_ProcessOrders` в†’ `GroundPath::Execute`).
///
/// Note: the port's local loop ticks the simulation during the player's turn, so the
/// order starts executing right away; in the oracle `GAME_STATE_11_TURN_ACTIVE` gates
/// `UnitsManager_ProcessOrders` and the unit waits for the turn end. The divergence is
/// recorded in the defect report (turn-model task), the oracle-relevant part вЂ” the
/// route arrows and that the turn end neither cancels nor rolls back the order вЂ” is
/// covered here.
#[test]
fn an_ordered_route_survives_the_turn_end_and_is_walked() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = spawn(&mut app, UnitType::Tank);
    let start = app.state_ref().unit(id).expect("unit").grid;
    let land = max_core::enums::Surface::LAND.bits();
    let target = (1..4)
        .flat_map(|radius| {
            (-radius..=radius).flat_map(move |dy| (-radius..=radius).map(move |dx| (dx, dy)))
        })
        .map(|(dx, dy)| Point::new(start.x + dx, start.y + dy))
        .find(|cell| {
            app.state_ref().units_at(*cell).is_empty()
                && app.state_ref().map().surface_at(cell.x, cell.y).bits() & land != 0
        })
        .expect("a free neighbouring cell");

    // LMB on the destination вЂ” a `ORDER_MOVE` order (packet 38) with a route.
    let point = screen_point(&app, target);
    let _ =
        max_app::handle_session_input(&mut app, max_app::InputEvent::MouseClick(point.0, point.1));

    // The arrows appear for the queued path (`GameManager_RenderScanRangeIndicators`).
    let mut arrows = 0;
    let mut queued = false;
    for _ in 0..40 {
        app.local_update(1.0 / 60.0);
        queued |= app.hud().preview.is_some()
            || app
                .state_ref()
                .unit(id)
                .is_some_and(|unit| unit.path.is_some());
        arrows = arrows.max(
            app.state_ref()
                .unit(id)
                .map(|unit| app.state_ref().path_markers(unit).len())
                .unwrap_or(0),
        );
    }

    assert!(queued, "the move order is queued (route/preview)");
    assert!(
        arrows >= 1,
        "the route arrows are shown for the queued path"
    );

    // End Turn does not lose the order: the unit walks the path to the target.
    let end_turn = max_render::layout::END_TURN_BUTTON;
    let button = (
        end_turn.ulx + end_turn.width / 2,
        end_turn.uly + end_turn.height / 2,
    );
    let _ = max_app::handle_session_input(
        &mut app,
        max_app::InputEvent::MouseClick(button.0, button.1),
    );

    for _ in 0..900 {
        app.local_update(1.0 / 60.0);
    }

    let position = app.state_ref().unit(id).map(|unit| unit.grid);
    assert!(
        position != Some(start),
        "the unit walks the ordered route through the turn end"
    );
}

// ---------------------------------------------------------------------------
// Defect 3: scrolling closes the command popup
// ---------------------------------------------------------------------------

/// `GameManager_UpdateMainMapView` (`game_manager.cpp:1745-1775`) calls
/// `GameManager_DeinitPopupButtons(false)` after a successful
/// `MAP_VIEW_SCROLL`: an open command menu must close when the map scrolls.
#[test]
fn scrolling_the_map_closes_the_unit_menu() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, id);
    assert!(app.unit_menu().is_some());

    // The pointer in the top scroll strip (`WINDOW_SCROLL_UP_WINDOW`, 50..590 Г— 0..8)
    // and a few frames of edge scrolling. The default `click_scroll` scrolls only
    // while the LMB is held (`GameManager_SetGridOffset`, `game_manager.cpp:6352`).
    app.note_mouse_move(300, 2);
    app.note_mouse_click(300, 2);

    let before = app.viewport().center();
    let mut scrolled = false;
    for _ in 0..60 {
        if app.update_scroll(1.0 / 60.0).is_some() {
            scrolled = true;
            break;
        }
    }

    assert!(scrolled, "the edge scroll moved the camera");
    assert_ne!(app.viewport().center(), before);
    assert!(
        app.unit_menu().is_none(),
        "scrolling the map closes the command menu (`DeinitPopupButtons`)"
    );
}

/// No scrolling вЂ” no closing: the popup stays while the camera does not move
/// (`GameManager_UpdateMainMapView` returns early when the view did not change).
#[test]
fn a_hover_without_scrolling_keeps_the_menu_open() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, id);

    // The pointer in the middle of the map вЂ” no scroll strip.
    app.note_mouse_move(400, 200);

    for _ in 0..10 {
        let _ = app.update_scroll(1.0 / 60.0);
    }

    assert!(
        app.unit_menu().is_some(),
        "without scrolling the menu stays open"
    );
}

/// Zooming is a `MAP_VIEW_ZOOM` update: it closes the popup too
/// (`game_manager.cpp:1769`).
#[test]
fn zooming_closes_the_unit_menu() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, id);

    // The panel zoom button goes through `session_panel_button`
    // в†’ `GameManager_UpdateMainMapView(MAP_VIEW_ZOOM, вЂ¦)`.
    let plus = max_render::layout::ZOOM_PLUS;
    let point = (plus.ulx + plus.width / 2, plus.uly + plus.height / 2);
    let _ =
        max_app::handle_session_input(&mut app, max_app::InputEvent::MouseClick(point.0, point.1));

    assert!(
        app.unit_menu().is_none(),
        "zooming closes the command menu (`DeinitPopupButtons`)"
    );
}

// ---------------------------------------------------------------------------
// Defects 4/5: the hand over actionable elements, the "no" sign over the rest
// ---------------------------------------------------------------------------

/// Over the command popup: the hand over an enabled item, `CURSOR_UNIT_NO_GO` over
/// a disabled one (`Cursor_GetDefaultWindowCursor`, `cursor.cpp:57-73`; the popup
/// buttons are `CURSOR_HAND` windows вЂ” `popupmenu.cpp:40`).
#[test]
fn the_cursor_is_a_hand_over_enabled_menu_items_and_no_go_over_disabled() {
    let Some(mut app) = game_app() else {
        return;
    };

    let id = spawn(&mut app, UnitType::Tank);
    open_menu(&mut app, id);

    let menu = app.unit_menu().expect("menu").clone();
    let enabled = menu
        .items
        .iter()
        .position(|item| item.enabled)
        .expect("an enabled item exists");
    let disabled = menu.items.iter().position(|item| !item.enabled);

    let point_of = |index: usize| {
        let rect = menu.button_rect(index);

        (rect.ulx + rect.width / 2, rect.uly + rect.height / 2)
    };

    let (cursor, _) = session_cursor(&app, Some(point_of(enabled)));
    assert_eq!(cursor, CursorId::Hand, "an enabled item вЂ” the hand");

    if let Some(disabled) = disabled {
        let (cursor, _) = session_cursor(&app, Some(point_of(disabled)));
        assert_eq!(
            cursor,
            CursorId::UnitNoGo,
            "a disabled item вЂ” the forbidden cursor"
        );
    }
}

/// Panel buttons are clickable вЂ” the hand; the statistics/log/coordinates areas of
/// the window are not вЂ” the "no" sign.
#[test]
fn the_cursor_is_a_hand_over_panel_buttons_and_no_go_over_inactive_areas() {
    let Some(mut app) = game_app() else {
        return;
    };

    // `PanelButton::EndTurn` / `Center` / `Minimap2x` вЂ” real buttons of the 1:1 chrome.
    for button in [
        max_app::panels::PanelButton::EndTurn,
        max_app::panels::PanelButton::Center,
        max_app::panels::PanelButton::Minimap2x,
    ] {
        let rect = button.rect();
        let (cursor, _) = session_cursor(
            &app,
            Some((rect.ulx + rect.width / 2, rect.uly + rect.height / 2)),
        );
        assert_eq!(cursor, CursorId::Hand, "{button:?}: a button вЂ” the hand");
    }

    // The statistics display (`STAT_WINDOW`) is not clickable.
    let stat = max_render::layout::STAT_WINDOW;
    let (cursor, _) = session_cursor(
        &app,
        Some((stat.ulx + stat.width / 2, stat.uly + stat.height / 2)),
    );
    assert_eq!(
        cursor,
        CursorId::UnitNoGo,
        "a non-interactive panel вЂ” the forbidden cursor"
    );

    // Hovering a map cell still gives a map cursor (not the panel one).
    let id = spawn(&mut app, UnitType::Tank);
    let grid = app.state_ref().unit(id).expect("unit").grid;
    let (cursor, _) = session_cursor(&app, Some(screen_point(&app, grid)));
    assert_ne!(
        cursor,
        CursorId::UnitNoGo,
        "over an own unit вЂ” not the forbidden cursor"
    );
}
