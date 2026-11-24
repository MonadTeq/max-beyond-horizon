//! Resource timing of the turn machine — 1:1 by the C++ oracle.
//!
//! Oracle:
//! * `GameManager_RefreshOrders` (`game_manager.cpp:838-891`), run when a team
//!   **ends its turn** (before packet 06): `Access_UpdateResourcesTotal` credits the
//!   turn's mining into the complex stocks, and only then `GameManager_OptimizeProduction`
//!   determines the shortage;
//! * `GameManager_TeamTurnFinish` (`:583`) closes the round — no economy;
//! * the economy of a team runs at the **start of its own turn**:
//!   `GameManager_ManageEconomy` (`:2058`: `OptimizeComplexes` → income credit and
//!   shortage, `UpdateProductions` → mobile builder charge / storage distribution /
//!   speed reset) then `GameManager_ProgressBuildState` (`:8183`: `SpawnNewUnits` —
//!   the factory pays from the already credited stock and advances the build).
//!
//! The manual defect: the port ran the production phase in `finish_turn` for all
//! teams, i.e. before the next `manage_economy`, so a factory was charged and
//! judged shortage-stricken on the previous turn's storages.

mod support;

use support::{game, spawn};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::universe::GameUniverse;

/// The complex's cached stock (`Complex::raw`, `Access_UpdateResourcesTotal`).
fn complex_raw(state: &GameUniverse, team: TeamId) -> i32 {
    let units = state.snapshot().team_units().get(team.0 as usize).expect("team");
    let complex = units.complexes().first().expect("complex").complex_id();
    i32::from(
        state
            .snapshot()
            .complex(team, complex.0 as u16)
            .expect("complex")
            .raw,
    )
}

/// Total raw material held by the complex members (sum of storages).
fn stored_raw(state: &GameUniverse, team: TeamId) -> i32 {
    state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.snapshot().unit(id))
        .filter(|unit| unit.team == team)
        .filter(|unit| max_core::cargo_type_of(unit.unit_type) == max_core::CargoType::Raw)
        .map(|unit| i32::from(unit.storage))
        .sum()
}

/// A mine that produces 4 raw per turn (`Cargo_GetNetProduction` for a human team).
fn producing_mine(state: &mut GameUniverse, team: TeamId, grid: Point) -> max_core::UnitId {
    let id = spawn(state, UnitType::MiningStation, team, grid);

    let unit = state.unit_mut(id).expect("mine");
    unit.orders = UnitOrder::PowerOn;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.prior_orders = UnitOrder::PowerOn;
    unit.prior_state = UnitOrderState::ExecutingOrder;
    unit.raw_mining = 4;
    unit.total_mining = 4;
    unit.storage = 0;
    id
}

/// `GameManager_RefreshOrders` (end of turn, packet 06): the mining of the turn is
/// credited into the complex stock **before** any shortage is considered.
#[test]
fn ending_the_turn_credits_the_mining_income() {
    let mut state = game();
    let mine = producing_mine(&mut state, TeamId::RED, Point::new(10, 10));
    // Keep the match alive (`menu_check_end_game_conditions`).
    spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(20, 20));
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(40, 40));

    // Enter the round (`GameManager_TeamTurnTurnBased`): RED's turn starts.
    state.begin_local_round();

    // Empty the stores: the turn's mining must be what gets credited. (The complex
    // cache is a projection recomputed by `Access_UpdateResourcesTotal`; the end of
    // the turn recomputes it from the now empty inventory plus the mining.)
    state.unit_mut(mine).expect("mine").storage = 0;

    // RED ends its turn: `06` → `GameManager_RefreshOrders` (income credit).
    let over = state.end_local_turn(TeamId::RED);
    assert!(!over, "the match continues");

    assert_eq!(
        complex_raw(&state, TeamId::RED),
        4,
        "the 4 raw mined this turn are credited into the complex at the end of the turn"
    );
    assert_eq!(
        state.snapshot().unit(mine).map(|unit| unit.raw_mining),
        Some(4)
    );

    // GREEN ends its turn too: the round closes; RED's next turn start distributes
    // the credited stock into the member storages (`UnitInfo::UpdateProduction`).
    assert!(!state.end_local_turn(TeamId::GREEN));
    assert_eq!(
        stored_raw(&state, TeamId::RED),
        4,
        "the credited mining is distributed over the complex storages at the turn start"
    );
}

