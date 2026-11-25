use super::*;
use crate::enums::UnitType;
use crate::geo::Point;
use crate::TeamUnits;

fn state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = crate::team::TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
    state
}

fn spawn(state: &mut GameState, unit_type: UnitType, grid: Point) -> UnitId {
    state
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .expect("unit")
}

#[test]
fn totals_include_inventory_and_net_production() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(12, 10));
    state.unit_mut(dump).unwrap().storage = 7;

    let complex = state.unit(mine).unwrap().complex.unwrap();
    assert_eq!(
        state.unit(dump).unwrap().complex,
        Some(complex),
            "ADUMP вплотную к шахте — один комплекс"
    );

    let totals = complex_totals(&state, TeamId::RED, complex);
    assert_eq!(
        totals.raw,
        7 + i32::from(state.unit(mine).unwrap().raw_mining)
    );
    assert_eq!(totals.power, -1, "включённая шахта потребляет 1 энергию");
}

#[test]
fn surplus_turns_extra_generators_off_and_deficit_turns_them_on() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(12, 10));
    let gen1 = spawn(&mut state, UnitType::PowerGenerator, Point::new(9, 11));
    let gen2 = spawn(&mut state, UnitType::PowerGenerator, Point::new(9, 12));
    let raw = spawn(&mut state, UnitType::MaterialStorage, Point::new(14, 12));

    state.unit_mut(fuel).unwrap().storage = 40;
    state.unit_mut(raw).unwrap().storage = 10;
    let complex = state.unit(mine).unwrap().complex.unwrap();
    for id in [gen1, gen2] {
        assert_eq!(state.unit(id).unwrap().complex, Some(complex));
    }

    // Demand 1 (mine), one generator suffices: the second must turn off.
    let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, false);
    assert!(outcome.unresolved == false);
    assert_eq!(
        [gen1, gen2]
            .iter()
            .filter(|id| state.unit(**id).unwrap().orders == UnitOrder::PowerOn)
            .count(),
        1,
            "на спрос 1 энергии достаточно одного генератора"
    );

    // Added a factory under construction: demand 2 — both turn on.
    let plant = spawn(&mut state, UnitType::LightVehiclePlant, Point::new(12, 11));
    assert_eq!(state.unit(plant).unwrap().complex, Some(complex));
    assert_eq!(state.unit(raw).unwrap().complex, Some(complex));
    if let Some(unit) = state.unit_mut(plant) {
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::Init;
        unit.build_time = 2;
        unit.build_rate = 1;
        unit.build_list = vec![UnitType::Scout];
    }
    let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, false);
    assert!(
        outcome.paused.is_empty(),
            "генераторов хватает: {outcome:?}"
    );
    assert_eq!(
        [gen1, gen2]
            .iter()
            .filter(|id| state.unit(**id).unwrap().orders == UnitOrder::PowerOn)
            .count(),
        2,
            "на спрос 2 нужны оба генератора"
    );
}

#[test]
fn totals_first_shortage_follows_the_cpp_validation_order() {
    // `ValidateIndustry`/`Cargo_ApplyUnitConsumption` order:
    // materials → power → workers → fuel → gold.
    let mut totals = EnergyTotals::default();
    assert!(!totals.has_deficit());
    assert_eq!(totals.first_shortage(), None);

    totals = EnergyTotals {
        raw: -1,
        fuel: -1,
        gold: -1,
        power: -1,
        life: -1,
    };
    assert!(totals.has_deficit());
    assert_eq!(totals.first_shortage(), Some(Shortage::Raw));

    totals.raw = 0;
    assert_eq!(totals.first_shortage(), Some(Shortage::Power));

    totals.power = 0;
    assert_eq!(totals.first_shortage(), Some(Shortage::Workers));

    totals.life = 0;
    assert_eq!(totals.first_shortage(), Some(Shortage::Fuel));

    totals.fuel = 0;
    assert_eq!(totals.first_shortage(), Some(Shortage::Gold));

    totals.gold = 0;
    assert!(!totals.has_deficit());
}

#[test]
fn shortage_build_error_covers_every_shortage() {
    use crate::production::BuildError;

    assert_eq!(
        shortage_build_error(Shortage::Raw),
        BuildError::NotEnoughMaterials
    );
    assert_eq!(
        shortage_build_error(Shortage::Power),
        BuildError::NotEnoughPower
    );
    assert_eq!(
        shortage_build_error(Shortage::Workers),
        BuildError::NotEnoughWorkers
    );
    assert_eq!(
        shortage_build_error(Shortage::Fuel),
        BuildError::NotEnoughFuel
    );
    assert_eq!(
        shortage_build_error(Shortage::Gold),
        BuildError::NotEnoughGold
    );
}

