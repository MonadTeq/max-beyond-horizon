use super::*;
use crate::enums::TeamClan;
use crate::team::TeamId;
use crate::team::TeamKind;

fn state_with_tank() -> (GameState, UnitId) {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    // Tank speed from the attribute table — so path steps are possible.
    let id = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE | UPGRADABLE,
        )
        .expect("spawn");
    (state, id)
}

fn order_move_to(state: &mut GameState, id: UnitId, target: Point) {
    state.set_order_data(
        id,
        OrderData {
            move_to: target,
            fire_on: Point::ZERO,
        },
    );
    state.set_order(id, UnitOrder::Move, UnitOrderState::Init);
}

#[test]
fn straight_line_move_reaches_target_and_restores_await() {
    let (mut state, id) = state_with_tank();
    order_move_to(&mut state, id, Point::new(14, 10));

    let mut ticks = 0;
    loop {
        state.tick();
        ticks += 1;
        let unit = state.unit(id).expect("unit");
        if unit.grid == Point::new(14, 10) && !unit.has_move_order() {
            break;
        }
        assert!(ticks < 64, "юнит не дошёл до цели за 64 тика");
    }

    let unit = state.unit(id).expect("unit");
    assert_eq!(unit.grid, Point::new(14, 10));
    assert_eq!(unit.orders, UnitOrder::Await);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert!(unit.path.is_none(), "путь снимается на завершении");
    // The tank spends 1 speed unit per cell: 4 cells → speed-4.
    assert_eq!(
        unit.speed,
        unit.base_values.get_attribute(ATTRIB_SPEED) as u16 - 4
    );
}

#[test]
fn move_order_is_replaced_by_new_order() {
    let (mut state, id) = state_with_tank();
    order_move_to(&mut state, id, Point::new(30, 10));
    // The first tick places a path request (`NEW_ORDER`), the second applies it
    // (`poll_results` → `IN_PROGRESS`) — as in C++ with `PathsManager`.
    state.tick();
    state.tick();
    assert_eq!(state.unit(id).unwrap().state, UnitOrderState::InProgress);

    // Order change during movement: prior records Move/InProgress.
    state.set_order(id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
    let unit = state.unit(id).expect("unit");
    assert_eq!(unit.orders, UnitOrder::Sentry);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert_eq!(unit.prior_orders, UnitOrder::Move);

    // SENTRY does not move the unit.
    let before = state.unit(id).unwrap().grid;
    for _ in 0..4 {
        state.tick();
    }
    assert_eq!(state.unit(id).unwrap().grid, before);
}

#[test]
fn set_new_order_int_keeps_prior_and_explode_override() {
    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::InProgress;

    assert!(!set_new_order_int(
        &mut unit,
        UnitOrder::Fire,
        UnitOrderState::Init
    ));
    assert_eq!(unit.orders, UnitOrder::Fire);
    assert_eq!(unit.prior_orders, UnitOrder::Move);
    assert_eq!(unit.prior_state, UnitOrderState::InProgress);

    // Explosion overrides fire and does not preserve prior.
    assert!(!set_new_order_int(
        &mut unit,
        UnitOrder::Explode,
        UnitOrderState::Init
    ));
    assert_eq!(unit.orders, UnitOrder::Explode);

    // From NEW_ORDER the order returns to AWAIT and asks to remove the path.
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::NewOrder;
    assert!(set_new_order_int(
        &mut unit,
        UnitOrder::Await,
        UnitOrderState::ExecutingOrder
    ));
    assert_eq!(unit.prior_orders, UnitOrder::Await);
}

#[test]
fn restore_orders_matches_cpp_building_exception() {
    let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    tank.orders = UnitOrder::Move;
    tank.state = UnitOrderState::InProgress;
    tank.prior_orders = UnitOrder::Move;
    tank.prior_state = UnitOrderState::InProgress;
    restore_orders(&mut tank);
    assert_eq!(tank.orders, UnitOrder::Await);
    assert_eq!(tank.state, UnitOrderState::ExecutingOrder);

    // For powered buildings the prior/order match is preserved (as in C++).
    let mut tower = UnitInfo::new(UnitType::GoldRefinery, TeamId::RED, UnitId(2));
    tower.orders = UnitOrder::PowerOn;
    tower.state = UnitOrderState::Init;
    tower.prior_orders = UnitOrder::PowerOn;
    tower.prior_state = UnitOrderState::Init;
    restore_orders(&mut tower);
    assert_eq!(tower.orders, UnitOrder::PowerOn);
    assert_eq!(tower.state, UnitOrderState::Init);
}

#[test]
fn all_32_orders_dispatch_without_panic() {
    let (mut state, id) = state_with_tank();
    for order in UnitOrder::ALL {
        state.set_order_data(
            id,
            OrderData {
                move_to: Point::new(12, 10),
                fire_on: Point::new(12, 10),
            },
        );
        state.set_order(id, *order, UnitOrderState::Init);
        state.tick();
    }
    assert!(state.has_unit(id));
}

#[test]
fn path_blocked_without_speed_keeps_order_until_new_turn() {
    let (mut state, id) = state_with_tank();
    order_move_to(&mut state, id, Point::new(14, 10));
    // Zero the speed: the path step does not start, `MoveFinished` resets
    // movement, but the order stays MOVE (as in `GroundPath::Execute` with
    // `speed == 0`: state EXECUTING_ORDER, the path is kept until
    // speed is restored at the start of the turn).
    state.unit_mut(id).unwrap().speed = 0;
    // Tick 1 — path request; tick 2 — path applied and `GroundPath::Execute`
    // with `speed == 0` leaves the order MOVE with state EXECUTING_ORDER.
    state.tick();
    state.tick();
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.orders, UnitOrder::Move);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert!(unit.path.is_some());
    // `MoveFinished` set `delayed_reaction`, but `AssessAttacks` at
    // the end of the same tick (`ProcessOrders`) cleared it: the team has no
    // delayed targets (`UnitsManager_ClearDelayedReactions`).
    assert_eq!(unit.delayed_reaction, 0);
    assert_eq!(unit.moved, 0);

    // The start of the turn restores speed (`UpdateProduction`) — movement
    // continues from the same place.
    let base_speed = state
        .unit(id)
        .unwrap()
        .base_values
        .get_attribute(ATTRIB_SPEED) as u16;
    state.unit_mut(id).unwrap().speed = base_speed;
    state.set_order(id, UnitOrder::Move, UnitOrderState::Init);
    let mut ticks = 0;
    loop {
        state.tick();
        ticks += 1;
        assert!(ticks < 64, "юнит не дошёл до цели после сброса скорости");
        let unit = state.unit(id).unwrap();
        if unit.grid == Point::new(14, 10) && !unit.has_move_order() {
            break;
        }
    }
    assert_eq!(state.unit(id).unwrap().orders, UnitOrder::Await);
}

