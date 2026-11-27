//! Frame test of the pixel movement: a ground unit and an aircraft walk **smoothly**
//! and the sprite is the simulation pixel on every frame.
//!
//! Oracle: `UnitInfo::Move` (`unitinfo.cpp:1730-1892`) advances `x/y` a few pixels
//! per tick, `AirPath::Execute` (`airpath.cpp:219-244`) does the same along the air
//! segment, and `UnitInfo::Render` draws the sprite exactly at `x/y`. The port's
//! movement budget (`max_core::MOVEMENT_CELLS_PER_SECOND`, four cells per second at
//! 60 Hz) replaces the `velocity` acceleration, and the frame draws every routed
//! unit on the simulation pixel — no interpolation, no whole-cell hop.
//!
//! The assertions are the defect's frame properties: the sprite step of a frame is
//! at most the budget window (4-5 px), never a whole cell (the old 64 px ground jump
//! and the old 16 px `AirPath` jump), and the route stops exactly on the center of
//! the destination cell.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::{App, AppConfig};
use max_core::access::unit_type_flags;
use max_core::enums::{unit_flags, Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{PathStep, Point};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{GroundPath, UnitId, UnitPath};

/// One simulation tick of the fixed 60 Hz step.
const FRAME_SECONDS: f32 = 1.0 / 60.0;

/// The whole-pixel step of one frame at the normal pace.
const MAX_PIXEL_STEP: i32 = 5;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn game_app() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("tests/fixtures/assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("pixel_motion_frame: no game data next to the repository");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"));
    let mut app = App::new(config);
    app.init_assets().expect("game assets");

    let mut setup = LocalSetup::default();
    setup.world = 0;
    setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    setup.timer = 0;
    app.start_local_game(&setup).expect("local match");

    Some(app)
}

/// A horizontal run of `count` free land cells (`None` — the map has none).
fn free_row(app: &App, count: i16) -> Option<Point> {
    for y in 10..(max_core::geo::MAP_HEIGHT - 10) {
        'x: for x in 10..(max_core::geo::MAP_WIDTH - 10 - count) {
            for step in 0..count {
                let grid = Point::new(x + step, y);
                let land = app.state_ref().map().surface_at(grid.x, grid.y).bits()
                    & Surface::LAND.bits()
                    != 0;

                if !land || !app.state_ref().units_at(grid).is_empty() {
                    continue 'x;
                }
            }

            return Some(Point::new(x, y));
        }
    }

    None
}

/// Clears the map and spawns `unit_type` on a free row.
fn spawn_on_free_row(app: &mut App, unit_type: UnitType, row: i16) -> Option<(UnitId, Point)> {
    for id in app.state_ref().unit_ids_in_order() {
        app.universe_mut().destroy_unit(id);
    }

    let start = free_row(app, row + 6)?;
    let id = app
        .universe_mut()
        .spawn_unit(unit_type, TeamId::RED, start, unit_type_flags(unit_type))
        .expect("unit spawned");

    Some((id, start))
}

/// The frame's sprite pixel of `id` (`App::visual_position` is in cells).
fn sprite_pixel(app: &App, id: UnitId) -> (i32, i32) {
    let (x, y) = app.visual_position(id).expect("sprite position");

    ((x * 64.0).round() as i32, (y * 64.0).round() as i32)
}

