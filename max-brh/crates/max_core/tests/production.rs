//! P2: production and construction — tests of queues, prices, spawning and cancellation.
//!
//! Oracle: `unitinfo.cpp` (`BuildOrder`/`StartBuilding`/`SpawnNewUnit`/
//! `CancelBuilding`), `cargo.cpp` (`Cargo_GetRawConsumptionRate`),
//! `game_manager.cpp` (`GameManager_ProgressBuildState`), `buildmenu.cpp`
//! (`BuildMenu_GetTurnsToBuild`, `BuildMenu_GetMaxPossibleBuildRate`),
//! the `default.game_rules.lua` table (`max_builder_capability_list`).

use max_core::enums::{unit_flags, Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::production::{
    self, buildable_units, builder_type, can_build, construction_slab_type, construction_utility,
    construction_utility_types, cost_per_turn, is_builder, is_factory, max_possible_build_rate,
    total_cost, turns_and_consumption, BuildError, ProductionEventKind, ProductionManager,
};
use max_core::team::{TeamId, TeamKind};
use max_core::universe::GameUniverse;

/// A game with one player team and gold.
fn state_with_gold(gold: u32) -> GameUniverse {
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    // `TeamUnits::with_clan` fills `current_values` from `SC_ATTRI`: otherwise
    // `ATTRIB_TURNS` = 0 and production is "instant" (in a real game the table
    // is filled by `ResourceManager_InitTeamUnits`).
    state.state_mut().team_units[0] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    state.state_mut().team_units[0].set_gold(gold);
    state
}

/// A stationary materials storage **in the test factory's complex** — an `ADUMP`
/// with `storage` standing right next to the factory at `(10,10)` (2×2): connectors
/// (`UnitsManager_UpdateConnectors`) merge them into one complex, and the factory
/// spends raw material from the common pool (`Complex::GetCargoInfo`).
///
/// In C++ factories pay with **complex materials** (`complex->material` /
/// `Cargo_GetRawConsumptionRate`), not gold; a standalone storage does not
/// enter the pool.
fn add_materials(state: &mut GameUniverse, materials: i16) -> max_core::UnitId {
    let id = state
        .spawn_unit(
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(12, 10),
            0,
        )
        .expect("MaterialStorage");
    state.unit_mut(id).expect("MaterialStorage").storage = materials;
    id
}

/// Provide the factory complex with power/workers the way a base does in
/// C++: `POWGEN` (+1 power for 2 fuel), `FDUMP` with a fuel reserve and
/// `HABITAT` (+3 workers). All three stand right next to the factory, so
/// `UnitsManager_UpdateConnectors` merges them into one complex.
///
/// Needed because `Cargo_ApplyUnitConsumption` for a factory charges
/// `power` (and `life` for `TRAINHAL`) from the complex pool — without a producer
/// the order does not start (`production_manager.cpp:677-711`).
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
    assert!(complex.is_some(), "завод должен входить в комплекс");
    for id in [generator, fuel, habitat] {
        assert_eq!(
            state.snapshot().unit(id).expect("power unit").complex,
            complex,
            "производитель энергии/рабочих должен слиться с комплексом завода"
        );
    }

    habitat
}

/// One "production turn" — like `GameUniverse::finish_turn` for one team.
fn production_turn(state: &mut GameUniverse) -> Vec<max_core::ProductionEvent> {
    // Start of the team turn: `GameManager_ManageEconomy` → `UpdateProduction`
    // (charging raw material from the builder's storage), then `ProgressBuildState`.
    state.manage_economy(TeamId::RED);
    max_core::production::progress_build_state(state.state_mut(), TeamId::RED)
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

// ---------------------------------------------------------------------------
// "Who builds what" tables (default.game_rules.lua)
// ---------------------------------------------------------------------------

#[test]
fn builder_tables_match_the_game_rules_script() {
    // `max_builder_capability_list` — 7 builders.
    assert_eq!(buildable_units(UnitType::Constructor).len(), 15);
    assert_eq!(buildable_units(UnitType::Engineer).len(), 15);
    assert_eq!(buildable_units(UnitType::LightVehiclePlant).len(), 11);
    assert_eq!(buildable_units(UnitType::HeavyVehiclePlant).len(), 6);
    assert_eq!(buildable_units(UnitType::AirUnitsPlant).len(), 4);
    assert_eq!(buildable_units(UnitType::Shipyard).len(), 8);
    assert_eq!(buildable_units(UnitType::TrainingHall).len(), 2);

    // Order — as in Lua: CONSTRCT has MININGST first, LIGHTPLT — SCOUT.
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
    assert_eq!(builder_type(UnitType::Gunboat), Some(UnitType::Shipyard));
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
// Prices: `Cargo_GetRawConsumptionRate` (cargo.cpp:140)
// ---------------------------------------------------------------------------

#[test]
fn costs_follow_the_cpp_formulas() {
    // cargo.cpp:140 — TRAINHAL 1, CONSTRCT/ENGINEER 2, factories 3 at 1×.
    assert_eq!(cost_per_turn(UnitType::TrainingHall, 1), 1);
    assert_eq!(cost_per_turn(UnitType::Constructor, 1), 2);
    assert_eq!(cost_per_turn(UnitType::Engineer, 1), 2);
    assert_eq!(cost_per_turn(UnitType::HeavyVehiclePlant, 1), 3);
    assert_eq!(cost_per_turn(UnitType::LightVehiclePlant, 1), 3);

    // Acceleration: 2× → multiplier 4, 4× → multiplier 12 (the same C++ function).
    assert_eq!(cost_per_turn(UnitType::HeavyVehiclePlant, 2), 12);
    assert_eq!(cost_per_turn(UnitType::HeavyVehiclePlant, 4), 36);
    assert_eq!(cost_per_turn(UnitType::Constructor, 2), 8);

    // Construction cost at 1× = rate × turns (CargoMenu::BuyUnit,
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
    // Buildings (not MOBILE_LAND_UNIT): > 2 → 4×, 2 → 2×.
    assert_eq!(
        max_possible_build_rate(UnitType::HeavyVehiclePlant, 3, i32::from(i16::MAX)),
        4
    );
    assert_eq!(
        max_possible_build_rate(UnitType::HeavyVehiclePlant, 2, i32::from(i16::MAX)),
        2
    );
    // A mobile builder is limited by storage: with a small storage 1× remains.
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
    let heavy = state.snapshot().unit(heavy).expect("factory").clone();

    // `UnitInfo::GetTurnsToBuild`: TANK = 4 turns, factory — 3/turn at 1×, 12 at 2×,
    // 36 at 4×. 4×: 4 turns divided by 4 → (1,36); 2×: (2,24).
    assert_eq!(
        turns_and_consumption(state.state(), &heavy, UnitType::Tank, 1),
        (4, 12)
    );
    assert_eq!(
        turns_and_consumption(state.state(), &heavy, UnitType::Tank, 2),
        (2, 24)
    );
    assert_eq!(
        turns_and_consumption(state.state(), &heavy, UnitType::Tank, 4),
        (1, 36)
    );

    // If the builder already builds the type (`build_time`), the remainder is taken, not
    // `ATTRIB_TURNS`: 3 turns at 2× → 12 + 3 = 15, two turns.
    let mut active = heavy.clone();
    active.build_time = 3;
    active.build_list = vec![UnitType::Tank];
    assert_eq!(
        turns_and_consumption(state.state(), &active, UnitType::Tank, 2),
        (2, 15)
    );

    // Training Hall has 2×/4× disabled in C++, but the function computes like
    // `UnitInfo::GetTurnsToBuild`: INFANTRY = 2 turns, rate 1/turn.
    let hall = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(20, 20), 0)
        .expect("training hall");
    let hall = state.snapshot().unit(hall).expect("factory").clone();
    assert_eq!(
        turns_and_consumption(state.state(), &hall, UnitType::Infantry, 1),
        (2, 2)
    );
    assert_eq!(
        turns_and_consumption(state.state(), &hall, UnitType::Infantry, 2),
        (1, 4)
    );

    // A mobile builder with empty storage: rate always 1×, full
    // cost = turns × 2 (MININGST = 12 → 24), even if 2×/4× was requested.
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(30, 30), 0)
        .expect("constructor");
    let mut constructor = state.snapshot().unit(constructor).expect("builder").clone();
    constructor.storage = 0;
    for rate in [1, 2, 4] {
        assert_eq!(
            turns_and_consumption(state.state(), &constructor, UnitType::MiningStation, rate),
            (12, 24),
            "ставка {rate}× ограничена пустым складом"
        );
    }
}

