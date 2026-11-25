use super::*;
use crate::enums::{Surface, TeamClan};
use crate::map::Map;
use crate::team::TeamKind;
use crate::game::GameState;

fn land_map() -> Map {
    let mut map = Map::new();

    for y in 0..crate::geo::MAP_HEIGHT {
        for x in 0..crate::geo::MAP_WIDTH {
            map.surface.set(x, y, Surface::LAND);
        }
    }

    map
}

fn state_with(count: usize) -> GameState {
    let map = land_map();
    let mut kinds = [TeamKind::None; 4];
    for kind in kinds.iter_mut().take(count) {
        *kind = TeamKind::Computer;
    }

    let mut state = GameState::with_setup(map, kinds);

    for team in 0..count {
        state.teams[team].team_type = TeamKind::Computer;
        state.teams[team].team_clan = TeamClan::Musashi;
    }
    state.ai.settings.opponent = Opponent::Average as u8;

    state
}

fn player(world: &mut GameState, team: TeamId) -> AiPlayer {
    let mut player = AiPlayer::new(team);
    player.init(world.ai.settings.opponent);
    player.strategy = AiStrategy::TankHorde;
    player
}

fn spot(world: &mut GameState, player: &mut AiPlayer, unit: UnitId) {
    let mut info = world.unit(unit).expect("unit").clone();
    info.set_visible_to_team(player.player_team, true);
    world.write_unit(unit, info);
    player
        .spotted_units
        .push(super::super::strategy::SpottedUnit::new(
            world.unit(unit).expect("unit"),
            player.player_team,
        ));
}

use crate::team::TeamId;
use crate::unit::UnitId;

#[test]
fn weight_table_selects_the_cpp_table_per_type() {
    let mut world = state_with(1);
    let player = player(&mut world, TeamId::RED);

    let table = player.get_weight_table(&world, Some(UnitType::Tank));
    assert_eq!(table.entries(), player.weight_table_tanks.entries());

    let table = player.get_weight_table(&world, Some(UnitType::Escort));
    assert_eq!(table.entries(), player.weight_table_fastboat.entries());

    // Unknown/foreign types fall back to the generic table.
    let table = player.get_weight_table(&world, Some(UnitType::Connector));
    assert_eq!(table.entries(), player.weight_table_generic.entries());
}

#[test]
fn composed_weight_table_merges_air_defense_bomber_submarine_and_spotted() {
    let mut world = state_with(2);
    let mut player = player(&mut world, TeamId::RED);
    let enemy = world
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(12, 12), 0)
        .expect("enemy tank");
    spot(&mut world, &mut player, enemy);

    let table = player.get_weight_table(&world, None);
    let expected = {
        let mut expected = WeightTable::new(0);
        expected.merge(&player.weight_table_air_defense);
        expected.merge(&player.weight_table_bomber);
        expected.merge(&player.weight_table_submarine);
        expected.merge(&player.weight_table_tanks);
        expected
    };

    assert_eq!(table.entries(), expected.entries());

    // Expert opponents additionally compose the commando table.
    world.ai.settings.opponent = Opponent::Expert as u8;
    let table = player.get_weight_table(&world, None);
    assert!(table
        .entries()
        .iter()
        .any(|entry| entry.unit_type == Some(UnitType::Infiltrator)));
}

#[test]
fn filtered_weight_table_zeroes_stationary_and_regenerating_types() {
    let mut world = state_with(1);
    let player = player(&mut world, TeamId::RED);

    // The ground-defense table mixes stationary defenses and mobile units.
    let table = player.get_filtered_weight_table(&world, Some(UnitType::GunTurret), 0x01);
    assert!(table
        .entries()
        .iter()
        .filter(|entry| entry
            .unit_type
            .map(|unit_type| unit_type_flags(unit_type) & unit_flags::STATIONARY != 0)
            .unwrap_or(false))
        .all(|entry| entry.weight == 0));
    assert!(table
        .entries()
        .iter()
        .any(|entry| entry.unit_type == Some(UnitType::Tank) && entry.weight > 0));
}

#[test]
fn extended_weight_table_gives_commando_against_expert_aliens() {
    let mut world = state_with(1);
    let mut player = player(&mut world, TeamId::RED);
    world.ai.settings.opponent = Opponent::Expert as u8;

    let alien = world
        .spawn_unit(UnitType::AlienTank, TeamId::ALIEN, Point::new(12, 12), 0)
        .expect("alien tank");
    let info = world.unit(alien).expect("alien tank").clone();

    let table = player.get_extended_weight_table(&world, &info, 0x01);
    assert_eq!(table.len(), 1);
    assert_eq!(table.entries()[0].unit_type, Some(UnitType::Infiltrator));

    // Below expert difficulty the alien target uses the ADUMP table as the
    // base; the reachability pass may zero weights but not change the types.
    world.ai.settings.opponent = Opponent::Average as u8;
    player.spotted_units.clear();
    let table = player.get_extended_weight_table(&world, &info, 0x01);
    let adump = player.get_filtered_weight_table(&world, Some(UnitType::MaterialStorage), 0x01);

    let types: Vec<_> = table
        .entries()
        .iter()
        .map(|entry| entry.unit_type)
        .collect();
    let adump_types: Vec<_> = adump
        .entries()
        .iter()
        .map(|entry| entry.unit_type)
        .collect();
    assert_eq!(types, adump_types);
}

