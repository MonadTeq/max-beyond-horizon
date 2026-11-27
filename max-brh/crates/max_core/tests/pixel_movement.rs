//! Pixel movement of routed units — the defect "the sprite jumps on cell centers".
//!
//! Oracle: `UnitInfo::Move` (`unitinfo.cpp:1730-1892`) advances `x/y` by `velocity`
//! pixels per tick, recomputes `grid_x = x >> 6` when the pixel crosses a cell
//! border, and completes the cell on its center; `AirPath::Execute`
//! (`airpath.cpp:219-244`) does the same for aircraft along a straight segment.
//! `UnitInfo::Render` draws the sprite at `x/y`, so the drawing is smooth.
//!
//! These scenarios drive the public [`max_core::universe::GameUniverse`] API and
//! assert the simulation state: every order pass moves a few pixels (the movement
//! budget, `max_core::MOVEMENT_CELLS_PER_SECOND` cells per second), never a whole
//! cell, and the route ends exactly on the center of the last cell.

mod support;

use max_core::enums::{UnitOrder, UnitType};
use max_core::geo::{PathStep, Point};
use max_core::team::TeamId;
use max_core::unit::{GroundPath, UnitId, UnitPath};
use max_core::universe::GameUniverse;

/// The whole-pixel step of one order pass at the normal pace: `ceil(64 * 4 / 60)`
/// (`DRAIN_UNITS_PER_TICK / DRAIN_UNITS_PER_PIXEL`).
const MAX_PIXEL_STEP: i32 = 5;

/// Gives the unit a ready straight route east of `steps` cells (the state the path
/// manager leaves after `PathRequest::Finish`).
fn give_route(universe: &mut GameUniverse, id: UnitId, steps: i16, speed: u16) {
    let end = {
        let unit = universe.unit_mut(id).expect("unit");
        let end = Point::new(unit.grid.x + steps, unit.grid.y);

        unit.angle = 2;
        unit.orders = UnitOrder::Move;
        unit.state = max_core::enums::UnitOrderState::InProgress;
        unit.speed = speed;
        unit.path = Some(UnitPath::Ground(GroundPath {
            end_x: end.x,
            end_y: end.y,
            step_index: 0,
            steps: vec![PathStep::new(1, 0); steps as usize],
        }));

        end
    };

    universe.set_order_data(
        id,
        max_core::game::OrderData {
            move_to: end,
            fire_on: Point::ZERO,
        },
    );
}

/// A ground route walks **pixels**: every pass moves 4-5 pixels, no pass jumps a
/// cell, the hull passes through non-center pixels, and the route ends exactly on
/// the center of the destination cell (`UnitInfo::SetPosition`).
#[test]
fn a_ground_route_walks_pixels_and_lands_on_the_cell_center() {
    let mut universe = support::game();
    let id = support::spawn(
        &mut universe,
        UnitType::Tank,
        TeamId::RED,
        Point::new(10, 10),
    );
    give_route(&mut universe, id, 4, 6);

    let start = universe.snapshot().unit(id).expect("unit").clone();
    assert_eq!(
        (i32::from(start.x), i32::from(start.y)),
        (10 * 64 + 32, 10 * 64 + 32),
        "a standing unit is on the cell center"
    );

    let mut previous = (i32::from(start.x), i32::from(start.y));
    let mut max_step = 0;
    let mut saw_off_center = false;
    let mut ticks = 0usize;

    for _ in 0..400 {
        universe.tick();
        ticks += 1;

        let unit = universe.snapshot().unit(id).expect("unit").clone();
        let now = (i32::from(unit.x), i32::from(unit.y));
        let step = (now.0 - previous.0).abs().max((now.1 - previous.1).abs());

        max_step = max_step.max(step);
        assert!(
            step <= MAX_PIXEL_STEP,
            "tick {ticks}: the hull moves {step} pixels, more than the budget window \
             ({MAX_PIXEL_STEP}); previous {previous:?} -> {now:?}"
        );
        assert!(
            step < 32,
            "tick {ticks}: the hull jumps half a cell ({step} px): {previous:?} -> {now:?}"
        );

        if (now.0 - 32) % 64 != 0 || (now.1 - 32) % 64 != 0 {
            saw_off_center = true;
        }

        previous = now;

        if unit.path.is_none() {
            break;
        }
    }

    assert!(
        saw_off_center,
        "the hull must pass through pixels that are not cell centers"
    );
    assert!(
        max_step >= 4,
        "the hull must actually walk (max step {max_step} px)"
    );

    let unit = universe.snapshot().unit(id).expect("unit").clone();
    assert_eq!(
        (i32::from(unit.x), i32::from(unit.y)),
        (14 * 64 + 32, 10 * 64 + 32),
        "the route ends exactly on the center of the destination cell"
    );
    assert_eq!(unit.grid, Point::new(14, 10));
    assert_eq!(unit.moved, 0, "no half-cell offset is left over");
}

