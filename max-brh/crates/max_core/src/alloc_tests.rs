use super::*;
use crate::enums::{unit_flags, TeamClan, UnitOrderState};
use crate::team::TeamId;
use crate::TeamUnits;

/// A game with one RED mine on a cell with known resources:
/// the 2×2 square around `(20, 20)` — raw 8+8, fuel 8, gold 8
/// (`Survey_GetResourcesInArea` sums 4 cells and clamps at 16).
fn state_with_mine() -> (GameState, UnitId, ComplexId) {
    let mut state = GameState::new();
    state.teams[0].team_type = crate::team::TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);

    state.map.resources.set(
        20,
        20,
        crate::map::ResourceMapEntry::new(8, Some(crate::map::ResourceKind::Raw), 0),
    );
    state.map.resources.set(
        21,
        20,
        crate::map::ResourceMapEntry::new(8, Some(crate::map::ResourceKind::Raw), 0),
    );
    state.map.resources.set(
        20,
        21,
        crate::map::ResourceMapEntry::new(8, Some(crate::map::ResourceKind::Gold), 0),
    );
    state.map.resources.set(
        21,
        21,
        crate::map::ResourceMapEntry::new(8, Some(crate::map::ResourceKind::Fuel), 0),
    );

    let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
    let grid = Point::new(20, 20);
    let id = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, grid, flags)
        .expect("шахта");

    let mut unit = state.unit(id).cloned().expect("unit");
    unit.orders = UnitOrder::PowerOn;
    unit.total_mining = 0;
    unit.raw_mining = 0;
    unit.fuel_mining = 0;
    unit.gold_mining = 0;
    state.write_unit(id, unit);

    let complex = ComplexId(1);
    state.team_units[0].complexes.push(crate::team::Complex::new(1));
    state.unit_mut(id).expect("unit").complex = Some(complex);

    (state, id, complex)
}

#[test]
fn mining_kind_fields_match_cpp_layout() {
    assert_eq!(MiningKind::Raw.flag(), 0x80);
    assert_eq!(MiningKind::Fuel.flag(), 0x20);
    assert_eq!(MiningKind::Gold.flag(), 0x40);
    assert_eq!(MiningKind::Raw.cargo_type(), CargoType::Raw);
    assert_eq!(MiningKind::ALL.map(MiningKind::index), [0, 1, 2]);
    assert_eq!(MiningKind::parse("RAW"), Some(MiningKind::Raw));
    assert_eq!(MiningKind::parse("nope"), None);
}

#[test]
fn optimize_fills_free_capacity_up_to_the_cell_maximum() {
    let (mut state, id, complex) = state_with_mine();
    let unit = state.unit(id).cloned().expect("unit");
    let capacity = survey_capacity(&state, unit.grid, MiningKind::Raw);
    assert_eq!(capacity, 16, "в квадрате 2×2 лежит сырьё 8+8");

    let added = optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 4);
    assert_eq!(added, 4);
    assert_eq!(i32::from(state.unit(id).unwrap().raw_mining), 4);
    assert_eq!(i32::from(state.unit(id).unwrap().total_mining), 4);

    // The excess hits the mine's free units (16 in total).
    let added = optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 99);
    assert_eq!(added, 12, "16 − 4 свободных единиц");
    assert_eq!(i32::from(state.unit(id).unwrap().raw_mining), 16);
}

#[test]
fn survey_limit_sixteen_matches_cpp() {
    let (state, id, _complex) = state_with_mine();
    let unit = state.unit(id).cloned().expect("unit");
    let (raw, gold, fuel) = mining_survey(&state, unit.grid);

    assert_eq!(raw, 16, "8+8, но лимит 16");
    assert_eq!(gold, 8);
    assert_eq!(fuel, 8);
}

#[test]
fn reduce_production_keeps_total_mining_in_sync() {
    let (mut state, id, complex) = state_with_mine();
    optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 6);

    let before = i32::from(state.unit(id).unwrap().raw_mining);
    assert_eq!(before, 6);

    reduce_production(&mut state, TeamId::RED, complex, MiningKind::Raw, 2);
    let unit = state.unit(id).unwrap();
    assert_eq!(i32::from(unit.raw_mining), before - 2);
    assert_eq!(
        i32::from(unit.total_mining),
        i32::from(unit.raw_mining) + i32::from(unit.fuel_mining) + i32::from(unit.gold_mining)
    );
}

