use super::*;
use crate::enums::{Opponent, TeamClan};
use crate::game::GameState;
use crate::team::{TeamId, TeamKind};

/// Entries as `(type, weight)` in insertion order.
fn entries(table: &WeightTable) -> Vec<(UnitType, u16)> {
    table
        .entries()
        .iter()
        .map(|entry| (entry.unit_type.expect("weight entry"), entry.weight))
        .collect()
}

fn player_at(opponent: Opponent) -> AiPlayer {
    let mut player = AiPlayer::new(TeamId::RED);
    player.init_weight_tables(opponent as u8);
    player
}

#[test]
fn init_weight_tables_uses_the_cpp_entry_order_and_weights() {
    let player = player_at(Opponent::Expert);

    // `weight_table_ground_defense`: JUGGRNT/ALN* are not buildable under the
    // default rules, so `Add` drops them (C++ `Builder_IsBuildable`).
    assert_eq!(
        entries(&player.weight_table_ground_defense),
        vec![
            (UnitType::Infiltrator, 2),
            (UnitType::Tank, 1),
            (UnitType::AssaultGun, 1),
            (UnitType::RocketLauncher, 1),
            (UnitType::MissileCrawler, 2),
            (UnitType::Gunboat, 1),
            (UnitType::MissileCruiser, 2),
            (UnitType::Bomber, 2),
        ]
    );

    // `weight_table_submarine` has one entry in C++.
    assert_eq!(
        entries(&player.weight_table_submarine),
        vec![(UnitType::Corvette, 1)]
    );

    // `weight_table_commando`.
    assert_eq!(
        entries(&player.weight_table_commando),
        vec![(UnitType::Infantry, 1)]
    );

    // `weight_table_aircrafts`: ANTIAIR only from AVERAGE on, ALNPLANE dropped.
    assert_eq!(
        entries(&player.weight_table_aircrafts),
        vec![
            (UnitType::AntiAir, 3),
            (UnitType::MobileAntiAir, 3),
            (UnitType::Escort, 3),
            (UnitType::Fighter, 1),
        ]
    );

    // Every table gets the player's team and is non-empty.
    assert_eq!(player.weight_table_ground_defense.team(), TeamId::RED.0);
    assert_eq!(player.weight_table_9.team(), TeamId::RED.0);
    assert!(!player.weight_table_9.is_empty());
    assert!(!player.weight_table_bomber.is_empty());
    assert!(!player.weight_table_fastboat.is_empty());
    assert!(!player.weight_table_corvette.is_empty());
    assert!(!player.weight_table_battleships.is_empty());
    assert!(!player.weight_table_missileboat.is_empty());
    assert!(!player.weight_table_support_ships.is_empty());
    assert!(!player.weight_table_infantry.is_empty());
    assert!(!player.weight_table_generic.is_empty());
    assert!(!player.weight_table_tanks.is_empty());
    assert!(!player.weight_table_scout.is_empty());
    assert!(!player.weight_table_assault_guns.is_empty());
    assert!(!player.weight_table_missile_launcher.is_empty());
    assert!(!player.weight_table_air_defense.is_empty());
    assert!(!player.weight_table_aircrafts.is_empty());
    assert!(!player.weight_table_bomber.is_empty());
    assert!(!player.weight_table_commando.is_empty());
}

#[test]
fn init_weight_tables_follows_opponent_thresholds() {
    // CLUELESS: no threshold-guarded entries at all.
    let clueless = player_at(Opponent::Clueless);
    assert_eq!(
        entries(&clueless.weight_table_scout),
        vec![
            (UnitType::Scout, 2),
            (UnitType::Tank, 1),
            (UnitType::AssaultGun, 2),
            (UnitType::RocketLauncher, 1),
            (UnitType::MissileCrawler, 2),
            (UnitType::Gunboat, 2),
            (UnitType::MissileCruiser, 2),
            (UnitType::Bomber, 3),
        ]
    );
    assert_eq!(
        clueless
            .weight_table_ground_defense
            .get_weight(UnitType::Infiltrator),
        0
    );

    // APPRENTICE: GUNTURRT appears (the guard is `>= APPRENTICE`).
    let apprentice = player_at(Opponent::Apprentice);
    assert_eq!(
        apprentice
            .weight_table_scout
            .get_weight(UnitType::GunTurret),
        1
    );
    assert_eq!(
        apprentice
            .weight_table_scout
            .get_weight(UnitType::Artillery),
        0
    );
    assert_eq!(
        apprentice
            .weight_table_scout
            .get_weight(UnitType::Submarine),
        0
    );

    // AVERAGE: ARTYTRRT/ANTIMSSL (and SUBMARNE) are added after the
    // APPRENTICE block — the C++ order (artillery, missile launcher,
    // gun turret, submarine) is preserved.
    let average = player_at(Opponent::Average);
    assert_eq!(
        &entries(&average.weight_table_scout)[..4],
        &[
            (UnitType::Artillery, 1),
            (UnitType::MissileLauncher, 1),
            (UnitType::GunTurret, 1),
            (UnitType::Submarine, 1),
        ]
    );
    assert_eq!(
        average
            .weight_table_ground_defense
            .get_weight(UnitType::Infiltrator),
        0
    );

    // EXPERT: COMMANDO/INFANTRY entries appear.
    let expert = player_at(Opponent::Expert);
    assert_eq!(
        expert
            .weight_table_ground_defense
            .get_weight(UnitType::Infiltrator),
        2
    );
    assert_eq!(
        expert.weight_table_scout.get_weight(UnitType::Infiltrator),
        1
    );
    assert_eq!(
        expert
            .weight_table_generic
            .get_weight(UnitType::Infiltrator),
        1
    );
    assert_eq!(
        expert.weight_table_generic.get_weight(UnitType::Infantry),
        1
    );
    assert_eq!(
        average.weight_table_generic.get_weight(UnitType::Infantry),
        0
    );
}

