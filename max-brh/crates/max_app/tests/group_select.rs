//! Group selection with the mouse — rectangle drag, Shift+click, the member
//! frames and the group move order.
//!
//! Oracle: `Access_MultiSelect` (`access.cpp:2177-2253`), the Shift+click branch of
//! `GameManager_UpdateSelection` (`game_manager.cpp:6303-6334`), the release that
//! runs the frame (`:6513-6525`), `Access_GroupAttackOrder` (`access.cpp:1218-1289`)
//! and the member frames of `DrawMap_RenderColorsDisplay` (`drawmap.cpp:674-690`).
//!
//! The match is synthetic (`NewGameConfig::p2_reference` + a LAND surface), so the
//! test needs no game assets and the port renders the frame without the 1:1 chrome:
//! mouse coordinates are frame pixels (`App::chrome_active == false`).

use max_app::hud;
use max_app::{handle_session_input, App, AppConfig, InputEvent, NewGameConfig};
use max_core::enums::{Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{Point, MAP_TILES};
use max_core::map::SurfaceMap;
use max_core::team::TeamId;
use max_core::unit::UnitId;

/// A synthetic match: both teams landed on an all-land surface, the camera sits on
/// the player's start base.
fn game_app() -> App {
    let config = NewGameConfig::p2_reference();
    let surface = SurfaceMap::from_tiles([Surface::LAND; MAP_TILES]);
    let mut app = App::new(AppConfig::default());
    let report = app.new_game_on_surface(&config, &surface);

    assert!(report.units > 0, "the match has its starting armies");
    app.tick();

    app
}

/// Screen point of the cell center in the current (chrome-less) frame.
fn cell_point(app: &App, grid: Point) -> (i32, i32) {
    hud::tile_center(&app.map_viewport(), grid)
}

fn is_free_land(app: &App, grid: Point) -> bool {
    let land = Surface::LAND.bits();
    let state = app.state_ref();

    state.units_at(grid).is_empty() && state.map().surface_at(grid.x, grid.y).bits() & land != 0
}

/// Three free LAND cells near the camera center, in a 2×2 cluster:
/// `(left, right)` of the upper row and the left cell of the lower row.
fn free_cluster(app: &App) -> (Point, Point, Point) {
    let (center_x, center_y) = app.map_viewport().center();
    let center = Point::new((center_x / 64) as i16, (center_y / 64) as i16);

    let on_screen = |grid: Point| {
        let (x, y) = cell_point(app, grid);

        (64..=576).contains(&x) && (64..=400).contains(&y)
    };

    for radius in 0..6 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let left = Point::new(center.x + dx, center.y + dy);
                let right = Point::new(left.x + 1, left.y);
                let below = Point::new(left.x, left.y + 1);
                let diagonal = Point::new(left.x + 1, left.y + 1);

                if !on_screen(left) || !on_screen(right) || !on_screen(below) {
                    continue;
                }

                // The scan of `Access_MultiSelect` covers the whole rectangle, so the
                // fourth cell must be free of own units as well.
                if is_free_land(app, left)
                    && is_free_land(app, right)
                    && is_free_land(app, below)
                    && is_free_land(app, diagonal)
                {
                    return (left, right, below);
                }
            }
        }
    }

    panic!("no free 2x2 land cluster near the camera ({center:?})");
}

fn spawn_tank(app: &mut App, grid: Point) -> UnitId {
    let flags = max_core::access::unit_type_flags(UnitType::Tank);

    app.universe_mut()
        .spawn_unit(UnitType::Tank, TeamId::RED, grid, flags)
        .expect("tank spawned")
}

fn click(app: &mut App, point: (i32, i32)) {
    handle_session_input(app, InputEvent::MouseClick(point.0, point.1));
}

/// The rectangle drag: press, one pointer move past the 16 px threshold, release.
fn drag(app: &mut App, from: (i32, i32), to: (i32, i32)) {
    click(app, from);
    handle_session_input(app, InputEvent::MouseMove(to.0, to.1));
    handle_session_input(app, InputEvent::MouseRelease(to.0, to.1));
}

fn log_has(app: &App, prefix: &str) -> bool {
    app.hud()
        .log
        .lines()
        .iter()
        .any(|line| line.starts_with(prefix))
}

/// Runs the body on a thread with a larger stack: a debug `App` scene plus the
/// 640×480 frame needs more than the default 2 MiB test stack.
fn with_big_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(body)
        .expect("test thread")
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

