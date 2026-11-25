use super::*;
use crate::enums::{FileType, TeamClan};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::TeamUnits;
use crate::unit::UnitId;

fn empty_match() -> GameState {
    let mut state = GameState::new();
    state.game_state = crate::enums::GameStateType::InGame;
    state.mission_category = MissionCategory::Custom;
    state.mission_index = 0;
    state.victory_type = VictoryType::Duration;
    state.victory_limit = 50;
    // A duel: red — human, green — computer, blue/gray do not play.
    // The network branch is disabled to check the local semantics of
    // `menu_check_end_game_conditions` (marking Eliminated).
    state.player_team = TeamId::RED;
    state.network_game = false;

    for team in 0..PLAYER_TEAM_COUNT {
        state.teams[team].team_type = if team == 0 {
            TeamKind::Player
        } else if team == 1 {
            TeamKind::Computer
        } else {
            TeamKind::None
        };
        state.team_units[team] = TeamUnits::with_clan(TeamClan::Random);
    }

    state
}

fn spawn(state: &mut GameState, unit_type: UnitType, team: TeamId, x: i16, y: i16) {
    let flags = crate::access::unit_type_flags(unit_type);
    state
        .spawn_unit(unit_type, team, Point::new(x, y), flags)
        .expect("unit spawns");
}

#[test]
fn team_without_units_is_lost() {
    let state = empty_match();
    assert_eq!(
        default_win_loss_conditions(&state, TeamId::RED),
        WinLossState::Lost
    );
}

#[test]
fn army_with_ammo_is_alive_and_gets_generic() {
    let mut state = empty_match();
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    assert_eq!(
        default_win_loss_conditions(&state, TeamId::RED),
        WinLossState::Generic
    );
    // A submarine and a saboteur are not counted as military power (save.md, corner case).
    let mut stealth = empty_match();
    spawn(&mut stealth, UnitType::Submarine, TeamId::RED, 20, 20);
    spawn(&mut stealth, UnitType::Infiltrator, TeamId::RED, 21, 20);
    assert_eq!(
        default_win_loss_conditions(&stealth, TeamId::RED),
        WinLossState::Lost
    );
}

#[test]
fn production_and_construction_infrastructure_keep_team_alive() {
    let mut state = empty_match();
    spawn(&mut state, UnitType::LightVehiclePlant, TeamId::RED, 20, 20);
    // A factory without fuel/materials and without a mine is not production.
    assert!(!has_production_infrastructure(&state, TeamId::RED));

    spawn(&mut state, UnitType::MiningStation, TeamId::RED, 22, 20);
    assert!(!has_production_infrastructure(&state, TeamId::RED));
    // The generator provides power (power_demand = 2 for the mine).
    spawn(&mut state, UnitType::PowerGenerator, TeamId::RED, 24, 20);
    assert!(!has_production_infrastructure(&state, TeamId::RED));
    // Power is enough (POWGEN gives 1) — no, demand 2 → another one is needed.
    spawn(&mut state, UnitType::PowerGenerator, TeamId::RED, 26, 20);
    assert!(has_production_infrastructure(&state, TeamId::RED));
    assert_eq!(
        default_win_loss_conditions(&state, TeamId::RED),
        WinLossState::Generic
    );

    let mut construction = empty_match();
    spawn(
        &mut construction,
        UnitType::Constructor,
        TeamId::RED,
        20,
        20,
    );
    spawn(&mut construction, UnitType::Engineer, TeamId::RED, 21, 20);
    // 38 materials (without a mine and without own generation) — no resources.
    // Spawn sets `storage` from the attribute table, so we zero it explicitly.
    let ids = construction.unit_ids_in_order();
    for id in ids {
        if let Some(mut unit) = construction.unit(id).cloned() {
            unit.storage = 0;
            construction.write_unit(id, unit);
        }
    }
    assert_eq!(has_materials(&construction, TeamId::RED, CargoType::Raw), 0);
    assert!(!has_construction_infrastructure(&construction, TeamId::RED));
    let first = construction.unit_ids_in_order()[0];
    if let Some(mut unit) = construction.unit(first).cloned() {
        unit.storage = 38;
        construction.write_unit(first, unit);
    }
    assert!(has_construction_infrastructure(&construction, TeamId::RED));
}

#[test]
fn last_team_standing_wins_generic_game() {
    let mut state = empty_match();
    state.player_team = TeamId::RED;
    // Red is alive with a tank, green is empty (and therefore LOST).
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    let status = evaluate_status(&state, state.turn_counter);
    assert_eq!(status.teams_total, 2);
    assert_eq!(status.teams_in_play, 1);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Generic
    );
    assert_eq!(
        status.team_status[TeamId::GREEN.0 as usize],
        WinLossState::Lost
    );
    assert_eq!(status.team_rank[TeamId::RED.0 as usize], 1);
    assert!(check_end_game_conditions(&mut state));
}

