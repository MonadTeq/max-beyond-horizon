//! AI research topics and purchased unit upgrades.
//!
//! Oracle: `aiplayer.cpp` — `DetermineResearchProjects` (331-372),
//! `ChooseInitialUpgrades` (2056-2073), `UpgradeUnitType` (2075-2104) and
//! `ChooseUpgrade` (4194-4245); `teamunits.cpp` (`TeamUnits_GetUpgradeCost`,
//! `TeamUnits_UpgradeOffsetFactor`); `researchmenu.cpp`
//! (`ResearchMenu_UpdateResearchProgress`).
//!
//! # Research topics (`DetermineResearchProjects`)
//!
//! Once per AI turn the player picks the topic that completes soonest from the
//! team's current `turns_to_complete`, with the C++ corrections: a range level
//! counts half, a shots level is scaled by `5 - level % 5` (the topic gets
//! cheaper as its five-level cycle advances), a speed level counts double. The
//! attack topic is the initial best and is never corrected; the cost topic is
//! not considered. Every powered research center (`RESEARCH`, `ORDER_POWER_ON`,
//! not in `ORDER_STATE_INIT`) is moved to that topic through
//! `ResearchMenu_UpdateResearchProgress(-1/+1)`, which keeps the per-topic
//! center allocation and the projected completion in sync. The research rate
//! factors are the `RESEARCH_TOPIC_FACTORS` defaults, as everywhere else in
//! `max_core` (`ResearchMenu_UpdateResearchProgress` reads the same settings).
//!
//! # Purchased upgrades (`ChooseUpgrade` / `UpgradeUnitType`)
//!
//! `ChooseUpgrade` walks the strategic build orders (phase A.3) and picks the
//! cheapest attribute upgrade; a generic order may only replace the current
//! choice when it is at least twice as cheap, and after the first such
//! replacement a generic order needs to be strictly cheaper than the current
//! choice. Hits and speed upgrades are skipped once the attribute reached
//! `UINT8_MAX` (the C++ guard uses the narrower type).
//!
//! `BeginTurn` buys upgrades while the chosen cost fits the team gold, calling
//! `UpgradeUnitType` after each purchase: the step comes from
//! `TeamUnits_UpgradeOffsetFactor` (1/2/5/10 by the *base* value of the
//! attribute), hits and speed are clamped to `UINT16_MAX`, then the adjusted
//! `UnitValues` copy replaces the team's current values. The net packets
//! 09/10 of the C++ call sites are emitted by the transport layer, not by
//! `max_core`.
//!
//! Determinism: no RNG is used here; the result is a pure function of the team
//! research state, the unit values and the build orders of the turn.