#[test]
fn power_tables_match_cargo_cpp() {
    // `Cargo_GetPowerConsumptionRate` (cargo.cpp:203).
    assert!(is_power_producer(UnitType::PowerStation));
    assert!(is_power_producer(UnitType::PowerGenerator));
    assert!(!is_power_producer(UnitType::MiningStation));
    assert_eq!(producer_power(UnitType::PowerStation), 6);
    assert_eq!(producer_power(UnitType::PowerGenerator), 1);
    assert_eq!(producer_fuel(UnitType::PowerStation), 6);
    assert_eq!(producer_fuel(UnitType::PowerGenerator), 2);
    assert_eq!(cargo_power_consumption_rate(UnitType::TrainingHall), 1);
    assert_eq!(cargo_power_consumption_rate(UnitType::Tank), 0);

    // `OptimizeAuxilaryIndustry`/`OptimizeMiningIndustry` scope.
    assert!(is_switchable_consumer(UnitType::ResearchCentre));
    assert!(is_switchable_consumer(UnitType::EcoSphere));
    assert!(is_switchable_consumer(UnitType::GoldRefinery));
    assert!(is_switchable_consumer(UnitType::MiningStation));
    assert!(!is_switchable_consumer(UnitType::TrainingHall));
    assert_eq!(
        CONSUMER_OFF_ORDER,
        [
            UnitType::ResearchCentre,
            UnitType::EcoSphere,
            UnitType::GoldRefinery
        ]
    );

    // Message names (`ResourceManager_GetUnit(type).GetSingularName()`).
    assert_eq!(
        industry_unit_name(UnitType::ResearchCentre),
            "Research Center"
    );
    assert_eq!(industry_unit_name(UnitType::EcoSphere), "Greenhouse");
    assert_eq!(industry_unit_name(UnitType::GoldRefinery), "Comm Tower");
    assert_eq!(industry_unit_name(UnitType::Tank), "building");
}

#[test]
fn a_producer_is_not_started_without_fuel() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(9, 10));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(9, 11));

    // `POWGEN` needs 2 fuel (`Cargo_GetFuelConsumptionRate`); only 1 is stocked.
    state.unit_mut(fuel).unwrap().storage = 1;
    let complex = state.unit(mine).unwrap().complex.unwrap();
    assert_eq!(state.unit(generator).unwrap().complex, Some(complex));

    let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, true);

    assert_eq!(
        state.unit(generator).unwrap().orders,
        UnitOrder::PowerOff,
            "POWGEN with 1 fuel cannot cover its rate"
    );
    assert!(
        outcome.message.as_deref().is_some_and(
            |message| message.contains("Cannot turn Power Generator on, fuel needed.")
        ),
            "message: {:?}",
        outcome.message
    );
    // With no producer left the mine becomes the last consumer to switch off.
    assert_eq!(state.unit(mine).unwrap().orders, UnitOrder::PowerOff);
}

/// Regression: demand 1 (a single mine) with a `POWERSTN` and a fuel reserve.
/// C++ plans the station through the second stage of
/// `OptimizePowerProduction` (`production_manager.cpp:519-540`); the port
/// used to plan zero stations for a demand below 3, turn the running station
/// off and report "fuel needed" despite the reserve.
#[test]
fn a_running_power_station_keeps_working_from_the_fuel_reserve() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let station = spawn(&mut state, UnitType::PowerStation, Point::new(8, 10));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(8, 12));

    state.unit_mut(fuel).unwrap().storage = 40;

    let complex = state.unit(mine).unwrap().complex.unwrap();
    assert_eq!(state.unit(station).unwrap().complex, Some(complex));

    // The station is running (`ORDER_POWER_ON`) and the mine demands 1 power.
    state.unit_mut(station).unwrap().orders = UnitOrder::PowerOn;
    state.unit_mut(station).unwrap().state = UnitOrderState::ExecutingOrder;
    state.unit_mut(mine).unwrap().orders = UnitOrder::PowerOn;
    state.unit_mut(mine).unwrap().state = UnitOrderState::ExecutingOrder;

    let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, true);

    assert_eq!(
        state.unit(station).unwrap().orders,
        UnitOrder::PowerOn,
            "the fuel reserve (40) covers the station rate (6): {outcome:?}"
    );
    assert_eq!(state.unit(mine).unwrap().orders, UnitOrder::PowerOn);
    assert!(outcome.powered_off.is_empty(), "{outcome:?}");
    assert_eq!(outcome.message, None, "{outcome:?}");
    assert!(!outcome.unresolved, "{outcome:?}");
    assert_eq!(
        outcome.totals.fuel, 34,
            "the storage reserve paid the station rate (40 - 6)"
    );
}

