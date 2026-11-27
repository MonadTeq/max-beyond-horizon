//! Playtest defect: a unit stopped by an intercept (`UnitEventEmergencyStop`) was
//! attacked and killed **ahead of its sprite**.
//!
//! # Oracle
//!
//! * The sprite is drawn at `UnitInfo::x/y`; `UnitInfo::Move` (`unitinfo.cpp:1730`)
//!   advances that pixel `velocity` per tick and `UnitInfo::Render` draws exactly
//!   there. The stop itself — `UnitEventEmergencyStop::Process`
//!   (`unitevents.cpp:38`) → `UnitInfo::StopMovement` (`unitinfo.cpp:3222`) →
//!   `BlockedOnPathRequest` (`:3628`) — only clears the path and the movement
//!   counters (`MoveFinished`, `:3654`); it never rewrites `x/y`.
//! * The original moves the simulation and the sprite together, so the halt cell is
//!   the cell the player sees the unit stopped on.
//!
//! # Defect
//!
//! The turn close walked the whole remaining route **inside one `end_local_turn`
//! call**, while the frame interpolated the same route at
//! `VISUAL_CELLS_PER_SECOND`; reaction fire is resolved in simulation cells, so the
//! intercept, the shot and the death happened several cells ahead of the sprite.
//!
//! # Fix under test
//!
//! The simulation paces every routed unit by the per-tick movement budget
//! (`max_core::MOVEMENT_CELLS_PER_SECOND`, alone and during the turn close), and
//! `App::update_visual_units` draws a routed unit on the **simulation** cell — no
//! interpolation, no corpse walk. A ground step is a whole cell, so the sprite steps
//! cell by cell and is always exactly where the simulation is.
//!
//! # Frames under test
//!
//! * A surviving unit steps to the halt cell and stops exactly there — the sprint
//!   cell the simulation entered, never one behind it.
//! * A unit killed on the halt cell is drawn there when the killing shot lands, and
//!   its sprite disappears with the hull — no walk back into the death cell.
//!
//! (See `intercept_explosion.rs` for the blast drawn on the death pixel.)

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

/// The largest sprite step a frame may take: one cell.
///
/// The simulation itself walks a routed unit a cell per budget window
/// (`max_core::MOVEMENT_CELLS_PER_SECOND`) and the frame draws the sprite on the
/// simulation cell, so a frame never moves the sprite by more than the one cell the
/// pass entered — in the active turn and during the turn close alike.
const DRAIN_CELL_STEP: f32 = 1.0 + 1e-3;

#[test]
fn a_surviving_unit_steps_to_the_halt_cell_with_its_simulation_cell() {
    let Some(mut app) = game_app() else {
        return;
    };
    // The reaction fire must not kill the mover: the walk to the halt cell is the
    // frame under test.
    let Some((mover, destination)) = intercept_scenario(&mut app, 2000) else {
        return;
    };

    let start = app.visual_position(mover).expect("mover position");
    let mut previous = start;
    let mut steps = 0usize;
    let mut hops = 0usize;
    let mut halt = None;
    // The first drain frame re-synchronizes the sprite with the simulation, so it may
    // cover the lag of the player's own turn — see the check below.
    let mut resynced = false;

    for _ in 0..400 {
        let closing = app.turn_close_pending();
        app.tick();
        app.update_visual_units(1.0 / 60.0);

        let Some(unit) = app.state_ref().unit(mover).cloned() else {
            break;
        };
        let now = app.visual_position(mover).expect("mover position");
        // The simulation **pixel** in cells — what the frame draws.
        let sim = (
            f32::from(unit.x) / 64.0,
            f32::from(unit.y) / 64.0,
        );
        let step = (now.0 - previous.0).abs().max((now.1 - previous.1).abs());

        // The sprite is already on the simulation pixel when the turn closes: the
        // simulation paces the movement in the active turn too. The first closing
        // frame is still checked as a phase boundary — it must be on the simulation
        // pixel, and every following frame steps a few pixels.
        if closing && !resynced {
            resynced = true;
            assert_eq!(
                now, sim,
                "frame {steps}: the re-synchronization must land on the simulation pixel"
            );
        } else {
            assert!(
                step <= DRAIN_CELL_STEP,
                "frame {steps}: the sprite of the intercepted unit jumps {step:.4} \
                 cells: {previous:?} -> {now:?}"
            );
        }

        // The route is a straight row, so the sprite only moves forward.
        assert!(
            now.0 >= previous.0 - 1e-4,
            "frame {steps}: the sprite moved backwards: {previous:?} -> {now:?}"
        );

        // During the turn close the sprite **is** the simulation pixel.
        if closing {
            assert_eq!(
                now, sim,
                "frame {steps}: the sprite must be on the simulation pixel"
            );
            if step > 1e-4 {
                hops += 1;
            }
        }

        previous = now;
        steps += 1;

        assert_ne!(
            unit.grid, destination,
            "the intercept must stop the unit before its destination"
        );
        assert!(unit.hits > 0, "the mover was killed: {unit:?}");

        if unit.path.is_none() {
            halt = Some(now);
            break;
        }
    }

    let halt = halt.expect("the sprite must reach the halt pixel");
    assert_eq!(
        app.visual_position(mover),
        Some(halt),
        "the sprite stops exactly on the halt pixel"
    );
    let halted = app.state_ref().unit(mover).cloned().expect("halted unit");
    assert_eq!(
        halted.grid,
        Point::new(halt.0.floor() as i16, halt.1.floor() as i16),
        "the halt cell is the simulation cell"
    );
    // The intercept left a multi-cell gap between the sprite and the halt cell; the
    // drain covered it pixel by pixel.
    let gap = halt.0 - start.0;
    assert!(
        gap > 1.0,
        "the scenario must leave the sprite behind the halt cell: {start:?} -> {halt:?}"
    );
    assert!(
        hops >= 2,
        "the drain must walk the cells one by one: {hops} steps in {steps} frames"
    );
}

