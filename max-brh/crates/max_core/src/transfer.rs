//! Cargo transfer: `TransferMenu` and `UnitInfo::ProcessTransfer`/`TransferRaw`.
//!
//! Oracle: `max-cpp-port/src/transfermenu.cpp`,
//! `max-cpp-port/src/unitinfo.cpp` (`TransferRaw`/`TransferFuel`/`TransferGold`,
//! `ProcessTransfer`), `max-cpp-port/src/complex.cpp`
//! (`Complex::Transfer`/`TransferCargo`/`GetCargoInfo`),
//! `max-cpp-port/src/game_manager.cpp` (`GameManager_TransferCargo`,
//! `GameManager_GetUnitWithCargoType`).
//!
//! # Model
//!
//! A complex is `(TeamId, ComplexId)`; its stock lives in
//! `Complex::{raw,fuel,gold}` and at the same time in the `storage` of member buildings
//! (see [`crate::alloc::get_cargo_info`]). A transfer therefore always does two
//! things, as in C++: changes the building's stock (`unit.storage`) and the complex field.
//!
//! * [`find_unit_with_cargo_type`] — `GameManager_GetUnitWithCargoType`: when
//!   any stationary complex building is clicked, C++ replaces the target with
//!   a building of the matching cargo type.
//! * [`transfer_cargo`] — `Complex::TransferCargo`: pour in/take out up to
//!   `amount` units to/from one building's `storage` (limit `ATTRIB_STORAGE`).
//! * [`complex_transfer`] — `Complex::Transfer`: change the complex fields and
//!   distribute the change over member buildings **in
//!   `UnitsManager_StationaryUnits` order** (determinism: the list order is part of
//!   the state).
//! * [`transfer_cargo_kind`] — `UnitInfo::TransferRaw`/`TransferFuel`/
//!   `TransferGold`: `storage += amount`, the complex stock, and the excess over
//!   `ATTRIB_STORAGE` goes back to the complex via `Complex::Transfer`.
//! * [`process_transfer`] — `UnitInfo::ProcessTransfer` (`ORDER_TRANSFER`):
//!   the sign of `transfer_cargo` sets the direction (plus — from the unit to the parent,
//!   minus — from the parent to the unit), then `RestoreOrders`.
//!
//! # Overflow
//!
//! A building's storage capacity is `ATTRIB_STORAGE` (`ADUMP`/`FDUMP`/`GOLDSM` = 50,
//! `MININGST` = 25, `CONSTRCT` = 60, `FUELTRCK`/`GOLDTRIK` = 50, …). When
//! `UnitInfo::UpdateProduction` (see [`crate::game::GameState::update_production`])
//! moves the complex stock into buildings, each takes
//! `min(complex.raw, ATTRIB_STORAGE)`; the remainder stays in the complex field and
//! on the next turn `Access_UpdateResourcesTotal` recomputes it from
//! the buildings' inventory — i.e. **the excess beyond storage capacity is lost**, as in
//! C++ (the `Complex` fields are only a cache for the current turn).

use crate::alloc::{get_cargo_info, get_cargo_mining};
use crate::game::GameState;
use crate::team::{Cargo, ComplexId, TeamId};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::unit_values::ATTRIB_STORAGE;
use crate::victory::{cargo_type_of, CargoType};

/// `GameManager_GetUnitWithCargoType` (`game_manager.cpp:5964`): the first
/// stationary complex building with cargo type `cargo_type`.
///
/// The order is `UnitsManager_StationaryUnits` (`UnitListKind::Stationary`), as
/// in C++; the team is not checked because `ComplexId` is unique only
/// within a team and the caller passes the complex of an already found building.
pub fn find_unit_with_cargo_type(
    state: &GameState,
    team: TeamId,
    complex: ComplexId,
    cargo_type: CargoType,
) -> Option<UnitId> {
    state
        .unit_list(UnitListKind::Stationary)
        .iter()
        .copied()
        .find(|id| {
            state.unit(*id).is_some_and(|unit| {
                unit.team == team
                    && unit.complex == Some(complex)
                    && cargo_type_of(unit.unit_type) == cargo_type
            })
        })
}