#[test]
fn clan_tradeoffs_change_the_build_turns_and_total_cost() {
    // `CLAN_TRADEOFFS`: CrimsonPath (2) gives SHIPYARD (0x17) `ATTRIB_TURNS -5`
    // (20 → 15). C++ `BuildMenu_GetTurnsToBuild` reads current values, so
    // the 1× price of the constructor also drops 40 → 30.
    let mut random = state_with_gold(100);
    let builder = random
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let mut builder = random.snapshot().unit(builder).expect("builder").clone();
    builder.storage = 0;
    assert_eq!(
        turns_and_consumption(random.state(), &builder, UnitType::Shipyard, 1),
        (20, 40)
    );

    let mut crimson = GameUniverse::default();
    crimson.state_mut().teams[0].team_type = TeamKind::Player;
    crimson.state_mut().team_units[0] =
        max_core::TeamUnits::with_clan(max_core::TeamClan::CrimsonPath);
    let builder = crimson
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let mut builder = crimson.snapshot().unit(builder).expect("builder").clone();
    builder.storage = 0;
    assert_eq!(
        turns_and_consumption(crimson.state(), &builder, UnitType::Shipyard, 1),
        (15, 30),
        "клан сокращает SHIPYARD на 5 ходов"
    );
}

// ---------------------------------------------------------------------------
// Queue → spawn after N turns
// ---------------------------------------------------------------------------

#[test]
fn factory_queue_spawns_a_unit_after_n_turns() {
    let mut state = state_with_gold(0);
    let dump = add_materials(&mut state, 20);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");
    power_the_complex(&mut state, factory);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager
        .enqueue(factory, UnitType::Infantry)
        .expect("infantry is buildable at the training hall");
    let report = manager.commit(factory).expect("build order accepted");

    // INFANTRY: ATTRIB_TURNS = 2, TRAINHAL rate = 1/turn.
    assert_eq!(report.unit_type, UnitType::Infantry);
    assert_eq!(report.turns, 2);
    assert_eq!(report.build_rate, 1);
    assert_eq!(report.cost_per_turn, 1);

    // Turn 1: 1 material charged, build_time 2 → 1 (no unit yet).
    production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 19);

    // Turn 2: `build_time <= build_rate` → `SpawnNewUnit`.
    let events = production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Infantry), 1);
    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 18);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "спавн должен попасть в события: {events:?}"
    );

    // Team gold is not spent by production (C++: only `CargoMenu`/
    // upgrades) — previously the factory "paid with gold" and stopped at zero credits.
    assert_eq!(state.state().team_units[0].gold(), 0, "золото не тратится");

    let factory_unit = state.snapshot().unit(factory).expect("factory alive");
    assert_eq!(factory_unit.state, UnitOrderState::UnitReady);
    assert_eq!(factory_unit.orders, UnitOrder::Build);
    assert!(factory_unit.build_list.is_empty());
}

