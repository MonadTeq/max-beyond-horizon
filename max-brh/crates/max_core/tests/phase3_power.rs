//! Phase 3 (economy): complex power — surplus, deficit and the shutdown order.
//!
//! Oracle (`max-cpp-port/src`):
//! * `cargo.cpp:183-233` — `Cargo_GetFuelConsumptionRate` (POWERSTN 6, POWGEN 2)
//!   and `Cargo_GetPowerConsumptionRate` (POWERSTN −6, POWGEN −1, consumers 1);
//! * `production_manager.cpp:481-562` — `OptimizePowerProduction`/
//!   `OptimizePowerConsumption` (producers follow the balance and the fuel reserve);
//! * `production_manager.cpp:642-664` — `CheckPowerNeed`;
//! * `production_manager.cpp:677-790` — `ValidateIndustry` (construction pause) and
//!   `OptimizeAuxilaryIndustry`/`OptimizeMiningIndustry` — the consumer order
//!   `RESEARCH → GREENHSE → COMMTWR → constructions → MININGST`;
//! * `production_manager.cpp:366-380` — `UpdateUnitLifeConsumption` (HABITAT);
//! * `production_manager.cpp:308-330/792-804` — the industry message text.
//!
//! Port contract (documented in `game-mechanics.md` §3/§5): a running construction
//! **pauses** instead of C++ `ORDER_STATE_BUILD_ABORT` and no resource ever goes
//! negative; the observed balance and the consumer order stay C++-exact.

