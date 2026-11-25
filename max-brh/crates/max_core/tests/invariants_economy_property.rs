//! Property tests for the economic simulation invariants E1-E7.
//!
//! The catalog (`max-brh-docs/development/simulation-invariants.md`) defines each
//! property as an unconditional oracle guarantee; this suite generates orders,
//! states, reserves and mining splits (including the 0 and 16 boundaries) and
//! checks the guarantee, instead of replaying one fixed scenario.
//!
//! Oracles are in `max-cpp-port/src` and are cited next to each test.
//! These tests only observe production code: a failing case means the invariant
//! is violated, not that the catalog is wrong.

mod support;

use max_core::alloc::MiningKind;
use max_core::enums::{Opponent, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::map::{ResourceKind, ResourceMapEntry};
use max_core::team::{TeamId, TeamKind};
use max_core::{
    cargo_fuel_consumption_rate, cargo_gold_consumption_rate, cargo_life_consumption_rate,
    cargo_power_consumption_rate, cargo_raw_consumption_rate,
};
use support::invariants::{complex_of, complex_stock, set_order_pair, Lcg};
use support::{game, spawn};

const FACTORIES: [UnitType; 5] = [
    UnitType::Shipyard,
    UnitType::LightVehiclePlant,
    UnitType::HeavyVehiclePlant,
    UnitType::TrainingHall,
    UnitType::AirUnitsPlant,
];

const POWER_BUILDINGS: [UnitType; 6] = [
    UnitType::GoldRefinery,
    UnitType::PowerStation,
    UnitType::PowerGenerator,
    UnitType::Habitat,
    UnitType::ResearchCentre,
    UnitType::EcoSphere,
];

const FACTORY_ORDERS: [UnitOrder; 6] = [
    UnitOrder::Build,
    UnitOrder::PowerOff,
    UnitOrder::HaltBuilding,
    UnitOrder::Disable,
    UnitOrder::Idle,
    UnitOrder::PowerOn,
];

const FULL_ORDERS: [UnitOrder; 6] = [
    UnitOrder::PowerOn,
    UnitOrder::PowerOff,
    UnitOrder::Disable,
    UnitOrder::Idle,
    UnitOrder::Build,
    UnitOrder::NewAllocate,
];

const BUILD_STATES: [UnitOrderState; 3] = [
    UnitOrderState::Init,
    UnitOrderState::ExecutingOrder,
    UnitOrderState::UnitReady,
];

/// Invariant E1: a factory spends `raw` only for `ORDER_BUILD` outside
/// `ORDER_STATE_UNIT_READY`; every other order/ready state spends 0.
/// Oracle: `max-cpp-port/src/cargo.cpp:88-95` (`Cargo_GetNetProduction`, factory case).
#[test]
fn e1_factory_raw_consumption_requires_build_and_not_unit_ready() {
    let mut state = game();
    let mut case = 0i32;

    for unit_type in FACTORIES {
        for order in FACTORY_ORDERS {
            for order_state in BUILD_STATES {
                let build_rate = 1 + (case % 4) as u16;
                let grid = Point::new((case % 8) as i16 * 3 + 2, (case / 8) as i16 * 3 + 2);
                let id = spawn(&mut state, unit_type, TeamId::RED, grid);
                case += 1;

                {
                    let unit = state.unit_mut(id).expect("factory");
                    unit.orders = order;
                    unit.state = order_state;
                    unit.build_rate = build_rate;
                    unit.build_paused = false;
                }

                let unit = state.snapshot().unit(id).expect("factory").clone();
                let net = state.snapshot().cargo_get_net_production(&unit, true);

                let consumes =
                    order == UnitOrder::Build && order_state != UnitOrderState::UnitReady;
                let expected = if consumes {
                    -cargo_raw_consumption_rate(unit_type, i32::from(build_rate))
                } else {
                    0
                };

                assert_eq!(
                    net.raw, expected,
                    "E1: factory {unit_type:?} in order {order:?}/{order_state:?} \
                     (rate {build_rate}) consumed {} raw, expected {expected}",
                    -net.raw
                );
            }
        }
    }
}

/// Invariant E2: `COMMTWR/POWERSTN/POWGEN/HABITAT/RESEARCH/GREENHSE` consume
/// fuel/power/life/gold only under `ORDER_POWER_ON`; any other order yields a
/// zero net-production contribution.
/// Oracle: `max-cpp-port/src/cargo.cpp:99-105` (`Cargo_GetNetProduction`,
/// building case) and `cargo.cpp:183-271` (the per-type rates).
#[test]
fn e2_power_buildings_consume_only_when_powered_on() {
    let mut state = game();
    let mut case = 0i16;

    for unit_type in POWER_BUILDINGS {
        for order in FULL_ORDERS {
            let grid = Point::new((case % 8) * 3 + 2, (case / 8) * 3 + 30);
            let id = spawn(&mut state, unit_type, TeamId::RED, grid);
            case += 1;
            set_order_pair(&mut state, id, order, UnitOrderState::ExecutingOrder);

            let unit = state.snapshot().unit(id).expect("building").clone();
            let net = state.snapshot().cargo_get_net_production(&unit, true);

            if order == UnitOrder::PowerOn {
                assert_eq!(
                    net.raw,
                    -cargo_raw_consumption_rate(unit_type, 1),
                    "E2: {unit_type:?} powered on must spend raw at the oracle rate"
                );
                assert_eq!(
                    net.fuel,
                    -cargo_fuel_consumption_rate(unit_type),
                    "E2: {unit_type:?} powered on must spend fuel at the oracle rate"
                );
                assert_eq!(
                    net.power,
                    -cargo_power_consumption_rate(unit_type),
                    "E2: {unit_type:?} powered on must apply the power rate"
                );
                assert_eq!(
                    net.life,
                    -cargo_life_consumption_rate(unit_type),
                    "E2: {unit_type:?} powered on must apply the life rate"
                );
                assert_eq!(
                    net.gold,
                    -cargo_gold_consumption_rate(unit_type),
                    "E2: {unit_type:?} powered on must spend gold at the oracle rate"
                );
            } else {
                assert_eq!(
                    (net.raw, net.fuel, net.power, net.life, net.gold),
                    (0, 0, 0, 0, 0),
                    "E2: {unit_type:?} in order {order:?} must consume nothing"
                );
            }
        }
    }
}

/// Invariant E3: `MININGST` yields `(rate * difficulty) / 4` only for
/// `ORDER_POWER_ON`/`ORDER_NEW_ALLOCATE`, with difficulty 4 for a human team and
/// 5/6 for a computer on Master/God.
/// Oracle: `max-cpp-port/src/cargo.cpp:106-124` (`Cargo_GetNetProduction`,
/// `MININGST` case).
#[test]
fn e3_mining_output_follows_order_and_difficulty() {
    let cases: [(TeamKind, Option<Opponent>, i32); 3] = [
        (TeamKind::Player, None, 4),
        (TeamKind::Computer, Some(Opponent::Master), 5),
        (TeamKind::Computer, Some(Opponent::God), 6),
    ];
    let orders = [
        UnitOrder::PowerOn,
        UnitOrder::NewAllocate,
        UnitOrder::PowerOff,
        UnitOrder::Idle,
        UnitOrder::Disable,
    ];

    for (case, (team_kind, opponent, difficulty)) in cases.iter().enumerate() {
        for round in 0..8 {
            let mut state = game();
            {
                let team = state.team_mut(TeamId::RED).expect("RED slot");
                team.team_type = *team_kind;
            }
            if let Some(opponent) = opponent {
                state.ai_mut().settings.opponent = *opponent as u8;
            }

            let mine = spawn(
                &mut state,
                UnitType::MiningStation,
                TeamId::RED,
                Point::new(30 + round as i16, 30 + case as i16),
            );
            let order = orders[(round + case) % orders.len()];
            let raw = (round * 2) as u8;
            let fuel = (round + 1) as u8;
            let gold = (round % 4) as u8;
            {
                let unit = state.unit_mut(mine).expect("mine");
                unit.orders = order;
                unit.state = UnitOrderState::ExecutingOrder;
                unit.raw_mining = raw;
                unit.fuel_mining = fuel;
                unit.gold_mining = gold;
                unit.total_mining = raw.wrapping_add(fuel).wrapping_add(gold);
            }

            let unit = state.snapshot().unit(mine).expect("mine").clone();
            let net = state.snapshot().cargo_get_net_production(&unit, true);
            let active = matches!(order, UnitOrder::PowerOn | UnitOrder::NewAllocate);

            let expected = |rate: u8| {
                if active {
                    (i32::from(rate) * difficulty) / 4
                } else {
                    0
                }
            };

            assert_eq!(
                net.raw,
                expected(raw),
                "E3: mining output {} != rate×difficulty/4 {} (difficulty {difficulty}, \
                 order {order:?})",
                net.raw,
                expected(raw)
            );
            assert_eq!(
                net.fuel,
                expected(fuel),
                "E3: fuel output {} != rate×difficulty/4 {} (difficulty {difficulty})",
                net.fuel,
                expected(fuel)
            );
            assert_eq!(
                net.gold,
                expected(gold),
                "E3: gold output {} != rate×difficulty/4 {} (difficulty {difficulty})",
                net.gold,
                expected(gold)
            );
        }
    }
}

/// Invariant E4: the free capacity of one mine is exactly `16 - total_mining`;
/// the total never exceeds 16, including the saturated boundary 16.
/// Oracle: `max-cpp-port/src/cargo.cpp:122` (`cargo.free_capacity += 16 - total_mining`)
/// and `max-cpp-port/src/allocmenu.cpp:485-500` (the 16-unit allocation cap).
#[test]
fn e4_free_capacity_is_sixteen_minus_total_mining() {
    for total in 0..=16u8 {
        let mut state = game();
        let mine = spawn(
            &mut state,
            UnitType::MiningStation,
            TeamId::RED,
            Point::new(20 + (total % 3) as i16, 20 + (total / 3) as i16),
        );
        {
            let unit = state.unit_mut(mine).expect("mine");
            unit.orders = UnitOrder::PowerOn;
            unit.state = UnitOrderState::ExecutingOrder;
            unit.raw_mining = total;
            unit.fuel_mining = 0;
            unit.gold_mining = 0;
            unit.total_mining = total;
        }

        let unit = state.snapshot().unit(mine).expect("mine").clone();
        let net = state.snapshot().cargo_get_net_production(&unit, true);

        assert_eq!(
            net.free_capacity,
            16 - i32::from(total),
            "E4: free_capacity {} != 16 - total_mining {total}",
            net.free_capacity
        );
        assert!(
            i32::from(unit.total_mining) <= 16,
            "E4: mine total_mining {} exceeds cap 16",
            unit.total_mining
        );
    }
}

/// Invariant E4 (cap enforcement): an allocation request larger than the mine's
/// free capacity saturates at 16 and never overflows it.
/// Oracle: `max-cpp-port/src/allocmenu.cpp:485-500` (`AllocMenu_Optimize`).
#[test]
fn e4_allocation_saturates_at_sixteen() {
    let mut state = game();
    for (x, y) in [(20, 20), (21, 20), (20, 21), (21, 21)] {
        state
            .map_mut()
            .resources
            .set(x, y, ResourceMapEntry::new(8, Some(ResourceKind::Raw), 0));
    }

    let mine = spawn(
        &mut state,
        UnitType::MiningStation,
        TeamId::RED,
        Point::new(20, 20),
    );
    let complex = complex_of(&state, mine);
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.raw_mining = 0;
        unit.fuel_mining = 0;
        unit.gold_mining = 0;
        unit.total_mining = 0;
    }

    let added = state.optimize_allocation(TeamId::RED, complex, MiningKind::Raw, 99);
    let unit = state.snapshot().unit(mine).expect("mine").clone();

    assert_eq!(
        unit.total_mining, 16,
        "E4: allocation must stop exactly at the 16-unit cap"
    );
    assert_eq!(added, 16, "E4: only 16 units can be added to an empty mine");
    assert!(
        i32::from(unit.raw_mining) <= 16,
        "E4: raw mining {} exceeds cap 16",
        unit.raw_mining
    );
}

