//! P2: производство и строительство — тесты очередей, цен, спавна и отмены.
//!
//! Оракул: `unitinfo.cpp` (`BuildOrder`/`StartBuilding`/`SpawnNewUnit`/
//! `CancelBuilding`), `cargo.cpp` (`Cargo_GetRawConsumptionRate`),
//! `game_manager.cpp` (`GameManager_ProgressBuildState`), `buildmenu.cpp`
//! (`BuildMenu_GetTurnsToBuild`, `BuildMenu_GetMaxPossibleBuildRate`),
//! таблица `default.game_rules.lua` (`max_builder_capability_list`).

use max_core::enums::{unit_flags, Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::production::{
    self, buildable_units, builder_type, can_build, construction_slab_type, construction_utility,
    construction_utility_types, cost_per_turn, is_builder, is_factory, max_possible_build_rate,
    total_cost, turns_and_consumption, BuildError, ProductionEventKind, ProductionManager,
};
use max_core::team::{TeamId, TeamKind};
use max_core::GameState;

/// Партия с одной командой-игроком и золотом.
fn state_with_gold(gold: u32) -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    // `TeamUnits::with_clan` заполняет `current_values` из `SC_ATTRI`: без этого
    // `ATTRIB_TURNS` = 0 и производство «мгновенное» (в реальной партии таблицу
    // заполняет `ResourceManager_InitTeamUnits`).
    state.team_units[0] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    state.team_units[0].set_gold(gold);
    state
}

/// Стационарный склад материалов команды — `ADUMP` с `storage`.
///
/// Заводы в C++ платят **материалами** (`complex->material` /
/// `Cargo_GetRawConsumptionRate`), а не золотом; `ADUMP` — здание с
/// `CARGO_TYPE_RAW`, чей склад видит [`max_core::production::team_raw_materials`].
/// Ставим склад далеко от тестовых юнитов, чтобы он не мешал площадкам.
fn add_materials(state: &mut GameState, materials: i16) -> max_core::UnitId {
    let id = state
        .spawn_unit(UnitType::ADump, TeamId::RED, Point::new(2, 2), 0)
        .expect("ADump");
    state.unit_mut(id).expect("ADump").storage = materials;
    id
}

/// Один «ход производства» — как `GameState::finish_turn` для одной команды.
fn production_turn(state: &mut GameState) -> Vec<max_core::ProductionEvent> {
    // Начало хода команды: `GameManager_ManageEconomy` → `UpdateProduction`
    // (списание сырья со склада строителя), затем `ProgressBuildState`.
    state.manage_economy(TeamId::RED);
    max_core::production::progress_build_state(state, TeamId::RED)
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

// ---------------------------------------------------------------------------
// Таблицы «кто что строит» (default.game_rules.lua)
// ---------------------------------------------------------------------------

#[test]
fn builder_tables_match_the_game_rules_script() {
    // `max_builder_capability_list` — 7 строителей.
    assert_eq!(buildable_units(UnitType::Constructor).len(), 15);
    assert_eq!(buildable_units(UnitType::Engineer).len(), 15);
    assert_eq!(buildable_units(UnitType::LightVehiclePlant).len(), 11);
    assert_eq!(buildable_units(UnitType::HeavyVehiclePlant).len(), 6);
    assert_eq!(buildable_units(UnitType::AirUnitsPlant).len(), 4);
    assert_eq!(buildable_units(UnitType::Shipyard).len(), 8);
    assert_eq!(buildable_units(UnitType::TrainingHall).len(), 2);

    // Порядок — как в Lua: у CONSTRCT первый MININGST, у LIGHTPLT — SCOUT.
    assert_eq!(
        buildable_units(UnitType::Constructor).first(),
        Some(&UnitType::MiningStation)
    );
    assert_eq!(
        buildable_units(UnitType::LightVehiclePlant).first(),
        Some(&UnitType::Scout)
    );

    assert!(can_build(
        UnitType::Constructor,
        UnitType::LightVehiclePlant
    ));
    assert!(can_build(UnitType::Engineer, UnitType::PowerGenerator));
    assert!(!can_build(UnitType::Constructor, UnitType::PowerGenerator));
    assert!(!can_build(UnitType::Tank, UnitType::Scout));
    assert!(!can_build(UnitType::TrainingHall, UnitType::Tank));

    // `Builder_GetBuilderType`.
    assert_eq!(
        builder_type(UnitType::Tank),
        Some(UnitType::HeavyVehiclePlant)
    );
    assert_eq!(
        builder_type(UnitType::Scout),
        Some(UnitType::LightVehiclePlant)
    );
    assert_eq!(
        builder_type(UnitType::Infantry),
        Some(UnitType::TrainingHall)
    );
    assert_eq!(
        builder_type(UnitType::Fighter),
        Some(UnitType::AirUnitsPlant)
    );
    assert_eq!(builder_type(UnitType::Battleship), Some(UnitType::Shipyard));
    assert_eq!(
        builder_type(UnitType::MiningStation),
        Some(UnitType::Constructor)
    );
    assert_eq!(builder_type(UnitType::AlienTank), None);

    assert!(is_builder(UnitType::HeavyVehiclePlant));
    assert!(is_factory(UnitType::AirUnitsPlant));
    assert!(!is_factory(UnitType::Constructor));
    assert!(!is_builder(UnitType::Tank));
}

// ---------------------------------------------------------------------------
// Цены: `Cargo_GetRawConsumptionRate` (cargo.cpp:140)
// ---------------------------------------------------------------------------

#[test]
fn costs_follow_the_cpp_formulas() {
    // cargo.cpp:140 — TRAINHAL 1, CONSTRCT/ENGINEER 2, заводы 3 на 1×.
    assert_eq!(cost_per_turn(UnitType::TrainingHall, 1), 1);
    assert_eq!(cost_per_turn(UnitType::Constructor, 1), 2);
    assert_eq!(cost_per_turn(UnitType::Engineer, 1), 2);
    assert_eq!(cost_per_turn(UnitType::HeavyVehiclePlant, 1), 3);
    assert_eq!(cost_per_turn(UnitType::LightVehiclePlant, 1), 3);

    // Ускорение: 2× → множитель 4, 4× → множитель 12 (та же функция C++).
    assert_eq!(cost_per_turn(UnitType::HeavyVehiclePlant, 2), 12);
    assert_eq!(cost_per_turn(UnitType::HeavyVehiclePlant, 4), 36);
    assert_eq!(cost_per_turn(UnitType::Constructor, 2), 8);

    // Стоимость постройки при 1× = ставка × ходы (CargoMenu::BuyUnit,
    // GetNormalRateBuildCost).
    assert_eq!(total_cost(UnitType::HeavyVehiclePlant, 4), 12);
    assert_eq!(total_cost(UnitType::Constructor, 10), 20);
    assert_eq!(total_cost(UnitType::TrainingHall, 2), 2);
}

#[test]
fn max_build_rate_matches_the_cpp_algorithm() {
    // `BuildMenu_GetMaxPossibleBuildRate`: build_time <= 1 → 1×.
    assert_eq!(
        max_possible_build_rate(UnitType::HeavyVehiclePlant, 1, i32::from(i16::MAX)),
        1
    );
    // Здания (не MOBILE_LAND_UNIT): > 2 → 4×, 2 → 2×.
    assert_eq!(
        max_possible_build_rate(UnitType::HeavyVehiclePlant, 3, i32::from(i16::MAX)),
        4
    );
    assert_eq!(
        max_possible_build_rate(UnitType::HeavyVehiclePlant, 2, i32::from(i16::MAX)),
        2
    );
    // Мобильный строитель ограничен складом: при малом складе остаётся 1×.
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 10, 0), 1);
}

