//! Air units: the `HOVERING` flight state and its presentation.
//!
//! Oracle `max-cpp-port`:
//!
//! * `UnitInfo::Take` (`unitinfo.cpp:3862-3889`) — five animation steps, every
//!   step shifts the shadow by `-16` (`:3879-3880`), the fifth sets `HOVERING`;
//! * `UnitInfo::Land` (`unitinfo.cpp:3829-3858`) — nine steps, every step shifts
//!   the shadow by `+8` (`:3848-3849`), the ninth clears `HOVERING` and calls
//!   `Redraw()`;
//! * `UnitInfo::Redraw` (`unitinfo.cpp:1452-1459`) — `(MOBILE_AIR_UNIT &&
//!   HOVERING) ? (-64, -64) : (0, 0)`: the shadow of a flying aircraft is one cell
//!   down-right of the hull (`RenderShadow`, `:3067`: `point -= shadow_offset`),
//!   which is what makes it look airborne;
//! * `AirPath::Execute` (`airpath.cpp:272-278`) — `Paths_TakeStep(unit, 1)` once
//!   per crossed cell, so an aircraft spends exactly its `speed` in cells;
//!   `airpath.cpp:186-199` stops the flight when the usable speed is gone.
//!
//! Defects of the manual 1.2 run: the port never maintained
//! `UnitPresentation::shadow_offset` (aircraft were drawn with the shadow under
//! the hull, "crawling on the ground") and never spent `speed` on the air path
//! (aircraft flew for free).

mod support;

use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use support::{game, order_move, spawn, start_turn, ticks};

/// Grid of the unit.
fn grid(state: &max_core::universe::GameUniverse, id: max_core::UnitId) -> Point {
    state.snapshot().unit(id).expect("unit").grid
}

/// `HOVERING` bit of the unit.
fn hovering(state: &max_core::universe::GameUniverse, id: max_core::UnitId) -> bool {
    state.snapshot().unit(id).expect("unit").flags & unit_flags::HOVERING != 0
}

/// Remaining speed of the unit.
fn speed(state: &max_core::universe::GameUniverse, id: max_core::UnitId) -> u16 {
    state.snapshot().unit(id).expect("unit").speed
}

/// `UnitPresentation::shadow_offset` of the unit.
fn shadow_offset(state: &max_core::universe::GameUniverse, id: max_core::UnitId) -> Point {
    state
        .snapshot()
        .presentation(id)
        .expect("presentation")
        .shadow_offset
}

/// `UnitInfo::Take` / `Land` / `Redraw`: the shadow slides `-16` per takeoff step
/// and `+8` per landing step, and the steady states are `(-64, -64)` (hovering)
/// and `(0, 0)` (on the ground).
#[test]
fn takeoff_and_landing_slide_the_air_shadow() {
    let mut state = game();
    let fighter = spawn(&mut state, UnitType::Fighter, TeamId::RED, Point::new(10, 10));

    // A unit that is born on the ground has no shadow shift (`Redraw`).
    assert_eq!(shadow_offset(&state, fighter), Point::ZERO, "shadow under the hull");

    state.set_order(fighter, UnitOrder::TakeOff, UnitOrderState::Init);

    // Four animation steps: `shadow_offset -= 16` (`unitinfo.cpp:3879-3880`).
    for step in 1..=4 {
        state.tick();
        assert!(!hovering(&state, fighter), "step {step}: not airborne yet");
        assert_eq!(
            shadow_offset(&state, fighter),
            Point::new(-16 * step, -16 * step),
            "step {step}: shadow shift"
        );
    }

    // The fifth step sets `HOVERING` and does not touch the offset (`Take`'s
    // final branch) — the accumulated `-64` is exactly the `Redraw()` value.
    state.tick();
    assert!(hovering(&state, fighter), "fifth step sets HOVERING");
    assert_eq!(
        shadow_offset(&state, fighter),
        Point::new(-64, -64),
        "airborne: the shadow is one cell down-right of the hull"
    );

    state.set_order(fighter, UnitOrder::Land, UnitOrderState::Init);

    // Eight animation steps: `shadow_offset += 8` (`unitinfo.cpp:3848-3849`).
    for step in 1..=8 {
        state.tick();
        assert!(hovering(&state, fighter), "step {step}: still airborne");
        assert_eq!(
            shadow_offset(&state, fighter),
            Point::new(-64 + 8 * step, -64 + 8 * step),
            "step {step}: shadow shift"
        );
    }

    // The ninth step clears `HOVERING` and calls `Redraw()`.
    state.tick();
    assert!(!hovering(&state, fighter), "ninth step clears HOVERING");
    assert_eq!(shadow_offset(&state, fighter), Point::ZERO, "shadow back under the hull");
}

