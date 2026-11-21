//! P6: исследования и покупные апгрейды против C++-эталонов.
//!
//! `data/upgrade_vectors.txt` сгенерирован `tools/upgrade_vectors` из
//! `std::pow`-формул `upgradecontrol.cpp` / `researchmenu.cpp` / `teamunits.cpp`
//! (см. README инструмента). Здесь значения сравниваются бит-в-бит: наша
//! реализация считает `x^7.5` целочисленно, без `f64`.

use max_core::enums::{ResearchTopic, TeamClan, UnitType};
use max_core::research::{
    apply_research_upgrades, research_attribute_values, research_factor, research_new_turn,
    research_turns_to_complete, team_upgrade_cost, team_upgrade_cost_step, update_research_progress,
    upgrade_amount_for_base, upgrade_control_cost, upgrade_cost_step_with, UpgradeQuote,
    RESEARCH_TOPIC_FACTORS,
};
use max_core::team::{ResearchTopicInfo, TeamId, TeamInfo, TeamKind, TeamUnits};
use max_core::unit_values::{
    ATTRIB_ATTACK, ATTRIB_HITS, ATTRIB_SPEED, ATTRIB_TURNS,
};
use max_core::GameState;

const VECTORS: &str = include_str!("../../../tools/upgrade_vectors/vectors.txt");

#[test]
fn cost_vectors_match_cpp_bit_for_bit() {
    let mut checked = 0usize;
    for line in VECTORS.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("cost") {
            continue;
        }
        let id: u8 = parts.next().unwrap().parse().unwrap();
        let current: u16 = parts.next().unwrap().parse().unwrap();
        let factor: u16 = parts.next().unwrap().parse().unwrap();
        let base: u16 = parts.next().unwrap().parse().unwrap();
        let expected: i32 = parts.next().unwrap().parse().unwrap();

        assert_eq!(
            upgrade_control_cost(id, current, factor, base),
            expected,
            "cost id={id} current={current} factor={factor} base={base}"
        );
        checked += 1;
    }
    assert!(checked > 50_000, "проверено {checked} cost-векторов");
}

#[test]
fn research_turn_vectors_match_cpp() {
    let mut checked = 0usize;
    for line in VECTORS.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("turns_default") => {
                let topic: usize = parts.next().unwrap().parse().unwrap();
                let level: u32 = parts.next().unwrap().parse().unwrap();
                let expected: i32 = parts.next().unwrap().parse().unwrap();
                assert_eq!(
                    research_turns_to_complete(level, RESEARCH_TOPIC_FACTORS[topic]),
                    expected,
                    "turns_default topic={topic} level={level}"
                );
                checked += 1;
            }
            Some("research") => {
                let factor: i32 = parts.next().unwrap().parse().unwrap();
                let level: u32 = parts.next().unwrap().parse().unwrap();
                let expected: i32 = parts.next().unwrap().parse().unwrap();
                assert_eq!(
                    research_turns_to_complete(level, factor),
                    expected,
                    "research factor={factor} level={level}"
                );
                checked += 1;
            }
            _ => {}
        }
    }
    assert!(checked >= 656);
}

#[test]
fn research_factor_vectors_match_cpp() {
    let mut checked = 0usize;
    for line in VECTORS.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("factor") {
            continue;
        }
        let value: i32 = parts.next().unwrap().parse().unwrap();
        let level: u32 = parts.next().unwrap().parse().unwrap();
        let expected: i32 = parts.next().unwrap().parse().unwrap();
        assert_eq!(research_factor(value, level), expected);
        checked += 1;
    }
    assert!(checked > 3000);
}

#[test]
fn research_apply_vectors_match_cpp() {
    let mut checked = 0usize;
    for line in VECTORS.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("apply") {
            continue;
        }
        let topic_index: u8 = parts.next().unwrap().parse().unwrap();
        let base_value: i32 = parts.next().unwrap().parse().unwrap();
        let level: u32 = parts.next().unwrap().parse().unwrap();
        let expected_old: i32 = parts.next().unwrap().parse().unwrap();
        let expected_new: i32 = parts.next().unwrap().parse().unwrap();
        let topic = ResearchTopic::try_from(topic_index).unwrap();
        let (old_value, new_value) = research_attribute_values(topic, base_value, level);
        assert_eq!(
            (old_value, new_value),
            (expected_old, expected_new),
            "apply topic={topic_index} base={base_value} level={level}"
        );
        checked += 1;
    }
    assert!(checked > 20_000);
}

#[test]
fn upgrade_amount_vectors_match_cpp() {
    for line in VECTORS.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("offset") {
            continue;
        }
        let value: i32 = parts.next().unwrap().parse().unwrap();
        let expected: u16 = parts.next().unwrap().parse().unwrap();
        assert_eq!(upgrade_amount_for_base(value), expected, "value={value}");
    }
}

