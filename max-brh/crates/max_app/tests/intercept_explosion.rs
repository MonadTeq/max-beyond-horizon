//! Playtest defect: the explosion of a unit stopped by an intercept is shown on the
//! **halt cell**, not on the destination of the interrupted route — and the corpse
//! reaches that cell by walking, without a jump.
//!
//! # Oracle
//!
//! * `UnitsManager_StartExplosion` (`max-cpp-port/src/units_manager.cpp:3915-3994`)
//!   deploys the effect at `unit->grid_x, unit->grid_y` — the cell the dead unit
//!   occupies — and then aligns it with the unit's pixel position
//!   (`explosion->OffsetDrawZones(unit->x - explosion->x, unit->y - explosion->y)`,
//!   `:3991-3994`). The order data (`move_to`) is the destination of the interrupted
//!   route and is never a position source.
//! * `UnitEventEmergencyStop::Process` (`unitevents.cpp:38`) → `BlockedOnPathRequest`
//!   (`unitinfo.cpp:3628`) stops the mover on the cell where it entered the enemy's
//!   weapon range; the reaction fire and the blast happen there.
//! * The sprite is drawn at `UnitInfo::x/y`. `UnitInfo::Move` (`unitinfo.cpp:1730`)
//!   advances that pixel; an emergency stop (`UnitInfo::StopMovement`, `:3222`)
//!   only clears the path — **it never rewrites `x/y`**. The unit therefore ends on
//!   the halt cell and the blast is aligned with that same pixel.
//!
//! # Defects
//!
//! 1. The simulation spawns the blast on the halt cell, but the **frame** kept the
//!    blast on the *destination* of the interrupted route — "the unit stops and is
//!    fired at on the halt cell, but the explosion is shown on the destination"
//!    (fixed: `App::explosion_draw_offset` is `frame pixel - simulation pixel`, so
//!    the blast is drawn where the sprite is).
//! 2. The first fix pulled the corpse to the simulation cell on the death frame
//!    (`App::update_visual_units`: `*current = target`) — the sprite teleported.
//! 3. The next fix froze the corpse on the pixel the frame had reached. That is not
//!    a jump, but the turn-close drain (`finish_end_turn_movement`) runs the whole
//!    route in one `end_local_turn`, so the sprite was still several cells behind:
//!    the unit died **in mid-air**, away from the cell where the intercept stopped
//!    it, and its blast froze there too (the owner's death-scenario defect).
//!
//! # Fix
//!
//! A dead unit keeps its queued route: the corpse **walks** to the pixel where the
//! simulation killed it — the halt cell, i.e. the point of the first attack — at
//! `VISUAL_CELLS_PER_SECOND`, and the frame keeps it drawable after
//! `unit_order::progress_explosion` deletes it half-way through the blast
//! ([`max_app::CorpseVisual`]). The blast stays on the corpse
//! (`App::explosion_draw_offset`) and both end on the death pixel together.
//!
//! The simulation (the blast cell, the damage, the CRC) is untouched.

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
        eprintln!("intercept_explosion: no game data next to the repository");
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

/// The explosion effect of a frame (`LNDEXPLD`/`HITEXPLD`/`AIREXPLD`/…).
fn explosion_of(app: &App) -> Option<UnitId> {
    app.state_ref().unit_ids_in_order().into_iter().find(|id| {
        app.state_ref().unit(*id).is_some_and(|unit| {
            matches!(
                unit.unit_type,
                UnitType::HitExplosion
                    | UnitType::LandExplosion
                    | UnitType::BuildingExplosion
                    | UnitType::AirExplosion
                    | UnitType::SeaExplosion
            )
        })
    })
}

/// `UnitInfo::SetPosition` pixel of a frame position — the sprite anchor of a
/// ground unit (`unitinfo.cpp:1301`).
fn cell_pixel(position: (f32, f32)) -> (i32, i32) {
    (
        (position.0 * 64.0 + 32.0).round() as i32,
        (position.1 * 64.0 + 32.0).round() as i32,
    )
}