/// The shadow shift is presentation only: it never enters the snapshot/CRC
/// (`state-model.md` §6.4), so a change in it cannot desync a match.
#[test]
fn the_shadow_shift_is_not_part_of_the_state_digest() {
    let mut state = game();
    let fighter = spawn(&mut state, UnitType::Fighter, TeamId::RED, Point::new(10, 10));

    let before = state.snapshot().snapshot_bytes();

    let mut presentation = state
        .snapshot()
        .presentation(fighter)
        .expect("presentation")
        .clone();
    presentation.shadow_offset = Point::new(-64, -64);
    state.write_presentation(fighter, presentation);

    assert_eq!(
        state.snapshot().snapshot_bytes(),
        before,
        "presentation fields do not enter the canonical digest"
    );
    assert_eq!(shadow_offset(&state, fighter), Point::new(-64, -64));
}

/// Pixel position of the unit (`UnitInfo::x/y`).
fn pixel(state: &max_core::universe::GameUniverse, id: max_core::UnitId) -> (i32, i32) {
    let unit = state.snapshot().unit(id).expect("unit").clone();

    (i32::from(unit.x), i32::from(unit.y))
}

/// Perpendicular distance from `point` to the line `start`вЂ“`end` in pixels.
fn distance_to_line(point: (i32, i32), start: (i32, i32), end: (i32, i32)) -> f32 {
    let dx = (end.0 - start.0) as f32;
    let dy = (end.1 - start.1) as f32;
    let px = (point.0 - start.0) as f32;
    let py = (point.1 - start.1) as f32;

    if dx == 0.0 && dy == 0.0 {
        return (px * px + py * py).sqrt();
    }

    (px * dy - py * dx).abs() / (dx * dx + dy * dy).sqrt()
}