/// The same reserve turns a **switched-off** station on, as C++ does.
#[test]
fn a_switched_off_power_station_starts_from_the_fuel_reserve() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let station = spawn(&mut state, UnitType::PowerStation, Point::new(8, 10));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(8, 12));

    state.unit_mut(fuel).unwrap().storage = 40;
    state.unit_mut(station).unwrap().orders = UnitOrder::PowerOff;
    state.unit_mut(station).unwrap().state = UnitOrderState::ExecutingOrder;
    state.unit_mut(mine).unwrap().orders = UnitOrder::PowerOn;
    state.unit_mut(mine).unwrap().state = UnitOrderState::ExecutingOrder;

    let complex = state.unit(mine).unwrap().complex.unwrap();
    let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, false);

    assert_eq!(
        state.unit(station).unwrap().orders,
        UnitOrder::PowerOn,
            "the station is the only producer and the reserve pays for it: {outcome:?}"
    );
    assert!(outcome.powered_on.contains(&station), "{outcome:?}");
    assert_eq!(state.unit(mine).unwrap().orders, UnitOrder::PowerOn);
    assert!(!outcome.unresolved, "{outcome:?}");
}

/// Fuel is also available from the complex's **current production**: the
/// station rate is paid by the mine's fuel mining even without a storage
/// building (C++ `total.fuel` = inventory + `Cargo_GetNetProduction`).
#[test]
fn a_power_station_runs_on_the_current_fuel_production() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let station = spawn(&mut state, UnitType::PowerStation, Point::new(8, 10));

    // 8 fuel per turn from the mine: more than the station rate (6).
    state.unit_mut(mine).unwrap().fuel_mining = 8;
    state.unit_mut(mine).unwrap().orders = UnitOrder::PowerOn;
    state.unit_mut(mine).unwrap().state = UnitOrderState::ExecutingOrder;
    state.unit_mut(station).unwrap().orders = UnitOrder::PowerOff;
    state.unit_mut(station).unwrap().state = UnitOrderState::ExecutingOrder;

    let complex = state.unit(mine).unwrap().complex.unwrap();
    let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, false);

    assert_eq!(
        state.unit(station).unwrap().orders,
        UnitOrder::PowerOn,
            "the per-turn fuel production covers the station: {outcome:?}"
    );
    assert!(!outcome.unresolved, "{outcome:?}");
    assert_eq!(
        outcome.totals.fuel, 2,
            "8 fuel mined this turn minus the station rate 6"
    );
}

/// The selected unit gets the "other buildings need …" form when the optimizer
/// switches it **on** because the complex consumes its power: `_(cfb8)` with
/// `_(07d7)` (`ComposeIndustryMessage`, `production_manager.cpp:308-330`,
/// `UpdateUnitPowerConsumption` `:164-186`).
#[test]
fn a_selected_producer_turned_on_names_the_demand() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let station = spawn(&mut state, UnitType::PowerStation, Point::new(8, 10));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(8, 12));

    state.unit_mut(fuel).unwrap().storage = 40;
    state.unit_mut(station).unwrap().orders = UnitOrder::PowerOff;
    state.unit_mut(station).unwrap().state = UnitOrderState::ExecutingOrder;
    state.unit_mut(mine).unwrap().orders = UnitOrder::PowerOn;
    state.unit_mut(mine).unwrap().state = UnitOrderState::ExecutingOrder;

    let complex = state.unit(mine).unwrap().complex.unwrap();
    let outcome = optimize_complex(&mut state, TeamId::RED, complex, Some(station), None, true);

    assert!(outcome.powered_on.contains(&station), "{outcome:?}");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Cannot turn Power Station off, other buildings need power."),
            "{outcome:?}"
    );
}

