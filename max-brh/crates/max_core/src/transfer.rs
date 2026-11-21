//! Перенос груза: `TransferMenu` и `UnitInfo::ProcessTransfer`/`TransferRaw`.
//!
//! Оракул: `max-cpp-port/src/transfermenu.cpp`,
//! `max-cpp-port/src/unitinfo.cpp` (`TransferRaw`/`TransferFuel`/`TransferGold`,
//! `ProcessTransfer`), `max-cpp-port/src/complex.cpp`
//! (`Complex::Transfer`/`TransferCargo`/`GetCargoInfo`),
//! `max-cpp-port/src/game_manager.cpp` (`GameManager_TransferCargo`,
//! `GameManager_GetUnitWithCargoType`).
//!
//! # Модель
//!
//! Комплекс — `(TeamId, ComplexId)`; его запас лежит в
//! `Complex::{material,fuel,gold}` и одновременно в `storage` зданий-членов
//! (см. [`crate::alloc::get_cargo_info`]). Перенос поэтому всегда делает две
//! вещи, как C++: меняет запас здания (`unit.storage`) и поле комплекса.
//!
//! * [`find_unit_with_cargo_type`] — `GameManager_GetUnitWithCargoType`: при
//!   клике по любому стационарному зданию комплекса C++ заменяет цель на
//!   здание с подходящим типом груза.
//! * [`transfer_cargo`] — `Complex::TransferCargo`: залить/забрать до
//!   `amount` единиц в `storage` одного здания (лимит `ATTRIB_STORAGE`).
//! * [`complex_transfer`] — `Complex::Transfer`: изменить поля комплекса и
//!   разложить изменение по зданиям-членам **в порядке
//!   `UnitsManager_StationaryUnits`** (детерминизм: порядок списка — часть
//!   состояния).
//! * [`transfer_cargo_kind`] — `UnitInfo::TransferRaw`/`TransferFuel`/
//!   `TransferGold`: `storage += amount`, запас комплекса, а излишек сверх
//!   `ATTRIB_STORAGE` уходит назад в комплекс через `Complex::Transfer`.
//! * [`process_transfer`] — `UnitInfo::ProcessTransfer` (`ORDER_TRANSFER`):
//!   знак `transfer_cargo` задаёт направление (плюс — из юнита в родителя,
//!   минус — из родителя в юнит), затем `RestoreOrders`.
//!
//! # Переполнение
//!
//! Ёмкость склада здания — `ATTRIB_STORAGE` (`ADUMP`/`FDUMP`/`GOLDSM` = 50,
//! `MININGST` = 25, `CONSTRCT` = 60, `FUELTRCK`/`GOLDTRIK` = 50, …). Когда
//! `UnitInfo::UpdateProduction` (см. [`crate::game::GameState::update_production`])
//! переносит запас комплекса в здания, каждое берёт
//! `min(complex.material, ATTRIB_STORAGE)`; остаток остаётся в поле комплекса и
//! на следующем ходу `Access_UpdateResourcesTotal` пересчитывает его из
//! инвентаря зданий — то есть **излишек за ёмкость складов теряется**, как в
//! C++ (в `Complex` поля — только кэш на текущий ход).

use crate::alloc::{get_cargo_info, get_cargo_mining};
use crate::game::GameState;
use crate::team::{Cargo, ComplexId, TeamId};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::unit_values::ATTRIB_STORAGE;
use crate::victory::{cargo_type_of, CargoType};

/// `GameManager_GetUnitWithCargoType` (`game_manager.cpp:5964`): первое
/// стационарное здание комплекса с типом груза `cargo_type`.
///
/// Порядок — `UnitsManager_StationaryUnits` (`UnitListKind::Stationary`), как
/// в C++; команда не проверяется, потому что `ComplexId` уникален только
/// внутри команды и вызывающий передаёт комплекс уже найденного здания.
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
/// `amount` — входной/выходной параметр: сколько удалось перенести. При
/// `amount >= 0` здание наполняется до `ATTRIB_STORAGE`; при отрицательном —
/// отдаёт из своего `storage`, пока он не кончится.
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
        // C++ `int16_t`-поле: присваивание усекает (значения малы, но
        // поведение сохранено).
        unit.storage = new_storage as i16;
    }
}

/// Поле комплекса, которое меняет ресурс (`Complex::{material,fuel,gold}`).
fn complex_field(state: &mut GameState, team: TeamId, complex: ComplexId, cargo: CargoType) -> Option<&mut i16> {
    let target = state.complex_mut(team, complex.0 as u16)?;

    Some(match cargo {
        CargoType::Raw => &mut target.material,
        CargoType::Fuel => &mut target.fuel,
        CargoType::Gold => &mut target.gold,
        _ => return None,
    })
}