/// `AirPath::Execute` (`airpath.cpp:219-242`): an aircraft flies the **straight
/// pixel segment** between the start and the target cell centers —
/// `m_step_x += m_delta_x`, `position = (m_step >> 16) + m_start`, `unit->x/y` and
/// `grid_x = x >> 6`. It is not the 8-direction cell stair of the ground path
/// (`UnitInfo::Move`): a non-45° route (`dx = 10`, `dy = 4`) would bend there.
///
/// The previous port moved the aircraft cell by cell toward `path.end_x/end_y`
/// and never touched `x/y`, so every sample stayed at the starting cell center.
#[test]
fn an_aircraft_flies_the_straight_pixel_segment() {
    let mut state = game();
    let fighter = spawn(&mut state, UnitType::Fighter, TeamId::RED, Point::new(10, 10));

    if let Some(unit) = state.unit_mut(fighter) {
        unit.flags |= unit_flags::HOVERING;
    }

    let target = Point::new(20, 14);
    order_move(&mut state, fighter, target);

    let start = (10 * 64 + 32, 10 * 64 + 32);
    let end = (20 * 64 + 32, 14 * 64 + 32);
    let mut samples = Vec::new();

    for _ in 0..120 {
        state.tick();

        let unit = state.snapshot().unit(fighter).expect("fighter").clone();
        samples.push(pixel(&state, fighter));

        // The path is over when `Paths_FinishMove` cleared it
        // (`m_length == 0`, `airpath.cpp:281-283`).
        if unit.path.is_none() {
            break;
        }
    }

    assert!(samples.len() > 4, "the aircraft flies for several ticks");

    for sample in &samples {
        let distance = distance_to_line(*sample, start, end);

        assert!(
            distance <= 1.5,
            "sample {sample:?} is {distance:.2} px off the air path line"
        );
    }

    // The pixel position really moves between ticks (the old port left it at the
    // spawn cell) and passes through points that are not cell centers.
    assert!(
        samples.iter().any(|sample| *sample != start),
        "`AirPath::Execute` moves `unit->x/y`, not just the grid"
    );
    assert!(
        samples
            .iter()
            .any(|sample| (sample.0 - 32) % 64 != 0 || (sample.1 - 32) % 64 != 0),
        "the aircraft is drawn on the straight segment, not on cell centers"
    );

    // The pixel walk lands on the segment end up to the integer truncation of the
    // last halved step (`m_length == 1`, `airpath.cpp:272-278`), and the grid is
    // recomputed from the pixel (`grid_x = x >> 6`).
    let last = *samples.last().expect("samples");
    assert!(
        (last.0 - end.0).abs() <= 2 && (last.1 - end.1).abs() <= 2,
        "the aircraft arrives at the target cell center: {last:?} vs {end:?}"
    );
    assert_eq!(grid(&state, fighter), target, "the grid follows the pixel");
}

/// `UnitsManager_StartExplosion` (`units_manager.cpp:3991-3994`): an aircraft shot
/// down **between two cells** explodes on its own pixel position
/// (`explosion->OffsetDrawZones(unit->x - explosion->x, unit->y - explosion->y)`),
/// not on the center of the cell it happened to be over.
#[test]
fn an_aircraft_killed_mid_flight_explodes_on_its_pixel() {
    let mut state = game();
    let fighter = spawn(&mut state, UnitType::Fighter, TeamId::RED, Point::new(10, 10));

    if let Some(unit) = state.unit_mut(fighter) {
        unit.flags |= unit_flags::HOVERING;
    }

    order_move(&mut state, fighter, Point::new(20, 14));

    // A few pixels into the segment: `x/y` is no longer a cell center.
    let mut pixel = (0, 0);
    let mut cell = Point::ZERO;

    for _ in 0..6 {
        state.tick();
        let unit = state.snapshot().unit(fighter).expect("fighter").clone();
        pixel = (i32::from(unit.x), i32::from(unit.y));
        cell = unit.grid;

        if pixel != (10 * 64 + 32, 10 * 64 + 32) {
            break;
        }
    }

    assert!(
        (pixel.0 - 32) % 64 != 0 || (pixel.1 - 32) % 64 != 0,
        "the aircraft is mid-cell: {pixel:?}"
    );

    // Kill it right there (`ORDER_EXPLODE`/`INIT` → `UnitsManager_StartExplosion`).
    {
        let unit = state.unit_mut(fighter).expect("fighter");
        unit.hits = 0;
        unit.orders = UnitOrder::Explode;
        unit.state = UnitOrderState::Init;
        unit.angle = 0;
    }

    state.tick();

    let blast = state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.snapshot().unit(id).cloned())
        .find(|unit| unit.unit_type == UnitType::AirExplosion)
        .expect("an aircraft explodes with `AIREXPLD`");

    assert_eq!(
        (i32::from(blast.x), i32::from(blast.y)),
        pixel,
        "`OffsetDrawZones(unit->x - explosion->x, ...)`: the blast is drawn on the \
         aircraft pixel, not on the cell center"
    );
    assert_eq!(blast.grid, cell, "the effect is spawned on the aircraft cell");
}