#[test]
fn extended_weight_table_disables_unreachable_land_types() {
    let mut world = state_with(1);
    let player = player(&mut world, TeamId::RED);

    // A water building in the middle of a large lake: no land within any
    // weapon range.
    for y in 0..crate::geo::MAP_HEIGHT {
        for x in 0..crate::geo::MAP_WIDTH {
            let surface = if x < 30 && y < 30 {
                Surface::WATER
            } else {
                Surface::LAND
            };
            world.map.surface.set(x, y, surface);
        }
    }

    let target = world
        .spawn_unit(UnitType::Shipyard, TeamId::GREEN, Point::new(15, 15), 0)
        .expect("shipyard");
    let info = world.unit(target).expect("target").clone();

    let table = player.get_extended_weight_table(&world, &info, 0x01);
    let tank = table
        .entries()
        .iter()
        .find(|entry| entry.unit_type == Some(UnitType::Tank))
        .expect("tank entry");

    assert_eq!(tank.weight, 0, "land types out of reach are disabled");
    // The stationary target touches water: corvettes are added.
    assert!(table
        .entries()
        .iter()
        .any(|entry| entry.unit_type == Some(UnitType::Corvette)));
}

#[test]
fn strategic_orders_follow_the_strategy_and_mode() {
    let mut world = state_with(1);
    let mut player = player(&mut world, TeamId::RED);
    player.spotted_units.clear();

    // `mode = true` (`ChooseInitialUpgrades`) skips the generic chain and
    // the final `ProcessBuildOrders`, so the strategy list survives.
    let initial = player.choose_strategic_build_orders(&world, true);
    assert_eq!(
        initial.first().copied(),
        Some(BuildOrder::new(ATTRIB_SPEED, UnitType::Scout))
    );
    assert!(initial.contains(&BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank)));
    assert!(
        !initial.contains(&BuildOrder::new(ATTRIB_RANGE, UnitType::Tank)),
            "mode=true omits the low-priority extras"
    );

    // Defensive strategy: the first two orders are the anti-missile ones.
    player.strategy = AiStrategy::Defensive;
    let initial = player.choose_strategic_build_orders(&world, true);
    assert_eq!(
        initial.first().copied(),
        Some(BuildOrder::new(ATTRIB_RANGE, UnitType::MissileLauncher))
    );
    assert_eq!(
        initial.get(1).copied(),
        Some(BuildOrder::new(ATTRIB_ATTACK, UnitType::MissileLauncher))
    );

    // `mode = false` (`BeginTurn`) keeps only types under construction.
    player.strategy = AiStrategy::TankHorde;
    for (unit_type, build) in [
        (UnitType::Scout, Point::new(8, 8)),
        (UnitType::Tank, Point::new(9, 9)),
    ] {
        let builder = world
            .spawn_unit(UnitType::Constructor, TeamId::RED, build, 0)
            .expect("constructor");
        let mut unit = world.unit(builder).expect("constructor").clone();
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit.build_list = vec![unit_type];
        world.write_unit(builder, unit);
    }

    let full = player.choose_strategic_build_orders(&world, false);
    assert_eq!(
        full.first().copied(),
        Some(BuildOrder::new(ATTRIB_SPEED, UnitType::Scout))
    );
    assert!(full.contains(&BuildOrder::new(ATTRIB_RANGE, UnitType::Tank)));
}

#[test]
fn process_build_orders_keeps_only_types_under_construction() {
    let mut world = state_with(1);
    let player = player(&mut world, TeamId::RED);
    let mut orders = vec![
        BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank),
        BuildOrder::new(ATTRIB_SCAN, UnitType::Scanner),
    ];

    // No construction yet: every order is dropped.
    process_build_orders(&player, &world, &mut orders);
    assert!(orders.is_empty());

    // A builder that is building a tank keeps the tank order.
    let constructor = world
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let mut unit = world.unit(constructor).expect("constructor").clone();
    unit.orders = UnitOrder::Build;
    unit.state = UnitOrderState::BuildInProgress;
    unit.build_list = vec![UnitType::Tank];
    world.write_unit(constructor, unit);

    let mut orders = vec![
        BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank),
        BuildOrder::new(ATTRIB_SCAN, UnitType::Scanner),
    ];
    process_build_orders(&player, &world, &mut orders);
    assert_eq!(orders, vec![BuildOrder::new(ATTRIB_ATTACK, UnitType::Tank)]);
}

#[test]
fn reconnaissance_needs_react_to_outranging_enemies() {
    let mut world = state_with(2);
    let mut player = player(&mut world, TeamId::RED);
    let recon = UnitType::Corvette;
    let base_scan = world.team_units[TeamId::RED.0 as usize]
        .base_unit_values(recon as u16)
        .get_attribute(ATTRIB_SCAN);

    // An alien tank (range == the corvette scan) outranges the corvette by
    // one cell: the reconnaissance type must be ordered.
    let enemy = world
        .spawn_unit(UnitType::AlienTank, TeamId::GREEN, Point::new(20, 20), 0)
        .expect("enemy");
    spot(&mut world, &mut player, enemy);

    let mut orders = Vec::new();
    check_reconnaissance_needs(&player, &world, &mut orders, recon, true);
    assert_eq!(orders, vec![BuildOrder::new(ATTRIB_SCAN, recon)]);

    // No spotted enemies: nothing to do.
    player.spotted_units.clear();
    let mut orders = Vec::new();
    check_reconnaissance_needs(&player, &world, &mut orders, recon, true);
    assert!(orders.is_empty());
    assert!(base_scan > 0);
}