#[test]
fn turns_and_consumption_matches_the_cpp_loop() {
    let mut state = state_with_gold(100);
    let heavy = state
        .spawn_unit(
            UnitType::HeavyVehiclePlant,
            TeamId::RED,
            Point::new(10, 10),
            0,
        )
        .expect("heavy vehicle plant");
    let heavy = state.unit(heavy).expect("factory").clone();

    // `UnitInfo::GetTurnsToBuild`: TANK = 4 хода, завод — 3/ход на 1×, 12 на 2×,
    // 36 на 4×. 4×: 4 хода делятся на 4 → (1,36); 2×: (2,24).
    assert_eq!(
        turns_and_consumption(&state, &heavy, UnitType::Tank, 1),
        (4, 12)
    );
    assert_eq!(
        turns_and_consumption(&state, &heavy, UnitType::Tank, 2),
        (2, 24)
    );
    assert_eq!(
        turns_and_consumption(&state, &heavy, UnitType::Tank, 4),
        (1, 36)
    );

    // Если строитель уже строит тип (`build_time`), берётся остаток, а не
    // `ATTRIB_TURNS`: 3 хода на 2× → 12 + 3 = 15, два хода.
    let mut active = heavy.clone();
    active.build_time = 3;
    active.build_list = vec![UnitType::Tank];
    assert_eq!(
        turns_and_consumption(&state, &active, UnitType::Tank, 2),
        (2, 15)
    );

    // У Training Hall 2×/4× в C++ отключены, но функция считает так же, как
    // `UnitInfo::GetTurnsToBuild`: INFANTRY = 2 хода, ставка 1/ход.
    let hall = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(20, 20), 0)
        .expect("training hall");
    let hall = state.unit(hall).expect("factory").clone();
    assert_eq!(
        turns_and_consumption(&state, &hall, UnitType::Infantry, 1),
        (2, 2)
    );
    assert_eq!(
        turns_and_consumption(&state, &hall, UnitType::Infantry, 2),
        (1, 4)
    );

    // Мобильный строитель с пустым складом: скорость всегда 1×, полная
    // стоимость = ходы × 2 (MININGST = 12 → 24), даже если запросили 2×/4×.
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(30, 30), 0)
        .expect("constructor");
    let mut constructor = state.unit(constructor).expect("builder").clone();
    constructor.storage = 0;
    for rate in [1, 2, 4] {
        assert_eq!(
            turns_and_consumption(&state, &constructor, UnitType::MiningStation, rate),
            (12, 24),
            "ставка {rate}× ограничена пустым складом"
        );
    }
}

#[test]
fn clan_tradeoffs_change_the_build_turns_and_total_cost() {
    // `CLAN_TRADEOFFS`: CrimsonPath (2) даёт SHIPYARD (0x17) `ATTRIB_TURNS -5`
    // (20 → 15). C++ `BuildMenu_GetTurnsToBuild` читает current values, значит
    // и 1×-цена у конструктора падает 40 → 30.
    let mut random = state_with_gold(100);
    let builder = random
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let mut builder = random.unit(builder).expect("builder").clone();
    builder.storage = 0;
    assert_eq!(
        turns_and_consumption(&random, &builder, UnitType::Shipyard, 1),
        (20, 40)
    );

    let mut crimson = GameState::new();
    crimson.teams[0].team_type = TeamKind::Player;
    crimson.team_units[0] = max_core::TeamUnits::with_clan(max_core::TeamClan::CrimsonPath);
    let builder = crimson
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let mut builder = crimson.unit(builder).expect("builder").clone();
    builder.storage = 0;
    assert_eq!(
        turns_and_consumption(&crimson, &builder, UnitType::Shipyard, 1),
        (15, 30),
        "клан сокращает SHIPYARD на 5 ходов"
    );
}

// ---------------------------------------------------------------------------
// Очередь → спавн через N ходов
// ---------------------------------------------------------------------------

#[test]
fn factory_queue_spawns_a_unit_after_n_turns() {
    let mut state = state_with_gold(0);
    let dump = add_materials(&mut state, 20);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(factory, UnitType::Infantry)
        .expect("infantry is buildable at the training hall");
    let report = manager.commit(factory).expect("build order accepted");

    // INFANTRY: ATTRIB_TURNS = 2, ставка TRAINHAL = 1/ход.
    assert_eq!(report.unit_type, UnitType::Infantry);
    assert_eq!(report.turns, 2);
    assert_eq!(report.build_rate, 1);
    assert_eq!(report.cost_per_turn, 1);

    // Ход 1: списание 1 материала, build_time 2 → 1 (юнита ещё нет).
    production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(state.unit(dump).expect("dump").storage, 19);

    // Ход 2: `build_time <= build_rate` → `SpawnNewUnit`.
    let events = production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Infantry), 1);
    assert_eq!(state.unit(dump).expect("dump").storage, 18);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "спавн должен попасть в события: {events:?}"
    );

    // Золото команды производством не тратится (C++: только `CargoMenu`/
    // апгрейды) — раньше завод «платил золотом» и вставал при нуле кредитов.
    assert_eq!(state.team_units[0].gold(), 0, "золото не тратится");

    let factory_unit = state.unit(factory).expect("factory alive");
    assert_eq!(factory_unit.state, UnitOrderState::UnitReady);
    assert_eq!(factory_unit.orders, UnitOrder::Build);
    assert!(factory_unit.build_list.is_empty());
}