#[test]
fn drag_frame_creates_the_group_and_focuses_the_anchor() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, below) = free_cluster(&app);
        let leader = spawn_tank(&mut app, left);
        let wing = spawn_tank(&mut app, right);
        let tail = spawn_tank(&mut app, below);

        // Select the leader first: the frame's anchor is `GameManager_SelectedUnit`
        // (`access.cpp:2188-2199`).
        let left_point = cell_point(&app, left);
        click(&mut app, left_point);
        assert_eq!(
            app.hud().selected,
            Some(leader),
            "the click selects the leader"
        );

        // The press on the second tank arms the frame (the anchor stays the leader),
        // the release past the third tank runs `Access_MultiSelect`.
        let right_point = cell_point(&app, right);
        let below_point = cell_point(&app, below);
        drag(&mut app, right_point, below_point);

        assert_eq!(
            app.state_ref().group_members_of(leader),
            Some(vec![leader, wing, tail]),
            "the frame selects the three own tanks"
        );
        assert_eq!(
            app.hud().selected,
            Some(leader),
            "`GameManager_MenuUnitSelect(parent)` focuses the group leader"
        );
        assert_eq!(app.hud().multi_select, None, "the frame is finished");
    });
}

#[test]
fn a_short_press_without_a_drag_does_not_create_a_group() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, _below) = free_cluster(&app);
        let leader = spawn_tank(&mut app, left);
        let wing = spawn_tank(&mut app, right);

        let left_point = cell_point(&app, left);
        click(&mut app, left_point);
        // Press and release in place: the 16 px threshold is not passed, so this is
        // a plain click (`GameManager_RenderState == 1`, `game_manager.cpp:6516-6523`).
        let point = cell_point(&app, right);
        click(&mut app, point);
        handle_session_input(&mut app, InputEvent::MouseRelease(point.0, point.1));

        assert_eq!(
            app.state_ref().group_members_of(wing),
            None,
            "a click without a drag creates no group"
        );
        assert_eq!(app.hud().selected, Some(wing), "the click itself selects");
        assert_eq!(app.state_ref().group_members_of(leader), None);
    });
}

#[test]
fn shift_click_adds_and_removes_a_member() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, below) = free_cluster(&app);
        let leader = spawn_tank(&mut app, left);
        let wing = spawn_tank(&mut app, right);
        let tail = spawn_tank(&mut app, below);

        let left_point = cell_point(&app, left);
        let right_point = cell_point(&app, right);
        let below_point = cell_point(&app, below);

        click(&mut app, left_point);
        drag(&mut app, right_point, below_point);
        assert_eq!(
            app.state_ref().group_members_of(leader),
            Some(vec![leader, wing, tail])
        );

        // Shift+click on a member removes it (`unit2->GetFirstFromUnitList() ==
        // unit1` -> `ClearUnitList`, `game_manager.cpp:6310-6311`).
        app.set_shift_pressed(true);
        click(&mut app, right_point);
        assert_eq!(
            app.state_ref().group_members_of(leader),
            Some(vec![leader, tail]),
            "Shift+click removes the clicked member"
        );
        assert_eq!(
            app.hud().selected,
            Some(leader),
            "the selection stays the leader"
        );

        // Shift+click on a unit outside the group adds it at the back
        // (`unit2->AssignUnitList(unit1)`, `:6320-6321`).
        click(&mut app, right_point);
        assert_eq!(
            app.state_ref().group_members_of(leader),
            Some(vec![leader, tail, wing]),
            "Shift+click adds the unit back"
        );

        // Shift on the leader itself dissolves its group
        // (`GetFirstFromUnitList() == unit1`).
        click(&mut app, left_point);
        assert_eq!(
            app.state_ref().group_members_of(leader),
            None,
            "Shift+click on the head dissolves the group"
        );
    });
}