/// Invariant E5: complex stocks aggregate `inventory + net_production` strictly
/// over the members of that complex; a building of another complex contributes
/// nothing.
/// Oracle: `max-cpp-port/src/access.cpp:912-934` (`Access_UpdateResourcesTotal`).
#[test]
fn e5_complex_balance_is_isolated_by_complex_membership() {
    let mut rng = Lcg::new(0xE5_0005);

    for case in 0..16 {
        let mut state = game();
        // Explicit zero-balance boundary plus generated non-zero stocks.
        let stock_value_a = if case == 0 { 0 } else { rng.range(0, 50) };
        let stock_value_b = if case <= 1 { 0 } else { rng.range(0, 50) };

        let a = spawn(
            &mut state,
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(10, 10),
        );
        let b = spawn(
            &mut state,
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(40, 40),
        );
        state.unit_mut(a).expect("storage A").storage = stock_value_a as i16;
        state.unit_mut(b).expect("storage B").storage = stock_value_b as i16;

        // A power generator belongs to complex A only: its net production
        // (fuel -2, power +1) must not leak into complex B.
        let generator = spawn(
            &mut state,
            UnitType::PowerGenerator,
            TeamId::RED,
            Point::new(10, 11),
        );
        set_order_pair(
            &mut state,
            generator,
            UnitOrder::PowerOn,
            UnitOrderState::ExecutingOrder,
        );

        let complex_a = complex_of(&state, a);
        let complex_b = complex_of(&state, b);
        assert_ne!(
            complex_a, complex_b,
            "E5: the two isolated storages must belong to different complexes"
        );
        assert_eq!(
            complex_of(&state, generator),
            complex_a,
            "E5: the generator must join complex A"
        );

        state.update_resources_total(TeamId::RED, complex_a);
        state.update_resources_total(TeamId::RED, complex_b);

        let complex_a_stock = complex_stock(&state, TeamId::RED, complex_a);
        assert_eq!(
            complex_a_stock.raw, stock_value_a as i16,
            "E5: complex A raw {} != its own storage {}",
            complex_a_stock.raw, stock_value_a
        );
        assert_eq!(
            complex_a_stock.fuel, -2,
            "E5: complex A must include the generator fuel net production"
        );
        assert_eq!(
            complex_a_stock.power, 1,
            "E5: complex A must include the generator power net production"
        );

        let complex_b_stock = complex_stock(&state, TeamId::RED, complex_b);
        assert_eq!(
            complex_b_stock.raw, stock_value_b as i16,
            "E5: complex B raw {} != its own storage {}",
            complex_b_stock.raw, stock_value_b
        );
        assert_eq!(
            complex_b_stock.fuel, 0,
            "E5: complex A's generator must not pollute complex B fuel (case {case})"
        );
        assert_eq!(
            complex_b_stock.power, 0,
            "E5: complex A's generator must not pollute complex B power (case {case})"
        );
    }
}