/// `Complex::TransferCargo(unit, &cargo)` (`complex.cpp:149`).
///
/// `amount` is an in/out parameter: how much was transferred. With
/// `amount >= 0` the building is filled up to `ATTRIB_STORAGE`; with a negative one it
/// gives from its `storage` until it runs out.
pub fn transfer_cargo(
    state: &mut GameState,
    unit_id: UnitId,
    cargo_type: CargoType,
    amount: &mut i32,
) {
    let Some(unit) = state.unit(unit_id).cloned() else {
        return;
    };

    if cargo_type_of(unit.unit_type) != cargo_type {
        return;
    }

    let capacity = unit.base_values.get_attribute(ATTRIB_STORAGE);
    let storage = i32::from(unit.storage);
    let new_storage;

    if *amount >= 0 {
        let free = capacity - storage;
        if *amount <= free {
            new_storage = storage + *amount;
            *amount = 0;
        } else {
            new_storage = storage + free;
            *amount -= free;
        }
    } else if -*amount <= storage {
        new_storage = storage + *amount;
        *amount = 0;
    } else {
        *amount += storage;
        new_storage = 0;
    }

    if let Some(unit) = state.unit_mut(unit_id) {
        // C++ `int16_t` field: assignment truncates (the values are small, but
        // the behavior is preserved).
        unit.storage = new_storage as i16;
    }
}

/// The complex field changed by a resource (`Complex::{raw,fuel,gold}`).
fn complex_field(state: &mut GameState, team: TeamId, complex: ComplexId, cargo: CargoType) -> Option<&mut i16> {
    let target = state.complex_mut(team, complex.0 as u16)?;

    Some(match cargo {
        CargoType::Raw => &mut target.raw,
        CargoType::Fuel => &mut target.fuel,
        CargoType::Gold => &mut target.gold,
        _ => return None,
    })
}

/// `Complex::Transfer(raw, fuel, gold)` (`complex.cpp:178`).
///
/// First the complex fields change, then **in stationary-list order**
/// each member building with the matching cargo type receives/is charged a share
/// (`Complex::TransferCargo`). The `TransferRaw` excess is distributed the same way.
pub fn complex_transfer(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    mut raw: i32,
    mut fuel: i32,
    mut gold: i32,
) {
    if let Some(target) = state.complex_mut(team, complex.0 as u16) {
        target.raw = target.raw.wrapping_add(raw as i16);
        target.fuel = target.fuel.wrapping_add(fuel as i16);
        target.gold = target.gold.wrapping_add(gold as i16);
    }

    let ids = state.unit_list(UnitListKind::Stationary).to_vec();

    for id in ids {
        if raw == 0 && fuel == 0 && gold == 0 {
            continue;
        }

        let Some(unit) = state.unit(id) else {
            continue;
        };

        if unit.team != team || unit.complex != Some(complex) {
            continue;
        }

        match cargo_type_of(unit.unit_type) {
            CargoType::Raw => transfer_cargo(state, id, CargoType::Raw, &mut raw),
            CargoType::Fuel => transfer_cargo(state, id, CargoType::Fuel, &mut fuel),
            CargoType::Gold => transfer_cargo(state, id, CargoType::Gold, &mut gold),
            _ => {}
        }
    }
}

/// `UnitInfo::TransferRaw` / `TransferFuel` / `TransferGold`
/// (`unitinfo.cpp:990/1056/1124`).
///
/// Public wrapper: `cargo` must match the unit's cargo type, otherwise
/// nothing happens (like `if (GetCargoType() == ...)` in C++).
pub fn transfer_cargo_kind(
    state: &mut GameState,
    unit_id: UnitId,
    cargo: CargoType,
    amount: i32,
) {
    let Some(unit) = state.unit(unit_id).cloned() else {
        return;
    };

    if cargo_type_of(unit.unit_type) != cargo {
        return;
    }

    let capacity = unit.base_values.get_attribute(ATTRIB_STORAGE);
    let new_storage = i32::from(unit.storage) + amount;

    if let Some(target) = state.unit_mut(unit_id) {
        target.storage = new_storage as i16;
    }

    let Some(complex) = unit.complex else {
        return;
    };

    if let Some(field) = complex_field(state, unit.team, complex, cargo) {
        *field = field.wrapping_add(amount as i16);
    }

    if new_storage > capacity {
        let overflow = new_storage - capacity;

        if let Some(target) = state.unit_mut(unit_id) {
            target.storage = capacity as i16;
        }

        if let Some(field) = complex_field(state, unit.team, complex, cargo) {
            *field = field.wrapping_sub(overflow as i16);
        }

        match cargo {
            CargoType::Raw => complex_transfer(state, unit.team, complex, overflow, 0, 0),
            CargoType::Fuel => complex_transfer(state, unit.team, complex, 0, overflow, 0),
            CargoType::Gold => complex_transfer(state, unit.team, complex, 0, 0, overflow),
            _ => {}
        }
    }
}

