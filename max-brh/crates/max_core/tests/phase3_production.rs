//! Phase 3 (economy): production — every factory, cost and build time, full cycle.
//!
//! Oracle (`max-cpp-port/src`):
//! * `cargo.cpp:140` — `Cargo_GetRawConsumptionRate` (TRAINHAL 1, LVP/HVP/AIRPLT/
//!   SHIPYARD 3 per 1× turn; 2× → 4, 4× → 12);
//! * `buildmenu.cpp:688` — `BuildMenu_GetTurnsToBuild` (`ATTRIB_TURNS` of the type);
//! * `buildmenu.cpp:729` — `BuildMenu_GetMaxPossibleBuildRate`;
//! * `production_manager.cpp:677-727` — `ValidateIndustry` (a shortage pauses the
//!   construction);
//! * `game_manager.cpp:8076-8094` — `GameManager_SpawnNewUnits` (on
//!   `build_time <= build_rate` → `SpawnNewUnit`);
//! * `unitinfo.cpp:3441-3520` — `UnitInfo::SpawnNewUnit` (the factory keeps the
//!   produced unit inside until an activation order).
//!
//! The table test commits every `(factory, buildable unit)` pair and checks the
//! `Turns`/`Cost` columns; the cycle tests run `HeavyVehiclePlant → Tank` and
//! `Shipyard → Escort` from queue to spawn on a live `GameState`.

use max_core::enums::{TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::production::{
    self, cost_per_turn, total_cost, BuildError, ProductionEventKind, ProductionManager, BUILDABLE,
    FACTORIES,
};
use max_core::production_turns_to_build;
use max_core::team::{ComplexId, TeamId, TeamKind, TeamUnits};
use max_core::unit::{UnitId, UnitListKind};
use max_core::{crc, GameState};

fn state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(state: &mut GameState, unit_type: UnitType, grid: Point) -> UnitId {
    state
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .expect("spawn")
}

/// A powered factory complex:
///
/// ```text
///   FACTORY(20,20) 2×2 -- ADUMP(22,20) -- FDUMP(23,20) -- POWGEN(24,20)
///                                                            |
///                                                        HABITAT(24,21)
/// ```
///
/// `ADUMP` holds 50 raw material (enough for the whole construction at the C++
/// rates), `FDUMP` — 50 fuel for the generator, `HABITAT` — +3 workers.
fn factory_state(factory_type: UnitType) -> (GameState, UnitId) {
    let mut state = state();
    let factory = spawn(&mut state, factory_type, Point::new(20, 20));
    let raw = spawn(&mut state, UnitType::MaterialStorage, Point::new(22, 20));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(23, 20));
    let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(24, 20));
    let habitat = spawn(&mut state, UnitType::Habitat, Point::new(24, 21));

    state.unit_mut(raw).expect("ADUMP").storage = 50;
    state.unit_mut(fuel).expect("FDUMP").storage = 50;

    let complex = state.unit(factory).expect("factory").complex.expect("complex");
    for id in [raw, fuel, generator, habitat] {
        assert_eq!(
            state.unit(id).expect("member").complex,
            Some(complex),
            "{factory_type:?} complex"
        );
    }

    (state, factory)
}

fn production_turn(state: &mut GameState) -> Vec<max_core::ProductionEvent> {
    state.manage_economy(TeamId::RED);
    production::progress_build_state(state, TeamId::RED)
}

fn count_type(state: &GameState, unit_type: UnitType) -> usize {
    state
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == unit_type)
        })
        .count()
}

fn raw_storage(state: &GameState, complex: ComplexId) -> i32 {
    max_core::alloc::get_cargo_info(state, TeamId::RED, complex).0.raw
}

// ---------------------------------------------------------------------------
// I — every factory and every type of its table
// ---------------------------------------------------------------------------

#[test]
fn every_factory_can_start_each_buildable_type_with_the_cpp_cost_and_time() {
    let mut checked = 0usize;

    for (factory_type, units) in BUILDABLE {
        if !FACTORIES.contains(&factory_type) {
            continue; // builders (`Constructor`/`Engineer`) are covered elsewhere
        }

        for &unit_type in units {
            let (mut state, factory) = factory_state(factory_type);

            let mut manager = ProductionManager::new(&mut state, TeamId::RED);
            manager.enqueue(factory, unit_type).expect("queue");
            let report = manager
                .commit(factory)
                .unwrap_or_else(|error| panic!("{factory_type:?} → {unit_type:?}: {error:?}"));
            drop(manager);

            assert_eq!(
                report.turns,
                production_turns_to_build(&state, TeamId::RED, unit_type),
                "{factory_type:?} → {unit_type:?} turns"
            );
            assert!(report.turns > 0);
            assert_eq!(report.build_rate, 1, "a fresh commit is 1×");
            assert_eq!(
                report.cost_per_turn,
                cost_per_turn(factory_type, 1),
                "{factory_type:?} per-turn raw cost (cargo.cpp:140)"
            );
            assert_eq!(
                total_cost(factory_type, report.turns),
                report.cost_per_turn * report.turns,
                "the menu Cost column is rate × turns"
            );
            assert!(report.cost_per_turn > 0);
            assert_eq!(
                state.unit(factory).expect("factory").build_time,
                report.turns as u8,
                "the order starts with the whole build time"
            );
            assert_eq!(state.unit(factory).expect("factory").orders, UnitOrder::Build);

            checked += 1;
        }
    }

    assert!(checked >= 30, "checked {checked} factory/type pairs");
}

