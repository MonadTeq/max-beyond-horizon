use super::*;
use crate::ai::task::{TASK_PRIORITY_ATTACK_DEFAULT, TASK_TYPE_MINE_ASSISTANT};

#[test]
fn reminders_execute_two_priority_then_normal() {
    let mut world = GameState::new();
    let mut manager = TaskManager::new();

    let task = Task::new(
        0,
        TeamId::RED,
        None,
        TASK_PRIORITY_ATTACK_DEFAULT,
        TaskKind::MineAssistant { manager: None },
    );
    let task_id = manager.append_task(&mut world, task);

    // `append_task` calls `Task::Init` → `RemindTurnStart(true)`.
    assert!(manager.reminders_count() > 0);

    manager.normal_reminders.clear();
    manager.priority_reminders.clear();

    for _ in 0..3 {
        manager.append_reminder(&mut world, Reminder::Available { unit: UnitId(1) }, false);
    }
    for _ in 0..4 {
        manager.append_reminder(&mut world, Reminder::Retreat { unit: UnitId(2) }, true);
    }

    let order = manager.execute_reminders(&mut world);
    assert!(order);
    assert_eq!(manager.reminders_count(), 0);
    assert!(manager.has_task(task_id));
}

#[test]
fn unit_task_list_is_push_front() {
    let mut manager = TaskManager::new();
    let unit = UnitId::new(TeamId::RED, 1);

    manager.attach_task_to_unit(unit, 10);
    manager.attach_task_to_unit(unit, 11);
    assert_eq!(manager.unit_task(unit), Some(11));
    assert_eq!(manager.unit_tasks_of(unit), vec![11, 10]);

    let mut world = GameState::new();
    manager.detach_task_from_unit(&mut world, 11, unit, false);
    assert_eq!(manager.unit_task(unit), Some(10));
}

#[test]
fn reminder_selection_alternates_two_priority_then_one_normal() {
    let mut manager = TaskManager::new();

    // 4 priority + 3 normal: the expected sequence is P,P,N,P,P,N,N.
    for _ in 0..4 {
        manager
            .priority_reminders
            .push(Reminder::Retreat { unit: UnitId(1) });
    }
    for _ in 0..3 {
        manager
            .normal_reminders
            .push(Reminder::Available { unit: UnitId(2) });
    }

    let mut sequence = Vec::new();
    while let Some(reminder) = manager.pop_next_reminder() {
        sequence.push(reminder.get_type());
    }

    assert_eq!(
        sequence,
        vec![
            super::super::reminders::REMINDER_TYPE_RETREAT,
            super::super::reminders::REMINDER_TYPE_RETREAT,
            super::super::reminders::REMINDER_TYPE_AVAILABLE,
            super::super::reminders::REMINDER_TYPE_RETREAT,
            super::super::reminders::REMINDER_TYPE_RETREAT,
            super::super::reminders::REMINDER_TYPE_AVAILABLE,
            super::super::reminders::REMINDER_TYPE_AVAILABLE,
        ]
    );
    assert_eq!(manager.reminder_counter, 0);
}

#[test]
fn digest_is_stable_for_empty_manager() {
    let manager = TaskManager::new();
    let mut first = Vec::new();
    manager.digest_into(&mut first);
    let mut second = Vec::new();
    manager.digest_into(&mut second);
    assert_eq!(first, second);
    assert_eq!(manager.next_task_id, 1);
    assert_eq!(TASK_TYPE_MINE_ASSISTANT, 22);
}
