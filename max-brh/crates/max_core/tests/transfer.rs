//! Сквозные тесты переноса груза и учёта запасов комплекса.
//!
//! Оракул: `max-cpp-port/src/unitinfo.cpp` (`ProcessTransfer`,
//! `TransferRaw/Fuel/Gold`, `UpdateProduction`, `Resupply`),
//! `complex.cpp` (`Transfer`, `TransferCargo`, `GetCargoInfo`),
//! `access.cpp` (`Access_UpdateResourcesTotal`),
//! `game_manager.cpp` (`ManageEconomy`, `GameManager_TransferCargo`).

use max_core::alloc::{get_cargo_info, update_resources_total};
use max_core::enums::{unit_flags, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::Point;
use max_core::team::{ComplexId, TeamId, TeamKind, TeamUnits};
use max_core::transfer::{
    complex_free_capacity, complex_transfer, find_unit_with_cargo_type, process_transfer,
    transfer_cargo_kind, transfer_menu_cargo_info,
};
use max_core::unit::{UnitId, UnitInfo, UnitListKind};
use max_core::victory::CargoType;

fn test_state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

/// Спавн здания и **принудительный** перенос его комплекса в `target`.
///
/// Боевой путь слияния — `UnitsManager_UpdateConnectors` (см. `complex.rs`):
/// соседние здания сливаются сами. Хелпер нужен тестам, где здания стоят
/// далеко друг от друга, а комплекс всё равно должен быть общим.
fn spawn_in_complex(
    state: &mut GameState,
    unit_type: UnitType,
    grid: Point,
    target: Option<ComplexId>,
) -> UnitId {
    let id = state
        .spawn_unit(
            unit_type,
            TeamId::RED,
            grid,
            unit_flags::STATIONARY | unit_flags::BUILDING,
        )
        .expect("spawn");

    if let Some(target) = target {
        if let Some(own) = state.unit(id).and_then(|unit| unit.complex) {
            if own != target {
                state.team_units[0].remove_complex(own);
            }
        }
        state.unit_mut(id).expect("unit").complex = Some(target);
        if let Some(complex) = state.team_units[0].get_complex_mut(target) {
            complex.grow();
        }
    }

    id
}

fn spawn_truck(state: &mut GameState, unit_type: UnitType, grid: Point) -> UnitId {
    let id = state
        .spawn_unit(
            unit_type,
            TeamId::RED,
            grid,
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("truck");
    state.unit_mut(id).expect("truck").storage = 0;
    id
}

fn complex_of(state: &GameState, id: UnitId) -> ComplexId {
    state.unit(id).expect("unit").complex.expect("complex")
}

fn storage(state: &GameState, id: UnitId) -> i16 {
    state.unit(id).expect("unit").storage
}

// ---------------------------------------------------------------------------
// Перенос
// ---------------------------------------------------------------------------

#[test]
fn transfer_menu_info_matches_get_cargo_info_for_own_complex() {
    let mut state = test_state();
    let mine = spawn_in_complex(
        &mut state,
        UnitType::MiningStation,
        Point::new(20, 20),
        None,
    );
    let complex = complex_of(&state, mine);
    let dump = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 24),
        Some(complex),
    );

    state.unit_mut(mine).expect("mine").storage = 4;
    state.unit_mut(dump).expect("dump").storage = 11;

    // Цель — юнит вне комплекса: источник показывает весь запас комплекса.
    let truck = spawn_truck(&mut state, UnitType::SupplyTruck, Point::new(21, 21));

    assert_eq!(
        transfer_menu_cargo_info(&state, mine, truck, CargoType::Raw),
        (15, 75),
        "mine 25 + ADUMP 50"
    );
    assert_eq!(
        transfer_menu_cargo_info(&state, truck, mine, CargoType::Raw),
        (0, 50),
        "у грузовика нет комплекса — только storage/ATTRIB_STORAGE"
    );
    assert_eq!(
        complex_free_capacity(&state, TeamId::RED, complex, CargoType::Raw),
        60
    );
}

