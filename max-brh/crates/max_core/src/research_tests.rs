use super::*;
use crate::enums::{Opponent, TeamClan};

#[test]
fn upgrade_amount_matches_cpp_thresholds() {
    assert_eq!(upgrade_amount_for_base(0), 1);
    assert_eq!(upgrade_amount_for_base(9), 1);
    assert_eq!(upgrade_amount_for_base(10), 2);
    assert_eq!(upgrade_amount_for_base(24), 2);
    assert_eq!(upgrade_amount_for_base(25), 5);
    assert_eq!(upgrade_amount_for_base(49), 5);
    assert_eq!(upgrade_amount_for_base(50), 10);
    assert_eq!(upgrade_amount(ATTRIB_TURNS, 100), 1);
}

#[test]
fn research_factor_matches_cpp() {
    assert_eq!(research_factor(20, 0), 0);
    assert_eq!(research_factor(20, 1), 2);
    assert_eq!(research_factor(20, 9), 18);
    assert_eq!(research_factor(7, 4), 2);
    assert_eq!(research_factor(50, 3), 15);
}

#[test]
fn research_attribute_matches_the_topic_enum() {
    // `RESEARCH_TOPIC_* = ATTRIB_*` (researchmenu.cpp): the topic value is the
    // attribute index.
    for topic in [
        ResearchTopic::Attack,
        ResearchTopic::Shots,
        ResearchTopic::Range,
        ResearchTopic::Armor,
        ResearchTopic::Hits,
        ResearchTopic::Speed,
        ResearchTopic::Scan,
        ResearchTopic::Cost,
    ] {
        assert_eq!(research_attribute(topic), topic as u8);
    }
    assert_eq!(research_attribute(ResearchTopic::Attack), ATTRIB_ATTACK);
    assert_eq!(research_attribute(ResearchTopic::Hits), ATTRIB_HITS);
}

#[test]
fn upgrade_control_factor_table_is_total() {
    assert_eq!(UPGRADE_CONTROL_FACTORS, [4, 4, 8, 2, 2, 4, 8, 16, 2]);
    for id in 0..UPGRADE_CONTROL_COUNT as u8 {
        assert_eq!(
            upgrade_control_factor(id),
            UPGRADE_CONTROL_FACTORS[id as usize]
        );
    }
    // Unknown ids return the C++ `default` value (0), never panic.
    assert_eq!(upgrade_control_factor(UPGRADE_CONTROL_COUNT as u8), 0);
    assert_eq!(upgrade_control_factor(u8::MAX), 0);
}

#[test]
fn research_turns_rejects_degenerate_factors() {
    // C++ would divide by zero (inf → INT_MIN); the port returns "never".
    assert_eq!(research_turns_to_complete(0, 0), i32::MAX);
    assert_eq!(research_turns_to_complete(0, -5), i32::MAX);

    // A positive factor always yields a finite, strictly positive number of turns.
    let level_zero = research_turns_to_complete(0, 16);
    assert!(level_zero > 0);
    // A higher level needs more turns (`(base + 0.1)^7.5 - base^7.5` grows).
    assert!(research_turns_to_complete(10, 16) > level_zero);
    // A larger factor (cheaper topic) needs fewer turns.
    assert!(research_turns_to_complete(5, 32) < research_turns_to_complete(5, 8));
}

#[test]
fn research_attribute_values_clamp_cost_and_hits() {
    // The Cost branch divides by `level + 9/10` and clamps at 1.
    let (old_value, new_value) = research_attribute_values(ResearchTopic::Cost, 1, 1);
    assert!(old_value >= 1 && new_value >= 1);
    // Hits/Speed saturate at `u16::MAX` when the base value is huge.
    let (_, hits) = research_attribute_values(ResearchTopic::Hits, i32::from(u16::MAX), 20);
    assert_eq!(hits, i32::from(u16::MAX));
    let (_, speed) = research_attribute_values(ResearchTopic::Speed, i32::from(u16::MAX), 20);
    assert_eq!(speed, i32::from(u16::MAX));
    // The conversion truncates to i32 (`(int32_t)` in C++).
    assert!(research_attribute_values(ResearchTopic::Attack, 10, 3).1 <= i32::MAX);
}

#[test]
fn upgrade_packet_values_follow_the_cpp_field_order() {
    let values = crate::unit_values::base_unit_values(UnitType::Tank);
    let packet = upgrade_packet_values(&values);

    assert_eq!(packet.len(), 13);
    assert_eq!(UPGRADE_PACKET_ATTRIBUTES.len(), 13);
    assert_eq!(UPGRADE_PACKET_ATTRIBUTES[0], ATTRIB_TURNS);
    assert_eq!(UPGRADE_PACKET_ATTRIBUTES[12], ATTRIB_AGENT_ADJUST);
    for (index, attribute) in UPGRADE_PACKET_ATTRIBUTES.iter().enumerate() {
        assert_eq!(
            i32::from(packet[index]),
            values.get_attribute(*attribute),
                "packet field {index}"
        );
    }

    // A value stored as `0xFFFF` stays in range; negative math never leaks in
    // (the fields are already `u16` in `UnitValues`).
    let mut extreme = UnitValues::new();
    extreme.set_attribute(ATTRIB_HITS, -1);
    assert_eq!(upgrade_packet_values(&extreme)[1], u16::MAX);
}

