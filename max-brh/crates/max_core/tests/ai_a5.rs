//! Phase A.5 integration tests: `BeginTurn` drives research topics and
//! purchased upgrades.
//!
//! Oracle: `aiplayer.cpp:2503-2525` — `ChooseUpgrade`, the purchase loop and
//! `DetermineResearchProjects` run inside `AiPlayer::BeginTurn`. The tests call
//! `ai_begin_turn` directly (no ticks): only the planning part of the turn is
//! under test.

use max_core::enums::{
    Opponent, ResearchTopic, Surface, TeamClan, UnitOrder, UnitOrderState, UnitType,
};
use max_core::geo::Point;
use max_core::map::Map;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit_values::ATTRIB_ATTACK;
use max_core::universe::GameUniverse;

/// Land 24×24 in the top-left corner, water elsewhere.
fn small_land_map() -> Map {
    let mut map = Map::new();

    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            let surface = if x < 24 && y < 24 {
                Surface::LAND
            } else {
                Surface::WATER
            };
            map.surface.set(x, y, surface);
        }
    }

    map
}

fn computer_state() -> GameUniverse {
    let mut state = GameUniverse::with_setup(
        small_land_map(),
        [
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    state.state_mut().teams[TeamId::RED.0 as usize].team_type = TeamKind::Computer;
    state.state_mut().teams[TeamId::RED.0 as usize].team_clan = TeamClan::Musashi;
    state.state_mut().team_units[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Musashi);
    state.state_mut().ai.settings.opponent = Opponent::Average as u8;
    max_core::ai_init(state.state_mut());
    state.state_mut().ai.players[TeamId::RED.0 as usize].strategy =
        max_core::enums::AiStrategy::TankHorde;

    state
}

#[test]
fn begin_turn_moves_powered_research_centres_to_the_fastest_topic() {
    let mut state = computer_state();

    // The attack topic is the initial best; shots is five times faster.
    for topic in ResearchTopic::ALL.iter().copied() {
        state.state_mut().teams[TeamId::RED.0 as usize].research_topics[topic as usize]
            .turns_to_complete = 12;
    }
    state.state_mut().teams[TeamId::RED.0 as usize].research_topics
        [ResearchTopic::Shots as usize]
        .turns_to_complete = 1;

    let centre = state
        .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(10, 10), 0)
        .expect("research centre");
    {
        let unit = state.unit_mut(centre).expect("centre");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::UnitReady;
        unit.research_topic = ResearchTopic::Speed;
    }

    max_core::ai_begin_turn(state.state_mut(), TeamId::RED);

    assert_eq!(
        state
            .snapshot()
            .unit(centre)
            .map(|unit| unit.research_topic),
        Some(ResearchTopic::Shots),
        "DetermineResearchProjects must reassign the powered centre"
    );

    let allocation = state.state().research_allocation(TeamId::RED);
    assert_eq!(allocation[ResearchTopic::Shots as usize], 1);
    assert_eq!(allocation[ResearchTopic::Speed as usize], 0);
}

#[test]
fn begin_turn_buys_upgrades_for_types_under_construction() {
    let mut state = computer_state();
    state.state_mut().team_units[TeamId::RED.0 as usize].set_gold(1000);

    // A builder constructing a tank keeps the tank orders in
    // `ProcessBuildOrders`, so `ChooseUpgrade` sees them.
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    {
        let unit = state.unit_mut(constructor).expect("constructor");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit.build_list = vec![UnitType::Tank];
    }

    let index = TeamId::RED.0 as usize;
    let base_attack = state.state().team_units[index].base_values[UnitType::Tank as usize]
        .get_attribute(ATTRIB_ATTACK);

    max_core::ai_begin_turn(state.state_mut(), TeamId::RED);

    assert!(
        state.state().teams[index].stats_gold_spent_on_upgrades > 0,
        "BeginTurn must spend gold on upgrades"
    );
    assert!(
        state.state().team_units[index].gold < 1000,
        "the team treasury must be charged"
    );
    assert!(
        state.state().team_units[index].current_values[UnitType::Tank as usize]
            .get_attribute(ATTRIB_ATTACK)
            > base_attack,
        "the chosen tank attack upgrade must land in the current values"
    );
    // The pending order is kept on the player, as in C++.
    assert!(state.state().ai.players[index].upgrade_cost > 0);
}

#[test]
fn begin_turn_without_gold_and_orders_changes_nothing() {
    let mut state = computer_state();
    state.state_mut().team_units[TeamId::RED.0 as usize].set_gold(0);

    let index = TeamId::RED.0 as usize;
    let before = state.state().team_units[index].current_values;

    max_core::ai_begin_turn(state.state_mut(), TeamId::RED);

    assert_eq!(
        state.state().team_units[index].current_values,
        before,
        "no build orders and no gold: no purchased upgrades"
    );
    assert_eq!(state.state().teams[index].stats_gold_spent_on_upgrades, 0);
}
