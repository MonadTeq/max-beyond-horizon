use super::*;
use crate::enums::{unit_flags, TeamClan, UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::TeamUnits;

fn test_state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = crate::team::TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

/// Spawn a building moving its complex into `complex` (in the port each
/// building gets its own complex: there is no merging via connectors).
fn spawn_member(
    state: &mut GameState,
    unit_type: UnitType,
    grid: Point,
    complex: Option<ComplexId>,
) -> UnitId {
    let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
    let id = state
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("unit");
    let own = state.unit(id).and_then(|unit| unit.complex);

    if let Some(complex) = complex {
        if let Some(own) = own {
            if own != complex {
                state.team_units[0].remove_complex(own);
            }
        }

        state.unit_mut(id).expect("unit").complex = Some(complex);
        if let Some(target) = state.team_units[0].get_complex_mut(complex) {
            target.grow();
        }
    }

    id
}

fn new_complex(state: &mut GameState) -> ComplexId {
    state.team_units[0].create_complex()
}

#[test]
fn transfer_cargo_fills_and_drains_building() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let dump = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        Some(complex),
    );

    state.unit_mut(dump).expect("dump").storage = 0;

    let mut amount = 7;
    transfer_cargo(&mut state, dump, CargoType::Raw, &mut amount);
    assert_eq!(amount, 0);
    assert_eq!(state.unit(dump).expect("dump").storage, 7);

    let mut amount = -3;
    transfer_cargo(&mut state, dump, CargoType::Raw, &mut amount);
    assert_eq!(amount, 0);
    assert_eq!(state.unit(dump).expect("dump").storage, 4);

    let mut amount = -10;
    transfer_cargo(&mut state, dump, CargoType::Raw, &mut amount);
    assert_eq!(amount, -6, "забрали только 4");
    assert_eq!(state.unit(dump).expect("dump").storage, 0);
}

#[test]
fn transfer_cargo_caps_at_attribute_storage() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let dump = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        Some(complex),
    );

    state.unit_mut(dump).expect("dump").storage = 0;

    let mut amount = 80;
    transfer_cargo(&mut state, dump, CargoType::Raw, &mut amount);
    assert_eq!(state.unit(dump).expect("dump").storage, 50, "ADUMP = 50");
    assert_eq!(amount, 30, "излишек остаётся вызывающему");
}

#[test]
fn transfer_cargo_kind_distributes_overflow_across_members() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let first = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        Some(complex),
    );
    let second = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 24),
        Some(complex),
    );

    for id in [first, second] {
        state.unit_mut(id).expect("dump").storage = 0;
    }

    transfer_cargo_kind(&mut state, first, CargoType::Raw, 70);

    assert_eq!(state.unit(first).expect("first").storage, 50);
    assert_eq!(state.unit(second).expect("second").storage, 20);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        70
    );
}

#[test]
fn process_transfer_unit_to_complex_and_back() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let dump = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        Some(complex),
    );
    state.unit_mut(dump).expect("dump").storage = 0;

    // A mobile truck outside a complex.
    let truck = state
        .spawn_unit(
            UnitType::SupplyTruck,
            TeamId::RED,
            Point::new(21, 21),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("truck");
    state.unit_mut(truck).expect("truck").storage = 40;
    state.unit_mut(truck).expect("truck").parent_unit = Some(dump);
    state.unit_mut(truck).expect("truck").transfer_cargo = 15;

    process_transfer(&mut state, truck);

    assert_eq!(state.unit(truck).expect("truck").storage, 25);
    assert_eq!(state.unit(dump).expect("dump").storage, 15);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        15
    );

    // Back: minus — from the complex to the unit.
    state.unit_mut(truck).expect("truck").transfer_cargo = -10;
    process_transfer(&mut state, truck);

    assert_eq!(state.unit(truck).expect("truck").storage, 35);
    assert_eq!(state.unit(dump).expect("dump").storage, 5);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        5
    );
}