#[test]
fn repeated_build_keeps_producing() {
    let mut state = state_with_gold(0);
    let dump = add_materials(&mut state, 20);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.set_repeat(factory, true);
    manager.commit(factory).expect("build");

    for _ in 0..6 {
        production_turn(&mut state);
    }

    assert_eq!(
        count_type(&state, UnitType::Infantry),
        3,
        "repeat: 3 юнита за 6 ходов по 2 хода на юнита"
    );
    assert_eq!(
        state.unit(dump).expect("dump").storage,
        14,
        "6 ходов × 1 материал"
    );
}

#[test]
fn factory_produces_with_zero_gold_and_pauses_until_materials_return() {
    // Реальный сценарий игрока: кредиты на мобилизации потрачены (0 золота),
    // завод обязан работать на материалах шахт и вставать только когда
    // кончились материалы — C++ `ValidateIndustry` (`production_manager.cpp:673`).
    let mut state = state_with_gold(0);
    let dump = add_materials(&mut state, 1);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    drop(manager);

    // Ход 1: 1 материал ушёл, стройка сдвинулась 2 → 1.
    production_turn(&mut state);
    assert_eq!(state.unit(dump).expect("dump").storage, 0);
    assert_eq!(state.unit(factory).map(|unit| unit.build_time), Some(1));

    // Ход 2: материалов нет — пауза, `build_time` не двигается, заказ цел.
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::PausedNoMaterials),
        "ожидалась пауза: {events:?}"
    );
    assert_eq!(state.unit(factory).map(|unit| unit.build_time), Some(1));
    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(state.team_units[0].gold(), 0, "золото не тратится");

    // Подвоз материалов (шахта/склад): стройка доходит с того же места.
    state.unit_mut(dump).expect("dump").storage = 5;
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "после подвоза завод обязан выпустить юнита: {events:?}"
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 1);
    assert_eq!(state.unit(dump).expect("dump").storage, 4);
}

#[test]
fn constructor_builds_a_building_on_a_site() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    // Реальный десант даёт конструктору 40 сырья (`DEFAULT_MISSION_LOADOUT`);
    // `BuildMenu::Build` требует всю стоимость постройки на руках
    // (`buildmenu.cpp:903-906`: 2 × 10 ходов = 20).
    state.unit_mut(constructor).expect("builder").storage = 40;

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("constructor builds training halls");
    let report = manager.commit(constructor).expect("site found");

    assert_eq!(report.turns, 10, "TRAINHAL: ATTRIB_TURNS = 10");
    assert_ne!(
        report.site,
        Point::new(10, 10),
        "площадка не под строителем"
    );

    for _ in 0..10 {
        production_turn(&mut state);
    }

    assert_eq!(
        count_type(&state, UnitType::TrainingHall),
        1,
        "здание должно появиться через 10 ходов"
    );
    assert_eq!(
        state.team_units[0].gold(),
        100,
        "стройка платится сырьём со склада, золото команды не тратится"
    );
    assert_eq!(
        state.unit(constructor).expect("constructor alive").storage,
        20,
        "40 − 2 × 10 ходов = 20 сырья"
    );

    // C++ `SpawnNewUnit`: строитель достроил и ждёт ухода с площадки
    // (`ORDER_BUILD`/`ORDER_STATE_UNIT_READY`), очередь очищена (!path).
    let builder = state.unit(constructor).expect("constructor alive");
    assert_eq!(
        (builder.orders, builder.state),
        (UnitOrder::Build, UnitOrderState::UnitReady)
    );
    assert!(builder.build_list.is_empty(), "очередь очищена");
}
#[test]
fn cancel_stops_the_construction() {
    let mut state = state_with_gold(0);
    let dump = add_materials(&mut state, 20);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    manager.progress();
    assert!(manager.cancel(factory), "постройка отменяется");
    assert!(manager.queue(factory).is_empty());
    drop(manager);

    assert_eq!(state.unit(dump).expect("dump").storage, 19);
    assert_eq!(
        state.unit(factory).map(|unit| unit.state),
        Some(UnitOrderState::BuildCancel)
    );

    for _ in 0..4 {
        production_turn(&mut state);
    }

    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(
        state.unit(dump).expect("dump").storage,
        19,
        "после отмены не списывается"
    );
}

#[test]
fn empty_queue_is_rejected() {
    let mut state = state_with_gold(100);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");
    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(12, 12), 0)
        .expect("tank");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    assert_eq!(manager.commit(factory), Err(BuildError::EmptyQueue));
    assert_eq!(
        manager.enqueue(tank, UnitType::Infantry),
        Err(BuildError::NotABuilder)
    );
    assert_eq!(
        manager.enqueue(factory, UnitType::Tank),
        Err(BuildError::NotBuildable),
        "TRAINHAL не строит танки"
    );
}

#[test]
fn missing_materials_pause_the_build_without_losing_the_queue() {
    let mut state = state_with_gold(100);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");

    let events = manager.progress();

    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::PausedNoMaterials),
        "нехватка материалов — событие паузы: {events:?}"
    );
    assert_eq!(
        manager.queue(factory),
        vec![UnitType::Infantry],
        "заказ не теряется"
    );
    drop(manager);

    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(
        state.unit(factory).map(|unit| unit.build_time),
        Some(2),
        "build_time не двигается на паузе"
    );
    assert_eq!(
        state.team_units[0].gold(),
        100,
        "золото не тратится на производство"
    );
}

