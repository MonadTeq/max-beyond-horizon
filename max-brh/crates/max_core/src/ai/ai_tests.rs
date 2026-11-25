use super::*;
use crate::enums::Surface;
use crate::geo::{MAP_HEIGHT, MAP_WIDTH};
use crate::input::{MissionSupplyEntry, Packet12MissionSupply};

/// Fixed RNG seeds of the starting-position property runs: deterministic (the
/// property must not flake), but several streams instead of one for coverage.
const START_POSITION_SEEDS: [u32; 4] = [0, 1, 5, 0x00C0_FFEE];

/// Positions generated per seed. Each test covers `4 * 30 = 120` positions, so
/// the required `N >= 100` holds even for a single test.
const START_POSITIONS_PER_SEED: usize = 30;

/// `TeamMissionSupplies` record with one deployable unit, as the landing phase
/// leaves it. The unit list must be non-empty: the safety check skips teams
/// without units.
fn mission_supplies(team: TeamId, position: Point) -> Packet12MissionSupply {
    Packet12MissionSupply {
        team,
        round: 0,
        start_gold: 0,
        team_gold: 0,
        units: vec![MissionSupplyEntry {
            unit_type: UnitType::Constructor,
            cargo: 0,
        }],
        gold_spent_on_upgrades: 0,
        starting_position: position,
        proximity_alert_ack: 0,
    }
}

/// Scene for `Ai_SelectStartingPosition`: the whole map is one surface, the RNG
/// is seeded, and the selecting team (RED) owns a `TeamMissionSupplies` record —
/// without it the selected point would not be stored. `enemy` adds a landed
/// GREEN team, making the enemy-distance branch of the safety check live.
fn landing_scene(seed: u32, surface: Surface, enemy: Option<Point>) -> GameState {
    let mut world = GameState::new();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            world.map.surface.set(x, y, surface);
        }
    }

    world.rng.set_seed(seed);
    world.mission_supplies[TeamId::RED.0 as usize] =
        Some(mission_supplies(TeamId::RED, Point::ZERO));

    if let Some(position) = enemy {
        world.mission_supplies[TeamId::GREEN.0 as usize] =
            Some(mission_supplies(TeamId::GREEN, position));
    }

    world
}

/// The point `ai_select_starting_position` wrote into
/// `UnitsManager_TeamMissionSupplies[RED].starting_position`.
fn selected_position(world: &GameState) -> Point {
    world.mission_supplies[TeamId::RED.0 as usize]
        .as_ref()
        .expect("the scene provides TeamMissionSupplies")
        .starting_position
}

/// `Access_IsInsideBounds`: the point addresses a cell of the 112×112 map.
fn is_within_map(position: Point) -> bool {
    position.x >= 0 && position.y >= 0 && position.x < MAP_WIDTH && position.y < MAP_HEIGHT
}

/// The 5×5 start area `Ai_IsValidStartingPosition`/`Ai_IsSafeStartingPosition`
/// require around a point.
fn start_area(position: Point) -> (i32, i32, i32, i32) {
    (
        i32::from(position.x) - 2,
        i32::from(position.y) - 2,
        i32::from(position.x) + 2,
        i32::from(position.y) + 2,
    )
}

/// Property: on a land-only, enemy-free scene every position selected by
/// `Ai_SelectStartingPosition` lies inside the map and passes both oracle
/// checks — the 5×5 area is land (`Ai_IsValidStartingPosition`, `ai.cpp:68`) and
/// the point is safe (`Ai_IsSafeStartingPosition`, `ai.cpp:80`). With no enemy
/// supplies safety reduces to validity, so the two must agree.
///
/// Restores the removed `select_starting_position_is_within_map`: instead of a
/// single unasserted call this checks 4 fixed seeds × 30 generated positions.
#[test]
fn ai_starting_positions_are_within_map() {
    let mut generated = 0;

    for seed in START_POSITION_SEEDS {
        let mut world = landing_scene(seed, Surface::LAND, None);

        for index in 0..START_POSITIONS_PER_SEED {
            ai_select_starting_position(&mut world, TeamId::RED);
            let position = selected_position(&world);

            assert!(
                is_within_map(position),
                "seed {seed}, position {index}: {position:?} is outside the map"
            );

            let valid = is_valid_starting_position(&world, start_area(position));
            let safe = is_safe_starting_position(&world, position, TeamId::RED);

            assert!(
                valid,
                "seed {seed}, position {index}: {position:?} must have a 5x5 land area"
            );
            assert!(
                safe,
                "seed {seed}, position {index}: {position:?} must be safe without enemies"
            );
            assert_eq!(
                safe, valid,
                "seed {seed}, position {index}: without enemy supplies safety is validity"
            );

            generated += 1;
        }
    }

    assert!(
        generated >= 100,
        "the property must cover at least 100 positions, covered {generated}"
    );
}