/// `Complex::Transfer(raw, fuel, gold)` (`complex.cpp:178`).
///
/// Сначала меняются поля комплекса, затем **в порядке стационарного списка**
/// каждому зданию-члену с подходящим типом груза достаётся/списывается доля
/// (`Complex::TransferCargo`). Так же раскладывается излишек `TransferRaw`.
pub fn complex_transfer(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    mut raw: i32,
    mut fuel: i32,
    mut gold: i32,
) {
    if let Some(target) = state.complex_mut(team, complex.0 as u16) {
        target.material = target.material.wrapping_add(raw as i16);
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
/// Публичная обёртка: `cargo` должен совпадать с типом груза юнита, иначе
/// ничего не происходит (как `if (GetCargoType() == ...)` в C++).
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

/// `UnitInfo::ProcessTransfer` (`unitinfo.cpp:5294`) — процессор
/// `ORDER_TRANSFER` (`UnitsManager_ProcessOrderTransfer`).
///
/// `unit.transfer_cargo` — знак и количество: `> 0` — из юнита в родителя,
/// `< 0` — из родителя в юнит. Точная последовательность C++: сначала
/// `target->Transfer<Kind>(amount)`, затем списание/добавление у источника,
/// при нехватке `storage` — `Complex::Transfer` (для направления из
/// комплекса) и компенсация поля комплекса.
///
/// В конце — `RestoreOrders`; целевой задаче шлётся `EventCargoTransfer` (в
/// порту задач это событие не реализовано, см. `ai`).
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

    // `target->GetTask()->EventCargoTransfer(*target)` — задачи ИИ в порту не
    // хранят событий груза; приказ уже завершён.
}

/// `TransferMenu_GetUnitCargoInfo` (`transfermenu.cpp:37`).
///
/// Если у `source` есть комплекс, отличный от комплекса `destination`, меню
/// показывает **весь запас комплекса** (`Complex::GetCargoInfo`); иначе —
/// только `storage` здания и его `ATTRIB_STORAGE`. Возвращает
/// `(materials, capacity)` по типу груза `cargo`.
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

/// Запас комплекса по типу груза — для QA/отладочных проверок.
pub fn complex_stored(state: &GameState, team: TeamId, complex: ComplexId, cargo: CargoType) -> i32 {
    let (materials, _) = get_cargo_info(state, team, complex);

    match cargo {
        CargoType::Raw => materials.raw,
        CargoType::Fuel => materials.fuel,
        CargoType::Gold => materials.gold,
        _ => 0,
    }
}

/// Свободная ёмкость комплекса по типу груза — сумма `ATTRIB_STORAGE` всех
/// зданий-членов с этим типом груза минус занятое (`Complex::GetCargoInfo`).
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

/// Суммарная добыча комплекса за ход по типу груза (для QA лимитов
/// хранилищ): `Complex::GetCargoMining` → `materials`.
pub fn complex_income(state: &GameState, team: TeamId, complex: ComplexId, cargo: CargoType) -> i32 {
    let (materials, _) = get_cargo_mining(state, team, complex);

    match cargo {
        CargoType::Raw => materials.raw,
        CargoType::Fuel => materials.fuel,
        CargoType::Gold => materials.gold,
        _ => 0,
    }
}

/// Груз юнита одним `Cargo` (для тестов/QA).
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

    /// Спавн здания с переносом его комплекса в `complex` (в порте каждое
    /// здание получает свой комплекс: слияния по коннекторам нет).
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
                .material,
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
            target.material = 20;
        }

        complex_transfer(&mut state, TeamId::RED, complex, -15, 0, 0);

        assert_eq!(state.unit(first).expect("first").storage, 0);
        assert_eq!(state.unit(second).expect("second").storage, 5);
        assert_eq!(
            state
                .complex(TeamId::RED, complex.0 as u16)
                .expect("complex")
                .material,
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

        // Мобильный грузовик вне комплекса.
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
                .material,
            15
        );

        // Обратно: минус — из комплекса в юнит.
        state.unit_mut(truck).expect("truck").transfer_cargo = -10;
        process_transfer(&mut state, truck);

        assert_eq!(state.unit(truck).expect("truck").storage, 35);
        assert_eq!(state.unit(dump).expect("dump").storage, 5);
        assert_eq!(
            state
                .complex(TeamId::RED, complex.0 as u16)
                .expect("complex")
                .material,
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

        // Первое здание отдало 10, затем `Complex::Transfer` взял 25 из второго.
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
            target.material = 7;
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

        // Чужой (без комплекса) получатель — весь запас/ёмкость комплекса
        // источника (два ADUMP: 50 + 50).
        assert_eq!(
            transfer_menu_cargo_info(&state, dump, truck, CargoType::Raw),
            (7, 100)
        );
        // Свой комплекс — весь запас/ёмкость.
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
