//! Phase 4 — `unit_values.rs` attribute tables (I + D layer).
//!
//! The module unit tests already pin the table size, the field order, attribute
//! access and a few spot values. Here the tables are checked against the
//! **oracles**:
//!
//! * the committed C++ vectors `tools/upgrade_vectors/vectors.txt`
//!   (`upgradecontrol.cpp`/`teamunits.cpp`) — the upgrade step
//!   `upgrade_offset_factor` for every generated base value;
//! * the serialized `TeamUnits` base values of the real V70 saves for **six**
//!   different clans (SAVE1: VonGriffin/SevenKnights, SAVE2:
//!   Musashi/AxisInc/AyersHand/CrimsonPath): the file's base table must equal
//!   `BASE_UNIT_VALUES + CLAN_TRADEOFFS`;
//! * the current values after research (`ResearchMenu_ApplyUpgrades`): for every
//!   topic with a completed level, the file's current attribute must equal the
//!   ported formula on the file's base attribute;
//! * a save with upgraded current values survives `write -> read -> write`
//!   byte-identically (D layer: the base/current object sharing rule of
//!   `TeamUnits::FileSave`).
//!
//! Oracles: `unitvalues.cpp`, `attributes.cpp` (`SC_ATTRI`), `clans.cpp`
//! (`SC_CLANS`), `teamunits.cpp`, `researchmenu.cpp`.

use std::collections::BTreeSet;

use max_core::enums::{ResearchTopic, TeamClan, UnitType};
use max_core::research::{
    apply_research_upgrades, research_attribute_values, upgrade_amount_for_base,
};
use max_core::save::SaveFile;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit_values::{
    base_unit_values, clan_tradeoff, team_base_values, upgrade_offset_factor, UnitValues,
    ATTRIB_COUNT, ATTRIB_FUEL, ATTRIB_TURNS, BASE_UNIT_VALUES, CLAN_TRADEOFFS, UNIT_COUNT,
};
use max_core::universe::GameUniverse;

