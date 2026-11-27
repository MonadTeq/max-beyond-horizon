//! Playtest defect: a unit stopped by an intercept (`UnitEventEmergencyStop`) did
//! not walk to the halt cell — its sprite **jumped** there.
//!
//! # Oracle
//!
//! * The sprite is drawn at `UnitInfo::x/y`; `UnitInfo::Move` (`unitinfo.cpp:1730`)
//!   advances that pixel `velocity` per tick and `UnitInfo::Render` draws exactly
//!   there. The stop itself — `UnitEventEmergencyStop::Process`
//!   (`unitevents.cpp:38`) → `UnitInfo::StopMovement` (`unitinfo.cpp:3222`) →
//!   `BlockedOnPathRequest` (`:3628`) — only clears the path and the movement
//!   counters (`MoveFinished`, `:3654`); it never rewrites `x/y`. The sprite always
//!   arrives at the halt pixel by walking, never by a jump.
//! * The port's simulation moves a whole cell per tick while the frame walks the
//!   same route at `VISUAL_CELLS_PER_SECOND` (`App::update_visual_units`), so a
//!   stopped unit still has queued visual cells — the sprite has to pass them.
//!
//! # Defects
//!
//! * `update_visual_units` pulled the visual position to the simulation cell in the
//!   dead-unit branch (`*current = target`, the "corpse on the death cell" fix). An
//!   intercepted unit is killed on the halt cell while its sprite is still several
//!   cells behind — the player saw the sprite **teleport** into the stop cell.
//!   (The `VISUAL_TELEPORT_CELLS` branch — a jump of more than six cells in one
//!   simulation step, treated as a save/load or transport-style synchronization — is
//!   a separate policy and is not this defect.)
//!
//! # Frames under test
//!
//! * A surviving unit walks the queued route to the halt cell at the original speed
//!   and stops exactly there — the same sequence of frames, no jump.
//! * A unit killed on the halt cell **walks there too**: the corpse keeps its queued
//!   route, finishes it on the halt cell (the point of the first attack) and only
//!   then disappears. It never freezes — or vanishes — several cells short of it
//!   (see `intercept_explosion.rs` for the blast that rides the corpse).

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::{App, AppConfig};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{PathStep, Point};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{GroundPath, UnitId, UnitPath};

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
        eprintln!("intercept_visual_halt: no game data next to the repository");
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
                    & max_core::enums::Surface::LAND.bits()
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

/// Spawns the unit with the type flags of `SC_UNITS`.
fn spawn(app: &mut App, unit_type: UnitType, team: TeamId, grid: Point) -> UnitId {
    let flags = max_core::access::unit_type_flags(unit_type);

    app.universe_mut()
        .spawn_unit(unit_type, team, grid, flags)
        .expect("unit spawned")
}

/// The intercept scenario: a RED tank walks 8 cells along a free row toward a GREEN
/// tank standing 10 cells away. `hits` decides whether the reaction fire kills it.
///
/// The match is prepared by the caller: the application is large and a second copy
/// on the test stack overflows it (the helper only borrows it).
fn intercept_scenario(app: &mut App, hits: u16) -> Option<(UnitId, Point)> {
    for id in app.state_ref().unit_ids_in_order() {
        app.universe_mut().destroy_unit(id);
    }

    let Some(start) = free_row(app, 14) else {
        eprintln!("intercept_visual_halt: no free land row");
        return None;
    };

    app.universe_mut().set_enemy_halt(true);

    let mover = spawn(app, UnitType::Tank, TeamId::RED, start);
    let enemy = spawn(
        app,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(start.x + 10, start.y),
    );
    let destination = Point::new(start.x + 8, start.y);

    {
        let unit = app.universe_mut().unit_mut(mover).expect("mover");
        unit.hits = hits;
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::InProgress;
        unit.speed = 8;
        unit.path = Some(UnitPath::Ground(GroundPath {
            end_x: destination.x,
            end_y: destination.y,
            step_index: 0,
            steps: vec![PathStep::new(1, 0); 8],
        }));
    }
    app.universe_mut().set_order_data(
        mover,
        max_core::game::OrderData {
            move_to: destination,
            fire_on: Point::ZERO,
        },
    );
    app.universe_mut().spot_by_team(enemy, TeamId::RED);
    app.universe_mut().update_visibility_status(false);

    app.update_visual_units(0.0);

    // A few frames at the start cell: the turn-close drain then executes the whole
    // route in one simulation step, leaving the frame behind it.
    for _ in 0..5 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);
    }

    app.universe_mut().end_local_turn(TeamId::RED);

    Some((mover, destination))
}

/// The largest sprite step a frame may take: the visual speed times the frame time.
///
/// `fast_movement` ("Double Unit Steps") doubles the visual speed
/// (`App::update_visual_units`, the C++ `UnitInfo::Move` `velocity += 2`).
fn max_frame_step(app: &App) -> f32 {
    let scale = if app.preferences().fast_movement {
        2.0
    } else {
        1.0
    };

    max_app::VISUAL_CELLS_PER_SECOND * scale * (1.0 / 60.0) + 1e-3
}

