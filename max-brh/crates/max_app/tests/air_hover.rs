//! Flight animation of hovering aircraft — the port of `UnitInfo::ShakeAir`
//! (`max-cpp-port/src/unitinfo.cpp:4636-4678`).
//!
//! The oracle calls `ShakeAir` from `UnitInfo::Animate` once per order tick for
//! units with `HOVERING`: every second tick the sprite moves one pixel along
//! `DIRECTION_OFFSETS[moved / 8]` (two pixels at the peak of an 8-tick block) and
//! the shadow moves three times as far (`shadow_offset -= 2 * offset` on top of
//! the hull's `OffsetDrawZones(offset)`). The port advances the same 64-step
//! counter from real frame time — frame only, not part of the snapshot/CRC, and
//! `dt = 0` (QA/headless) leaves the sprite at rest.

use max_app::{App, AppConfig};
use max_core::enums::{unit_flags, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

/// RED player with an airborne fighter and a grounded tank.
fn scenario() -> (App, UnitId, UnitId) {
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

    let fighter = universe
        .spawn_unit(UnitType::Fighter, TeamId::RED, Point::new(20, 20), 0)
        .expect("fighter");
    let tank = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(24, 20), 0)
        .expect("tank");

    // Airborne: `UnitInfo::Take` sets `HOVERING`, `Redraw` the air shadow.
    let mut unit = universe.snapshot().unit(fighter).cloned().expect("fighter");
    unit.flags |= unit_flags::HOVERING;
    let flags = unit.flags;
    universe.write_unit(fighter, unit);

    let mut presentation = universe
        .snapshot()
        .presentation(fighter)
        .cloned()
        .unwrap_or_default();
    presentation.shadow_offset = max_core::unit_order::air_shadow_offset(flags);
    universe.write_presentation(fighter, presentation);

    let mut app = App::new(AppConfig::default());
    app.adopt_universe(universe);

    (app, fighter, tank)
}

/// Advances the visual clock `ticks` order ticks (`UnitInfo::Animate` rate).
fn advance(app: &mut App, ticks: u32) {
    for _ in 0..ticks {
        app.update_visual_units(max_app::ORDER_TICK_SECONDS);
    }
}

/// The 8-tick block: two even ticks out, two back; then the direction rotates.
#[test]
fn a_hovering_aircraft_bobs_in_the_air() {
    let (mut app, fighter, _) = scenario();

    app.update_visual_units(0.0);
    assert_eq!(
        app.hover_offset(fighter),
        (0, 0),
        "dt = 0 (QA/headless): статичный кадр"
    );

    // `moved = 1`; the first offset of the block is one pixel north.
    advance(&mut app, 1);
    assert_eq!(app.hover_offset(fighter), (0, -1));

    // `moved = 2` — the second even tick, the peak of the block.
    advance(&mut app, 1);
    assert_eq!(app.hover_offset(fighter), (0, -2), "пик колебания: 2 px");

    // `moved = 4` — the first tick of the return half.
    advance(&mut app, 2);
    assert_eq!(app.hover_offset(fighter), (0, -1));

    // `moved = 6` — back at the anchor; the block nets to zero.
    advance(&mut app, 2);
    assert_eq!(app.hover_offset(fighter), (0, 0));

    // `moved = 8` — the next direction (north-east) starts.
    advance(&mut app, 2);
    assert_eq!(app.hover_offset(fighter), (1, -1));
}

/// A grounded unit has no flight bob (the clock exists only for `HOVERING`).
#[test]
fn a_grounded_unit_does_not_bob() {
    let (mut app, fighter, tank) = scenario();

    advance(&mut app, 8);

    assert_ne!(app.hover_offset(fighter), (0, 0), "самолёт покачивается");
    assert_eq!(app.hover_offset(tank), (0, 0), "танк стоит на земле");
}

/// The bob is frame only: a long animation does not change the snapshot/CRC.
#[test]
fn the_flight_bob_does_not_change_the_snapshot() {
    let (mut app, fighter, _) = scenario();

    let before = app.snapshot();

    for _ in 0..64 {
        app.update_visual_units(max_app::ORDER_TICK_SECONDS);
    }

    assert_eq!(
        app.snapshot(),
        before,
        "анимация полёта — только презентация"
    );
    assert_ne!(app.hover_offset(fighter), (0, 0));
}

/// `settings["effects"] = false` freezes the animation (`UnitInfo::Animate`
/// checks the setting, `unitinfo.cpp:5057`).
#[test]
fn effects_off_stops_the_flight_bob() {
    let (mut app, fighter, _) = scenario();

    let mut preferences = app.preferences().clone();
    preferences.effects = false;
    app.set_preferences(preferences);

    advance(&mut app, 8);

    assert_eq!(
        app.hover_offset(fighter),
        (0, 0),
        "effects = false: спрайт не покачивается"
    );
}
