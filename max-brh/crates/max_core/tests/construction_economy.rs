//! Construction economy in Turn Based — 1:1 by the C++ oracle.
//!
//! Oracle:
//! * `buildmenu.cpp:688` — `BuildMenu_GetTurnsToBuild`;
//! * `buildmenu.cpp:729` — `BuildMenu_GetMaxPossibleBuildRate` (storage cuts 2×/4×);
//! * `buildmenu.cpp:903-906` — `MobileBuildMenu::Build` does not issue an order if
//!   at 1× the builder does not have the **whole** construction cost on hand (`_(c529)`);
//! * `cargo.cpp:140` — `Cargo_GetRawConsumptionRate` (raw rate per turn);
//! * `unitinfo.cpp:2904-2915` — `UnitInfo::UpdateProduction` cuts `build_rate`
//!   by storage and charges raw material (`storage -= rate`);
//! * `game_manager.cpp:8076-8094` — `GameManager_SpawnNewUnits`: with
//!   `build_time <= build_rate` the construction completes (`SpawnNewUnit`).
//!
//! Port requirement: the resource **never goes negative**, and an unpaid turn does not advance
//! `build_time` (in C++ debt is allowed, and the construction "completes" out of thin air).

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::production::{
    self, max_possible_build_rate, BuildError, ProductionEventKind,
};
use max_core::team::{TeamId, TeamKind};
use max_core::universe::GameUniverse;
use max_core::{TeamClan, TeamUnits};

fn economy_state(gold: u32) -> GameUniverse {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units_table_mut()[0].set_gold(gold);
    state
}

fn production_turn(state: &mut GameUniverse) -> Vec<max_core::ProductionEvent> {
    state.manage_economy(TeamId::RED);
    state.progress_build_state(TeamId::RED)
}

fn count_type(state: &GameUniverse, unit_type: UnitType) -> usize {
    state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
                .snapshot()
                .unit(*id)
                .map(|unit| unit.unit_type == unit_type)
                .unwrap_or(false)
        })
        .count()
}

fn constructor(state: &mut GameUniverse, storage: i16) -> max_core::UnitId {
    let id = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");

    state.unit_mut(id).expect("constructor").storage = storage;
    id
}

/// Power/workers for the factory complex: `POWGEN` (+1), `FDUMP` (fuel) and
/// `HABITAT` (+3 workers) right next to the factory. Without them `Cargo_ApplyUnitConsumption`
/// gives negative `power`/`life`, and C++ `ValidateIndustry` does not let the factory
/// build (`production_manager.cpp:677-711`).
fn power_the_complex(state: &mut GameUniverse, factory: max_core::UnitId) -> max_core::UnitId {
    let grid = state.snapshot().unit(factory).expect("factory").grid;
    let generator = state
        .spawn_unit(
            UnitType::PowerGenerator,
            TeamId::RED,
            Point::new(grid.x - 1, grid.y),
            0,
        )
        .expect("power generator");
    let fuel = state
        .spawn_unit(
            UnitType::FuelStorage,
            TeamId::RED,
            Point::new(grid.x - 2, grid.y),
            0,
        )
        .expect("fuel dump");
    let habitat = state
        .spawn_unit(
            UnitType::Habitat,
            TeamId::RED,
            Point::new(grid.x - 1, grid.y + 1),
            0,
        )
        .expect("habitat");
    state.unit_mut(fuel).expect("fuel dump").storage = 50;

    let complex = state.snapshot().unit(factory).expect("factory").complex;
    for id in [generator, fuel, habitat] {
        assert_eq!(
            state.snapshot().unit(id).expect("power unit").complex,
            complex,
            "комплекс завода должен получить энергию/рабочих"
        );
    }

    habitat
}

#[test]
fn constructor_pays_raw_material_and_never_goes_negative() {
    let mut state = economy_state(150);
    let builder = constructor(&mut state, 40);

    let mut manager = state.production_manager(TeamId::RED);
    manager
        .enqueue(builder, UnitType::TrainingHall)
        .expect("queue");
    manager
        .commit(builder)
        .expect("10 ходов × 2 сырья = 20 ≤ 40");

    // C++ `AbstractBuildMenu::Draw`: the `Turns`/`Cost` columns — 10 turns and 20
    // raw units at 1×.
    assert_eq!(
        production::total_cost(UnitType::Constructor, 10),
        20,
        "Cargo_GetRawConsumptionRate(CONSTRCT, 1) = 2 × 10 ходов"
    );

    for turn in 0..10 {
        production_turn(&mut state);

        let storage = state.snapshot().unit(builder).expect("builder").storage;
        assert!(
            storage >= 0,
            "склад не уходит в минус (ход {turn}, storage = {storage})"
        );
    }

    assert_eq!(count_type(&state, UnitType::TrainingHall), 1);
    assert_eq!(
        state.snapshot().unit(builder).expect("builder").storage,
        20,
        "40 − 10 × 2"
    );
    assert_eq!(
        state.snapshot().team_units()[0].gold(),
        150,
        "золото команды стройкой не тратится"
    );
}

