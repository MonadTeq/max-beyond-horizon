//! Phase 3 (economy): the complex/connector graph — mine ↔ factories ↔ power.
//!
//! Oracle (`max-cpp-port/src`):
//! * `units_manager.cpp:2051-2225` — `UnitsManager_UpdateConnectors`;
//! * `unitinfo.cpp:1978-2313` — `AttachComplex`/`AttachToPrimaryComplex`/
//!   `TestConnections`/`DetachComplex`;
//! * `complex.cpp:109-203` — `Complex::{Grow,Shrink,GetCargoInfo,GetCargoMining}`;
//! * `access.cpp:912` — `Access_UpdateResourcesTotal` (the complex stock cache);
//! * `units_manager.cpp:1556` — `UnitsManager_RemoveConnections`.
//!
//! The scenario graph is one connected complex:
//!
//! ```text
//!   ADUMP(20,19)          HABITAT(22,22)
//!      |                       |
//!   MINE(20,20) 2×2 -- PLANT(22,20) 2×2 -- POWGEN(24,20) -- FDUMP(24,21)
//! ```
//!
//! Every stage is checked against the per-turn stock recomputation and the
//! canonical digests (`state_checksum`, `state_digest`, `replay_digest_crc`).

use max_core::classification::UnitTypeClassify;
use max_core::complex::unit_size;
use max_core::enums::{TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{ComplexId, TeamId, TeamKind, TeamUnits};
use max_core::unit::{UnitId, UnitListKind};
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

fn complex_of(state: &GameUniverse, id: UnitId) -> ComplexId {
    state
        .snapshot()
        .unit(id)
        .expect("unit")
        .complex
        .expect("complex")
}

/// The full economy graph from the module header. Every building is spawned
/// adjacent to the previous one, so `UpdateConnectors` merges them by itself
/// (no manual complex reassignment).
fn economy_graph(state: &mut GameUniverse) -> Vec<UnitId> {
    let mine = spawn(state, UnitType::MiningStation, Point::new(20, 20));
    let raw = spawn(state, UnitType::MaterialStorage, Point::new(20, 19));
    let plant = spawn(state, UnitType::LightVehiclePlant, Point::new(22, 20));
    let workers = spawn(state, UnitType::Habitat, Point::new(22, 22));
    let generator = spawn(state, UnitType::PowerGenerator, Point::new(24, 20));
    let fuel = spawn(state, UnitType::FuelStorage, Point::new(24, 21));

    vec![mine, raw, plant, workers, generator, fuel]
}

// ---------------------------------------------------------------------------
// I — the graph and its aggregate stock/capacity
// ---------------------------------------------------------------------------

#[test]
fn mine_factories_and_power_form_one_connected_complex() {
    let mut state = state();
    let ids = economy_graph(&mut state);
    let complex = complex_of(&state, ids[0]);

    for id in &ids {
        assert_eq!(
            state.snapshot().unit(*id).expect("unit").complex,
            Some(complex),
            "{:?} must join the same complex",
            state.snapshot().unit(*id).expect("unit").unit_type
        );
    }

    let buildings = state.snapshot().team_units()[0]
        .get_complex(complex)
        .expect("complex")
        .buildings;
    assert_eq!(buildings, 6, "Complex::Grow counts every member");

    // 2×2 buildings vs 1×1 plants (`UnitInfo::GetConnectedBuilding`).
    assert_eq!(unit_size(UnitType::MiningStation.footprint()), 2);
    assert_eq!(unit_size(UnitType::PowerGenerator.footprint()), 1);

    // Stocks: mine 10 + ADUMP 5 (raw), FDUMP 20 (fuel), mine mines 4 raw/turn,
    // POWGEN consumes 2 fuel and gives 1 power while the mine takes 1 power.
    {
        let mine = state.unit_mut(ids[0]).expect("mine");
        mine.storage = 10;
        mine.raw_mining = 4;
        mine.total_mining = 4;
        mine.orders = UnitOrder::PowerOn;
        mine.state = UnitOrderState::ExecutingOrder;
    }
    state.unit_mut(ids[1]).expect("ADUMP").storage = 5;
    state.unit_mut(ids[5]).expect("FDUMP").storage = 20;
    let _ = state.set_order(ids[4], UnitOrder::PowerOn, UnitOrderState::ExecutingOrder);

    state.update_resources_total(TeamId::RED, complex);

    let stored = state
        .snapshot()
        .complex(TeamId::RED, complex.0 as u16)
        .expect("complex");
    assert_eq!(stored.raw, 19, "10 (mine) + 5 (ADUMP) + 4 mining");
    assert_eq!(stored.fuel, 18, "20 − 2 (POWGEN rate)");
    assert_eq!(stored.power, 0, "+1 POWGEN −1 mine");
    assert_eq!(stored.workers, 3, "HABITAT +3");

    let (materials, capacity) = state.snapshot().get_cargo_info(TeamId::RED, complex);
    assert_eq!(materials.raw, 15, "inventory only, mining is a cache");
    assert_eq!(capacity.raw, 75, "MININGST 25 + ADUMP 50");
    assert_eq!(capacity.fuel, 50, "FDUMP 50");
    assert_eq!(capacity.gold, 0, "no gold storage in the graph");

    // The same balance through the power module.
    let totals = state.snapshot().complex_totals(TeamId::RED, complex);
    assert_eq!(totals.raw, stored.raw as i32);
    assert_eq!(totals.fuel, stored.fuel as i32);
    assert_eq!(totals.power, 0);
    assert_eq!(totals.life, 3);
}

#[test]
fn splitting_a_connector_chain_splits_the_stock_and_capacity() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(20, 20));
    let first = spawn(&mut state, UnitType::Connector, Point::new(22, 20));
    let bridge = spawn(&mut state, UnitType::Connector, Point::new(23, 20));
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(24, 20));

    let merged = complex_of(&state, mine);
    assert_eq!(
        complex_of(&state, dump),
        merged,
        "the chain merges everything"
    );

    state.unit_mut(mine).expect("mine").storage = 10;
    state.unit_mut(dump).expect("dump").storage = 20;
    state.update_resources_total(TeamId::RED, merged);
    let (materials, capacity) = state.snapshot().get_cargo_info(TeamId::RED, merged);
    assert_eq!(materials.raw, 30);
    assert_eq!(capacity.raw, 75);

    // `DestroyUnit` → `UnitsManager_RemoveConnections` → `DetachComplex`:
    // the graph splits and the stocks are recomputed per remaining group.
    state.destroy_unit(bridge);

    let mine_complex = complex_of(&state, mine);
    let dump_complex = complex_of(&state, dump);
    assert_ne!(mine_complex, dump_complex, "the complex split in two");
    assert_eq!(
        complex_of(&state, first),
        mine_complex,
        "connector #1 stays with the mine"
    );

    let (mine_materials, mine_capacity) = state.snapshot().get_cargo_info(TeamId::RED, mine_complex);
    let (dump_materials, dump_capacity) = state.snapshot().get_cargo_info(TeamId::RED, dump_complex);
    assert_eq!(mine_materials.raw, 10);
    assert_eq!(mine_capacity.raw, 25, "only the mine");
    assert_eq!(dump_materials.raw, 20);
    assert_eq!(dump_capacity.raw, 50, "only the ADUMP");

    assert_eq!(
        state.snapshot().team_units()[0]
            .get_complex(mine_complex)
            .expect("mine complex")
            .buildings,
        2
    );
    assert_eq!(
        state.snapshot().team_units()[0]
            .get_complex(dump_complex)
            .expect("dump complex")
            .buildings,
        1
    );
}