#[test]
fn update_weight_tables_matches_the_cpp_strategy_switch() {
    // DEFENSIVE: GUNTURRT/ANTIMSSL/ANTIAIR ×3 in every table that holds them.
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::Defensive;
    player.update_weight_tables();
    assert_eq!(player.weight_table_tanks.get_weight(UnitType::GunTurret), 3);
    assert_eq!(
        player
            .weight_table_air_defense
            .get_weight(UnitType::MissileLauncher),
        3
    );
    assert_eq!(
        player.weight_table_aircrafts.get_weight(UnitType::AntiAir),
        9
    );

    // MISSILES: MISSLLCH/MSSLBOAT ×5.
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::Missiles;
    player.update_weight_tables();
    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::MissileCrawler),
        10
    );
    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::MissileCruiser),
        10
    );

    // AIR: FIGHTER/BOMBER ×5 (the `weight_table_bomber` table lists the
    // counters of bombers — FIGHTER — not BOMBER itself).
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::Air;
    player.update_weight_tables();
    assert_eq!(
        player.weight_table_aircrafts.get_weight(UnitType::Fighter),
        5
    );
    assert_eq!(player.weight_table_bomber.get_weight(UnitType::Fighter), 15);

    // SEA: FASTBOAT/CORVETTE/SUBMARNE/BATTLSHP/MSSLBOAT ×2.
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::Sea;
    player.update_weight_tables();
    assert_eq!(
        player.weight_table_aircrafts.get_weight(UnitType::Escort),
        6
    );
    assert_eq!(
        player.weight_table_corvette.get_weight(UnitType::Corvette),
        2
    );
    assert_eq!(player.weight_table_scout.get_weight(UnitType::Submarine), 2);
    assert_eq!(player.weight_table_tanks.get_weight(UnitType::Gunboat), 4);
    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::MissileCruiser),
        4
    );

    // SCOUT_HORDE: SCOUT is added to four tables, then ×10 everywhere.
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::ScoutHorde;
    player.update_weight_tables();
    assert_eq!(player.weight_table_tanks.get_weight(UnitType::Scout), 10);
    assert_eq!(
        player.weight_table_assault_guns.get_weight(UnitType::Scout),
        20
    );
    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::Scout),
        10
    );
    assert_eq!(
        player
            .weight_table_missile_launcher
            .get_weight(UnitType::Scout),
        10
    );
    assert_eq!(player.weight_table_scout.get_weight(UnitType::Scout), 20);

    // TANK_HORDE: TANK ×10.
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::TankHorde;
    player.update_weight_tables();
    assert_eq!(player.weight_table_tanks.get_weight(UnitType::Tank), 20);
    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::Tank),
        10
    );

    // FAST_ATTACK: ARTILLRY ×3, BOMBER/SP_FLAK/CORVETTE/FASTBOAT ×2.
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::FastAttack;
    player.update_weight_tables();
    assert_eq!(
        player.weight_table_tanks.get_weight(UnitType::AssaultGun),
        3
    );
    assert_eq!(player.weight_table_tanks.get_weight(UnitType::Bomber), 6);
    assert_eq!(
        player
            .weight_table_aircrafts
            .get_weight(UnitType::MobileAntiAir),
        6
    );
    assert_eq!(
        player.weight_table_fastboat.get_weight(UnitType::Corvette),
        2
    );
    assert_eq!(
        player.weight_table_aircrafts.get_weight(UnitType::Escort),
        6
    );

    // COMBINED_ARMS: TANK/MISSLLCH ×5.
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::CombinedArms;
    player.update_weight_tables();
    assert_eq!(player.weight_table_tanks.get_weight(UnitType::Tank), 10);
    assert_eq!(
        player
            .weight_table_tanks
            .get_weight(UnitType::MissileCrawler),
        10
    );

    // ESPIONAGE: SUBMARNE/COMMANDO ×4, MISSLLCH/MSSLBOAT ×2.
    // (The `weight_table_commando` table itself holds INFANTRY — the counter —
    // so COMMANDO is multiplied where it is listed, e.g. ground defense.)
    let mut player = player_at(Opponent::Expert);
    player.strategy = AiStrategy::Espionage;
    player.update_weight_tables();
    assert_eq!(player.weight_table_scout.get_weight(UnitType::Submarine), 4);
    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::Infiltrator),
        8
    );
    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::MissileCrawler),
        4
    );
    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::MissileCruiser),
        4
    );

    // RANDOM: no `default:` in C++, the tables stay untouched.
    let mut player = player_at(Opponent::Average);
    player.strategy = AiStrategy::Random;
    let before = entries(&player.weight_table_tanks);
    player.update_weight_tables();
    assert_eq!(entries(&player.weight_table_tanks), before);
}