#[test]
fn committing_a_non_builder_or_a_foreign_type_is_refused() {
    let (mut state, factory) = factory_state(UnitType::TrainingHall);
    let tank = spawn(&mut state, UnitType::Tank, Point::new(30, 30));

    assert_eq!(
        ProductionManager::new(&mut state, TeamId::RED).enqueue(tank, UnitType::Scout),
        Err(BuildError::NotABuilder)
    );
    assert_eq!(
        ProductionManager::new(&mut state, TeamId::RED).enqueue(tank, UnitType::Tank),
        Err(BuildError::NotABuilder),
        "a tank has no build queue"
    );
    assert_eq!(
        ProductionManager::new(&mut state, TeamId::RED).enqueue(factory, UnitType::Tank),
        Err(BuildError::NotBuildable),
        "the Training Hall only builds infantry types"
    );
    assert_eq!(
        ProductionManager::new(&mut state, TeamId::RED).commit(factory),
        Err(BuildError::EmptyQueue)
    );
}

// ---------------------------------------------------------------------------
// I — full cycle: queue → paid turns → spawn
// ---------------------------------------------------------------------------

fn run_cycle(factory_type: UnitType, unit_type: UnitType) -> (GameState, i32, Vec<max_core::ProductionEvent>) {
    let (mut state, factory) = factory_state(factory_type);
    let complex = state.unit(factory).expect("factory").complex.expect("complex");
    let rate = cost_per_turn(factory_type, 1);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, unit_type).expect("queue");
    let turns = manager.commit(factory).expect("commit").turns;
    drop(manager);

    let mut events = Vec::new();
    for _ in 0..turns {
        events = production_turn(&mut state);
        if events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned)
        {
            break;
        }
    }

    assert_eq!(count_type(&state, unit_type), 1, "the unit was produced");
    assert_eq!(
        raw_storage(&state, complex),
        50 - rate * turns,
        "the complex paid the C++ per-turn rate for every turn"
    );
    assert_eq!(
        state.unit(factory).expect("factory").state,
        UnitOrderState::UnitReady,
        "the factory waits for the unit to be activated"
    );

    (state, turns, events)
}

#[test]
fn heavy_vehicle_plant_builds_a_tank_through_the_full_cycle() {
    let (state, turns, events) = run_cycle(UnitType::HeavyVehiclePlant, UnitType::Tank);
    assert!(turns > 0);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "spawn event: {events:?}"
    );

    // The produced unit is stored **inside** the factory (`SpawnNewUnit`): parent set,
    // `ORDER_IDLE`/`ORDER_STATE_STORE`.
    let tank = state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::Tank)
        })
        .expect("tank");
    let unit = state.unit(tank).expect("tank");
    assert_eq!(unit.orders, UnitOrder::Idle);
    assert_eq!(unit.state, UnitOrderState::Store);
    assert!(unit.parent_unit.is_some());
}

#[test]
fn shipyard_builds_an_escort_through_the_full_cycle() {
    let (state, turns, events) = run_cycle(UnitType::Shipyard, UnitType::Escort);
    assert!(turns > 0);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "spawn event: {events:?}"
    );
    assert_eq!(count_type(&state, UnitType::Escort), 1);
}

// ---------------------------------------------------------------------------
// D — determinism
// ---------------------------------------------------------------------------

fn cycle_digests(unit_type: UnitType) -> (u16, u16, u16) {
    let (state, _, _) = run_cycle(UnitType::HeavyVehiclePlant, unit_type);
    (
        crc::state_checksum(&state),
        crc::state_digest(&state),
        crc::replay_digest_crc(&state),
    )
}

#[test]
fn production_cycle_digests_are_reproducible() {
    assert_eq!(
        cycle_digests(UnitType::Tank),
        cycle_digests(UnitType::Tank),
        "two identical Tank cycles have the same CRC/digest"
    );
    // A different product changes the digests (different unit type and time).
    assert_ne!(cycle_digests(UnitType::Tank), cycle_digests(UnitType::AssaultGun));

    // `state_checksum` covers the five unit lists; the produced unit is stationary
    // only while stored, so both a tank and an assault gun produce a non-zero CRC.
    let (checksum, _, _) = cycle_digests(UnitType::Tank);
    assert_ne!(checksum, 0);
    assert_eq!(
        UnitListKind::Stationary.index(),
        2,
        "C++ list order used by the checksum"
    );
}
