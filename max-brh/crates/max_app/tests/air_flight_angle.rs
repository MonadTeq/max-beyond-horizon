//! Frame-level regression test for the aircraft flight angle.
//!
//! # Oracle
//!
//! * `AirPath::AirPath` (`max-cpp-port/src/airpath.cpp:46-62`): an air unit gets
//!   `m_angle = UnitsManager_GetTargetAngle(distance_x, distance_y)`
//!   (`units_manager.cpp:1711`) — the **octant** direction of the whole straight
//!   segment (0..7). `AirPath::Execute` turns the aircraft to `m_angle` once
//!   (`Paths_UpdateAngle`, `paths.cpp:258`) and then flies the pixel line
//!   (`airpath.cpp:219-242`); the angle does not change for the rest of the
//!   flight.
//! * `UnitInfo::Render` (`unitinfo.cpp:3138`) draws `image_index =
//!   image_base + angle` (`unitinfo.cpp:285`), so the sprite faces `m_angle`.
//!
//! # Defect (fixed)
//!
//! The frame waypoints took `paths_get_angle` of the sign of each sub-cell
//! displacement (`lib.rs`, `update_visual_units`). That 8-direction angle is
//! sign-based: a segment with a slope between 1:2 and 1:4 (for example 4 east,
//! 1 south) is "diagonal" (3, SE) for it, while the oracle faces the aircraft
//! **east** (2, `level_x / 2 > level_y`) — the plane flew 45° sideways. The
//! halved last pixel step (`airpath.cpp:203-213`) could additionally flip the
//! sign, so the angle jumped on a straight line. The waypoint now takes the
//! segment angle of the simulation's `AirPath` (the oracle's `m_angle`).
//!
//! The test flies a Fighter along 16 direction vectors (8 axial/diagonal and
//! 8 shallow/steep slopes — the vectors where the sign angle and the octant
//! angle disagree) and checks every frame while the sprite moves: the visual
//! angle equals `UnitsManager_GetTargetAngle` of the segment, and stays that
//! way after the arrival (the aircraft never turned away from the segment).

use max_app::{App, AppConfig};
use max_core::combat::get_target_angle;
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit::{AirPath, UnitPath};
use max_core::{TeamClan, UnitId};

/// The 16 flight directions: 8 basic and 8 shallow/steep slopes.
///
/// The expected angle is the oracle's `UnitsManager_GetTargetAngle` of the
/// segment (`airpath.cpp:56-58`). For the shallow vectors the sign-based
/// `paths_get_angle` returns the 45° diagonal instead — the regression.
const DIRECTIONS: [(i16, i16); 16] = [
    (1, 0),
    (0, 1),
    (-1, 0),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
    (4, 1),
    (1, 4),
    (4, -1),
    (1, -4),
    (-4, 1),
    (-1, 4),
    (-4, -1),
    (-1, -4),
];

const START: Point = Point::new(20, 20);
const TILE: i32 = 64;

/// `sqrt(dx² + dy²) * 4 + 0.5` in floats — the C++ `Paths_GetAirPath`
/// (`paths.cpp:131`); the exact value only sets the flight length, the angle
/// assertion does not depend on it.
fn scaled_distance4(distance_x: i32, distance_y: i32) -> i32 {
    (((distance_x * distance_x + distance_y * distance_y) as f64).sqrt() * 4.0 + 0.5) as i32
}

/// Builds the same `AirPath` `Paths_GetAirPath` produces for a straight
/// segment (`paths.cpp:209-210`, `airpath.cpp:46-62`): the segment angle is
/// `UnitsManager_GetTargetAngle` of the cell delta and the deltas step the
/// pixel line in `max_velocity = 4`-pixel-steps per tick.
fn air_path(start: Point, dx: i16, dy: i16, start_x: u16, start_y: u16) -> AirPath {
    let distance = scaled_distance4(i32::from(dx), i32::from(dy));
    let steps_distance = (distance + 3) / 4;
    let length = steps_distance * 4;

    AirPath {
        length: length as i16,
        angle: get_target_angle(i32::from(dx), i32::from(dy)) as i8,
        start_x: start_x as i16,
        start_y: start_y as i16,
        end_x: start.x + dx,
        end_y: start.y + dy,
        step_x: 0,
        step_y: 0,
        delta_x: (i32::from(dx) << 22) / length,
        delta_y: (i32::from(dy) << 22) / length,
    }
}

