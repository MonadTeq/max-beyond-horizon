use super::*;

/// The base table covers exactly 93 types (`UNIT_END`).
#[test]
fn table_covers_all_unit_types() {
    assert_eq!(BASE_UNIT_VALUES.len(), UNIT_COUNT);
    assert_eq!(UNIT_COUNT, 93);
    assert_eq!(UnitType::COUNT, UNIT_COUNT);
    for (index, values) in BASE_UNIT_VALUES.iter().enumerate() {
        let expected = UnitType::try_from(index as u16).expect("valid unit type");
        assert_eq!(base_unit_values(expected), *values);
        // C++ `UnitValues()`: version = 1, m_is_in_use = false.
        assert_eq!(values.version, 1, "type {index}");
        assert!(!values.in_use, "type {index}");
        assert_eq!(values.fuel, 0, "type {index}");
    }
}

/// Spot check against the `SC_ATTRI` table (see `gen_unit_values.py`).
#[test]
fn representative_base_values_match_cpp_table() {
    // COMMTWR / POWERSTN: 12 turns, 56 hp, 8 armor, scan 3.
    let commtower = base_unit_values(UnitType::GoldRefinery);
    assert_eq!(
        (commtower.turns, commtower.hits, commtower.armor),
        (12, 56, 8)
    );
    assert_eq!(commtower.scan, 3);
    // POWGEN: 4 turns, 14 hp.
    let powgen = base_unit_values(UnitType::PowerGenerator);
    assert_eq!((powgen.turns, powgen.hits), (4, 14));
    // TANK (`TANK`, index 51): 4 turns, 24 hp, 10 armor, 16 attack,
    // 6 speed, 4 range, 2 shots, 4 scan, 14 ammo.
    let tank = base_unit_values(UnitType::Tank);
    assert_eq!(
        (
            tank.turns,
            tank.hits,
            tank.armor,
            tank.attack,
            tank.speed,
            tank.range,
            tank.rounds,
            tank.scan,
            tank.ammo,
            tank.attack_radius,
        ),
        (4, 24, 10, 16, 6, 4, 2, 4, 14, 0)
    );
    // SCOUT (`SCOUT`, 50): 3 turns, 16 hp, 12 attack, 12 speed, scan 9.
    let scout = base_unit_values(UnitType::Scout);
    assert_eq!(
        (
            scout.turns,
            scout.hits,
            scout.attack,
            scout.speed,
            scout.scan
        ),
        (3, 16, 12, 12, 9)
    );
    // GUNTURRT (34): stationary, 4 turns, 24 hp, 12 armor, scan 4.
    let turret = base_unit_values(UnitType::GunTurret);
    assert_eq!(
        (turret.turns, turret.hits, turret.armor, turret.speed),
        (4, 24, 12, 0)
    );
    // WALDO (`WALDO`, 92) — an easter egg without attributes.
    let waldo = base_unit_values(UnitType::DeadWaldo);
    assert_eq!(waldo.hits, 0);
    assert_eq!(waldo.turns, 0);
}

/// Attribute access: `ATTRIB_*` indices and field order as in C++.
#[test]
fn attribute_access_matches_cpp_indices() {
    let mut values = UnitValues::new();
    for attribute in 0..ATTRIB_COUNT {
        values.set_attribute(attribute, i32::from(attribute) + 1);
        assert_eq!(values.get_attribute(attribute), i32::from(attribute) + 1);
    }
    assert_eq!(values.attack, 1);
    assert_eq!(values.rounds, 2);
    assert_eq!(values.range, 3);
    assert_eq!(values.armor, 4);
    assert_eq!(values.hits, 5);
    assert_eq!(values.speed, 6);
    assert_eq!(values.scan, 7);
    assert_eq!(values.turns, 8);
    assert_eq!(values.ammo, 9);
    assert_eq!(values.move_and_fire, 10);
    assert_eq!(values.fuel, 11);
    assert_eq!(values.storage, 12);
    assert_eq!(values.attack_radius, 13);
    assert_eq!(values.agent_adjust, 14);

    // `AddAttribute` narrows to the field width (C++: int -> uint16_t).
    values.set_attribute(ATTRIB_HITS, 65_535);
    values.add_attribute(ATTRIB_HITS, 1);
    assert_eq!(values.hits, 0);
    values.set_attribute(ATTRIB_MOVE_AND_FIRE, 255);
    values.add_attribute(ATTRIB_MOVE_AND_FIRE, 1);
    assert_eq!(values.move_and_fire, 0);
}

/// `operator==` ignores `fuel` (as in C++).
#[test]
fn equality_ignores_fuel() {
    let mut left = UnitValues::new();
    let mut right = UnitValues::new();
    assert_eq!(left, right);
    right.fuel = 7;
    assert_eq!(left, right);
    left.hits = 1;
    assert_ne!(left, right);
}

/// `UpdateVersion` increments the version only for an occupied slot.
#[test]
fn version_updates_only_in_use() {
    let mut values = UnitValues::new();
    values.update_version();
    assert_eq!(values.version, 1);
    values.mark_as_in_use();
    values.update_version();
    assert_eq!(values.version, 2);
    values.mark_as_not_in_use();
    values.update_version();
    assert_eq!(values.version, 2);
}