#[test]
fn rebuild_weight_tables_touches_all_eighteen_tables() {
    let mut player = player_at(Opponent::Expert);
    player.rebuild_weight_tables(UnitType::Tank, 3);

    for table in [
        &player.weight_table_ground_defense,
        &player.weight_table_scout,
        &player.weight_table_tanks,
        &player.weight_table_assault_guns,
        &player.weight_table_missile_launcher,
        &player.weight_table_air_defense,
        &player.weight_table_support_ships,
        &player.weight_table_infantry,
        &player.weight_table_generic,
    ] {
        assert!(table.get_weight(UnitType::Tank) > 0);
    }

    assert_eq!(
        player
            .weight_table_ground_defense
            .get_weight(UnitType::Tank),
        3
    );
    assert_eq!(player.weight_table_tanks.get_weight(UnitType::Tank), 6);

    // Tables without TANK keep their entries untouched.
    assert_eq!(
        entries(&player.weight_table_submarine),
        vec![(UnitType::Corvette, 1)]
    );
    assert_eq!(player.weight_table_fastboat.get_weight(UnitType::Tank), 0);
    assert_eq!(player.weight_table_corvette.get_weight(UnitType::Tank), 0);
    assert_eq!(
        player.weight_table_battleships.get_weight(UnitType::Tank),
        0
    );
}

#[test]
fn ai_init_fills_the_tables_of_every_team() {
    let mut world = GameState::new();
    world.ai.settings.opponent = Opponent::Average as u8;

    crate::ai::ai_init(&mut world);

    for (index, player) in world.ai.players.iter().enumerate() {
        assert!(!player.weight_table_generic.is_empty(), "team {index}");
        assert!(!player.weight_table_tanks.is_empty(), "team {index}");
        assert_eq!(player.weight_table_tanks.team(), index as u8);
    }
}

#[test]
fn begin_turn_applies_strategy_adjustments_once() {
    let mut world = GameState::new();
    world.ai.settings.opponent = Opponent::Average as u8;
    let team = TeamId::RED;
    world.teams[team.0 as usize].team_type = TeamKind::Computer;
    world.teams[team.0 as usize].team_clan = TeamClan::TheChosen;

    crate::ai::ai_init(&mut world);
    assert_eq!(
        world.ai.players[team.0 as usize]
            .weight_table_aircrafts
            .get_weight(UnitType::Fighter),
        1
    );

    // `BeginTurn`'s `need_init` block: clan THE_CHOSEN → AI_STRATEGY_AIR,
    // then `UpdateWeightTables` (FIGHTER/BOMBER ×5).
    crate::ai::ai_begin_turn(&mut world, team);
    let player = &world.ai.players[team.0 as usize];
    assert_eq!(player.strategy, AiStrategy::Air);
    assert_eq!(
        player.weight_table_aircrafts.get_weight(UnitType::Fighter),
        5
    );
    assert_eq!(player.weight_table_bomber.get_weight(UnitType::Fighter), 15);

    // The `need_init` block runs once: a second turn must not multiply again.
    crate::ai::ai_begin_turn(&mut world, team);
    assert_eq!(
        world.ai.players[team.0 as usize]
            .weight_table_aircrafts
            .get_weight(UnitType::Fighter),
        5
    );
}