#[test]
fn air_move_reaches_target_cell() {
    use crate::enums::unit_flags::MOBILE_AIR_UNIT;
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    let id = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_AIR_UNIT,
        )
        .expect("spawn");
    order_move_to(&mut state, id, Point::new(10, 10));

    let mut ticks = 0;
    loop {
        state.tick();
        ticks += 1;
        assert!(ticks < 64, "самолёт не долетел до цели");
        let unit = state.unit(id).unwrap();
        if unit.grid == Point::new(10, 10) && !unit.has_move_order() {
            break;
        }
    }
    assert_eq!(state.unit(id).unwrap().grid, Point::new(10, 10));
}

#[test]
fn paths_calculate_step_spends_speed() {
    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    unit.speed = 12;
    unit.base_values.set_attribute(ATTRIB_SPEED, 12);
    unit.base_values.set_attribute(ATTRIB_ROUNDS, 2);
    unit.base_values.set_attribute(ATTRIB_MOVE_AND_FIRE, 0);
    unit.shots = 2;

    assert!(paths_calculate_step(&mut unit, 4, false, false, false));
    assert_eq!(unit.speed, 11);
    assert_eq!(unit.move_fraction, 0);
    assert_eq!(unit.max_velocity, 8);

    // The diagonal costs 6: 2 is charged with an empty move_fraction.
    unit.speed = 12;
    assert!(paths_calculate_step(&mut unit, 4, true, false, false));
    assert_eq!(unit.speed, 10);

    // Not enough speed — the step does not start.
    unit.speed = 0;
    assert!(!paths_calculate_step(&mut unit, 4, false, false, false));
}

#[test]
fn paths_angle_and_turn_match_cpp() {
    assert_eq!(paths_get_angle(1, 0), 2);
    assert_eq!(paths_get_angle(1, 1), 3);
    assert_eq!(paths_get_angle(0, 1), 4);
    assert_eq!(paths_get_angle(-1, 1), 5);
    assert_eq!(paths_get_angle(-1, 0), 6);
    assert_eq!(paths_get_angle(-1, -1), 7);
    assert_eq!(paths_get_angle(0, -1), 0);
    assert_eq!(paths_get_angle(1, -1), 1);

    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    unit.angle = 0;
    // One rotation step per call until it matches.
    let mut calls = 0;
    while paths_update_angle(&mut unit, 4) {
        calls += 1;
        assert!(calls < 8);
    }
    assert_eq!(unit.angle, 4);
}

/// Angle 0..7 → step (dx, dy) (`DIRECTION_OFFSETS`, `path.hpp`).
fn step_for_angle(angle: i32) -> PathStep {
    let offset = DIRECTION_OFFSETS[(angle & 7) as usize];
    PathStep::new(offset.x as i8, offset.y as i8)
}

/// A tank with a manually set ground path and a `Move/IN_PROGRESS` order.
fn state_with_path(angle: u8, steps: Vec<PathStep>) -> (GameState, UnitId) {
    let (mut state, id) = state_with_tank();
    state.unit_mut(id).unwrap().angle = angle;
    state.set_order_data(
        id,
        OrderData {
            move_to: Point::new(10, 10),
            fire_on: Point::ZERO,
        },
    );
    state.unit_mut(id).unwrap().path = Some(UnitPath::Ground(GroundPath {
        end_x: 10,
        end_y: 10,
        step_index: 0,
        steps,
    }));
    state.set_order(id, UnitOrder::Move, UnitOrderState::InProgress);
    (state, id)
}

/// Circular angle difference `a - b` in the range −4..=3.
fn angle_delta(a: u8, b: u8) -> i32 {
    ((i32::from(a) - i32::from(b) + 12) & 7) - 4
}

/// C++ `GroundPath::UpdateUnitAngle`: on cell completion — exactly one
/// 45° step toward the next route step; without a next step the angle stays.
#[test]
fn ground_path_update_unit_angle_matches_cpp() {
    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    unit.angle = 2;

    let path = GroundPath {
        end_x: 0,
        end_y: 0,
        step_index: 0,
        steps: vec![PathStep::new(1, 0), PathStep::new(0, -1)],
    };
    assert!(ground_path_update_unit_angle(&mut unit, &path));
    assert_eq!(unit.angle, 1, "E → NE: один шаг к следующему шагу (N)");

    // No next step — rotation does not start (the last cell).
    let last = GroundPath {
        end_x: 0,
        end_y: 0,
        step_index: 1,
        steps: vec![PathStep::new(1, 0), PathStep::new(0, -1)],
    };
    let before = unit.angle;
    assert!(!ground_path_update_unit_angle(&mut unit, &last));
    assert_eq!(unit.angle, before);
}

/// `UnitInfo::UpdateAngle`: the turret rotates with the hull.
#[test]
fn body_turn_rotates_turret_like_cpp_update_angle() {
    let mut turreted = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    turreted.flags |= TURRET_SPRITE;
    turreted.angle = 2;
    turreted.turret_angle = 0;

    assert!(paths_update_angle(&mut turreted, 3));
    assert_eq!(turreted.angle, 3);
    assert_eq!(turreted.turret_angle, 1);

    // Without the turret flag the second field is not touched.
    let mut plain = UnitInfo::new(UnitType::Scout, TeamId::RED, UnitId(2));
    plain.angle = 2;
    plain.turret_angle = 6;
    assert!(paths_update_angle(&mut plain, 3));
    assert_eq!(plain.angle, 3);
    assert_eq!(plain.turret_angle, 6);
}

/// Straight movement in all 8 directions: the unit turns toward the step
/// direction, moves only "facing" the course and keeps the last angle.
#[test]
fn all_eight_directions_move_facing_the_step() {
    for direction in 0..8i32 {
        let step = step_for_angle(direction);
        let start_angle = ((direction + 4) & 7) as u8;
        let (mut state, id) = state_with_path(start_angle, vec![step; 3]);

        let mut previous = state.unit(id).unwrap().grid;
        let mut moves = 0;
        let mut last_angle = start_angle;

        for _ in 0..40 {
            state.tick();
            let unit = state.unit(id).unwrap();
            let changed = unit.grid != previous;

            if changed {
                let dx = unit.grid.x - previous.x;
                let dy = unit.grid.y - previous.y;
                assert_eq!(
                    paths_get_angle(i32::from(dx), i32::from(dy)),
                    direction,
                        "шаг {direction} ушёл не в ту сторону: {previous:?} → {:?}",
                    unit.grid
                );
                // On a straight line the next step matches — the angle must be
                // the movement direction right after the step.
                assert_eq!(
                    i32::from(unit.angle),
                    direction,
                        "прямой ход {direction}: угол не совпал с шагом"
                );
                moves += 1;
                previous = unit.grid;
            }

            // Rotation — at most one 45° step per tick.
            let delta = angle_delta(unit.angle, last_angle);
            assert!(
                (-1..=1).contains(&delta),
                    "поворот {direction}: скачок угла {last_angle} → {} за тик",
                unit.angle
            );
            last_angle = unit.angle;

            if moves == 3 {
                break;
            }
        }

        assert_eq!(moves, 3, "направление {direction}: пройдено не 3 клетки");
        let unit = state.unit(id).unwrap();
        assert_eq!(
            i32::from(unit.angle),
            direction,
                "после остановки угол — последнее направление {direction}"
        );

        // After stopping the angle does not change.
        for _ in 0..8 {
            state.tick();
            assert_eq!(i32::from(state.unit(id).unwrap().angle), direction);
        }
    }
}