/// The intercept scenario: a RED tank walks 8 cells along a free row toward a GREEN
/// tank standing 10 cells away, so the emergency stop and the reaction fire catch it
/// before the destination. Returns the mover and the destination.
///
/// The caller prepares the match: the application is large, and a second copy on the
/// test stack overflows it (the helper only borrows it).
fn intercept_scenario(app: &mut App) -> Option<(UnitId, Point)> {
    // Only the scenario units on the map.
    for id in app.state_ref().unit_ids_in_order() {
        app.universe_mut().destroy_unit(id);
    }

    let Some(start) = free_row(app, 14) else {
        eprintln!("intercept_explosion: no free land row");
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
        unit.hits = 1;
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

    // A few frames at the start cell: the frame now has a visual position that lags
    // the route the turn-close drain executes in one step.
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
/// (`App::update_visual_units`; C++ `UnitInfo::Move` `velocity += 2`).
fn max_frame_step(app: &App) -> f32 {
    let scale = if app.preferences().fast_movement {
        2.0
    } else {
        1.0
    };

    max_app::VISUAL_CELLS_PER_SECOND * scale * (1.0 / 60.0) + 1e-3
}

#[test]
fn an_intercepted_unit_is_drawn_and_explodes_where_it_was_killed() {
    let Some(mut app) = game_app() else {
        return;
    };
    let Some((mover, destination)) = intercept_scenario(&mut app) else {
        return;
    };

    // The sprite may move at most the visual speed of one frame; a bigger step is a
    // teleport (the defect under test).
    let max_frame_step = max_frame_step(&app);
    let mut previous = app.visual_position(mover);
    let mut death_cell: Option<Point> = None;
    let mut death_pixel: Option<(i32, i32)> = None;
    let mut last_drawn: Option<(f32, f32)> = None;
    let mut frames_after_death = 0usize;
    let mut corpse_released = false;
    let mut blast_moved_with_the_corpse = 0usize;

    for _ in 0..400 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);

        let now = app.visual_position(mover);

        if let (Some(previous), Some(now)) = (previous, now) {
            let step = (now.0 - previous.0).abs().max((now.1 - previous.1).abs());
            assert!(
                step <= max_frame_step,
                "the sprite of the intercepted unit teleports: {previous:?} -> {now:?}"
            );
        }
        if let Some(now) = now {
            last_drawn = Some(now);
        }
        previous = now;

        let corpse = app.state_ref().unit(mover).cloned();
        let Some(explosion) = explosion_of(&app) else {
            if corpse.is_none() && now.is_none() {
                break;
            }
            continue;
        };

        let offset = app
            .explosion_draw_offset(explosion)
            .expect("the blast keeps the dying unit's frame offset");
        let blast = app
            .state_ref()
            .unit(explosion)
            .cloned()
            .expect("explosion unit");

        let Some(unit) = corpse else {
            // The explosion released the dead unit in the middle of the animation
            // (`unit_order::progress_explosion`), and the frame keeps walking the
            // corpse to the death pixel ([`max_app::CorpseVisual`]). The blast stays
            // on that corpse: its drawn pixel never leaves the sprite.
            corpse_released = true;
            frames_after_death += 1;

            if let Some(corpse_position) = now {
                assert_eq!(
                    (i32::from(blast.x) + offset.0, i32::from(blast.y) + offset.1),
                    cell_pixel(corpse_position),
                    "the blast stays on the corpse after the unit is released"
                );
                blast_moved_with_the_corpse += 1;
            }
            continue;
        };

        // The intercept stopped the mover before its destination; the simulation
        // spawns the blast on the cell where the unit really died — the destination
        // of the interrupted route is not a position source.
        assert_ne!(unit.grid, destination, "the route was interrupted");
        assert_eq!(
            blast.grid, unit.grid,
            "the explosion is spawned on the cell where the unit died"
        );

        // The death frame: the simulation pixel of the corpse is the point of the
        // first attack, and it never moves afterwards (the stop froze the route).
        let pixel = (i32::from(unit.x), i32::from(unit.y));
        let cell = *death_cell.get_or_insert(unit.grid);
        let point = *death_pixel.get_or_insert(pixel);
        assert_eq!(unit.grid, cell, "the corpse does not move after the death");
        assert_eq!(pixel, point, "the corpse keeps the death pixel");

        // `UnitsManager_StartExplosion` (`units_manager.cpp:3991-3994`): the blast is
        // drawn at the dying unit's pixel — the corpse and its explosion are one
        // point. The frame walks that corpse to the very same pixel, so while the
        // animation plays both are on the same sprite.
        if unit.hits == 0 {
            let corpse_position = now.expect("the corpse is drawn while it walks");
            assert_eq!(
                (i32::from(blast.x) + offset.0, i32::from(blast.y) + offset.1),
                cell_pixel(corpse_position),
                "the explosion is drawn on the corpse"
            );
            blast_moved_with_the_corpse += 1;
        }

        frames_after_death += 1;
    }

    let death_cell = death_cell.expect("the intercept killed the mover");
    let death_pixel = death_pixel.expect("the intercept killed the mover");
    assert!(
        frames_after_death > 0,
        "the explosion was observed while its animation played"
    );
    assert!(
        corpse_released,
        "the explosion must release the dead unit before it ends"
    );

    // The frame finished the walk: the last drawn sprite is exactly on the pixel the
    // simulation killed the unit on — the halt cell of the intercept, not a cell in
    // mid-air (the owner's death-scenario defect).
    let last = last_drawn.expect("the corpse is drawn while it walks");
    assert!(
        (last.0 - death_cell.x as f32).abs() < 1e-3 && (last.1 - death_cell.y as f32).abs() < 1e-3,
        "the corpse must end on the halt cell {death_cell:?}: {last:?}"
    );
    let _ = death_pixel;
    assert!(
        app.visual_position(mover).is_none(),
        "the corpse is gone once the walk is over"
    );
    assert!(
        blast_moved_with_the_corpse > 0,
        "the blast was drawn on the walking corpse"
    );
}