#[test]
fn group_move_expands_to_the_members_with_the_shared_budget() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, below) = free_cluster(&app);
        let leader = spawn_tank(&mut app, left);
        let wing = spawn_tank(&mut app, right);
        let tail = spawn_tank(&mut app, below);

        let left_point = cell_point(&app, left);
        let right_point = cell_point(&app, right);
        let below_point = cell_point(&app, below);

        click(&mut app, left_point);
        drag(&mut app, right_point, below_point);
        assert_eq!(
            app.state_ref().group_members_of(leader),
            Some(vec![leader, wing, tail])
        );

        // The destination: a free cell a few cells to the right of the cluster.
        let mut destination = Point::new(left.x + 3, left.y + 2);
        for dx in 0..6 {
            let candidate = Point::new(left.x + 3 + dx, left.y + 2);
            if is_free_land(&app, candidate) {
                destination = candidate;
                break;
            }
        }

        // The group leader gets the order; the simulation expands it to the members
        // (`Access_GroupAttackOrder` via `UnitsManager_InitUnitPath`).
        let destination_point = cell_point(&app, destination);
        click(&mut app, destination_point);
        assert!(
            log_has(&app, "Move (group 3)"),
            "the group move is issued to the leader: {:?}",
            app.hud().log.lines()
        );

        app.tick();

        let formation = |member: Point| {
            Point::new(
                destination.x + (member.x - left.x),
                destination.y + (member.y - left.y),
            )
        };

        let speed = app
            .state_ref()
            .unit(leader)
            .map(|unit| {
                unit.base_values
                    .get_attribute(max_core::unit_values::ATTRIB_SPEED)
            })
            .expect("leader");
        let budget = (speed + 1) as u16;

        for (id, member) in [(leader, left), (wing, right), (tail, below)] {
            let unit = app.state_ref().unit(id).expect("member");
            assert_eq!(unit.orders, UnitOrder::Move, "every member moves");
            assert!(matches!(
                unit.state,
                UnitOrderState::NewOrder | UnitOrderState::IssuingPath
            ));
            assert_eq!(
                app.state_ref().order_data(id).move_to,
                formation(member),
                "the formation is kept relative to the leader"
            );
            assert_eq!(
                unit.group_speed, budget,
                "the shared budget is min(speed) + 1"
            );
        }

        // The barrier releases the group: every member holds its path and starts.
        for _ in 0..4 {
            app.tick();
        }
        for (id, home) in [(leader, left), (wing, right), (tail, below)] {
            let unit = app.state_ref().unit(id).expect("member");
            assert!(
                unit.path.is_some() || unit.grid != home,
                "the member started after the barrier ({:?})",
                unit.state
            );
        }
    });
}

#[test]
fn a_selected_group_draws_the_leader_selector_and_the_member_frames() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, below) = free_cluster(&app);
        let leader = spawn_tank(&mut app, left);
        let wing = spawn_tank(&mut app, right);
        let _tail = spawn_tank(&mut app, below);

        let left_point = cell_point(&app, left);
        let right_point = cell_point(&app, right);
        let below_point = cell_point(&app, below);

        click(&mut app, left_point);
        // Without a group only the leader selector is drawn.
        let (_, stats) = app.render_frame_with_stats();
        assert_eq!(stats.selector_units, 1, "a single selection: one selector");

        drag(&mut app, right_point, below_point);
        let (frame, stats) = app.render_frame_with_stats();

        assert_eq!(
            stats.selector_units, 3,
            "the leader selector plus one yellow frame per member"
        );

        // The member frame is the 1 px `COLOR_YELLOW` rectangle of
        // `DrawMap_RenderColorsDisplay(unit)` at the unit's bounds.
        let unit = app.state_ref().unit(wing).expect("member");
        let visual = (
            f32::from(unit.x) / max_core::unit_order::GFX_MAP_TILE_SIZE as f32,
            f32::from(unit.y) / max_core::unit_order::GFX_MAP_TILE_SIZE as f32,
        );
        let (world_x, world_y) = max_app::render::unit_anchor_world(unit, visual);
        let viewport = app.map_viewport();
        let (screen_x, screen_y) = viewport.world_to_screen(world_x - 32, world_y - 32);
        // The synthetic match has no assets, so the overlays use the fallback yellow
        // (see `App::group_overlay_color`).
        let yellow = max_render::buffer::Rgba::from_rgb([255, 255, 0]);

        assert_eq!(
            frame.get(screen_x + 1, screen_y + 1),
            Some(yellow),
            "the top-left corner of the member frame is yellow"
        );

        // The leader keeps the full selector (not the member frame): its corner is
        // the blinking marker color, not yellow.
        let leader_unit = app.state_ref().unit(leader).expect("leader");
        let visual = (
            f32::from(leader_unit.x) / max_core::unit_order::GFX_MAP_TILE_SIZE as f32,
            f32::from(leader_unit.y) / max_core::unit_order::GFX_MAP_TILE_SIZE as f32,
        );
        let (world_x, world_y) = max_app::render::unit_anchor_world(leader_unit, visual);
        let (screen_x, screen_y) = viewport.world_to_screen(world_x - 32, world_y - 32);
        assert_ne!(
            frame.get(screen_x + 1, screen_y + 1),
            Some(yellow),
            "the leader draws the selector, not a member frame"
        );
    });
}