#[test]
fn team_upgrade_cost_vectors_match_cpp() {
    let mut checked = 0usize;
    for line in VECTORS.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("cost_team") {
            continue;
        }
        let id: u8 = parts.next().unwrap().parse().unwrap();
        let base: u16 = parts.next().unwrap().parse().unwrap();
        let current: u16 = parts.next().unwrap().parse().unwrap();
        let factor: u16 = parts.next().unwrap().parse().unwrap();
        let step: u16 = parts.next().unwrap().parse().unwrap();
        let expected: i32 = parts.next().unwrap().parse().unwrap();
        assert_eq!(
            team_upgrade_cost_step(id, current, factor, base, step),            expected,
            "cost_team id={id} base={base} current={current} factor={factor} step={step}"
        );
        checked += 1;
    }
    assert!(checked > 3000);
}

// ---------------------------------------------------------------------------
// Игровые сценарии
// ---------------------------------------------------------------------------

fn team_units() -> TeamUnits {
    TeamUnits::with_clan(TeamClan::Random)
}

#[test]
fn a_completed_level_applies_attack_upgrade_to_current_values() {
    let mut units = team_units();
    let tank = UnitType::Tank as usize;
    let base_attack = units.base_values[tank].attack;

    let mut info = TeamInfo::new();
    info.team_type = TeamKind::Player;
    info.research_topics[ResearchTopic::Attack as usize] = ResearchTopicInfo {
        research_level: 0,
        turns_to_complete: 1,
        allocation: 1,
    };

    let events = research_new_turn(&mut units, &mut info, TeamId::RED);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, ResearchTopic::Attack);
    assert_eq!(events[0].level, 1);
    assert_eq!(info.research_topics[ResearchTopic::Attack as usize].research_level, 1);

    // C++ `ResearchMenu_ApplyUpgrades`: delta = (level * base) / 10 - 0.
    let expected_delta = (base_attack / 10) as u16;
    assert_eq!(
        units.current_values[tank].attack,
        base_attack + expected_delta
    );
    // База не меняется — апгрейд идёт в current.
    assert_eq!(units.base_values[tank].attack, base_attack);
}

#[test]
fn second_level_keeps_the_cumulative_delta() {
    let mut units = team_units();
    let tank = UnitType::Tank as usize;
    let base_attack = units.base_values[tank].attack;

    let mut info = TeamInfo::new();
    info.research_topics[ResearchTopic::Attack as usize] = ResearchTopicInfo {
        research_level: 0,
        turns_to_complete: 1,
        allocation: 1,
    };
    research_new_turn(&mut units, &mut info, TeamId::RED);

    // Следующий уровень инициализируется как в `ManageEconomy`.
    update_research_progress(
        &mut info.research_topics[ResearchTopic::Attack as usize],
        RESEARCH_TOPIC_FACTORS[ResearchTopic::Attack as usize],
        0,
    );
    assert!(info.research_topics[ResearchTopic::Attack as usize].turns_to_complete > 0);

    info.research_topics[ResearchTopic::Attack as usize].turns_to_complete = 1;
    let events = research_new_turn(&mut units, &mut info, TeamId::RED);
    assert_eq!(events[0].level, 2);
    let expected = base_attack + (2 * base_attack) / 10;
    assert_eq!(units.current_values[tank].attack, expected);
}

#[test]
fn cost_topic_lowers_build_turns_and_never_below_one() {
    let mut units = team_units();
    let tank = UnitType::Tank as usize;
    let base_turns = units.base_values[tank].turns;
    apply_research_upgrades(&mut units, ResearchTopic::Cost, 1);
    let old_value = (((1 + 9) / 2 + i32::from(base_turns) * 10) / (1 + 9)).max(1);
    let new_value = (((1 + 10) / 2 + i32::from(base_turns) * 10) / (1 + 10)).max(1);
    let delta = old_value - new_value;
    assert_eq!(
        i32::from(units.current_values[tank].turns),
        i32::from(base_turns) - delta
    );
    assert!(units.current_values[tank].turns >= 1);
}

#[test]
fn upgrade_quote_respects_gold_step_and_cost_limit() {
    let base = 12u16;
    let mut quote = UpgradeQuote::new(ATTRIB_ATTACK, base, base, 0);
    assert_eq!(quote.amount, 2);
    let cost = quote.cost();
    assert!(cost >= 1 && cost < 800);

    let mut gold = 1000u32;
    assert!(quote.increase(&mut gold));
    assert_eq!(quote.actual, base + 2);
    assert_eq!(gold, 1000 - cost as u32);

    // Возврат назад — та же цена (C++ `Decrease`).
    assert!(quote.decrease(&mut gold));
    assert_eq!(quote.actual, base);
    assert_eq!(gold, 1000);

    // На базовом значении дальше вниз нельзя.
    assert!(!quote.can_decrease());
}

