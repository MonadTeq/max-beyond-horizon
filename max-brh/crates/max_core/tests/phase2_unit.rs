//! Phase 2 integration: `unit.rs` — unit state over the tick loop.
//!
//! Oracle: `max-cpp-port/src/unitinfo.cpp` (`UnitInfo::UnitInfo`, `ExecuteMove`,
//! `Move`, `PrepareFire`, `Reload`), `units_manager.cpp`
//! (`UnitsManager_ProcessOrderReload`, `UnitsManager_DeployUnit`).
//!
//! Each scenario is "given → order → ticks → expected state"; deterministic runs
//! also compare `state_checksum`/`snapshot_bytes`/`replay_digest` (D).

mod support;

use max_core::crc;
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;

use support::{game, order_fire, order_move, run_until, spawn, ticks};

const TANK_HITS: u16 = 24;
const TANK_AMMO: u8 = 14;
const TANK_SPEED: u16 = 6;

/// Given a tank and a `MOVE` endpoint, ticks move it exactly one cell per tick,
/// spend one speed point per cell and restore `AWAIT` on arrival. Two identical
/// runs are bit-identical (D).
#[test]
fn move_spends_speed_and_restores_await_deterministically() {
    fn run() -> max_core::GameState {
        let mut state = game();
        let unit = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
        order_move(&mut state, unit, Point::new(14, 10));

        assert!(
            run_until(&mut state, 64, |state| {
                state
                    .unit(unit)
                    .is_some_and(|unit| unit.grid == Point::new(14, 10) && !unit.has_move_order())
            }),
            "tank did not reach the target in 64 ticks"
        );

        let unit = state.unit(unit).expect("unit");
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert_eq!(
            unit.speed,
            TANK_SPEED - 4,
            "4 cells cost 1 speed point each"
        );
        assert_eq!(unit.moved, 0);
        assert!(unit.path.is_none(), "the route is dropped on arrival");
        assert_eq!(unit.ammo, TANK_AMMO, "movement does not consume ammo");
        state
    }

    let first = run();
    let second = run();

    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(crc::state_checksum(&first), crc::state_checksum(&second));
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
    assert_ne!(
        crc::state_checksum(&first),
        crc::state_checksum(&game()),
        "the scenario must change the state CRC"
    );
}

/// Moving recomputes the shot allowance from the remaining speed
/// (`Paths_CalculateStep`, `ATTRIB_ROUNDS * speed / ATTRIB_SPEED`) for units
/// without `MOVE_AND_FIRE`.
#[test]
fn movement_readiness_recomputes_shots() {
    let mut state = game();
    let unit = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    assert_eq!(state.unit(unit).unwrap().shots, 2, "TANK rounds");

    order_move(&mut state, unit, Point::new(14, 10));
    assert!(run_until(&mut state, 64, |state| {
        state
            .unit(unit)
            .is_some_and(|unit| unit.grid == Point::new(14, 10) && !unit.has_move_order())
    }));

    let unit = state.unit(unit).expect("unit");
    assert_eq!(unit.speed, 2);
    assert_eq!(
        unit.shots, 0,
        "2 * remaining speed 2 / base speed 6 = 0 shots"
    );
    assert_eq!(unit.moved, 0);
}

/// Given a tank and a visible target, `FIRE` runs AIM → recoil → shot, deducts
/// ammo and one shot, and deals `attack - armor` damage. Then a supply truck's
/// `RELOAD` refills the tank's magazine from its cargo (`UnitInfo::Reload`).
#[test]
fn fire_consumes_ammo_and_reload_restores_it() {
    fn run() -> max_core::GameState {
        let mut state = game();
        let shooter = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
        let target = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(13, 10),
        );

        order_fire(&mut state, shooter, Point::new(13, 10));
        assert!(
            run_until(&mut state, 40, |state| {
                state.unit(target).is_some_and(|unit| unit.hits < TANK_HITS)
            }),
            "the shot never happened"
        );

        let shooter_unit = state.unit(shooter).expect("shooter");
        assert_eq!(shooter_unit.ammo, TANK_AMMO - 1);
        assert_eq!(shooter_unit.shots, 1, "one of two rounds spent");
        assert_eq!(shooter_unit.live_attack, 0, "the attack is finished");
        assert_eq!(
            state.unit(target).unwrap().hits,
            TANK_HITS - 6,
            "TANK attack 16 - TANK armor 10"
        );

        // The truck carries cargo and points at the unit it services.
        let truck = spawn(
            &mut state,
            UnitType::SupplyTruck,
            TeamId::RED,
            Point::new(10, 11),
        );
        state.unit_mut(truck).unwrap().storage = 5;
        state.unit_mut(truck).unwrap().parent_unit = Some(shooter);
        state.unit_mut(shooter).unwrap().ammo = 3;

        // `UnitsManager_ProcessOrderReload`: spend one cargo unit, refill ammo.
        state.set_order(truck, UnitOrder::Reload, UnitOrderState::Init);
        ticks(&mut state, 2);

        assert_eq!(state.unit(shooter).unwrap().ammo, TANK_AMMO);
        assert_eq!(state.unit(truck).unwrap().storage, 4);
        assert_eq!(state.unit(truck).unwrap().orders, UnitOrder::Await);
        state
    }

    let first = run();
    let second = run();

    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(crc::state_checksum(&first), crc::state_checksum(&second));
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
}