#[test]
fn the_frame_is_drawn_while_the_pointer_moves() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, below) = free_cluster(&app);
        let _leader = spawn_tank(&mut app, left);
        let _wing = spawn_tank(&mut app, right);
        let _tail = spawn_tank(&mut app, below);

        let from = cell_point(&app, right);
        let to = cell_point(&app, below);

        click(&mut app, from);
        handle_session_input(&mut app, InputEvent::MouseMove(to.0, to.1));

        let drag = app.hud().multi_select_frame().expect("the drag is armed");
        let (upper, lower) = drag.bounds().expect("the frame has both corners");
        let (frame, stats) = app.render_frame_with_stats();

        // `GameManager_RenderMultiSelectIndicator` (`game_manager.cpp:997-1035`):
        // four `COLOR_YELLOW` lines between the two corners.
        let viewport = app.map_viewport();
        let (ulx, uly) = viewport.world_to_screen(upper.0, upper.1);
        let (lrx, lry) = viewport.world_to_screen(lower.0, lower.1);
        // The synthetic match has no assets, so the overlays use the fallback yellow
        // (see `App::group_overlay_color`).
        let yellow = max_render::buffer::Rgba::from_rgb([255, 255, 0]);

        assert!(stats.overlay_pixels > 0, "the frame draws overlay pixels");
        // The four edges are drawn between the two corners; a couple of corners may
        // coincide with a HUD pixel, so the count only has to be positive.
        let yellow_pixels = (0..frame.height() as i32)
            .flat_map(|y| (0..frame.width() as i32).map(move |x| (x, y)))
            .filter(|(x, y)| frame.get(*x, *y) == Some(yellow))
            .count();
        assert!(yellow_pixels >= 3, "the frame is drawn in the overlay color");
        assert_eq!(
            frame.get(ulx, uly),
            Some(yellow),
            "the top-left corner of the frame is yellow"
        );
        assert_eq!(
            frame.get(lrx, uly),
            Some(yellow),
            "the top-right corner of the frame is yellow"
        );
        assert_eq!(
            frame.get(ulx, lry),
            Some(yellow),
            "the bottom-left corner of the frame is yellow"
        );
    });
}

/// A free LAND cell near the camera, at least `gap` cells away from every cell of
/// `taken` (so the newcomer is clearly outside the rectangle).
fn free_spot(app: &App, taken: &[Point], gap: i16) -> Point {
    let (center_x, center_y) = app.map_viewport().center();
    let center = Point::new((center_x / 64) as i16, (center_y / 64) as i16);

    let on_screen = |grid: Point| {
        let (x, y) = cell_point(app, grid);

        (64..=576).contains(&x) && (64..=400).contains(&y)
    };

    for radius in 0..10 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(center.x + dx, center.y + dy);

                if grid.x < 1
                    || grid.y < 1
                    || grid.x + 1 >= max_core::geo::MAP_WIDTH
                    || grid.y + 1 >= max_core::geo::MAP_HEIGHT
                    || !on_screen(grid)
                    || !is_free_land(app, grid)
                {
                    continue;
                }

                if taken
                    .iter()
                    .any(|cell| (cell.x - grid.x).abs() <= gap && (cell.y - grid.y).abs() <= gap)
                {
                    continue;
                }

                return grid;
            }
        }
    }

    panic!("no free cell near the camera ({center:?})");
}

/// A free 2×2 LAND block for a building, away from the group.
fn free_building_site(app: &App, taken: &[Point]) -> Point {
    let (center_x, center_y) = app.map_viewport().center();
    let center = Point::new((center_x / 64) as i16, (center_y / 64) as i16);

    let on_screen = |grid: Point| {
        let (x, y) = cell_point(app, grid);

        (64..=576).contains(&x) && (64..=400).contains(&y)
    };

    for radius in 0..10 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let site = Point::new(center.x + dx, center.y + dy);
                let cells = [
                    site,
                    Point::new(site.x + 1, site.y),
                    Point::new(site.x, site.y + 1),
                    Point::new(site.x + 1, site.y + 1),
                ];

                if site.x < 1
                    || site.y < 1
                    || site.x + 2 >= max_core::geo::MAP_WIDTH
                    || site.y + 2 >= max_core::geo::MAP_HEIGHT
                    || !on_screen(site)
                    || !on_screen(cells[3])
                {
                    continue;
                }

                if !cells.iter().all(|cell| is_free_land(app, *cell)) {
                    continue;
                }

                if taken.iter().any(|cell| {
                    cells.iter().any(|building| {
                        (cell.x - building.x).abs() <= 1 && (cell.y - building.y).abs() <= 1
                    })
                }) {
                    continue;
                }

                return site;
            }
        }
    }

    panic!("no free 2x2 building site near the camera ({center:?})");
}