/// L-route: on completing the current cell the hull makes one step toward
/// the next one (`GroundPath::UpdateUnitAngle`), then finishes turning in
/// `Execute` and only after that moves. Movement is always along the course.
#[test]
fn turn_at_cell_boundary_precedes_the_next_step() {
    let (mut state, id) = state_with_path(
        2,
        vec![
            PathStep::new(1, 0),
            PathStep::new(1, 0),
            PathStep::new(0, -1),
            PathStep::new(0, -1),
        ],
    );

    // Tick 1: the angle is already east — step to (11,10); the next step is still
    // east, so at the cell boundary there is no rotation.
    state.tick();
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.grid, Point::new(11, 10));
    assert_eq!(unit.angle, 2);

    // Tick 2: step to (12,10); at the boundary rotation toward the next
    // step (north) starts: E → NE.
    state.tick();
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.grid, Point::new(12, 10));
    assert_eq!(unit.angle, 1, "на границе клетки — один шаг 45° к северу");

    // Tick 3: finish turning NE → N without movement (as `GroundPath::Execute`).
    state.tick();
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.grid, Point::new(12, 10));
    assert_eq!(unit.angle, 0);

    // Tick 4: movement north.
    state.tick();
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.grid, Point::new(12, 9));
    assert_eq!(unit.angle, 0);

    // On every moving tick the angle matches the transition direction
    // (tolerance ±1 — only a rotation started at the boundary toward the next step).
    let mut previous = unit.grid;
    for _ in 0..40 {
        state.tick();
        let unit = state.unit(id).unwrap();
        if unit.grid != previous {
            let dx = i32::from(unit.grid.x) - i32::from(previous.x);
            let dy = i32::from(unit.grid.y) - i32::from(previous.y);
            let moved_angle = paths_get_angle(dx, dy) as u8;
            assert!(
                angle_delta(unit.angle, moved_angle).abs() <= 1,
                    "кадр/сим повернулся на 45°+ против движения: {:?} → {:?}, angle {}",
                previous,
                unit.grid,
                unit.angle
            );
            previous = unit.grid;
        }
        if !unit.has_move_order() {
            break;
        }
    }

    assert_eq!(state.unit(id).unwrap().grid, Point::new(12, 8));
    assert_eq!(state.unit(id).unwrap().angle, 0);
}

/// Two identical L-route runs give byte-identical snapshots on
/// every tick: `angle` is part of the state and changes deterministically.
#[test]
fn angle_updates_deterministically_in_snapshot() {
    let steps = vec![
        PathStep::new(1, 0),
        PathStep::new(1, -1),
        PathStep::new(0, -1),
        PathStep::new(0, -1),
    ];
    let (mut first, first_id) = state_with_path(7, steps.clone());
    let (mut second, second_id) = state_with_path(7, steps);
    assert_eq!(first_id, second_id);

    let mut angle_changed = false;
    for tick in 0..24 {
        first.tick();
        second.tick();
        assert_eq!(
            first.snapshot_bytes(),
            second.snapshot_bytes(),
                "снапшот разошёлся на тике {tick}"
        );
        angle_changed |= first.unit(first_id).unwrap().angle != 7;
    }

    assert!(angle_changed, "тест обязан пройти через смену угла");

    // `angle` enters the snapshot (state CRC).
    let before = first.snapshot_bytes();
    first.unit_mut(first_id).unwrap().angle ^= 1;
    assert_ne!(before, first.snapshot_bytes());
}

#[test]
fn land_and_take_animation_counters() {
    let mut unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(1));
    unit.moved = 0;
    let mut calls = 0;
    while !take(&mut unit) {
        calls += 1;
        assert!(calls < 8);
    }
    assert_eq!(unit.moved, 5);
    assert_ne!(unit.flags & HOVERING, 0);

    let mut calls = 0;
    while !land(&mut unit) {
        calls += 1;
        assert!(calls < 16);
    }
    assert_eq!(unit.moved, 9);
    assert_eq!(unit.flags & HOVERING, 0);
}

#[test]
fn order_move_to_unit_stores_into_idle() {
    let (mut state, id) = state_with_tank();
    // Parent transport and passenger.
    let carrier = state
        .spawn_unit(
            UnitType::ArmouredPersonnelCarrier,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_SEA_UNIT | MOBILE_LAND_UNIT,
        )
        .expect("carrier");
    state.unit_mut(id).unwrap().parent_unit = Some(carrier);
    state.unit_mut(carrier).unwrap().storage = 0;

    state.set_order(id, UnitOrder::Idle, UnitOrderState::PrepareStore);
    state.set_order(id, UnitOrder::Load, UnitOrderState::Init);
    for _ in 0..24 {
        state.tick();
    }
    let unit = state.unit(id).unwrap();
    // `ProcessLoading` completes boarding: the passenger in AWAIT, without a parent.
    assert_eq!(unit.orders, UnitOrder::Await);
    assert!(unit.parent_unit.is_none());
    assert!(state.unit(carrier).unwrap().storage >= 0);
}

/// `Paths_FinishMove` (`airpath.cpp:430-443`): the air transport reached the
/// passenger it was sent to load — `CURSOR_LOAD` gives it `ORDER_MOVE/INIT` to the
/// passenger cell with `parent = passenger` (`game_manager.cpp:6814-6818`). On
/// arrival the transport takes `ORDER_LOAD/INIT` (not merely `BlockedOnPathRequest`)
/// and the passenger goes into the cargo bay.
#[test]
fn air_transport_loads_passenger_on_arrival() {
    let (mut state, passenger, transport) = state_with_air_transport_and_passenger();
    state.unit_mut(transport).unwrap().parent_unit = Some(passenger);
    order_move_to(&mut state, transport, Point::new(10, 10));

    let mut saw_load = false;
    for _ in 0..160 {
        state.tick();
        let transport_unit = state.unit(transport).expect("transport");
        saw_load |= transport_unit.orders == UnitOrder::Load;
    }

    assert!(
        saw_load,
            "the air transport must take ORDER_LOAD on arrival at the passenger cell"
    );

    let transport_unit = state.unit(transport).expect("transport");
    assert_eq!(transport_unit.storage, 1, "passenger occupies a cargo bay");
    assert_eq!(transport_unit.orders, UnitOrder::Await);
    assert_eq!(transport_unit.state, UnitOrderState::ExecutingOrder);
    assert!(
        transport_unit.parent_unit.is_none(),
            "`ProcessLoading` clears the transport parent"
    );

    let passenger_unit = state.unit(passenger).expect("passenger");
    assert_eq!(
        passenger_unit.parent_unit,
        Some(transport),
            "passenger parent is the transport"
    );
    assert_eq!(passenger_unit.orders, UnitOrder::Idle);
}

