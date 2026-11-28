//! Frame-level regression: a killed unit's hull sprite disappears at once and
//! only the death animation is drawn at the death pixel.
//!
//! # Oracle
//!
//! `UnitsManager_StartExplosion` (`max-cpp-port/src/units_manager.cpp:3915-4004`):
//! a unit with `hits == 0` gets its death effect deployed on its **pixel**
//! (`OffsetDrawZones(unit->x - explosion->x, unit->y - explosion->y)`, `:3991`)
//! and enters `ORDER_STATE_DESTROY`. `UnitsManager_ProgressExplosion`
//! (`:4006-4033`) removes the corpse from the unit lists only half-way through
//! the blast (`UnitsManager_BuildClearing`, `:4015-4023`), so the dying unit's
//! own sprite is not part of the frame at all — the death animation replaces it.
//!
//! # Defect (fixed)
//!
//! The port kept drawing the dead unit from the unit lists until the blast
//! reached its middle frame: the hull sprite showed through the transparent
//! corners of the blast for the first half of the animation. `draw_units` now
//! skips a `hits == 0` / `ORDER_STATE_DESTROY` hull (`render::hull_sprite_drawn`),
//! so only the death effect (`EXPLODING`, drawn at the death pixel) remains.
//! The exceptions are exactly the oracle's: explosion/trail particles
//! (`hits == 0` for their whole life by `SC_ATTRI`) and the `INFANTRY`/`COMMANDO`
//! own death animation (`units_manager.cpp:3932-3946`).
//!
//! The test kills a tank through the simulation order pipeline, renders the
//! frame while the blast is playing and compares it with a frame of the same
//! state without the dead unit: the hull must not contribute a single pixel.

use std::path::{Path, PathBuf};

use max_app::{App, AppConfig};
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

/// Path inside the repository (`max-brh/crates/max_app/../../..`).
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// The game data is required by the frame test: skip it when the data is not
/// next to the repository.
fn game_data_present() -> bool {
    repo_path("max-original/MAX.RES").is_file()
        && repo_path("max-original/COLOR.PAL").is_file()
        && repo_path("tests/fixtures/assets/PATCHES.RES").is_file()
}

/// Cell of the dying tank and the camera center.
const CELL: Point = Point::new(20, 20);

/// An empty match with the assets of the game and a 448×448 full-screen frame.
fn clean_app() -> Box<App> {
    let mut universe = GameUniverse::with_setup(
        GameUniverse::default().snapshot().map().clone(),
        [
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );
    universe.set_player_team(TeamId::RED);

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        .with_screen(448, 448);
    let mut app = Box::new(App::new(config));
    app.init_assets().expect("assets of the game");
    app.adopt_universe(universe);
    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(CELL.x, CELL.y);

    app
}

/// Spawns a RED tank with the `SC_UNITS` flags of its type.
fn spawn_tank(app: &mut App) -> UnitId {
    app.universe_mut()
        .spawn_unit(UnitType::Tank, TeamId::RED, CELL, 0)
        .expect("tank spawned")
}

/// Drives the tank into the death sequence the way `UnitInfo::AttackUnit`
/// (`unitinfo.cpp:1580-1616`) leaves it: `hits == 0`, `ORDER_EXPLODE`/`INIT`.
fn kill_tank(app: &mut App, tank: UnitId) {
    let mut unit = app.state_ref().unit(tank).cloned().expect("tank");
    unit.hits = 0;
    unit.prior_orders = unit.orders;
    unit.prior_state = unit.state;
    unit.orders = UnitOrder::Explode;
    unit.state = UnitOrderState::Init;
    app.universe_mut().write_unit(tank, unit);
}

/// The explosion effect of the death (`LNDEXPLD`).
fn explosion_of(app: &App) -> Option<UnitId> {
    app.state_ref().unit_ids_in_order().into_iter().find(|id| {
        app.state_ref().unit(*id).is_some_and(|unit| {
            unit.flags & unit_flags::EXPLODING != 0
                && matches!(
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

/// Counts the pixels two frames differ in.
fn changed_pixels(left: &max_render::buffer::RgbaBuffer, right: &max_render::buffer::RgbaBuffer) -> usize {
    left.pixels()
        .chunks_exact(4)
        .zip(right.pixels().chunks_exact(4))
        .filter(|(left, right)| left != right)
        .count()
}

#[test]
fn the_hull_of_a_dying_unit_is_not_drawn_during_its_death_animation() {
    if !game_data_present() {
        eprintln!("death_sprite: нет данных — нет данных игры рядом с репозиторием");
        return;
    }

    let mut app = clean_app();
    let map_only = app.render_frame();

    let tank = spawn_tank(&mut app);
    app.update_visual_units(0.0);
    let alive = app.render_frame();
    assert!(
        changed_pixels(&alive, &map_only) > 0,
        "the alive tank sprite must be visible in the frame"
    );

    // Death through the simulation: `ORDER_EXPLODE`/`INIT` → `StartExplosion`
    // spawns the blast and puts the unit into `ORDER_STATE_DESTROY`.
    kill_tank(&mut app, tank);
    let mut explosion = None;
    for _ in 0..8 {
        app.tick();
        explosion = explosion_of(&app);
        if explosion.is_some() {
            break;
        }
    }
    let explosion = explosion.expect("the death effect must be spawned");

    let dead = app.state_ref().unit(tank).cloned().expect("the corpse stays");
    assert_eq!(dead.hits, 0);
    assert_eq!(
        dead.state,
        UnitOrderState::Destroy,
        "the dying unit is in `ORDER_STATE_DESTROY`"
    );
    assert!(
        !max_app::render::hull_sprite_drawn(&dead),
        "the hull of the dying unit must not be drawn"
    );

    app.update_visual_units(0.0);
    let (dead_frame, stats) = app.render_frame_with_stats();
    assert_eq!(
        stats.units_drawn, 1,
        "only the death animation is drawn (units_drawn={}, units_skipped={})",
        stats.units_drawn, stats.units_skipped
    );
    assert_eq!(stats.units_skipped, 1, "the hull of the dead tank is skipped");

    // The blast sits on the death pixel and is visible.
    let death_pixel = (i32::from(dead.x), i32::from(dead.y));
    let blast = app.state_ref().unit(explosion).cloned().expect("blast");
    assert_eq!(
        (i32::from(blast.x), i32::from(blast.y)),
        death_pixel,
        "`UnitsManager_StartExplosion`: the effect is aligned with the dying unit's pixel"
    );
    assert!(
        changed_pixels(&dead_frame, &map_only) > 0,
        "the death animation is drawn in the frame"
    );

    // The proof of the fix: the dead unit's own sprite contributes nothing to the
    // frame while the blast plays. Removing the corpse from the state leaves the
    // image byte-identical; before the fix the hull showed through the blast.
    app.universe_mut().destroy_unit(tank);
    let after_removal = app.render_frame();
    assert_eq!(
        changed_pixels(&dead_frame, &after_removal),
        0,
        "the hull of a dying unit must not contribute a single pixel while its \
         death animation is playing"
    );
}
