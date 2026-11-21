//! Экономика стройки в Turn Based — 1:1 по C++-оракулу.
//!
//! Оракул:
//! * `buildmenu.cpp:688` — `BuildMenu_GetTurnsToBuild`;
//! * `buildmenu.cpp:729` — `BuildMenu_GetMaxPossibleBuildRate` (склад режет 2×/4×);
//! * `buildmenu.cpp:903-906` — `MobileBuildMenu::Build` не отдаёт приказ, если
//!   при 1× у строителя нет **всей** стоимости постройки на руках (`_(c529)`);
//! * `cargo.cpp:140` — `Cargo_GetRawConsumptionRate` (ставка сырья за ход);
//! * `unitinfo.cpp:2904-2915` — `UnitInfo::UpdateProduction` режет `build_rate`
//!   по складу и списывает сырьё (`storage -= rate`);
//! * `game_manager.cpp:8076-8094` — `GameManager_SpawnNewUnits`: при
//!   `build_time <= build_rate` постройка завершается (`SpawnNewUnit`).
//!
//! Требование порта: ресурс **не уходит в минус**, а ход без оплаты не двигает
//! `build_time` (в C++ долг допускается, и стройка «достраивается» из воздуха).

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::production::{
    self, max_possible_build_rate, BuildError, ProductionEventKind, ProductionManager,
};
use max_core::team::{TeamId, TeamKind};
use max_core::{GameState, TeamClan, TeamUnits};

fn economy_state(gold: u32) -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[0].set_gold(gold);
    state
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
                .map(|unit| unit.unit_type == unit_type)
                .unwrap_or(false)
        })
        .count()
}

fn constructor(state: &mut GameState, storage: i16) -> max_core::UnitId {
    let id = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");

    state.unit_mut(id).expect("constructor").storage = storage;
    id
}

#[test]
fn constructor_pays_raw_material_and_never_goes_negative() {
    let mut state = economy_state(150);
    let builder = constructor(&mut state, 40);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(builder, UnitType::TrainingHall)
        .expect("queue");
    manager.commit(builder).expect("10 ходов × 2 сырья = 20 ≤ 40");

    // C++ `AbstractBuildMenu::Draw`: колонки `Turns`/`Cost` — 10 ходов и 20
    // единиц сырья при 1×.
    assert_eq!(
        production::total_cost(UnitType::Constructor, 10),
        20,
        "Cargo_GetRawConsumptionRate(CONSTRCT, 1) = 2 × 10 ходов"
    );

    for turn in 0..10 {
        production_turn(&mut state);

        let storage = state.unit(builder).expect("builder").storage;
        assert!(
            storage >= 0,
            "склад не уходит в минус (ход {turn}, storage = {storage})"
        );
    }

    assert_eq!(count_type(&state, UnitType::TrainingHall), 1);
    assert_eq!(
        state.unit(builder).expect("builder").storage,
        20,
        "40 − 10 × 2"
    );
    assert_eq!(
        state.team_units[0].gold(),
        150,
        "золото команды стройкой не тратится"
    );
}

#[test]
fn short_materials_pause_the_build_without_spending_or_progressing() {
    // 1 сырья меньше ставки 2: C++ ушёл бы в −1 и всё равно сдвинул бы
    // `build_time`; порт обязан остановиться, не тратя и не двигая.
    let mut state = economy_state(0);
    let builder = constructor(&mut state, 1);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(builder, UnitType::TrainingHall)
        .expect("queue");
    // `BuildMenu::Build` при 1× требует всю стоимость (20) — заказ не отдаётся.
    assert_eq!(manager.commit(builder), Err(BuildError::NotEnoughMaterials));
    drop(manager);

    // Обходим проверку BuildMenu (как это сделал бы игрок с достаточным
    // запасом, который затем потратил сырьё на перевозку) — пауза всё равно
    // должна работать: `UpdateProduction` не списывает в минус.
    let mut unit = state.unit(builder).expect("builder").clone();
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
        state.unit(builder).expect("builder").storage,
        1,
        "нечем платить — склад не меняется и не уходит в минус"
    );
    assert_eq!(
        state.unit(builder).expect("builder").build_time,
        10,
        "пауза не двигает build_time"
    );
    assert_eq!(count_type(&state, UnitType::TrainingHall), 0);
}

#[test]
fn build_completes_exactly_after_the_paid_turns() {
    let mut state = economy_state(0);
    // Ровно на 5 ходов по ставке 2 — заказ отклоняется (нужно 20 на все 10).
    let builder = constructor(&mut state, 20);
    assert_eq!(
        ProductionManager::new(&mut state, TeamId::RED).commit(builder),
        Err(BuildError::EmptyQueue),
        "очередь пуста — сначала enqueue"
    );

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(builder, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(builder).expect("20 = 2 × 10");
    assert_eq!(report.turns, 10);
    drop(manager);

    // После 9 ходов стройка ещё идёт (`build_time` = 2), завершение — на 10-м:
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
        state.unit(builder).expect("builder").build_time,
        1,
        "10 − 9 ходов: остался последний"
    );
    assert_eq!(state.unit(builder).expect("builder").storage, 2);

    production_turn(&mut state);

    assert_eq!(count_type(&state, UnitType::TrainingHall), 1);
    assert_eq!(state.unit(builder).expect("builder").storage, 0);
    assert_eq!(
        (
            state.unit(builder).expect("builder").orders,
            state.unit(builder).expect("builder").state
        ),
        (UnitOrder::Build, UnitOrderState::UnitReady)
    );
}