#[test]
fn two_speed_build_costs_the_cpp_rate() {
    let mut state = state_with_gold(0);
    // `ADUMP` вмещает 50 единиц (`ATTRIB_STORAGE`); `manage_economy` разложит
    // материалы комплекса по складам — берём ровно вместимость.
    let dump = add_materials(&mut state, 50);
    let factory = state
        .spawn_unit(
            UnitType::HeavyVehiclePlant,
            TeamId::RED,
            Point::new(10, 10),
            0,
        )
        .expect("heavy vehicle plant");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Tank).expect("queue");
    manager.set_build_rate(factory, 2);
    let report = manager.commit(factory).expect("build");

    assert_eq!(report.build_rate, 2);
    assert_eq!(report.cost_per_turn, 12, "3 × 4 (множитель 2× в C++)");

    production_turn(&mut state);
    assert_eq!(state.unit(dump).expect("dump").storage, 38);
    assert_eq!(state.unit(factory).map(|unit| unit.build_time), Some(2));

    production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Tank), 1);
    assert_eq!(state.unit(dump).expect("dump").storage, 26);
}

#[test]
fn mobile_two_speed_build_spends_storage_and_matches_the_menu_cost() {
    // Реальный сценарий игрока: у конструктора 40 сырья в cargo (как
    // `deploy_starting_units`), заказ 2×. C++ `UnitInfo::UpdateProduction`
    // каждый ход уменьшает `storage`; когда его не хватает на 2×, скорость
    // падает до 1×. Итог — ровно (8 ходов, 40 золота), как в колонке `Cost`
    // `BuildMenu` (`turns_and_consumption`).
    let mut state = state_with_gold(1000);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    state.unit_mut(constructor).expect("builder").storage = 40;

    let before = state.unit(constructor).expect("builder").clone();
    let expected = turns_and_consumption(&state, &before, UnitType::MiningStation, 2);
    assert_eq!(expected, (8, 40));

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::MiningStation)
        .expect("constructor builds mining stations");
    manager.set_build_rate(constructor, 2);
    let report = manager.commit(constructor).expect("site found");
    assert_eq!(report.build_rate, 2);

    let mut turns = 0;
    while count_type(&state, UnitType::MiningStation) == 0 && turns < 20 {
        production_turn(&mut state);
        turns += 1;
    }

    assert_eq!(turns, 8, "2× держится 4 хода, затем 1× ещё 4");
    assert_eq!(
        state.team_units[0].gold(),
        1000,
        "мобильный строитель платит сырьём со склада (C++ \
         `Cargo_GetRawConsumptionRate` → `UpdateProduction`), не золотом"
    );
    assert_eq!(
        state.unit(constructor).expect("builder").storage,
        0,
        "C++ `storage -= Cargo_GetRawConsumptionRate(...)`: 40 → 0"
    );
    assert!(
        state
            .unit(constructor)
            .expect("builder")
            .storage
            >= 0,
        "склад не уходит в минус"
    );
}

// ---------------------------------------------------------------------------
// Стройплощадка (C++ `DeployConstructionSiteUtilities` + `PrepareConstructionSite`)
// ---------------------------------------------------------------------------

/// Все юниты команды в клетке `site` (C++ `Hash_MapHash[Point]`).
fn units_at(state: &GameState, site: Point) -> Vec<&max_core::unit::UnitInfo> {
    state
        .units_at(site)
        .iter()
        .filter_map(|id| state.unit(*id))
        .collect()
}

fn find_at<'a>(
    state: &'a GameState,
    site: Point,
    unit_type: UnitType,
) -> Option<&'a max_core::unit::UnitInfo> {
    units_at(state, site)
        .into_iter()
        .find(|unit| unit.unit_type == unit_type)
}

/// C++ `DeployConstructionSiteUtilities` + `PrepareConstructionSite`: заказ
/// конструктора ставит **большую** площадку `LRGTAPE`/`LRGCONES`/`LRGSLAB`
/// ровно в клетке постройки, все — `AWAIT`/`EXECUTING_ORDER`, лента с
/// родителем-строителем, плита — случайный кадр `0..5`.
#[test]
fn constructor_order_deploys_the_large_construction_site() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(constructor).expect("build order");
    let site = report.site;

    // Раскладка C++: конусы и лента — в клетке площадки, большие декали.
    assert_eq!(
        construction_utility_types(UnitType::TrainingHall),
        (UnitType::LargeTape, UnitType::LargeCones)
    );
    assert_eq!(
        construction_slab_type(UnitType::TrainingHall),
        UnitType::LargeSlab
    );
    assert_eq!(
        construction_utility(&state, TeamId::RED, site),
        state.units_at(site).iter().copied().find(|id| state
            .unit(*id)
            .is_some_and(|u| u.unit_type == UnitType::LargeTape))
    );

    let tape = find_at(&state, site, UnitType::LargeTape).expect("LRGTAPE на площадке");
    assert_eq!(tape.angle, 0, "на земле C++ ставит ленту углом 0");
    assert_eq!(
        tape.parent_unit,
        Some(constructor),
        "лента привязана к строителю"
    );
    assert_eq!(tape.orders, UnitOrder::Await);
    assert_eq!(tape.state, UnitOrderState::ExecutingOrder);

    let cones = find_at(&state, site, UnitType::LargeCones).expect("LRGCONES на площадке");
    assert_eq!(cones.angle, 0);
    assert_eq!(cones.orders, UnitOrder::Await);
    assert_eq!(cones.state, UnitOrderState::ExecutingOrder);

    let slab = find_at(&state, site, UnitType::LargeSlab).expect("LRGSLAB на площадке");
    assert!(
        (0..5).contains(&slab.angle),
        "кадр плиты — Randomizer_Generate(image_count = 5), получено {}",
        slab.angle
    );
    assert_eq!(
        slab.scaler_adjust, 4,
        "C++ PrepareConstructionSite: scaler_adjust = 4"
    );
    assert_eq!(
        slab.orders,
        UnitOrder::AwaitScaling,
        "C++ ScaleUnit(slab, ORDER_STATE_EXPAND)"
    );

    // Ровно один комплект: повторный вызов (как из start_building/машины
    // приказов) не плодит вторую площадку и не жжёт RNG.
    let mut duplicate_state = state.clone();
    let rng_before = duplicate_state.rng;
    let duplicate = max_core::production::deploy_construction_site(
        &mut duplicate_state,
        constructor,
        UnitType::TrainingHall,
        site,
    );
    assert!(
        duplicate.is_none(),
        "площадка уже стоит — повторный спавн запрещён"
    );
    assert_eq!(duplicate_state.rng, rng_before, "RNG не сдвинут");
}