#[test]
fn process_transfer_from_complex_spills_across_buildings() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let first = spawn_member(
        &mut state,
        UnitType::FuelStorage,
        Point::new(20, 20),
        Some(complex),
    );
    let second = spawn_member(
        &mut state,
        UnitType::FuelStorage,
        Point::new(22, 24),
        Some(complex),
    );

    state.unit_mut(first).expect("first").storage = 10;
    state.unit_mut(second).expect("second").storage = 30;
    if let Some(target) = state.complex_mut(TeamId::RED, complex.0 as u16) {
        target.fuel = 40;
    }

    let truck = state
        .spawn_unit(
            UnitType::FuelTruck,
            TeamId::RED,
            Point::new(21, 21),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("truck");
    state.unit_mut(truck).expect("truck").storage = 0;
    state.unit_mut(truck).expect("truck").parent_unit = Some(first);
    state.unit_mut(truck).expect("truck").transfer_cargo = -35;

    process_transfer(&mut state, truck);

    // The first building gave 10, then `Complex::Transfer` took 25 from the second.
    assert_eq!(state.unit(first).expect("first").storage, 0);
    assert_eq!(state.unit(second).expect("second").storage, 5);
    assert_eq!(state.unit(truck).expect("truck").storage, 35);
}

#[test]
fn transfer_menu_cargo_info_uses_complex_only_for_foreign_target() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let dump = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        Some(complex),
    );
    let other = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 24),
        Some(complex),
    );

    state.unit_mut(dump).expect("dump").storage = 7;
    state.unit_mut(other).expect("other").storage = 0;
    if let Some(target) = state.complex_mut(TeamId::RED, complex.0 as u16) {
        target.raw = 7;
    }

    let truck = state
        .spawn_unit(
            UnitType::SupplyTruck,
            TeamId::RED,
            Point::new(21, 21),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("truck");
    state.unit_mut(truck).expect("truck").storage = 11;

    // A foreign (complex-less) receiver — the whole stock/capacity of the source
    // complex (two ADUMPs: 50 + 50).
    assert_eq!(
        transfer_menu_cargo_info(&state, dump, truck, CargoType::Raw),
        (7, 100)
    );
    // An own complex — the whole stock/capacity.
    assert_eq!(
        transfer_menu_cargo_info(&state, dump, dump, CargoType::Raw),
        (7, 50)
    );
    assert_eq!(
        transfer_menu_cargo_info(&state, truck, dump, CargoType::Raw),
        (11, 50)
    );
}

#[test]
fn transfer_order_restores_orders() {
    let mut state = test_state();
    let dump = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        None,
    );
    state.unit_mut(dump).expect("dump").storage = 0;

    let truck = state
        .spawn_unit(
            UnitType::SupplyTruck,
            TeamId::RED,
            Point::new(21, 21),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("truck");
    {
        let unit = state.unit_mut(truck).expect("truck");
        unit.storage = 10;
        unit.parent_unit = Some(dump);
        unit.transfer_cargo = 5;
        unit.orders = UnitOrder::Transfer;
        unit.state = UnitOrderState::Init;
        unit.prior_orders = UnitOrder::Await;
        unit.prior_state = UnitOrderState::ExecutingOrder;
    }

    process_transfer(&mut state, truck);

    let unit = state.unit(truck).expect("truck");
    assert_eq!(unit.orders, UnitOrder::Await);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert_eq!(unit.storage, 5);
    assert_eq!(state.unit(dump).expect("dump").storage, 5);
}

#[test]
fn transfer_cargo_kind_moves_fuel_and_gold_as_well() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let fuel = spawn_member(
        &mut state,
        UnitType::FuelStorage,
        Point::new(20, 20),
        Some(complex),
    );
    let gold = spawn_member(
        &mut state,
        UnitType::GoldVault,
        Point::new(22, 20),
        Some(complex),
    );
    state.unit_mut(fuel).expect("fuel").storage = 0;
    state.unit_mut(gold).expect("gold").storage = 0;

    transfer_cargo_kind(&mut state, fuel, CargoType::Fuel, 40);
    assert_eq!(state.unit(fuel).expect("fuel").storage, 40);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .fuel,
        40
    );

    transfer_cargo_kind(&mut state, gold, CargoType::Gold, 25);
    assert_eq!(state.unit(gold).expect("gold").storage, 25);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .gold,
        25
    );
    // The other resources were not touched.
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        0
    );
}