#[test]
fn score_limit_ends_the_game() {
    let mut state = empty_match();
    state.player_team = TeamId::RED;
    state.victory_type = VictoryType::Score;
    state.victory_limit = 25;
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
    assert!(!check_end_game_conditions(&mut state));

    state.teams[TeamId::GREEN.0 as usize].team_points = 25;
    assert!(check_end_game_conditions(&mut state));
}

#[test]
fn duration_limit_ends_the_game() {
    let mut state = empty_match();
    state.player_team = TeamId::RED;
    state.victory_type = VictoryType::Duration;
    state.victory_limit = 30;
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
    state.turn_counter = 29;
    assert!(!check_end_game_conditions(&mut state));
    state.turn_counter = 30;
    assert!(check_end_game_conditions(&mut state));
}

#[test]
fn ranks_break_ties_by_team_index_then_assets() {
    let mut state = empty_match();
    state.player_team = TeamId::RED;
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
    let status = evaluate_status(&state, 0);
    // Equal points and equal bases → the lower index is higher (RED=0).
    assert_eq!(status.team_rank[TeamId::RED.0 as usize], 1);
    assert_eq!(status.team_rank[TeamId::GREEN.0 as usize], 2);

    // Green has more points → green is higher.
    state.teams[TeamId::GREEN.0 as usize].team_points = 10;
    let status = evaluate_status(&state, 0);
    assert_eq!(status.team_rank[TeamId::GREEN.0 as usize], 1);
    assert_eq!(status.team_rank[TeamId::RED.0 as usize], 2);
}

#[test]
fn mission_18_wins_when_green_has_no_research_center() {
    let mut state = empty_match();
    state.player_team = TeamId::RED;
    state.mission_category = MissionCategory::Scenario;
    state.mission_index = 18;
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
    spawn(&mut state, UnitType::ResearchCentre, TeamId::GREEN, 42, 40);

    let status = evaluate_status(&state, 0);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Pending
    );
    assert_eq!(
        status.team_status[TeamId::GREEN.0 as usize],
        WinLossState::Generic
    );

    // Destroy green's research center.
    let research = state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::ResearchCentre)
        })
        .expect("green research center");
    state.destroy_unit(research);

    let status = evaluate_status(&state, 0);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Won
    );
    assert_eq!(
        status.team_status[TeamId::GREEN.0 as usize],
        WinLossState::Generic
    );
}

#[test]
fn campaign_1_loses_when_green_loses_research_center() {
    let mut state = empty_match();
    state.player_team = TeamId::RED;
    state.mission_category = MissionCategory::Campaign;
    state.mission_index = 1;
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
    spawn(&mut state, UnitType::ResearchCentre, TeamId::GREEN, 42, 40);
    spawn(&mut state, UnitType::EcoSphere, TeamId::GREEN, 44, 40);

    assert_eq!(
        mission_win_loss(&state, MissionCategory::Campaign, 1, TeamId::RED),
        Some(WinLossState::Pending)
    );

    let research = state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::ResearchCentre)
        })
        .expect("green research center");
    state.destroy_unit(research);

    assert_eq!(
        mission_win_loss(&state, MissionCategory::Campaign, 1, TeamId::RED),
        Some(WinLossState::Lost)
    );

    // Green, left without a research center, also gets GENERIC
    // (it is not RED and is alive).
    assert_eq!(
        mission_win_loss(&state, MissionCategory::Campaign, 1, TeamId::GREEN),
        Some(WinLossState::Generic)
    );
}

#[test]
fn training_15_wins_on_armor_research() {
    let mut state = empty_match();
    state.mission_category = MissionCategory::Training;
    state.mission_index = 15;
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::ResearchCentre, TeamId::RED, 22, 20);

    assert_eq!(
        mission_win_loss(&state, MissionCategory::Training, 15, TeamId::RED),
        Some(WinLossState::Pending)
    );

    state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Armor as usize]
        .research_level = 1;
    assert_eq!(
        mission_win_loss(&state, MissionCategory::Training, 15, TeamId::RED),
        Some(WinLossState::Won)
    );
}

