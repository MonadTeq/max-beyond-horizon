//! Phase 5 — AI research and purchased upgrades (area A.5).
//!
//! Scenarios: an AI base + a passive enemy base → rounds through the core turn
//! machine → the team gold the AI spends (`ChooseUpgrade` + the purchase loop)
//! and the research topic `DetermineResearchProjects` assigns to the powered
//! research centre.
//!
//! Oracles: `aiplayer.cpp:2503-2525` (the `BeginTurn` purchase loop),
//! `:4194-4245` (`ChooseUpgrade`), `:2075-2104` (`UpgradeUnitType`),
//! `:331-372` (`DetermineResearchProjects`).

mod support;

use max_core::enums::{AiStrategy, Opponent, ResearchTopic, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::unit_values::ATTRIB_ATTACK;
use support::phase5::{
    add_research_lab, green_base, play_rounds, red_base, spawn, strategy_state, ROUND_TICKS,
};

/// A builder constructing a tank makes `ProcessBuildOrders` keep the tank
/// order, so `ChooseUpgrade` buys tank attributes while the gold lasts.
#[test]
fn ai_spends_gold_on_upgrades_for_the_unit_under_construction() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    let red = red_base(&mut state, Point::new(8, 8), 800);
    green_base(&mut state, Point::new(32, 32));

    {
        let unit = state.unit_mut(red.constructor).expect("constructor");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit.build_list = vec![UnitType::Tank];
    }

    let index = TeamId::RED.0 as usize;
    let base_attack = state.team_units[index].current_values[UnitType::Tank as usize]
        .get_attribute(ATTRIB_ATTACK);

    let (_, over) = play_rounds(&mut state, 1, ROUND_TICKS);
    assert!(!over);

    let gold_after_first = state.team_units[index].gold;
    let spent_after_first = state.teams[index].stats_gold_spent_on_upgrades;

    assert!(
        spent_after_first > 0,
        "BeginTurn must spend gold on upgrades"
    );
    assert!(
        gold_after_first < 800,
        "the team treasury must be charged: {gold_after_first}"
    );
    assert!(
        state.team_units[index].current_values[UnitType::Tank as usize]
            .get_attribute(ATTRIB_ATTACK)
            > base_attack,
        "the tank attack upgrade must land in the current values"
    );
    assert!(
        state.ai.players[index].build_order.is_some(),
        "the turn keeps the pending upgrade order on the player"
    );

    // The remaining gold does not cover the next upgrade: no further spending.
    let (_, over) = play_rounds(&mut state, 2, ROUND_TICKS);
    assert!(!over);
    assert_eq!(
        state.teams[index].stats_gold_spent_on_upgrades, spent_after_first,
        "without gold the purchase loop must not spend more"
    );
}

/// Without gold `ChooseUpgrade` still picks an order, but the loop never buys
/// and the current values stay untouched.
#[test]
fn ai_without_gold_buys_nothing() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    let red = red_base(&mut state, Point::new(8, 8), 0);
    green_base(&mut state, Point::new(32, 32));

    {
        let unit = state.unit_mut(red.constructor).expect("constructor");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit.build_list = vec![UnitType::Tank];
    }

    let index = TeamId::RED.0 as usize;
    let before = state.team_units[index].current_values;

    let (_, over) = play_rounds(&mut state, 2, ROUND_TICKS);
    assert!(!over);

    assert_eq!(state.team_units[index].current_values, before);
    assert_eq!(state.teams[index].stats_gold_spent_on_upgrades, 0);
    assert_eq!(state.team_units[index].gold, 0);
}

/// `DetermineResearchProjects`: the powered centre is moved to the fastest
/// topic of the team, and the per-topic allocation follows it.
///
/// The centre's complex is powered by a fuel mine and three generators (see
/// [`add_research_lab`]); `ManageEconomy` recomputes the power balance at the
/// start of the turn, so a centre without that infrastructure is switched off
/// and never reaches the topic planner.
#[test]
fn ai_moves_the_research_centre_to_the_fastest_topic() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 800);
    green_base(&mut state, Point::new(32, 32));

    let centre = add_research_lab(&mut state, Point::new(14, 14));
    {
        let unit = state.unit_mut(centre).expect("centre");
        unit.research_topic = ResearchTopic::Speed;
    }

    // RANGE is halved (2 → 1), SHOTS is multiplied by the remaining levels
    // (40 × 5 = 200), everything else stays at 12 → RANGE is the fastest.
    for topic in ResearchTopic::ALL.iter().copied() {
        state.teams[TeamId::RED.0 as usize].research_topics[topic as usize].turns_to_complete = 12;
    }
    state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Range as usize]
        .turns_to_complete = 2;
    state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Shots as usize]
        .turns_to_complete = 40;

    let (_, over) = play_rounds(&mut state, 1, ROUND_TICKS);
    assert!(!over);

    assert_eq!(
        state.unit(centre).map(|unit| unit.orders),
        Some(UnitOrder::PowerOn),
        "the centre must be powered when the planner runs"
    );
    assert_eq!(
        state.unit(centre).map(|unit| unit.research_topic),
        Some(ResearchTopic::Range),
        "DetermineResearchProjects must move the centre to the fastest topic"
    );

    let allocation = state.research_allocation(TeamId::RED);
    assert_eq!(allocation[ResearchTopic::Range as usize], 1);
    assert_eq!(allocation[ResearchTopic::Speed as usize], 0);
}

/// Negative control: without power the economy turns the centre off, so the
/// planner skips it and the topic/allocation stay unchanged.
#[test]
fn an_unpowered_research_centre_keeps_its_topic() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 800);
    green_base(&mut state, Point::new(32, 32));

    let centre = spawn(
        &mut state,
        UnitType::ResearchCentre,
        TeamId::RED,
        Point::new(14, 14),
    );
    {
        let unit = state.unit_mut(centre).expect("centre");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::UnitReady;
        unit.research_topic = ResearchTopic::Speed;
    }

    state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Range as usize]
        .turns_to_complete = 2;

    let (_, over) = play_rounds(&mut state, 1, ROUND_TICKS);
    assert!(!over);

    assert_ne!(
        state.unit(centre).map(|unit| unit.orders),
        Some(UnitOrder::PowerOn),
        "a complex without power must switch the centre off"
    );
    assert_eq!(
        state.unit(centre).map(|unit| unit.research_topic),
        Some(ResearchTopic::Speed),
        "an unpowered centre is not reassigned"
    );
    assert_eq!(
        state.research_allocation(TeamId::RED)[ResearchTopic::Speed as usize],
        0
    );
    assert_eq!(
        state.research_allocation(TeamId::RED)[ResearchTopic::Range as usize],
        0
    );
}