/// A Fighter (air) with a ready straight air route `(dx, dy)` from `START`,
/// already facing the segment angle — the flight starts on the first tick.
fn flying_fighter(app: &mut App, dx: i16, dy: i16) -> (UnitId, u8) {
    let id = app
        .universe_mut()
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            START,
            unit_flags::MOBILE_AIR_UNIT | unit_flags::SELECTABLE,
        )
        .expect("fighter");

    let mut unit = app.state_ref().unit(id).expect("fighter").clone();
    let expected = get_target_angle(i32::from(dx), i32::from(dy)) as u8;
    unit.angle = expected;
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::InProgress;
    unit.max_velocity = 4;
    unit.path = Some(UnitPath::Air(air_path(START, dx, dy, unit.x, unit.y)));
    unit.visible_to_team = [true; 5];
    app.universe_mut().write_unit(id, unit);
    app.update_visual_units(0.0);

    (id, expected)
}

/// Every direction of the table flies straight: on every frame the sprite
/// moves, `visual_angle` equals the oracle's segment angle
/// (`UnitsManager_GetTargetAngle`), and the angle survives the arrival.
#[test]
fn the_aircraft_sprite_faces_the_flight_direction_on_all_directions() {
    for &(dx, dy) in &DIRECTIONS {
        let mut app = App::new(AppConfig::default());
        app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
        app.universe_mut().team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
        let (id, expected) = flying_fighter(&mut app, dx, dy);
        let target = Point::new(START.x + dx, START.y + dy);

        let mut moved = false;

        for frame in 0..256 {
            if frame < 64 {
                app.tick();
            }
            app.update_visual_units(1.0 / 60.0);

            let unit = app.state_ref().unit(id).expect("fighter stays alive");
            let visual = app.visual_position(id).expect("visual position");
            let visual_angle = app.visual_angle(id).expect("visual angle");

            let shift_x = visual.0 - f32::from(unit.grid.x);
            let shift_y = visual.1 - f32::from(unit.grid.y);
            moved |= shift_x.abs() > 1e-4 || shift_y.abs() > 1e-4;

            assert_eq!(
                visual_angle, expected,
                "({dx}, {dy}): frame {frame} faces {visual_angle}, \
                 the segment angle is {expected}"
            );
        }

        assert!(moved, "({dx}, {dy}): the sprite never flew");

        let unit = app.state_ref().unit(id).expect("fighter stays alive");
        assert_eq!(
            unit.grid, target,
            "({dx}, {dy}): the flight must end on the target cell"
        );
        // `AirPath::Execute` lands the aircraft with the halved last step
        // (`airpath.cpp:203-213`): the geometric series 1/2 + 1/4 + ... leaves
        // the pixel up to one step-share short of the exact center — the
        // oracle's own landing point, so the check allows 1.5 px.
        let target_px = (
            i32::from(target.x) * TILE + TILE / 2,
            i32::from(target.y) * TILE + TILE / 2,
        );
        assert!(
            (i32::from(unit.x) - target_px.0).abs() <= 1
                && (i32::from(unit.y) - target_px.1).abs() <= 1,
            "({dx}, {dy}): the aircraft lands on the target pixel, got ({}, {})",
            unit.x,
            unit.y
        );
        assert_eq!(
            app.visual_angle(id),
            Some(expected),
            "({dx}, {dy}): after the arrival the sprite keeps the segment angle"
        );
    }
}