#[test]
fn mission_categories_follow_file_types() {
    assert_eq!(
        MissionCategory::from_file_type(FileType::Custom),
        MissionCategory::Custom
    );
    assert_eq!(
        MissionCategory::from_file_type(FileType::Tutorial),
        MissionCategory::Training
    );
    assert_eq!(
        MissionCategory::from_file_type(FileType::Campaign),
        MissionCategory::Campaign
    );
    assert_eq!(
        MissionCategory::from_file_type(FileType::Scenario),
        MissionCategory::Scenario
    );
    assert_eq!(
        MissionCategory::from_file_type(FileType::MultiScenario),
        MissionCategory::MultiPlayerScenario
    );
    assert_eq!(MissionCategory::Training as i32, 1);
}

#[test]
fn cargo_types_match_sc_units_table() {
    assert_eq!(cargo_type_of(UnitType::MiningStation), CargoType::Raw);
    assert_eq!(cargo_type_of(UnitType::FuelStorage), CargoType::Fuel);
    assert_eq!(cargo_type_of(UnitType::GoldTruck), CargoType::Gold);
    assert_eq!(cargo_type_of(UnitType::Barracks), CargoType::Land);
    assert_eq!(cargo_type_of(UnitType::Hangar), CargoType::Air);
    assert_eq!(cargo_type_of(UnitType::Dock), CargoType::Sea);
    assert_eq!(cargo_type_of(UnitType::Tank), CargoType::None);
}

#[test]
fn build_cost_and_consumption_rates_match_cpp() {
    let mut unit = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId::NO_ID);
    unit.base_values.turns = 5;
    // A mobile unit (not STATIONARY): the LANDPLT rate = 3 × turns.
    assert_eq!(get_normal_rate_build_cost(&unit), 15);

    let mut stationary = UnitInfo::new(UnitType::MiningStation, TeamId::RED, UnitId::NO_ID);
    stationary.base_values.turns = 6;
    stationary.flags |= unit_flags::STATIONARY;
    assert_eq!(get_normal_rate_build_cost(&stationary), 12);

    let mut infantry = UnitInfo::new(UnitType::Infantry, TeamId::RED, UnitId::NO_ID);
    infantry.base_values.turns = 4;
    assert_eq!(get_normal_rate_build_cost(&infantry), 4);

    assert_eq!(cargo_power_consumption_rate(UnitType::PowerStation), -6);
    assert_eq!(cargo_power_consumption_rate(UnitType::MiningStation), 1);
    assert_eq!(
        cargo_raw_consumption_rate(UnitType::LightVehiclePlant, 2),
        12
    );
    assert_eq!(cargo_gold_consumption_rate(UnitType::GoldRefinery), 5);
}

#[test]
fn every_mission_index_has_a_rule_and_returns_a_status() {
    // The scripts cover training 1..15, scenario 1..24, campaign 1..9;
    // other numbers and categories fall back to the generic script `SC_L0002`.
    let mut state = empty_match();
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);

    for category in [
        MissionCategory::Custom,
        MissionCategory::Training,
        MissionCategory::Campaign,
        MissionCategory::HotSeat,
        MissionCategory::Multiplayer,
        MissionCategory::Demo,
        MissionCategory::Scenario,
        MissionCategory::MultiPlayerScenario,
    ] {
        for mission_index in 0..=25u16 {
            for team in 0..TEAM_SLOTS {
                let status =
                    mission_win_loss(&state, category, mission_index, TeamId(team as u8));
                assert!(status.is_some(), "{category:?}/{mission_index}/{team}");
            }
        }
    }
}

