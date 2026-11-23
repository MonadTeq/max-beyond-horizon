//! Phase 3 (economy): mining allocation — raw/fuel/gold shares, the 16-unit cap,
//! conversion priorities and the per-turn income.
//!
//! Oracle (`max-cpp-port/src`):
//! * `allocmenu.cpp:413-582` — `Allocator::Optimize`, `AllocMenu_Optimize`,
//!   `AllocMenu_ReduceProduction`, `AllocMenu::Balance` (limit `16 - total_mining`);
//! * `allocmenu.cpp:457` — `Survey_GetResourcesInArea(..., 1, 16, ...)`;
//! * `complex.cpp:109-133` — `Complex::GetCargoMining`/`GetCargoMinable`;
//! * `cargo.cpp:105-126` — `Cargo_GetNetProduction(MININGST)` (raw/fuel/gold per turn);
//! * `access.cpp:912` — `Access_UpdateResourcesTotal`;
//! * `game_manager.cpp:2058` — `GameManager_ManageEconomy`.
//!
//! The mine stands on a 2×2 surveyed square: raw 16 (clamped from 24), fuel 16,
//! gold 16 — every resource reaches the cell maximum.

use max_core::alloc::{
    get_cargo_info, get_cargo_mining, optimize, reduce_production, total_mining, MiningKind,
    MINING_CAPACITY, MINING_SURVEY_LIMIT,
};
use max_core::crc;
use max_core::enums::{TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::map::{ResourceKind, ResourceMapEntry};
use max_core::team::{ComplexId, TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

fn state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(state: &mut GameUniverse, unit_type: UnitType, grid: Point) -> UnitId {
    state
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .expect("spawn")
}

fn set_resource(state: &mut GameUniverse, x: i16, y: i16, amount: u8, kind: ResourceKind) {
    state
        .state_mut()
        .map
        .resources
        .set(x, y, ResourceMapEntry::new(amount, Some(kind), 0));
}

/// A powered complex around one mine:
///
/// ```text
///   MINE(20,20) -- ADUMP(22,20) -- FDUMP(23,20) -- GOLDSM(24,20)
///                                                     |
///                                                 POWGEN(24,21)
/// ```
fn mining_state() -> (GameUniverse, UnitId, ComplexId) {
    let mut state = state();
    // The surveyed square `[grid, grid + 1]` of the mine.
    set_resource(&mut state, 20, 20, 16, ResourceKind::Raw);
    set_resource(&mut state, 21, 20, 16, ResourceKind::Fuel);
    set_resource(&mut state, 20, 21, 16, ResourceKind::Gold);
    set_resource(&mut state, 21, 21, 8, ResourceKind::Raw);

    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(20, 20));
    let raw = spawn(&mut state, UnitType::MaterialStorage, Point::new(22, 20));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(23, 20));
    let gold = spawn(&mut state, UnitType::GoldVault, Point::new(24, 20));
    let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(24, 21));

    // Fuel for the generator (and the mined fuel) — 20 units.
    state.unit_mut(fuel).expect("FDUMP").storage = 20;
    let _ = state.set_order(
        generator,
        UnitOrder::PowerOn,
        UnitOrderState::ExecutingOrder,
    );

    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.raw_mining = 0;
        unit.fuel_mining = 0;
        unit.gold_mining = 0;
        unit.total_mining = 0;
        unit.storage = 0;
    }

    let complex = state
        .snapshot()
        .unit(mine)
        .expect("mine")
        .complex
        .expect("complex");
    for id in [raw, fuel, gold, generator] {
        assert_eq!(
            state.snapshot().unit(id).expect("member").complex,
            Some(complex),
            "all members share one complex"
        );
    }

    (state, mine, complex)
}

fn stored(state: &GameUniverse, complex: ComplexId) -> (i32, i32, i32) {
    let (materials, _) = get_cargo_info(state.state(), TeamId::RED, complex);
    (materials.raw, materials.fuel, materials.gold)
}

// ---------------------------------------------------------------------------
// I — shares, the 16-unit cap and the economy effect
// ---------------------------------------------------------------------------

#[test]
fn survey_limits_each_resource_to_the_allocmenu_maximum() {
    let (state, mine, complex) = mining_state();
    let unit = state.snapshot().unit(mine).expect("mine");
    let (raw, gold, fuel) = max_core::alloc::mining_survey(state.state(), unit.grid);

    assert_eq!(MINING_SURVEY_LIMIT, 16);
    assert_eq!(raw, 16, "16 + 8 = 24 clamped to 16 (allocmenu.cpp:457)");
    assert_eq!(gold, 16);
    assert_eq!(fuel, 16);

    let (_, capacity) = get_cargo_mining(state.state(), TeamId::RED, complex);
    assert_eq!(capacity.raw, 16);
    assert_eq!(capacity.fuel, 16);
    assert_eq!(capacity.gold, 16);
}