/// `UnitsManager_LoadUnit` (`units_manager.cpp:5027`), the first branch of
/// `Paths_FinishMove` (`airpath.cpp:430`): an aircraft that finishes its path over
/// an own landing pad starts landing (`ORDER_LAND/INIT`) instead of stopping.
#[test]
fn air_unit_lands_on_landing_pad_after_move() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);

    state
        .spawn_unit(
            UnitType::LandingPad,
            TeamId::RED,
            Point::new(10, 10),
            STATIONARY | SELECTABLE,
        )
        .expect("landing pad");
    let aircraft = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(10, 13),
            MOBILE_AIR_UNIT | SELECTABLE | UPGRADABLE,
        )
        .expect("aircraft");

    order_move_to(&mut state, aircraft, Point::new(10, 10));

    let mut saw_land = false;
    for _ in 0..160 {
        state.tick();
        saw_land |= state.unit(aircraft).map(|unit| unit.orders) == Some(UnitOrder::Land);
    }

    assert!(saw_land, "the aircraft must take ORDER_LAND over the pad");
}

fn state_with_air_transport_and_passenger() -> (GameState, UnitId, UnitId) {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);

    let passenger = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE | UPGRADABLE,
        )
        .expect("passenger");
    let transport = state
        .spawn_unit(
            UnitType::AirTransport,
            TeamId::RED,
            Point::new(10, 13),
            MOBILE_AIR_UNIT | SELECTABLE | UPGRADABLE,
        )
        .expect("transport");

    (state, passenger, transport)
}

#[test]
fn explode_of_dying_infantry_reaches_destroy() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    let id = state
        .spawn_unit(
            UnitType::Infantry,
            TeamId::RED,
            Point::new(3, 3),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("spawn");
    {
        let unit = state.unit_mut(id).unwrap();
        unit.hits = 0;
        unit.orders = UnitOrder::Explode;
        unit.state = UnitOrderState::Init;
    }
    state.tick();
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.state, UnitOrderState::Destroy);
    // `units_manager.cpp:3932-3946`: its own 8-frame death animation
    // (`image_base = 168 + (angle / 2) * 8`, `image_index_max = base + 7`).
    assert_eq!(unit.image_base, 168);
    assert_eq!(unit.image_index_max, 175);
    // The animation goes one frame per two ticks, then the unit is destroyed.
    let mut destroyed = false;
    for _ in 0..24 {
        state.tick();
        if !state.has_unit(id) {
            destroyed = true;
            break;
        }
    }
    assert!(destroyed);
}

#[test]
fn await_scaling_expand_decrements_adjust() {
    let (mut state, id) = state_with_tank();
    {
        let unit = state.unit_mut(id).unwrap();
        unit.orders = UnitOrder::AwaitScaling;
        unit.state = UnitOrderState::Expand;
        unit.prior_orders = UnitOrder::Move;
        unit.prior_state = UnitOrderState::InProgress;
        unit.scaler_adjust = 2;
    }
    state.tick();
    assert_eq!(state.unit(id).unwrap().scaler_adjust, 1);
    state.tick();
    state.tick();
    // scale to zero → RestoreOrders returns Move/InProgress.
    assert_eq!(state.unit(id).unwrap().orders, UnitOrder::Move);
}

// -----------------------------------------------------------------------
// Projectiles: a pixel line (`AirPath::Execute`, airpath.cpp:175-295)
// -----------------------------------------------------------------------

/// A missile at (10,10) flies to (13,14): dx=3, dy=4 — the line goes **not** along
/// the 8-direction route (the old port walked cells and wobbled).
fn state_with_missile_in_flight() -> (GameState, UnitId, UnitId, (i32, i32)) {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Computer;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = crate::team::TeamUnits::with_clan(TeamClan::Random);

    let target = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(13, 14),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("target");
    let attacker = state
        .spawn_unit(
            UnitType::MissileCrawler,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("attacker");
    let missile = state
        .spawn_unit(UnitType::Rocket, TeamId::RED, Point::new(10, 10), 0)
        .expect("rocket");
    let start = {
        let unit = state.unit(missile).expect("rocket");
        (i32::from(unit.x), i32::from(unit.y))
    };
    let mut unit = state.unit(missile).expect("rocket").clone();
    unit.parent_unit = Some(attacker);
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::Init;
    state.write_unit(missile, unit);
    state.set_order_data(
        missile,
        OrderData {
            move_to: Point::new(13, 14),
            fire_on: Point::new(13, 14),
        },
    );
    state.set_order_data(
        attacker,
        OrderData {
            move_to: Point::new(13, 14),
            fire_on: Point::new(13, 14),
        },
    );

    (state, missile, target, start)
}

/// Perpendicular distance from a point to the line `start → end` (pixels).
fn distance_to_line(point: (i32, i32), start: (i32, i32), end: (i32, i32)) -> f64 {
    let dx = f64::from(end.0 - start.0);
    let dy = f64::from(end.1 - start.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length == 0.0 {
        return 0.0;
    }
    let px = f64::from(point.0 - start.0);
    let py = f64::from(point.1 - start.1);
    (px * dy - py * dx).abs() / length
}

#[test]
fn missile_flies_a_pixel_straight_line() {
    let (mut state, missile, _target, start) = state_with_missile_in_flight();
    // The aim point is the cell center (13,14).
    let end = (13 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2, 14 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2);

    let mut samples = Vec::new();
    for _ in 0..12 {
        state.tick();
        let Some(unit) = state.unit(missile) else {
            break;
        };
        samples.push((i32::from(unit.x), i32::from(unit.y)));
    }

    assert!(samples.len() >= 3, "снаряд должен лететь несколько тиков");
    // The first point is still at the launch site, then strictly along the line.
    for point in &samples {
        let distance = distance_to_line(*point, start, end);
        assert!(
            distance <= 1.5,
                "точка {point:?} не на прямой {start:?}→{end:?}: отклонение {distance:.2} px"
        );
    }
    // The position really changes from tick to tick (a pixel step, not
    // "a cell per tick to the center").
    let unique: std::collections::BTreeSet<_> = samples.iter().collect();
    assert!(unique.len() >= 3, "снаряд должен двигаться попиксельно");
    // And it arrives at the target cell (the last live point — near the center).
    let last = *samples.last().expect("точки");
    assert!(
        distance_to_line(last, start, end) <= 1.5
            && (last.0 - end.0).abs() <= GFX_MAP_TILE_SIZE
            && (last.1 - end.1).abs() <= GFX_MAP_TILE_SIZE,
            "снаряд не дошёл до цели: {last:?}"
    );
}

#[test]
fn missile_trail_particles_lie_on_the_line() {
    let (mut state, missile, _target, start) = state_with_missile_in_flight();
    let end = (13 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2, 14 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2);

    // One tick: the projectile traverses the first cell and leaves three particles behind.
    state.tick();
    state.tick();
    let particles: Vec<_> = state
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.unit(id).cloned())
        .filter(|unit| unit.unit_type == UnitType::RocketSmoke)
        .collect();

    assert!(
        particles.len() >= 3,
            "Paths_DrawMissile: три частицы на покинутую клетку, найдено {}",
        particles.len()
    );
    for particle in &particles {
        let point = (i32::from(particle.x), i32::from(particle.y));
        let distance = distance_to_line(point, start, end);
        assert!(
            distance <= 1.5,
                "частица следа {point:?} не на прямой (отклонение {distance:.2} px)"
        );
    }
    let _ = missile;
}

#[test]
fn missile_impact_uses_the_engine_cell_and_destroys_the_round() {
    let (mut state, missile, target, _start) = state_with_missile_in_flight();

    let mut impact_cell = None;
    let mut destroyed = false;
    for _ in 0..24 {
        state.tick();

        if impact_cell.is_none() {
            impact_cell = state.unit_ids_in_order().into_iter().find_map(|id| {
                state.unit(id).and_then(|unit| {
                    matches!(
                        unit.unit_type,
                        UnitType::HitExplosion
                            | UnitType::LandExplosion
                            | UnitType::BuildingExplosion
                            | UnitType::AirExplosion
                            | UnitType::SeaExplosion
                    )
                    .then_some(unit.grid)
                })
            });
        }

        if !state.has_unit(missile) {
            destroyed = true;

            // The explosion is placed on the next tick (`ProcessOrderExplode`/`INIT`
            // → `UnitsManager_StartExplosion`), so we collect frames.
            if impact_cell.is_some() {
                break;
            }
        }
    }

    assert!(destroyed, "по `m_length == 0` снаряд уничтожается");
    // `Paths_FinishMove` strikes the target cell (`13,14`) — the explosion
    // `HITEXPLD`/`LNDEXPLD` appears exactly there.
    assert_eq!(
        impact_cell,
        Some(Point::new(13, 14)),
            "`parent->Attack(grid_x, grid_y)`: удар в клетке цели"
    );
    // And the target took damage.
    let tank = state.unit(target).expect("танк");
    assert!(
        tank.hits == 0
            || tank.hits
                < tank
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_HITS) as u16,
            "попадание должно нанести урон"
    );
}