#[test]
fn training_predicates_read_the_right_unit_fields() {
    let mut state = empty_match();
    state.mission_category = MissionCategory::Training;

    // A mine with power on: maximums and actual mining.
    let mining = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), 0)
        .expect("mining station");
    {
        let unit = state.unit_mut(mining).expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.raw_mining_max = 12;
        unit.fuel_mining_max = 3;
        unit.gold_mining_max = 1;
        unit.raw_mining = 5;
        unit.fuel_mining = 2;
        unit.gold_mining = 1;
        unit.total_mining = 8;
    }

    assert_eq!(count_total_mining(&state, TeamId::RED), (12, 3, 1));
    assert_eq!(get_total_mining(&state, TeamId::RED, CargoType::Raw), 5);
    assert_eq!(get_total_mining(&state, TeamId::RED, CargoType::Fuel), 2);
    assert_eq!(get_total_mining(&state, TeamId::RED, CargoType::Gold), 1);

    // A disabled mine gives neither maximums nor mining.
    {
        let unit = state.unit_mut(mining).expect("unit");
        unit.orders = UnitOrder::PowerOff;
    }
    assert_eq!(count_total_mining(&state, TeamId::RED), (0, 0, 0));
    assert_eq!(get_total_mining(&state, TeamId::RED, CargoType::Raw), 0);

    // `Cargo_GetNetProduction` of the mine: gold + raw + fuel and consumption.
    {
        let unit = state.unit_mut(mining).expect("unit");
        unit.orders = UnitOrder::PowerOn;
    }
    let net = cargo_get_net_production(&state, state.unit(mining).unwrap(), false);
    assert_eq!(net.raw, 5);
    assert_eq!(net.fuel, 2);
    assert_eq!(net.gold, 1);
    assert_eq!(net.power, -1);
    assert_eq!(net.free_capacity, 16 - 8);

    // A factory in ORDER_BUILD consumes materials by `build_rate`.
    let plant = state
        .spawn_unit(
            UnitType::LightVehiclePlant,
            TeamId::RED,
            Point::new(24, 20),
            0,
        )
        .expect("plant");
    {
        let unit = state.unit_mut(plant).expect("unit");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit.build_time = 3;
        unit.build_rate = 1;
        unit.build_list = vec![UnitType::Tank];
    }
    assert_eq!(
        get_total_units_being_constructed(&state, TeamId::RED, UnitType::Tank),
        1
    );
    assert_eq!(
        get_total_power_consumption(&state, TeamId::RED, UnitType::LightVehiclePlant),
        1
    );
    let net = cargo_get_net_production(&state, state.unit(plant).unwrap(), false);
    assert_eq!(net.raw, -3); // LIGHTPLT: 3 × build_rate
    assert_eq!(net.power, -1);
}

#[test]
fn unit_state_predicates_match_cpp() {
    let mut state = empty_match();
    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
        .expect("tank");

    // A damaged unit (hits below the base).
    {
        let unit = state.unit_mut(tank).expect("unit");
        unit.hits = unit.base_values.get_attribute(ATTRIB_HITS) as u16 - 1;
    }
    assert_eq!(count_damaged_units(&state, TeamId::RED, UnitType::Tank), 1);

    // Reduced ammo.
    {
        let unit = state.unit_mut(tank).expect("unit");
        unit.ammo = 0;
    }
    assert_eq!(
        count_units_with_reduced_attrib(&state, TeamId::RED, UnitType::Tank, ATTRIB_AMMO),
        1
    );
    assert_eq!(
        count_units_with_reduced_attrib(&state, TeamId::RED, UnitType::Tank, ATTRIB_HITS),
        1
    );
    assert_eq!(
        count_units_with_reduced_attrib(&state, TeamId::RED, UnitType::Tank, ATTRIB_STORAGE),
        0
    );

    // Experience (`storage` of COMMANDO) and version (mark level).
    let commando = state
        .spawn_unit(UnitType::Infiltrator, TeamId::RED, Point::new(22, 20), 0)
        .expect("commando");
    {
        let unit = state.unit_mut(commando).expect("unit");
        unit.storage = 1;
    }
    assert!(has_unit_experience(
        &state,
        TeamId::RED,
        UnitType::Infiltrator
    ));

    {
        let unit = state.unit_mut(tank).expect("unit");
        unit.base_values.version = 2;
    }
    assert!(has_unit_above_mark_level(
        &state,
        TeamId::RED,
        UnitType::Tank,
        1
    ));

    // Losses and research level.
    state.teams[TeamId::RED.0 as usize].casualties[UnitType::Tank as usize] = 3;
    assert_eq!(count_casualties(&state, TeamId::RED, UnitType::Tank), 3);

    state.teams[TeamId::RED.0 as usize].research_topics[ResearchTopic::Armor as usize]
        .research_level = 4;
    assert_eq!(
        get_research_level(&state, TeamId::RED, ResearchTopic::Armor),
        4
    );
}

#[test]
fn worth_of_assets_counts_three_lists_only() {
    let mut state = empty_match();
    spawn(&mut state, UnitType::Tank, TeamId::RED, 10, 10);
    spawn(&mut state, UnitType::Engineer, TeamId::RED, 12, 10);
    // Ground cover (a slab) is not included in the base worth (`winloss.cpp:47`).
    spawn(&mut state, UnitType::LargeSlab, TeamId::RED, 14, 10);

    let tank_cost = state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .unit(*id)
                .is_some_and(|u| u.unit_type == UnitType::Tank)
        })
        .map(|id| get_normal_rate_build_cost(state.unit(id).unwrap()))
        .expect("tank");
    let engineer_cost = state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .unit(*id)
                .is_some_and(|u| u.unit_type == UnitType::Engineer)
        })
        .map(|id| get_normal_rate_build_cost(state.unit(id).unwrap()))
        .expect("engineer");

    assert_eq!(
        get_total_worth_of_assets(&state, TeamId::RED),
        tank_cost + engineer_cost
    );
}

