//! Напоминания ИИ — C++ `class Reminder` и наследники (`reminders.cpp/.hpp`).
//!
//! Напоминание — отложенный вызов задачи или юнита. Очередей три
//! (см. [`crate::ai::task_manager::TaskManager`]): `priority`, `normal`,
//! `deferred`; `ExecuteReminders` обрабатывает их в детерминированном порядке
//! 2 priority : 1 normal, а `deferred` в начале каждого вызова переезжают в
//! `normal` (`reminders` ждут хода юнита, а юниты двигаются только между
//! вызовами).
//!
//! Побочные эффекты конструкторов C++ (`RemindTurnStart` ставит флаг
//! планирования, `RemindMoveFinished`/`RemindRetreat` — биты `ai_state_bits`)
//! выполняются при постановке в очередь — [`Reminder::on_append`].

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

/// Напоминание задачи или юнита — C++ `class Reminder` (иерархия класса).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reminder {
    /// `RemindTurnStart` — начало хода задачи.
    TurnStart {
        /// Задача.
        task: u32,
    },
    /// `RemindTurnEnd` — конец хода задачи.
    TurnEnd {
        /// Задача.
        task: u32,
    },
    /// `RemindAvailable` — юнит освободился, нужна новая задача.
    Available {
        /// Юнит.
        unit: UnitId,
    },
    /// `RemindMoveFinished` — юнит завершил ход/движение.
    MoveFinished {
        /// Юнит.
        unit: UnitId,
    },
    /// `RemindAttack` — юнит может атаковать.
    Attack {
        /// Юнит.
        unit: UnitId,
    },
    /// `RemindRetreat` — проверка реакции (отступление).
    Retreat {
        /// Юнит.
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

    /// Побочные эффекты конструктора C++ (выполняются при постановке в очередь).
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

    /// `Reminder::Execute` — действие конкретного напоминания.
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