#[test]
fn build_rate_is_capped_by_the_storage_like_cpp() {
    // `BuildMenu_GetMaxPossibleBuildRate(unit_type, build_time, storage)`
    // (`buildmenu.cpp:729`): при 6 сырья и 8 ходах 2× недостижим.
    assert_eq!(
        max_possible_build_rate(UnitType::Constructor, 8, 6),
        1,
        "2× требует 4 × 2 = 8 на первом ходу пары"
    );
    // Алгоритм C++ (`buildmenu.cpp:756-784`) при 8 ходах и 40 сырья уже
    // допускает 4×: `turns_2x` набирает 4 пары, и `const_change_4x`
    // укладывается в склад. Реальный расход при этом превысит 40 — порт
    // поставит стройку на паузу вместо ухода в минус.
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
    // Заводы платят материалами **своего комплекса** (`Cargo_GetRawConsumptionRate`
    // → `UnitInfo::StartBuilding` / `Cargo_GetNetProduction`), а не золотом и не
    // «котлом всей команды»: при нуле кредитов производство идёт, пока в
    // объединённом коннекторами комплексе есть сырьё. Нехватка ставит на паузу
    // без ухода в минус и без потери заказа.
    let mut state = economy_state(0);
    let factory = state
        .spawn_unit(
            UnitType::TrainingHall,
            TeamId::RED,
            Point::new(20, 20),
            0,
        )
        .expect("training hall");
    // Склад вплотную к заводу (`Access_GetTeamBuilding` + `UpdateConnectors`
    // сливают их в один комплекс: ёмкости складываются).
    let dump = state
        .spawn_unit(UnitType::ADump, TeamId::RED, Point::new(22, 20), 0)
        .expect("ADump");
    state.unit_mut(dump).expect("ADump").storage = 1;

    assert_eq!(
        state.unit(factory).expect("factory").complex,
        state.unit(dump).expect("ADump").complex,
        "завод и ADUMP — один комплекс"
    );

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    drop(manager);

    // Первый ход: 1 материал уходит из общего котла, `build_time` 2 → 1.
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .all(|event| event.kind != ProductionEventKind::PausedNoMaterials),
        "первый ход оплачен: {events:?}"
    );
    assert_eq!(state.unit(dump).expect("ADump").storage, 0, "1 материал ушёл");
    assert_eq!(state.unit(factory).expect("factory").build_time, 1);
    assert_eq!(state.team_units[0].gold(), 0, "золото не тратится");

    // Второй ход: платить нечем — пауза, заказ цел.
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::PausedNoMaterials),
        "нехватка материалов — пауза: {events:?}"
    );
    assert_eq!(state.team_units[0].gold(), 0, "золото не уходит в минус");
    assert_eq!(
        state.unit(dump).expect("ADump").storage,
        0,
        "склад не уходит в минус"
    );
    assert_eq!(
        ProductionManager::new(&mut state, TeamId::RED).queue(factory),
        vec![UnitType::Infantry],
        "заказ сохранён"
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 0);

    // Подвоз в тот же котёл: стройка завершается с того же места.
    state.unit_mut(dump).expect("ADump").storage = 5;
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "после подвоза завод обязан выпустить юнита: {events:?}"
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 1);
    assert_eq!(state.unit(dump).expect("ADump").storage, 4);
}

#[test]
fn factory_without_connected_storage_pauses_even_with_team_materials() {
    // Оракул: `ValidateIndustry` смотрит только комплекс завода
    // (`complex == unit->GetComplex()`). Склад, стоящий отдельно (без
    // коннекторов), в котёл завода не входит — даже если у команды есть сырьё.
    let mut state = economy_state(0);
    let factory = state
        .spawn_unit(
            UnitType::TrainingHall,
            TeamId::RED,
            Point::new(20, 20),
            0,
        )
        .expect("training hall");
    let far_dump = state
        .spawn_unit(UnitType::ADump, TeamId::RED, Point::new(2, 2), 0)
        .expect("ADump");
    state.unit_mut(far_dump).expect("ADump").storage = 50;

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    drop(manager);

    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::PausedNoMaterials),
        "склад вне комплекса не оплачивает завод: {events:?}"
    );
    assert_eq!(state.unit(far_dump).expect("ADump").storage, 50);
    assert_eq!(state.unit(factory).expect("factory").build_time, 2);
}

#[test]
fn paused_build_resumes_when_materials_arrive() {
    let mut state = economy_state(0);
    let builder = constructor(&mut state, 20);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(builder, UnitType::TrainingHall)
        .expect("queue");
    manager.commit(builder).expect("site");
    drop(manager);

    // Тратим склад искусственно, чтобы поймать паузу посреди стройки.
    state.unit_mut(builder).expect("builder").storage = 1;
    production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::TrainingHall), 0);
    assert_eq!(state.unit(builder).expect("builder").build_time, 10);

    // Подвоз сырья: стройка продолжается с того же `build_time`.
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
