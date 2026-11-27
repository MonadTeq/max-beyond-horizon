//! Playtest defect: the explosion of a unit stopped by an intercept is shown on
//! the **halt cell**, not on the destination of the interrupted route.
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
//!
//! # Defect
//!
//! The simulation already spawns the blast on the halt cell, but the **frame** kept
//! walking the dying unit's queued visual route (the turn-close drain executes the
//! whole route in one simulation step, the sprite follows at
//! `VISUAL_CELLS_PER_SECOND`). For ~1 s the player saw the sprite several cells
//! behind its blast — "the unit stops and is fired at on the halt cell, but the
//! explosion is shown on the destination". A dead unit must be drawn on the cell
//! where it died.

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

#[test]
fn an_intercepted_unit_is_drawn_and_explodes_on_the_halt_cell() {
    let Some(mut app) = game_app() else {
        return;
    };

    // Only the scenario units on the map.
    for id in app.state_ref().unit_ids_in_order() {
        app.universe_mut().destroy_unit(id);
    }

    let Some(start) = free_row(&app, 14) else {
        eprintln!("intercept_explosion: no free land row");
        return;
    };

    app.universe_mut().set_enemy_halt(true);

    let mover = spawn(&mut app, UnitType::Tank, TeamId::RED, start);
    let enemy = spawn(
        &mut app,
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

    // A few frames at the start cell: the frame now has a visual position that lags
    // the route the turn-close drain executes in one step.
    for _ in 0..5 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);
    }

    app.universe_mut().end_local_turn(TeamId::RED);

    let mut checked = false;

    for _ in 0..120 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);

        let Some(unit) = app.state_ref().unit(mover).cloned() else {
            break;
        };
        let Some(explosion) = explosion_of(&app) else {
            continue;
        };
        let blast = app
            .state_ref()
            .unit(explosion)
            .cloned()
            .expect("explosion unit");

        // The intercept stopped the mover before its destination.
        assert_ne!(unit.grid, destination, "the route was interrupted");
        assert_eq!(
            blast.grid,
            unit.grid,
            "the explosion is spawned on the cell where the unit died"
        );

        // Frame position: the corpse (death animation) and the blast are drawn on
        // the death cell together — the sprite must not keep walking the route.
        let corpse_position = app.visual_position(mover).expect("corpse position");
        let blast_position = app.visual_position(explosion).expect("blast position");
        let death_cell = (f32::from(unit.grid.x), f32::from(unit.grid.y));

        assert_eq!(
            corpse_position, death_cell,
            "the dead unit is drawn on the death cell, not on the route"
        );
        assert_eq!(
            blast_position, death_cell,
            "the explosion is drawn on the death cell"
        );

        checked = true;
        break;
    }

    assert!(checked, "the intercept killed the mover and spawned a blast");
}