/// `AirPath::Execute` + `Paths_GetAirPath`: an aircraft flies exactly `speed`
/// cells and spends one `speed` point per cell (`airpath.cpp:272-278`); the next
/// order in the same turn does not move it.
#[test]
fn an_aircraft_spends_one_speed_point_per_cell() {
    let mut state = game();
    let fighter = spawn(&mut state, UnitType::Fighter, TeamId::RED, Point::new(10, 10));
    // `spawn` does not take off; the flight tests start in the air.
    if let Some(unit) = state.unit_mut(fighter) {
        unit.flags |= unit_flags::HOVERING;
    }

    // FIGHTER `SC_ATTRI` speed = 24.
    let allowance = speed(&state, fighter);
    assert_eq!(allowance, 24, "FIGHTER speed");

    order_move(&mut state, fighter, Point::new(40, 10));

    let start = grid(&state, fighter);
    let mut flown = 0;
    for _ in 0..80 {
        let before = grid(&state, fighter);
        state.tick();
        if grid(&state, fighter) != before {
            flown += 1;
        }
        if speed(&state, fighter) == 0 {
            break;
        }
    }

    assert_eq!(
        flown, allowance,
        "the aircraft must cross exactly its speed in cells"
    );
    assert_eq!(
        grid(&state, fighter),
        Point::new(start.x + allowance as i16, start.y),
        "the air path is clamped to the remaining speed (`Paths_GetAirPath`)"
    );
    assert_eq!(speed(&state, fighter), 0, "the whole allowance is spent");

    // A fresh order in the same turn: `AirPath::Execute` stops on `speed == 0`
    // (`airpath.cpp:193-198`) — the aircraft does not fly for free.
    let resting = grid(&state, fighter);
    order_move(&mut state, fighter, Point::new(resting.x, resting.y + 10));
    ticks(&mut state, 10);
    assert_eq!(
        grid(&state, fighter),
        resting,
        "an aircraft without speed does not move"
    );

    // `UnitsManager_ManageEconomy` restores the speed at the start of the next turn.
    start_turn(&mut state, TeamId::RED);
    assert_eq!(speed(&state, fighter), allowance, "speed restored");

    order_move(&mut state, fighter, Point::new(resting.x, resting.y + 10));
    ticks(&mut state, 40);
    assert_ne!(
        grid(&state, fighter),
        resting,
        "the aircraft flies again with a fresh turn's speed"
    );
}

/// `GameManager_RealTime` (a debug setting, `preferences.rs`): the step costs
/// nothing, so the aircraft keeps its speed.
#[test]
fn real_time_does_not_spend_air_speed() {
    let mut state = game();
    state.set_real_time(true);

    let fighter = spawn(&mut state, UnitType::Fighter, TeamId::RED, Point::new(10, 10));
    if let Some(unit) = state.unit_mut(fighter) {
        unit.flags |= unit_flags::HOVERING;
    }

    let allowance = speed(&state, fighter);
    order_move(&mut state, fighter, Point::new(20, 10));
    ticks(&mut state, 30);

    assert_eq!(
        grid(&state, fighter),
        Point::new(20, 10),
        "in real time the whole segment is flown in one go"
    );
    assert_eq!(
        speed(&state, fighter),
        allowance,
        "`Paths_TakeStep` with `GameManager_RealTime` costs nothing"
    );
}

/// `GameManager_MaxSpy`/aircraft takeoff is not needed for the `HOVERING`
/// presentation: a unit spawned already flying starts with the air shadow
/// (`UnitInfo::Redraw` on the first draw).
#[test]
fn a_unit_spawned_in_the_air_gets_the_air_shadow() {
    let mut state = game();
    let fighter = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::HOVERING,
        )
        .expect("hovering fighter");

    assert!(hovering(&state, fighter));
    assert_eq!(shadow_offset(&state, fighter), Point::new(-64, -64));

    let ground = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(12, 12));
    assert_eq!(shadow_offset(&state, ground), Point::ZERO, "ground unit");
}