#[test]
fn xfer_takes_materials_from_the_whole_connected_complex() {
    let mut state = test_state();
    let mine = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), 0)
        .expect("mine");
    // Шахта 2×2 (20..=21), `ADUMP` — 1×1 `STANDALONE` вплотную справа:
    // `UpdateConnectors` сливает их в один комплекс.
    let dump = state
        .spawn_unit(
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(22, 20),
            0,
        )
        .expect("MaterialStorage");

    let mine_complex = complex_of(&state, mine);
    assert_eq!(
        state.unit(dump).expect("MaterialStorage").complex,
        Some(mine_complex),
        "шахта и ADUMP — один комплекс"
    );

    state.unit_mut(mine).expect("mine").storage = 20;
    state.unit_mut(dump).expect("MaterialStorage").storage = 0;
    update_resources_total(&mut state, TeamId::RED, mine_complex);

    let truck = spawn_truck(&mut state, UnitType::SupplyTruck, Point::new(23, 20));

    // `TransferMenu_GetUnitCargoInfo`: у источника комплекс, отличный от
    // комплекса цели (у грузовика его нет) — показывается весь котёл.
    assert_eq!(
        transfer_menu_cargo_info(&state, dump, truck, CargoType::Raw),
        (20, 75),
        "запас комплекса 20, ёмкость 25 (шахта) + 50 (ADUMP)"
    );

    // `ProcessTransfer`: минус — из комплекса в грузовик.
    {
        let unit = state.unit_mut(truck).expect("truck");
        unit.parent_unit = Some(dump);
        unit.transfer_cargo = -12;
        unit.orders = UnitOrder::Transfer;
        unit.state = UnitOrderState::Init;
        unit.prior_orders = UnitOrder::Await;
        unit.prior_state = UnitOrderState::ExecutingOrder;
    }

    process_transfer(&mut state, truck);

    assert_eq!(storage(&state, truck), 12);
    assert_eq!(storage(&state, dump), 0, "пустой склад ничего не отдал");
    assert_eq!(
        storage(&state, mine),
        8,
        "остаток взят из шахты того же комплекса"
    );
    assert_eq!(
        get_cargo_info(&state, TeamId::RED, mine_complex).0.raw,
        8,
        "общий котёл комплекса уменьшился на 12"
    );
}

#[test]
fn transfer_to_complex_and_back_changes_both_sides() {
    let mut state = test_state();
    let mine = spawn_in_complex(
        &mut state,
        UnitType::MiningStation,
        Point::new(20, 20),
        None,
    );
    let complex = complex_of(&state, mine);
    let dump = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 24),
        Some(complex),
    );
    state.unit_mut(dump).expect("dump").storage = 0;

    let truck = spawn_truck(&mut state, UnitType::SupplyTruck, Point::new(21, 21));
    state.unit_mut(truck).expect("truck").storage = 30;

    // Положительное направление: юнит -> родитель (склад).
    {
        let unit = state.unit_mut(truck).expect("truck");
        unit.parent_unit = Some(dump);
        unit.transfer_cargo = 12;
    }
    process_transfer(&mut state, truck);

    assert_eq!(storage(&state, truck), 18);
    assert_eq!(storage(&state, dump), 12);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        12
    );

    // Отрицательное: комплекс -> юнит.
    state.unit_mut(truck).expect("truck").transfer_cargo = -5;
    process_transfer(&mut state, truck);

    assert_eq!(storage(&state, truck), 23);
    assert_eq!(storage(&state, dump), 7);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        7
    );
}