#[test]
fn power_off_keeps_the_graph_but_stops_the_mining_income() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(20, 20));
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(22, 20));
    let complex = complex_of(&state, mine);
    assert_eq!(complex_of(&state, dump), complex);

    state.unit_mut(mine).expect("mine").raw_mining = 6;
    state.unit_mut(mine).expect("mine").total_mining = 6;

    // An off mine is still a complex member (`IsComplexMember` ignores the order).
    let _ = state.set_order(mine, UnitOrder::PowerOff, UnitOrderState::ExecutingOrder);
    assert_eq!(complex_of(&state, mine), complex);
    assert_eq!(
        state.snapshot().get_cargo_info(TeamId::RED, complex).1.raw,
        75,
        "capacity is unchanged by the order"
    );

    state.update_resources_total(TeamId::RED, complex);
    assert_eq!(
        state
            .snapshot()
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        0,
        "an off mine adds no production"
    );
    assert_eq!(state.snapshot().complex_totals(TeamId::RED, complex).power, 0);

    // Powered on again — the same graph, 6 raw of income, −1 power.
    let _ = state.set_order(mine, UnitOrder::PowerOn, UnitOrderState::ExecutingOrder);
    state.update_resources_total(TeamId::RED, complex);
    assert_eq!(
        state
            .snapshot()
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        6
    );
    assert_eq!(
        state.snapshot().complex_totals(TeamId::RED, complex).power,
        -1
    );
}