/// `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3489-3520`), `STATIONARY` branch:
/// the built unit appears **on the factory cell** and stays in it
/// (`ORDER_IDLE`/`ORDER_STATE_STORE`, `scaler_adjust = 4`), the unit's parent is
/// the factory, the factory's parent is the unit, `storage = 1`. The player brings it out by order
/// (`ORDER_ACTIVATE` → `UnitsManager_ActivateUnit`).
#[test]
fn air_plant_keeps_the_built_fighter_on_its_cell_until_activated() {
    let mut state = state_with_gold(0);
    let _dump = add_materials(&mut state, 40);
    let plant = state
        .spawn_unit(UnitType::AirUnitsPlant, TeamId::RED, Point::new(10, 10), 0)
        .expect("air plant");
    power_the_complex(&mut state, plant);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(plant, UnitType::Fighter).expect("fighter");
    manager.commit(plant).expect("build");

    // FIGHTER: `ATTRIB_TURNS` from SC_ATTRI; build until `UNIT_READY`.
    let turns = max_core::production::turns_to_build(state.state(), TeamId::RED, UnitType::Fighter);
    for _ in 0..turns {
        production_turn(&mut state);
    }

    let fighter = state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .snapshot()
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::Fighter)
        })
        .expect("истребитель построен");

    let unit = state.snapshot().unit(fighter).expect("fighter");
    let plant_unit = state.snapshot().unit(plant).expect("plant");

    // The unit stands ON the factory (same cell), not on an adjacent one.
    assert_eq!(unit.grid, Point::new(10, 10), "юнит на клетке завода");
    assert_eq!(unit.parent_unit, Some(plant), "родитель — завод");
    assert_eq!(unit.orders, UnitOrder::Idle);
    assert_eq!(unit.state, UnitOrderState::Store, "юнит «в трюме» завода");
    assert_eq!(unit.scaler_adjust, 4, "C++: scaler_adjust = 4");

    // Factory: the parent is the ready unit, `storage = 1`, waiting for the exit.
    assert_eq!(
        plant_unit.parent_unit,
        Some(fighter),
        "родитель завода — юнит"
    );
    assert_eq!(plant_unit.storage, 1);
    assert_eq!(plant_unit.orders, UnitOrder::Build);
    assert_eq!(plant_unit.state, UnitOrderState::UnitReady);
    assert!(plant_unit.build_list.is_empty());

    // A player click on an adjacent cell → `ORDER_ACTIVATE` to the factory.
    assert!(
        max_core::unit_order::issue_factory_exit(state.state_mut(), plant),
        "приказ вывода отдан"
    );
    for _ in 0..64 {
        state.tick();

        let fighter_unit = state.snapshot().unit(fighter).expect("fighter alive");
        // Takeoff: `AirPath` completes `HOVERING` (`UnitInfo::Take`), then the unit
        // leaves the factory cell.
        if fighter_unit.flags & unit_flags::HOVERING != 0 && fighter_unit.grid != Point::new(10, 10)
        {
            break;
        }
    }

    let fighter_unit = state.snapshot().unit(fighter).expect("fighter alive");
    assert_ne!(
        fighter_unit.grid,
        Point::new(10, 10),
        "истребитель вылетел из завода"
    );
    assert!(
        fighter_unit.flags & unit_flags::HOVERING != 0,
        "истребитель в воздухе (`ORDER_TAKE_OFF` → `HOVERING`)"
    );

    let plant_unit = state.snapshot().unit(plant).expect("plant");
    assert_eq!(plant_unit.parent_unit, None, "завод отдал юнита");
    assert_eq!(plant_unit.storage, 0, "--storage в ActivateUnit");
}

/// Without an order the unit stays in the factory: the queue does not move and the unit does not
/// appear on the map (`Access_GetTeamUnit` skips `ORDER_IDLE`).
#[test]
fn air_plant_unit_stays_inside_without_a_launch_order() {
    let mut state = state_with_gold(0);
    let _dump = add_materials(&mut state, 40);
    let plant = state
        .spawn_unit(UnitType::AirUnitsPlant, TeamId::RED, Point::new(10, 10), 0)
        .expect("air plant");
    power_the_complex(&mut state, plant);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(plant, UnitType::Fighter).expect("fighter");
    manager.commit(plant).expect("build");

    let turns = max_core::production::turns_to_build(state.state(), TeamId::RED, UnitType::Fighter);
    for _ in 0..turns {
        production_turn(&mut state);
    }

    let fighter = state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .snapshot()
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::Fighter)
        })
        .expect("истребитель построен");

    for _ in 0..8 {
        state.tick();
    }

    let unit = state.snapshot().unit(fighter).expect("fighter");
    assert_eq!(unit.grid, Point::new(10, 10), "юнит не покинул завод");
    assert_eq!(unit.orders, UnitOrder::Idle);
    assert_eq!(unit.state, UnitOrderState::Store);

    // A click on the factory cell selects the factory itself, not the unit in it.
    assert_eq!(
        max_core::access::get_team_unit(
            state.state(),
            Point::new(10, 10),
            TeamId::RED,
            unit_flags::SELECTABLE
        ),
        Some(plant),
        "`Access_GetTeamUnit` пропускает `ORDER_IDLE`"
    );
}

fn launch_ready_unit(state: &mut GameUniverse, factory: max_core::UnitId) -> bool {
    if !max_core::unit_order::issue_factory_exit(state.state_mut(), factory) {
        return false;
    }

    for _ in 0..16 {
        state.tick();

        let done = state.snapshot().unit(factory).is_some_and(|unit| {
            unit.parent_unit.is_none()
                && !(unit.orders == UnitOrder::Build && unit.state == UnitOrderState::UnitReady)
        });

        if done {
            return true;
        }
    }

    false
}

#[test]
fn repeated_build_keeps_producing_after_the_unit_is_launched() {
    let mut state = state_with_gold(0);
    let dump = add_materials(&mut state, 20);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");
    power_the_complex(&mut state, factory);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.set_repeat(factory, true);
    manager.commit(factory).expect("build");

    // 2 turns — the first unit is ready and stands **in the factory** (`UnitInfo::SpawnNewUnit`:
    // the unit on the factory cell, `ORDER_IDLE`/`ORDER_STATE_STORE`, the parent — the factory).
    for _ in 0..2 {
        production_turn(&mut state);
    }
    assert_eq!(count_type(&state, UnitType::Infantry), 1);

    let factory_unit = state.snapshot().unit(factory).expect("factory alive");
    assert_eq!(factory_unit.state, UnitOrderState::UnitReady);
    assert_eq!(factory_unit.storage, 1, "в заводе один готовый юнит");
    assert_eq!(
        factory_unit.build_list,
        vec![UnitType::Infantry],
        "repeat_build вернул тип в очередь"
    );

    // There is no auto-start of the next order: while the unit is not taken out, the factory stands —
    // the next order is started only by `UnitsManager_ActivateUnit`.
    for _ in 0..2 {
        production_turn(&mut state);
    }
    assert_eq!(
        count_type(&state, UnitType::Infantry),
        1,
        "без вывода юнита очередь не двигается (C++: UNIT_READY ждёт клиента)"
    );

    // Taking the unit out starts the next order (repeat).
    assert!(
        launch_ready_unit(&mut state, factory),
        "юнит выведен из завода"
    );
    let factory_unit = state.snapshot().unit(factory).expect("factory alive");
    assert_eq!(factory_unit.orders, UnitOrder::Build);
    assert_eq!(factory_unit.storage, 0);
    assert!(factory_unit.build_time > 0, "следующий заказ стартовал");

    // 2 turns — the second unit, another exit + 2 turns — the third.
    for _ in 0..2 {
        production_turn(&mut state);
    }
    assert_eq!(count_type(&state, UnitType::Infantry), 2);
    assert!(launch_ready_unit(&mut state, factory));
    for _ in 0..2 {
        production_turn(&mut state);
    }

    assert_eq!(
        count_type(&state, UnitType::Infantry),
        3,
        "repeat: 3 юнита за 6 ходов по 2 хода на юнита (каждый выведен)"
    );
    // 6 "build turns" × 1 material: three orders of 2 turns.
    assert_eq!(
        state.snapshot().unit(dump).expect("dump").storage,
        14,
        "6 ходов × 1 материал"
    );
}