/// Property: `Ai_IsSafeStartingPosition` is stricter than validity — a landed
/// enemy team makes points closer than `AI_SAFE_ENEMY_DISTANCE` unsafe even
/// though the land check accepts them, so every selected point must keep that
/// distance. The enemy's own cell is the negative control: valid but unsafe.
#[test]
fn ai_starting_positions_keep_enemy_distance() {
    let mut generated = 0;

    for seed in START_POSITION_SEEDS {
        // The map center, far enough from the rolled range's edges.
        let mut world = landing_scene(seed, Surface::LAND, Some(Point::new(56, 56)));
        let enemy = world.mission_supplies[TeamId::GREEN.0 as usize]
            .as_ref()
            .expect("the scene provides enemy TeamMissionSupplies")
            .starting_position;

        // Validity is not safety: the enemy's cell is land but inside the radius.
        assert!(
            is_valid_starting_position(&world, start_area(enemy)),
            "seed {seed}: the enemy cell must be valid land"
        );
        assert!(
            !is_safe_starting_position(&world, enemy, TeamId::RED),
            "seed {seed}: a point on the enemy supplies must be unsafe"
        );

        for index in 0..START_POSITIONS_PER_SEED {
            ai_select_starting_position(&mut world, TeamId::RED);
            let position = selected_position(&world);

            assert!(
                is_within_map(position),
                "seed {seed}, position {index}: {position:?} is outside the map"
            );
            assert!(
                is_valid_starting_position(&world, start_area(position)),
                "seed {seed}, position {index}: {position:?} must have a 5x5 land area"
            );
            assert!(
                is_safe_starting_position(&world, position, TeamId::RED),
                "seed {seed}, position {index}: {position:?} must be safe"
            );

            let dx = i32::from(position.x) - i32::from(enemy.x);
            let dy = i32::from(position.y) - i32::from(enemy.y);
            assert!(
                dx * dx + dy * dy >= AI_SAFE_ENEMY_DISTANCE,
                "seed {seed}, position {index}: {position:?} is inside the enemy exclusion radius"
            );

            generated += 1;
        }
    }

    assert!(
        generated >= 100,
        "the property must cover at least 100 positions, covered {generated}"
    );
}

/// Property: when the map has no land at all, no candidate is valid or safe —
/// the selector exhausts its 64 attempts and falls back to the last roll, which
/// must still lie inside the map. This is the `attempts >= 64` escape branch.
#[test]
fn ai_starting_position_fallback_stays_inside_the_map() {
    let mut generated = 0;

    for seed in START_POSITION_SEEDS {
        let mut world = landing_scene(seed, Surface::WATER, None);

        for index in 0..START_POSITIONS_PER_SEED {
            ai_select_starting_position(&mut world, TeamId::RED);
            let position = selected_position(&world);

            assert!(
                is_within_map(position),
                "seed {seed}, position {index}: the fallback {position:?} is outside the map"
            );
            assert!(
                !is_valid_starting_position(&world, start_area(position)),
                "seed {seed}, position {index}: no 5x5 area on a water map is land"
            );
            assert!(
                !is_safe_starting_position(&world, position, TeamId::RED),
                "seed {seed}, position {index}: no point on a water map is safe"
            );

            generated += 1;
        }
    }

    assert!(
        generated >= 100,
        "the property must cover at least 100 positions, covered {generated}"
    );
}

#[test]
fn ai_state_starts_idle_with_four_players() {
    let world = GameState::new();
    assert_eq!(world.ai.players.len(), PLAYER_TEAM_COUNT);
    assert_eq!(world.ai.reaction_state, AiReactionState::Idle);
    assert_eq!(world.ai.task_manager.task_count(), 0);
    assert_eq!(world.ai.heat_maps[0].get_complete(0, 0), 0);
    assert!(!world.ai.all_visible);
    assert_eq!(world.ai.settings.opponent, Opponent::Apprentice as u8);
    assert_eq!(world.ai.settings.max_percent, 90);
    assert_eq!(world.ai.settings.proximity_range, 14);
    assert_eq!(world.ai.settings.team_strategy, [None; PLAYER_TEAM_COUNT]);
}

#[test]
fn with_task_manager_restores_manager() {
    let mut world = GameState::new();
    let task_id = world.with_task_manager(|task_manager, world| {
        task_manager.append_task(
            world,
            Task::new(
                0,
                TeamId::RED,
                None,
                task::TASK_PRIORITY_ATTACK_DEFAULT,
                TaskKind::MineAssistant { manager: None },
            ),
        )
    });

    assert!(world.ai.task_manager.has_task(task_id));
    assert_eq!(world.ai.task_manager.task_count(), 1);
}

#[test]
fn digest_changes_when_ai_state_changes() {
    let mut world = GameState::new();
    let before = world.snapshot_bytes();

    world.ai.players[0].strategy = crate::enums::AiStrategy::TankHorde;
    let after = world.snapshot_bytes();

    assert_ne!(before, after);
}

#[test]
fn mine_map_entry_roundtrip() {
    let mut world = GameState::new();
    let site = Point::new(10, 10);

    assert_eq!(world.ai.players[0].get_mine_map_entry(site), 0);
    world.ai.players[0].mine_map[10 * 112 + 10] = 7;
    assert_eq!(world.ai.players[0].get_mine_map_entry(site), 7);
}