// -----------------------------------------------------------------------
// State-machine boundaries: waiting, stopping, order override, building
// -----------------------------------------------------------------------

/// `UnitInfo::MovementWaitCount`: saturates at `MOVEMENT_WAIT_LIMIT`, reports
/// `true` from that point on and never wraps; `ClearMovementWait` resets it.
#[test]
fn movement_wait_limit_saturates() {
    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    assert_eq!(unit.movement_wait_count, 0);
    assert!(!register_movement_wait(&mut unit));

    // One step before the limit becomes the timed-out state.
    unit.movement_wait_count = MOVEMENT_WAIT_LIMIT - 1;
    assert!(register_movement_wait(&mut unit));
    assert_eq!(unit.movement_wait_count, MOVEMENT_WAIT_LIMIT);

    // At the limit every further call reports the timeout and keeps the count.
    assert!(register_movement_wait(&mut unit));
    assert!(register_movement_wait(&mut unit));
    assert_eq!(unit.movement_wait_count, MOVEMENT_WAIT_LIMIT, "no wrap");

    clear_movement_wait(&mut unit);
    assert_eq!(unit.movement_wait_count, 0);
    assert!(!register_movement_wait(&mut unit));
}

/// `UnitInfo::StopMove` only converts movement orders to a stopped `MOVE`;
/// other orders and their counters are untouched.
#[test]
fn stop_move_affects_only_move_orders() {
    for order in [
        UnitOrder::Move,
        UnitOrder::MoveToUnit,
        UnitOrder::MoveToAttack,
    ] {
        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.orders = order;
        unit.state = UnitOrderState::InProgress;
        unit.path = Some(UnitPath::Ground(GroundPath::default()));
        unit.moved = 5;
        unit.velocity = 7;
        unit.in_transit = true;

        stop_move(&mut unit, false);
        assert_eq!(unit.orders, UnitOrder::Move, "{order:?}");
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder, "{order:?}");
        assert_eq!(unit.moved, 0);
        assert_eq!(unit.velocity, 0);
        assert!(!unit.in_transit);
        assert_eq!(unit.delayed_reaction, 0, "mode=false");
    }

    let mut awaiting = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
    awaiting.moved = 3;
    stop_move(&mut awaiting, true);
    assert_eq!(awaiting.orders, UnitOrder::Await);
    assert_eq!(awaiting.moved, 3, "non-movement order is not reset");

    let mut firing = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(3));
    firing.orders = UnitOrder::Fire;
    firing.state = UnitOrderState::FireInProgress;
    stop_move(&mut firing, true);
    assert_eq!(firing.orders, UnitOrder::Fire);
    assert_eq!(firing.state, UnitOrderState::FireInProgress);
}

/// `UnitInfo::StopMovement` (emergency stop): `IN_PROGRESS` blocks the route,
/// `IN_TRANSITION` truncates it to a zero step; other states are left alone.
#[test]
fn stop_movement_handles_in_progress_and_in_transition() {
    let path = || {
        Some(UnitPath::Ground(GroundPath {
            end_x: 5,
            end_y: 5,
            step_index: 1,
            steps: vec![PathStep::new(1, 0), PathStep::new(0, 1)],
        }))
    };

    let mut moving = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    moving.orders = UnitOrder::Move;
    moving.state = UnitOrderState::InProgress;
    moving.path = path();
    stop_movement(&mut moving, false);
    assert_eq!(moving.orders, UnitOrder::Await);
    assert_eq!(moving.state, UnitOrderState::ExecutingOrder);
    assert!(moving.path.is_none());
    assert_eq!(moving.delayed_reaction, 1);

    let mut transitioning = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
    transitioning.orders = UnitOrder::Move;
    transitioning.state = UnitOrderState::InTransition;
    transitioning.path = path();
    stop_movement(&mut transitioning, false);
    assert_eq!(transitioning.orders, UnitOrder::Move);
    assert_eq!(transitioning.state, UnitOrderState::InTransition);
    match transitioning.path.as_ref() {
        Some(UnitPath::Ground(path)) => {
            assert_eq!(path.step_index, 0);
            assert_eq!(path.steps, vec![PathStep::new(0, 0)]);
        }
        other => panic!("expected a truncated ground path, got {other:?}"),
    }

    // No path — nothing to stop.
    let mut idle = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(3));
    idle.orders = UnitOrder::Move;
    idle.state = UnitOrderState::InProgress;
    stop_movement(&mut idle, false);
    assert_eq!(idle.orders, UnitOrder::Move);

    // Wrong order keeps the path.
    let mut awaiting = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(4));
    awaiting.orders = UnitOrder::Await;
    awaiting.state = UnitOrderState::InProgress;
    awaiting.path = path();
    stop_movement(&mut awaiting, false);
    assert!(awaiting.path.is_some());

    // `disable_fire` suppresses the delayed reaction.
    let mut quiet = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(5));
    quiet.orders = UnitOrder::Move;
    quiet.state = UnitOrderState::InProgress;
    quiet.path = path();
    stop_movement(&mut quiet, true);
    assert_eq!(quiet.delayed_reaction, 0);
}