#[test]
fn factory_produces_with_zero_gold_and_pauses_until_materials_return() {
    // A real player scenario: credits spent during mobilization (0 gold),
    // the factory must work on mine materials and stop only when
    // materials run out — C++ `ValidateIndustry` (`production_manager.cpp:673`).
    let mut state = state_with_gold(0);
    let dump = add_materials(&mut state, 1);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");
    power_the_complex(&mut state, factory);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    drop(manager);

    // Turn 1: 1 material left, construction advanced 2 → 1.
    production_turn(&mut state);
    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 0);
    assert_eq!(
        state.snapshot().unit(factory).map(|unit| unit.build_time),
        Some(1)
    );

    // Turn 2: no materials — pause, `build_time` does not advance, the order is intact.
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::PausedNoMaterials),
        "ожидалась пауза: {events:?}"
    );
    assert_eq!(
        state.snapshot().unit(factory).map(|unit| unit.build_time),
        Some(1)
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(state.state().team_units[0].gold(), 0, "золото не тратится");

    // Material delivery (mine/storage): the construction reaches the end from the same place.
    state.unit_mut(dump).expect("dump").storage = 5;
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "после подвоза завод обязан выпустить юнита: {events:?}"
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 1);
    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 4);
}

#[test]
fn constructor_builds_a_building_on_a_site() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    // A real landing gives the constructor 40 raw (`DEFAULT_MISSION_LOADOUT`);
    // `BuildMenu::Build` requires the whole construction cost on hand
    // (`buildmenu.cpp:903-906`: 2 × 10 turns = 20).
    state.unit_mut(constructor).expect("builder").storage = 40;

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
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
        state.state().team_units[0].gold(),
        100,
        "стройка платится сырьём со склада, золото команды не тратится"
    );
    assert_eq!(
        state
            .snapshot()
            .unit(constructor)
            .expect("constructor alive")
            .storage,
        20,
        "40 − 2 × 10 ходов = 20 сырья"
    );

    // C++ `SpawnNewUnit`: the builder completed and waits to leave the site
    // (`ORDER_BUILD`/`ORDER_STATE_UNIT_READY`), the queue is cleared (!path).
    let builder = state
        .snapshot()
        .unit(constructor)
        .expect("constructor alive");
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
    power_the_complex(&mut state, factory);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    manager.progress();
    assert!(manager.cancel(factory), "постройка отменяется");
    assert!(manager.queue(factory).is_empty());
    drop(manager);

    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 19);
    assert_eq!(
        state.snapshot().unit(factory).map(|unit| unit.state),
        Some(UnitOrderState::BuildCancel)
    );

    for _ in 0..4 {
        production_turn(&mut state);
    }

    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(
        state.snapshot().unit(dump).expect("dump").storage,
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

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
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
    let dump = add_materials(&mut state, 1);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");
    power_the_complex(&mut state, factory);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    drop(manager);

    // The raw material was "taken away" after the start: the turn is not paid, the construction
    // pauses, but the order remains (C++ `ValidateIndustry`; the port does not lose the queue).
    state.unit_mut(dump).expect("dump").storage = 0;

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
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
        state.snapshot().unit(factory).map(|unit| unit.build_time),
        Some(2),
        "build_time не двигается на паузе"
    );
    assert_eq!(
        state.state().team_units[0].gold(),
        100,
        "золото не тратится на производство"
    );
}

#[test]
fn two_speed_build_costs_the_cpp_rate() {
    let mut state = state_with_gold(0);
    // `ADUMP` holds 50 units (`ATTRIB_STORAGE`); `manage_economy` distributes
    // the complex materials over storages — we take exactly the capacity.
    let dump = add_materials(&mut state, 50);
    let factory = state
        .spawn_unit(
            UnitType::HeavyVehiclePlant,
            TeamId::RED,
            Point::new(10, 10),
            0,
        )
        .expect("heavy vehicle plant");
    power_the_complex(&mut state, factory);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(factory, UnitType::Tank).expect("queue");
    manager.set_build_rate(factory, 2);
    let report = manager.commit(factory).expect("build");

    assert_eq!(report.build_rate, 2);
    assert_eq!(report.cost_per_turn, 12, "3 × 4 (множитель 2× в C++)");

    production_turn(&mut state);
    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 38);
    assert_eq!(
        state.snapshot().unit(factory).map(|unit| unit.build_time),
        Some(2)
    );

    production_turn(&mut state);
    assert_eq!(count_type(&state, UnitType::Tank), 1);
    assert_eq!(state.snapshot().unit(dump).expect("dump").storage, 26);
}

#[test]
fn mobile_two_speed_build_spends_storage_and_matches_the_menu_cost() {
    // A real player scenario: the constructor has 40 raw in cargo (as
    // `deploy_starting_units`), a 2× order. C++ `UnitInfo::UpdateProduction`
    // reduces `storage` every turn; when it is insufficient for 2×, the rate
    // drops to 1×. Result — exactly (8 turns, 40 gold), as in the `Cost` column
    // `BuildMenu` (`turns_and_consumption`).
    let mut state = state_with_gold(1000);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    state.unit_mut(constructor).expect("builder").storage = 40;

    let before = state.snapshot().unit(constructor).expect("builder").clone();
    let expected = turns_and_consumption(state.state(), &before, UnitType::MiningStation, 2);
    assert_eq!(expected, (8, 40));

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
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
        state.state().team_units[0].gold(),
        1000,
        "мобильный строитель платит сырьём со склада (C++ \
         `Cargo_GetRawConsumptionRate` → `UpdateProduction`), не золотом"
    );
    assert_eq!(
        state.snapshot().unit(constructor).expect("builder").storage,
        0,
        "C++ `storage -= Cargo_GetRawConsumptionRate(...)`: 40 → 0"
    );
    assert!(
        state.snapshot().unit(constructor).expect("builder").storage >= 0,
        "склад не уходит в минус"
    );
}

// ---------------------------------------------------------------------------
// Construction site (C++ `DeployConstructionSiteUtilities` + `PrepareConstructionSite`)
// ---------------------------------------------------------------------------

/// All team units on the `site` cell (C++ `Hash_MapHash[Point]`).
fn units_at(state: &GameUniverse, site: Point) -> Vec<&max_core::unit::UnitInfo> {
    state
        .snapshot()
        .units_at(site)
        .iter()
        .filter_map(|id| state.snapshot().unit(*id))
        .collect()
}

fn find_at<'a>(
    state: &'a GameUniverse,
    site: Point,
    unit_type: UnitType,
) -> Option<&'a max_core::unit::UnitInfo> {
    units_at(state, site)
        .into_iter()
        .find(|unit| unit.unit_type == unit_type)
}

