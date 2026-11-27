//! The `VISUAL_TELEPORT_CELLS` fallback: a position change the simulation did
//! **not** walk still snaps the sprite.
//!
//! `App::update_visual_units` treats a jump larger than
//! [`max_app::VISUAL_TELEPORT_CELLS`] as a state synchronization — a save/load or
//! a transport/network position write — and sets the sprite to the new target
//! without animation, because the route is **unknown**.
//!
//! The turn close does **not** reach this branch: it walks the closing routes inside
//! the simulation at the visual pace (`max_core::ClosingTurn`), so the frame sees at
//! most a one-cell step. This test guards the fallback for a position write the
//! simulation did not walk: the frame must not invent a route there.
//!
//! No game assets and no simulation tick are needed: the unit position is written
//! directly, the way a load or a transport synchronization does, and only the
//! frame clock runs.

use max_app::local::LocalSetup;
use max_app::{App, AppConfig};
use max_core::enums::unit_flags;
use max_core::geo::Point;
use max_core::unit::{UnitId, UnitInfo};

/// Campaign 1 mission — byte-identical to `max-original/SAVE1.CAM`.
const SAVE1_CAMPAIGN: &[u8] = include_bytes!("data/save1_v70.cam");

/// Frame step of the visual clock (`App::update_visual_units`).
const FRAME_SECONDS: f32 = 1.0 / 60.0;

/// The campaign mission started headless (no assets needed: the map comes from
/// the save). The application is boxed for the stack reason of
/// `air_intercept_visual_halt.rs`.
fn mission_app() -> Box<App> {
    let mut app = Box::new(App::new(AppConfig::default()));
    app.start_local_game_from_save(&LocalSetup::default(), SAVE1_CAMPAIGN)
        .expect("campaign 1 starts");
    app
}

/// Whether the frame draws the unit at its `x/y` pixel instead of its cell
/// (`visual_target`, `max_app/src/lib.rs`).
fn is_pixel_positioned(unit: &UnitInfo) -> bool {
    unit.is_air() && unit.flags & unit_flags::MISSILE_UNIT == 0
}

/// The player's first **mobile** unit matching `predicate`.
fn player_unit(app: &App, what: &str, predicate: impl Fn(&UnitInfo) -> bool) -> UnitId {
    let player = app.state_ref().player_team();

    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == player && unit.is_mobile() && predicate(unit))
        })
        .unwrap_or_else(|| panic!("the player has no {what} in campaign 1"))
}

/// A position write without a walked route — save/load, transport or state
/// synchronization — snaps in both coordinate spaces: the cell of a ground unit
/// and the pixel of an aircraft.
#[test]
fn a_position_change_without_a_walked_route_snaps() {
    let mut app = mission_app();
    // Twice the `VISUAL_TELEPORT_CELLS` threshold.
    const JUMP: i16 = 12;

    let ground = player_unit(&app, "ground vehicle", |unit| !is_pixel_positioned(unit));
    let air = player_unit(&app, "aircraft", is_pixel_positioned);

    for id in [ground, air] {
        app.update_visual_units(0.0);
        let unit_type = app.state_ref().unit(id).expect("unit").unit_type;
        let start = app.visual_position(id).expect("sprite");
        let from = app.state_ref().unit(id).expect("unit").grid;
        let to = Point::new(from.x + JUMP, from.y);

        // `GameState` writes the position directly, like `SaveLoad_LoadFormatV70`
        // or a transport/state-sync packet: no route is built, so the frame has
        // nothing to walk.
        {
            let unit = app.universe_mut().unit_mut(id).expect("unit");
            unit.grid = to;
            max_core::unit_order::update_grid_position(unit);
        }

        app.update_visual_units(FRAME_SECONDS);

        let unit = app.state_ref().unit(id).cloned().expect("unit");
        // The frame target the branch snaps to: the simulation pixel of every unit
        // (`x/y / 64`; a ground unit that was written directly stands on the cell
        // center, i.e. `grid + 0.5`).
        let target = {
            let tile = max_core::unit_order::GFX_MAP_TILE_SIZE as f32;

            (f32::from(unit.x) / tile, f32::from(unit.y) / tile)
        };
        let jump = (target.0 - start.0).abs().max((target.1 - start.1).abs());

        assert!(
            jump > max_app::VISUAL_TELEPORT_CELLS as f32,
            "{unit_type:?}: the write must jump farther than the threshold: {start:?} -> {target:?}"
        );
        assert_eq!(
            app.visual_position(id),
            Some(target),
            "{unit_type:?}: a position write without a walked route must snap the sprite"
        );
    }
}