/// Invariant E6: a `POWGEN`/`POWERSTN` is not powered on while the fuel reserve
/// is below its consumption rate (2/6); the exact reserve is the boundary where
/// it may run.
/// Oracle: `max-cpp-port/src/production_manager.cpp:332-352`
/// (`ProductionManager::PowerOn`) and `cargo.cpp:183-197` (the fuel rates).
#[test]
fn e6_power_plant_requires_its_fuel_reserve() {
    let cases = [(UnitType::PowerGenerator, 2), (UnitType::PowerStation, 6)];

    for (plant_type, rate) in cases {
        for reserve in 0..=rate + 2 {
            let mut state = game();
            let plant = spawn(&mut state, plant_type, TeamId::RED, Point::new(10, 10));
            // `PowerGenerator` is a 1×1 building, `PowerStation` is 2×2
            // (`Footprint::classify`); the helper points below put every member
            // on a cell adjacent to the plant.
            let (fuel_grid, consumer_grid) = if plant_type == UnitType::PowerStation {
                (Point::new(12, 10), Point::new(10, 12))
            } else {
                (Point::new(10, 11), Point::new(11, 10))
            };
            let fuel = spawn(&mut state, UnitType::FuelStorage, TeamId::RED, fuel_grid);
            state.unit_mut(fuel).expect("fuel dump").storage = reserve as i16;

            let consumer = spawn(
                &mut state,
                UnitType::ResearchCentre,
                TeamId::RED,
                consumer_grid,
            );
            set_order_pair(
                &mut state,
                consumer,
                UnitOrder::PowerOn,
                UnitOrderState::ExecutingOrder,
            );
            set_order_pair(
                &mut state,
                plant,
                UnitOrder::PowerOff,
                UnitOrderState::ExecutingOrder,
            );

            let complex = complex_of(&state, plant);
            assert_eq!(
                complex_of(&state, fuel),
                complex,
                "E6: the fuel dump must be in the plant's complex"
            );
            assert_eq!(
                complex_of(&state, consumer),
                complex,
                "E6: the power consumer must be in the plant's complex"
            );

            state.optimize_complex(TeamId::RED, complex, None, None, false);

            let orders = state.snapshot().unit(plant).expect("plant").orders;
            let expected_on = reserve >= rate;
            assert_eq!(
                orders == UnitOrder::PowerOn,
                expected_on,
                "E6: {plant_type:?} with fuel reserve {reserve} (rate {rate}) has order \
                 {orders:?}; expected powered on = {expected_on}"
            );
        }
    }
}