// -----------------------------------------------------------------------
// Phase 1: end-game boundaries (U)
// -----------------------------------------------------------------------

/// Boundary 0: a zero duration limit ends the game before the first turn
/// (`victory_limit <= turn_counter` is inclusive).
#[test]
fn zero_duration_limit_ends_the_game_immediately() {
    let mut state = empty_match();
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
    state.victory_type = VictoryType::Duration;
    state.victory_limit = 0;
    state.turn_counter = 0;

    assert!(check_end_game_conditions(&mut state));
}

/// A single playing team is never ended by the "last survivor" rule
/// (`teams_total > 1` is required).
#[test]
fn single_team_game_does_not_end_by_elimination() {
    let mut state = empty_match();
    state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::None;
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);

    let status = evaluate_status(&state, state.turn_counter);
    assert_eq!(status.teams_total, 1);
    assert_eq!(status.teams_in_play, 1);
    assert_eq!(status.team_rank[TeamId::RED.0 as usize], 1);
    assert!(!check_end_game_conditions(&mut state));
}

/// No team in play (`teams_in_play == 0`) ends the game.
#[test]
fn game_with_no_team_in_play_ends() {
    let mut state = empty_match();
    // Neither team has a unit: both mission scripts report LOST.
    let status = evaluate_status(&state, state.turn_counter);
    assert_eq!(status.teams_total, 2);
    assert_eq!(status.teams_in_play, 0);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Lost
    );
    assert!(check_end_game_conditions(&mut state));
}

/// `TeamKind::Eliminated` leaves `teams_in_play` and the rank table, even if
/// the team still has units (`WinLoss_EvaluateStatus` checks the team type).
#[test]
fn eliminated_team_leaves_the_ranking() {
    let mut state = empty_match();
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
    state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::Eliminated;

    let status = evaluate_status(&state, state.turn_counter);
    assert_eq!(status.teams_total, 2, "an eliminated team is still total");
    assert_eq!(status.teams_in_play, 1);
    assert_eq!(status.team_rank[TeamId::GREEN.0 as usize], 0);
    assert_eq!(status.team_rank[TeamId::RED.0 as usize], 1);
    assert!(check_end_game_conditions(&mut state));
}

/// `PLAYER_TEAM_ALIEN` is a service slot: its units never enter
/// `teams_total`, `teams_in_play` or the ranking (`winloss.cpp`).
#[test]
fn derelict_slot_does_not_affect_the_ranking() {
    let mut state = empty_match();
    spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
    spawn(&mut state, UnitType::Tank, TeamId::GREEN, 40, 40);
    state.teams[TeamId::ALIEN.0 as usize].team_type = TeamKind::Computer;
    spawn(&mut state, UnitType::AlienTank, TeamId::ALIEN, 60, 60);

    let status = evaluate_status(&state, state.turn_counter);
    assert_eq!(status.teams_total, 2);
    assert_eq!(status.teams_in_play, 2);
    assert_eq!(status.team_rank[TeamId::ALIEN.0 as usize], 0);
    assert!(!check_end_game_conditions(&mut state));
}

/// Non-network `menu_check_end_game_conditions` marks a lost human team
/// `Eliminated` and continues; the network branch ends the game at once
/// (`menu.cpp:593-609`).
#[test]
fn local_human_loss_marks_the_team_eliminated() {
    fn three_humans() -> GameState {
        let mut state = empty_match();
        for team in [TeamId::RED, TeamId::GREEN, TeamId::BLUE] {
            state.teams[team.0 as usize].team_type = TeamKind::Player;
        }
        spawn(&mut state, UnitType::Tank, TeamId::RED, 20, 20);
        spawn(&mut state, UnitType::Tank, TeamId::BLUE, 60, 60);
        state
    }

    // Local game: GREEN is lost, the game continues and GREEN is eliminated.
    let mut local = three_humans();
    assert_eq!(
        local.teams[TeamId::GREEN.0 as usize].team_type,
        TeamKind::Player
    );
    assert!(
        !check_end_game_conditions(&mut local),
            "two teams are still in play"
    );
    assert_eq!(
        local.teams[TeamId::GREEN.0 as usize].team_type,
        TeamKind::Eliminated
    );

    // Network game: the same state ends the game immediately.
    let mut network = three_humans();
    network.network_game = true;
    assert!(check_end_game_conditions(&mut network));
    assert_eq!(
        network.teams[TeamId::GREEN.0 as usize].team_type,
        TeamKind::Player,
            "the network branch does not mutate the team"
    );
}
