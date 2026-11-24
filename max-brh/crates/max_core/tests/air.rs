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