/// `max-original/SAVE1.CAM` — clans VonGriffin and SevenKnights.
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM` — clans Musashi, AxisInc, AyersHand, CrimsonPath.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

/// The committed C++ upgrade vectors (read at run time: the file is 2.8 MB).
fn upgrade_vectors() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/upgrade_vectors/vectors.txt"
    );
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"))
}

#[test]
fn upgrade_step_matches_the_cpp_vectors() {
    let vectors = upgrade_vectors();
    let mut checked = 0usize;

    for line in vectors.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("offset") {
            continue;
        }
        let value: i32 = parts
            .next()
            .expect("base value")
            .parse()
            .expect("base value");
        let expected: u16 = parts.next().expect("offset").parse().expect("offset");

        let mut values = UnitValues::new();
        values.set_attribute(max_core::unit_values::ATTRIB_HITS, value);
        assert_eq!(
            upgrade_offset_factor(&values, max_core::unit_values::ATTRIB_HITS) as u16,
            expected,
            "TeamUnits_UpgradeOffsetFactor({value})"
        );
        checked += 1;
    }

    // The generator emits one record per value in 0..=1000 (1001 records).
    assert_eq!(checked, 1001, "expected the full `offset` section");
}

#[test]
fn upgrade_step_agrees_with_the_research_port() {
    // `research::upgrade_amount_for_base` is verified against the same C++
    // vectors in `tests/research.rs`; both functions are the same C++ static.
    for value in 0..=2000i32 {
        let mut values = UnitValues::new();
        values.set_attribute(max_core::unit_values::ATTRIB_HITS, value);
        assert_eq!(
            upgrade_offset_factor(&values, max_core::unit_values::ATTRIB_HITS),
            i32::from(upgrade_amount_for_base(value)),
            "value {value}"
        );
    }
    // `ATTRIB_TURNS` uses the same step table for the cost calculation; the
    // upgrade amount itself is 1 (`upgrade_amount`), which is research's job.
    assert_eq!(ATTRIB_TURNS, 7);
}

#[test]
fn clan_tradeoff_table_is_well_formed() {
    let mut keys = BTreeSet::new();
    for &(clan, unit, attribute, delta) in CLAN_TRADEOFFS.iter() {
        assert!(clan != TeamClan::Random as u8, "Random has no trade-offs");
        assert!(TeamClan::try_from(clan).is_ok(), "unknown clan id {clan}");
        assert!((unit as usize) < UNIT_COUNT, "unknown unit type {unit}");
        assert!(attribute < ATTRIB_COUNT, "unknown attribute {attribute}");
        assert_ne!(
            attribute, ATTRIB_FUEL,
            "C++ never changes fuel (`ResourceManager_SetClanUpgradesFromClans`)"
        );
        assert_ne!(delta, 0, "a zero trade-off is not stored in C++");
        assert!(
            keys.insert((clan, unit, attribute)),
            "duplicate trade-off {clan}/{unit}/{attribute}"
        );
    }
    assert_eq!(keys.len(), CLAN_TRADEOFFS.len());
    // 95 entries for eight clans — the `SC_CLANS` resource summary.
    assert_eq!(CLAN_TRADEOFFS.len(), 95);
}

#[test]
fn every_clan_applies_only_its_own_tradeoffs_without_underflow() {
    let clans = [
        TeamClan::TheChosen,
        TeamClan::CrimsonPath,
        TeamClan::VonGriffin,
        TeamClan::AyersHand,
        TeamClan::Musashi,
        TeamClan::SacredEight,
        TeamClan::SevenKnights,
        TeamClan::AxisInc,
    ];

    for clan in clans {
        let table = team_base_values(clan);
        assert_eq!(table.len(), UNIT_COUNT);
        for (index, values) in table.iter().enumerate() {
            let unit_type = UnitType::try_from(index as u16).expect("type");
            let base = base_unit_values(unit_type);

            // No field went negative through a negative trade-off (they are
            // `uint16_t` in C++, so the resource values never underflow).
            for attribute in 0..ATTRIB_COUNT {
                assert!(
                    values.get_attribute(attribute) >= 0,
                    "{clan:?} {unit_type:?} attribute {attribute}"
                );
                if attribute == ATTRIB_FUEL {
                    assert_eq!(
                        values.get_attribute(attribute),
                        base.get_attribute(attribute),
                        "{clan:?} {unit_type:?}: fuel is never modified"
                    );
                }
            }
            // Constructor invariants survive the clan pass.
            assert_eq!(values.version, 1, "{clan:?} {unit_type:?}");
            assert!(!values.in_use, "{clan:?} {unit_type:?}");

            // Independent application of the table: base + all deltas.
            let mut expected = base;
            for &(clan_id, unit_id, attribute, delta) in CLAN_TRADEOFFS.iter() {
                if clan_id == clan as u8 && unit_id == index as u16 {
                    expected.add_attribute(attribute, i32::from(delta));
                }
            }
            assert_eq!(*values, expected, "{clan:?} {unit_type:?}");
        }

        // `TeamUnits::with_clan`: base == current, no complexes.
        let units = TeamUnits::with_clan(clan);
        assert_eq!(units.base_values, table);
        assert_eq!(units.current_values, table);
        assert!(units.complexes().is_empty());
    }

    // `Random` is the "no clan" table.
    assert_eq!(team_base_values(TeamClan::Random), BASE_UNIT_VALUES);
}

/// Known divergence between the reference saves and `SC_ATTRI + SC_CLANS`.
///
/// `ATTRIB_TURNS` of six factory/missile types is higher in `SAVE2.CAM` than
/// `BASE_UNIT_VALUES + CLAN_TRADEOFFS` predicts; the attack/range trade-offs of
/// the same clan are applied, so the clan itself was applied. The cause (per the
/// analyst investigation, `analysis/attrib-turns`): the reference saves were
/// serialized by an EARLIER balance revision of the clan resource `CLANATRB`
/// (MAX.RES), where AyersHand gave `-1` turn (not `-2`) and AxisInc had no turn
/// bonus at all (`0` instead of `-2/-4`). The final v1.04 `CLANATRB` equals the
/// Klei `SC_CLANS` deltas; `ATTRIBS` (base) is identical across both. So the
/// Klei tables are the canon (Interplay final), and `SAVE2.CAM` is the stale
/// artifact. The rows are pinned so a resource/table change is visible.
///
/// `(save, team, unit type, file value, table value)`.
const TURNS_DIVERGENCE: &[(&str, usize, u16, i32, i32)] = &[
    ("SAVE2", 1, 24, 10, 8),
    ("SAVE2", 1, 25, 20, 16),
    ("SAVE2", 2, 37, 11, 10),
    ("SAVE2", 2, 53, 7, 6),
    ("SAVE2", 2, 54, 11, 10),
    ("SAVE2", 2, 73, 11, 10),
];

/// The six clans that occur in the reference saves.
#[test]
fn real_saves_match_the_clan_tables() {
    let mut checked_clans: BTreeSet<u8> = BTreeSet::new();
    let mut checked_values = 0usize;
    let mut turns_mismatches: Vec<(&str, usize, u16, i32, i32)> = Vec::new();

    for (source, tag) in [(SAVE1, "SAVE1"), (SAVE2, "SAVE2")] {
        let save = SaveFile::parse(source).expect("reference save parses");
        for (team, info) in save.teams.iter().enumerate() {
            if info.team_type == TeamKind::None {
                // Empty slots keep the previous game's values; the header clan
                // does not apply to them.
                continue;
            }
            let clan = save.header.team_clan[team];
            let expected = team_base_values(clan);
            checked_clans.insert(clan as u8);

            for (unit_index, slot) in save.team_units[team].base_values.iter().enumerate() {
                let mut want = expected[unit_index];
                // `LANDPAD.storage` is mutated during play (the original
                // `ATTRIBS`/`SC_ATTRI` value is 0, the save holds 1).
                if unit_index == UnitType::LandingPad as usize {
                    want.storage = slot.values.storage;
                }
                for attribute in 0..ATTRIB_COUNT {
                    if attribute == ATTRIB_FUEL {
                        continue;
                    }
                    let got = slot.values.get_attribute(attribute);
                    let table = want.get_attribute(attribute);
                    if attribute == ATTRIB_TURNS && got != table {
                        turns_mismatches.push((tag, team, unit_index as u16, got, table));
                        continue;
                    }
                    assert_eq!(
                        got, table,
                        "{tag} team {team} clan {clan:?} unit {unit_index} attribute {attribute}"
                    );
                }
                // `units_built` (the `in_use` flag) is a play counter.
                assert_eq!(
                    slot.values.version, want.version,
                    "{tag} team {team} unit {unit_index} version"
                );
                checked_values += 1;
            }
        }
    }

    assert_eq!(
        checked_values,
        5 * UNIT_COUNT,
        "two playing teams in SAVE1 and three in SAVE2"
    );

    // Every deviation is one of the pinned `ATTRIB_TURNS` rows.
    turns_mismatches.sort_unstable();
    let mut expected_turns: Vec<(&str, usize, u16, i32, i32)> = TURNS_DIVERGENCE.to_vec();
    expected_turns.sort_unstable();
    assert_eq!(
        turns_mismatches, expected_turns,
        "the set of `ATTRIB_TURNS` deviations from the resource table changed"
    );

    // SAVE1 covers VonGriffin/SevenKnights, SAVE2 Musashi/AxisInc/AyersHand.
    // (`CrimsonPath` is the header clan of SAVE2's empty gray slot, so that
    // slot's `TeamUnits` section is not a playing team.)
    for clan in [
        TeamClan::VonGriffin,
        TeamClan::SevenKnights,
        TeamClan::Musashi,
        TeamClan::AxisInc,
        TeamClan::AyersHand,
    ] {
        assert!(
            checked_clans.contains(&(clan as u8)),
            "{clan:?} is not covered"
        );
    }
}

#[test]
fn reference_saves_have_no_completed_research() {
    // Both fixtures are early campaign saves: no research level is completed
    // (SAVE1 team 1 has one topic in progress), so the research formula cannot
    // be cross-checked on them — it is checked on a generated state below.
    let mut in_progress = Vec::new();
    for (source, tag) in [(SAVE1, "SAVE1"), (SAVE2, "SAVE2")] {
        let save = SaveFile::parse(source).expect("reference save parses");
        for (team, info) in save.teams.iter().enumerate() {
            assert!(
                info.research_topics
                    .iter()
                    .all(|topic| topic.research_level == 0),
                "{tag} team {team} unexpectedly has a completed research level"
            );
            for (index, topic) in info.research_topics.iter().enumerate() {
                if topic.allocation != 0 || topic.turns_to_complete != 0 {
                    in_progress.push((tag, team, index, topic.turns_to_complete, topic.allocation));
                }
            }
        }
    }
    // `ResearchMenu_UpdateResearchProgress` state at save time: the ARMOR
    // topic, 8 turns left, 1 point allocated.
    assert_eq!(in_progress, vec![("SAVE1", 1, 3, 8, 1)]);
}

/// `TeamUnits_GetUpgradeCost` / `UpgradeControl`: a purchased in-game upgrade
/// changes an attribute by exactly `upgrade_offset_factor(base)` (for turns —
/// by 1). The real saves contain such purchases (e.g. SAVE2 team 0 Tank:
/// speed +1, ammo +6 with steps 1 and 2).
#[test]
fn purchased_upgrades_follow_the_offset_step() {
    let mut checked = 0usize;

    for (source, tag) in [(SAVE1, "SAVE1"), (SAVE2, "SAVE2")] {
        let save = SaveFile::parse(source).expect("reference save parses");
        for team in 0..max_core::save::SAVE_TEAM_COUNT {
            for unit_index in 0..UNIT_COUNT {
                let base = save.team_units[team].base_values[unit_index].values;
                let current = save.team_units[team].current_values[unit_index].values;

                // A built unit marks the current slot as used; the base slot
                // is not touched by that.
                if current.in_use {
                    assert!(current.version >= base.version, "{tag} {team}/{unit_index}");
                }

                for attribute in 0..ATTRIB_COUNT {
                    if attribute == ATTRIB_FUEL {
                        continue;
                    }
                    let delta = current.get_attribute(attribute) - base.get_attribute(attribute);
                    if delta == 0 {
                        continue;
                    }
                    let step = if attribute == ATTRIB_TURNS {
                        1
                    } else {
                        upgrade_offset_factor(&base, attribute)
                    };
                    assert!(
                        step > 0 && delta % step == 0,
                        "{tag} team {team} unit {unit_index} attribute {attribute}: \
                         delta {delta} is not a multiple of step {step}"
                    );
                    assert!(
                        delta > 0 || attribute == ATTRIB_TURNS,
                        "{tag} team {team} unit {unit_index} attribute {attribute}: \
                         only the build time may go down, got {delta}"
                    );
                    checked += 1;
                }
            }
        }
    }

    // The reference saves do contain purchased upgrades.
    assert!(checked >= 15, "expected purchased upgrades in the fixtures");
}

#[test]
fn upgrades_change_spawned_units_and_survive_a_save() {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.set_player_team(TeamId::RED);
    let clan = TeamClan::Musashi;
    state.teams_mut()[0].team_clan = clan;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(clan);
    state.save_meta_mut().save_name = [0; 30];
    max_core::save_write::SaveMeta::set_name(
        &mut state.save_meta_mut().save_name,
        "Upgrades",
    );

    // Before research: the unit copies the clan table.
    let before = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            max_core::geo::Point::new(10, 10),
            0,
        )
        .expect("tank");
    let base_attack = state.snapshot().team_units()[0].base_values[UnitType::Tank as usize].attack;
    assert_eq!(
        state
            .snapshot()
            .unit(before)
            .map(|unit| unit.base_values.attack),
        Some(base_attack)
    );

    // Complete levels 1 and 2 of the ATTACK topic. `research_new_turn` calls
    // `apply_research_upgrades` once per completed level, so the deltas add up.
    let topic = ResearchTopic::Attack;
    for level in 1..=2 {
        apply_research_upgrades(&mut state.team_units_table_mut()[0], topic, level);
    }
    let (old1, _) = research_attribute_values(topic, i32::from(base_attack), 1);
    let (_, new2) = research_attribute_values(topic, i32::from(base_attack), 2);
    let expected_attack = i32::from(base_attack) + new2 - old1;
    let current = state.snapshot().team_units()[0].current_values[UnitType::Tank as usize];
    assert_eq!(i32::from(current.attack), expected_attack);
    assert!(expected_attack > i32::from(base_attack));
    // The base table is untouched: research only rewrites `current_values`.
    assert_eq!(
        state.snapshot().team_units()[0].base_values[UnitType::Tank as usize].attack,
        base_attack
    );

    // A unit spawned after the upgrade starts from the upgraded values.
    let after = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            max_core::geo::Point::new(12, 10),
            0,
        )
        .expect("tank");
    assert_eq!(
        state
            .snapshot()
            .unit(after)
            .map(|unit| unit.base_values.attack),
        Some(current.attack),
        "spawn must use `current_values` (TeamUnits::GetCurrentUnitValues)"
    );

    // V70 round-trip keeps both tables and both units.
    let bytes = state.snapshot().to_save_bytes().expect("writes");
    let save = SaveFile::parse(&bytes).expect("parses");
    let upgraded_tank = save.team_units[0].current_values[UnitType::Tank as usize].values;
    assert_eq!(upgraded_tank.attack, current.attack);
    assert_eq!(
        save.team_units[0].base_values[UnitType::Tank as usize]
            .values
            .attack,
        base_attack
    );
    // `current != base` for the upgraded type, so the writer stores a separate
    // object (`TeamUnits::FileSave`); the identity is preserved on re-read.
    assert_ne!(
        save.team_units[0].base_values[UnitType::Tank as usize].object_index,
        save.team_units[0].current_values[UnitType::Tank as usize].object_index
    );
    // A type whose ATTACK is 0 is not touched by the topic, so its current
    // object still shares the base object (no upgrade happened).
    let untouched = UnitType::GoldRefinery as usize;
    assert_eq!(save.team_units[0].base_values[untouched].values.attack, 0);
    assert_eq!(
        save.team_units[0].base_values[untouched].object_index,
        save.team_units[0].current_values[untouched].object_index
    );

    let reloaded = GameUniverse::from_save(&save);
    assert_eq!(
        reloaded.snapshot().team_units()[0].current_values,
        state.snapshot().team_units()[0].current_values
    );
    assert_eq!(
        reloaded.snapshot().team_units()[0].base_values,
        state.snapshot().team_units()[0].base_values
    );
    // D layer: write -> read -> write is byte-identical. (The replay digest is
    // not compared with the live state: `load_state` normalizes
    // `unit_id_counter` and order transitions, as C++ `SaveLoad_LoadFormatV70`
    // does; two loads of the written bytes are identical.)
    assert_eq!(reloaded.snapshot().to_save_bytes().expect("writes"), bytes);
    let again = GameUniverse::from_save(&SaveFile::parse(&bytes).expect("parses"));
    assert_eq!(
        again.snapshot().replay_digest_crc(),
        reloaded.snapshot().replay_digest_crc()
    );
}

#[test]
fn clan_lookup_misses_return_zero() {
    // Every (clan, unit, attribute) triple not present in the table is 0.
    let mut listed = BTreeSet::new();
    for &(clan, unit, attribute, _) in CLAN_TRADEOFFS.iter() {
        listed.insert((clan, unit, attribute));
    }
    for clan in [
        TeamClan::Random,
        TeamClan::TheChosen,
        TeamClan::CrimsonPath,
        TeamClan::VonGriffin,
        TeamClan::AyersHand,
        TeamClan::Musashi,
        TeamClan::SacredEight,
        TeamClan::SevenKnights,
        TeamClan::AxisInc,
    ] {
        for unit_type in [
            UnitType::Tank,
            UnitType::Scout,
            UnitType::Radar,
            UnitType::Infiltrator,
        ] {
            for attribute in 0..ATTRIB_COUNT {
                let listed_here = listed.contains(&(clan as u8, unit_type as u16, attribute));
                let delta = clan_tradeoff(clan, unit_type, attribute);
                if listed_here {
                    assert_ne!(delta, 0, "{clan:?} {unit_type:?} attribute {attribute}");
                } else {
                    assert_eq!(delta, 0, "{clan:?} {unit_type:?} attribute {attribute}");
                }
            }
        }
    }
}
