//! Phase 4 — `team.rs` (TeamInfo/TeamUnits/Complex) integration (I + D layer).
//!
//! The module unit tests cover the reset layout, the complex id order and the
//! clan initialization for a couple of spot cases. Here the team model is
//! exercised as the engine uses it:
//!
//! * clan initialization for **all** `TeamClan` values: base == current, no
//!   complex leakage, idempotence, no stacking of trade-offs on a clan switch;
//! * the "lowest free complex id" rule under a long create/remove sequence;
//! * the real saves' `TeamInfo`/`TeamUnits`/`Complex` sections are internally
//!   consistent (complex ids, building counters, unit membership);
//! * research progression through `research_new_turn` (levels, upgrades of the
//!   current values) and its byte-exact V70 round-trip;
//! * V70 boundary: gold is `uint16_t` in the file (`TeamUnits::FileLoad`), so a
//!   `u32` runtime value truncates exactly like C++.
//!
//! Oracles: `ctinfo.hpp`, `teamunits.cpp`, `complex.cpp`, `researchmenu.cpp`,
//! `resource_manager.cpp` (`ResourceManager_InitClanUnitValues`).

use max_core::enums::{ResearchTopic, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::research::{research_attribute_values, research_new_turn};
use max_core::save::SaveFile;
use max_core::team::{
    ComplexId, ResearchTopicInfo, TeamId, TeamInfo, TeamKind, TeamUnits, RESEARCH_TOPICS,
};
use max_core::unit_values::{team_base_values, BASE_UNIT_VALUES, UNIT_COUNT};
use max_core::universe::GameUniverse;

/// `max-original/SAVE1.CAM` — two playing teams.
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM` — three playing teams.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

/// All clan values except `Random` (a `Random` clan is resolved to one of these
/// by `ResourceManager_InitClanUnitValues`).
const CLANS: [TeamClan; 8] = [
    TeamClan::TheChosen,
    TeamClan::CrimsonPath,
    TeamClan::VonGriffin,
    TeamClan::AyersHand,
    TeamClan::Musashi,
    TeamClan::SacredEight,
    TeamClan::SevenKnights,
    TeamClan::AxisInc,
];

#[test]
fn clan_initialization_is_idempotent_for_every_clan() {
    for clan in CLANS {
        let table = team_base_values(clan);

        let mut units = TeamUnits::with_clan(clan);
        assert_eq!(units.gold, 0);
        assert!(units.complexes().is_empty());
        assert_eq!(units.base_values, table);
        assert_eq!(units.current_values, table);

        // Something accumulates: gold and a complex.
        units.set_gold(777);
        let complex = units.create_complex();
        assert_eq!(units.complexes().len(), 1);

        // Re-initialization: the complex list is cleared, the table is rebuilt
        // from scratch (a repeated call never stacks trade-offs).
        units.init_clan_unit_values(clan);
        assert_eq!(units.gold, 777, "gold is not a clan attribute");
        assert!(units.complexes().is_empty(), "ClearComplexes in C++");
        assert_eq!(units.base_values, table);
        assert_eq!(units.current_values, table);
        assert!(units.get_complex(complex).is_none());

        // A clan switch produces exactly the fresh table of the other clan.
        let other = TeamClan::VonGriffin;
        units.init_clan_unit_values(other);
        assert_eq!(units.base_values, team_base_values(other));
        assert_eq!(units.current_values, team_base_values(other));
    }

    // `TeamClan::Random` is "no modifiers" (the engine resolves it before
    // initialization; `team_base_values(Random)` is the plain table).
    let random = TeamUnits::with_clan(TeamClan::Random);
    assert_eq!(random.base_values, BASE_UNIT_VALUES);
    assert_eq!(random.current_values, BASE_UNIT_VALUES);
}

#[test]
fn complex_ids_always_take_the_lowest_free_number() {
    let mut units = TeamUnits::with_clan(TeamClan::Random);

    // Create 40 complexes: ids 1..=40, ascending.
    let mut ids = Vec::new();
    for _ in 0..40 {
        ids.push(units.create_complex());
    }
    assert_eq!(ids.first().copied(), Some(ComplexId(1)));
    assert_eq!(ids.last().copied(), Some(ComplexId(40)));
    assert_eq!(
        units
            .complexes()
            .iter()
            .map(|complex| complex.id)
            .collect::<Vec<_>>(),
        (1..=40).collect::<Vec<i16>>()
    );

    // Remove every third one; each removal keeps the list sorted.
    for id in ids.iter().step_by(3) {
        assert!(units.remove_complex(*id));
        assert!(units.get_complex(*id).is_none());
        assert!(!units.remove_complex(*id));
    }
    let remaining: Vec<i16> = units.complexes().iter().map(|complex| complex.id).collect();
    let mut sorted = remaining.clone();
    sorted.sort_unstable();
    assert_eq!(remaining, sorted);

    // New complexes fill the gaps from the lowest id.
    let first_free = (1..).find(|id| !remaining.contains(id)).expect("a free id");
    let created = units.create_complex();
    assert_eq!(created, ComplexId(first_free));
    assert_eq!(
        units.get_complex(created).map(|complex| complex.id),
        Some(first_free)
    );

    units.clear_complexes();
    assert!(units.complexes().is_empty());
    assert_eq!(
        units.create_complex(),
        ComplexId(1),
        "ids restart after Clear"
    );
}

#[test]
fn team_info_reset_restores_every_field() {
    let mut info = TeamInfo::new();
    info.team_type = TeamKind::Computer;
    info.finished_turn = true;
    info.team_clan = TeamClan::AxisInc;
    info.research_topics[ResearchTopic::Attack as usize] = ResearchTopicInfo {
        research_level: 3,
        turns_to_complete: 12,
        allocation: 4,
    };
    info.team_points = 0xDEAD_BEEF;
    info.number_of_objects_created = 4095;
    info.unit_counters[UnitType::Tank as usize] = 0xFFFF_FFFF;
    info.score_graph[49] = -32768;
    info.stats_factories_built = -2;
    info.stats_mines_built = 3;
    info.stats_buildings_built = 4;
    info.stats_units_built = 5;
    info.casualties[UnitType::Infiltrator as usize] = 0xFFFF_FFFF;
    info.stats_gold_spent_on_upgrades = 123_456;

    info.reset();

    let fresh = TeamInfo::new();
    assert_eq!(info, fresh);
    assert_eq!(info.team_type, TeamKind::None);
    assert!(!info.finished_turn);
    assert_eq!(info.team_clan, TeamClan::Random);
    assert_eq!(info.team_points, 0);
    assert_eq!(info.number_of_objects_created, 0);
    // C++ `CTInfo::Reset`: `unit_counters[i] = 1` for all 93 types.
    assert!(info.unit_counters.iter().all(|count| *count == 1));
    assert!(info.score_graph.iter().all(|value| *value == 0));
    assert!(info.casualties.iter().all(|value| *value == 0));
    assert_eq!(
        info.stats_factories_built
            + info.stats_mines_built
            + info.stats_buildings_built
            + info.stats_units_built,
        0
    );
    assert_eq!(info.stats_gold_spent_on_upgrades, 0);
    assert_eq!(info.research_topics.len(), RESEARCH_TOPICS.len());
    for topic in &info.research_topics {
        assert_eq!(topic.research_level, 0);
        assert_eq!(topic.turns_to_complete, 0);
        assert_eq!(topic.allocation, 0);
    }
}

#[test]
fn real_save_team_sections_are_consistent() {
    for (source, tag) in [(SAVE1, "SAVE1"), (SAVE2, "SAVE2")] {
        let save = SaveFile::parse(source).expect("reference save parses");
        let mut playing = 0usize;

        for (team, info) in save.teams.iter().enumerate() {
            if info.team_type == TeamKind::None {
                // An empty slot has no `TeamUnits`/heat map/AI block of its own.
                continue;
            }
            playing += 1;
            assert_eq!(
                info.team_clan, save.header.team_clan[team],
                "{tag}: TeamInfo clan must match the header"
            );
            // Every unit counter is a real play counter, not the reset value.
            assert!(
                info.unit_counters.iter().any(|count| *count > 1),
                "{tag} team {team}: the save has units, so some counter is > 1"
            );

            // Complex ids are unique and the list is sorted (C++ `InsertBefore`).
            let complex_ids: Vec<i16> = save.team_units[team]
                .complexes
                .iter()
                .map(|slot| slot.complex.id)
                .collect();
            let mut sorted = complex_ids.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(complex_ids, sorted, "{tag} team {team}: complex ids");
            let unique: std::collections::BTreeSet<i16> = complex_ids.iter().copied().collect();
            assert_eq!(
                unique.len(),
                complex_ids.len(),
                "{tag} team {team}: duplicate id"
            );

            // Every unit's `complex` reference points at a complex of its team,
            // and the complex's `buildings` counter is at least the number of
            // attached units (C++ `Complex::Grow`/`Shrink`).
            let mut members: std::collections::BTreeMap<i16, usize> =
                std::collections::BTreeMap::new();
            for unit in &save.units {
                if unit.info.team.0 as usize != team {
                    continue;
                }
                if let Some(complex) = unit.info.complex {
                    assert!(
                        complex_ids.contains(&complex.0),
                        "{tag}: unit {:?} references unknown complex {:?}",
                        unit.info.id,
                        complex
                    );
                    *members.entry(complex.0).or_default() += 1;
                }
            }
            for slot in &save.team_units[team].complexes {
                let count = members.get(&slot.complex.id).copied().unwrap_or(0);
                assert!(
                    slot.complex.buildings as usize >= count,
                    "{tag} team {team} complex {:?}: buildings counter {} < members {count}",
                    ComplexId(slot.complex.id),
                    slot.complex.buildings
                );
                // Known data quirk: SAVE2 team 0 keeps five single-building
                // complexes (ids 3..=7) with no attached unit in the file
                // (destroyed buildings whose complex object was still saved).
                // Pinned so that a parser regression that loses membership is
                // visible.
                let orphan = slot.complex.buildings as usize - count;
                let expected_orphan =
                    usize::from(tag == "SAVE2" && team == 0 && slot.complex.id >= 3);
                assert_eq!(
                    orphan,
                    expected_orphan,
                    "{tag} team {team} complex {:?}: orphan building count",
                    ComplexId(slot.complex.id)
                );
            }
        }

        assert!(playing >= 2, "{tag}: expected at least two playing teams");
    }
}

#[test]
fn research_progress_upgrades_current_values_and_round_trips() {
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().teams[0].team_clan = TeamClan::Random;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.state_mut().player_team = TeamId::RED;

    // The ATTACK topic with one point per turn.
    let topic = ResearchTopic::Attack;
    state.state_mut().teams[0].research_topics[topic as usize] = ResearchTopicInfo {
        research_level: 0,
        turns_to_complete: 0,
        allocation: 1,
    };
    let base_attack =
        i32::from(state.state().team_units[0].base_values[UnitType::Tank as usize].attack);

    // `research_new_turn` deducts `allocation`; a completed level applies the
    // upgrade (`ResearchMenu_ApplyUpgrades`). The first call initializes
    // `turns_to_complete` through the default factor.
    let mut events = 0usize;
    let mut calls = 0usize;
    while events == 0 {
        calls += 1;
        assert!(calls < 10_000, "research never completed");
        let produced = {
            let state = state.state_mut();
            research_new_turn(&mut state.team_units[0], &mut state.teams[0], TeamId::RED)
        };
        events += produced.len();
        for event in produced {
            assert_eq!(event.topic, topic);
        }
    }
    let level = state.state().teams[0].research_topics[topic as usize].research_level;
    assert_eq!(level, events as u32);
    assert_eq!(level, 1);

    // `current = base + new(level) - old(1)` (the deltas telescope).
    let (old1, _) = research_attribute_values(topic, base_attack, 1);
    let (_, new_level) = research_attribute_values(topic, base_attack, level);
    let expected = base_attack + new_level - old1;
    assert_eq!(
        i32::from(state.state().team_units[0].current_values[UnitType::Tank as usize].attack),
        expected
    );

    // The base table is untouched, and a newly spawned unit uses the upgraded
    // values.
    assert_eq!(
        i32::from(state.state().team_units[0].base_values[UnitType::Tank as usize].attack),
        base_attack
    );
    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("tank");
    assert_eq!(
        state
            .snapshot()
            .unit(tank)
            .map(|unit| i32::from(unit.base_values.attack)),
        Some(expected)
    );

    // V70 keeps the research state and both value tables; write -> read -> write
    // is byte-identical.
    let bytes = state.state().to_save_bytes().expect("writes");
    let save = SaveFile::parse(&bytes).expect("parses");
    let saved_topic = save.teams[0].research_topics[topic as usize];
    assert_eq!(saved_topic.research_level, level);
    assert_eq!(
        i32::from(
            save.team_units[0].current_values[UnitType::Tank as usize]
                .values
                .attack
        ),
        expected
    );
    let reloaded = GameUniverse::from_save(&save);
    assert_eq!(
        reloaded.state().teams[0].research_topics[topic as usize],
        state.state().teams[0].research_topics[topic as usize]
    );
    assert_eq!(reloaded.state().to_save_bytes().expect("writes"), bytes);
}

#[test]
fn gold_is_a_v70_u16_on_disk() {
    // `TeamUnits::FileLoad` reads `uint16_t gold_v70`, `FileSave` writes it back;
    // the runtime value is `uint32_t`. Values above 65535 truncate exactly like
    // the C++ narrowing cast.
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);

    for (gold, expected) in [
        (0u32, 0u16),
        (1, 1),
        (65_535, 65_535),
        (65_536, 0),
        (70_000, 70_000u32 as u16),
        (0xFFFF_FFFF, 0xFFFF),
    ] {
        state.state_mut().team_units[0].set_gold(gold);
        let bytes = state.state().to_save_bytes().expect("writes");
        let save = SaveFile::parse(&bytes).expect("parses");
        assert_eq!(save.team_units[0].gold, expected, "gold {gold}");
        let reloaded = GameUniverse::from_save(&save);
        assert_eq!(
            reloaded.state().team_units[0].gold,
            u32::from(expected),
            "gold {gold}"
        );
    }
}

#[test]
fn team_tables_cover_all_93_types_for_every_clan() {
    for clan in CLANS.iter().copied().chain([TeamClan::Random]) {
        let units = TeamUnits::with_clan(clan);
        assert_eq!(units.base_values.len(), UNIT_COUNT);
        assert_eq!(units.current_values.len(), UNIT_COUNT);
        // Every slot is addressable by the `UnitType` index.
        for (index, unit_type) in UnitType::ALL.iter().copied().enumerate() {
            assert_eq!(unit_type as usize, index);
            assert_eq!(
                units.base_unit_values(unit_type as u16),
                &units.base_values[index]
            );
            assert_eq!(
                units.current_unit_values(unit_type as u16),
                &units.current_values[index]
            );
        }
        // Mutating a current value does not touch the base table.
        let mut units = units;
        units.current_unit_values_mut(UnitType::Tank as u16).attack += 1;
        assert_ne!(units.current_values, units.base_values);
    }
    assert_eq!(RESEARCH_TOPICS.len(), 8);
}