fn spawn_building(app: &mut App, grid: Point) -> UnitId {
    let flags = max_core::access::unit_type_flags(UnitType::MiningStation);

    app.universe_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, grid, flags)
        .expect("building spawned")
}

/// Builds the three-tank group of the drag tests and returns the selection points.
///
/// The leader is selected first (the frame anchor), the press on the wing and the
/// release past the tail run `Access_MultiSelect`.
fn drag_group(app: &mut App) -> (Point, Point, Point, UnitId, UnitId, UnitId) {
    let (left, right, below) = free_cluster(app);
    let leader = spawn_tank(app, left);
    let wing = spawn_tank(app, right);
    let tail = spawn_tank(app, below);

    click(app, cell_point(app, left));
    drag(app, cell_point(app, right), cell_point(app, below));

    assert_eq!(
        app.state_ref().group_members_of(leader),
        Some(vec![leader, wing, tail]),
        "the drag created the three-unit group"
    );

    (left, right, below, leader, wing, tail)
}

#[test]
fn clicking_another_unit_dissolves_the_group_and_focuses_it() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, below, leader, wing, tail) = drag_group(&mut app);

        let spot = free_spot(&app, &[left, right, below], 3);
        let newcomer = spawn_tank(&mut app, spot);
        let spot_point = cell_point(&app, spot);
        click(&mut app, spot_point);

        assert_eq!(
            app.hud().selected,
            Some(newcomer),
            "the click focuses the newcomer"
        );
        for member in [leader, wing, tail] {
            assert_eq!(
                app.state_ref().group_members_of(member),
                None,
                "`GameManager_ClickUnit` (`game_manager.cpp:6265-6289`) + the port's reset: \
                 the group focused before the click is gone"
            );
        }
        assert!(
            log_has(&app, "Group reset"),
            "the reset is reported: {:?}",
            app.hud().log.lines()
        );
    });
}

#[test]
fn clicking_a_group_member_dissolves_the_group() {
    with_big_stack(|| {
        let mut app = game_app();
        let (_left, right, _below, leader, wing, tail) = drag_group(&mut app);

        let right_point = cell_point(&app, right);
        click(&mut app, right_point);

        assert_eq!(
            app.hud().selected,
            Some(wing),
            "the member becomes the focus"
        );
        for member in [leader, wing, tail] {
            assert_eq!(
                app.state_ref().group_members_of(member),
                None,
                "`unit2->ClearUnitList()` (`game_manager.cpp:6267`): the clicked member leaves \
                 the group and the group is reset"
            );
        }
        assert!(log_has(&app, "Group reset"));
    });
}

#[test]
fn clicking_a_building_dissolves_the_group() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, below, leader, wing, tail) = drag_group(&mut app);

        let site = free_building_site(&app, &[left, right, below]);
        let building = spawn_building(&mut app, site);
        let site_point = cell_point(&app, site);
        click(&mut app, site_point);

        assert_eq!(
            app.hud().selected,
            Some(building),
            "the standalone building takes the focus"
        );
        for member in [leader, wing, tail] {
            assert_eq!(
                app.state_ref().group_members_of(member),
                None,
                "focus on a building resets the rectangle selection"
            );
        }
        assert!(log_has(&app, "Group reset"));
    });
}

#[test]
fn right_click_on_a_free_cell_dissolves_the_group() {
    with_big_stack(|| {
        let mut app = game_app();
        let (left, right, below, leader, wing, tail) = drag_group(&mut app);

        let spot = free_spot(&app, &[left, right, below], 2);
        let (x, y) = cell_point(&app, spot);
        handle_session_input(&mut app, InputEvent::RightClick(x, y));

        assert_eq!(
            app.hud().selected,
            None,
            "RMB on a free cell drops the focus"
        );
        for member in [leader, wing, tail] {
            assert_eq!(
                app.state_ref().group_members_of(member),
                None,
                "the group is reset with the focus (`GameManager_MenuUnitSelect(nullptr)`)"
            );
        }
        assert!(log_has(&app, "Group reset"));
        assert!(log_has(&app, "Cancelled"));
    });
}
