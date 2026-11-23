//! Phase 3 (economy): cargo transfer — all three resources, directions and limits.
//!
//! Oracle (`max-cpp-port/src`):
//! * `transfermenu.cpp:37` — `TransferMenu_GetUnitCargoInfo`;
//! * `unitinfo.cpp:990/1056/1124` — `TransferRaw`/`TransferFuel`/`TransferGold`;
//! * `unitinfo.cpp:5294` — `ProcessTransfer` (`ORDER_TRANSFER`: the sign is the
//!   direction, plus — unit → parent, minus — parent → unit);
//! * `complex.cpp:149-203` — `Complex::{TransferCargo,Transfer,GetCargoInfo}`;
//! * `game_manager.cpp:5964` — `GameManager_GetUnitWithCargoType`;
//! * `access.cpp:912` — `Access_UpdateResourcesTotal` (the cache is recomputed
//!   from the member inventories at the start of a turn).
//!
//! The transfer always changes **two** sides (the building's `storage` and the
//! complex field), exactly as in C++.

use max_core::enums::{unit_flags, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::{ComplexId, TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;
use max_core::victory::CargoType;

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

fn spawn_truck(state: &mut GameUniverse, unit_type: UnitType, grid: Point) -> UnitId {
    state
        .spawn_unit(unit_type, TeamId::RED, grid, unit_flags::MOBILE_LAND_UNIT)
        .expect("truck")
}

fn complex_of(state: &GameUniverse, id: UnitId) -> ComplexId {
    state
        .snapshot()
        .unit(id)
        .expect("unit")
        .complex
        .expect("complex")
}

fn field(state: &GameUniverse, complex: ComplexId, cargo: CargoType) -> i16 {
    let target = state
        .snapshot()
        .complex(TeamId::RED, complex.0 as u16)
        .expect("complex");

    match cargo {
        CargoType::Raw => target.raw,
        CargoType::Fuel => target.fuel,
        CargoType::Gold => target.gold,
        _ => 0,
    }
}

/// Sets the transfer order of `unit` and runs `ProcessTransfer`.
fn order_transfer(state: &mut GameUniverse, unit: UnitId, parent: UnitId, amount: i16) {
    {
        let unit = state.unit_mut(unit).expect("unit");
        unit.parent_unit = Some(parent);
        unit.transfer_cargo = amount;
    }

    state.process_transfer(unit);
}

// ---------------------------------------------------------------------------
// I — all resources and both directions
// ---------------------------------------------------------------------------

#[test]
fn raw_transfer_round_trip_changes_both_sides() {
    let mut state = state();
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(20, 20));
    let complex = complex_of(&state, dump);
    state.unit_mut(dump).expect("dump").storage = 10;
    state
        .complex_mut(TeamId::RED, complex.0 as u16)
        .expect("complex")
        .raw = 10;

    let truck = spawn_truck(&mut state, UnitType::SupplyTruck, Point::new(20, 25));
    state.unit_mut(truck).expect("truck").storage = 40;

    // Plus — from the unit to the complex.
    order_transfer(&mut state, truck, dump, 15);
    assert_eq!(state.snapshot().unit(truck).expect("truck").storage, 25);
    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 25);
    assert_eq!(
        field(&state, complex, CargoType::Raw),
        25,
        "the transfer appends to the building and to the complex cache"
    );

    // Minus — from the complex to the unit.
    order_transfer(&mut state, truck, dump, -10);
    assert_eq!(state.snapshot().unit(truck).expect("truck").storage, 35);
    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 15);
    assert_eq!(field(&state, complex, CargoType::Raw), 15);
    assert_eq!(
        state.snapshot().complex_stored(TeamId::RED, complex, CargoType::Raw),
        15
    );
}

#[test]
fn fuel_and_gold_transfers_use_their_own_cargo_type() {
    let mut state = state();

    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(20, 20));
    let fuel_complex = complex_of(&state, fuel);
    state.unit_mut(fuel).expect("FDUMP").storage = 5;
    state
        .complex_mut(TeamId::RED, fuel_complex.0 as u16)
        .expect("complex")
        .fuel = 5;

    let fuel_truck = spawn_truck(&mut state, UnitType::FuelTruck, Point::new(20, 25));
    state.unit_mut(fuel_truck).expect("fuel truck").storage = 20;
    order_transfer(&mut state, fuel_truck, fuel, 10);

    assert_eq!(
        state.snapshot().unit(fuel_truck).expect("truck").storage,
        10
    );
    assert_eq!(state.snapshot().unit(fuel).expect("FDUMP").storage, 15);
    assert_eq!(field(&state, fuel_complex, CargoType::Fuel), 15);

    let gold = spawn(&mut state, UnitType::GoldVault, Point::new(20, 30));
    let gold_complex = complex_of(&state, gold);
    state.unit_mut(gold).expect("GOLDSM").storage = 0;

    let gold_truck = spawn_truck(&mut state, UnitType::GoldTruck, Point::new(20, 35));
    state.unit_mut(gold_truck).expect("gold truck").storage = 7;
    order_transfer(&mut state, gold_truck, gold, 3);

    assert_eq!(state.snapshot().unit(gold_truck).expect("truck").storage, 4);
    assert_eq!(state.snapshot().unit(gold).expect("GOLDSM").storage, 3);
    assert_eq!(field(&state, gold_complex, CargoType::Gold), 3);

    // The transfer menu shows the building's own stock when the complexes match.
    assert_eq!(
        state.snapshot().transfer_menu_cargo_info(gold, gold, CargoType::Gold),
        (3, 50)
    );
}