/// The intercept kills the mover on the halt cell: the sprite is drawn there when
/// the killing shot lands, and it disappears with the hull — no walk back into the
/// death cell, no freeze in mid-air.
///
/// Oracle: `UnitEventEmergencyStop::Process` (`unitevents.cpp:38`) →
/// `BlockedOnPathRequest` (`unitinfo.cpp:3628`) stops the mover on the cell where it
/// entered the enemy's weapon range and the reaction fire kills it there;
/// `UnitInfo::StopMovement` (`:3222`) clears the path and never rewrites `x/y`, so
/// the original shows the corpse and its blast on the halt cell.
#[test]
fn a_unit_killed_on_the_halt_cell_is_drawn_there() {
    let Some(mut app) = game_app() else {
        return;
    };
    let Some((mover, destination)) = intercept_scenario(&mut app, 1) else {
        return;
    };

    let mut previous = app.visual_position(mover);
    let mut saw_death = false;
    let mut death_cell = None;
    let mut last_drawn = None;
    let mut frames = 0usize;
    // See the survivor test: the first drain frame re-synchronizes the sprite.
    let mut resynced = false;

    for _ in 0..400 {
        let closing = app.turn_close_pending();
        app.tick();
        app.update_visual_units(1.0 / 60.0);
        frames += 1;

        let now = app.visual_position(mover);
        let simulation = app.state_ref().unit(mover).map(|unit| {
            (
                f32::from(unit.x) / 64.0,
                f32::from(unit.y) / 64.0,
            )
        });

        if let (Some(previous), Some(now)) = (previous, now) {
            if closing && !resynced {
                resynced = true;
                assert_eq!(
                    Some(now),
                    simulation,
                    "frame {frames}: the re-synchronization must land on the simulation pixel"
                );
            } else {
                let step = (now.0 - previous.0).abs().max((now.1 - previous.1).abs());
                assert!(
                    step <= DRAIN_CELL_STEP,
                    "frame {frames}: the sprite of the unit killed on the halt cell jumps \
                     {step:.4} cells: {previous:?} -> {now:?}"
                );
            }
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
                // The pixel the killing shot landed on: the halt pixel.
                death_cell = Some((f32::from(unit.x) / 64.0, f32::from(unit.y) / 64.0));
                // The sprite is drawn on that pixel already.
                assert_eq!(
                    now, death_cell,
                    "frame {frames}: the killing shot lands on the pixel the sprite is drawn on"
                );
            }
        }
    }

    assert!(saw_death, "the intercept killed the mover on the halt cell");

    let death_cell = death_cell.expect("the death cell of the mover");
    let end = last_drawn.expect("the hull is drawn up to its death");
    assert!(
        (end.0 - death_cell.0).abs() < 1e-3 && (end.1 - death_cell.1).abs() < 1e-3,
        "the last drawn sprite is on the halt cell: {end:?} != {death_cell:?}"
    );
    assert!(
        app.visual_position(mover).is_none(),
        "the sprite is gone with the hull once the simulation removes it"
    );
}