#[test]
fn optimize_stops_at_the_sixteen_unit_cap_across_resources() {
    let (mut state, mine, complex) = mining_state();

    // 20 requested, only 16 units exist on the mine.
    assert_eq!(
        optimize(state.state_mut(), TeamId::RED, complex, MiningKind::Raw, 20),
        MINING_CAPACITY
    );
    let unit = state.snapshot().unit(mine).expect("mine");
    assert_eq!(unit.raw_mining, 16);
    assert_eq!(unit.total_mining, 16);
    assert_eq!(
        optimize(state.state_mut(), TeamId::RED, complex, MiningKind::Fuel, 4),
        0,
        "no free mining units left"
    );
    assert_eq!(
        optimize(state.state_mut(), TeamId::RED, complex, MiningKind::Gold, 4),
        0
    );
    // The cell still holds fuel/gold, but the mine is saturated.
    assert_eq!(
        total_mining(state.state(), TeamId::RED, complex, MiningKind::Fuel),
        0
    );
    assert_eq!(
        total_mining(state.state(), TeamId::RED, complex, MiningKind::Gold),
        0
    );

    // `AllocMenu_ReduceProduction` frees the units again.
    reduce_production(state.state_mut(), TeamId::RED, complex, MiningKind::Raw, 16);
    assert_eq!(
        optimize(state.state_mut(), TeamId::RED, complex, MiningKind::Gold, 6),
        6
    );
    assert_eq!(
        state.snapshot().unit(mine).expect("mine").gold_mining,
        6,
        "freed units are reassignable"
    );
}

#[test]
fn raw_fuel_gold_shares_become_the_complex_income_after_a_turn() {
    let (mut state, mine, complex) = mining_state();
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.raw_mining = 5;
        unit.fuel_mining = 4;
        unit.gold_mining = 3;
        unit.total_mining = 12;
    }

    // `ManageEconomy`: `Access_UpdateResourcesTotal` → the stock cache →
    // `UpdateProduction` distributes the cache into the member storages.
    // The POWGEN burns 2 fuel per turn (`Cargo_GetFuelConsumptionRate`).
    state.manage_economy(TeamId::RED);

    assert_eq!(stored(&state, complex), (5, 22, 3), "20 − 2 + 4 fuel");
    assert_eq!(
        state.snapshot().unit(mine).expect("mine").total_mining,
        12,
        "allocation is preserved by the turn"
    );

    // Four turns: the shares accumulate at their per-turn rate.
    for _ in 0..3 {
        state.manage_economy(TeamId::RED);
    }
    assert_eq!(stored(&state, complex), (20, 28, 12));
}

#[test]
fn changing_the_share_changes_the_next_turn_income_only() {
    let (mut state, mine, complex) = mining_state();
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.raw_mining = 5;
        unit.fuel_mining = 4;
        unit.gold_mining = 3;
        unit.total_mining = 12;
    }
    state.manage_economy(TeamId::RED);
    assert_eq!(stored(&state, complex), (5, 22, 3));

    // Move every mining unit to gold: raw/fuel mining stops, gold grows by 12.
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.raw_mining = 0;
        unit.fuel_mining = 0;
        unit.gold_mining = 12;
        unit.total_mining = 12;
    }
    state.manage_economy(TeamId::RED);

    assert_eq!(
        stored(&state, complex),
        (5, 20, 15),
        "gold 3 + 12; fuel 22 − 2 (generator)"
    );
}

#[test]
fn mining_income_obeys_the_complex_storage_capacity() {
    let (mut state, mine, complex) = mining_state();
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.raw_mining = 16;
        unit.total_mining = 16;
    }

    // Raw capacity = mine 25 + ADUMP 50 = 75; 5 turns × 16 = 80 → 5 units are lost
    // at the next `Access_UpdateResourcesTotal`, exactly like C++ (the complex
    // field is a one-turn cache).
    for _ in 0..6 {
        state.manage_economy(TeamId::RED);
    }

    let (raw, _, _) = stored(&state, complex);
    assert_eq!(raw, 75, "capacity limit");
    let (_, capacity) = get_cargo_info(state.state(), TeamId::RED, complex);
    assert_eq!(capacity.raw, 75);
}

// ---------------------------------------------------------------------------
// D — determinism
// ---------------------------------------------------------------------------

fn run_economy() -> GameUniverse {
    let (mut state, mine, _complex) = mining_state();
    {
        let unit = state.unit_mut(mine).expect("mine");
        unit.raw_mining = 5;
        unit.fuel_mining = 4;
        unit.gold_mining = 3;
        unit.total_mining = 12;
    }
    for _ in 0..4 {
        state.manage_economy(TeamId::RED);
    }
    state
}

#[test]
fn allocation_digests_are_reproducible() {
    let first = run_economy();
    let second = run_economy();

    assert_eq!(
        crc::state_checksum(first.state()),
        crc::state_checksum(second.state())
    );
    assert_eq!(
        crc::state_digest(first.state()),
        crc::state_digest(second.state())
    );
    assert_eq!(
        crc::replay_digest_bytes(first.state()),
        crc::replay_digest_bytes(second.state()),
        "the whole replay digest (including the mining fields) is stable"
    );

    // The mining allocation is part of the canonical snapshot and of the replay
    // digest (`raw_mining`/`fuel_mining`/`gold_mining`). The packet-45 CRC
    // (`state_checksum`) only hashes the unit lists and does not include it —
    // this is C++ behavior, not a port regression.
    let mut changed = first.clone();
    {
        let mine = changed
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                changed
                    .snapshot()
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::MiningStation)
            })
            .expect("mine");
        let unit = changed.unit_mut(mine).expect("mine");
        unit.raw_mining = 15;
        unit.total_mining = 16;
    }
    assert_ne!(
        crc::state_digest(changed.state()),
        crc::state_digest(first.state())
    );
    assert_ne!(
        crc::replay_digest_crc(changed.state()),
        crc::replay_digest_crc(first.state())
    );
}