#[test]
fn mining_never_exceeds_sixteen_units() {
    let (mut state, id, complex) = state_with_mine();

    for kind in MiningKind::ALL {
        optimize(&mut state, TeamId::RED, complex, kind, 16);
    }

    let unit = state.unit(id).unwrap();
    let total = i32::from(unit.raw_mining) + i32::from(unit.fuel_mining) + i32::from(unit.gold_mining);
    assert_eq!(total, MINING_CAPACITY, "16 — потолок шахты");
    assert_eq!(i32::from(unit.total_mining), total);
    assert_eq!(i32::from(unit.raw_mining), 16, "первым забрал сырьё");
    assert_eq!(i32::from(unit.fuel_mining), 0, "свободных единиц уже нет");
}

#[test]
fn net_production_counts_mining_only_when_powered() {
    let (mut state, id, _complex) = state_with_mine();
    {
        let unit = state.unit_mut(id).unwrap();
        unit.raw_mining = 3;
        // `Cargo_GetNetProduction(..., current_order = false)` takes
        // `prior_order` at `ORDER_STATE_INIT` — the order must already be in
        // execution, as on a working mine.
        unit.state = UnitOrderState::ExecutingOrder;
    }

    let unit = state.unit(id).cloned().unwrap();
    let net = cargo_get_net_production(&state, &unit, false);
    assert_eq!(net.raw, 3);

    let mut unit = unit;
    unit.orders = UnitOrder::PowerOff;
    let net = cargo_get_net_production(&state, &unit, false);
    assert_eq!(net.raw, 0, "выключенная шахта не добывает");
}

#[test]
fn update_resources_total_puts_mining_into_the_complex() {
    let (mut state, id, complex) = state_with_mine();
    state.unit_mut(id).unwrap().raw_mining = 4;
    state.unit_mut(id).unwrap().total_mining = 4;

    update_resources_total(&mut state, TeamId::RED, complex);

    let stored = state.complex(TeamId::RED, complex.0 as u16).expect("комплекс");
    assert!(stored.raw >= 4, "добыча сырья в комплексе: {}", stored.raw);
}

#[test]
fn allocation_changes_the_complex_income() {
    let (mut state, id, complex) = state_with_mine();

    // Switch all mining to gold.
    state.unit_mut(id).unwrap().raw_mining = 0;
    state.unit_mut(id).unwrap().gold_mining = 5;
    state.unit_mut(id).unwrap().total_mining = 5;
    update_resources_total(&mut state, TeamId::RED, complex);
    let gold_income = state
        .complex(TeamId::RED, complex.0 as u16)
        .expect("комплекс")
        .gold;
    assert!(gold_income >= 5);

    // Switch to raw materials — gold leaves the mining.
    state.unit_mut(id).unwrap().gold_mining = 0;
    state.unit_mut(id).unwrap().raw_mining = 5;
    update_resources_total(&mut state, TeamId::RED, complex);
    let after = state.complex(TeamId::RED, complex.0 as u16).expect("комплекс");
    assert!(after.raw >= 5);
    assert_eq!(after.gold, 0, "доля золота 0 — добычи золота нет");
}

#[test]
fn mining_weight_uses_the_cpp_difficulty_factor() {
    let (mut state, id, _complex) = state_with_mine();
    state.unit_mut(id).unwrap().raw_mining = 4;
    let unit = state.unit(id).cloned().unwrap();
    assert_eq!(cargo_get_net_production(&state, &unit, false).raw, 4);

    state.teams[0].team_type = crate::team::TeamKind::Computer;
    state.ai.settings.opponent = crate::enums::Opponent::Master as u8;
    let unit = state.unit(id).cloned().unwrap();
    assert_eq!(cargo_get_net_production(&state, &unit, false).raw, 5);

    state.ai.settings.opponent = crate::enums::Opponent::God as u8;
    let unit = state.unit(id).cloned().unwrap();
    assert_eq!(cargo_get_net_production(&state, &unit, false).raw, 6);
}