/// `UnitsManager_SetNewOrderInt`: an active `FIRE` (and terminal `EXPLODE`/`DESTROY`)
/// cannot be replaced, `NEW_ALLOCATE` does not become the prior order, and
/// `NEW_ORDER` collapses to `AWAIT` first.
#[test]
fn set_new_order_int_override_boundaries() {
    // A unit that is firing ignores a non-EXPLODE order (`units_manager.cpp:1945`).
    let mut firing = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    firing.orders = UnitOrder::Fire;
    firing.state = UnitOrderState::FireInProgress;
    firing.prior_orders = UnitOrder::Await;
    assert!(!set_new_order_int(
        &mut firing,
        UnitOrder::Move,
        UnitOrderState::Init
    ));
    assert_eq!(firing.orders, UnitOrder::Fire);
    assert_eq!(firing.state, UnitOrderState::FireInProgress);

    // EXPLODE is always accepted but keeps the firing order's prior.
    assert!(!set_new_order_int(
        &mut firing,
        UnitOrder::Explode,
        UnitOrderState::Init
    ));
    assert_eq!(firing.orders, UnitOrder::Explode);

    // Terminal states are frozen.
    let mut exploding = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
    exploding.orders = UnitOrder::Explode;
    exploding.state = UnitOrderState::Destroy;
    assert!(!set_new_order_int(
        &mut exploding,
        UnitOrder::Await,
        UnitOrderState::ExecutingOrder
    ));
    assert_eq!(exploding.orders, UnitOrder::Explode);
    assert_eq!(exploding.state, UnitOrderState::Destroy);

    // `NEW_ALLOCATE` is not recorded as the prior order.
    let mut allocating = UnitInfo::new(UnitType::GoldRefinery, TeamId::RED, UnitId(3));
    allocating.orders = UnitOrder::NewAllocate;
    allocating.state = UnitOrderState::Init;
    allocating.prior_orders = UnitOrder::Await;
    allocating.prior_state = UnitOrderState::ExecutingOrder;
    assert!(!set_new_order_int(
        &mut allocating,
        UnitOrder::Move,
        UnitOrderState::Init
    ));
    assert_eq!(allocating.orders, UnitOrder::Move);
    assert_eq!(
        allocating.prior_orders,
        UnitOrder::Await,
            "NewAllocate kept out of prior"
    );
}

/// `UnitInfo::RestoreOrders` exception list: powered buildings keep a prior
/// equal to the current order; every other type resets it to `AWAIT`.
#[test]
fn restore_orders_building_exception_table() {
    for unit_type in [
        UnitType::GoldRefinery,
        UnitType::MiningStation,
        UnitType::Habitat,
        UnitType::PowerGenerator,
        UnitType::PowerStation,
        UnitType::ResearchCentre,
    ] {
        let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::InProgress;
        unit.prior_orders = UnitOrder::Move;
        unit.prior_state = UnitOrderState::InProgress;
        restore_orders(&mut unit);
        assert_eq!(unit.orders, UnitOrder::Move, "{unit_type:?}");
        assert_eq!(unit.state, UnitOrderState::InProgress, "{unit_type:?}");
    }

    for unit_type in [UnitType::Tank, UnitType::Scout, UnitType::Constructor] {
        let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::InProgress;
        unit.prior_orders = UnitOrder::Move;
        unit.prior_state = UnitOrderState::InProgress;
        restore_orders(&mut unit);
        assert_eq!(unit.orders, UnitOrder::Await, "{unit_type:?}");
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder, "{unit_type:?}");
    }

    // A differing prior is never reset, even for a building.
    let mut building = UnitInfo::new(UnitType::Habitat, TeamId::RED, UnitId(2));
    building.orders = UnitOrder::PowerOff;
    building.state = UnitOrderState::Init;
    building.prior_orders = UnitOrder::PowerOn;
    building.prior_state = UnitOrderState::ExecutingOrder;
    restore_orders(&mut building);
    assert_eq!(building.orders, UnitOrder::PowerOn);
    assert_eq!(building.state, UnitOrderState::ExecutingOrder);
}

/// `UnitInfo::CancelBuilding`: mobile builders return to `AWAIT` and drop the
/// queue/route; a stationary structure switches to the halt-building order.
#[test]
fn cancel_building_variants() {
    let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
    builder.flags = crate::access::unit_type_flags(UnitType::Constructor);
    builder.orders = UnitOrder::Build;
    builder.state = UnitOrderState::BuildInProgress;
    builder.build_list = vec![UnitType::PowerGenerator];
    builder.build_time = 5;
    builder.path = Some(UnitPath::Ground(GroundPath::default()));
    builder.moved = 4;
    cancel_building(&mut builder);
    assert_eq!(builder.orders, UnitOrder::Await);
    assert_eq!(builder.state, UnitOrderState::ExecutingOrder);
    assert!(builder.build_list.is_empty());
    assert_eq!(builder.build_time, 0);
    assert!(builder.path.is_none());
    assert_eq!(builder.moved, 0);

    // Stationary structure, cancel requested.
    let mut structure = UnitInfo::new(UnitType::GoldRefinery, TeamId::RED, UnitId(2));
    structure.flags = crate::access::unit_type_flags(UnitType::GoldRefinery);
    structure.orders = UnitOrder::Build;
    structure.state = UnitOrderState::BuildCancel;
    structure.build_list = vec![UnitType::Tank];
    structure.build_time = 3;
    cancel_building(&mut structure);
    assert_eq!(structure.orders, UnitOrder::HaltBuilding);
    assert_eq!(structure.state, UnitOrderState::ExecutingOrder);
    assert_eq!(
        structure.build_list,
        vec![UnitType::Tank],
            "queue kept until halt"
    );
    assert_eq!(structure.build_time, 3);

    // Stationary structure, abort without a cancel request.
    structure.orders = UnitOrder::Build;
    structure.state = UnitOrderState::BuildInProgress;
    cancel_building(&mut structure);
    assert_eq!(structure.orders, UnitOrder::HaltBuilding2);
    assert_eq!(structure.state, UnitOrderState::ExecutingOrder);

    // Any other stationary order just clears the queue.
    structure.orders = UnitOrder::Await;
    structure.state = UnitOrderState::ExecutingOrder;
    cancel_building(&mut structure);
    assert_eq!(structure.orders, UnitOrder::Await);
    assert!(structure.build_list.is_empty());
    assert_eq!(structure.build_time, 0);
}

/// `UnitInfo::ClearBuildListAndPath`: a parented unit waits for `ACTIVATE`,
/// a free one returns to `AWAIT`; both lose the queue and the path.
#[test]
fn clear_build_list_and_path_variants() {
    let mut parented = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
    parented.parent_unit = Some(UnitId(2));
    parented.path = Some(UnitPath::Ground(GroundPath::default()));
    parented.build_list = vec![UnitType::Tank];
    clear_build_list_and_path(&mut parented);
    assert_eq!(parented.orders, UnitOrder::Build);
    assert_eq!(parented.state, UnitOrderState::UnitReady);
    assert!(parented.path.is_none());
    assert!(parented.build_list.is_empty());

    let mut free = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(3));
    free.path = Some(UnitPath::Ground(GroundPath::default()));
    free.build_list = vec![UnitType::Tank];
    clear_build_list_and_path(&mut free);
    assert_eq!(free.orders, UnitOrder::Await);
    assert_eq!(free.state, UnitOrderState::ExecutingOrder);
    assert!(free.path.is_none());
    assert!(free.build_list.is_empty());
}