use max_core::crc;
use max_core::enums::{TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::power::{complex_totals, optimize_complex, EnergyTotals};
use max_core::production::{self, ProductionManager};
use max_core::team::{ComplexId, TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

fn state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(state: &mut GameUniverse, unit_type: UnitType, grid: Point) -> UnitId {
    state
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .expect("spawn")
}

/// A connector band at `y = 21`, `x = 20..=40`. Every building is placed at
/// `y = 22` and touches the band from the north, so buildings of any footprint
/// (1×1 or 2×2) end up in one complex — no manual complex surgery.
fn connector_band(state: &mut GameUniverse) {
    for x in 20..=40 {
        spawn(state, UnitType::Connector, Point::new(x, 21));
    }
}

fn building(state: &mut GameUniverse, unit_type: UnitType, x: i16) -> UnitId {
    spawn(state, unit_type, Point::new(x, 22))
}

fn power_on(state: &mut GameUniverse, id: UnitId) {
    // `GameUniverse::set_order` returns the `delete_path` flag, not success
    // (C++ `UnitsManager_SetNewOrder` always applies the order).
    let _ = state.set_order(id, UnitOrder::PowerOn, UnitOrderState::ExecutingOrder);
}

fn complex_of(state: &GameUniverse, id: UnitId) -> ComplexId {
    state
        .snapshot()
        .unit(id)
        .expect("unit")
        .complex
        .expect("complex")
}

// ---------------------------------------------------------------------------
// I — deficit, shutdown order, surplus
// ---------------------------------------------------------------------------

#[test]
fn deficit_shuts_down_research_then_greenhouse_then_comm_tower() {
    let mut state = state();
    connector_band(&mut state);

    let mine = building(&mut state, UnitType::MiningStation, 20);
    let research = building(&mut state, UnitType::ResearchCentre, 22);
    let greenhouse = building(&mut state, UnitType::EcoSphere, 24);
    let comm = building(&mut state, UnitType::GoldRefinery, 26);
    let generator = building(&mut state, UnitType::PowerGenerator, 28);
    let fuel = building(&mut state, UnitType::FuelStorage, 30);
    state.unit_mut(fuel).expect("fuel").storage = 50;

    let complex = complex_of(&state, mine);
    for id in [research, greenhouse, comm, generator, fuel] {
        assert_eq!(
            state.snapshot().unit(id).expect("member").complex,
            Some(complex)
        );
    }

    power_on(&mut state, mine);
    power_on(&mut state, research);
    power_on(&mut state, greenhouse);
    power_on(&mut state, comm);

    // Demand 4 (mine/RESEARCH/GREENHSE/COMMTWR); the POWGEN is still off.
    let before = complex_totals(state.state(), TeamId::RED, complex);
    assert_eq!(before.power, -4, "four consumers, producer off");

    let outcome = optimize_complex(state.state_mut(), TeamId::RED, complex, None, None, true);

    assert_eq!(
        outcome.powered_off,
        vec![research, greenhouse, comm],
        "C++ order: RESEARCH → GREENHSE → COMMTWR"
    );
    assert_eq!(
        state.snapshot().unit(mine).expect("mine").orders,
        UnitOrder::PowerOn,
        "MININGST is the last consumer C++ switches off"
    );
    assert_eq!(
        state.snapshot().unit(generator).expect("gen").orders,
        UnitOrder::PowerOn
    );
    assert!(!outcome.unresolved, "{outcome:?}");
    let message = outcome.message.expect("industry message");
    assert!(message.starts_with("Adjustments made:"), "{message:?}");
    assert!(
        message.contains("More power needed, Research Center turned off."),
        "{message:?}"
    );

    // Fuel is gone: the generator cannot run, and the last consumer (the mine)
    // is switched off too.
    state.unit_mut(fuel).expect("fuel").storage = 0;
    let outcome = optimize_complex(state.state_mut(), TeamId::RED, complex, None, None, true);

    assert_eq!(
        state.snapshot().unit(generator).expect("gen").orders,
        UnitOrder::PowerOff
    );
    assert_eq!(
        state.snapshot().unit(mine).expect("mine").orders,
        UnitOrder::PowerOff
    );
    assert_eq!(
        outcome.powered_off,
        vec![generator, mine],
        "the running generator is switched off first, then the consumer"
    );
    let message = outcome.message.expect("message");
    assert!(
        message.contains("More power needed, Power Generator turned off."),
        "{message:?}"
    );
    assert!(
        message.contains("More power needed, Mining Station turned off."),
        "{message:?}"
    );
}

#[test]
fn surplus_power_switches_producers_between_stations_and_generators() {
    let mut state = state();
    connector_band(&mut state);

    let mine = building(&mut state, UnitType::MiningStation, 20);
    let station = building(&mut state, UnitType::PowerStation, 22);
    let generator = building(&mut state, UnitType::PowerGenerator, 24);
    let fuel = building(&mut state, UnitType::FuelStorage, 26);
    state.unit_mut(fuel).expect("fuel").storage = 50;
    power_on(&mut state, mine);

    let complex = complex_of(&state, mine);

    // Demand 1: a POWGEN (+1 for 2 fuel) is enough, the POWERSTN (+6 for 6) is not
    // started.
    let outcome = optimize_complex(state.state_mut(), TeamId::RED, complex, None, None, false);
    assert!(!outcome.unresolved, "{outcome:?}");
    assert_eq!(
        state.snapshot().unit(generator).expect("gen").orders,
        UnitOrder::PowerOn
    );
    assert_eq!(
        state.snapshot().unit(station).expect("station").orders,
        UnitOrder::PowerOff
    );

    // Add five more mines: demand 6 — the station covers it alone and the
    // generator is switched off again.
    for x in [28, 30, 32, 34, 36] {
        let extra = building(&mut state, UnitType::MiningStation, x);
        assert_eq!(
            state.snapshot().unit(extra).expect("mine").complex,
            Some(complex)
        );
        power_on(&mut state, extra);
    }

    let outcome = optimize_complex(state.state_mut(), TeamId::RED, complex, None, None, false);
    assert!(!outcome.unresolved, "{outcome:?}");
    assert_eq!(
        state.snapshot().unit(station).expect("station").orders,
        UnitOrder::PowerOn,
        "one POWERSTN gives the 6 units of demand"
    );
    assert_eq!(
        state.snapshot().unit(generator).expect("gen").orders,
        UnitOrder::PowerOff,
        "the surplus generator is switched off"
    );
}

#[test]
fn a_running_construction_pauses_before_the_mine_is_switched_off() {
    let mut state = state();
    connector_band(&mut state);

    let mine = building(&mut state, UnitType::MiningStation, 20);
    let factory = building(&mut state, UnitType::TrainingHall, 22);
    let dump = building(&mut state, UnitType::MaterialStorage, 24);
    let generator = building(&mut state, UnitType::PowerGenerator, 26);
    let fuel = building(&mut state, UnitType::FuelStorage, 28);
    let habitat = building(&mut state, UnitType::Habitat, 30);

    state.unit_mut(dump).expect("ADUMP").storage = 20;
    state.unit_mut(fuel).expect("FDUMP").storage = 50;
    let complex = complex_of(&state, mine);
    for id in [factory, dump, generator, fuel, habitat] {
        assert_eq!(
            state.snapshot().unit(id).expect("member").complex,
            Some(complex),
            "member {id:?} must be in the complex"
        );
    }

    // `DeployUnit` puts a mine into `POWER_ON`; switch it off for the commit so the
    // first turn of the construction is affordable with one generator.
    let _ = state.set_order(mine, UnitOrder::PowerOff, UnitOrderState::ExecutingOrder);
    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager
        .commit(factory)
        .expect("raw, power and workers are enough for the first turn");
    drop(manager);

    power_on(&mut state, mine);
    assert_eq!(
        state.snapshot().unit(habitat).expect("habitat").orders,
        UnitOrder::PowerOn,
        "HABITAT supplies the factory workers"
    );

    let outcome = optimize_complex(state.state_mut(), TeamId::RED, complex, None, None, true);

    assert!(
        outcome.paused.iter().any(|(id, _)| *id == factory),
        "construction pauses first ({outcome:?})"
    );
    assert!(
        state
            .snapshot()
            .unit(factory)
            .expect("factory")
            .build_paused
    );
    assert_eq!(
        state.snapshot().unit(mine).expect("mine").orders,
        UnitOrder::PowerOn,
        "the mine is not switched off while a construction can be paused"
    );
    assert_eq!(
        production::ProductionManager::new(state.state_mut(), TeamId::RED).queue(factory),
        vec![UnitType::Infantry],
        "pause keeps the queue"
    );

    // A second generator removes the deficit; the next optimization resets the
    // pause and the construction resumes.
    let second = building(&mut state, UnitType::PowerGenerator, 32);
    let second_fuel = building(&mut state, UnitType::FuelStorage, 34);
    state.unit_mut(second_fuel).expect("FDUMP").storage = 50;
    assert_eq!(
        state.snapshot().unit(second).expect("gen").complex,
        Some(complex)
    );

    let outcome = optimize_complex(state.state_mut(), TeamId::RED, complex, None, None, false);
    assert!(outcome.paused.is_empty(), "{outcome:?}");
    assert!(
        !state
            .snapshot()
            .unit(factory)
            .expect("factory")
            .build_paused,
        "the pause is a per-turn decision"
    );
    assert!(state.snapshot().unit(factory).expect("factory").build_time > 0);
}

// ---------------------------------------------------------------------------
// D — determinism
// ---------------------------------------------------------------------------

fn deficit_run() -> GameUniverse {
    let mut state = state();
    connector_band(&mut state);
    let mine = building(&mut state, UnitType::MiningStation, 20);
    let research = building(&mut state, UnitType::ResearchCentre, 22);
    let greenhouse = building(&mut state, UnitType::EcoSphere, 24);
    let comm = building(&mut state, UnitType::GoldRefinery, 26);
    let generator = building(&mut state, UnitType::PowerGenerator, 28);
    let fuel = building(&mut state, UnitType::FuelStorage, 30);
    state.unit_mut(fuel).expect("fuel").storage = 50;
    let complex = complex_of(&state, mine);
    for id in [mine, research, greenhouse, comm] {
        power_on(&mut state, id);
    }
    let _ = generator;
    let _ = optimize_complex(state.state_mut(), TeamId::RED, complex, None, None, false);
    state
}

#[test]
fn power_deficit_digests_are_reproducible() {
    let first = deficit_run();
    let second = deficit_run();

    assert_eq!(
        crc::state_checksum(first.state()),
        crc::state_checksum(second.state())
    );
    assert_eq!(
        crc::state_digest(first.state()),
        crc::state_digest(second.state())
    );
    assert_eq!(
        crc::replay_digest_bytes(first.state()),
        crc::replay_digest_bytes(second.state())
    );

    // The power orders are part of the canonical snapshot and of the replay
    // digest. The packet-45 CRC hashes only the unit-list fields and does not
    // include `UnitInfo::orders` (C++ `GetUnitListChecksum`), so it is not used
    // for the difference assertion.
    let mut changed = first.clone();
    let research = changed
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            changed
                .snapshot()
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::ResearchCentre)
        })
        .expect("research center");
    let _ = changed.set_order(research, UnitOrder::PowerOn, UnitOrderState::ExecutingOrder);

    assert_ne!(
        crc::state_digest(changed.state()),
        crc::state_digest(first.state())
    );
    assert_ne!(
        crc::replay_digest_crc(changed.state()),
        crc::replay_digest_crc(first.state())
    );

    // The totals helper is pure: the same state yields the same balance.
    let complex = complex_of(
        &first,
        first
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                first
                    .snapshot()
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::MiningStation)
            })
            .expect("mine"),
    );
    let totals: EnergyTotals = complex_totals(first.state(), TeamId::RED, complex);
    assert_eq!(totals, complex_totals(first.state(), TeamId::RED, complex));
}
