//! Phase 3 (economy): research — all topics, cost, progress distribution, levels.
//!
//! Oracle (`max-cpp-port/src`):
//! * `researchmenu.cpp:109` — `ResearchMenu_UpdateResearchProgress`
//!   (`turns = (pow(base + 0.1, 7.5) - pow(base, 7.5)) * 256 / factor`);
//! * `settings.cpp:70-77` — the topic factors (`attack` 16, `shots` 16, `range` 8,
//!   `armor` 32, `hits` 32, `speed` 16, `scan` 8, `cost` 8);
//! * `researchmenu.cpp:137` — `ResearchMenu_ApplyUpgrades` (the level delta is
//!   added to the **current** values of all 93 types);
//! * `researchmenu.cpp:224` — `ResearchMenu_NewTurn` (allocation is subtracted from
//!   `turns_to_complete`, completion increments the level);
//! * `researchmenu.cpp:258` — `ResearchMenu_CalculateFactor`;
//! * `teamunits.cpp:189/271` — `TeamUnits_GetUpgradeCost`/`UpgradeOffsetFactor`.
//!
//! The exact fixed-point `pow(x, 7.5)` is already compared bit-for-bit against
//! `std::pow` vectors in `tests/research.rs`; here the **scenario** is checked: a
//! topic reaches a level, the upgrade lands on the current values, and a second
//! research center halves the time.

use max_core::enums::{ResearchTopic, TeamClan, UnitType};
use max_core::research::{
    research_attribute, research_attribute_values, research_factor, research_turns_to_complete,
    team_upgrade_cost, RESEARCH_TOPIC_FACTORS,
};
use max_core::team::{ResearchTopicInfo, TeamId, TeamInfo, TeamKind, TeamUnits};
use max_core::unit_values::ATTRIB_TURNS;
use max_core::{crc, GameState};

const TOPICS: [ResearchTopic; 8] = [
    ResearchTopic::Attack,
    ResearchTopic::Shots,
    ResearchTopic::Range,
    ResearchTopic::Armor,
    ResearchTopic::Hits,
    ResearchTopic::Speed,
    ResearchTopic::Scan,
    ResearchTopic::Cost,
];

fn state() -> GameState {
    let mut state = GameState::new();
    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    state.team_units[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
    state
}

fn topic(state: &GameState, topic: ResearchTopic) -> ResearchTopicInfo {
    state.teams[TeamId::RED.0 as usize].research_topics[topic as usize]
}

fn set_topic(state: &mut GameState, topic: ResearchTopic, info: ResearchTopicInfo) {
    state.teams[TeamId::RED.0 as usize].research_topics[topic as usize] = info;
}

// ---------------------------------------------------------------------------
// I — all topics, level growth, distribution
// ---------------------------------------------------------------------------

#[test]
fn all_eight_topics_complete_a_level_and_apply_the_delta() {
    let mut state = state();
    let units_before = state.team_units[TeamId::RED.0 as usize].clone();

    for topic_value in TOPICS {
        set_topic(
            &mut state,
            topic_value,
            ResearchTopicInfo {
                research_level: 0,
                turns_to_complete: 1,
                allocation: 1,
            },
        );
    }

    let events = state.advance_research(TeamId::RED);

    assert_eq!(events.len(), 8, "one event per topic: {events:?}");
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.topic, TOPICS[index], "C++ topic order");
        assert_eq!(event.level, 1);
        assert_eq!(topic(&state, event.topic).research_level, 1);
    }

    // `ResearchMenu_ApplyUpgrades`: for every type `current += new − old`, where
    // `old/new` come from the topic level and the **base** value.
    let units_after = &state.team_units[TeamId::RED.0 as usize];
    for topic_value in TOPICS {
        let attribute = research_attribute(topic_value);
        for unit_type in [UnitType::Tank, UnitType::Infantry, UnitType::MiningStation] {
            let index = unit_type as usize;
            let base = units_before.base_values[index].get_attribute(attribute);
            let (old_value, new_value) = research_attribute_values(topic_value, base, 1);
            assert_eq!(
                units_after.current_values[index].get_attribute(attribute),
                base + (new_value - old_value),
                "{topic_value:?}/{unit_type:?}"
            );
            assert_eq!(
                units_after.base_values[index].get_attribute(attribute),
                base,
                "the base table never changes"
            );
        }
    }
}

#[test]
fn three_levels_accumulate_the_attack_bonus() {
    let mut state = state();
    let tank = UnitType::Tank as usize;
    let base_attack = state.team_units[TeamId::RED.0 as usize].base_values[tank].attack;

    set_topic(
        &mut state,
        ResearchTopic::Attack,
        ResearchTopicInfo {
            research_level: 0,
            turns_to_complete: 1,
            allocation: 1,
        },
    );

    for level in 1..=3u32 {
        // `ManageEconomy` re-initializes the timer for the next level.
        state.manage_economy(TeamId::RED);
        // Fast-forward: one research turn completes the level.
        state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Attack as usize]
            .turns_to_complete = 1;
        let events = state.advance_research(TeamId::RED);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].level, level);
    }

    assert_eq!(topic(&state, ResearchTopic::Attack).research_level, 3);
    // Each level adds `level * base / 10 − (level − 1) * base / 10`; the sum is
    // `3 * base / 10` because C++ re-applies the cumulative difference.
    assert_eq!(
        state.team_units[TeamId::RED.0 as usize].current_values[tank].attack,
        base_attack + (3 * base_attack) / 10
    );
}

