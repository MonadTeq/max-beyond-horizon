use super::*;
use crate::enums::{Opponent, Surface, TeamClan};
use crate::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use crate::research::upgrade_amount_for_base;
use crate::team::{TeamId, TeamInfo, TeamKind, TeamUnits};
use crate::unit::UnitId;
use crate::unit_values::{ATTRIB_ATTACK, ATTRIB_SCAN};

fn land_state() -> GameState {
    let mut state = GameState::new();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            state.map.surface.set(x, y, Surface::LAND);
        }
    }

    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Computer;
    state.teams[TeamId::RED.0 as usize].team_clan = TeamClan::Musashi;
    state.team_units[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Musashi);
    state.ai.settings.opponent = Opponent::Average as u8;
    state
}

fn player() -> AiPlayer {
    AiPlayer::new(TeamId::RED)
}

fn set_turns(state: &mut GameState, topic: ResearchTopic, turns: u32) {
    state.teams[TeamId::RED.0 as usize].research_topics[topic as usize].turns_to_complete =
        turns;
}

fn spawn_research(
    state: &mut GameState,
    site: Point,
    topic: ResearchTopic,
    order_state: UnitOrderState,
) -> UnitId {
    let id = state
        .spawn_unit(UnitType::ResearchCentre, TeamId::RED, site, 0)
        .expect("research centre");

    let unit = state.unit_mut(id).expect("centre");
    unit.orders = UnitOrder::PowerOn;
    unit.state = order_state;
    unit.research_topic = topic;

    id
}

#[test]
fn determine_research_projects_picks_the_fastest_corrected_topic() {
    let mut state = land_state();

    // Shots: 1 turn * (5 - 0) = 5 — the fastest.
    // Range: 12 / 2 = 6; speed: 4 * 2 = 8; the rest: 12.
    for topic in ResearchTopic::ALL.iter().copied() {
        set_turns(&mut state, topic, 12);
    }
    set_turns(&mut state, ResearchTopic::Shots, 1);
    set_turns(&mut state, ResearchTopic::Speed, 4);

    let moved = spawn_research(
        &mut state,
        Point::new(10, 10),
        ResearchTopic::Speed,
        UnitOrderState::UnitReady,
    );
    // A center in `ORDER_STATE_INIT` is not reassigned.
    let initializing = spawn_research(
        &mut state,
        Point::new(14, 14),
        ResearchTopic::Armor,
        UnitOrderState::Init,
    );

    state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Speed as usize]
        .allocation = 1;

    determine_research_projects(&player(), &mut state);

    assert_eq!(
        state.unit(moved).map(|unit| unit.research_topic),
        Some(ResearchTopic::Shots)
    );
    assert_eq!(
        state.unit(initializing).map(|unit| unit.research_topic),
        Some(ResearchTopic::Armor),
            "an initializing center keeps its topic"
    );

    let allocation = state.research_allocation(TeamId::RED);
    assert_eq!(allocation[ResearchTopic::Shots as usize], 1);
    assert_eq!(allocation[ResearchTopic::Speed as usize], 0);
    assert_eq!(allocation[ResearchTopic::Armor as usize], 0);
}

#[test]
fn determine_research_projects_halves_range_and_multiplies_speed() {
    let mut state = land_state();

    // Attack 12; shots 3 * 5 = 15; range 12 / 2 = 6; armor 12; hits 12;
    // speed 2 * 2 = 4; scan 12 → speed wins only because of the doubling.
    for topic in ResearchTopic::ALL.iter().copied() {
        set_turns(&mut state, topic, 12);
    }
    set_turns(&mut state, ResearchTopic::Shots, 3);
    set_turns(&mut state, ResearchTopic::Speed, 2);

    let centre = spawn_research(
        &mut state,
        Point::new(10, 10),
        ResearchTopic::Attack,
        UnitOrderState::UnitReady,
    );

    determine_research_projects(&player(), &mut state);

    assert_eq!(
        state.unit(centre).map(|unit| unit.research_topic),
        Some(ResearchTopic::Speed)
    );
}