fn add_complex_field(state: &mut GameState, team: TeamId, complex: ComplexId, cargo: CargoType, delta: i32) {
    if let Some(field) = complex_field(state, team, complex, cargo) {
        *field = field.wrapping_add(delta as i16);
    }
}

fn set_unit_storage(state: &mut GameState, unit_id: UnitId, storage: i16) {
    if let Some(unit) = state.unit_mut(unit_id) {
        unit.storage = storage;
    }
}

fn unit_storage(state: &GameState, unit_id: UnitId) -> i16 {
    state.unit(unit_id).map_or(0, |unit| unit.storage)
}

/// `UnitInfo::ProcessTransfer` (`unitinfo.cpp:5294`) — the processor
/// `ORDER_TRANSFER` (`UnitsManager_ProcessOrderTransfer`).
///
/// `unit.transfer_cargo` — sign and amount: `> 0` — from the unit to the parent,
/// `< 0` — from the parent to the unit. The exact C++ sequence: first
/// `target->Transfer<Kind>(amount)`, then deduction/addition at the source,
/// on a `storage` shortage — `Complex::Transfer` (for the direction from
/// the complex) and compensation of the complex field.
///
/// At the end — `RestoreOrders`; the target task receives `EventCargoTransfer` (in
/// the task port this event is not implemented, see `ai`).
pub fn process_transfer(state: &mut GameState, unit_id: UnitId) {
    let Some(unit) = state.unit(unit_id).cloned() else {
        return;
    };
    let Some(parent) = unit.parent_unit else {
        return;
    };

    let cargo = cargo_type_of(unit.unit_type);
    if !matches!(cargo, CargoType::Raw | CargoType::Fuel | CargoType::Gold) {
        return;
    }

    let mut source = unit_id;
    let mut target = parent;
    let mut amount = i32::from(unit.transfer_cargo);

    if amount < 0 {
        core::mem::swap(&mut source, &mut target);
        amount = amount.abs();
    }

    let source_unit = state.unit(source).cloned();
    let source_complex = source_unit.as_ref().and_then(|unit| unit.complex);
    let source_team = source_unit.as_ref().map(|unit| unit.team);

    // `target->TransferRaw/Fuel/Gold(transfer_amount)`.
    transfer_cargo_kind(state, target, cargo, amount);

    // `if (source->GetComplex()) source->GetComplex()->material -= transfer_amount;`
    if let (Some(complex), Some(team)) = (source_complex, source_team) {
        add_complex_field(state, team, complex, cargo, -amount);
    }

    let storage = i32::from(unit_storage(state, source));

    if storage >= amount {
        set_unit_storage(state, source, (storage - amount) as i16);
    } else {
        let rest = amount - storage;
        set_unit_storage(state, source, 0);

        if let (Some(complex), Some(team)) = (source_complex, source_team) {
            match cargo {
                CargoType::Raw => complex_transfer(state, team, complex, -rest, 0, 0),
                CargoType::Fuel => complex_transfer(state, team, complex, 0, -rest, 0),
                CargoType::Gold => complex_transfer(state, team, complex, 0, 0, -rest),
                _ => {}
            }

            add_complex_field(state, team, complex, cargo, rest);
        }
    }

    if let Some(unit) = state.unit_mut(unit_id) {
        crate::unit_order::restore_orders(unit);
    }

    // `target->GetTask()->EventCargoTransfer(*target)` — AI tasks in the port do not
    // store cargo events; the order is already complete.
}

