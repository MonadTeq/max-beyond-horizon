//! P2: производство и строительство — тесты очередей, цен, спавна и отмены.
//!
//! Оракул: `unitinfo.cpp` (`BuildOrder`/`StartBuilding`/`SpawnNewUnit`/
//! `CancelBuilding`), `cargo.cpp` (`Cargo_GetRawConsumptionRate`),
//! `game_manager.cpp` (`GameManager_ProgressBuildState`), `buildmenu.cpp`
//! (`BuildMenu_GetTurnsToBuild`, `BuildMenu_GetMaxPossibleBuildRate`),
//! таблица `default.game_rules.lua` (`max_builder_capability_list`).

use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::production::{
    builder_type, buildable_units, can_build, cost_per_turn, is_builder, is_factory,
    max_possible_build_rate, total_cost, BuildError, ProductionEventKind, ProductionManager,
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

/// Один «ход производства» — как `GameState::finish_turn` для одной команды.
fn production_turn(state: &mut GameState) -> Vec<max_core::ProductionEvent> {
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

    assert!(can_build(UnitType::Constructor, UnitType::LightVehiclePlant));
    assert!(can_build(UnitType::Engineer, UnitType::PowerGenerator));
    assert!(!can_build(UnitType::Constructor, UnitType::PowerGenerator));
    assert!(!can_build(UnitType::Tank, UnitType::Scout));
    assert!(!can_build(UnitType::TrainingHall, UnitType::Tank));

    // `Builder_GetBuilderType`.
    assert_eq!(builder_type(UnitType::Tank), Some(UnitType::HeavyVehiclePlant));
    assert_eq!(builder_type(UnitType::Scout), Some(UnitType::LightVehiclePlant));
    assert_eq!(builder_type(UnitType::Infantry), Some(UnitType::TrainingHall));
    assert_eq!(builder_type(UnitType::Fighter), Some(UnitType::AirUnitsPlant));
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

// ---------------------------------------------------------------------------
// Очередь → спавн через N ходов
// ---------------------------------------------------------------------------

#[test]
fn factory_queue_spawns_a_unit_after_n_turns() {
    let mut state = state_with_gold(100);
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

    // Ход 1: списание 1, build_time 2 → 1 (юнита ещё нет).
    production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(state.team_units[0].gold(), 99);

    // Ход 2: `build_time <= build_rate` → `SpawnNewUnit`.
    let events = production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Infantry), 1);
    assert_eq!(state.team_units[0].gold(), 98);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "спавн должен попасть в события: {events:?}"
    );

    let factory_unit = state.unit(factory).expect("factory alive");
    assert_eq!(factory_unit.state, UnitOrderState::UnitReady);
    assert_eq!(factory_unit.orders, UnitOrder::Build);
    assert!(factory_unit.build_list.is_empty());
}

#[test]
fn repeated_build_keeps_producing() {
    let mut state = state_with_gold(100);
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
}

#[test]
fn constructor_builds_a_building_on_a_site() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("constructor builds training halls");
    let report = manager.commit(constructor).expect("site found");

    assert_eq!(report.turns, 10, "TRAINHAL: ATTRIB_TURNS = 10");
    assert_ne!(report.site, Point::new(10, 10), "площадка не под строителем");

    for _ in 0..10 {
        production_turn(&mut state);
    }

    assert_eq!(
        count_type(&state, UnitType::TrainingHall),
        1,
        "здание должно появиться через 10 ходов"
    );
    assert_eq!(state.team_units[0].gold(), 100 - 20, "2 золота × 10 ходов");

    let builder = state.unit(constructor).expect("constructor alive");
    assert_eq!(builder.orders, UnitOrder::Await);
    assert!(builder.build_list.is_empty(), "очередь очищена");
}

#[test]
fn cancel_stops_the_construction() {
    let mut state = state_with_gold(100);
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

    assert_eq!(state.team_units[0].gold(), 99);
    assert_eq!(
        state.unit(factory).map(|unit| unit.state),
        Some(UnitOrderState::BuildCancel)
    );

    for _ in 0..4 {
        production_turn(&mut state);
    }

    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(state.team_units[0].gold(), 99, "после отмены не списывается");
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
fn missing_gold_pauses_the_build_without_losing_the_queue() {
    let mut state = state_with_gold(0);
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
            .any(|event| event.kind == ProductionEventKind::PausedNoGold),
        "нехватка золота — событие паузы: {events:?}"
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
    assert_eq!(state.team_units[0].gold(), 0, "списывать нечего");
}

#[test]
fn two_speed_build_costs_the_cpp_rate() {
    let mut state = state_with_gold(200);
    let factory = state
        .spawn_unit(UnitType::HeavyVehiclePlant, TeamId::RED, Point::new(10, 10), 0)
        .expect("heavy vehicle plant");

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Tank).expect("queue");
    manager.set_build_rate(factory, 2);
    let report = manager.commit(factory).expect("build");

    assert_eq!(report.build_rate, 2);
    assert_eq!(report.cost_per_turn, 12, "3 × 4 (множитель 2× в C++)");

    production_turn(&mut state);
    assert_eq!(state.team_units[0].gold(), 188);
    assert_eq!(state.unit(factory).map(|unit| unit.build_time), Some(2));

    production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Tank), 1);
    assert_eq!(state.team_units[0].gold(), 176);
}

// ---------------------------------------------------------------------------
// Детерминизм
// ---------------------------------------------------------------------------

#[test]
fn two_identical_production_runs_have_identical_state() {
    fn run() -> Vec<u8> {
        let mut state = state_with_gold(100);
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

    assert_eq!(run(), run(), "два прогона производства совпадают байт-в-байт");
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
    assert_eq!(state.team_units[0].gold(), 99, "стоимость хода списана");
    assert_eq!(state.unit(factory).map(|unit| unit.build_time), Some(1));
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
