//! Phase 2 integration: `unit_order.rs` — order → effect → next state.
//!
//! Oracle: `max-cpp-port/src/units_manager.cpp` (`UnitsManager_ProcessOrder*`)
//! and `unitinfo.cpp` (`RestoreOrders`, `Land`, `Take`, `StopMovement`).
//!
//! Each case is "given order/state → one tick → expected order/state"; the
//! death/landing cycles and the D checksums close the loop over multiple ticks.

mod support;

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::unit_order;
use max_core::universe::GameUniverse;

use support::{game, order_move, run_until, set_hovering, spawn, ticks};

/// One `ProcessOrder*` transition: `given (order, state)` after a tick the unit
/// must report the expected `(order, state)` (oracle mapping of the C++ switch).
#[test]
fn order_dispatch_effects_match_cpp() {
    // (issued order, from state, expected order, expected state)
    let cases = [
        (
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        ),
        (
            UnitOrder::Sentry,
            UnitOrderState::ExecutingOrder,
            UnitOrder::Sentry,
            UnitOrderState::ExecutingOrder,
        ),
        (
            UnitOrder::Disable,
            UnitOrderState::Init,
            UnitOrder::Disable,
            UnitOrderState::Init,
        ),
        (
            UnitOrder::Idle,
            UnitOrderState::Init,
            UnitOrder::Idle,
            UnitOrderState::Init,
        ),
        (
            UnitOrder::NewAllocate,
            UnitOrderState::Init,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        ),
        (
            UnitOrder::Repair,
            UnitOrderState::Init,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        ),
        (
            UnitOrder::Upgrade,
            UnitOrderState::Init,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        ),
        (
            UnitOrder::PowerOn,
            UnitOrderState::Init,
            UnitOrder::PowerOn,
            UnitOrderState::ExecutingOrder,
        ),
        (
            UnitOrder::PowerOff,
            UnitOrderState::Init,
            UnitOrder::PowerOff,
            UnitOrderState::ExecutingOrder,
        ),
        (
            UnitOrder::Transform,
            UnitOrderState::Init,
            UnitOrder::AwaitTapePositioning,
            UnitOrderState::TapePositioningInit,
        ),
        (
            UnitOrder::Clear,
            UnitOrderState::Init,
            UnitOrder::Clear,
            UnitOrderState::InProgress,
        ),
    ];

    for (order, from_state, expected_order, expected_state) in cases {
        let mut state = game();
        let unit = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
        // The order data is irrelevant for these processors; set it for safety.
        state.set_order_data(
            unit,
            max_core::game::OrderData {
                move_to: Point::new(12, 10),
                fire_on: Point::new(12, 10),
            },
        );
        state.set_order(unit, order, from_state);
        state.tick();

        let unit = state.snapshot().unit(unit).expect("unit survives the tick");
        assert_eq!(
            (unit.orders, unit.state),
            (expected_order, expected_state),
            "{order:?} from {from_state:?}"
        );
    }
}

/// `UPGRADE` drops the parent link; `NEW_ALLOCATE`/`REPAIR` restore the deferred
/// order (the state did not change for an unpowered tank).
#[test]
fn upgrade_and_deferred_orders_keep_prior_state() {
    let mut state = game();
    let unit = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    let parent = spawn(
        &mut state,
        UnitType::SupplyTruck,
        TeamId::RED,
        Point::new(10, 11),
    );
    state.unit_mut(unit).unwrap().parent_unit = Some(parent);
    state.set_order(unit, UnitOrder::Upgrade, UnitOrderState::Init);
    state.tick();
    assert!(state.snapshot().unit(unit).unwrap().parent_unit.is_none());
    assert_eq!(
        state.snapshot().unit(unit).unwrap().orders,
        UnitOrder::Await
    );
}

/// A lethal hit puts the target into `EXPLODE/INIT`; the effect unit appears on
/// the next tick and both the corpse and the explosion are removed after the
/// animation (`UnitsManager_StartExplosion`/`ProgressExplosion`).
#[test]
fn explode_lifecycle_spawns_and_despawns_the_effect() {
    fn run() -> GameUniverse {
        let mut state = game();
        let victim = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(5, 5));
        let attacker = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(6, 5));

        state.unit_mut(victim).unwrap().hits = 1;
        state.attack_unit(victim, attacker, 0, 2);
        {
            let victim_unit = state.snapshot().unit(victim).expect("victim");
            assert_eq!(victim_unit.hits, 0);
            assert_eq!(victim_unit.orders, UnitOrder::Explode);
            assert_eq!(victim_unit.state, UnitOrderState::Init);
        }

        // First tick: the explosion effect is deployed, the victim waits in DESTROY.
        state.tick();
        assert_eq!(
            state.snapshot().unit(victim).unwrap().state,
            UnitOrderState::Destroy
        );
        let explosion = state
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                state
                    .snapshot()
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::LandExplosion)
            })
            .expect("land explosion on the victim cell");
        assert_eq!(
            state.snapshot().unit(explosion).unwrap().grid,
            Point::new(5, 5)
        );

        // Frames advance while the effect lives.
        let mut frames = Vec::new();
        // The animation lasts `EXPLOSION_FRAME_STEPS × image_index_max + 1` steps
        // (5 × 13 + 1 = 66, `unit_order::EXPLOSION_FRAME_STEPS`), so the window
        // must cover it.
        for _ in 0..80 {
            state.tick();
            if let Some(effect) = state.snapshot().unit(explosion) {
                frames.push(effect.image_index);
            }
        }

        assert!(frames.len() >= 4, "the explosion animated: {frames:?}");
        assert!(
            frames.windows(2).all(|pair| pair[0] <= pair[1]),
            "no frame rewind"
        );
        assert!(
            !state.snapshot().has_unit(explosion),
            "the effect is removed"
        );
        assert!(!state.snapshot().has_unit(victim), "the corpse is removed");
        state
    }

    let first = run();
    let second = run();
    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes()
    );
    assert_eq!(
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
}