#[test]
fn transfer_order_processor_runs_restores_orders_and_is_deterministic() {
    fn run() -> (i16, i16, i16) {
        let mut state = test_state();
        let mine = spawn_in_complex(
            &mut state,
            UnitType::MiningStation,
            Point::new(20, 20),
            None,
        );
        let complex = complex_of(&state, mine);
        let dump = spawn_in_complex(
            &mut state,
            UnitType::MaterialStorage,
            Point::new(22, 24),
            Some(complex),
        );
        state.unit_mut(dump).expect("dump").storage = 0;

        let truck = spawn_truck(&mut state, UnitType::SupplyTruck, Point::new(21, 21));
        {
            let unit = state.unit_mut(truck).expect("truck");
            unit.storage = 20;
            unit.parent_unit = Some(dump);
            unit.transfer_cargo = 9;
            unit.orders = UnitOrder::Transfer;
            unit.state = UnitOrderState::Init;
            unit.prior_orders = UnitOrder::Await;
            unit.prior_state = UnitOrderState::ExecutingOrder;
        }

        max_core::unit_order::process_order(&mut state, truck);

        let unit = state.unit(truck).expect("truck");
        assert_eq!(
            unit.orders,
            UnitOrder::Await,
            "RestoreOrders после переноса"
        );
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        (
            unit.storage,
            storage(&state, dump),
            state
                .complex(TeamId::RED, complex.0 as u16)
                .expect("complex")
                .raw,
        )
    }

    let first = run();
    let second = run();
    assert_eq!(first, (11, 9, 9));
    assert_eq!(first, second, "перенос детерминирован");
}

#[test]
fn find_unit_with_cargo_type_follows_stationary_order() {
    let mut state = test_state();
    let mine = spawn_in_complex(
        &mut state,
        UnitType::MiningStation,
        Point::new(20, 20),
        None,
    );
    let complex = complex_of(&state, mine);
    let first = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 20),
        Some(complex),
    );
    let second = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 24),
        Some(complex),
    );

    // Список стационарных — по y; одинаковый y вставляет новое здание раньше.
    let found = find_unit_with_cargo_type(&state, TeamId::RED, complex, CargoType::Raw);
    assert!(found == Some(first) || found == Some(second));
    assert_eq!(
        find_unit_with_cargo_type(&state, TeamId::RED, complex, CargoType::Fuel),
        None
    );
    assert_eq!(
        find_unit_with_cargo_type(&state, TeamId::RED, complex, CargoType::Raw)
            .and_then(|id| state.unit(id))
            .map(|unit| unit.unit_type),
        Some(UnitType::MaterialStorage)
    );
}

// ---------------------------------------------------------------------------
// Хранилища: ёмкости и переполнение
// ---------------------------------------------------------------------------

#[test]
fn dump_capacities_match_cpp_attribute_storage() {
    let mut state = test_state();
    let adump = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        None,
    );
    let fdump = spawn_in_complex(&mut state, UnitType::FuelStorage, Point::new(22, 20), None);
    let goldsm = spawn_in_complex(&mut state, UnitType::GoldVault, Point::new(24, 20), None);

    for (id, expected) in [(adump, 50), (fdump, 50), (goldsm, 50)] {
        let unit = state.unit(id).expect("dump");
        assert_eq!(
            unit.base_values
                .get_attribute(max_core::unit_values::ATTRIB_STORAGE),
            expected,
            "{:?} storage",
            unit.unit_type
        );
    }
}

#[test]
fn mining_income_is_pulled_into_the_mine_and_then_the_dump() {
    let mut state = test_state();
    let mine = spawn_in_complex(
        &mut state,
        UnitType::MiningStation,
        Point::new(20, 20),
        None,
    );
    let complex = complex_of(&state, mine);
    let dump = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 24),
        Some(complex),
    );

    // 10 сырья за ход; шахта хранит до 25, ADUMP — до 50.
    let unit = state.unit_mut(mine).expect("mine");
    unit.orders = UnitOrder::PowerOn;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.prior_orders = UnitOrder::PowerOn;
    unit.raw_mining = 10;
    unit.total_mining = 10;
    unit.storage = 0;
    state.unit_mut(dump).expect("dump").storage = 0;

    // 1-й ход: доход 10 -> шахта (её очередь раньше в списке).
    state.manage_economy(TeamId::RED);
    assert_eq!(storage(&state, mine), 10);
    assert_eq!(storage(&state, dump), 0);

    // 2-й ход: 10 (инвентарь) + 10 (добыча) = 20 в комплексе.
    state.manage_economy(TeamId::RED);
    assert_eq!(storage(&state, mine), 20);

    // 3-й ход: шахта заполнена до 25, излишки уходят в ADUMP.
    state.manage_economy(TeamId::RED);
    assert_eq!(storage(&state, mine), 25);
    assert_eq!(storage(&state, dump), 5);

    let (materials, capacity) = get_cargo_info(&state, TeamId::RED, complex);
    assert_eq!(materials.raw, 30);
    assert_eq!(capacity.raw, 75);
}