/// Инженер строит малый объект (`ROAD`): `SMLTAPE`/`SMLCONES`, плиты нет
/// (`REQUIRES_SLAB` у дороги не выставлен).
#[test]
fn engineer_order_deploys_the_small_construction_site_without_a_slab() {
    let mut state = state_with_gold(100);
    let engineer = state
        .spawn_unit(UnitType::Engineer, TeamId::RED, Point::new(20, 20), 0)
        .expect("engineer");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(engineer, UnitType::Road).expect("queue");
    let report = manager.commit(engineer).expect("build order");
    let site = report.site;

    assert_eq!(
        construction_utility_types(UnitType::Road),
        (UnitType::SmallTape, UnitType::SmallCones)
    );
    assert!(
        find_at(&state, site, UnitType::SmallTape).is_some(),
        "SMLTAPE"
    );
    assert!(
        find_at(&state, site, UnitType::SmallCones).is_some(),
        "SMLCONES"
    );
    assert!(
        find_at(&state, site, UnitType::SmallSlab).is_none(),
        "ROAD не требует плиты"
    );
    assert!(
        find_at(&state, site, UnitType::LargeTape).is_none(),
        "у малого объекта нет большой ленты"
    );
}

/// Вода/побережье: C++ `DeployConstructionSiteUtilities` не ставит конусы, а
/// лента получает угол 1; плита (`REQUIRES_SLAB` у здания) всё равно ставится.
#[test]
fn construction_site_on_water_has_no_cones_and_tape_angle_one() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let site = Point::new(30, 30);
    assert!(state
        .map
        .surface
        .set(site.x, site.y, max_core::enums::Surface::WATER));

    let deployed = max_core::production::deploy_construction_site(
        &mut state,
        constructor,
        UnitType::LightVehiclePlant,
        site,
    )
    .expect("площадка на воде");

    assert_eq!(deployed.cones, None, "на воде конусы не ставятся");
    let tape = state.unit(deployed.tape).expect("лента");
    assert_eq!(tape.angle, 1, "C++: на воде/побережье unit_angle = 1");
    assert!(
        find_at(&state, site, UnitType::LargeSlab).is_some(),
        "плита здания ставится и на воде"
    );
}

/// Готовит партию с достроенным зданием: заказ конструктора + `turns` ходов
/// производства. Возвращает id строителя и клетку площадки.
fn state_with_finished_building(turns: usize) -> (GameState, max_core::UnitId, Point) {
    let mut state = state_with_gold(100);
    // Суша: `Access_IsAccessible` иначе не находит клетку схода с площадки.
    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            let _ = state.map.surface.set(x, y, max_core::enums::Surface::LAND);
        }
    }
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(constructor).expect("site");
    drop(manager);

    // В партии строитель стоит на площадке (C++ `PositionInTape` въезжает в
    // центр ленты); здесь ставим его в клетку якоря, чтобы завершение шло по
    // ветке «строитель внутри габаритов 2×2».
    if let Some(mut unit) = state.unit(constructor).cloned() {
        unit.grid = report.site;
        state.write_unit(constructor, unit);
    }

    for _ in 0..turns {
        production_turn(&mut state);
    }

    (state, constructor, report.site)
}

/// Один тик симуляции — `GameState::tick` (`GameManager_ProcessTick`): пути
/// (`PollResults`) + машина приказов. `process_orders` отдельно недостаточно:
/// `ORDER_MOVE`/`NEW_ORDER` ждёт результата запроса пути, который применяется
/// именно в тике.
fn tick_orders(state: &mut GameState) {
    state.tick();
}