/// Clan modifiers: `Clan C` (Von Griffin) gives Scout +2 attack/speed
/// and +2 scan, Radar +6 scan, Infiltrator -8 build turns
/// (`SC_CLANS`, `Clans::GetUnitTradeoff`).
#[test]
fn clan_tradeoffs_apply() {
    let scout = UnitType::Scout;
    assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_ATTACK), 2);
    assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_SPEED), 2);
    assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_SCAN), 2);
    assert_eq!(
        clan_tradeoff(TeamClan::VonGriffin, UnitType::Radar, ATTRIB_SCAN),
        6
    );
    assert_eq!(clan_tradeoff(TeamClan::Random, scout, ATTRIB_ATTACK), 0);
    // Another clan of the same unit — a different modifier.
    assert_eq!(
        clan_tradeoff(TeamClan::SevenKnights, scout, ATTRIB_ATTACK),
        0
    );

    let plain = base_unit_values(scout);
    let clan_values = team_base_values(TeamClan::VonGriffin);
    let modified = clan_values[scout as usize];
    assert_eq!(modified.attack, plain.attack + 2);
    assert_eq!(modified.speed, plain.speed + 2);
    assert_eq!(modified.scan, plain.scan + 2);
    // Other types do not change.
    assert_eq!(
        clan_values[UnitType::Tank as usize],
        base_unit_values(UnitType::Tank)
    );
    // `Random` — a table without modifiers.
    assert_eq!(team_base_values(TeamClan::Random), BASE_UNIT_VALUES);
}

/// `TeamUnits_UpgradeOffsetFactor` — upgrade step boundaries.
#[test]
fn upgrade_offset_factor_boundaries() {
    let mut values = UnitValues::new();
    for (base, expected) in [
        (0, 1),
        (9, 1),
        (10, 2),
        (24, 2),
        (25, 5),
        (49, 5),
        (50, 10),
        (999, 10),
    ] {
        values.set_attribute(ATTRIB_HITS, base);
        assert_eq!(
            upgrade_offset_factor(&values, ATTRIB_HITS),
            expected,
                "base {base}"
        );
    }
}

/// `ATTRIB_*` indices match `ResearchTopic` (the first 8).
#[test]
fn research_topics_match_attribute_order() {
    use crate::enums::ResearchTopic;
    assert_eq!(ATTRIB_ATTACK, ResearchTopic::Attack as u8);
    assert_eq!(ATTRIB_ROUNDS, ResearchTopic::Shots as u8);
    assert_eq!(ATTRIB_RANGE, ResearchTopic::Range as u8);
    assert_eq!(ATTRIB_ARMOR, ResearchTopic::Armor as u8);
    assert_eq!(ATTRIB_HITS, ResearchTopic::Hits as u8);
    assert_eq!(ATTRIB_SPEED, ResearchTopic::Speed as u8);
    assert_eq!(ATTRIB_SCAN, ResearchTopic::Scan as u8);
    assert_eq!(ATTRIB_TURNS, ResearchTopic::Cost as u8);
    assert_eq!(ATTRIB_COUNT as usize, ATTRIB_NAMES.len());
}

/// `base_unit_values_by_index` is a checked accessor: 0..92 valid, 93 is
/// `UNIT_END` and everything above is out of range.
#[test]
fn base_values_by_index_boundaries() {
    assert_eq!(
        base_unit_values_by_index(0),
        Some(base_unit_values(UnitType::GoldRefinery))
    );
    assert_eq!(
        base_unit_values_by_index(UNIT_COUNT as u16 - 1),
        Some(base_unit_values(UnitType::DeadWaldo))
    );
    assert_eq!(base_unit_values_by_index(UNIT_COUNT as u16), None);
    assert_eq!(base_unit_values_by_index(u16::MAX), None);
    // The index and the enum path describe the same row for all 93 types.
    for (index, unit_type) in UnitType::ALL.iter().copied().enumerate() {
        assert_eq!(
            base_unit_values_by_index(index as u16),
            Some(base_unit_values(unit_type)),
                "{unit_type:?}"
        );
    }
}

/// Attribute narrowing at the type boundaries (C++ `uint16_t = int32_t`
/// keeps the low 16 bits; `move_and_fire` is a `uint8_t`).
#[test]
fn set_attribute_narrowing_boundaries() {
    let mut values = UnitValues::new();
    for (input, expected) in [
        (0, 0),
        (1, 1),
        (i32::from(u16::MAX), 0xFFFF),
        (i32::from(u16::MAX) + 1, 0),
        (-1, 0xFFFF),
        (i32::MIN, 0),
        (i32::MAX, 0xFFFF),
    ] {
        values.set_attribute(ATTRIB_ARMOR, input);
        assert_eq!(i32::from(values.armor), expected, "armor {input}");
    }
    for (input, expected) in [(0i32, 0u8), (255, 255), (256, 0), (-1, 255)] {
        values.set_attribute(ATTRIB_MOVE_AND_FIRE, input);
        assert_eq!(values.move_and_fire, expected, "move_and_fire {input}");
    }
    // `AddAttribute` wraps at the field width.
    values.set_attribute(ATTRIB_AMMO, i32::from(u16::MAX));
    values.add_attribute(ATTRIB_AMMO, 1);
    assert_eq!(values.ammo, 0);
}

/// `agent_skill` and the "mark" version boundaries.
#[test]
fn agent_skill_and_version() {
    let mut values = UnitValues::new();
    assert_eq!(values.agent_skill(), 0);
    values.agent_adjust = 7;
    assert_eq!(values.agent_skill(), 7);
    // `UpdateVersion` wraps at `u16::MAX` (wrapping add, no panic).
    values.mark_as_in_use();
    values.version = u16::MAX;
    values.update_version();
    assert_eq!(values.version, 0);
    values.update_version();
    assert_eq!(values.version, 1);
}