/// The same for `HABITAT` on a worker shortage — `_(5fd6)` with `_(ed57)`.
#[test]
fn a_selected_habitat_turned_on_names_the_workers() {
    let mut state = state();
    let factory = spawn(&mut state, UnitType::TrainingHall, Point::new(12, 11));
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(12, 13));
    let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(14, 11));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(14, 12));
    let habitat = spawn(&mut state, UnitType::Habitat, Point::new(11, 12));

    state.unit_mut(dump).unwrap().storage = 20;
    state.unit_mut(fuel).unwrap().storage = 50;
    state.unit_mut(habitat).unwrap().orders = UnitOrder::PowerOff;
    state.unit_mut(habitat).unwrap().state = UnitOrderState::ExecutingOrder;
    {
        let unit = state.unit_mut(factory).unwrap();
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::Init;
        unit.build_time = 2;
        unit.build_rate = 1;
        unit.build_list = vec![UnitType::Infantry];
    }

    let complex = state.unit(factory).unwrap().complex.unwrap();
    let _ = generator;
    let outcome = optimize_complex(&mut state, TeamId::RED, complex, Some(habitat), None, true);

    assert!(outcome.powered_on.contains(&habitat), "{outcome:?}");
    assert!(
        outcome.message.as_deref().is_some_and(|message| message
            .contains("Cannot turn Habitat off, other buildings need workers.")),
            "{outcome:?}"
    );
}

/// The plan arithmetic of `OptimizePowerProduction` (`production_manager.cpp:481-549`),
/// including the second stage (`:519-540`).
#[test]
fn producer_plan_follows_optimize_power_production() {
    // No deficit: nothing has to run.
    assert_eq!(plan_power_production(0, 2, 2), (0, 0));

    // A station is the only producer. Demand 1 or 2 is below its 6 power and
    // the first-stage formula yields zero stations; the second stage covers
    // the residual with one station (demand 3 is the first-stage boundary).
    assert_eq!(plan_power_production(1, 1, 0), (1, 0));
    assert_eq!(plan_power_production(2, 1, 0), (1, 0));
    assert_eq!(plan_power_production(3, 1, 0), (1, 0));

    // A generator covers a small deficit exactly: `(demand + 3) / 6 = 0`,
    // then one generator per power unit.
    assert_eq!(plan_power_production(1, 1, 1), (0, 1));

    // The generator covers 1 of 2, the residual 1 brings the station in and
    // the station output covers the whole demand, so the generator goes out.
    assert_eq!(plan_power_production(2, 1, 1), (1, 0));

    // One station covers 6; a seventh unit of demand needs a second one.
    assert_eq!(plan_power_production(6, 2, 0), (1, 0));
    assert_eq!(plan_power_production(7, 2, 0), (2, 0));

    // Generators fill the deficit the single station leaves.
    assert_eq!(plan_power_production(8, 1, 3), (1, 2));

    // Nothing can cover it all: the plan uses every available producer.
    assert_eq!(plan_power_production(13, 2, 0), (2, 0));
}

#[test]
fn worker_shortage_turns_a_habitat_on() {
    let mut state = state();
    let factory = spawn(&mut state, UnitType::TrainingHall, Point::new(12, 11));
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(12, 13));
    let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(14, 11));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(14, 12));
    let habitat = spawn(&mut state, UnitType::Habitat, Point::new(11, 12));

    state.unit_mut(dump).unwrap().storage = 20;
    state.unit_mut(fuel).unwrap().storage = 50;
    // `DeployUnit` puts `HABITAT` into `POWER_ON`; the C++ optimizer turns it on
    // only when it was off (`UpdateUnitLifeConsumption`).
    state.unit_mut(habitat).unwrap().orders = UnitOrder::PowerOff;
    state.unit_mut(habitat).unwrap().state = UnitOrderState::ExecutingOrder;

    let complex = state.unit(factory).unwrap().complex.unwrap();
    for id in [dump, generator, fuel, habitat] {
        assert_eq!(state.unit(id).unwrap().complex, Some(complex));
    }

    // A factory under construction consumes 1 worker (`Cargo_GetLifeConsumptionRate`).
    {
        let unit = state.unit_mut(factory).unwrap();
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::Init;
        unit.build_time = 2;
        unit.build_rate = 1;
        unit.build_list = vec![UnitType::Infantry];
    }

    let totals_before = complex_totals(&state, TeamId::RED, complex);
    assert_eq!(totals_before.life, -1, "TRAINHAL needs one worker");

    let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, false);

    assert_eq!(state.unit(habitat).unwrap().orders, UnitOrder::PowerOn);
    assert!(outcome.paused.is_empty(), "workers supplied: {outcome:?}");
    assert!(
        outcome.powered_on.contains(&habitat),
            "HABITAT turned on: {outcome:?}"
    );
    assert!(outcome.totals.life >= 0);
}