#[test]
fn choose_upgrade_prefers_the_cheapest_strategic_order() {
    let state = land_state();
    let player = player();
    let orders = [
        BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank),
        BuildOrder::new(ATTRIB_SCAN, UnitType::Scout),
    ];

    let (chosen, cost) = choose_upgrade(&player, &state, &orders, &[]);

    let costs: Vec<i32> = orders
        .iter()
        .map(|order| {
            crate::research::team_upgrade_cost(
                &state.team_units[TeamId::RED.0 as usize],
                &state.teams[TeamId::RED.0 as usize],
                order.unit_type,
                order.primary_attribute,
                state.ai.settings.opponent,
            )
        })
        .collect();
    let cheapest = if costs[0] <= costs[1] {
        (orders[0], costs[0])
    } else {
        (orders[1], costs[1])
    };

    assert_eq!(chosen, Some(cheapest.0));
    assert_eq!(i32::from(cost), cheapest.1);
}

#[test]
fn a_generic_order_must_be_twice_as_cheap_as_the_strategic_choice() {
    let state = land_state();
    let player = player();
    let strategic = [BuildOrder::new(ATTRIB_SCAN, UnitType::Scout)];
    let generic = [BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank)];

    let team_units = &state.team_units[TeamId::RED.0 as usize];
    let info = &state.teams[TeamId::RED.0 as usize];
    let strategic_cost = crate::research::team_upgrade_cost(
        team_units,
        info,
        UnitType::Scout,
        ATTRIB_SCAN,
        state.ai.settings.opponent,
    );
    let generic_cost = crate::research::team_upgrade_cost(
        team_units,
        info,
        UnitType::Tank,
        ATTRIB_ATTACK,
        state.ai.settings.opponent,
    );

    // In this setup the scan upgrade costs more than twice the attack
    // upgrade, so the generic order wins (and `factor` drops to 1).
    assert!(
        generic_cost * 2 < strategic_cost,
            "scenario must exercise the factor-2 rule: generic {generic_cost}, strategic {strategic_cost}"
    );

    let (chosen, cost) = choose_upgrade(&player, &state, &strategic, &generic);
    assert_eq!(chosen, Some(generic[0]));
    assert_eq!(i32::from(cost), generic_cost);
}

#[test]
fn a_generic_order_that_is_not_twice_as_cheap_loses() {
    let state = land_state();
    let player = player();
    let strategic = [BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank)];
    let generic = [BuildOrder::new(ATTRIB_SCAN, UnitType::Scout)];

    let team_units = &state.team_units[TeamId::RED.0 as usize];
    let info = &state.teams[TeamId::RED.0 as usize];
    let strategic_cost = crate::research::team_upgrade_cost(
        team_units,
        info,
        UnitType::Tank,
        ATTRIB_ATTACK,
        state.ai.settings.opponent,
    );
    let generic_cost = crate::research::team_upgrade_cost(
        team_units,
        info,
        UnitType::Scout,
        ATTRIB_SCAN,
        state.ai.settings.opponent,
    );
    assert!(
        generic_cost * 2 >= strategic_cost,
            "scenario must keep the strategic order: generic {generic_cost}, strategic {strategic_cost}"
    );

    let (chosen, cost) = choose_upgrade(&player, &state, &strategic, &generic);
    assert_eq!(chosen, Some(strategic[0]));
    assert_eq!(i32::from(cost), strategic_cost);
}

#[test]
fn hits_and_speed_at_uint8_max_are_not_upgraded() {
    let mut state = land_state();
    state.team_units[TeamId::RED.0 as usize]
        .current_unit_values_mut(UnitType::Tank as u16)
        .hits = u8::MAX as u16;

    let player = player();
    let orders = [BuildOrder::new(ATTRIB_HITS, UnitType::Tank)];

    let (chosen, cost) = choose_upgrade(&player, &state, &orders, &[]);
    assert_eq!(chosen, None);
    assert_eq!(cost, INVALID_UPGRADE_COST);
}

#[test]
fn upgrade_unit_type_uses_the_base_value_step() {
    let mut state = land_state();
    let mut player = player();
    player.build_order = Some(BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank));

    let index = TeamId::RED.0 as usize;
    let base = state.team_units[index].base_values[UnitType::Tank as usize];
    let before = base.get_attribute(ATTRIB_ATTACK);
    let step = upgrade_amount_for_base(before);

    upgrade_unit_type(&player, &mut state);

    assert_eq!(
        state.team_units[index].current_values[UnitType::Tank as usize]
            .get_attribute(ATTRIB_ATTACK),
        before + i32::from(step)
    );
    // The base values are untouched: only the team's current values change.
    assert_eq!(
        state.team_units[index].base_values[UnitType::Tank as usize]
            .get_attribute(ATTRIB_ATTACK),
        before
    );
}