use crate::enums::{ResearchTopic, UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::unit_values::{ATTRIB_HITS, ATTRIB_SPEED};

use super::build_orders::BuildOrder;
use super::strategy::AiPlayer;

/// `INT16_MAX` — the C++ `upgrade_cost` "nothing chosen" sentinel.
pub const INVALID_UPGRADE_COST: i16 = i16::MAX;

/// `DetermineResearchProjects` (`aiplayer.cpp:331`).
///
/// Picks the fastest research topic for the team and re-assigns every powered
/// research center to it. The C++ `Remote_SendNetPacket_09` is left to the
/// transport layer.
pub fn determine_research_projects(player: &AiPlayer, world: &mut GameState) {
    let team = player.player_team;
    let team_index = team.0 as usize;

    if team_index >= world.teams.len() {
        return;
    }

    // C++ starts with `RESEARCH_TOPIC_ATTACK` and applies no corrections to it;
    // the loop covers SHOTS..SCAN inclusive (`research_topic < RESEARCH_TOPIC_COST`).
    let best_topic = {
        let info = &world.teams[team_index];
        let mut best_topic = ResearchTopic::Attack;
        let mut best_turns = i64::from(info.research_topics[best_topic as usize].turns_to_complete);

        for topic in [
            ResearchTopic::Shots,
            ResearchTopic::Range,
            ResearchTopic::Armor,
            ResearchTopic::Hits,
            ResearchTopic::Speed,
            ResearchTopic::Scan,
        ] {
            let entry = &info.research_topics[topic as usize];
            let mut turns = i64::from(entry.turns_to_complete);

            if topic == ResearchTopic::Range {
                turns /= 2;
            }

            if topic == ResearchTopic::Shots {
                // `5 - (research_level % 5)`: the remaining shots levels.
                turns *= 5 - i64::from(entry.research_level % 5);
            }

            if topic == ResearchTopic::Speed {
                turns *= 2;
            }

            if turns < best_turns {
                best_topic = topic;
                best_turns = turns;
            }
        }

        best_topic
    };

    let stationary = world
        .unit_list(crate::unit::UnitListKind::Stationary)
        .to_vec();

    for id in stationary {
        let Some(unit) = world.unit(id).cloned() else {
            continue;
        };

        if unit.team != team
            || unit.unit_type != UnitType::ResearchCentre
            || unit.orders != UnitOrder::PowerOn
            || unit.state == UnitOrderState::Init
            || unit.research_topic == best_topic
        {
            continue;
        }

        let old_topic = unit.research_topic;
        world.update_research_progress(team, old_topic, -1);

        let mut unit = unit;
        unit.research_topic = best_topic;
        world.update_research_progress(team, best_topic, 1);
        world.write_unit(id, unit);
    }
}

/// `ChooseUpgrade` (`aiplayer.cpp:4194`) — the cheapest upgrade among the
/// strategic build orders, with the generic list as a fallback.
///
/// Returns the chosen order (`None` is C++ `build_order.unit_type == INVALID_ID`)
/// and its cost (`INT16_MAX` when nothing is chosen). The caller stores both on
/// [`AiPlayer`] (`build_order`/`upgrade_cost`), as C++ does.
pub fn choose_upgrade(
    player: &AiPlayer,
    world: &GameState,
    strategic_orders: &[BuildOrder],
    generic_orders: &[BuildOrder],
) -> (Option<BuildOrder>, i16) {
    let team_index = player.player_team.0 as usize;
    let team_units = &world.team_units[team_index];
    let info = &world.teams[team_index];
    let opponent = world.ai.settings.opponent;

    let mut chosen = None;
    let mut upgrade_cost = INVALID_UPGRADE_COST;

    for order in strategic_orders {
        if !upgrade_allowed(team_units, order) {
            continue;
        }

        let cost = crate::research::team_upgrade_cost(
            team_units,
            info,
            order.unit_type,
            order.primary_attribute,
            opponent,
        );

        if cost < i32::from(upgrade_cost) {
            chosen = Some(*order);
            upgrade_cost = cost as i16;
        }
    }

    // A generic order must be twice as cheap as the current choice; after the
    // first replacement (`factor = 1`) only a strictly cheaper one wins.
    let mut factor = 2;

    for order in generic_orders {
        if !upgrade_allowed(team_units, order) {
            continue;
        }

        let cost = crate::research::team_upgrade_cost(
            team_units,
            info,
            order.unit_type,
            order.primary_attribute,
            opponent,
        );

        if cost * factor < i32::from(upgrade_cost) {
            chosen = Some(*order);
            upgrade_cost = cost as i16;
            factor = 1;
        }
    }

    (chosen, upgrade_cost)
}

/// The hits/speed guard of `ChooseUpgrade`: these attributes stop at
/// `UINT8_MAX` in the C++ check (the `UnitValues` field itself is `uint16_t`).
fn upgrade_allowed(team_units: &crate::team::TeamUnits, order: &BuildOrder) -> bool {
    if order.primary_attribute != ATTRIB_HITS && order.primary_attribute != ATTRIB_SPEED {
        return true;
    }

    let current = team_units
        .current_unit_values(order.unit_type as u16)
        .get_attribute(order.primary_attribute);

    current != i32::from(u8::MAX)
}

/// `UpgradeUnitType` (`aiplayer.cpp:2075`) — apply the stored `build_order` to
/// the team's current unit values.
pub fn upgrade_unit_type(player: &AiPlayer, world: &mut GameState) {
    let Some(order) = player.build_order else {
        return;
    };

    let Some(team_units) = world.team_units.get_mut(player.player_team.0 as usize) else {
        return;
    };

    let unit_index = order.unit_type as usize;

    // `TeamUnits_UpgradeOffsetFactor` — the step is based on the *base* value.
    let base_value = team_units.base_values[unit_index].get_attribute(order.primary_attribute);
    let mut upgrade_level = i32::from(crate::research::upgrade_amount_for_base(base_value));

    if order.primary_attribute == ATTRIB_HITS || order.primary_attribute == ATTRIB_SPEED {
        let current_level =
            team_units.current_values[unit_index].get_attribute(order.primary_attribute);

        if current_level + upgrade_level > i32::from(u16::MAX) {
            upgrade_level = i32::from(u16::MAX) - current_level;
        }
    }

    // C++ copies the shared current values, bumps the mark, marks the copy as
    // free for serialization, adds the step and stores it back.
    let mut values = team_units.current_values[unit_index];
    values.update_version();
    values.mark_as_not_in_use();
    values.add_attribute(order.primary_attribute, upgrade_level);
    team_units.current_values[unit_index] = values;
}

/// The `BeginTurn` purchase loop (`aiplayer.cpp:2503-2522`).
///
/// Chooses an upgrade from the player's `strategic_build_orders` and
/// `generic_build_orders`, then buys while the team gold covers the cost. Each
/// purchase spends gold, adds to `stats_gold_spent_on_upgrades` and applies
/// [`upgrade_unit_type`]; the next `ChooseUpgrade` sees the updated values.
pub fn purchase_upgrades(player: &mut AiPlayer, world: &mut GameState) {
    let team_index = player.player_team.0 as usize;

    choose_and_store(player, world);

    let mut team_gold = world.team_units[team_index].gold as i32;

    while player.build_order.is_some() && i32::from(player.upgrade_cost) <= team_gold {
        team_gold -= i32::from(player.upgrade_cost);
        world.team_units[team_index].set_gold(team_gold as u32);
        world.teams[team_index].stats_gold_spent_on_upgrades = world.teams[team_index]
            .stats_gold_spent_on_upgrades
            .wrapping_add(u32::from(player.upgrade_cost as u16));

        upgrade_unit_type(player, world);

        choose_and_store(player, world);
    }
}

/// `ChooseInitialUpgrades` (`aiplayer.cpp:2056`) — the starting-loadout
/// variant (`RollTeamMissionSupplies`, phase A.3+): the strategic orders are
/// recomputed in `mode = true` after every purchase and the gold comes from
/// `TeamMissionSupplies::team_gold`, not from the team treasury.
pub fn choose_initial_upgrades(player: &mut AiPlayer, world: &mut GameState, team_gold: i32) {
    let team_index = player.player_team.0 as usize;
    let mut team_gold = team_gold;

    let strategic = player.choose_strategic_build_orders(world, true);
    let (chosen, cost) = choose_upgrade(player, world, &strategic, &[]);
    player.build_order = chosen;
    player.upgrade_cost = cost;

    while player.build_order.is_some() && i32::from(player.upgrade_cost) <= team_gold {
        team_gold -= i32::from(player.upgrade_cost);

        if let Some(supplies) = world.mission_supplies[team_index].as_mut() {
            supplies.team_gold = supplies.team_gold.wrapping_sub(player.upgrade_cost as u16);
        }

        world.teams[team_index].stats_gold_spent_on_upgrades = world.teams[team_index]
            .stats_gold_spent_on_upgrades
            .wrapping_add(u32::from(player.upgrade_cost as u16));

        upgrade_unit_type(player, world);

        let strategic = player.choose_strategic_build_orders(world, true);
        let (chosen, cost) = choose_upgrade(player, world, &strategic, &[]);
        player.build_order = chosen;
        player.upgrade_cost = cost;
    }
}

/// Runs `ChooseUpgrade` over the orders of the current turn and stores the
/// result on the player (`aiplayer.cpp:2506` / `2521`).
fn choose_and_store(player: &mut AiPlayer, world: &GameState) {
    let (chosen, cost) = choose_upgrade(
        player,
        world,
        &player.strategic_build_orders,
        &player.generic_build_orders,
    );

    player.build_order = chosen;
    player.upgrade_cost = cost;
}

#[cfg(test)]
mod tests {
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
}