/// `active_build_site` returns the site only for a mobile builder with a
/// running construction and a non-empty queue; terminal states hide it.
#[test]
fn active_build_site_boundaries() {
    let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
    builder.flags = crate::access::unit_type_flags(UnitType::Constructor);
    builder.orders = UnitOrder::Build;
    builder.state = UnitOrderState::BuildInProgress;
    builder.build_list = vec![UnitType::PowerGenerator];
    builder.target_grid = Point::new(7, 8);
    assert_eq!(active_build_site(&builder), Some(Point::new(7, 8)));

    builder.orders = UnitOrder::AwaitTapePositioning;
    builder.state = UnitOrderState::TapePositioningEnter;
    assert_eq!(active_build_site(&builder), Some(Point::new(7, 8)));

    for state in [
        UnitOrderState::SelectSite,
        UnitOrderState::UnitReady,
        UnitOrderState::BuildCancel,
        UnitOrderState::BuildAbort,
    ] {
        builder.state = state;
        assert_eq!(active_build_site(&builder), None, "{state:?}");
    }

    builder.state = UnitOrderState::BuildInProgress;
    builder.build_list.clear();
    assert_eq!(active_build_site(&builder), None, "empty queue");

    builder.build_list = vec![UnitType::PowerGenerator];
    builder.orders = UnitOrder::Await;
    assert_eq!(active_build_site(&builder), None, "not a build order");

    let tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
    assert_eq!(active_build_site(&tank), None, "not a mobile builder");
}

/// `Paths_CalculateStep` cost classes: `< 4` doubles `max_velocity`, `> 4`
/// halves it; the diagonal costs `cost * 3 / 2`; `move_fraction` carries the
/// remainder and `shots` are reduced only for `MOVE_AND_FIRE == 0`.
#[test]
fn paths_calculate_step_cost_classes_and_fraction() {
    fn tank() -> UnitInfo {
        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.base_values.set_attribute(ATTRIB_SPEED, 6);
        unit.base_values.set_attribute(ATTRIB_ROUNDS, 2);
        unit.base_values.set_attribute(ATTRIB_MOVE_AND_FIRE, 0);
        unit.shots = 2;
        unit.speed = 12;
        unit.move_fraction = 0;
        unit
    }

    // cost 2: max_velocity 8 → 16, normalized 1, remainder 2.
    let mut cheap = tank();
    assert!(paths_calculate_step(&mut cheap, 2, false, false, false));
    assert_eq!(cheap.max_velocity, 16);
    assert_eq!(cheap.speed, 11);
    assert_eq!(cheap.move_fraction, 2);
    assert_eq!(cheap.shots, 2, "2*11/6 = 3 is not below 2");

    // The carried fraction is consumed by the next step of the same cost:
    // the whole cost is covered by the fraction, so no speed is spent.
    assert!(paths_calculate_step(&mut cheap, 2, false, false, false));
    assert_eq!(cheap.move_fraction, 0);
    assert_eq!(cheap.speed, 11);

    // cost 4: max_velocity 8, normalized 1, no remainder.
    let mut normal = tank();
    assert!(paths_calculate_step(&mut normal, 4, false, false, false));
    assert_eq!(normal.max_velocity, 8);
    assert_eq!(normal.speed, 11);
    assert_eq!(normal.move_fraction, 0);

    // cost 6: max_velocity 4, normalized 2, remainder 2.
    let mut expensive = tank();
    assert!(paths_calculate_step(&mut expensive, 6, false, false, false));
    assert_eq!(expensive.max_velocity, 4);
    assert_eq!(expensive.speed, 10);
    assert_eq!(expensive.move_fraction, 2);

    // Diagonal: cost 4 is charged as 6, but the velocity class follows the
    // raw cost argument.
    let mut diagonal = tank();
    assert!(paths_calculate_step(&mut diagonal, 4, true, false, false));
    assert_eq!(diagonal.max_velocity, 8);
    assert_eq!(diagonal.speed, 10);
    assert_eq!(diagonal.move_fraction, 2);

    // `MOVE_AND_FIRE` units keep their shot count; the others recompute it.
    let mut fire_on_move = tank();
    fire_on_move
        .base_values
        .set_attribute(ATTRIB_MOVE_AND_FIRE, 1);
    fire_on_move.speed = 1;
    assert!(paths_calculate_step(
        &mut fire_on_move,
        4,
        false,
        false,
        false
    ));
    assert_eq!(fire_on_move.speed, 0);
    assert_eq!(fire_on_move.shots, 2);

    let mut stationary_fire = tank();
    stationary_fire.speed = 1;
    assert!(paths_calculate_step(
        &mut stationary_fire,
        4,
        false,
        false,
        false
    ));
    assert_eq!(stationary_fire.shots, 0, "2*0/6 = 0");

    // Not enough speed: the step does not start, the counter is charged into
    // `move_fraction`, the group speed collapses to 1.
    let mut slow = tank();
    slow.speed = 1;
    slow.group_speed = 5;
    assert!(!paths_calculate_step(&mut slow, 6, false, false, false));
    assert_eq!(slow.speed, 0);
    assert_eq!(slow.move_fraction, 4);
    assert_eq!(slow.group_speed, 1);

    // Infantry halves `max_velocity` and records `velocity`.
    let mut infantry = UnitInfo::new(UnitType::Infantry, TeamId::RED, UnitId(2));
    infantry.speed = 4;
    infantry.move_fraction = 0;
    assert!(paths_calculate_step(&mut infantry, 4, false, false, false));
    assert_eq!(infantry.max_velocity, 4);
    assert_eq!(infantry.velocity, 4);
}