#[test]
fn mining_kind_parses_every_cargo_spelling() {
    assert_eq!(MiningKind::parse("Raw"), Some(MiningKind::Raw));
    assert_eq!(MiningKind::parse("materials"), Some(MiningKind::Raw));
    assert_eq!(MiningKind::parse(" Material "), Some(MiningKind::Raw));
    assert_eq!(MiningKind::parse("FUEL"), Some(MiningKind::Fuel));
    assert_eq!(MiningKind::parse("gold"), Some(MiningKind::Gold));
    assert_eq!(MiningKind::parse(""), None);
    assert_eq!(MiningKind::parse("power"), None);

    // `uint8_t` fields in C++: the port clamps a raw setter value to 0..=255
    // (C++ narrows/truncates; values above 16 never reach the setter from the UI).
    let mut unit = UnitInfo::new(UnitType::MiningStation, TeamId::RED, UnitId(1));
    MiningKind::Raw.set(&mut unit, 300);
    assert_eq!(unit.raw_mining, 255);
    MiningKind::Fuel.set(&mut unit, -5);
    assert_eq!(unit.fuel_mining, 0, "clamped to zero, not wrapped");
}

#[test]
fn optimize_with_zero_or_negative_material_does_nothing() {
    let (mut state, id, complex) = state_with_mine();

    assert_eq!(optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 0), 0);
    assert_eq!(state.unit(id).unwrap().raw_mining, 0);
    assert_eq!(state.unit(id).unwrap().total_mining, 0);
}

#[test]
fn optimize_skips_mines_that_are_not_active() {
    for order in [UnitOrder::PowerOff, UnitOrder::Disable, UnitOrder::Idle] {
        let (mut state, id, complex) = state_with_mine();
        state.unit_mut(id).unwrap().orders = order;

        assert_eq!(
            optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 4),
            0,
                "{order:?} mines are not counted (allocmenu.cpp:454)"
        );
        assert_eq!(state.unit(id).unwrap().raw_mining, 0);
    }
}

#[test]
fn reduce_production_beyond_the_allocated_amount_zeroes_the_mine() {
    let (mut state, id, complex) = state_with_mine();
    optimize(&mut state, TeamId::RED, complex, MiningKind::Gold, 5);

    reduce_production(&mut state, TeamId::RED, complex, MiningKind::Gold, 99);
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.gold_mining, 0);
    assert_eq!(unit.total_mining, 0, "total_mining never goes negative");
}

#[test]
fn balance_returns_the_requested_value_when_no_optimization_is_needed() {
    let (mut state, _id, complex) = state_with_mine();
    // `AllocMenu::Balance`: `material1 >= material2` returns `material2` untouched.
    assert_eq!(
        balance(&mut state, TeamId::RED, complex, MiningKind::Raw, 10, 4),
        4
    );
}

#[test]
fn balance_optimizes_and_then_restores_the_allocation() {
    let (mut state, id, complex) = state_with_mine();
    // The mine starts empty, the cell has 16 raw: `Balance` can show 16.
    let shown = balance(&mut state, TeamId::RED, complex, MiningKind::Raw, 0, 99);
    assert_eq!(shown, 16);
    // `AllocMenu_ReduceProduction(extra)` removes the temporary allocation again.
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.raw_mining, 0);
    assert_eq!(unit.total_mining, 0);
}

#[test]
fn cargo_mining_reports_free_capacity_and_cell_maxima() {
    let (mut state, id, complex) = state_with_mine();
    {
        let unit = state.unit_mut(id).unwrap();
        unit.raw_mining = 4;
        unit.fuel_mining = 3;
        unit.gold_mining = 2;
        unit.total_mining = 9;
    }

    let (materials, capacity) = get_cargo_mining(&state, TeamId::RED, complex);
    assert_eq!(materials.raw, 4);
    assert_eq!(materials.fuel, 3);
    assert_eq!(materials.gold, 2);
    assert_eq!(materials.free_capacity, 7, "16 − total_mining");
    assert_eq!(capacity.raw, 16);
    assert_eq!(capacity.gold, 8);
    assert_eq!(capacity.fuel, 8);

    assert_eq!(total_mining(&state, TeamId::RED, complex, MiningKind::Raw), 4);
    assert_eq!(total_mining(&state, TeamId::RED, complex, MiningKind::Fuel), 3);
    assert_eq!(total_mining(&state, TeamId::RED, complex, MiningKind::Gold), 2);
}