/// Достройка 1:1 C++. `SpawnNewUnit` (`unitinfo.cpp:3440`) ставит здание на
/// площадку в `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY` (оно не рисуется —
/// `UnitInfoGroup::IsRelevant`) и оставляет строителя на площадке в
/// `ORDER_BUILD`/`ORDER_STATE_UNIT_READY`. **Автоматического** ухода нет
/// (`UnitsManager_ProcessOrderBuild`/`UNIT_READY`: `if (path) BuildNext else
/// Animate`): приказ даёт игрок кликом по клетке (`game_manager.cpp:6879-6890`)
/// или ИИ задачей `TaskActivate`. Лента и конусы снимаются, а здание
/// раскрывается (`ScaleUnit(EXPAND)`) только после того, как строитель сошёл с
/// площадки — `UnitsManager_BuildingReady` (`units_manager.cpp:4183`).
#[test]
fn completion_waits_for_the_builder_to_leave_before_the_building_appears() {
    let (mut state, constructor, site) = state_with_finished_building(10);

    // Кадр сразу после `SpawnNewUnit`: здание создано, но ждёт строителя.
    assert_eq!(count_type(&state, UnitType::TrainingHall), 1);
    let (building_id, building_grid) = {
        let building = find_at(&state, site, UnitType::TrainingHall).expect("здание создано");
        (building.id, building.grid)
    };
    {
        let building = state.unit(building_id).expect("здание");
        assert_eq!(
            (building.orders, building.state),
            (UnitOrder::Idle, UnitOrderState::BuildingReady),
            "C++ SpawnNewUnit: здание ждёт в ORDER_IDLE/ORDER_STATE_BUILDING_READY"
        );
        assert_eq!(
            building.parent_unit,
            Some(constructor),
            "C++ SpawnNewUnit: utility_unit->SetParent(this)"
        );
    }

    {
        let builder = state.unit(constructor).expect("constructor alive");
        assert_eq!(
            (builder.orders, builder.state),
            (UnitOrder::Build, UnitOrderState::UnitReady),
            "C++ SpawnNewUnit: строитель достроил и ждёт ухода"
        );
        assert_eq!(builder.parent_unit, Some(building_id), "взаимный родитель");
        assert!(builder.build_list.is_empty(), "!path → build_list.Clear()");
    }

    // Пока строитель стоит на площадке, лента и конусы на месте, а здание не
    // раскрыто (нет `ORDER_AWAIT_SCALING`).
    assert!(
        construction_utility(&state, TeamId::RED, site).is_some(),
        "лента ещё на площадке: здание не раскрыто"
    );
    assert!(
        find_at(&state, site, UnitType::LargeCones).is_some(),
        "конусы ещё на площадке"
    );
    {
        let builder = state.unit(constructor).expect("constructor alive");
        let building = state.unit(building_id).expect("здание");
        assert!(
            production::unit_bounds_contains(building, builder.grid),
            "строитель стоит в габаритах здания"
        );
    }
    assert_eq!(
        production::builder_awaiting_exit(&state, constructor),
        Some(building_id),
        "C++: строитель в ORDER_BUILD/UNIT_READY у достроенного здания"
    );

    // Игрок **не** отдавал приказ: за 30 тиков ничего не происходит — ни
    // `ORDER_ACTIVATE`, ни снятия площадки, ни раскрытия здания.
    for _ in 0..30 {
        tick_orders(&mut state);
    }
    {
        let builder = state.unit(constructor).expect("constructor alive");
        assert_eq!(
            (builder.orders, builder.state),
            (UnitOrder::Build, UnitOrderState::UnitReady),
            "C++ ProcessOrderBuild/UNIT_READY: строитель только анимируется"
        );
        assert_eq!(builder.grid, site, "строитель остаётся на площадке");
    }
    assert!(
        state
            .unit(building_id)
            .is_some_and(|unit| unit.orders == UnitOrder::Idle),
        "здание не раскрывается без ухода строителя"
    );
    assert!(construction_utility(&state, TeamId::RED, site).is_some());

    // Приказ игрока (`game_manager.cpp:6879-6890`): `ORDER_ACTIVATE`/
    // `ORDER_STATE_IN_TRANSITION` с `move_to` = клетка клика. В порте это
    // `unit_order::issue_builder_exit` (путь ИИ) или пакет 08/P3 от UI; цель —
    // первая проходимая клетка вокруг здания.
    assert!(
        max_core::unit_order::issue_builder_exit(&mut state, constructor),
        "приказ на уход выставлен"
    );
    {
        let builder = state.unit(constructor).expect("constructor alive");
        assert_eq!(
            (builder.orders, builder.state),
            (UnitOrder::Activate, UnitOrderState::InTransition),
            "C++ порядок игрока: ORDER_ACTIVATE/ORDER_STATE_IN_TRANSITION"
        );
    }

    // Следующий тик: `PositionInTape` DEINIT/LEAVE — доворот и съезд с ленты.
    tick_orders(&mut state);
    {
        let builder = state.unit(constructor).expect("constructor alive");
        assert_eq!(
            (builder.orders, builder.state),
            (
                UnitOrder::AwaitTapePositioning,
                UnitOrderState::TapePositioningDeinit
            ),
            "C++ ProcessOrderActivate/IN_TRANSITION → AWAIT_TAPE_POSITIONING/DEINIT"
        );
    }

    // Уход: доворот + четыре шага по 8 px (PositionInTape), затем
    // `ActivateEngineer` → ORDER_MOVE к свободной клетке вокруг здания.
    let mut left_site = false;
    for _ in 0..20 {
        tick_orders(&mut state);
        let builder_grid = state.unit(constructor).map(|unit| unit.grid);
        let building = state.unit(building_id).expect("здание");
        if let Some(grid) = builder_grid {
            left_site = !production::unit_bounds_contains(building, grid);
        }
        if left_site {
            break;
        }
    }
    assert!(left_site, "строитель обязан сойти с площадки");

    // Строитель вне габаритов → `UnitsManager_BuildingReady`: утилиты сняты,
    // плита осталась, здание раскрывается.
    let mut expanded = false;
    for _ in 0..12 {
        tick_orders(&mut state);
        if state
            .unit(building_id)
            .is_some_and(|unit| unit.orders == UnitOrder::AwaitScaling)
        {
            expanded = true;
            break;
        }
    }
    assert!(expanded, "C++ BuildingReady: ScaleUnit(building, EXPAND)");

    assert!(
        construction_utility(&state, TeamId::RED, site).is_none(),
        "лента снимается после ухода строителя"
    );
    assert!(
        find_at(&state, site, UnitType::LargeCones).is_none(),
        "конусы снимаются вместе с лентой"
    );
    assert!(
        find_at(&state, site, UnitType::LargeSlab).is_some(),
        "плита остаётся под зданием (remove_slabs = false)"
    );

    {
        let building = state.unit(building_id).expect("здание");
        assert_eq!(building.scaler_adjust, 4);
        assert_eq!(building.parent_unit, None);
        assert_eq!(
            building.grid, building_grid,
            "здание стоит в клетке площадки"
        );
    }
    assert_eq!(count_type(&state, UnitType::TrainingHall), 1);

    // После раскрытия здание возвращается в `ORDER_AWAIT`, а строитель идёт к
    // клетке схода (`ORDER_MOVE`) и по прибытии — в `ORDER_AWAIT`.
    for _ in 0..40 {
        tick_orders(&mut state);
    }
    let builder_grid = {
        let building = state.unit(building_id).expect("здание");
        assert_eq!(
            (building.orders, building.state),
            (UnitOrder::Await, UnitOrderState::ExecutingOrder),
            "после EXPAND здание в ORDER_AWAIT/EXECUTING_ORDER (сейчас {:?}/{:?}, prior {:?}/{:?})",
            building.orders,
            building.state,
            building.prior_orders,
            building.prior_state
        );
        let builder = state.unit(constructor).expect("constructor alive");
        assert_eq!(builder.orders, UnitOrder::Await);
        assert_eq!(builder.state, UnitOrderState::ExecutingOrder);
        assert!(
            !production::unit_bounds_contains(building, builder.grid),
            "строитель остаётся вне площадки"
        );
        builder.grid
    };
    assert_ne!(builder_grid, building_grid, "строитель сошёл с площадки");
}

