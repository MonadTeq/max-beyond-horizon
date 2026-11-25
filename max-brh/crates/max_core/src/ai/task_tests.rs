use super::*;
use crate::ai::task_manager::TaskManager;
use crate::team::TeamId;

#[test]
fn task_names_match_cpp_table() {
    assert_eq!(task_get_name(TASK_TYPE_ATTACK), "Attack");
    assert_eq!(task_get_name(TASK_TYPE_CREATE_BUILDING), "CreateBuilding");
    assert_eq!(task_get_name(TASK_TYPE_WAIT_TO_ATTACK), "WaitToAttack");
    assert_eq!(task_get_name(5), "Unknown");
    assert_eq!(task_get_name(200), "Unknown");
}

#[test]
fn priorities_are_ordered_by_urgency() {
    assert!(TASK_PRIORITY_EMERGENCY < TASK_PRIORITY_ATTACK_DEFAULT);
    assert!(TASK_PRIORITY_CHECK_ASSAULTS < TASK_PRIORITY_REMOVE_RUBBLE);
    assert!(TASK_PRIORITY_BUILDING_LOW < TASK_PRIORITY_BUILDING_DUMP);
    assert_eq!(TASK_PRIORITY_REPAIR, TASK_PRIORITY_FOLLOW_DEFENSE);
    assert_eq!(TASK_PRIORITY_FIND_MINES, TASK_PRIORITY_FOLLOW_ATTACK);
}

#[test]
fn compare_priority_matches_cpp_window() {
    let world = GameState::new();
    let task = Task::new(
        1,
        TeamId::RED,
        None,
        TASK_PRIORITY_MANAGE_BUILDINGS,
        TaskKind::MineAssistant { manager: None },
    );

    // Inside the window the difference of the dynamic priority is returned.
    assert_eq!(
        task.compare_priority(&world, TASK_PRIORITY_MANAGE_BUILDINGS),
        0
    );
    // More urgent than the argument — a negative value.
    assert!(task.compare_priority(&world, TASK_PRIORITY_MANAGE_BUILDINGS + 0x40) < 0);
    // Less urgent (a larger number) — a positive value.
    assert!(task.compare_priority(&world, TASK_PRIORITY_MANAGE_BUILDINGS - 0x40) > 0);
    // Outside the window — saturation.
    assert_eq!(task.compare_priority(&world, 0xFFFF), -1);
    assert_eq!(task.compare_priority(&world, 0x0000), 1);
}

#[test]
fn adjacency_and_repair_due_follow_cpp() {
    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::new(TeamId::RED, 1));
    unit.grid = Point::new(5, 5);
    assert!(task_is_adjacent(&unit, 6, 6));
    assert!(!task_is_adjacent(&unit, 8, 5));

    unit.flags = unit_flags::BUILDING;
    assert!(task_is_adjacent(&unit, 7, 7));

    let base_hits = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_HITS) as u16;
    // `hits < base_hits / 4` — strictly less.
    unit.hits = base_hits / 4;
    assert!(!task_is_repair_due(&unit, false));
    unit.hits = base_hits / 4 - 1;
    assert!(task_is_repair_due(&unit, false));
    unit.hits = base_hits / 3;
    assert!(!task_is_repair_due(&unit, true) || base_hits / 3 < base_hits / 2);
    unit.hits = base_hits / 2 - 1;
    assert!(task_is_repair_due(&unit, true));
}

#[test]
fn task_manager_starts_empty() {
    let manager = TaskManager::new();
    assert_eq!(manager.task_count(), 0);
    assert_eq!(manager.reminders_count(), 0);
}