#[test]
fn inventory_and_capacity_helpers_follow_the_cargo_type() {
    let mut unit = UnitInfo::new(UnitType::FuelTruck, TeamId::RED, UnitId(1));
    unit.storage = 17;

    let inventory = cargo_get_inventory(&unit);
    assert_eq!(inventory.fuel, 17);
    assert_eq!(inventory.raw, 0);
    assert_eq!(inventory.gold, 0);

    let capacity = cargo_get_cargo_capacity(&unit);
    assert_eq!(
        capacity.fuel,
        unit.base_values.get_attribute(crate::unit_values::ATTRIB_STORAGE)
    );

    // A type without cargo has no capacity and no inventory.
    let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
    tank.storage = 5;
    assert_eq!(cargo_get_inventory(&tank), Cargo::init());
    assert_eq!(cargo_get_cargo_capacity(&tank), Cargo::init());
}

/// `Allocator::Optimize` (`allocmenu.cpp:413-436`): when a mine has no free
/// mining units left, the target resource is increased by converting the
/// mining of another resource of the same mine. The converted amount is
/// re-allocated to the source resource elsewhere in the complex by the nested
/// `AllocMenu_Optimize` (`allocmenu.cpp:427`), so the complex total stays
/// constant — the fixture therefore needs a second mine with free mining
/// units, otherwise the nested call returns 0 and nothing is converted.
#[test]
fn cpp_conversion_of_other_resources_is_ported() {
    let (mut state, id, complex) = state_with_mine();
    {
        let unit = state.unit_mut(id).unwrap();
        unit.raw_mining = 10;
        unit.fuel_mining = 2;
        unit.total_mining = 12;
    }

    // The second mine of the same complex absorbs the 2 fuel units that the
    // nested `AllocMenu_Optimize` re-allocates when converting fuel into raw.
    let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
    let grid = Point::new(21, 21);
    let other = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, grid, flags)
        .expect("second mine");
    {
        let mut unit = state.unit(other).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.complex = Some(complex);
        unit.raw_mining = 0;
        unit.fuel_mining = 0;
        unit.gold_mining = 0;
        unit.total_mining = 0;
        state.write_unit(other, unit);
    }

    // C++: 4 free units, then 2 fuel units are converted to raw → 16 raw, 0 fuel.
    let added = optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 6);
    assert_eq!(added, 6);
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.raw_mining, 16);
    assert_eq!(unit.fuel_mining, 0);
    assert_eq!(
        unit.total_mining, 16,
            "the swap keeps the mine's total at 16"
    );

    // The converted fuel production moved to the other mine of the complex.
    let other = state.unit(other).unwrap();
    assert_eq!(other.fuel_mining, 2);
    assert_eq!(other.total_mining, 2);
}

/// Termination of the conversion recursion: the nested `AllocMenu_Optimize`
/// gets `source` as its `cargo_type1` (`allocmenu.cpp:427`), so every level
/// adds one distinct `CARGO_*` bit to the mask. The gate
/// (`allocmenu.cpp:414`) rejects a resource already in the mask, hence the
/// depth is bounded by the three resource kinds (at most 3 levels) and the
/// recursion cannot loop.
#[test]
fn optimize_conversion_recursion_terminates() {
    let (mut state, id, complex) = state_with_mine();
    {
        let unit = state.unit_mut(id).unwrap();
        unit.raw_mining = 10;
        unit.fuel_mining = 3;
        unit.gold_mining = 3;
        unit.total_mining = 16;
    }

    // Saturated mine and no other mine: every conversion attempt recurses
    // down to the mask holding all three resources and then fails.
    let added = optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 6);
    assert_eq!(added, 0);
    let unit = state.unit(id).unwrap();
    assert_eq!(
        (
            unit.raw_mining,
            unit.fuel_mining,
            unit.gold_mining,
            unit.total_mining
        ),
        (10, 3, 3, 16)
    );
}