#[test]
fn mining_overflow_is_lost_when_complex_capacity_is_exhausted() {
    let mut state = test_state();
    let mine = spawn_in_complex(
        &mut state,
        UnitType::MiningStation,
        Point::new(20, 20),
        None,
    );
    // Энергия для шахты: `POWGEN` (+1) и топливо в `FDUMP`. Без них
    // `ProductionManager_OptimizeProduction` выключит включённую шахту
    // (`OptimizeMiningIndustry`), и добычи не будет вовсе.
    let generator = spawn_in_complex(
        &mut state,
        UnitType::PowerGenerator,
        Point::new(19, 20),
        None,
    );
    let fuel = spawn_in_complex(&mut state, UnitType::FuelStorage, Point::new(18, 20), None);
    state.unit_mut(fuel).expect("fuel dump").storage = 50;
    let complex = state.unit(mine).expect("mine").complex;
    assert_eq!(state.unit(generator).expect("generator").complex, complex);
    assert_eq!(state.unit(fuel).expect("fuel dump").complex, complex);

    let unit = state.unit_mut(mine).expect("mine");
    unit.orders = UnitOrder::PowerOn;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.raw_mining = 16;
    unit.total_mining = 16;
    unit.storage = 0;

    // Без ADUMP ёмкость комплекса = только склад шахты (25).
    for _ in 0..5 {
        state.manage_economy(TeamId::RED);
    }

    assert_eq!(
        storage(&state, mine),
        25,
        "больше ATTRIB_STORAGE не влезает"
    );

    let complex = complex_of(&state, mine);
    let (materials, capacity) = get_cargo_info(&state, TeamId::RED, complex);
    assert_eq!(capacity.raw, 25);
    assert_eq!(materials.raw, 25, "инвентарь не растёт сверх ёмкости");

    // Остаток лежит в кэше комплекса до следующего пересчёта и теряется:
    // `Access_UpdateResourcesTotal` сбрасывает поле и считает инвентарь.
    update_resources_total(&mut state, TeamId::RED, complex);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        41,
        "25 инвентаря + 16 добычи за ход (кэш на один ход)"
    );
    state.manage_economy(TeamId::RED);
    assert_eq!(storage(&state, mine), 25);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        16,
        "излишек 16 не накапливается — теряется"
    );
}

#[test]
fn fuel_and_gold_income_need_their_own_storage_buildings() {
    let mut state = test_state();
    let mine = spawn_in_complex(
        &mut state,
        UnitType::MiningStation,
        Point::new(20, 20),
        None,
    );
    let complex = complex_of(&state, mine);

    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.fuel_mining = 4;
        unit.gold_mining = 3;
        unit.total_mining = 7;
        unit.storage = 0;
    }

    // У шахты груз RAW: топливо и золото некуда положить.
    state.manage_economy(TeamId::RED);
    assert_eq!(storage(&state, mine), 0);
    let (materials, _) = get_cargo_info(&state, TeamId::RED, complex);
    assert_eq!(materials.fuel, 0);
    assert_eq!(materials.gold, 0);

    // Ставим FDUMP и GOLDSM: доход начинает накапливаться в них.
    let fdump = spawn_in_complex(
        &mut state,
        UnitType::FuelStorage,
        Point::new(22, 24),
        Some(complex),
    );
    let goldsm = spawn_in_complex(
        &mut state,
        UnitType::GoldVault,
        Point::new(24, 24),
        Some(complex),
    );
    state.unit_mut(fdump).expect("fdump").storage = 0;
    state.unit_mut(goldsm).expect("goldsm").storage = 0;

    state.manage_economy(TeamId::RED);
    assert_eq!(storage(&state, fdump), 4, "FDUMP принял топливо");
    assert_eq!(storage(&state, goldsm), 3, "GOLDSM принял золото");

    state.manage_economy(TeamId::RED);
    assert_eq!(storage(&state, fdump), 8);
    assert_eq!(storage(&state, goldsm), 6);
}