#[test]
fn upgrade_unit_type_clamps_hits_to_uint16_max() {
    let mut state = land_state();
    let mut player = player();
    player.build_order = Some(BuildOrder::new(ATTRIB_HITS, UnitType::Tank));

    let index = TeamId::RED.0 as usize;
    let step = upgrade_amount_for_base(
        state.team_units[index].base_values[UnitType::Tank as usize].get_attribute(ATTRIB_HITS),
    );
    let near_max = u16::MAX - step + 1;
    state.team_units[index].current_values[UnitType::Tank as usize].hits = near_max;

    upgrade_unit_type(&player, &mut state);

    assert_eq!(
        state.team_units[index].current_values[UnitType::Tank as usize].hits,
        u16::MAX,
            "the step is cut to the remaining room below UINT16_MAX"
    );
}

#[test]
fn purchase_upgrades_spends_gold_and_improves_the_type() {
    let mut state = land_state();
    let mut player = player();

    // `ChooseUpgrade` is driven by the build orders of the turn.
    player.strategic_build_orders = vec![BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank)];

    let index = TeamId::RED.0 as usize;
    let info = TeamInfo {
        team_type: TeamKind::Computer,
        ..TeamInfo::new()
    };
    state.teams[index] = info;
    state.team_units[index] = TeamUnits::with_clan(TeamClan::Musashi);
    state.ai.settings.opponent = Opponent::Average as u8;

    let cost = crate::research::team_upgrade_cost(
        &state.team_units[index],
        &state.teams[index],
        UnitType::Tank,
        ATTRIB_ATTACK,
        state.ai.settings.opponent,
    );

    // Not enough gold: no purchase.
    state.team_units[index].set_gold(cost as u32 - 1);
    let before = state.team_units[index].current_values[UnitType::Tank as usize]
        .get_attribute(ATTRIB_ATTACK);
    purchase_upgrades(&mut player, &mut state);
    assert_eq!(
        state.team_units[index].current_values[UnitType::Tank as usize]
            .get_attribute(ATTRIB_ATTACK),
        before
    );
    assert_eq!(state.teams[index].stats_gold_spent_on_upgrades, 0);

    // Exactly one purchase fits: the loop re-chooses, the new cost is
    // larger than the remaining gold, and it stops.
    state.team_units[index].set_gold(cost as u32);
    purchase_upgrades(&mut player, &mut state);

    let after = state.team_units[index].current_values[UnitType::Tank as usize]
        .get_attribute(ATTRIB_ATTACK);
    assert!(after > before, "attack must be upgraded");
    assert_eq!(state.team_units[index].gold, 0);
    assert_eq!(state.teams[index].stats_gold_spent_on_upgrades, cost as u32);
    // The loop re-chooses and stops on the first cost it cannot pay; the
    // pending order stays planned (C++ leaves `build_order` set too).
    assert!(player.build_order.is_some());
    assert!(i32::from(player.upgrade_cost) > state.team_units[index].gold as i32);
}

#[test]
fn choose_initial_upgrades_spends_mission_gold() {
    let mut state = land_state();
    let mut player = player();
    player.strategy = crate::enums::AiStrategy::TankHorde;

    state.mission_supplies[TeamId::RED.0 as usize] =
        Some(crate::input::Packet12MissionSupply {
            team: TeamId::RED,
            round: 0,
            start_gold: 500,
            team_gold: 500,
            units: Vec::new(),
            gold_spent_on_upgrades: 0,
            starting_position: Point::ZERO,
            proximity_alert_ack: 0,
        });

    let index = TeamId::RED.0 as usize;

    choose_initial_upgrades(&mut player, &mut state, 500);

    let supplies = state.mission_supplies[index].as_ref().expect("supplies");
    assert!(supplies.team_gold < 500, "mission gold must be spent");
    assert!(state.teams[index].stats_gold_spent_on_upgrades > 0);
    let upgraded = (0..crate::unit_values::UNIT_COUNT).any(|unit_index| {
        state.team_units[index].current_values[unit_index]
            != state.team_units[index].base_values[unit_index]
    });
    assert!(upgraded, "at least one type must be upgraded");
}