/// C++ `DeployConstructionSiteUtilities` + `PrepareConstructionSite`: the constructor's
/// order places the **large** site `LRGTAPE`/`LRGCONES`/`LRGSLAB`
/// exactly on the construction cell, all — `AWAIT`/`EXECUTING_ORDER`, the tape with
/// the builder parent, the slab — a random frame `0..5`.
#[test]
fn constructor_order_deploys_the_large_construction_site() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    // `UnitsManager_DeployUnit` zeroes the storage; landing gives 40.
    state.unit_mut(constructor).expect("constructor").storage = 40;

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(constructor).expect("build order");
    let site = report.site;

    // C++ layout: cones and tape — on the site cell, large decals.
    assert_eq!(
        construction_utility_types(UnitType::TrainingHall),
        (UnitType::LargeTape, UnitType::LargeCones)
    );
    assert_eq!(
        construction_slab_type(UnitType::TrainingHall),
        UnitType::LargeSlab
    );
    assert_eq!(
        construction_utility(state.state(), TeamId::RED, site),
        state
            .snapshot()
            .units_at(site)
            .iter()
            .copied()
            .find(|id| state
                .snapshot()
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

    // Exactly one set: a repeated call (as from start_building/the order
    // machine) does not spawn a second site and does not burn RNG.
    let mut duplicate_state = state.clone();
    let rng_before = duplicate_state.state().rng;
    let duplicate = max_core::production::deploy_construction_site(
        duplicate_state.state_mut(),
        constructor,
        UnitType::TrainingHall,
        site,
    );
    assert!(
        duplicate.is_none(),
        "площадка уже стоит — повторный спавн запрещён"
    );
    assert_eq!(duplicate_state.state().rng, rng_before, "RNG не сдвинут");
}

/// The engineer builds a small object (`ROAD`): `SMLTAPE`/`SMLCONES`, no slab
/// (`REQUIRES_SLAB` is not set for the road).
#[test]
fn engineer_order_deploys_the_small_construction_site_without_a_slab() {
    let mut state = state_with_gold(100);
    let engineer = state
        .spawn_unit(UnitType::Engineer, TeamId::RED, Point::new(20, 20), 0)
        .expect("engineer");
    state.unit_mut(engineer).expect("engineer").storage = 40;

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
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

/// Water/coast: C++ `DeployConstructionSiteUtilities` does not place cones, and
/// the tape gets angle 1; the slab (`REQUIRES_SLAB` for a building) is still placed.
#[test]
fn construction_site_on_water_has_no_cones_and_tape_angle_one() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let site = Point::new(30, 30);
    assert!(state
        .state_mut()
        .map
        .surface
        .set(site.x, site.y, max_core::enums::Surface::WATER));

    let deployed = max_core::production::deploy_construction_site(
        state.state_mut(),
        constructor,
        UnitType::LightVehiclePlant,
        site,
    )
    .expect("площадка на воде");

    assert_eq!(deployed.cones, None, "на воде конусы не ставятся");
    let tape = state.snapshot().unit(deployed.tape).expect("лента");
    assert_eq!(tape.angle, 1, "C++: на воде/побережье unit_angle = 1");
    assert!(
        find_at(&state, site, UnitType::LargeSlab).is_some(),
        "плита здания ставится и на воде"
    );
}

/// Prepares a game with a completed building: constructor order + `turns` production
/// turns. Returns the builder id and the site cell.
fn state_with_finished_building(turns: usize) -> (GameUniverse, max_core::UnitId, Point) {
    let mut state = state_with_gold(100);
    // Land: otherwise `Access_IsAccessible` does not find a cell to leave the site.
    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            let _ = state
                .state_mut()
                .map
                .surface
                .set(x, y, max_core::enums::Surface::LAND);
        }
    }
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    state.unit_mut(constructor).expect("constructor").storage = 40;

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(constructor).expect("site");
    drop(manager);

    // In game the builder stands on the site (C++ `PositionInTape` enters the
    // tape center); here we put it on the anchor cell so that completion follows
    // the "builder inside the 2×2 footprint" branch.
    if let Some(mut unit) = state.snapshot().unit(constructor).cloned() {
        unit.grid = report.site;
        state.write_unit(constructor, unit);
    }

    for _ in 0..turns {
        production_turn(&mut state);
    }

    (state, constructor, report.site)
}

/// One simulation tick — `GameUniverse::tick` (`GameManager_ProcessTick`): paths
/// (`PollResults`) + the order machine. `process_orders` alone is not enough:
/// `ORDER_MOVE`/`NEW_ORDER` waits for the path request result, which is applied
/// in the tick itself.
fn tick_orders(state: &mut GameUniverse) {
    state.tick();
}