#[test]
fn storage_economy_is_deterministic() {
    fn run() -> Vec<(UnitType, i16)> {
        let mut state = test_state();
        let mine = spawn_in_complex(
            &mut state,
            UnitType::MiningStation,
            Point::new(20, 20),
            None,
        );
        let complex = complex_of(&state, mine);
        let dump = spawn_in_complex(
            &mut state,
            UnitType::MaterialStorage,
            Point::new(22, 24),
            Some(complex),
        );
        let fdump = spawn_in_complex(
            &mut state,
            UnitType::FuelStorage,
            Point::new(24, 24),
            Some(complex),
        );

        {
            let unit = state.unit_mut(mine).expect("mine");
            unit.orders = UnitOrder::PowerOn;
            unit.state = UnitOrderState::ExecutingOrder;
            unit.raw_mining = 12;
            unit.fuel_mining = 4;
            unit.total_mining = 16;
            unit.storage = 0;
        }
        state.unit_mut(dump).expect("dump").storage = 0;
        state.unit_mut(fdump).expect("fdump").storage = 0;

        for _ in 0..6 {
            state.manage_economy(TeamId::RED);
        }

        let mut rows = Vec::new();
        for id in state.unit_list(UnitListKind::Stationary) {
            if let Some(unit) = state.unit(*id) {
                rows.push((unit.unit_type, unit.storage));
            }
        }
        rows
    }

    assert_eq!(run(), run());
}

#[test]
fn complex_transfer_negative_uses_buildings_in_stationary_order() {
    let mut state = test_state();
    let mine = spawn_in_complex(
        &mut state,
        UnitType::MiningStation,
        Point::new(20, 20),
        None,
    );
    let complex = complex_of(&state, mine);
    let first = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 24),
        Some(complex),
    );
    let second = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(22, 28),
        Some(complex),
    );

    state.unit_mut(mine).expect("mine").storage = 0;
    state.unit_mut(first).expect("first").storage = 10;
    state.unit_mut(second).expect("second").storage = 10;
    if let Some(target) = state.complex_mut(TeamId::RED, complex.0 as u16) {
        target.raw = 20;
    }

    complex_transfer(&mut state, TeamId::RED, complex, -15, 0, 0);

    assert_eq!(storage(&state, first), 0, "первое в списке отдаёт первым");
    assert_eq!(storage(&state, second), 5);
}

#[test]
fn transfer_cargo_kind_never_exceeds_storage_capacity() {
    let mut state = test_state();
    let dump = spawn_in_complex(
        &mut state,
        UnitType::MaterialStorage,
        Point::new(20, 20),
        None,
    );
    let complex = complex_of(&state, dump);
    state.unit_mut(dump).expect("dump").storage = 45;
    if let Some(target) = state.complex_mut(TeamId::RED, complex.0 as u16) {
        target.raw = 45;
    }

    transfer_cargo_kind(&mut state, dump, CargoType::Raw, 20);

    let unit = state.unit(dump).expect("dump");
    assert_eq!(unit.storage, 50, "склад не превышает ATTRIB_STORAGE");

    // Точный C++ quirk: `TransferRaw` добавляет `amount` к полю комплекса,
    // вычитает излишек и снова добавляет его в `Complex::Transfer`. Если
    // распределить излишек некуда, кэш комплекса раздут (45 + 20 = 65) и
    // нормализуется на следующем `Access_UpdateResourcesTotal` из инвентаря.
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        65
    );

    update_resources_total(&mut state, TeamId::RED, complex);
    assert_eq!(
        state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("complex")
            .raw,
        50,
        "пересчёт из инвентаря отбрасывает нераспределённый излишек"
    );
}

#[test]
fn unit_cargo_helper_reads_storage_by_cargo_type() {
    let mut state = test_state();
    let truck = spawn_truck(&mut state, UnitType::FuelTruck, Point::new(20, 20));
    state.unit_mut(truck).expect("truck").storage = 17;

    let unit: &UnitInfo = state.unit(truck).expect("truck");
    assert_eq!(max_core::transfer::unit_cargo(unit).fuel, 17);
    assert_eq!(max_core::transfer::unit_cargo(unit).raw, 0);
}
