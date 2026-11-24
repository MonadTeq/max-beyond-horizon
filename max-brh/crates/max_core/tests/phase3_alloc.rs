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
    MiningKind,
    MINING_CAPACITY, MINING_SURVEY_LIMIT,
};
use max_core::enums::{TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::map::{ResourceKind, ResourceMapEntry};
use max_core::team::{ComplexId, TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

fn state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(state: &mut GameUniverse, unit_type: UnitType, grid: Point) -> UnitId {
    state
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .expect("spawn")
}

fn set_resource(state: &mut GameUniverse, x: i16, y: i16, amount: u8, kind: ResourceKind) {
    state
        .map_mut()
        
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
    let (materials, _) = state.snapshot().get_cargo_info(TeamId::RED, complex);
    (materials.raw, materials.fuel, materials.gold)
}

// ---------------------------------------------------------------------------
// I — shares, the 16-unit cap and the economy effect
// ---------------------------------------------------------------------------

#[test]
fn survey_limits_each_resource_to_the_allocmenu_maximum() {
    let (state, mine, complex) = mining_state();
    let unit = state.snapshot().unit(mine).expect("mine");
    let (raw, gold, fuel) = state.snapshot().mining_survey(unit.grid);

    assert_eq!(MINING_SURVEY_LIMIT, 16);
    assert_eq!(raw, 16, "16 + 8 = 24 clamped to 16 (allocmenu.cpp:457)");
    assert_eq!(gold, 16);
    assert_eq!(fuel, 16);

    let (_, capacity) = state.snapshot().get_cargo_mining(TeamId::RED, complex);
    assert_eq!(capacity.raw, 16);
    assert_eq!(capacity.fuel, 16);
    assert_eq!(capacity.gold, 16);
}

#[test]
fn optimize_stops_at_the_sixteen_unit_cap_across_resources() {
    let (mut state, mine, complex) = mining_state();

    // 20 requested, only 16 units exist on the mine.
    assert_eq!(
        state.optimize_allocation(TeamId::RED, complex, MiningKind::Raw, 20),
        MINING_CAPACITY
    );
    let unit = state.snapshot().unit(mine).expect("mine");
    assert_eq!(unit.raw_mining, 16);
    assert_eq!(unit.total_mining, 16);
    assert_eq!(
        state.optimize_allocation(TeamId::RED, complex, MiningKind::Fuel, 4),
        0,
        "no free mining units left"
    );
    assert_eq!(
        state.optimize_allocation(TeamId::RED, complex, MiningKind::Gold, 4),
        0
    );
    // The cell still holds fuel/gold, but the mine is saturated.
    assert_eq!(
        state.snapshot().total_mining(TeamId::RED, complex, MiningKind::Fuel),
        0
    );
    assert_eq!(
        state.snapshot().total_mining(TeamId::RED, complex, MiningKind::Gold),
        0
    );

    // `AllocMenu_ReduceProduction` frees the units again.
    state.reduce_production_allocation(TeamId::RED, complex, MiningKind::Raw, 16);
    assert_eq!(
        state.optimize_allocation(TeamId::RED, complex, MiningKind::Gold, 6),
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
    let (_, capacity) = state.snapshot().get_cargo_info(TeamId::RED, complex);
    assert_eq!(capacity.raw, 75);
}

/// Defect 1 of the manual run 1.2: the storage capacity of the connected mines
/// is part of the complex's total capacity.
///
/// `Complex::GetCargoInfo` (`complex.cpp:136-147`) sums `Cargo_GetCargoCapacity`
/// over **every** complex member, and `Cargo_GetCargoCapacity`
/// (`cargo.cpp:50-68`) is generic over the cargo type: a `MININGST` contributes
/// its `ATTRIB_STORAGE` (25 raw) exactly like an `ADUMP` contributes 50. The port
/// previously narrowed the aggregation to the storage buildings, so the mine's
/// buffer was missing from the reserve and a full mine silently lost its output.
#[test]
fn connected_mine_storage_is_part_of_the_complex_capacity() {
    // A lone mine: the complex reserve is the mine buffer alone.
    let mut state = state();
    set_resource(&mut state, 20, 20, 16, ResourceKind::Raw);
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(20, 20));
    let complex = state
        .snapshot()
        .unit(mine)
        .expect("mine")
        .complex
        .expect("complex");

    let unit = state.snapshot().unit(mine).expect("mine").clone();
    let mine_storage = max_core::alloc::cargo_get_cargo_capacity(&unit);
    assert_eq!(mine_storage.raw, 25, "MININGST ATTRIB_STORAGE");
    assert_eq!(mine_storage.fuel, 0, "the mine carries raw only");

    let (_, capacity) = state.snapshot().get_cargo_info(TeamId::RED, complex);
    assert_eq!(
        capacity.raw, 25,
        "the mine's own buffer is part of the complex raw reserve"
    );

    // A connected ADUMP adds its own 50 on top of the mine's 25.
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(22, 20));
    assert_eq!(
        state.snapshot().unit(dump).expect("dump").complex,
        Some(complex)
    );
    assert_eq!(
        state.snapshot().get_cargo_info(TeamId::RED, complex).1.raw,
        75,
        "connected mine 25 + ADUMP 50 (C++ Cargo_GetCargoCapacity is generic)"
    );
}

/// Defect 2 of the manual run 1.2: `AllocMenu_Optimize` balances the extraction
/// over **the whole complex**, not over one mine.
///
/// The C++ allocator walks `UnitsManager_StationaryUnits` and reallocates every
/// `MININGST` whose `GetComplex()` matches (`allocmenu.cpp:452-514`); the dialog
/// opened from a mine has no per-mine scope at all, and `AllocMenu::Balance`
/// (`:567-582`) reports the sum over the complex. The port previously restricted
/// the entry points to the dialog's mine, so a second mine of the same complex
/// could not take the request (and `Balance` showed only one mine's maximum).
#[test]
fn allocation_and_balance_span_the_whole_complex() {
    let (mut state, first, complex) = mining_state();

    // A second mine of the same complex, with 8 raw in its surveyed square. It
    // stands south of the POWGEN, so `UpdateConnectors` joins it to the complex
    // on spawn; the stationary list is ordered by `y` (`UnitsManager_*Units`),
    // so the first mine stays first in the allocation walk.
    for x in [24, 25] {
        for y in [22, 23] {
            set_resource(&mut state, x, y, 2, ResourceKind::Raw);
        }
    }
    let second = spawn(&mut state, UnitType::MiningStation, Point::new(24, 22));
    {
        let unit = state.unit_mut(second).expect("second");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.raw_mining = 0;
        unit.fuel_mining = 0;
        unit.gold_mining = 0;
        unit.total_mining = 0;
        unit.storage = 0;
    }
    assert_eq!(
        state.snapshot().unit(second).expect("second").complex,
        Some(complex),
        "the second mine is a member of the same complex"
    );

    // `AllocMenu::Balance` with the dialog capacity = sum of the mine cells:
    // 16 (first mine) + 8 (second mine) = 24, not one mine's 16.
    let shown = state.allocation_balance(TeamId::RED, complex, MiningKind::Raw, 0, 99);
    assert_eq!(shown, 24, "the balance covers every mine of the complex");
    // `AllocMenu::Balance` normalizes the extraction back (`AllocMenu_ReduceProduction`).
    assert_eq!(state.snapshot().unit(first).expect("first").raw_mining, 0);
    assert_eq!(state.snapshot().unit(second).expect("second").raw_mining, 0);

    // The same for `AllocMenu_Optimize`: the first complex mine is filled to its
    // cell maximum, the remainder goes to the second mine.
    let added = state.optimize_allocation(TeamId::RED, complex, MiningKind::Raw, 20);
    assert_eq!(added, 20, "20 units were allocated across the complex");
    assert_eq!(
        state.snapshot().unit(first).expect("first").raw_mining,
        16,
        "the first mine of the complex absorbs its 16 raw units"
    );
    assert_eq!(
        state.snapshot().unit(second).expect("second").raw_mining,
        4,
        "the rest of the request lands on the second mine"
    );
    assert_eq!(
        state.snapshot().total_mining(TeamId::RED, complex, MiningKind::Raw),
        20
    );
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
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
    assert_eq!(
        first.snapshot().state_digest(),
        second.snapshot().state_digest()
    );
    assert_eq!(
        first.snapshot().replay_digest_bytes(),
        second.snapshot().replay_digest_bytes(),
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
        changed.snapshot().state_digest(),
        first.snapshot().state_digest()
    );
    assert_ne!(
        changed.snapshot().replay_digest_crc(),
        first.snapshot().replay_digest_crc()
    );
}
