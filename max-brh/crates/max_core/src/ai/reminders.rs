//! AI reminders — C++ `class Reminder` and its subclasses (`reminders.cpp/.hpp`).
//!
//! A reminder is a deferred call of a task or unit. There are three queues
//! (see [`crate::ai::task_manager::TaskManager`]): `priority`, `normal`,
//! `deferred`; `ExecuteReminders` processes them in a deterministic order
//! 2 priority : 1 normal, while `deferred` reminders move to
//! `normal` at the start of every call (they wait for the unit's turn, and units move only between
//! calls).
//!
//! Side effects of the C++ constructors (`RemindTurnStart` sets the scheduling
//! flag, `RemindMoveFinished`/`RemindRetreat` — `ai_state_bits` bits)
//! are performed when the reminder is enqueued — [`Reminder::on_append`].

use crate::enums::AI_STATE_MOVE_FINISHED_REMINDER;
use crate::game::GameState;
use crate::unit::UnitId;

use super::task_manager::TaskManager;
use super::tasks;

/// `REMINDER_TYPE_TURN_START`.
pub const REMINDER_TYPE_TURN_START: u8 = 0;
/// `REMINDER_TYPE_TURN_END`.
pub const REMINDER_TYPE_TURN_END: u8 = 1;
/// `REMINDER_TYPE_AVAILABLE`.
pub const REMINDER_TYPE_AVAILABLE: u8 = 2;
/// `REMINDER_TYPE_MOVE`.
pub const REMINDER_TYPE_MOVE: u8 = 3;
/// `REMINDER_TYPE_ATTACK`.
pub const REMINDER_TYPE_ATTACK: u8 = 4;
/// `REMINDER_TYPE_RETREAT`.
pub const REMINDER_TYPE_RETREAT: u8 = 5;
/// `REMINDER_TYPE_COUNT`.
pub const REMINDER_TYPE_COUNT: usize = 6;

/// A task or unit reminder — C++ `class Reminder` (class hierarchy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reminder {
    /// `RemindTurnStart` — start of the task's turn.
    TurnStart {
        /// Task.
        task: u32,
    },
    /// `RemindTurnEnd` — end of the task's turn.
    TurnEnd {
        /// Task.
        task: u32,
    },
    /// `RemindAvailable` — the unit became free and needs a new task.
    Available {
        /// Unit.
        unit: UnitId,
    },
    /// `RemindMoveFinished` — the unit finished its turn/movement.
    MoveFinished {
        /// Unit.
        unit: UnitId,
    },
    /// `RemindAttack` — the unit can attack.
    Attack {
        /// Unit.
        unit: UnitId,
    },
    /// `RemindRetreat` — reaction check (retreat).
    Retreat {
        /// Unit.
        unit: UnitId,
    },
}

impl Reminder {
    /// `Reminder::GetType`.
    pub fn get_type(&self) -> u8 {
        match self {
            Self::TurnStart { .. } => REMINDER_TYPE_TURN_START,
            Self::TurnEnd { .. } => REMINDER_TYPE_TURN_END,
            Self::Available { .. } => REMINDER_TYPE_AVAILABLE,
            Self::MoveFinished { .. } => REMINDER_TYPE_MOVE,
            Self::Attack { .. } => REMINDER_TYPE_ATTACK,
            Self::Retreat { .. } => REMINDER_TYPE_RETREAT,
        }
    }

    /// Side effects of the C++ constructor (performed when the reminder is enqueued).
    pub fn on_append(&self, manager: &mut TaskManager, world: &mut GameState) {
        match self {
            Self::TurnStart { task } => {
                if let Some(task) = manager.task_mut(*task) {
                    task.scheduled_for_turn_start = true;
                }
            }
            Self::TurnEnd { task } => {
                if let Some(task) = manager.task_mut(*task) {
                    task.scheduled_for_turn_end = true;
                }
            }
            Self::Available { .. } => {}
            Self::MoveFinished { unit } => {
                if let Some(mut unit_info) = world.unit(*unit).cloned() {
                    unit_info.ai_state_bits |= AI_STATE_MOVE_FINISHED_REMINDER;
                    world.write_unit(*unit, unit_info);
                }
            }
            Self::Attack { .. } => {}
            Self::Retreat { unit } => {
                if let Some(mut unit_info) = world.unit(*unit).cloned() {
                    unit_info.ai_state_bits |= crate::enums::AI_STATE_RETREAT_REMINDER;
                    world.write_unit(*unit, unit_info);
                }
            }
        }
    }

    /// `Reminder::Execute` — the action of a concrete reminder.
    pub fn execute(&self, manager: &mut TaskManager, world: &mut GameState) {
        match self {
            Self::TurnStart { task } => {
                if let Some(task) = manager.task_mut(*task) {
                    task.scheduled_for_turn_start = false;
                }
                tasks::task_begin_turn(manager, world, *task);
            }
            Self::TurnEnd { task } => {
                if let Some(task) = manager.task_mut(*task) {
                    task.scheduled_for_turn_end = false;
                }
                tasks::task_end_turn(manager, world, *task);
            }
            Self::Available { unit } => {
                // `if (!unit->GetTask()) TaskManager.FindTaskForUnit(&*unit);`
                if manager.unit_task(*unit).is_none() {
                    manager.find_task_for_unit(world, *unit);
                }
            }
            Self::MoveFinished { unit } => {
                if matches!(world.unit(*unit), Some(_)) {
                    if let Some(mut unit_info) = world.unit(*unit).cloned() {
                        unit_info.ai_state_bits &= !AI_STATE_MOVE_FINISHED_REMINDER;
                        world.write_unit(*unit, unit_info);
                    }
                }

                let Some(task_id) = manager.unit_task(*unit) else {
                    return;
                };
                let hits = world.unit(*unit).map(|unit| unit.hits).unwrap_or(0);
                if hits > 0 {
                    tasks::task_execute(manager, world, task_id, *unit);
                }
            }
            Self::Attack { unit } => {
                let Some(unit_info) = world.unit(*unit) else {
                    return;
                };
                let team_computer =
                    world.teams[unit_info.team.0 as usize].team_type == crate::team::TeamKind::Computer;

                if unit_info.hits > 0 && unit_info.shots > 0 && team_computer {
                    super::attack::evaluate_attack(manager, world, *unit, true);
                }
            }
            Self::Retreat { unit } => {
                manager.check_unit_reaction(world, *unit);
            }
        }
    }
}