// ---------------------------------------------------------------------------
// I — limits and overflow
// ---------------------------------------------------------------------------

#[test]
fn transfer_overflow_is_distributed_over_member_storage_in_order() {
    let mut state = state();
    let first = spawn(&mut state, UnitType::MaterialStorage, Point::new(20, 20));
    let second = spawn(&mut state, UnitType::MaterialStorage, Point::new(21, 20));
    let complex = complex_of(&state, first);
    assert_eq!(complex_of(&state, second), complex);

    state.unit_mut(first).expect("first").storage = 0;
    state.unit_mut(second).expect("second").storage = 0;

    let truck = spawn_truck(&mut state, UnitType::SupplyTruck, Point::new(20, 25));
    state.unit_mut(truck).expect("truck").storage = 70;
    order_transfer(&mut state, truck, first, 70);

    assert_eq!(state.snapshot().unit(truck).expect("truck").storage, 0);
    assert_eq!(
        state.snapshot().unit(first).expect("first").storage,
        50,
        "ADUMP cap"
    );
    assert_eq!(
        state.snapshot().unit(second).expect("second").storage,
        20,
        "overflow"
    );
    assert_eq!(
        state.snapshot().complex_stored(TeamId::RED, complex, CargoType::Raw),
        70
    );
    // C++ `TransferRaw` adds the amount to the complex field, subtracts the
    // overflow and `Complex::Transfer` adds it back.
    assert_eq!(field(&state, complex, CargoType::Raw), 70);
}

#[test]
fn draining_more_than_the_complex_holds_never_drives_storage_negative() {
    let mut state = state();
    let first = spawn(&mut state, UnitType::MaterialStorage, Point::new(20, 20));
    let second = spawn(&mut state, UnitType::MaterialStorage, Point::new(21, 20));
    let complex = complex_of(&state, first);
    state.unit_mut(first).expect("first").storage = 10;
    state.unit_mut(second).expect("second").storage = 10;
    state
        .complex_mut(TeamId::RED, complex.0 as u16)
        .expect("complex")
        .raw = 20;

    state.complex_transfer(TeamId::RED, complex, -30, 0, 0);

    assert_eq!(state.snapshot().unit(first).expect("first").storage, 0);
    assert_eq!(state.snapshot().unit(second).expect("second").storage, 0);
    assert_eq!(
        field(&state, complex, CargoType::Raw),
        -10,
        "Complex::Transfer lets the one-turn cache go negative, as C++"
    );

    // `Access_UpdateResourcesTotal` recomputes the cache from the inventory — the
    // negative value disappears and nothing is lost or created.
    state.update_resources_total(TeamId::RED, complex);
    assert_eq!(field(&state, complex, CargoType::Raw), 0);
    assert_eq!(state.snapshot().get_cargo_info(TeamId::RED, complex).0.raw, 0);
}

#[test]
fn a_transfer_of_a_foreign_cargo_type_is_ignored() {
    let mut state = state();
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(20, 20));
    let complex = complex_of(&state, dump);
    state.unit_mut(dump).expect("dump").storage = 0;

    state.transfer_cargo_kind(dump, CargoType::Fuel, 10);

    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 0);
    assert_eq!(field(&state, complex, CargoType::Fuel), 0);
    assert_eq!(field(&state, complex, CargoType::Raw), 0);
}

// ---------------------------------------------------------------------------
// D — determinism
// ---------------------------------------------------------------------------

fn run_transfers() -> GameUniverse {
    let mut state = state();
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(20, 20));
    let complex = complex_of(&state, dump);
    state.unit_mut(dump).expect("dump").storage = 10;
    state
        .complex_mut(TeamId::RED, complex.0 as u16)
        .expect("complex")
        .raw = 10;

    let truck = spawn_truck(&mut state, UnitType::SupplyTruck, Point::new(20, 25));
    state.unit_mut(truck).expect("truck").storage = 40;

    order_transfer(&mut state, truck, dump, 15);
    order_transfer(&mut state, truck, dump, -10);
    order_transfer(&mut state, truck, dump, 5);

    state
}

#[test]
fn transfer_digests_are_reproducible() {
    let first = run_transfers();
    let second = run_transfers();

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

    // The truck's storage enters both the packet-45 CRC (`GetUnitListChecksum`
    // hashes `storage` for cargo units) and the canonical digest.
    let mut changed = first.clone();
    let truck = changed
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            changed
                .snapshot()
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::SupplyTruck)
        })
        .expect("truck");
    changed.unit_mut(truck).expect("truck").storage += 1;

    assert_ne!(
        changed.snapshot().state_checksum(),
        first.snapshot().state_checksum()
    );
    assert_ne!(
        changed.snapshot().state_digest(),
        first.snapshot().state_digest()
    );
    assert_ne!(
        changed.snapshot().replay_digest_crc(),
        first.snapshot().replay_digest_crc()
    );
}