/// `RADAR` после достройки обязан вернуться в `ORDER_SENTRY`.
///
/// C++: `UnitsManager_DeployUnit` (`units_manager.cpp:2412-2421`) ставит
/// `RADAR` (как турели и мины) в `ORDER_SENTRY`; `SpawnNewUnit`
/// (`unitinfo.cpp:3581`) переводит здание в `ORDER_IDLE`/
/// `ORDER_STATE_BUILDING_READY` через **`SetNewOrderInt`**, который сохраняет
/// текущий приказ в `prior_orders` (`units_manager.cpp:1969-1975`). После ухода
/// строителя `UnitsManager_BuildingReady` (`units_manager.cpp:4240`) делает
/// `RestoreOrders` (`unitinfo.cpp:3783`) → снова `ORDER_SENTRY`, и
/// `ProcessOrderSentry` листает 16 кадров `D_RADAR` (`ANIMATED`). Порт раньше
/// жёстко писал `prior = AWAIT`, поэтому радар замирал в `ORDER_AWAIT`.
#[test]
fn a_built_radar_restores_its_sentry_order() {
    let mut state = state_with_gold(400);

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let _ = state.map.surface.set(x, y, Surface::LAND);
        }
    }

    // `RADAR` строит **инженер** (`builder_type(Radar) == Engineer`), но ветка
    // `SpawnNewUnit` для мобильного строителя — та же, что у конструктора.
    let constructor = state
        .spawn_unit(UnitType::Engineer, TeamId::RED, Point::new(10, 10), 0)
        .expect("engineer");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::Radar)
        .expect("queue");
    let report = manager.commit(constructor).expect("site");
    drop(manager);

    if let Some(mut unit) = state.unit(constructor).cloned() {
        unit.grid = report.site;
        state.write_unit(constructor, unit);
    }

    let mut radar_id = None;
    for _ in 0..80 {
        production_turn(&mut state);

        if let Some(unit) = units_at(&state, report.site)
            .into_iter()
            .find(|unit| unit.unit_type == UnitType::Radar)
        {
            radar_id = Some(unit.id);
            break;
        }
    }

    let radar_id = radar_id.unwrap_or_else(|| {
        let builder = state.unit(constructor).expect("builder");
        panic!(
            "RADAR достроен: builder={:?}/{:?} build_time={} site={:?} radar_count={}",
            builder.orders,
            builder.state,
            builder.build_time,
            report.site,
            count_type(&state, UnitType::Radar)
        )
    });

    {
        let radar = state.unit(radar_id).expect("radar");
        assert_eq!(
            (radar.orders, radar.state),
            (UnitOrder::Idle, UnitOrderState::BuildingReady),
            "SpawnNewUnit: здание ждёт строителя"
        );
        assert_eq!(
            (radar.prior_orders, radar.prior_state),
            (UnitOrder::Sentry, UnitOrderState::ExecutingOrder),
            "SetNewOrderInt сохранил ORDER_SENTRY в prior"
        );
    }

    // Приказ на уход даёт игрок/ИИ (`issue_builder_exit`) — см. A1.
    assert!(max_core::unit_order::issue_builder_exit(
        &mut state,
        constructor
    ));

    let mut restored = false;
    for _ in 0..60 {
        tick_orders(&mut state);

        if state
            .unit(radar_id)
            .is_some_and(|unit| unit.orders == UnitOrder::Sentry)
        {
            restored = true;
            break;
        }
    }

    assert!(
        restored,
        "после достройки RADAR обязан вернуться в ORDER_SENTRY"
    );
    let radar = state.unit(radar_id).expect("radar");
    assert_eq!(
        (radar.orders, radar.state),
        (UnitOrder::Sentry, UnitOrderState::ExecutingOrder),
        "RestoreOrders вернул ORDER_SENTRY/EXECUTING_ORDER"
    );
    assert!(
        radar.flags & unit_flags::ANIMATED != 0,
        "RADAR — ANIMATED: кадры листает ProcessOrderSentry"
    );
}

/// Достройка детерминирована: два одинаковых прогона дают один снапшот на
/// каждом кадре (порядок «уход строителя → раскрытие здания» не зависит от
/// итерации по хешам).
#[test]
fn completion_sequence_is_deterministic() {
    fn run() -> Vec<Vec<u8>> {
        let (mut state, constructor, _) = state_with_finished_building(10);
        let mut frames = vec![state.snapshot_bytes()];

        // Приказ игрока на уход (`ORDER_ACTIVATE`), затем тики до раскрытия.
        assert!(max_core::unit_order::issue_builder_exit(
            &mut state,
            constructor
        ));

        for _ in 0..30 {
            tick_orders(&mut state);
            frames.push(state.snapshot_bytes());
        }

        frames
    }

    assert_eq!(run(), run());
}

/// `stop` мобильного строителя (`CancelBuilding` → `BuildCancel`) убирает
/// площадку из клетки заказа: лента, конусы и плита не остаются на карте.
#[test]
fn cancelling_the_build_removes_the_site() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(constructor).expect("site");
    let site = report.site;
    drop(manager);
    assert!(construction_utility(&state, TeamId::RED, site).is_some());

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    assert!(manager.cancel(constructor));
    drop(manager);

    // `CancelBuilding` исполняется процессором приказа на следующем тике.
    state.process_orders();

    assert_eq!(count_type(&state, UnitType::LargeTape), 0);
    assert_eq!(count_type(&state, UnitType::LargeCones), 0);
    assert_eq!(count_type(&state, UnitType::LargeSlab), 0);
    assert_eq!(count_type(&state, UnitType::TrainingHall), 0);
    assert_eq!(
        state.unit(constructor).map(|unit| unit.orders),
        Some(UnitOrder::Await)
    );
}

/// Раскладка площадки и порядок RNG детерминированы: два одинаковых заказа
/// дают одинаковые типы/клетки/кадры и одинаковый снапшот.
#[test]
fn construction_site_layout_is_deterministic() {
    fn run() -> (Vec<u8>, Point, Vec<(UnitType, Point, u8)>) {
        let mut state = state_with_gold(100);
        let constructor = state
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(30, 30), 0)
            .expect("constructor");

        let mut manager = ProductionManager::new(&mut state, TeamId::RED);
        manager
            .enqueue(constructor, UnitType::TrainingHall)
            .expect("queue");
        let report = manager.commit(constructor).expect("site");

        let mut layout: Vec<(UnitType, Point, u8)> = state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.unit(id))
            .filter(|unit| {
                unit.team == TeamId::RED
                    && matches!(
                        unit.unit_type,
                        UnitType::LargeTape | UnitType::LargeCones | UnitType::LargeSlab
                    )
            })
            .map(|unit| (unit.unit_type, unit.grid, unit.angle))
            .collect();
        layout.sort_unstable_by_key(|(unit_type, point, angle)| {
            (*unit_type as u16, point.y, point.x, *angle)
        });

        (state.snapshot_bytes(), report.site, layout)
    }

    let (bytes_a, site_a, layout_a) = run();
    let (bytes_b, site_b, layout_b) = run();

    assert_eq!(layout_a, layout_b, "раскладка площадки совпадает");
    assert_eq!(site_a, site_b, "клетка площадки совпадает");
    assert_eq!(layout_a.len(), 3, "лента + конусы + плита");
    assert_eq!(
        layout_a
            .iter()
            .filter(|(unit_type, _, _)| *unit_type == UnitType::LargeSlab)
            .count(),
        1,
        "плита ровно одна"
    );
    assert_eq!(bytes_a, bytes_b, "снапшот состояния совпадает байт-в-байт");
}