/// Invariant E7: after `ProductionManager_OptimizeProduction` a complex balance
/// is non-negative (or the optimizer reports the unresolved deficit); the exact
/// zero balance and the exact-cost boundary are both covered.
/// Oracle: `max-cpp-port/src/production_manager.cpp:830-901`
/// (`ProductionManager_OptimizeProduction`, the main deficit loop and early exit).
#[test]
fn e7_optimization_leaves_no_negative_balance() {
    for raw_stock in 0..=4 {
        let mut state = game();
        let factory = spawn(
            &mut state,
            UnitType::TrainingHall,
            TeamId::RED,
            Point::new(10, 10),
        );
        let storage = spawn(
            &mut state,
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(12, 10),
        );
        state.unit_mut(storage).expect("storage").storage = raw_stock as i16;

        let generator = spawn(
            &mut state,
            UnitType::PowerGenerator,
            TeamId::RED,
            Point::new(10, 12),
        );
        let fuel = spawn(
            &mut state,
            UnitType::FuelStorage,
            TeamId::RED,
            Point::new(11, 12),
        );
        state.unit_mut(fuel).expect("fuel dump").storage = 50;
        let habitat = spawn(
            &mut state,
            UnitType::Habitat,
            TeamId::RED,
            Point::new(13, 10),
        );

        {
            let unit = state.unit_mut(factory).expect("factory");
            unit.orders = UnitOrder::Build;
            unit.state = UnitOrderState::ExecutingOrder;
            unit.prior_orders = UnitOrder::Build;
            unit.prior_state = UnitOrderState::ExecutingOrder;
            unit.build_rate = 1;
            unit.build_paused = false;
        }

        let complex = complex_of(&state, factory);
        for id in [storage, generator, fuel, habitat] {
            assert_eq!(
                complex_of(&state, id),
                complex,
                "E7: the support buildings must share the factory's complex"
            );
        }

        let outcome = state.optimize_complex(TeamId::RED, complex, None, None, false);
        let totals = outcome.totals;

        assert!(
            totals.raw >= 0
                && totals.fuel >= 0
                && totals.gold >= 0
                && totals.power >= 0
                && totals.life >= 0,
            "E7: complex deficit after optimization with raw stock {raw_stock}: {totals:?}"
        );
        assert_eq!(
            outcome.unresolved,
            totals.has_deficit(),
            "E7: the unresolved flag must match the final balance ({totals:?})"
        );

        let factory_state = state.snapshot().unit(factory).expect("factory");
        if raw_stock == 0 {
            assert_eq!(
                totals.raw, 0,
                "E7: a zero balance is the valid boundary (no negative raw)"
            );
            assert!(
                factory_state.build_paused,
                "E7: the factory with no materials must pause instead of going negative"
            );
        } else {
            assert_eq!(
                totals.raw,
                raw_stock - 1,
                "E7: the factory must pay exactly one raw when the stock covers it"
            );
            assert!(
                !factory_state.build_paused,
                "E7: a covered construction must not pause"
            );
        }
    }
}