#[test]
fn hits_and_speed_stop_at_u16_max() {
    let mut quote = UpgradeQuote::new(ATTRIB_HITS, 1, u16::MAX - 1, 0);
    let mut gold = 1_000_000;
    // Шаг 1, потолок u16::MAX: одна покупка — на границу, вторая невозможна.
    assert!(quote.increase(&mut gold));
    assert_eq!(quote.actual, u16::MAX);
    assert!(!quote.can_increase(gold));

    let speed = UpgradeQuote::new(ATTRIB_SPEED, 1, u16::MAX, 0);
    assert!(!speed.can_increase(1_000_000));
}

#[test]
fn upgrade_costs_depend_on_research_level() {
    let units = team_units();
    let mut info = TeamInfo::new();
    info.team_type = TeamKind::Player;
    let tank = UnitType::Tank;

    let base_cost = team_upgrade_cost(&units, &info, tank, ATTRIB_ATTACK, 0);
    info.research_topics[ResearchTopic::Attack as usize].research_level = 5;
    let researched_cost = team_upgrade_cost(&units, &info, tank, ATTRIB_ATTACK, 0);
    assert!(
        researched_cost < base_cost,
        "исследования снижают цену шага (factor вычитается): {base_cost} -> {researched_cost}"
    );
}

#[test]
fn turns_attribute_cost_uses_the_inverse_formula() {
    // `TeamUnits_GetUpgradeCost(ATTRIB_TURNS)` уменьшает current на шаг;
    // стоимость — разница level-cost, минимум 1.
    let cost = upgrade_cost_step_with(ATTRIB_TURNS, 10, 1, 10, 2);
    assert!(cost >= 1);
    let over = upgrade_control_cost(ATTRIB_TURNS, 10, 25, 10);
    assert_eq!(over, i32::MIN, "x86-приведение переполнения");
}

#[test]
fn research_state_is_deterministic() {
    fn run() -> Vec<u8> {
        let mut state = GameState::new();
        state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
        state.team_units[TeamId::RED.0 as usize] = team_units();
        state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Attack as usize] =
            ResearchTopicInfo {
                research_level: 0,
                turns_to_complete: 2,
                allocation: 1,
            };
        state.advance_research(TeamId::RED);
        state.advance_research(TeamId::RED);
        state.snapshot_bytes()
    }

    assert_eq!(run(), run(), "два прогона исследований совпадают");
}

#[test]
fn allocation_is_stored_in_team_info() {
    let mut state = GameState::new();
    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    state.update_research_progress(TeamId::RED, ResearchTopic::Range, 1);
    let info = &state.teams[TeamId::RED.0 as usize].research_topics
        [ResearchTopic::Range as usize];
    assert_eq!(info.allocation, 1);
    assert!(info.turns_to_complete > 0, "прогресс инициализирован");
    assert_eq!(state.research_allocation(TeamId::RED)[2], 1);
}

#[test]
fn research_center_allocation_reassigns_and_powers_off() {
    use max_core::enums::UnitOrder;

    let mut state = GameState::new();
    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    state.team_units[TeamId::RED.0 as usize] = team_units();
    let id = state
        .spawn_unit(
            UnitType::ResearchCentre,
            TeamId::RED,
            max_core::geo::Point::new(10, 10),
            0,
        )
        .expect("research center");
    let mut unit = state.unit(id).cloned().unwrap();
    unit.orders = UnitOrder::PowerOn;
    unit.state = max_core::enums::UnitOrderState::ExecutingOrder;
    unit.research_topic = ResearchTopic::Range;
    state.write_unit(id, unit);
    state.update_research_progress(TeamId::RED, ResearchTopic::Range, 1);
    assert_eq!(state.research_allocation(TeamId::RED)[2], 1);

    // Переводим центр на Attack: Range allocation -1, Attack +1.
    let mut target = [0u32; 8];
    target[ResearchTopic::Attack as usize] = 1;
    state.set_research_allocation(TeamId::RED, target);

    let unit = state.unit(id).unwrap();
    assert_eq!(unit.research_topic, ResearchTopic::Attack);
    assert_eq!(state.research_allocation(TeamId::RED)[2], 0);
    assert_eq!(state.research_allocation(TeamId::RED)[0], 1);

    // Цель без мест — центр выключается.
    state.set_research_allocation(TeamId::RED, [0u32; 8]);
    assert_eq!(state.unit(id).unwrap().orders, UnitOrder::PowerOff);
}
