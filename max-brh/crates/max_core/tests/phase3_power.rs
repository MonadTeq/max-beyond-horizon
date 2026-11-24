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

use max_core::enums::{TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::map::{ResourceKind, ResourceMapEntry};
use max_core::power::EnergyTotals;
use max_core::team::{ComplexId, TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

fn state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
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
    let before = state.snapshot().complex_totals(TeamId::RED, complex);
    assert_eq!(before.power, -4, "four consumers, producer off");

    let outcome = state.optimize_complex(TeamId::RED, complex, None, None, true);

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
    let outcome = state.optimize_complex(TeamId::RED, complex, None, None, true);

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
    // The oracle names the missing resource of a switched-off producer:
    // `ComposeIndustryMessage(unit, _(cdcc), _(b9d9), _(acbe))` with `acbe` = "fuel"
    // (`production_manager.cpp:164-186`).
    assert!(
        message.contains("More fuel needed, Power Generator turned off."),
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
    let outcome = state.optimize_complex(TeamId::RED, complex, None, None, false);
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

    let outcome = state.optimize_complex(TeamId::RED, complex, None, None, false);
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
    let mut manager = state.production_manager(TeamId::RED);
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

    let outcome = state.optimize_complex(TeamId::RED, complex, None, None, true);

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
        state.production_manager(TeamId::RED).queue(factory),
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

    let outcome = state.optimize_complex(TeamId::RED, complex, None, None, false);
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
// D — automatic fuel redistribution on a power deficit
// ---------------------------------------------------------------------------

/// Defect 3 of the manual run 1.2: when power is short but there are switched-off
/// power sources, the end-of-turn optimizer must **reallocate** the complex's
/// mining — turn raw/gold extraction into fuel — instead of giving the producer up
/// and switching the mine off.
///
/// `ProductionManager_OptimizeProduction` runs `CheckPowerNeed(0)` in its debt
/// loop; `CheckPowerNeed` calls `OptimizeCargoProduction(CARGO_FUEL,
/// -total.fuel, false)` (`production_manager.cpp:642-664`, repeated at
/// `:849-861`), which converts the mining of the complex into fuel
/// (`allocmenu.cpp:438-523`). The port performs the same step in
/// `settle_producers` before cutting the planned producers back.
///
/// Fixture: mine A is saturated with 16 raw units and its surveyed square
/// carries 8 fuel; mine B has free mining units and 8 raw. Both generators are
/// off and the fuel stock is empty — without the redistribution the generators
/// would stay off and the mines would be switched off one by one.
#[test]
fn power_deficit_redistributes_raw_mining_into_fuel() {
    let mut state = state();
    connector_band(&mut state);

    let mine = building(&mut state, UnitType::MiningStation, 20);
    let other_mine = building(&mut state, UnitType::MiningStation, 30);
    let first_generator = building(&mut state, UnitType::PowerGenerator, 24);
    let second_generator = building(&mut state, UnitType::PowerGenerator, 26);
    let fuel = building(&mut state, UnitType::FuelStorage, 33);
    state.unit_mut(fuel).expect("fuel").storage = 0;

    // Mine A, surveyed square [20,21]×[22,23]: 8 fuel and 8 raw.
    // Mine B, surveyed square [30,31]×[22,23]: 8 raw on free mining units.
    for (x, y, kind) in [
        (20, 22, ResourceKind::Fuel),
        (21, 22, ResourceKind::Fuel),
        (20, 23, ResourceKind::Raw),
        (21, 23, ResourceKind::Raw),
    ] {
        state
            .map_mut()
            .resources
            .set(x, y, ResourceMapEntry::new(4, Some(kind), 0));
    }
    for x in [30, 31] {
        for y in [22, 23] {
            state.map_mut().resources.set(
                x,
                y,
                ResourceMapEntry::new(2, Some(ResourceKind::Raw), 0),
            );
        }
    }

    power_on(&mut state, mine);
    power_on(&mut state, other_mine);
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.raw_mining = 16;
        unit.fuel_mining = 0;
        unit.gold_mining = 0;
        unit.total_mining = 16;
    }
    {
        let unit = state.unit_mut(other_mine).expect("other mine");
        unit.raw_mining = 0;
        unit.fuel_mining = 0;
        unit.gold_mining = 0;
        unit.total_mining = 0;
    }

    let complex = complex_of(&state, mine);
    for id in [other_mine, first_generator, second_generator] {
        assert_eq!(
            state.snapshot().unit(id).expect("member").complex,
            Some(complex)
        );
    }

    // Two powered mines = 2 power; two off generators = +1 each for 2 fuel.
    // `manage_economy` is the end-of-turn entry point (`OptimizeComplexes`).
    state.manage_economy(TeamId::RED);

    let mine_unit = state.snapshot().unit(mine).expect("mine");
    assert_eq!(
        mine_unit.fuel_mining, 4,
        "the 4 fuel units the two generators need were allocated"
    );
    assert_eq!(
        mine_unit.raw_mining, 12,
        "the same 4 units came out of the mine's raw extraction"
    );
    assert_eq!(
        mine_unit.total_mining, 16,
        "the mine's 16-unit cap is unchanged"
    );
    assert_eq!(
        mine_unit.orders,
        UnitOrder::PowerOn,
        "the mine is not switched off: the fuel was redistributed instead"
    );

    let other = state.snapshot().unit(other_mine).expect("other mine");
    assert_eq!(
        other.raw_mining, 4,
        "the displaced raw extraction moved to the free units of the second mine"
    );
    assert_eq!(other.orders, UnitOrder::PowerOn);

    for generator in [first_generator, second_generator] {
        assert_eq!(
            state.snapshot().unit(generator).expect("generator").orders,
            UnitOrder::PowerOn,
            "the switched-off power sources were started"
        );
    }

    let totals = state.snapshot().complex_totals(TeamId::RED, complex);
    assert_eq!(totals.power, 0, "the demand is covered");
    assert!(totals.fuel >= 0, "fuel never goes negative: {totals:?}");
    assert!(totals.raw >= 0, "{totals:?}");
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
    let _ = state.optimize_complex(TeamId::RED, complex, None, None, false);
    state
}

#[test]
fn power_deficit_digests_are_reproducible() {
    let first = deficit_run();
    let second = deficit_run();

    assert_eq!(
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
    assert_eq!(
        first.snapshot().state_digest(),
        second.snapshot().state_digest()
    );
    assert_eq!(
        first.snapshot().replay_digest_bytes(),
        second.snapshot().replay_digest_bytes()
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
        changed.snapshot().state_digest(),
        first.snapshot().state_digest()
    );
    assert_ne!(
        changed.snapshot().replay_digest_crc(),
        first.snapshot().replay_digest_crc()
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
    let totals: EnergyTotals = first.snapshot().complex_totals(TeamId::RED, complex);
    assert_eq!(
        totals,
        first.snapshot().complex_totals(TeamId::RED, complex)
    );
}