#[test]
fn a_surviving_unit_walks_to_the_halt_cell_without_a_jump() {
    let Some(mut app) = game_app() else {
        return;
    };
    // The reaction fire must not kill the mover: the walk to the halt cell is the
    // frame under test.
    let Some((mover, destination)) = intercept_scenario(&mut app, 2000) else {
        return;
    };

    let start = app.visual_position(mover).expect("mover position");
    let max_frame_step = max_frame_step(&app);
    let mut previous = start;
    let mut steps = 0usize;
    let mut moved = 0usize;
    let mut halt = None;

    for _ in 0..300 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);

        let Some(unit) = app.state_ref().unit(mover).cloned() else {
            break;
        };
        let now = app.visual_position(mover).expect("mover position");

        // `UnitInfo::Move` walks the pixels; the sprite may not jump.
        let step = (now.0 - previous.0).abs().max((now.1 - previous.1).abs());
        assert!(
            step <= max_frame_step,
            "frame {steps}: the sprite of the intercepted unit teleports: {previous:?} -> {now:?}"
        );

        // The route is a straight row, so the sprite only moves forward: nothing is
        // ever re-targeted behind it (`StopMovement` clears the path, not the pixel).
        assert!(
            now.0 >= previous.0 - 1e-4,
            "frame {steps}: the sprite moved backwards: {previous:?} -> {now:?}"
        );
        if step > 1e-4 {
            moved += 1;
        }
        previous = now;
        steps += 1;

        assert_ne!(
            unit.grid, destination,
            "the intercept must stop the unit before its destination"
        );
        assert!(unit.hits > 0, "the mover was killed: {unit:?}");

        let halt_cell = (f32::from(unit.grid.x), f32::from(unit.grid.y));
        if now == halt_cell {
            halt = Some(halt_cell);
            break;
        }
    }

    let halt = halt.expect("the sprite must reach the halt cell");
    assert_eq!(
        app.visual_position(mover),
        Some(halt),
        "the sprite stops exactly on the halt cell"
    );
    assert_eq!(
        app.state_ref().unit(mover).map(|unit| unit.grid),
        Some(Point::new(halt.0 as i16, halt.1 as i16)),
        "the halt cell is the simulation cell"
    );
    // A full animation, not a jump: the gap is several cells and the frame passes it
    // at `VISUAL_CELLS_PER_SECOND` (times `fast_movement`), i.e. many frames per cell.
    assert!(
        steps >= 30 && moved >= 30,
        "the frame must walk to the halt cell, not jump: {steps} frames, {moved} moving"
    );
    // The intercept left a multi-cell gap between the sprite and the halt cell; the
    // frame covered it cell by cell.
    let gap = halt.0 - start.0;
    assert!(
        gap > 3.0,
        "the scenario must leave the sprite behind the halt cell: {start:?} -> {halt:?}"
    );
}

/// The intercept kills the mover on the halt cell: its sprite must still **walk**
/// there — no jump and no freeze in mid-air — and the walk must end exactly on that
/// cell, the point of the first attack.
///
/// Oracle: `UnitEventEmergencyStop::Process` (`unitevents.cpp:38`) →
/// `BlockedOnPathRequest` (`unitinfo.cpp:3628`) stops the mover on the cell where it
/// entered the enemy's weapon range and the reaction fire kills it there;
/// `UnitInfo::StopMovement` (`:3222`) clears the path and never rewrites `x/y`, so
/// the original shows the corpse and its blast on the halt cell.
///
/// The port's frame lags the cell-per-tick simulation by several cells, so the
/// corpse finishes its queued route at `VISUAL_CELLS_PER_SECOND` and the frame keeps
/// it drawable after `unit_order::progress_explosion` removes the unit
/// (`max_app::CorpseVisual`): the sprite arrives on the halt cell instead of freezing
/// short of it.
#[test]
fn a_unit_killed_on_the_halt_cell_walks_there_instead_of_jumping() {
    let Some(mut app) = game_app() else {
        return;
    };
    let Some((mover, destination)) = intercept_scenario(&mut app, 1) else {
        return;
    };

    let max_frame_step = max_frame_step(&app);
    let mut previous = app.visual_position(mover);
    let mut saw_death = false;
    let mut death_cell = None;
    let mut last_drawn = None;
    let mut frames = 0usize;

    for _ in 0..400 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);
        frames += 1;

        let now = app.visual_position(mover);

        if let (Some(previous), Some(now)) = (previous, now) {
            let step = (now.0 - previous.0).abs().max((now.1 - previous.1).abs());
            assert!(
                step <= max_frame_step,
                "frame {frames}: the sprite of the unit killed on the halt cell teleports: \
                 {previous:?} -> {now:?}"
            );
            // The route is a straight row: nothing is ever re-targeted behind the
            // sprite (`StopMovement` clears the path, not the pixel).
            assert!(
                now.0 >= previous.0 - 1e-4,
                "frame {frames}: the sprite moved backwards: {previous:?} -> {now:?}"
            );
        }
        if let Some(now) = now {
            last_drawn = Some(now);
        }
        previous = now;

        if let Some(unit) = app.state_ref().unit(mover) {
            assert_ne!(
                unit.grid, destination,
                "the intercept must stop the unit before its destination"
            );
            if unit.hits == 0 {
                saw_death = true;
                // The cell the killing shot landed on: the halt cell, the point of
                // the first attack.
                death_cell = Some((f32::from(unit.grid.x), f32::from(unit.grid.y)));
            }
        }
    }

    assert!(saw_death, "the intercept killed the mover on the halt cell");

    let death_cell = death_cell.expect("the death cell of the mover");
    let end = last_drawn.expect("the corpse is drawn while it walks");
    assert!(
        (end.0 - death_cell.0).abs() < 1e-3 && (end.1 - death_cell.1).abs() < 1e-3,
        "the corpse must end on the halt cell — the point where the anti-aircraft \
         started attacking: {end:?} != {death_cell:?}"
    );
    assert!(
        app.visual_position(mover).is_none(),
        "the corpse is gone once the walk is over"
    );
}