/// Completion 1:1 C++. `SpawnNewUnit` (`unitinfo.cpp:3440`) places the building on
/// the site in `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY` (it is not drawn —
/// `UnitInfoGroup::IsRelevant`) and leaves the builder on the site in
/// `ORDER_BUILD`/`ORDER_STATE_UNIT_READY`. There is **no automatic** exit
/// (`UnitsManager_ProcessOrderBuild`/`UNIT_READY`: `if (path) BuildNext else
/// Animate`): the order is given by the player clicking a cell (`game_manager.cpp:6879-6890`)
/// or by the AI with the `TaskActivate` task. The tape and cones are removed, and the building
/// unfolds (`ScaleUnit(EXPAND)`) only after the builder leaves
/// the site — `UnitsManager_BuildingReady` (`units_manager.cpp:4183`).
#[test]
fn completion_waits_for_the_builder_to_leave_before_the_building_appears() {
    let (mut state, constructor, site) = state_with_finished_building(10);

    // The frame right after `SpawnNewUnit`: the building is created but waits for the builder.
    assert_eq!(count_type(&state, UnitType::TrainingHall), 1);
    let (building_id, building_grid) = {
        let building = find_at(&state, site, UnitType::TrainingHall).expect("здание создано");
        (building.id, building.grid)
    };
    {
        let building = state.snapshot().unit(building_id).expect("здание");
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
        let builder = state
            .snapshot()
            .unit(constructor)
            .expect("constructor alive");
        assert_eq!(
            (builder.orders, builder.state),
            (UnitOrder::Build, UnitOrderState::UnitReady),
            "C++ SpawnNewUnit: строитель достроил и ждёт ухода"
        );
        assert_eq!(builder.parent_unit, Some(building_id), "взаимный родитель");
        assert!(builder.build_list.is_empty(), "!path → build_list.Clear()");
    }

    // While the builder stands on the site, the tape and cones are in place, and the building is not
    // unfolded (no `ORDER_AWAIT_SCALING`).
    assert!(
        construction_utility(state.state(), TeamId::RED, site).is_some(),
        "лента ещё на площадке: здание не раскрыто"
    );
    assert!(
        find_at(&state, site, UnitType::LargeCones).is_some(),
        "конусы ещё на площадке"
    );
    {
        let builder = state
            .snapshot()
            .unit(constructor)
            .expect("constructor alive");
        let building = state.snapshot().unit(building_id).expect("здание");
        assert!(
            production::unit_bounds_contains(building, builder.grid),
            "строитель стоит в габаритах здания"
        );
    }
    assert_eq!(
        production::builder_awaiting_exit(state.state(), constructor),
        Some(building_id),
        "C++: строитель в ORDER_BUILD/UNIT_READY у достроенного здания"
    );

    // The player did **not** give an order: for 30 ticks nothing happens — neither
    // `ORDER_ACTIVATE`, nor site removal, nor building unfolding.
    for _ in 0..30 {
        tick_orders(&mut state);
    }
    {
        let builder = state
            .snapshot()
            .unit(constructor)
            .expect("constructor alive");
        assert_eq!(
            (builder.orders, builder.state),
            (UnitOrder::Build, UnitOrderState::UnitReady),
            "C++ ProcessOrderBuild/UNIT_READY: строитель только анимируется"
        );
        assert_eq!(builder.grid, site, "строитель остаётся на площадке");
    }
    assert!(
        state
            .snapshot()
            .unit(building_id)
            .is_some_and(|unit| unit.orders == UnitOrder::Idle),
        "здание не раскрывается без ухода строителя"
    );
    assert!(construction_utility(state.state(), TeamId::RED, site).is_some());

    // Player order (`game_manager.cpp:6879-6890`): `ORDER_ACTIVATE`/
    // `ORDER_STATE_IN_TRANSITION` with `move_to` = the click cell. In the port this is
    // `unit_order::issue_builder_exit` (the AI path) or packet 08/P3 from the UI; the target is
    // the first passable cell around the building.
    assert!(
        max_core::unit_order::issue_builder_exit(state.state_mut(), constructor),
        "приказ на уход выставлен"
    );
    {
        let builder = state
            .snapshot()
            .unit(constructor)
            .expect("constructor alive");
        assert_eq!(
            (builder.orders, builder.state),
            (UnitOrder::Activate, UnitOrderState::InTransition),
            "C++ порядок игрока: ORDER_ACTIVATE/ORDER_STATE_IN_TRANSITION"
        );
    }

    // Next tick: `PositionInTape` DEINIT/LEAVE — final rotation and exit from the tape.
    tick_orders(&mut state);
    {
        let builder = state
            .snapshot()
            .unit(constructor)
            .expect("constructor alive");
        assert_eq!(
            (builder.orders, builder.state),
            (
                UnitOrder::AwaitTapePositioning,
                UnitOrderState::TapePositioningDeinit
            ),
            "C++ ProcessOrderActivate/IN_TRANSITION → AWAIT_TAPE_POSITIONING/DEINIT"
        );
    }

    // Exit: final rotation + four steps of 8 px (PositionInTape), then
    // `ActivateEngineer` → ORDER_MOVE to a free cell around the building.
    let mut left_site = false;
    for _ in 0..20 {
        tick_orders(&mut state);
        let builder_grid = state.snapshot().unit(constructor).map(|unit| unit.grid);
        let building = state.snapshot().unit(building_id).expect("здание");
        if let Some(grid) = builder_grid {
            left_site = !production::unit_bounds_contains(building, grid);
        }
        if left_site {
            break;
        }
    }
    assert!(left_site, "строитель обязан сойти с площадки");

    // The builder is outside the footprint → `UnitsManager_BuildingReady`: utilities removed,
    // the slab remains, the building unfolds.
    let mut expanded = false;
    for _ in 0..12 {
        tick_orders(&mut state);
        if state
            .snapshot()
            .unit(building_id)
            .is_some_and(|unit| unit.orders == UnitOrder::AwaitScaling)
        {
            expanded = true;
            break;
        }
    }
    assert!(expanded, "C++ BuildingReady: ScaleUnit(building, EXPAND)");

    assert!(
        construction_utility(state.state(), TeamId::RED, site).is_none(),
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
        let building = state.snapshot().unit(building_id).expect("здание");
        assert_eq!(building.scaler_adjust, 4);
        assert_eq!(building.parent_unit, None);
        assert_eq!(
            building.grid, building_grid,
            "здание стоит в клетке площадки"
        );
    }
    assert_eq!(count_type(&state, UnitType::TrainingHall), 1);

    // After unfolding the building returns to `ORDER_AWAIT`, and the builder goes to
    // the exit cell (`ORDER_MOVE`) and on arrival — to `ORDER_AWAIT`.
    for _ in 0..40 {
        tick_orders(&mut state);
    }
    let builder_grid = {
        let building = state.snapshot().unit(building_id).expect("здание");
        assert_eq!(
            (building.orders, building.state),
            (UnitOrder::Await, UnitOrderState::ExecutingOrder),
            "после EXPAND здание в ORDER_AWAIT/EXECUTING_ORDER (сейчас {:?}/{:?}, prior {:?}/{:?})",
            building.orders,
            building.state,
            building.prior_orders,
            building.prior_state
        );
        let builder = state
            .snapshot()
            .unit(constructor)
            .expect("constructor alive");
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

/// `RADAR` must return to `ORDER_SENTRY` after completion.
///
/// C++: `UnitsManager_DeployUnit` (`units_manager.cpp:2412-2421`) puts
/// `RADAR` (like turrets and mines) in `ORDER_SENTRY`; `SpawnNewUnit`
/// (`unitinfo.cpp:3581`) moves the building to `ORDER_IDLE`/
/// `ORDER_STATE_BUILDING_READY` via **`SetNewOrderInt`**, which preserves
/// the current order in `prior_orders` (`units_manager.cpp:1969-1975`). After the builder
/// leaves, `UnitsManager_BuildingReady` (`units_manager.cpp:4240`) does
/// `RestoreOrders` (`unitinfo.cpp:3783`) → `ORDER_SENTRY` again, and
/// `ProcessOrderSentry` cycles through 16 `D_RADAR` frames (`ANIMATED`). The port previously
/// hard-coded `prior = AWAIT`, so the radar froze in `ORDER_AWAIT`.
#[test]
fn a_built_radar_restores_its_sentry_order() {
    let mut state = state_with_gold(400);

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let _ = state.state_mut().map.surface.set(x, y, Surface::LAND);
        }
    }

    // `RADAR` is built by an **engineer** (`builder_type(Radar) == Engineer`), but the
    // `SpawnNewUnit` branch for a mobile builder is the same as for the constructor.
    let constructor = state
        .spawn_unit(UnitType::Engineer, TeamId::RED, Point::new(10, 10), 0)
        .expect("engineer");
    state.unit_mut(constructor).expect("engineer").storage = 40;

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager
        .enqueue(constructor, UnitType::Radar)
        .expect("queue");
    let report = manager.commit(constructor).expect("site");
    drop(manager);

    if let Some(mut unit) = state.snapshot().unit(constructor).cloned() {
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
        let builder = state.snapshot().unit(constructor).expect("builder");
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
        let radar = state.snapshot().unit(radar_id).expect("radar");
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

    // The exit order is given by the player/AI (`issue_builder_exit`) — see A1.
    assert!(max_core::unit_order::issue_builder_exit(
        state.state_mut(),
        constructor
    ));

    let mut restored = false;
    for _ in 0..60 {
        tick_orders(&mut state);

        if state
            .snapshot()
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
    let radar = state.snapshot().unit(radar_id).expect("radar");
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

/// Completion is deterministic: two identical runs give one snapshot on
/// every frame (the "builder exit → building unfold" order does not depend on
/// hash iteration).
#[test]
fn completion_sequence_is_deterministic() {
    fn run() -> Vec<Vec<u8>> {
        let (mut state, constructor, _) = state_with_finished_building(10);
        let mut frames = vec![state.snapshot().snapshot_bytes()];

        // Player exit order (`ORDER_ACTIVATE`), then ticks until unfolding.
        assert!(max_core::unit_order::issue_builder_exit(
            state.state_mut(),
            constructor
        ));

        for _ in 0..30 {
            tick_orders(&mut state);
            frames.push(state.snapshot().snapshot_bytes());
        }

        frames
    }

    assert_eq!(run(), run());
}

/// `stop` of a mobile builder (`CancelBuilding` → `BuildCancel`) removes
/// the site from the order cell: tape, cones and slab do not remain on the map.
#[test]
fn cancelling_the_build_removes_the_site() {
    let mut state = state_with_gold(100);
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    state.unit_mut(constructor).expect("constructor").storage = 40;

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(constructor).expect("site");
    let site = report.site;
    drop(manager);
    assert!(construction_utility(state.state(), TeamId::RED, site).is_some());

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    assert!(manager.cancel(constructor));
    drop(manager);

    // `CancelBuilding` is executed by the order processor on the next tick.
    state.state_mut().process_orders();

    assert_eq!(count_type(&state, UnitType::LargeTape), 0);
    assert_eq!(count_type(&state, UnitType::LargeCones), 0);
    assert_eq!(count_type(&state, UnitType::LargeSlab), 0);
    assert_eq!(count_type(&state, UnitType::TrainingHall), 0);
    assert_eq!(
        state.snapshot().unit(constructor).map(|unit| unit.orders),
        Some(UnitOrder::Await)
    );
}

/// Site layout and RNG order are deterministic: two identical orders
/// give the same types/cells/frames and the same snapshot.
#[test]
fn construction_site_layout_is_deterministic() {
    fn run() -> (Vec<u8>, Point, Vec<(UnitType, Point, u8)>) {
        let mut state = state_with_gold(100);
        let constructor = state
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(30, 30), 0)
            .expect("constructor");
        state.unit_mut(constructor).expect("constructor").storage = 40;

        let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
        manager
            .enqueue(constructor, UnitType::TrainingHall)
            .expect("queue");
        let report = manager.commit(constructor).expect("site");

        let mut layout: Vec<(UnitType, Point, u8)> = state
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.snapshot().unit(id))
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

        (state.snapshot().snapshot_bytes(), report.site, layout)
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
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn two_identical_production_runs_have_identical_state() {
    fn run() -> Vec<u8> {
        let mut state = state_with_gold(100);
        add_materials(&mut state, 100);
        let factory = state
            .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
            .expect("training hall");
        power_the_complex(&mut state, factory);
        let constructor = state
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(20, 20), 0)
            .expect("constructor");
        state.unit_mut(constructor).expect("constructor").storage = 40;

        let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
        manager.enqueue(factory, UnitType::Infantry).expect("queue");
        manager
            .enqueue(factory, UnitType::Infiltrator)
            .expect("queue");
        manager.commit(factory).expect("build");
        manager
            .enqueue(constructor, UnitType::LightVehiclePlant)
            .expect("constructor queue");
        manager.commit(constructor).expect("constructor build");

        for _ in 0..14 {
            production_turn(&mut state);
        }

        state.snapshot().snapshot_bytes()
    }

    assert_eq!(
        run(),
        run(),
        "два прогона производства совпадают байт-в-байт"
    );
}

#[test]
fn awac_completes_from_the_merged_complex_pot() {
    // User scenario: the AWAC "never completes" because the factory
    // did not see raw material. C++ (`Cargo_GetNetProduction` → `Cargo_ApplyUnitConsumption`)
    // charges raw from the **complex** (`Complex::GetCargoInfo` = the sum of connected
    // building storages), not from one building. Here two ADUMPs stand
    // right next to AIRPLT and are merged by connectors into one complex: the AWAC is built
    // in 6 turns; the first turn 4× (36), the last — `GetMaxAllowedBuildRate`
    // limits to 2× by the remaining `build_time = 2` (12), 48 of 80 in total.
    let mut state = state_with_gold(0);
    let factory = state
        .spawn_unit(UnitType::AirUnitsPlant, TeamId::RED, Point::new(10, 10), 0)
        .expect("air plant");
    let first = state
        .spawn_unit(
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(12, 10),
            0,
        )
        .expect("MaterialStorage 1");
    // `ADUMP` is a 1×1 `STANDALONE` (`UpdateConnectors` gives `unit_size = 1`),
    // so the second storage is placed right next to it: `(12,10)` and `(13,10)`.
    let second = state
        .spawn_unit(
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(13, 10),
            0,
        )
        .expect("MaterialStorage 2");
    state.unit_mut(first).expect("MaterialStorage 1").storage = 40;
    state.unit_mut(second).expect("MaterialStorage 2").storage = 40;

    let complex = state
        .snapshot()
        .unit(factory)
        .expect("factory")
        .complex
        .expect("complex");
    assert_eq!(
        state
            .snapshot()
            .unit(first)
            .expect("MaterialStorage 1")
            .complex,
        Some(complex)
    );
    assert_eq!(
        state
            .snapshot()
            .unit(second)
            .expect("MaterialStorage 2")
            .complex,
        Some(complex)
    );
    assert_eq!(
        state.state().team_units[0]
            .get_complex(complex)
            .expect("complex")
            .buildings,
        3,
        "завод и два склада — один комплекс"
    );
    assert_eq!(
        max_core::production::complex_raw_materials(state.state(), TeamId::RED, complex),
        80,
        "общий котёл комплекса"
    );
    power_the_complex(&mut state, factory);

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(factory, UnitType::Awac).expect("queue");
    manager.set_build_rate(factory, 4);
    let report = manager.commit(factory).expect("build");
    drop(manager);

    assert_eq!(report.build_rate, 4, "AIRPLT ускоряется до 4×");
    assert_eq!(report.cost_per_turn, 36, "3 × 12 (множитель 4×)");

    // Turn 1: 36 from the common pool, `build_time` 6 → 2.
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .all(|event| event.kind != ProductionEventKind::PausedNoMaterials),
        "первый ход оплачен из комплекса: {events:?}"
    );
    assert_eq!(
        state.snapshot().unit(factory).expect("factory").build_time,
        2
    );
    assert_eq!(
        max_core::production::complex_raw_materials(state.state(), TeamId::RED, complex),
        44
    );

    // Turn 2: `GetMaxAllowedBuildRate` cuts 4× to 2× (2 turns remain) —
    // 12 from the common pool; `build_time <= build_rate` → `SpawnNewUnit` (AWAC).
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "AWAC обязан выпуститься: {events:?}"
    );
    assert_eq!(count_type(&state, UnitType::Awac), 1, "AWAC достроен");
    assert_eq!(
        max_core::production::complex_raw_materials(state.state(), TeamId::RED, complex),
        32,
        "80 − 36 (4×) − 12 (2× на последнем ходу)"
    );
    assert_eq!(state.state().team_units[0].gold(), 0, "золото не тратится");
}

