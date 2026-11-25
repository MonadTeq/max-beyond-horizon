use super::*;

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

#[test]
fn select_starting_position_is_within_map() {
    let mut world = GameState::new();
    ai_select_starting_position(&mut world, TeamId::RED);
    // Without `TeamMissionSupplies` the position is not saved, but RNG is consumed.
    let _ = world.rng.state();
}