#[test]
fn a_second_research_center_halves_the_time_to_complete() {
    let factor = RESEARCH_TOPIC_FACTORS[ResearchTopic::Range as usize];
    let turns = research_turns_to_complete(0, factor);
    assert!(turns >= 2, "the scenario needs a multi-turn topic: {turns}");

    // One center: exactly `turns` research turns.
    let mut single = state();
    set_topic(
        &mut single,
        ResearchTopic::Range,
        ResearchTopicInfo {
            research_level: 0,
            turns_to_complete: turns as u32,
            allocation: 1,
        },
    );
    for turn in 1..=turns {
        let events = single.advance_research(TeamId::RED);
        assert_eq!(
            events.is_empty(),
            turn < turns,
            "single center completes on turn {turn}"
        );
    }
    assert_eq!(topic(&single, ResearchTopic::Range).research_level, 1);

    // Two centers: `ceil(turns / 2)` research turns.
    let expected = (turns + 1) / 2;
    let mut double = state();
    set_topic(
        &mut double,
        ResearchTopic::Range,
        ResearchTopicInfo {
            research_level: 0,
            turns_to_complete: turns as u32,
            allocation: 2,
        },
    );
    for turn in 1..=expected {
        let events = double.advance_research(TeamId::RED);
        assert_eq!(
            events.len(),
            usize::from(turn == expected),
            "two centers complete on turn {turn} of {expected}"
        );
    }
    assert_eq!(topic(&double, ResearchTopic::Range).research_level, 1);
}

#[test]
fn cost_and_time_grow_with_the_level_and_research_lowers_the_price() {
    // `research_turns_to_complete` is strictly increasing in the level.
    let factor = RESEARCH_TOPIC_FACTORS[ResearchTopic::Armor as usize];
    let mut previous = 0;
    for level in 0..12 {
        let turns = research_turns_to_complete(level, factor);
        assert!(turns > previous, "level {level}: {turns} > {previous}");
        previous = turns;
    }

    // `ResearchMenu_CalculateFactor` grows linearly with the level.
    assert_eq!(research_factor(20, 0), 0);
    assert!(research_factor(20, 5) > research_factor(20, 1));

    // `TeamUnits_GetUpgradeCost` subtracts the research factor, so the next
    // purchased step is cheaper after research.
    let units = TeamUnits::with_clan(TeamClan::Random);
    let mut info = TeamInfo::new();
    info.team_type = TeamKind::Player;
    let plain = team_upgrade_cost(&units, &info, UnitType::Tank, max_core::ATTRIB_ATTACK, 0);
    info.research_topics[ResearchTopic::Attack as usize].research_level = 5;
    let researched = team_upgrade_cost(&units, &info, UnitType::Tank, max_core::ATTRIB_ATTACK, 0);
    assert!(researched < plain, "{plain} -> {researched}");

    // The `Cost` topic lowers `ATTRIB_TURNS` and never goes below one.
    let base_turns = units.base_values[UnitType::Tank as usize].turns;
    let (old_value, new_value) = research_attribute_values(ResearchTopic::Cost, i32::from(base_turns), 4);
    assert!(new_value <= old_value);
    assert!(new_value >= 1);
    assert_eq!(ATTRIB_TURNS, ResearchTopic::Cost as u8);
}

// ---------------------------------------------------------------------------
// D — determinism
// ---------------------------------------------------------------------------

fn run_research(turns: usize) -> GameState {
    let mut state = state();
    for topic_value in TOPICS {
        set_topic(
            &mut state,
            topic_value,
            ResearchTopicInfo {
                research_level: 0,
                turns_to_complete: 1,
                allocation: 1,
            },
        );
    }

    for _ in 0..turns {
        state.manage_economy(TeamId::RED);
        for topic_value in TOPICS {
            state.teams[TeamId::RED.0 as usize].research_topics[topic_value as usize]
                .turns_to_complete = 1;
        }
        state.advance_research(TeamId::RED);
    }

    state
}

#[test]
fn research_digests_are_reproducible() {
    let first = run_research(4);
    let second = run_research(4);

    assert_eq!(crc::state_checksum(&first), crc::state_checksum(&second));
    assert_eq!(crc::state_digest(&first), crc::state_digest(&second));
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );

    // The research levels and the current attribute values are part of the
    // canonical snapshot and of the replay digest. The packet-45 CRC hashes only
    // the unit lists (`GetUnitListChecksum`) and does not include team research,
    // which is C++ behavior.
    let mut changed = first.clone();
    changed.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Attack as usize]
        .research_level += 1;
    assert_ne!(crc::state_digest(&changed), crc::state_digest(&first));
    assert_ne!(
        crc::replay_digest_crc(&changed),
        crc::replay_digest_crc(&first)
    );

    // `run_research` produced four completed levels per topic.
    for topic_value in TOPICS {
        assert_eq!(topic(&first, topic_value).research_level, 4);
    }
}