#[test]
fn quote_rejects_zero_base_and_costs_over_the_limit() {
    // `team_base == 0` — no formula denominator, `Increase` is disabled.
    let zero = UpgradeQuote::new(ATTRIB_ATTACK, 0, 0, 0);
    assert!(!zero.can_increase(u32::MAX));

    // `current / base = 2.2` → ~1476 > `UPGRADECONTROL_UPGRADE_COST_LIMIT` (800).
    let expensive = UpgradeQuote::new(ATTRIB_ATTACK, 10, 32, 0);
    assert!(expensive.cost() >= UPGRADE_COST_LIMIT, "{}", expensive.cost());
    assert!(!expensive.can_increase(u32::MAX));

    // An i32 overflow in `upgrade_control_cost` is reported as `i32::MIN`
    // (x86 `cvttsd2si`), and the step difference then clamps at 1.
    assert_eq!(
        upgrade_control_cost(ATTRIB_ATTACK, 200, 0, 1),
        i32::MIN,
            "value above i32 is truncated like C++"
    );
    let overflow = UpgradeQuote::new(ATTRIB_ATTACK, 1, 200, 0);
    assert_eq!(overflow.cost(), 1, "minimum step cost");

    // A normal quote is refused with one credit less than the price.
    let normal = UpgradeQuote::new(ATTRIB_ATTACK, 12, 12, 0);
    let cost = normal.cost();
    assert!(cost >= 1 && cost < UPGRADE_COST_LIMIT);
    assert!(!normal.can_increase(cost as u32 - 1));
    assert!(normal.can_increase(cost as u32));
}

#[test]
fn team_upgrade_cost_applies_the_computer_difficulty_multipliers() {
    let units = TeamUnits::with_clan(TeamClan::Random);
    let mut info = TeamInfo::new();
    info.team_type = TeamKind::Computer;

    let average = team_upgrade_cost(&units, &info, UnitType::Tank, ATTRIB_ATTACK, Opponent::Average as u8);
    let clueless = team_upgrade_cost(&units, &info, UnitType::Tank, ATTRIB_ATTACK, Opponent::Clueless as u8);
    let master = team_upgrade_cost(&units, &info, UnitType::Tank, ATTRIB_ATTACK, Opponent::Master as u8);
    let god = team_upgrade_cost(&units, &info, UnitType::Tank, ATTRIB_ATTACK, Opponent::God as u8);

    assert_eq!(clueless, (average * 5) / 4);
    assert_eq!(master, (average * 4) / 5);
    assert_eq!(god, (average * 2) / 3);

    // Human teams never get a difficulty discount.
    info.team_type = TeamKind::Player;
    assert_eq!(
        team_upgrade_cost(&units, &info, UnitType::Tank, ATTRIB_ATTACK, Opponent::God as u8),
        team_upgrade_cost(&units, &info, UnitType::Tank, ATTRIB_ATTACK, Opponent::Average as u8)
    );
}

#[test]
fn applied_upgrades_only_touch_current_values() {
    let mut units = TeamUnits::with_clan(TeamClan::Random);
    let tank = UnitType::Tank as usize;
    let base_attack = units.base_values[tank].attack;
    let current_before = units.current_values[tank].attack;

    apply_research_upgrades(&mut units, ResearchTopic::Attack, 1);

    assert_eq!(units.base_values[tank].attack, base_attack, "base is immutable");
    assert!(units.current_values[tank].attack >= current_before);
    assert_eq!(
        units.current_values[tank].attack,
        base_attack + base_attack / 10
    );
}

#[test]
fn research_new_turn_is_idempotent_without_allocation() {
    let mut units = TeamUnits::with_clan(TeamClan::Random);
    let mut info = TeamInfo::new();
    info.team_type = TeamKind::Player;

    // No allocation — no progress and no events.
    assert!(research_new_turn(&mut units, &mut info, TeamId::RED).is_empty());
    assert!(research_new_turn(&mut units, &mut info, TeamId::RED).is_empty());
    for topic in &info.research_topics {
        assert_eq!(topic.research_level, 0);
        assert_eq!(topic.turns_to_complete, 0);
    }
}

#[test]
fn update_research_progress_clears_the_timer_when_allocation_is_removed() {
    let mut topic = ResearchTopicInfo {
        research_level: 0,
        turns_to_complete: 0,
        allocation: 0,
    };

    update_research_progress(&mut topic, 16, 1);
    assert_eq!(topic.allocation, 1);
    assert!(topic.turns_to_complete > 0);

    // Negative allocation is how a research center is reassigned.
    update_research_progress(&mut topic, 16, -1);
    assert_eq!(topic.allocation, 0);
    assert_eq!(topic.turns_to_complete, 0, "progress timer cleared");
}