/// `TransferMenu_GetUnitCargoInfo` (`transfermenu.cpp:37`).
///
/// If `source` has a complex different from the `destination` complex, the menu
/// shows the **whole complex stock** (`Complex::GetCargoInfo`); otherwise —
/// only the building's `storage` and its `ATTRIB_STORAGE`. Returns
/// `(materials, capacity)` for cargo type `cargo`.
pub fn transfer_menu_cargo_info(
    state: &GameState,
    source: UnitId,
    destination: UnitId,
    cargo: CargoType,
) -> (i32, i32) {
    let (Some(source_unit), Some(destination_unit)) =
        (state.unit(source), state.unit(destination))
    else {
        return (0, 0);
    };

    let destination_complex = destination_unit
        .complex
        .map(|complex| (destination_unit.team, complex));

    if let Some(complex) = source_unit.complex {
        if Some((source_unit.team, complex)) != destination_complex {
            let (materials, capacity) = get_cargo_info(state, source_unit.team, complex);

            return match cargo {
                CargoType::Raw => (materials.raw, capacity.raw),
                CargoType::Fuel => (materials.fuel, capacity.fuel),
                CargoType::Gold => (materials.gold, capacity.gold),
                _ => (0, 0),
            };
        }
    }

    (
        i32::from(source_unit.storage),
        source_unit.base_values.get_attribute(ATTRIB_STORAGE),
    )
}

/// Complex stock by cargo type — for QA/debug checks.
pub fn complex_stored(state: &GameState, team: TeamId, complex: ComplexId, cargo: CargoType) -> i32 {
    let (materials, _) = get_cargo_info(state, team, complex);

    match cargo {
        CargoType::Raw => materials.raw,
        CargoType::Fuel => materials.fuel,
        CargoType::Gold => materials.gold,
        _ => 0,
    }
}

/// Free complex capacity by cargo type — the sum of `ATTRIB_STORAGE` of all
/// member buildings with this cargo type minus the occupied amount (`Complex::GetCargoInfo`).
pub fn complex_free_capacity(
    state: &GameState,
    team: TeamId,
    complex: ComplexId,
    cargo: CargoType,
) -> i32 {
    let (materials, capacity) = get_cargo_info(state, team, complex);

    let (stored, total) = match cargo {
        CargoType::Raw => (materials.raw, capacity.raw),
        CargoType::Fuel => (materials.fuel, capacity.fuel),
        CargoType::Gold => (materials.gold, capacity.gold),
        _ => (0, 0),
    };

    total - stored
}

/// Total complex production per turn by cargo type (for QA of storage
/// limits): `Complex::GetCargoMining` → `materials`.
pub fn complex_income(state: &GameState, team: TeamId, complex: ComplexId, cargo: CargoType) -> i32 {
    let (materials, _) = get_cargo_mining(state, team, complex);

    match cargo {
        CargoType::Raw => materials.raw,
        CargoType::Fuel => materials.fuel,
        CargoType::Gold => materials.gold,
        _ => 0,
    }
}

/// The unit's cargo as one `Cargo` (for tests/QA).
pub fn unit_cargo(unit: &UnitInfo) -> Cargo {
    crate::alloc::cargo_get_inventory(unit)
}

#[cfg(test)]
mod tests {
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
    fn complex_transfer_negative_drains_members_in_order() {
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

        state.unit_mut(first).expect("first").storage = 10;
        state.unit_mut(second).expect("second").storage = 10;
        if let Some(target) = state.complex_mut(TeamId::RED, complex.0 as u16) {
            target.raw = 20;
        }

        complex_transfer(&mut state, TeamId::RED, complex, -15, 0, 0);

        assert_eq!(state.unit(first).expect("first").storage, 0);
        assert_eq!(state.unit(second).expect("second").storage, 5);
        assert_eq!(
            state
                .complex(TeamId::RED, complex.0 as u16)
                .expect("complex")
                .raw,
            5
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
}