// ---------------------------------------------------------------------------
// Детерминизм
// ---------------------------------------------------------------------------

#[test]
fn two_identical_production_runs_have_identical_state() {
    fn run() -> Vec<u8> {
        let mut state = state_with_gold(100);
        add_materials(&mut state, 100);
        let factory = state
            .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
            .expect("training hall");
        let constructor = state
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(20, 20), 0)
            .expect("constructor");

        let mut manager = ProductionManager::new(&mut state, TeamId::RED);
        manager.enqueue(factory, UnitType::Infantry).expect("queue");
        manager.enqueue(factory, UnitType::Commando).expect("queue");
        manager.commit(factory).expect("build");
        manager
            .enqueue(constructor, UnitType::LightVehiclePlant)
            .expect("constructor queue");
        manager.commit(constructor).expect("constructor build");

        for _ in 0..14 {
            production_turn(&mut state);
        }

        state.snapshot_bytes()
    }

    assert_eq!(
        run(),
        run(),
        "два прогона производства совпадают байт-в-байт"
    );
}

// ---------------------------------------------------------------------------
// `finish_turn` — интеграция хода производства
// ---------------------------------------------------------------------------

#[test]
fn finish_turn_runs_the_production_phase() {
    let mut state = state_with_gold(100);
    // Дуэль как в партии: RED — человек, GREEN — компьютер, сетевая ветка
    // выключена (локальная семантика `menu_check_end_game_conditions`).
    state.teams[1].team_type = TeamKind::Computer;
    state.team_units[1] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    state.team_units[1].set_gold(100);
    state.network_game = false;
    add_materials(&mut state, 20);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");
    state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(14, 14), 0)
        .expect("constructor");
    // Боевая мощь держит команду «живой» по generic-правилам (`SC_L0002`):
    // без неё `menu_check_end_game_conditions` закрывает партию.
    state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(16, 16), 0)
        .expect("red tank");
    state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(40, 40), 0)
        .expect("green tank");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    drop(manager);

    state.game_state = max_core::enums::GameStateType::EndTurn;

    let status = max_core::victory::evaluate_status(&state, state.turn_counter);
    let over = state.finish_turn();

    assert!(
        !over,
        "status: teams_total={} in_play={} humans={} non_computers={} player={:?} red={:?} green={:?}",
        status.teams_total,
        status.teams_in_play,
        status.teams_humans_in_play,
        status.teams_non_computers,
        status.team_status[0],
        status.team_status[0],
        status.team_status[1]
    );
    assert_eq!(state.turn_counter, 1);
    assert_eq!(
        state.team_units[0].gold(),
        100,
        "золото команды производством не тратится"
    );
    assert_eq!(
        state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.unit(id))
            .find(|unit| unit.unit_type == UnitType::ADump)
            .map(|unit| unit.storage),
        Some(19),
        "ход завода списан материалами (1 за INFANTRY)"
    );
    assert_eq!(state.unit(factory).map(|unit| unit.build_time), Some(1));
}

/// Цвет команды — `TeamUnits::hash_team_id` (`resource_manager.cpp:969-981`):
/// RED = `HASH_TEAM_RED` 0x2000, GREEN = 0x1000, BLUE = 0x800, GRAY = 0x400,
/// ALIEN/derelict = 0x8000. C++ пишет этот бит в флаги юнита
/// (`unitinfo.cpp:179`), порт — восстанавливает цвет тем же отображением
/// (флаги входят в эталонный дайджест и остаются без битов).
#[test]
fn team_hash_ids_match_the_cpp_color_slots() {
    assert_eq!(max_core::team::hash_team_id(TeamId::RED), 0x2000);
    assert_eq!(max_core::team::hash_team_id(TeamId::GREEN), 0x1000);
    assert_eq!(max_core::team::hash_team_id(TeamId::BLUE), 0x0800);
    assert_eq!(max_core::team::hash_team_id(TeamId::GRAY), 0x0400);
    assert_eq!(max_core::team::hash_team_id(TeamId::ALIEN), 0x8000);
}

/// Материалы базы — сумма складов стационарных зданий с `CARGO_TYPE_RAW`:
/// шахт, `ADUMP`, теплиц и казарм (C++ `Complex::material` + `Complex::Transfer`;
/// в порте комплексы не сливаются, поэтому — вся команда).
#[test]
fn team_raw_materials_sums_raw_stationary_storage() {
    let mut state = state_with_gold(0);
    assert_eq!(max_core::production::team_raw_materials(&state, TeamId::RED), 0);

    let dump = add_materials(&mut state, 30);
    let mine = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(30, 30), 0)
        .expect("mine");
    state.unit_mut(mine).expect("mine").storage = 7;
    // Не-сырьевые здания и мобильные юниты в склад материалов не входят.
    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(40, 40), 0)
        .expect("tank");
    state.unit_mut(tank).expect("tank").storage = 9;
    let gas = state
        .spawn_unit(UnitType::FDump, TeamId::RED, Point::new(50, 50), 0)
        .expect("fuel dump");
    state.unit_mut(gas).expect("fuel dump").storage = 11;

    assert_eq!(
        max_core::production::team_raw_materials(&state, TeamId::RED),
        37,
        "ADUMP 30 + шахта 7; топливный склад и танк не в счёт"
    );
    assert_eq!(
        max_core::production::team_raw_materials(&state, TeamId::GREEN),
        0
    );
    assert_eq!(state.unit(dump).expect("dump").storage, 30, "склад не тронут");
}

/// Флаги здания/юнита — проверка, что заводы действительно стационарны.
#[test]
fn factories_are_stationary_units() {
    for factory in [
        UnitType::Shipyard,
        UnitType::LightVehiclePlant,
        UnitType::HeavyVehiclePlant,
        UnitType::AirUnitsPlant,
        UnitType::TrainingHall,
    ] {
        let flags = max_core::access::unit_type_flags(factory);
        assert_ne!(flags & unit_flags::STATIONARY, 0, "{factory:?}");
        assert_ne!(flags & unit_flags::BUILDING, 0, "{factory:?}");
    }
}