// ---------------------------------------------------------------------------
// `finish_turn` — production turn integration
// ---------------------------------------------------------------------------

#[test]
fn finish_turn_runs_the_production_phase() {
    let mut state = state_with_gold(100);
    // A duel as in a game: RED is a human, GREEN is a computer, the network branch
    // is disabled (local semantics of `menu_check_end_game_conditions`).
    state.state_mut().teams[1].team_type = TeamKind::Computer;
    state.state_mut().team_units[1] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    state.state_mut().team_units[1].set_gold(100);
    state.state_mut().network_game = false;
    add_materials(&mut state, 20);
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");
    power_the_complex(&mut state, factory);
    state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(14, 14), 0)
        .expect("constructor");
    // Combat power keeps the team "alive" by generic rules (`SC_L0002`):
    // without it `menu_check_end_game_conditions` closes the game.
    state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(16, 16), 0)
        .expect("red tank");
    state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(40, 40), 0)
        .expect("green tank");

    let mut manager = ProductionManager::new(state.state_mut(), TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("build");
    drop(manager);

    state.state_mut().game_state = max_core::enums::GameStateType::EndTurn;

    let status = max_core::victory::evaluate_status(state.state(), state.state().turn_counter);
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
    assert_eq!(state.state().turn_counter, 1);
    assert_eq!(
        state.state().team_units[0].gold(),
        100,
        "золото команды производством не тратится"
    );
    assert_eq!(
        state
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.snapshot().unit(id))
            .find(|unit| unit.unit_type == UnitType::MaterialStorage)
            .map(|unit| unit.storage),
        Some(19),
        "ход завода списан материалами (1 за INFANTRY)"
    );
    assert_eq!(
        state.snapshot().unit(factory).map(|unit| unit.build_time),
        Some(1)
    );
}