/// An aircraft route walks **pixels** along the straight segment: the axis step of
/// a frame never exceeds the budget window, no frame jumps the old 16-pixel
/// `AirPath` step, and the flight ends exactly on the destination cell center.
#[test]
fn an_air_route_walks_pixels_and_lands_on_the_cell_center() {
    let mut universe = support::game();
    let id = support::spawn(
        &mut universe,
        UnitType::Fighter,
        TeamId::RED,
        Point::new(10, 10),
    );
    support::set_hovering(&mut universe, id, true);
    support::order_move(&mut universe, id, Point::new(20, 14));

    let start = universe.snapshot().unit(id).expect("unit").clone();
    let mut previous = (i32::from(start.x), i32::from(start.y));
    let mut max_axis_step = 0;
    let mut max_euclid_step = 0.0f32;
    let mut ticks = 0usize;

    for _ in 0..2_000 {
        universe.tick();
        ticks += 1;

        let unit = universe.snapshot().unit(id).expect("unit").clone();
        let now = (i32::from(unit.x), i32::from(unit.y));
        let dx = (now.0 - previous.0).abs();
        let dy = (now.1 - previous.1).abs();
        let axis_step = dx.max(dy);

        assert_eq!(
            unit.grid,
            Point::new((now.0 >> 6) as i16, (now.1 >> 6) as i16),
            "tick {ticks}: the cell follows the pixel (`grid_x = x >> 6`)"
        );
        assert!(
            axis_step <= MAX_PIXEL_STEP,
            "tick {ticks}: the aircraft moves {axis_step} pixels on an axis, more than \
             the budget window ({MAX_PIXEL_STEP}): {previous:?} -> {now:?}"
        );
        assert!(
            axis_step < 16,
            "tick {ticks}: the aircraft jumps a whole `AirPath` step ({axis_step} px): \
             {previous:?} -> {now:?}"
        );

        max_axis_step = max_axis_step.max(axis_step);
        max_euclid_step = max_euclid_step.max(((dx * dx + dy * dy) as f32).sqrt());
        previous = now;

        if unit.path.is_none() {
            break;
        }
    }

    assert!(ticks > 30, "the aircraft flies for several budget windows");
    assert!(
        max_axis_step >= 4,
        "the aircraft must actually fly (max axis step {max_axis_step} px)"
    );
    assert!(
        max_euclid_step <= 8.0,
        "the diagonal of one frame stays within the budget window: {max_euclid_step} px"
    );

    let unit = universe.snapshot().unit(id).expect("unit").clone();
    assert_eq!(
        (i32::from(unit.x), i32::from(unit.y)),
        (20 * 64 + 32, 14 * 64 + 32),
        "the flight ends exactly on the center of the destination cell"
    );
    assert_eq!(unit.grid, Point::new(20, 14));
}

/// The intercept fires on the **cell border**: the mover's `grid` changes when its
/// pixel crosses into the halt cell (half a cell before the center), the emergency
/// stop is queued there, and the hull finishes the current cell to its center.
#[test]
fn the_intercept_triggers_on_the_cell_border() {
    let mut universe = support::fog_game();
    universe.set_enemy_halt(true);
    universe.begin_local_round();

    let mover = support::spawn(
        &mut universe,
        UnitType::Tank,
        TeamId::RED,
        Point::new(10, 10),
    );
    let enemy = support::spawn(
        &mut universe,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(20, 10),
    );
    universe.update_visibility_status(false);
    universe.spot_by_team(enemy, TeamId::RED);

    let range = universe
        .snapshot()
        .unit(enemy)
        .expect("enemy")
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_RANGE);
    let halt = Point::new(20 - range as i16, 10);

    give_route(&mut universe, mover, 8, 8);

    let mut border_crossing = None;

    for _ in 0..support::TURN_CLOSE_TICK_LIMIT {
        universe.tick();

        let unit = universe.snapshot().unit(mover).expect("mover").clone();

        if border_crossing.is_none() && unit.grid == halt {
            border_crossing = Some((
                i32::from(unit.x),
                i32::from(unit.y),
                unit.moved,
                unit.orders,
            ));
        }

        if unit.orders == UnitOrder::Await {
            break;
        }
    }

    let (x, y, moved, _) = border_crossing.expect("the mover enters the halt cell");
    assert_eq!(y, 10 * 64 + 32, "the route is straight east");
    assert!(
        (i32::from(halt.x) * 64..i32::from(halt.x) * 64 + MAX_PIXEL_STEP).contains(&x),
        "the cell changes on its border, not on the center: x = {x}"
    );
    assert!(
        (32..64).contains(&moved),
        "the hull is mid-cell when the intercept fires: moved = {moved}"
    );

    let unit = universe.snapshot().unit(mover).expect("mover").clone();
    assert_eq!(unit.grid, halt, "the mover halts on the range boundary");
    assert_eq!(
        (i32::from(unit.x), i32::from(unit.y)),
        (i32::from(halt.x) * 64 + 32, 10 * 64 + 32),
        "the halted hull finishes the current cell on its center"
    );
    assert_eq!(unit.orders, UnitOrder::Await);
    assert!(unit.path.is_none(), "the intercept clears the route");
}