/// The factory pays at the **team turn start**, after `ManageEconomy` has credited
/// and distributed this turn's mining — not in `finish_turn` and not before the
/// income (`finish_turn` only closes the round).
#[test]
fn factory_pays_at_the_team_turn_start_after_the_income_is_credited() {
    let mut state = game();
    // Factory complex: Training Hall + ADUMP (empty) + mine (4 raw/turn) + power.
    let factory = spawn(&mut state, UnitType::TrainingHall, TeamId::RED, Point::new(10, 10));
    let dump = spawn(&mut state, UnitType::MaterialStorage, TeamId::RED, Point::new(12, 10));
    let mine = producing_mine(&mut state, TeamId::RED, Point::new(14, 10));
    spawn(&mut state, UnitType::Connector, TeamId::RED, Point::new(13, 10));
    let generator = spawn(
        &mut state,
        UnitType::PowerGenerator,
        TeamId::RED,
        Point::new(9, 10),
    );
    // A second generator: the mine and the factory consume 1 power each, one
    // POWGEN gives 1 (`Cargo_GetPowerConsumptionRate`).
    let generator2 = spawn(
        &mut state,
        UnitType::PowerGenerator,
        TeamId::RED,
        Point::new(7, 10),
    );
    let fuel = spawn(&mut state, UnitType::FuelStorage, TeamId::RED, Point::new(8, 10));
    let habitat = spawn(&mut state, UnitType::Habitat, TeamId::RED, Point::new(12, 11));
    state.unit_mut(fuel).expect("fuel").storage = 50;
    state.unit_mut(dump).expect("dump").storage = 0;

    let mine_complex = state.snapshot().unit(mine).expect("mine").complex;
    for id in [factory, dump, generator, generator2, fuel, habitat, mine] {
        assert_eq!(
            state.snapshot().unit(id).expect("complex member").complex,
            mine_complex,
            "the fixture must be one complex"
        );
    }

    // Keep the match alive (`menu_check_end_game_conditions`).
    spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(20, 20));
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(40, 40));

    // Enter the round: RED's turn starts (its economy credits/distributes once).
    state.begin_local_round();
    state.unit_mut(mine).expect("mine").storage = 0;
    state.unit_mut(dump).expect("dump").storage = 0;

    // Start the construction directly (the `BuildMenu` first-turn affordability
    // check is a different concern): 2 turns at 1×.
    {
        let mut unit = state.snapshot().unit(factory).expect("factory").clone();
        unit.build_list = vec![UnitType::Infantry];
        unit.build_time = 2;
        unit.build_rate = 1;
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.build_paused = false;
        state.write_unit(factory, unit);
    }

    // RED ends its turn: the income is credited, no production is charged yet.
    assert!(!state.end_local_turn(TeamId::RED));
    assert_eq!(
        complex_raw(&state, TeamId::RED),
        4,
        "the end of the turn credits the 4 mined raw into the complex"
    );
    assert_eq!(
        state.snapshot().unit(factory).expect("factory").build_time,
        2,
        "the end of the turn does not advance the construction"
    );

    // GREEN ends its turn: the round closes and RED's next turn starts — the pair
    // `ManageEconomy` → `ProgressBuildState` runs for RED.
    assert!(!state.end_local_turn(TeamId::GREEN));

    let factory_unit = state.snapshot().unit(factory).expect("factory");
    assert!(
        !factory_unit.build_paused,
        "the factory must not be short: the income was credited before the charge"
    );
    assert_eq!(
        factory_unit.build_time, 1,
        "the factory paid 1 raw of the 4 mined this turn and advanced to 1 turn left"
    );
    assert_eq!(
        stored_raw(&state, TeamId::RED),
        3,
        "4 mined − 1 paid for the turn = 3 left in the complex"
    );
}