#[test]
fn short_materials_pause_the_build_without_spending_or_progressing() {
    // 1 raw is less than the rate 2: C++ would go to −1 and still advance
    // `build_time`; the port must stop without charging or advancing.
    let mut state = economy_state(0);
    let builder = constructor(&mut state, 1);

    let mut manager = state.production_manager(TeamId::RED);
    manager
        .enqueue(builder, UnitType::TrainingHall)
        .expect("queue");
    // `BuildMenu::Build` at 1× requires the whole cost (20) — the order is not issued.
    assert_eq!(manager.commit(builder), Err(BuildError::NotEnoughMaterials));
    drop(manager);

    // Bypass the BuildMenu check (as a player with a sufficient
    // stock would, who then spent the raw material on transport) — the pause must
    // still work: `UpdateProduction` does not charge into the negative.
    let mut unit = state.snapshot().unit(builder).expect("builder").clone();
    unit.build_list = vec![UnitType::TrainingHall];
    unit.build_time = 10;
    unit.build_rate = 1;
    unit.orders = UnitOrder::Build;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.target_grid = Point::new(11, 10);
    state.write_unit(builder, unit);

    let events = production_turn(&mut state);

    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::PausedNoResources),
        "ожидалось событие паузы: {events:?}"
    );
    assert_eq!(
        state.snapshot().unit(builder).expect("builder").storage,
        1,
        "нечем платить — склад не меняется и не уходит в минус"
    );
    assert_eq!(
        state.snapshot().unit(builder).expect("builder").build_time,
        10,
        "пауза не двигает build_time"
    );
    assert_eq!(count_type(&state, UnitType::TrainingHall), 0);
}

#[test]
fn build_completes_exactly_after_the_paid_turns() {
    let mut state = economy_state(0);
    // Exactly 5 turns at rate 2 — the order is rejected (20 is needed for all 10).
    let builder = constructor(&mut state, 20);
    assert_eq!(
        state.production_manager(TeamId::RED).commit(builder),
        Err(BuildError::EmptyQueue),
        "очередь пуста — сначала enqueue"
    );

    let mut manager = state.production_manager(TeamId::RED);
    manager
        .enqueue(builder, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(builder).expect("20 = 2 × 10");
    assert_eq!(report.turns, 10);
    drop(manager);

    // After 9 turns the construction is still running (`build_time` = 2), completion is on the 10th:
    // C++ `GameManager_SpawnNewUnits`: `build_time <= build_rate`.
    for turn in 0..9 {
        production_turn(&mut state);
        assert_eq!(
            count_type(&state, UnitType::TrainingHall),
            0,
            "здание не появляется раньше 10-го хода (после {turn} ходов)"
        );
    }

    assert_eq!(
        state.snapshot().unit(builder).expect("builder").build_time,
        1,
        "10 − 9 ходов: остался последний"
    );
    assert_eq!(state.snapshot().unit(builder).expect("builder").storage, 2);

    production_turn(&mut state);

    assert_eq!(count_type(&state, UnitType::TrainingHall), 1);
    assert_eq!(state.snapshot().unit(builder).expect("builder").storage, 0);
    assert_eq!(
        (
            state.snapshot().unit(builder).expect("builder").orders,
            state.snapshot().unit(builder).expect("builder").state
        ),
        (UnitOrder::Build, UnitOrderState::UnitReady)
    );
}

#[test]
fn build_rate_is_capped_by_the_storage_like_cpp() {
    // `BuildMenu_GetMaxPossibleBuildRate(unit_type, build_time, storage)`
    // (`buildmenu.cpp:729`): with 6 raw and 8 turns, 2× is unreachable.
    assert_eq!(
        max_possible_build_rate(UnitType::Constructor, 8, 6),
        1,
        "2× требует 4 × 2 = 8 на первом ходу пары"
    );
    // The C++ algorithm (`buildmenu.cpp:756-784`) with 8 turns and 40 raw already
    // allows 4×: `turns_2x` accumulates 4 pairs, and `const_change_4x`
    // fits into storage. Actual consumption will then exceed 40 — the port
    // will pause the construction instead of going negative.
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 8, 40), 4);
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 8, 20), 2);
    assert_eq!(
        max_possible_build_rate(UnitType::Constructor, 3, 100),
        4,
        "build_time > 2 и склад позволяет 4× (buildmenu.cpp:739-745)"
    );
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 2, 100), 2);
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 1, 100), 1);
}