/// `Paths_TakeStep` (`airpath.cpp:458`): one `speed` point per air cell, the
/// group speed never drops below 1, shots follow the remaining speed for
/// `MOVE_AND_FIRE == 0`, and `GameManager_RealTime` makes the step free.
#[test]
fn paths_take_step_charges_air_movement() {
    fn fighter() -> UnitInfo {
        let mut unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(1));
        unit.base_values.set_attribute(ATTRIB_SPEED, 24);
        unit.base_values.set_attribute(ATTRIB_ROUNDS, 2);
        unit.base_values.set_attribute(ATTRIB_MOVE_AND_FIRE, 1);
        unit.speed = 3;
        unit.shots = 2;
        unit
    }

    // One point per cell: after three cells the aircraft has no speed left.
    let mut unit = fighter();
    for expected in [2, 1, 0] {
        paths_take_step(&mut unit, 1, false);
        assert_eq!(unit.speed, expected);
    }

    // `MOVE_AND_FIRE` units keep their rounds.
    assert_eq!(unit.shots, 2);

    // The group speed is floored at 1 (`airpath.cpp:465-473`).
    let mut group = fighter();
    group.group_speed = 1;
    paths_take_step(&mut group, 1, false);
    assert_eq!(group.group_speed, 1, "group speed never reaches 0");

    // `MOVE_AND_FIRE == 0`: `shots = rounds * speed / speed_attr` only ever
    // decreases (`airpath.cpp:477-484`).
    let mut ground_attack = fighter();
    ground_attack
        .base_values
        .set_attribute(ATTRIB_MOVE_AND_FIRE, 0);
    ground_attack.base_values.set_attribute(ATTRIB_SPEED, 4);
    ground_attack.base_values.set_attribute(ATTRIB_ROUNDS, 4);
    ground_attack.speed = 4;
    ground_attack.shots = 4;
    paths_take_step(&mut ground_attack, 2, false);
    assert_eq!(ground_attack.speed, 2);
    assert_eq!(ground_attack.shots, 2, "4 * 2 / 4");

    // `GameManager_RealTime` (debug): the step costs nothing.
    let mut real_time = fighter();
    real_time.group_speed = 5;
    paths_take_step(&mut real_time, 2, true);
    assert_eq!(real_time.speed, 3, "speed kept");
    assert_eq!(real_time.group_speed, 5, "group speed kept");
}

/// Invariant C5 (concrete scenario): an air unit spends one `speed` per path
/// step; the step is free only under the real-time debug setting.
///
/// Oracle: `max-cpp-port/src/airpath.cpp:458-484` (`Paths_TakeStep`, the
/// `unit->speed -= cost` at `:463` and the `shots` recalculation).
#[test]
fn invariant_c5_air_step_spends_speed() {
    fn aircraft() -> UnitInfo {
        let mut unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(1));
        unit.base_values.set_attribute(ATTRIB_SPEED, 8);
        unit.base_values.set_attribute(ATTRIB_ROUNDS, 4);
        unit.base_values.set_attribute(ATTRIB_MOVE_AND_FIRE, 0);
        unit.speed = 8;
        unit.shots = 4;
        unit
    }

    for cost in 1..=8i32 {
        let mut unit = aircraft();
        paths_take_step(&mut unit, cost, false);

        assert_eq!(
            unit.speed,
            (8 - cost) as u16,
                "C5: an air step of {cost} must spend {cost} speed"
        );

        // `shots = rounds * speed / ATTRIB_SPEED`, only decreasing.
        let expected_shots = ((4 * (8 - cost)) / 8).max(0) as u8;
        assert_eq!(
            unit.shots, expected_shots,
                "C5: shots must follow the remaining speed (cost {cost})"
        );
    }

    // The exact-speed boundary lands on zero without wrapping.
    let mut exact = aircraft();
    paths_take_step(&mut exact, 8, false);
    assert_eq!(exact.speed, 0, "C5: spending the exact speed leaves 0");
    assert_eq!(exact.shots, 0, "C5: no speed left means no shots");

    // `GameManager_RealTime`: the step costs nothing.
    let mut real_time = aircraft();
    paths_take_step(&mut real_time, 5, true);
    assert_eq!(real_time.speed, 8, "C5: real-time air movement is free");
    assert_eq!(real_time.shots, 4, "C5: real-time movement keeps shots");
}

/// `UnitInfo::Redraw` (`unitinfo.cpp:1452-1459`): only a hovering aircraft has
/// a non-zero shadow shift, and it is exactly one cell (`GFX_MAP_TILE_SIZE`).
#[test]
fn air_shadow_offset_follows_the_hovering_flag() {
    assert_eq!(air_shadow_offset(0), Point::ZERO);
    assert_eq!(
        air_shadow_offset(MOBILE_AIR_UNIT),
        Point::ZERO,
            "an aircraft on the ground has no shift"
    );
    assert_eq!(
        air_shadow_offset(MOBILE_AIR_UNIT | HOVERING),
        Point::new(-GFX_MAP_TILE_SIZE as i16, -GFX_MAP_TILE_SIZE as i16)
    );
    assert_eq!(
        air_shadow_offset(HOVERING | MOBILE_LAND_UNIT),
        Point::ZERO,
            "a non-air unit never gets the air shadow"
    );
}

/// `UnitInfo::Land`/`Take` animation counters are 6-bit and saturate at the
/// 9th/5th call; `HOVERING` flips exactly on completion.
#[test]
fn land_and_take_boundaries_wrap() {
    let mut unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(1));
    unit.moved = 8;
    assert!(land(&mut unit));
    assert_eq!(unit.moved, 9);
    assert_eq!(unit.flags & HOVERING, 0, "landing clears HOVERING");

    unit.moved = 0x3F;
    assert!(!land(&mut unit), "0x3F wraps to 0: animation restarts");
    assert_eq!(unit.moved, 0);

    let mut unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(2));
    unit.moved = 3;
    assert!(!take(&mut unit));
    assert_eq!(unit.moved, 4);
    assert!(take(&mut unit));
    assert_eq!(unit.moved, 5);
    assert_ne!(unit.flags & HOVERING, 0, "takeoff sets HOVERING");

    unit.moved = 0x3F;
    assert!(!take(&mut unit));
    assert_eq!(unit.moved, 0);
}

/// `is_bridge_elevated`: only the bridge type, only while its frame differs
/// from the base frame (`UnitInfo::IsBridgeElevated`).
#[test]
fn bridge_elevation_boundaries() {
    let mut bridge = UnitInfo::new(UnitType::Bridge, TeamId::RED, UnitId(1));
    bridge.image_base = 10;
    bridge.image_index = 10;
    assert!(!is_bridge_elevated(&bridge));
    bridge.image_index = 11;
    assert!(is_bridge_elevated(&bridge));

    let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
    tank.image_base = 0;
    tank.image_index = 1;
    assert!(!is_bridge_elevated(&tank));
}

/// `Paths_UpdateAngle` rotates one 45° step the short way, wraps 7↔0 and
/// rotates the turret with the hull.
#[test]
fn paths_update_angle_wraps_between_7_and_0() {
    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    unit.flags = crate::access::unit_type_flags(UnitType::Tank);
    unit.angle = 7;
    unit.turret_angle = 7;
    assert!(paths_update_angle(&mut unit, 0));
    assert_eq!(unit.angle, 0);
    assert_eq!(unit.turret_angle, 0);

    assert!(
        !paths_update_angle(&mut unit, 0),
            "already facing the target"
    );

    unit.angle = 0;
    unit.turret_angle = 0;
    assert!(paths_update_angle(&mut unit, 7));
    assert_eq!(unit.angle, 7);
    assert_eq!(unit.turret_angle, 7);
}

/// `UnitInfo::PowerUp`/`PowerDown` leave the unit in `EXECUTING_ORDER`.
#[test]
fn power_up_and_down_set_executing_order() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);

    let mut unit = UnitInfo::new(UnitType::PowerGenerator, TeamId::RED, UnitId(1));
    unit.state = UnitOrderState::Init;
    power_up(&mut unit, 1);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);

    unit.state = UnitOrderState::Init;
    power_down(&mut state, &mut unit);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
}