/// A ground route: the sprite advances 4-5 pixels per frame, never a whole cell,
/// passes through non-center pixels, and stops exactly on the destination center.
#[test]
fn a_ground_unit_is_drawn_smoothly_and_stops_on_the_cell_center() {
    let Some(mut app) = game_app() else {
        return;
    };
    let Some((mover, start)) = spawn_on_free_row(&mut app, UnitType::Tank, 12) else {
        eprintln!("pixel_motion_frame: no free land row");
        return;
    };

    let destination = Point::new(start.x + 10, start.y);
    {
        let unit = app.universe_mut().unit_mut(mover).expect("mover");
        unit.hits = 2000;
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::InProgress;
        unit.speed = 10;
        unit.angle = 2;
        unit.path = Some(UnitPath::Ground(GroundPath {
            end_x: destination.x,
            end_y: destination.y,
            step_index: 0,
            steps: vec![PathStep::new(1, 0); 10],
        }));
    }
    app.universe_mut().set_order_data(
        mover,
        max_core::game::OrderData {
            move_to: destination,
            fire_on: Point::ZERO,
        },
    );
    // The tests pin the normal pace (4 cells/s): `fast_movement` ("Double Unit
    // Steps") doubles it to 8-pixel passes.
    app.universe_mut().set_fast_movement(false);
    app.update_visual_units(0.0);

    let mut previous = sprite_pixel(&app, mover);
    let mut max_step = 0;
    let mut saw_off_center = false;
    let mut frames = 0usize;

    for _ in 0..600 {
        app.tick();
        app.update_visual_units(FRAME_SECONDS);
        frames += 1;

        let unit = app.state_ref().unit(mover).cloned().expect("mover");
        let now = sprite_pixel(&app, mover);
        let step = (now.0 - previous.0).abs().max((now.1 - previous.1).abs());

        assert_eq!(
            now,
            (i32::from(unit.x), i32::from(unit.y)),
            "frame {frames}: the sprite must be the simulation pixel"
        );
        assert!(
            step <= MAX_PIXEL_STEP,
            "frame {frames}: the sprite jumps {step} px (budget window {MAX_PIXEL_STEP}): \
             {previous:?} -> {now:?}"
        );
        assert!(
            step < 32,
            "frame {frames}: the sprite jumps half a cell ({step} px): {previous:?} -> {now:?}"
        );

        if (now.0 - 32) % 64 != 0 {
            saw_off_center = true;
        }

        max_step = max_step.max(step);
        previous = now;

        if unit.path.is_none() {
            break;
        }
    }

    assert!(frames > 80, "ten cells take ten budget windows: {frames} frames");
    assert!(
        saw_off_center,
        "the sprite must pass through pixels between the cell centers"
    );
    assert!(max_step >= 4, "the sprite must move (max step {max_step} px)");

    let unit = app.state_ref().unit(mover).cloned().expect("mover");
    assert_eq!(
        sprite_pixel(&app, mover),
        (
            i32::from(destination.x) * 64 + 32,
            i32::from(destination.y) * 64 + 32
        ),
        "the sprite stops exactly on the destination cell center"
    );
    assert_eq!(unit.grid, destination);
}

/// An aircraft route: the sprite advances a few pixels per frame along the straight
/// segment, never the old 16-pixel `AirPath` step, and lands exactly on the center
/// of the destination cell.
#[test]
fn an_aircraft_is_drawn_smoothly_and_lands_on_the_cell_center() {
    let Some(mut app) = game_app() else {
        return;
    };
    let Some((mover, start)) = spawn_on_free_row(&mut app, UnitType::Fighter, 12) else {
        eprintln!("pixel_motion_frame: no free land row");
        return;
    };

    let destination = Point::new(start.x + 10, start.y + 4);
    {
        let unit = app.universe_mut().unit_mut(mover).expect("fighter");
        unit.flags |= unit_flags::HOVERING;
        unit.hits = 2000;
    }
    app.universe_mut().set_order_data(
        mover,
        max_core::game::OrderData {
            move_to: destination,
            fire_on: Point::ZERO,
        },
    );
    app.universe_mut()
        .set_order(mover, UnitOrder::Move, UnitOrderState::Init);
    // The normal pace, as in the ground test.
    app.universe_mut().set_fast_movement(false);
    app.update_visual_units(0.0);

    let mut previous = sprite_pixel(&app, mover);
    let mut max_axis_step = 0;
    let mut frames = 0usize;

    for _ in 0..1_500 {
        app.tick();
        app.update_visual_units(FRAME_SECONDS);
        frames += 1;

        let unit = app.state_ref().unit(mover).cloned().expect("fighter");
        let now = sprite_pixel(&app, mover);
        let dx = (now.0 - previous.0).abs();
        let dy = (now.1 - previous.1).abs();
        let axis_step = dx.max(dy);

        assert_eq!(
            now,
            (i32::from(unit.x), i32::from(unit.y)),
            "frame {frames}: the sprite must be the simulation pixel"
        );
        assert!(
            axis_step <= MAX_PIXEL_STEP,
            "frame {frames}: the aircraft jumps {axis_step} px on an axis: \
             {previous:?} -> {now:?}"
        );
        assert!(
            axis_step < 16,
            "frame {frames}: the aircraft jumps a whole `AirPath` step ({axis_step} px): \
             {previous:?} -> {now:?}"
        );
        assert!(
            ((dx * dx + dy * dy) as f32).sqrt() <= 8.0,
            "frame {frames}: the aircraft moves more than the diagonal of a budget \
             window: {previous:?} -> {now:?}"
        );

        max_axis_step = max_axis_step.max(axis_step);
        previous = now;

        if unit.path.is_none() {
            break;
        }
    }

    assert!(frames > 100, "the aircraft flies for many frames: {frames}");
    assert!(
        max_axis_step >= 4,
        "the aircraft must fly (max axis step {max_axis_step} px)"
    );

    let unit = app.state_ref().unit(mover).cloned().expect("fighter");
    assert_eq!(
        sprite_pixel(&app, mover),
        (
            i32::from(destination.x) * 64 + 32,
            i32::from(destination.y) * 64 + 32
        ),
        "the aircraft lands exactly on the destination cell center"
    );
    assert_eq!(unit.grid, destination);
}