#[test]
fn transfer_cargo_kind_rejects_a_foreign_cargo_type() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let dump = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        Some(complex),
    );
    state.unit_mut(dump).expect("dump").storage = 0;

    // An ADUMP carries raw materials, so a fuel transfer is a no-op
    // (`if cargo_type != GetCargoType()` in `UnitInfo::TransferFuel`).
    transfer_cargo_kind(&mut state, dump, CargoType::Fuel, 10);
    assert_eq!(state.unit(dump).expect("dump").storage, 0);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .fuel,
        0
    );

    // The low-level `Complex::TransferCargo` has the same guard.
    let mut amount = 10;
    transfer_cargo(&mut state, dump, CargoType::Gold, &mut amount);
    assert_eq!(amount, 10, "not transferred");
    assert_eq!(state.unit(dump).expect("dump").storage, 0);
}

#[test]
fn process_transfer_without_a_parent_is_a_noop() {
    let mut state = test_state();
    let truck = state
        .spawn_unit(
            UnitType::SupplyTruck,
            TeamId::RED,
            Point::new(21, 21),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("truck");
    {
        let unit = state.unit_mut(truck).expect("truck");
        unit.storage = 10;
        unit.transfer_cargo = 5;
        unit.orders = UnitOrder::Transfer;
        unit.state = UnitOrderState::Init;
    }

    process_transfer(&mut state, truck);

    let unit = state.unit(truck).expect("truck");
    assert_eq!(unit.storage, 10, "nothing leaves without a parent");
    // The order is not restored: C++ returns before `RestoreOrders`.
    assert_eq!(unit.orders, UnitOrder::Transfer);
}

#[test]
fn transfer_menu_cargo_info_uses_the_right_cargo_type() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let raw = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        Some(complex),
    );
    let fuel = spawn_member(
        &mut state,
        UnitType::FuelStorage,
        Point::new(22, 20),
        Some(complex),
    );
    let gold = spawn_member(
        &mut state,
        UnitType::GoldVault,
        Point::new(24, 20),
        Some(complex),
    );
    state.unit_mut(raw).expect("raw").storage = 12;
    state.unit_mut(fuel).expect("fuel").storage = 7;
    state.unit_mut(gold).expect("gold").storage = 3;

    let truck = state
        .spawn_unit(
            UnitType::SupplyTruck,
            TeamId::RED,
            Point::new(21, 21),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("truck");

    // A foreign target: the whole complex pool and capacity per cargo type.
    assert_eq!(
        transfer_menu_cargo_info(&state, raw, truck, CargoType::Raw),
        (12, 50),
            "only ADUMP holds raw materials"
    );
    assert_eq!(
        transfer_menu_cargo_info(&state, raw, truck, CargoType::Fuel),
        (7, 50)
    );
    assert_eq!(
        transfer_menu_cargo_info(&state, raw, truck, CargoType::Gold),
        (3, 50)
    );
    assert_eq!(
        transfer_menu_cargo_info(&state, raw, truck, CargoType::None),
        (0, 0)
    );
    // Equal complexes: only the building's own storage and `ATTRIB_STORAGE`.
    assert_eq!(
        transfer_menu_cargo_info(&state, fuel, raw, CargoType::Fuel),
        (7, 50)
    );
}

#[test]
fn complex_free_capacity_and_income_helpers() {
    let mut state = test_state();
    let complex = new_complex(&mut state);
    let mine = spawn_member(
        &mut state,
        UnitType::MiningStation,
        Point::new(20, 20),
        Some(complex),
    );
    let dump = spawn_member(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 20),
        Some(complex),
    );
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.raw_mining = 5;
        unit.fuel_mining = 2;
        unit.gold_mining = 1;
        unit.total_mining = 8;
        unit.storage = 4;
    }
    state.unit_mut(dump).expect("dump").storage = 6;

    assert_eq!(
        complex_free_capacity(&state, TeamId::RED, complex, CargoType::Raw),
        75 - 10,
            "capacity 25 (mine) + 50 (ADUMP) minus 10 stored"
    );
    assert_eq!(
        complex_income(&state, TeamId::RED, complex, CargoType::Raw),
        5
    );
    assert_eq!(
        complex_income(&state, TeamId::RED, complex, CargoType::Fuel),
        2
    );
    assert_eq!(
        complex_income(&state, TeamId::RED, complex, CargoType::Gold),
        1
    );
    assert_eq!(
        complex_stored(&state, TeamId::RED, complex, CargoType::Raw),
        10
    );
}