/// Team color — `TeamUnits::hash_team_id` (`resource_manager.cpp:969-981`):
/// RED = `HASH_TEAM_RED` 0x2000, GREEN = 0x1000, BLUE = 0x800, GRAY = 0x400,
/// ALIEN/derelict = 0x8000. C++ writes this bit into unit flags
/// (`unitinfo.cpp:179`), the port restores the color by the same mapping
/// (flags enter the reference digest and remain without the bits).
#[test]
fn team_hash_ids_match_the_cpp_color_slots() {
    assert_eq!(max_core::team::hash_team_id(TeamId::RED), 0x2000);
    assert_eq!(max_core::team::hash_team_id(TeamId::GREEN), 0x1000);
    assert_eq!(max_core::team::hash_team_id(TeamId::BLUE), 0x0800);
    assert_eq!(max_core::team::hash_team_id(TeamId::GRAY), 0x0400);
    assert_eq!(max_core::team::hash_team_id(TeamId::ALIEN), 0x8000);
}

/// Complex materials — the sum of storages of **connected** buildings with
/// `CARGO_TYPE_RAW` (C++ `Complex::GetCargoInfo`, `complex.cpp:136-147`), not
/// of the whole team: an unconnected storage does not enter the factory pool.
#[test]
fn complex_raw_materials_sums_only_connected_raw_storage() {
    let mut state = state_with_gold(0);
    let dump = add_materials(&mut state, 30);
    let mine = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(30, 30), 0)
        .expect("mine");
    state.unit_mut(mine).expect("mine").storage = 7;
    // Non-raw buildings and mobile units do not enter the materials storage.
    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(40, 40), 0)
        .expect("tank");
    state.unit_mut(tank).expect("tank").storage = 9;
    let gas = state
        .spawn_unit(UnitType::FuelStorage, TeamId::RED, Point::new(50, 50), 0)
        .expect("fuel dump");
    state.unit_mut(gas).expect("fuel dump").storage = 11;

    let dump_complex = state
        .snapshot()
        .unit(dump)
        .expect("dump")
        .complex
        .expect("complex");
    let mine_complex = state
        .snapshot()
        .unit(mine)
        .expect("mine")
        .complex
        .expect("complex");
    assert_ne!(dump_complex, mine_complex, "склады не связаны коннекторами");

    assert_eq!(
        max_core::production::complex_raw_materials(state.state(), TeamId::RED, dump_complex),
        30,
        "ADUMP — свой котёл"
    );
    assert_eq!(
        max_core::production::complex_raw_materials(state.state(), TeamId::RED, mine_complex),
        7,
        "шахта — свой котёл"
    );
    assert_eq!(
        max_core::production::complex_raw_materials(state.state(), TeamId::GREEN, dump_complex),
        0,
        "чужая команда"
    );
    assert_eq!(
        state.snapshot().unit(dump).expect("dump").storage,
        30,
        "склад не тронут"
    );
}

/// Building/unit flags — checking that factories are really stationary.
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
