//! Phase 5 — AI production and construction (area A.3).
//!
//! Scenarios: an AI base + a passive enemy base → rounds through the core turn
//! machine → the construction tasks the manager plans, the completed buildings
//! and the unit production rolls.
//!
//! Oracles: `taskmanagebuildings.cpp` (`PlanNextBuildJob` `:770`,
//! `CreateBuilding` `:2360`, `CreateBuildings` `:997`),
//! `taskattackreserve.cpp:136-164` (the factory branch the port performs in
//! `plan_unit_production`), `aiplayer.cpp:945/3387` (`GetWeightTable` /
//! `GetFilteredWeightTable`).

mod support;

use max_core::enums::{AiStrategy, Opponent, UnitOrder, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use support::phase5::{
    add_vehicle_plant, count_units, create_building_types, factory_unit_requests, green_base,
    play_rounds, play_rounds_observed, red_base, sentry, spawn, strategy_state, AiObservation,
    ROUND_TICKS,
};

/// A tank-horde base with the light plant already standing plans the heavy
/// vehicle plant — the factory its strategy expands to (and the second factory
/// of every base, `taskmanagebuildings.cpp:770`).
#[test]
fn ai_orders_the_heavy_plant_for_a_tank_horde() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    let light_plant = spawn(
        &mut state,
        UnitType::LightVehiclePlant,
        TeamId::RED,
        Point::new(16, 16),
    );
    state.unit_mut(light_plant).expect("light plant").orders = UnitOrder::PowerOn;

    let mut observation = AiObservation::default();
    let (_, over) = play_rounds_observed(&mut state, 3, ROUND_TICKS, TeamId::RED, &mut observation);
    assert!(!over, "the match must stay open");

    let planned = create_building_types(&state, TeamId::RED);
    assert!(
        planned.contains(&UnitType::HeavyVehiclePlant),
        "TankHorde must plan its vehicle plant, planned: {planned:?}"
    );
    assert!(
        observation.max_create_building_tasks >= 1,
        "TaskCreateBuilding must be alive during the turn"
    );
    assert!(
        !planned.contains(&UnitType::AirUnitsPlant)
            && !planned.contains(&UnitType::Shipyard)
            && !planned.contains(&UnitType::TrainingHall),
        "the strategy must not rush the other factories, planned: {planned:?}"
    );
}

/// From a bare base the AI orders and completes a mining station through the
/// real turn machine: `CheckNeeds` → `TaskCreateBuilding(MININGST)` →
/// `ORDER_BUILD` on the constructor → `ProgressBuildState`.
#[test]
fn ai_builds_a_mining_station_from_scratch() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    let mut observation = AiObservation::default();
    let (_, over) =
        play_rounds_observed(&mut state, 14, ROUND_TICKS, TeamId::RED, &mut observation);

    assert!(
        observation.saw_constructor_build_order,
        "the constructor must receive ORDER_BUILD"
    );
    assert!(
        count_units(&state, TeamId::RED, UnitType::MiningStation) >= 1,
        "the AI must complete its mining station, over={over}"
    );
}

/// `plan_unit_production` (`taskattackreserve.cpp:136-164`): an idle factory
/// with a raw/fuel stockpile rolls a type from the composed filtered weight
/// table and orders it through `TaskObtainUnits`.
///
/// The composed table (`GetWeightTable(INVALID_ID)`) includes the table of
/// every **spotted** enemy type (`aiplayer.cpp:945-970`), so a heavy plant only
/// has a rollable type once an enemy tank is seen: that is the C++ behavior the
/// scenario reproduces (spotted tank → tanks table → `TANK` ×10 for TankHorde).
#[test]
fn ai_orders_a_tank_from_the_heavy_plant_when_an_enemy_tank_is_spotted() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    let red = red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    // The forward enemy tank is inside the base tank's scan (4 cells).
    let _enemy = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(11, 13),
    );

    add_vehicle_plant(&mut state, Point::new(16, 16));

    // The base tank scans (`ORDER_SENTRY`) and the coverage is recomputed as
    // after a landing; the spotted type then enters the composed weight table.
    sentry(&mut state, red.tank);
    support::phase5::refresh_visibility(&mut state);
    assert_eq!(
        state.snapshot().ai().players[TeamId::RED.0 as usize]
            .spotted_units
            .len(),
        1,
        "the enemy tank must be spotted before production can react to it"
    );

    let (_, over) = play_rounds(&mut state, 2, ROUND_TICKS);
    assert!(!over);

    let requested = factory_unit_requests(&state, TeamId::RED, UnitType::HeavyVehiclePlant);
    assert!(
        requested.contains(&UnitType::Tank),
        "the heavy plant must order a tank, requests: {requested:?}"
    );

    // The roll came from the TankHorde weight table (base TANK weight 2 × 10).
    let player = &state.snapshot().ai().players[TeamId::RED.0 as usize];
    assert_eq!(player.weight_table_tanks.get_weight(UnitType::Tank), 20);
    let filtered = state.snapshot().get_filtered_weight_table(player, None, 0x03);
    assert!(
        filtered.get_weight(UnitType::Tank) > 0,
        "the spotted tank keeps TANK in the composed table"
    );
}

/// The same plant without a raw/fuel stockpile cannot start a construction:
/// `plan_unit_production` requires `material > 10 && fuel > 10` of the complex.
#[test]
fn ai_does_not_order_units_without_complex_stock() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    let red = red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    let _enemy = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(11, 13),
    );

    // A plant without dumps: the complex cache stays at zero.
    let plant = spawn(
        &mut state,
        UnitType::HeavyVehiclePlant,
        TeamId::RED,
        Point::new(16, 16),
    );
    state.unit_mut(plant).expect("plant").orders = UnitOrder::PowerOn;

    sentry(&mut state, red.tank);
    support::phase5::refresh_visibility(&mut state);

    let (_, over) = play_rounds(&mut state, 2, ROUND_TICKS);
    assert!(!over);

    assert!(
        factory_unit_requests(&state, TeamId::RED, UnitType::HeavyVehiclePlant).is_empty(),
        "an empty complex must not order production"
    );
}