/// `LAND` → `TAKE_OFF` round trip: nine landing steps clear `HOVERING`, five
/// takeoff steps set it again, both restore the deferred `AWAIT` order.
#[test]
fn land_and_takeoff_cycle_restores_await() {
    let mut state = game();
    let fighter = spawn(
        &mut state,
        UnitType::Fighter,
        TeamId::RED,
        Point::new(10, 10),
    );
    set_hovering(&mut state, fighter, true);

    state.set_order(fighter, UnitOrder::Land, UnitOrderState::Init);
    ticks(&mut state, 12);
    {
        let unit = state.snapshot().unit(fighter).expect("fighter");
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert_eq!(
            unit.flags & max_core::enums::unit_flags::HOVERING,
            0,
            "landing clears HOVERING"
        );
        assert_eq!(unit.moved, 9, "nine landing frames");
    }

    state.set_order(fighter, UnitOrder::TakeOff, UnitOrderState::Init);
    ticks(&mut state, 7);
    {
        let unit = state.snapshot().unit(fighter).expect("fighter");
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_ne!(
            unit.flags & max_core::enums::unit_flags::HOVERING,
            0,
            "takeoff sets HOVERING"
        );
        assert_eq!(unit.moved, 5, "five takeoff frames");
    }
}

/// `MOVE` over multiple ticks: request (`NEW_ORDER`) → path (`IN_PROGRESS`) →
/// one cell per tick; `STOP_MOVEMENT` mid-route blocks the order and drops the
/// path (`GroundPath::CancelMovement`).
#[test]
fn move_request_path_and_emergency_stop() {
    let mut state = game();
    let unit = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    order_move(&mut state, unit, Point::new(20, 10));

    // Tick 1: the path request is queued, the order waits for it.
    state.tick();
    assert_eq!(
        state.snapshot().unit(unit).unwrap().state,
        UnitOrderState::NewOrder
    );
    assert_eq!(state.snapshot().path_requests().len(), 1);

    // Tick 2: the path is applied; the hull turns toward the next step before
    // the first cell transition (`GroundPath::Execute` + `Move`).
    state.tick();
    assert!(state.snapshot().unit(unit).unwrap().path.is_some());
    assert_eq!(
        state.snapshot().unit(unit).unwrap().state,
        UnitOrderState::InProgress
    );

    assert!(
        run_until(&mut state, 8, |state| {
            state
                .snapshot()
                .unit(unit)
                .is_some_and(|unit| unit.grid == Point::new(11, 10))
        }),
        "the tank did not traverse the first cell"
    );

    // Emergency stop in the middle of the route.
    let unit_ref = state.unit_mut(unit).expect("unit");
    unit_order::stop_movement(unit_ref, false);
    {
        let unit = state.snapshot().unit(unit).expect("unit");
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert!(unit.path.is_none());
        assert_eq!(unit.delayed_reaction, 1);
    }

    // Nothing moves after the stop.
    let grid = state.snapshot().unit(unit).unwrap().grid;
    ticks(&mut state, 5);
    assert_eq!(state.snapshot().unit(unit).unwrap().grid, grid);
}

/// Determinism of the whole order cycle (land → takeoff → move) on two runs.
#[test]
fn order_cycle_is_bit_identical() {
    fn run() -> (Vec<u8>, u16, Vec<u8>) {
        let mut state = game();
        let fighter = spawn(
            &mut state,
            UnitType::Fighter,
            TeamId::RED,
            Point::new(10, 10),
        );
        set_hovering(&mut state, fighter, true);
        state.set_order(fighter, UnitOrder::Land, UnitOrderState::Init);
        ticks(&mut state, 12);
        state.set_order(fighter, UnitOrder::TakeOff, UnitOrderState::Init);
        ticks(&mut state, 7);
        order_move(&mut state, fighter, Point::new(15, 10));
        assert!(run_until(&mut state, 64, |state| {
            state
                .snapshot()
                .unit(fighter)
                .is_some_and(|unit| unit.grid == Point::new(15, 10))
        }));

        (
            state.snapshot().snapshot_bytes(),
            state.snapshot().state_checksum(),
            state.snapshot().replay_digest_bytes(),
        )
    }

    assert_eq!(run(), run());
}