// ---------------------------------------------------------------------------
// D — determinism
// ---------------------------------------------------------------------------

fn digest_run(split: bool) -> (u16, u16, u16) {
    let mut state = state();
    if split {
        let mine = spawn(&mut state, UnitType::MiningStation, Point::new(20, 20));
        let _first = spawn(&mut state, UnitType::Connector, Point::new(22, 20));
        let bridge = spawn(&mut state, UnitType::Connector, Point::new(23, 20));
        let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(24, 20));
        state.unit_mut(mine).expect("mine").storage = 7;
        state.unit_mut(dump).expect("dump").storage = 3;
        state.destroy_unit(bridge);
    } else {
        let ids = economy_graph(&mut state);
        {
            let mine = state.unit_mut(ids[0]).expect("mine");
            mine.raw_mining = 4;
            mine.total_mining = 4;
            mine.orders = UnitOrder::PowerOn;
            mine.state = UnitOrderState::ExecutingOrder;
        }
        state.unit_mut(ids[1]).expect("ADUMP").storage = 5;
        state.unit_mut(ids[5]).expect("FDUMP").storage = 20;
        let _ = state.set_order(ids[4], UnitOrder::PowerOn, UnitOrderState::ExecutingOrder);
        let complex = complex_of(&state, ids[0]);
        state.update_resources_total(TeamId::RED, complex);
    }

    (
        state.snapshot().state_checksum(),
        state.snapshot().state_digest(),
        state.snapshot().replay_digest_crc(),
    )
}

#[test]
fn complex_graph_digests_are_reproducible() {
    assert_eq!(digest_run(false), digest_run(false), "graph digests");
    assert_eq!(digest_run(true), digest_run(true), "split digests");

    // A broken link changes the canonical digest and the replay digest; the
    // packet-45 CRC only hashes the unit lists and may stay identical, so it is
    // not asserted here.
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(20, 20));
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(22, 20));
    let _ = dump;
    let before = state.snapshot().state_digest();
    let before_replay = state.snapshot().replay_digest_crc();

    state.unit_mut(mine).expect("mine").connectors ^= 0x0004; // CONNECTOR_EAST_TOP
    assert_ne!(
        state.snapshot().state_digest(),
        before,
        "connectors enter the snapshot"
    );
    assert_ne!(
        state.snapshot().replay_digest_crc(),
        before_replay,
        "connectors enter the replay digest"
    );

    // The stationary list is the deterministic traversal order used by
    // `Complex::Transfer`; it must keep its insertion-ordered content.
    assert_eq!(
        state.snapshot().unit_list(UnitListKind::Stationary).len(),
        2,
        "both buildings are stationary"
    );
    assert_eq!(
        max_core::complex::CONNECTOR_ORDER.len(),
        8,
        "eight C++ connector directions"
    );
}