#[test]
fn factory_pays_the_connected_complex_materials_and_never_goes_negative() {
    // Factories pay with the materials of **their complex** (`Cargo_GetRawConsumptionRate`
    // → `UnitInfo::StartBuilding` / `Cargo_GetNetProduction`), not with gold and not
    // from the "whole team pool": at zero credits production runs while the
    // complex connected by connectors has raw material. A shortage pauses
    // without going negative and without losing the order.
    let mut state = economy_state(0);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(20, 20), 0)
        .expect("training hall");
    // The storage right next to the factory (`Access_GetTeamBuilding` + `UpdateConnectors`
    // merge them into one complex: capacities add up).
    let dump = state
        .spawn_unit(
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(22, 20),
            0,
        )
        .expect("MaterialStorage");
    state.unit_mut(dump).expect("MaterialStorage").storage = 1;
    power_the_complex(&mut state, factory);

    assert_eq!(
        state.snapshot().unit(factory).expect("factory").complex,
        state
            .snapshot()
            .unit(dump)
            .expect("MaterialStorage")
            .complex,
        "завод и ADUMP — один комплекс"
    );

    let mut manager = state.production_manager(TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    drop(manager);

    // First turn: 1 material leaves the common pool, `build_time` 2 → 1.
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .all(|event| event.kind != ProductionEventKind::PausedNoMaterials),
        "первый ход оплачен: {events:?}"
    );
    assert_eq!(
        state
            .snapshot()
            .unit(dump)
            .expect("MaterialStorage")
            .storage,
        0,
        "1 материал ушёл"
    );
    assert_eq!(
        state.snapshot().unit(factory).expect("factory").build_time,
        1
    );
    assert_eq!(state.snapshot().team_units()[0].gold(), 0, "золото не тратится");

    // Second turn: nothing to pay with — pause, the order is intact.
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::PausedNoMaterials),
        "нехватка материалов — пауза: {events:?}"
    );
    assert_eq!(
        state.snapshot().team_units()[0].gold(),
        0,
        "золото не уходит в минус"
    );
    assert_eq!(
        state
            .snapshot()
            .unit(dump)
            .expect("MaterialStorage")
            .storage,
        0,
        "склад не уходит в минус"
    );
    assert_eq!(
        state.production_manager(TeamId::RED).queue(factory),
        vec![UnitType::Infantry],
        "заказ сохранён"
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 0);

    // Delivery to the same pool: the construction completes from the same place.
    state.unit_mut(dump).expect("MaterialStorage").storage = 5;
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "после подвоза завод обязан выпустить юнита: {events:?}"
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 1);
    assert_eq!(
        state
            .snapshot()
            .unit(dump)
            .expect("MaterialStorage")
            .storage,
        4
    );
}

#[test]
fn factory_without_connected_storage_pauses_even_with_team_materials() {
    // Oracle: `ValidateIndustry` looks only at the factory complex
    // (`complex == unit->GetComplex()`). A storage standing separately (without
    // connectors) does not enter the factory pool — even if the team has raw material.
    // The port checks the first turn at start (`StartBuilding` in C++ charges
    // raw/power/workers at once) and does not issue the order.
    let mut state = economy_state(0);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(20, 20), 0)
        .expect("training hall");
    let far_dump = state
        .spawn_unit(UnitType::MaterialStorage, TeamId::RED, Point::new(2, 2), 0)
        .expect("MaterialStorage");
    state.unit_mut(far_dump).expect("MaterialStorage").storage = 50;
    power_the_complex(&mut state, factory);

    let mut manager = state.production_manager(TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    assert_eq!(
        manager.commit(factory),
        Err(BuildError::NotEnoughMaterials),
        "сырьё вне комплекса не оплачивает первый ход"
    );
    drop(manager);

    // The order stayed in the queue, but the construction did not start.
    assert_eq!(
        state.snapshot().unit(factory).expect("factory").build_time,
        0,
        "очередь не стартует без материалов"
    );
    assert_ne!(
        state.snapshot().unit(factory).expect("factory").orders,
        UnitOrder::Build
    );
    assert_eq!(
        state
            .snapshot()
            .unit(far_dump)
            .expect("MaterialStorage")
            .storage,
        50
    );
}

#[test]
fn paused_build_resumes_when_materials_arrive() {
    let mut state = economy_state(0);
    let builder = constructor(&mut state, 20);

    let mut manager = state.production_manager(TeamId::RED);
    manager
        .enqueue(builder, UnitType::TrainingHall)
        .expect("queue");
    manager.commit(builder).expect("site");
    drop(manager);

    // Spend the storage artificially to catch a pause in the middle of construction.
    state.unit_mut(builder).expect("builder").storage = 1;
    production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::TrainingHall), 0);
    assert_eq!(
        state.snapshot().unit(builder).expect("builder").build_time,
        10
    );

    // Raw material delivery: construction continues from the same `build_time`.
    state.unit_mut(builder).expect("builder").storage = 20;

    for _ in 0..10 {
        production_turn(&mut state);
    }

    assert_eq!(
        count_type(&state, UnitType::TrainingHall),
        1,
        "после подвоза стройка доходит до конца"
    );
}
