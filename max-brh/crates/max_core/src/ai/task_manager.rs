//! Менеджер задач ИИ — C++ `class TaskManager` (`task_manager.cpp`).
//!
//! # Что портировано
//!
//! * очереди напоминаний `priority` / `normal` / `deferred` и их
//!   детерминированный разбор `ExecuteReminders`: `deferred` переезжают в
//!   `normal`, затем 2 priority : 1 normal, пока очереди не опустеют
//!   (`task_manager.cpp:277`). Тайм-бюджет (`TickTimer_HaveTimeToThink`) не
//!   портируется — DESIGN §5.1, риск 4: очередь обрабатывается полностью;
//! * жизненный цикл задач (`AppendTask`, `RemoveTask`, `Clear`,
//!   `MarkTasksForProcessing`), назначение юнитов (`FindTaskForUnit`,
//!   `ClearUnitTasksAndRemindAvailable`, `ScheduleDelayedTasks`), запросы
//!   производства (`CreateBuilding`, `CreateUnit`, `IsUnitNeeded`),
//!   реакции (`CheckComputerReactions`, `CheckUnitReaction`,
//!   `CollectPotentialAttackTargets`, `EnqueueUnitForReactionCheck`);
//! * списки задач юнита (`UnitInfo::tasks` в C++ — `PushFront`, поэтому
//!   индекс 0 — «верхняя» задача, `UnitInfo::GetTask`).
//!
//! # Детерминизм
//!
//! Все списки — `Vec` в порядке вставки (C++ `SmartList`); обход задач и
//! напоминаний идёт строго в этом порядке; карт с неупорядоченной итерацией
//! нет. RNG вызывается только внутри задач и `AiPlayer` (см. `rng.rs`).

use crate::enums::{unit_flags, Opponent, UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitListKind};

use super::reminders::{Reminder, REMINDER_TYPE_COUNT};
use super::task::{
    task_estimate_turns_till_mission_end, task_get_name, task_is_ready_to_take_orders,
    TASK_PRIORITY_OFFSET_CENSUS_SLACK, TASK_TYPE_CREATE_BUILDING, TASK_TYPE_CREATE_UNIT,
    TASK_TYPE_OBTAIN_UNITS, TASK_TYPE_TRANSPORT,
};
use super::tasks::{TaskCtx, TaskKind};
use super::Task;

/// Менеджер задач и напоминаний команды ИИ (один на партию).
#[derive(Debug, Clone, Default)]
pub struct TaskManager {
    /// `SmartList<Task> tasks` — порядок регистрации.
    tasks: Vec<Task>,
    /// `SmartList<TaskObtainUnits> unit_requests` — id задач (`PushBack`).
    unit_requests: Vec<u32>,
    /// `SmartList<Reminder> normal_reminders`.
    normal_reminders: Vec<Reminder>,
    /// `SmartList<Reminder> priority_reminders`.
    priority_reminders: Vec<Reminder>,
    /// `SmartList<Reminder> deferred_reminders`.
    deferred_reminders: Vec<Reminder>,
    /// `SmartList<UnitInfo> units_awaiting_reaction`.
    units_awaiting_reaction: Vec<UnitId>,
    /// Задачи юнитов (`UnitInfo::tasks`): `(юнит, [topmost, …])`.
    unit_tasks: Vec<(UnitId, Vec<u32>)>,
    /// `reminder_counter` — счётчик 2:1.
    reminder_counter: u32,
    /// `Task::task_id` — следующий id задачи.
    next_task_id: u32,
    /// Идёт ли разбор очереди (`ExecuteReminders`). Нужен, чтобы задача,
    /// планирующая себя на начало/конец хода **изнутри** обработчика, не
    /// образовывала live-lock: такие напоминания уходят в `deferred` и
    /// выполняются на следующем вызове (в C++ их ограничивал тайм-бюджет,
    /// DESIGN §5.1 риск 4).
    executing_reminders: bool,
}

/// Страховочный лимит напоминаний на один `ExecuteReminders`.
///
/// В C++ число напоминаний за кадр ограничено `TickTimer_HaveTimeToThink`;
/// в детерминированной модели очередь обрабатывается полностью, поэтому
/// лимит нужен только против патологического самовоспроизведения задач.
/// Он не должен достигаться: превышение — ошибка в логике задач.
pub const MAX_REMINDERS_PER_CALL: usize = 100_000;

impl TaskManager {
    /// `TaskManager::TaskManager` — пустой менеджер.
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            unit_requests: Vec::new(),
            normal_reminders: Vec::new(),
            priority_reminders: Vec::new(),
            deferred_reminders: Vec::new(),
            units_awaiting_reaction: Vec::new(),
            unit_tasks: Vec::new(),
            reminder_counter: 0,
            next_task_id: 1,
            executing_reminders: false,
        }
    }

    // -----------------------------------------------------------------------
    // Доступ к списку задач
    // -----------------------------------------------------------------------

    /// `TaskManager::GetTaskList`.
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// Количество задач (`tasks.GetCount()`).
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Задача по id.
    pub fn task(&self, id: u32) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    /// Задача по id (мутабельно).
    pub fn task_mut(&mut self, id: u32) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    /// Есть ли задача с таким id.
    pub fn has_task(&self, id: u32) -> bool {
        self.tasks.iter().any(|task| task.id == id)
    }

    /// Вносит изменённое состояние класса задачи (базовые поля задачи —
    /// авторитетны в менеджере, меняются через [`TaskCtx`]).
    pub fn write_kind(&mut self, task: Task) {
        if let Some(current) = self.tasks.iter_mut().find(|current| current.id == task.id) {
            current.kind = task.kind;
        }
    }

    /// Задачи запроса на юнитов (`unit_requests`).
    pub fn unit_requests(&self) -> &[u32] {
        &self.unit_requests
    }

    /// Все напоминания (для дайджеста/тестов): `(тип, очередь)`.
    pub fn reminder_types(&self) -> Vec<(u8, u8)> {
        let mut result = Vec::new();
        for reminder in &self.priority_reminders {
            result.push((0u8, reminder.get_type()));
        }
        for reminder in &self.normal_reminders {
            result.push((1u8, reminder.get_type()));
        }
        for reminder in &self.deferred_reminders {
            result.push((2u8, reminder.get_type()));
        }
        result
    }

    /// `TaskManager::GetRemindersCount`.
    pub fn reminders_count(&self) -> usize {
        self.normal_reminders.len() + self.priority_reminders.len() + self.deferred_reminders.len()
    }

    /// Количество напоминаний по типам (`REMINDER_TYPE_COUNT`).
    pub fn reminders_by_type(&self) -> [usize; REMINDER_TYPE_COUNT] {
        let mut counts = [0usize; REMINDER_TYPE_COUNT];
        for reminder in self
            .normal_reminders
            .iter()
            .chain(self.priority_reminders.iter())
            .chain(self.deferred_reminders.iter())
        {
            counts[reminder.get_type() as usize] += 1;
        }
        counts
    }

    // -----------------------------------------------------------------------
    // Задачи юнита (`UnitInfo::tasks`)
    // -----------------------------------------------------------------------

    fn unit_slot(&self, unit: UnitId) -> Option<usize> {
        self.unit_tasks.iter().position(|(id, _)| *id == unit)
    }

    /// `UnitInfo::GetTask` — верхняя задача юнита (индекс 0).
    pub fn unit_task(&self, unit: UnitId) -> Option<u32> {
        self.unit_slot(unit)
            .and_then(|slot| self.unit_tasks[slot].1.first().copied())
    }

    /// Все задачи юнита в порядке `PushFront` (новые первыми).
    pub fn unit_tasks_of(&self, unit: UnitId) -> Vec<u32> {
        self.unit_slot(unit)
            .map(|slot| self.unit_tasks[slot].1.clone())
            .unwrap_or_default()
    }

    /// `UnitInfo::AddTask` (`unitinfo.cpp:632`) — `PushFront`.
    pub fn attach_task_to_unit(&mut self, unit: UnitId, task: u32) {
        match self.unit_slot(unit) {
            Some(slot) => {
                if !self.unit_tasks[slot].1.contains(&task) {
                    self.unit_tasks[slot].1.insert(0, task);
                }
            }
            None => self.unit_tasks.push((unit, vec![task])),
        }
    }

    fn remove_unit_task_entry(&mut self, unit: UnitId, task: u32) -> bool {
        if let Some(slot) = self.unit_slot(unit) {
            let before = self.unit_tasks[slot].1.len();
            self.unit_tasks[slot].1.retain(|id| *id != task);
            let changed = self.unit_tasks[slot].1.len() != before;

            if self.unit_tasks[slot].1.is_empty() {
                self.unit_tasks.remove(slot);
            }

            changed
        } else {
            false
        }
    }

    /// `UnitInfo::RemoveTask` (`unitinfo.cpp:3739`) с уведомлением новой
    /// верхней задачи (если юнит готов принимать приказы и `mode`).
    pub fn detach_task_from_unit(
        &mut self,
        world: &mut GameState,
        task_id: u32,
        unit: UnitId,
        mode: bool,
    ) {
        let previous_top = self.unit_task(unit);

        if !self.remove_unit_task_entry(unit, task_id) {
            return;
        }

        let new_top = self.unit_task(unit);

        if previous_top != new_top && new_top.is_some() && mode {
            if task_is_ready_to_take_orders(world, unit) {
                self.remind_move_finished(world, unit, false);
            }
        }
    }

    /// `UnitInfo::RemoveTasks` (`unitinfo.cpp:1325`) — очистка и уведомление
    /// каждой задачи (`RemoveUnit`).
    pub fn remove_unit_tasks(&mut self, world: &mut GameState, unit: UnitId) {
        let tasks = self.unit_tasks_of(unit);
        for task_id in tasks {
            if let Some(task) = self.task(task_id) {
                let _ = task;
            }
            self.detach_task_from_unit(world, task_id, unit, false);
            self.task_remove_unit(world, task_id, unit);
        }
    }

    /// `UnitInfo::ClearUnitTasksAndRemindAvailable` (`task_manager.cpp:412`).
    pub fn clear_unit_tasks_and_remind_available(
        &mut self,
        world: &mut GameState,
        unit: UnitId,
        priority: bool,
    ) {
        self.remove_unit_tasks(world, unit);

        if let Some(mut info) = world.unit(unit).cloned() {
            info.ai_state_bits &= !crate::enums::AI_STATE_MOVE_FINISHED_REMINDER;
            world.write_unit(unit, info);
        }

        let should_remind = world
            .unit(unit)
            .map(|info| {
                info.hits > 0 && world.teams[info.team.0 as usize].team_type == TeamKind::Computer
            })
            .unwrap_or(false);

        if should_remind {
            self.append_reminder(world, Reminder::Available { unit }, priority);
        }
    }

    /// `UnitInfo::ScheduleDelayedTasks` (`unitinfo.cpp:645`).
    ///
    /// `delayed_tasks` (отложенные задачи) в max_core не заводятся отдельным
    /// списком (их ставит только `OrderContext`-слой через
    /// [`Self::remind_move_finished`]), поэтому портирована вторая половина:
    /// напоминание «движение завершено» верхней задаче юнита.
    pub fn schedule_delayed_tasks(&mut self, world: &mut GameState, unit: UnitId, priority: bool) {
        let enabled = world
            .unit(unit)
            .map(|info| info.tasks_enabled)
            .unwrap_or(false);

        if !enabled {
            return;
        }

        if self.unit_task(unit).is_some() {
            self.remind_move_finished(world, unit, priority);
        }
    }

    /// `Task_RemindMoveFinished` (`task.cpp:99`).
    pub fn remind_move_finished(&mut self, world: &mut GameState, unit: UnitId, priority: bool) {
        let already = world
            .unit(unit)
            .map(|info| info.ai_state_bits & crate::enums::AI_STATE_MOVE_FINISHED_REMINDER != 0)
            .unwrap_or(true);

        if already {
            return;
        }

        self.append_reminder(world, Reminder::MoveFinished { unit }, priority);
    }

    // -----------------------------------------------------------------------
    // Регистрация задач
    // -----------------------------------------------------------------------

    /// `TaskManager::AppendTask` (`task_manager.cpp:254`).
    pub fn append_task(&mut self, world: &mut GameState, mut task: Task) -> u32 {
        let id = self.next_task_id;
        self.next_task_id = self.next_task_id.wrapping_add(1);

        task.id = id;

        if task.parent.is_some() && !self.has_task(task.parent.unwrap()) {
            task.parent = None;
        }

        if task.get_type() == TASK_TYPE_OBTAIN_UNITS {
            self.unit_requests.push(id);
        }

        self.tasks.push(task);

        self.task_init(world, id);

        id
    }

    /// `TaskManager::RemoveTask` (`task_manager.cpp:603`).
    pub fn remove_task(&mut self, world: &mut GameState, task_id: u32) {
        let is_request = self
            .task(task_id)
            .map(|task| task.get_type() == TASK_TYPE_OBTAIN_UNITS)
            .unwrap_or(false);

        if is_request {
            self.unit_requests.retain(|id| *id != task_id);
        }

        if let Some(task) = self.task(task_id) {
            let _ = task_get_name(task.get_type());
        }

        // Снять задачу со всех юнитов (`UnitInfo::RemoveTask` без напоминания).
        let units: Vec<UnitId> = self
            .unit_tasks
            .iter()
            .filter(|(_, tasks)| tasks.contains(&task_id))
            .map(|(unit, _)| *unit)
            .collect();

        for unit in units {
            self.detach_task_from_unit(world, task_id, unit, false);
        }

        self.tasks.retain(|task| task.id != task_id);

        // Дочерние задачи теряют родителя (C++: `m_parent` — refcount-указатель).
        for task in &mut self.tasks {
            if task.parent == Some(task_id) {
                task.parent = None;
            }
        }

        // Напоминания удалённой задачи больше не исполняются.
        self.normal_reminders
            .retain(|reminder| !matches!(reminder, Reminder::TurnStart { task } | Reminder::TurnEnd { task } if *task == task_id));
        self.priority_reminders
            .retain(|reminder| !matches!(reminder, Reminder::TurnStart { task } | Reminder::TurnEnd { task } if *task == task_id));
        self.deferred_reminders
            .retain(|reminder| !matches!(reminder, Reminder::TurnStart { task } | Reminder::TurnEnd { task } if *task == task_id));
    }

    // -----------------------------------------------------------------------
    // Напоминания
    // -----------------------------------------------------------------------

    /// `TaskManager::AppendReminder` (`task_manager.cpp:266`).
    pub fn append_reminder(&mut self, world: &mut GameState, reminder: Reminder, priority: bool) {
        reminder.on_append(self, world);

        if priority {
            self.priority_reminders.push(reminder);
        } else {
            self.normal_reminders.push(reminder);
        }
    }

    /// `TaskManager::AppendDeferredReminder` (`task_manager.cpp:275`).
    pub fn append_deferred_reminder(&mut self, world: &mut GameState, reminder: Reminder) {
        reminder.on_append(self, world);
        self.deferred_reminders.push(reminder);
    }

    /// `TaskManager::ExecuteReminders` (`task_manager.cpp:277`).
    ///
    /// Порядок детерминирован: сначала `deferred` → `normal`, затем выбор из
    /// `priority`/`normal` по схеме «2 priority, затем 1 normal»
    /// (`reminder_counter >= 2 || priority_reminders.GetCount() == 0`).
    /// Берёт следующее напоминание по схеме C++: 2 priority, затем 1 normal
    /// (`reminder_counter >= 2 || priority_reminders.GetCount() == 0`).
    fn pop_next_reminder(&mut self) -> Option<Reminder> {
        if self.normal_reminders.is_empty() && self.priority_reminders.is_empty() {
            return None;
        }

        if self.normal_reminders.is_empty() {
            self.reminder_counter = 0;
        }

        if self.reminder_counter >= 2 || self.priority_reminders.is_empty() {
            self.reminder_counter = 0;

            if self.normal_reminders.is_empty() {
                None
            } else {
                Some(self.normal_reminders.remove(0))
            }
        } else {
            self.reminder_counter += 1;
            Some(self.priority_reminders.remove(0))
        }
    }

    /// Тайм-бюджет C++ не моделируется (DESIGN §5.1, риск 4).
    pub fn execute_reminders(&mut self, world: &mut GameState) -> bool {
        for reminder in std::mem::take(&mut self.deferred_reminders) {
            self.normal_reminders.push(reminder);
        }

        if self.normal_reminders.is_empty() && self.priority_reminders.is_empty() {
            return false;
        }

        self.executing_reminders = true;
        let mut processed = 0usize;

        while let Some(reminder) = self.pop_next_reminder() {
            reminder.execute(self, world);
            processed += 1;

            debug_assert!(
                processed <= MAX_REMINDERS_PER_CALL,
                "reminder live-lock: {processed} reminders in one ExecuteReminders"
            );

            if processed >= MAX_REMINDERS_PER_CALL {
                break;
            }
        }

        self.executing_reminders = false;

        true
    }

    // -----------------------------------------------------------------------
    // Жизненный цикл хода
    // -----------------------------------------------------------------------

    /// `TaskManager::BeginTurn` (`task_manager.cpp:337`).
    pub fn begin_turn(&mut self, _world: &mut GameState, team: TeamId) {
        for task in &mut self.tasks {
            if task.team == team && task.get_type() != TASK_TYPE_TRANSPORT {
                task.needs_processing = true;
            }
        }

        let mut schedule = Vec::new();
        for task in &self.tasks {
            if task.team == team && task.get_type() != TASK_TYPE_TRANSPORT {
                schedule.push(task.id);
            }
        }
        for task_id in schedule {
            self.remind_turn_start(task_id, false);
        }

        let mut schedule = Vec::new();
        for task in &self.tasks {
            if task.team == team && task.get_type() == TASK_TYPE_TRANSPORT {
                schedule.push(task.id);
            }
        }
        for task_id in schedule {
            self.remind_turn_start(task_id, false);
        }
    }

    /// `TaskManager::EndTurn` (`task_manager.cpp:377`).
    pub fn end_turn(&mut self, _world: &mut GameState, team: TeamId) {
        let mut schedule = Vec::new();
        for task in &self.tasks {
            if task.team == team && task.is_end_turn_needed() {
                schedule.push(task.id);
            }
        }

        for task_id in schedule {
            self.remind_turn_end(task_id, false);
        }
    }

    /// `TaskManager::MarkTasksForProcessing` (`task_manager.cpp:387`).
    pub fn mark_tasks_for_processing(&mut self, team: TeamId) {
        for task in &mut self.tasks {
            if task.team == team {
                task.needs_processing = true;
            }
        }
    }

    /// `Task::RemindTurnStart` (`task.cpp:418`).
    pub fn remind_turn_start(&mut self, task_id: u32, priority: bool) {
        let Some(task) = self.task_mut(task_id) else {
            return;
        };

        if task.scheduled_for_turn_start {
            return;
        }

        task.scheduled_for_turn_start = true;
        let reminder = Reminder::TurnStart { task: task_id };

        // Изнутри обработчика — на следующий вызов `ExecuteReminders`.
        if self.executing_reminders {
            self.deferred_reminders.push(reminder);
        } else if priority {
            self.priority_reminders.push(reminder);
        } else {
            self.normal_reminders.push(reminder);
        }
    }

    /// `Task::RemindTurnEnd` (`task.cpp:412`).
    pub fn remind_turn_end(&mut self, task_id: u32, priority: bool) {
        let Some(task) = self.task_mut(task_id) else {
            return;
        };

        if task.scheduled_for_turn_end {
            return;
        }

        task.scheduled_for_turn_end = true;
        let reminder = Reminder::TurnEnd { task: task_id };

        if self.executing_reminders {
            self.deferred_reminders.push(reminder);
        } else if priority {
            self.priority_reminders.push(reminder);
        } else {
            self.normal_reminders.push(reminder);
        }
    }

    /// `TaskManager::Clear` (`task_manager.cpp:397`).
    pub fn clear(&mut self, world: &mut GameState) {
        let ids: Vec<u32> = self.tasks.iter().map(|task| task.id).collect();

        for id in ids {
            self.task_remove_self(world, id);
        }

        self.tasks.clear();
        self.unit_requests.clear();
        self.normal_reminders.clear();
        self.priority_reminders.clear();
        self.deferred_reminders.clear();
        self.units_awaiting_reaction.clear();
        self.unit_tasks.clear();
        self.reminder_counter = 0;
    }

    // -----------------------------------------------------------------------
    // Реакции
    // -----------------------------------------------------------------------

    /// `TaskManager::AreTasksThinking` (`task_manager.cpp:131`).
    pub fn are_tasks_thinking(&self, world: &GameState, team: TeamId) -> bool {
        self.tasks
            .iter()
            .any(|task| task.team == team && task.is_thinking(world))
    }

    /// Есть ли у команды незавершённая работа — **диагностика** (QA/логи), не
    /// решение о конце хода.
    ///
    /// `AiPlayer::CheckEndTurn` использует не это, а флаг `tasks_pending`
    /// (`Ai_SetTasksPendingFlag`/`Ai_ClearTasksPendingFlags`): статическая
    /// проверка «есть незавершённые задачи» держала ход ИИ вечно, если юнит
    /// израсходовал скорость, а задача ждала следующего хода.
    pub fn has_pending_work(&self, world: &GameState, team: TeamId) -> bool {
        if !self.normal_reminders.is_empty()
            || !self.priority_reminders.is_empty()
            || !self.deferred_reminders.is_empty()
        {
            return true;
        }

        self.tasks.iter().any(|task| {
            if task.team != team {
                return false;
            }

            match &task.kind {
                super::tasks::TaskKind::Attack(_)
                | super::tasks::TaskKind::KillUnit(_)
                | super::tasks::TaskKind::FrontalAttack { .. } => true,
                super::tasks::TaskKind::CreateBuilding { .. }
                | super::tasks::TaskKind::CreateUnit { .. }
                | super::tasks::TaskKind::ObtainUnits { .. } => task.kind.is_needed_kind(world),
                _ => false,
            }
        })
    }

    /// `TaskManager_IsReadyToReact` (`task_manager.cpp:198`).
    pub fn is_ready_to_react(world: &GameState, unit: UnitId) -> bool {
        let Some(info) = world.unit(unit) else {
            return false;
        };

        world.is_active_turn(info.team)
            && info.speed > 0
            && task_is_ready_to_take_orders(world, unit)
    }

    /// `TaskManager::CheckComputerReactions` (`task_manager.cpp:143`).
    pub fn check_computer_reactions(&mut self, world: &mut GameState) {
        if world.play_mode == crate::enums::PlayMode::Unknown {
            return;
        }

        let active_team_computer =
            world.teams[world.active_turn_team.0 as usize].team_type == TeamKind::Computer;

        if world.play_mode != crate::enums::PlayMode::TurnBased || active_team_computer {
            let ids: Vec<u32> = self.tasks.iter().map(|task| task.id).collect();

            for task_id in ids {
                let Some(team) = self.task(task_id).map(|task| task.team) else {
                    continue;
                };

                if world.is_active_turn(team) && self.task_check_reactions(world, task_id) {
                    return;
                }
            }

            if world.ai.reaction_state == super::AiReactionState::Idle {
                let awaiting = std::mem::take(&mut self.units_awaiting_reaction);

                for unit in awaiting {
                    let finished = world
                        .unit(unit)
                        .map(|info| {
                            info.hits == 0
                                || world.teams[info.team.0 as usize].team_type != TeamKind::Computer
                                || Self::is_ready_to_react(world, unit)
                        })
                        .unwrap_or(true);

                    if finished {
                        self.append_reminder(world, Reminder::Retreat { unit }, true);
                    } else {
                        self.units_awaiting_reaction.push(unit);
                    }
                }
            }
        }
    }

    /// `TaskManager::CheckUnitReaction` (`task_manager.cpp:173`).
    pub fn check_unit_reaction(&mut self, world: &mut GameState, unit: UnitId) {
        let Some(info) = world.unit(unit).cloned() else {
            return;
        };

        let is_computer = world.teams[info.team.0 as usize].team_type == TeamKind::Computer;

        if info.hits > 0 && is_computer {
            if Self::is_ready_to_react(world, unit) {
                if let Some(mut info) = world.unit(unit).cloned() {
                    info.ai_state_bits &= !crate::enums::AI_STATE_RETREAT_REMINDER;
                    world.write_unit(unit, info);
                }

                let caution = super::ai_determine_caution_level(world, unit);

                match self.unit_task(unit) {
                    Some(task_id) => {
                        super::task::task_retreat_from_danger(self, world, task_id, unit, caution);
                    }
                    None => {
                        super::task::task_retreat_if_necessary(self, world, None, unit, caution);
                    }
                }
            } else {
                self.units_awaiting_reaction.push(unit);
            }
        } else if let Some(mut info) = world.unit(unit).cloned() {
            info.ai_state_bits &= !crate::enums::AI_STATE_RETREAT_REMINDER;
            world.write_unit(unit, info);
        }
    }

    /// `TaskManager::EnqueueUnitForReactionCheck` (`task_manager.cpp:232`).
    pub fn enqueue_unit_for_reaction_check(&mut self, world: &mut GameState, unit: UnitId) {
        let Some(info) = world.unit(unit) else {
            return;
        };

        if info.ai_state_bits & crate::enums::AI_STATE_RETREAT_REMINDER != 0 {
            return;
        }

        if info
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_SPEED)
            > 0
        {
            self.append_reminder(world, Reminder::Retreat { unit }, true);
        }
    }

    /// `TaskManager::CollectPotentialAttackTargets` (`task_manager.cpp:202`).
    pub fn collect_potential_attack_targets(&mut self, world: &mut GameState, unit: UnitId) {
        let Some(attacker) = world.unit(unit).cloned() else {
            return;
        };

        if attacker.ammo == 0 {
            return;
        }

        let mut range = attacker
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_RANGE);
        range *= range;

        let target_types = crate::access::valid_attack_target_types(attacker.unit_type);

        if target_types & unit_flags::MOBILE_LAND_UNIT != 0 {
            let ids: Vec<UnitId> = world.unit_list(UnitListKind::MobileLandSea).to_vec();
            for other_id in ids {
                let Some(other) = world.unit(other_id) else {
                    continue;
                };
                if other.team != attacker.team
                    && other.is_visible_to_team(attacker.team)
                    && world.teams[other.team.0 as usize].team_type == TeamKind::Computer
                    && crate::access::squared_distance(other.grid, attacker.grid) <= range
                {
                    self.enqueue_unit_for_reaction_check(world, other_id);
                }
            }
        }

        if target_types & unit_flags::MOBILE_AIR_UNIT != 0 {
            let ids: Vec<UnitId> = world.unit_list(UnitListKind::MobileAir).to_vec();
            for other_id in ids {
                let Some(other) = world.unit(other_id) else {
                    continue;
                };
                if other.team != attacker.team
                    && other.is_visible_to_team(attacker.team)
                    && world.teams[other.team.0 as usize].team_type == TeamKind::Computer
                    && crate::access::squared_distance(other.grid, attacker.grid) <= range
                {
                    self.enqueue_unit_for_reaction_check(world, other_id);
                }
            }
        }
    }

    /// `TaskManager::RemoveDestroyedUnit` (`task_manager.cpp:613`).
    pub fn remove_destroyed_unit(&mut self, world: &mut GameState, unit: UnitId) {
        self.units_awaiting_reaction.retain(|id| *id != unit);

        let ids: Vec<u32> = self.tasks.iter().map(|task| task.id).collect();
        for task_id in ids {
            self.task_event_unit_destroyed(world, task_id, unit);
        }

        self.unit_tasks.retain(|(id, _)| *id != unit);
    }

    /// `TaskManager::AddSpottedUnit` (`task_manager.cpp:619`).
    pub fn add_spotted_unit(&mut self, world: &mut GameState, unit: UnitId) {
        let ids: Vec<u32> = self.tasks.iter().map(|task| task.id).collect();
        for task_id in ids {
            self.task_event_enemy_unit_spotted(world, task_id, unit);
        }
    }

    // -----------------------------------------------------------------------
    // Производство
    // -----------------------------------------------------------------------

    /// `TaskManager::CreateBuilding` (`task_manager.cpp:240`).
    pub fn create_building(
        &mut self,
        world: &mut GameState,
        unit_type: UnitType,
        team: TeamId,
        site: Point,
        task: u32,
    ) {
        let priority = self.task(task).map(|task| task.base_priority).unwrap_or(0);

        if self.is_unit_needed(world, unit_type, team, priority) {
            super::strategy::create_building(world, team, unit_type, site, Some(task));
        }
    }

    /// `TaskManager::CreateUnit` (`task_manager.cpp:246`).
    pub fn create_unit(
        &mut self,
        world: &mut GameState,
        unit_type: UnitType,
        team: TeamId,
        site: Point,
        task: u32,
    ) {
        let priority = self.task(task).map(|task| task.base_priority).unwrap_or(0);

        if self.is_unit_needed(world, unit_type, team, priority) {
            // Строитель, который по типу должен построить сам себя
            // (`CONSTRCT`/`MASTER_BUILDER`), заводом не производится: запрос
            // отсекается, иначе цепочка `TaskCreateUnit → TaskObtainUnits →
            // TaskCreateUnit` не завершится.
            if super::tasks::create::builder_type_for(unit_type) == unit_type {
                return;
            }

            let parent = Some(task);
            self.append_task(world, {
                let kind = TaskKind::create_unit(unit_type, site);
                Task::new(0, team, parent, priority, kind)
            });
        }
    }

    /// `TaskManager::IsUnitNeeded` (`task_manager.cpp:51`).
    pub fn is_unit_needed(
        &mut self,
        world: &GameState,
        unit_type: UnitType,
        team: TeamId,
        task_priority: u16,
    ) -> bool {
        let turns_till_mission_end = task_estimate_turns_till_mission_end(world);
        let current = world.team_units[team.0 as usize].current_unit_values(unit_type as u16);
        let turns_to_build = current.get_attribute(crate::unit_values::ATTRIB_TURNS);

        if turns_till_mission_end < turns_to_build + 5 {
            return false;
        }

        if unit_type == UnitType::Constructor && turns_till_mission_end < 25 {
            return false;
        }

        let list_kind = if !unit_type.is_mobile() {
            UnitListKind::Stationary
        } else {
            UnitListKind::MobileLandSea
        };

        let mut available_count = 0;
        for id in world.unit_list(list_kind) {
            let Some(unit) = world.unit(*id) else {
                continue;
            };
            if unit.team == team && unit.unit_type == unit_type {
                available_count += 1;

                if self.unit_task(*id).is_none() {
                    return false;
                }
            }
        }

        let mut requested_count = 0;
        const CENSUS_SLACK: u16 = TASK_PRIORITY_OFFSET_CENSUS_SLACK;

        for request_id in &self.unit_requests {
            let Some(request) = self.task(*request_id) else {
                continue;
            };
            let count = request.kind.count_instances_of_unit_type(unit_type);

            if request.team == team
                && count > 0
                && request.compare_priority(world, task_priority.wrapping_add(CENSUS_SLACK)) <= 0
            {
                requested_count += count;
            }
        }

        for task in &self.tasks {
            if task.team != team {
                continue;
            }

            let task_type = task.get_type();
            if task_type != TASK_TYPE_CREATE_BUILDING && task_type != TASK_TYPE_CREATE_UNIT {
                continue;
            }

            if task.kind.create_unit_type() != Some(unit_type) {
                continue;
            }

            if task.kind.is_actively_building()
                || task.compare_priority(world, task_priority.wrapping_add(CENSUS_SLACK)) <= 0
            {
                return false;
            }
        }

        if requested_count >= available_count {
            if available_count <= 0 {
                return true;
            }

            !super::task_manager_need_to_reserve_raw_materials(world, team)
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // Назначение задач юнитам
    // -----------------------------------------------------------------------

    /// `TaskManager::FindTaskForUnit` (`task_manager.cpp:430`).
    pub fn find_task_for_unit(&mut self, world: &mut GameState, unit: UnitId) {
        let Some(info) = world.unit(unit).cloned() else {
            return;
        };

        if world.teams[info.team.0 as usize].team_type != TeamKind::Computer {
            self.remove_unit_tasks(world, unit);
            return;
        }

        let station_ok = info.is_mobile()
            || info
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_AMMO)
                != 0
            || matches!(
                info.unit_type,
                UnitType::LightVehiclePlant
                    | UnitType::HeavyVehiclePlant
                    | UnitType::Shipyard
                    | UnitType::AirUnitsPlant
                    | UnitType::TrainingHall
            );

        if !station_ok {
            return;
        }

        if self.unit_task(unit).is_some() || info.hits == 0 {
            return;
        }

        // Юнит «в трюме» (завод/транспорт: `ORDER_IDLE` + `ORDER_STATE_STORE`)
        // задач не получает: C++ такие юниты не видит ни в одном цикле выдачи
        // задач (`GetOrder() != ORDER_IDLE` в `AiPlayer`), а выводит их
        // `UnitsManager_ActivateUnit` по приказу `TaskCreateUnit`/`TaskActivate`
        // (`unitinfo.cpp:3489-3520`, `taskcreateunit.cpp:147-155`).
        if info.orders == UnitOrder::Idle && info.state == UnitOrderState::Store {
            return;
        }

        // Отключённый юнит задачу не получает (C++: `ORDER_DISABLE`/`IDLE` после него).
        if info.orders == UnitOrder::Disable
            || (info.orders == UnitOrder::Idle && info.prior_orders == UnitOrder::Disable)
        {
            return;
        }

        let opponent = world.ai.settings.opponent;

        if opponent >= Opponent::Apprentice as u8
            && info.is_mobile()
            && i32::from(info.hits)
                != info
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_HITS)
        {
            let kind = TaskKind::repair(unit);
            self.append_task(
                world,
                Task::new(0, info.team, None, super::task::TASK_PRIORITY_REPAIR, kind),
            );
            return;
        }

        if i32::from(info.ammo)
            < info
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_ROUNDS)
        {
            let kind = TaskKind::reload(unit);
            self.append_task(
                world,
                Task::new(0, info.team, None, super::task::TASK_PRIORITY_REPAIR, kind),
            );
            return;
        }

        // `ExchangeOperator` — обмен оператора здания/комплекса.
        if info.environment().is_some() {
            let ids: Vec<u32> = self.tasks.iter().map(|task| task.id).collect();
            for task_id in ids {
                if self.task(task_id).map(|task| task.team) != Some(info.team) {
                    continue;
                }

                if self.task_exchange_operator(world, task_id, unit) {
                    return;
                }
            }
        }

        // Запросы на юнитов (`unit_requests`): подходящий по типу и срочности.
        let mut obtain_units_task: Option<u32> = None;
        let mut task_priority = u16::MAX;

        for request_id in self.unit_requests.clone() {
            let Some(request) = self.task(request_id) else {
                continue;
            };

            if request.team != info.team {
                continue;
            }

            if obtain_units_task
                .map(|_best| request.compare_priority(world, task_priority) < 0)
                .unwrap_or(true)
                && request.kind.count_instances_of_unit_type(info.unit_type) > 0
                && request.kind.is_needed_kind(world)
            {
                obtain_units_task = Some(request_id);
                task_priority = request.kind.get_priority(world, request);
            }
        }

        if let Some(request_id) = obtain_units_task {
            self.task_add_unit(world, request_id, unit);
            return;
        }

        // Лучшая задача по расстоянию до её `GetBounds().ul` (порядок задач).
        let mut best_task: Option<u32> = None;
        let mut minimum_distance = i32::MAX;

        for task in &self.tasks {
            if task.team != info.team {
                continue;
            }

            if !self.task_is_unit_usable(world, task.id, unit) {
                continue;
            }

            let bounds = self.task_get_bounds(world, task.id);
            let dx = i32::from(info.grid.x) - bounds.ulx;
            let dy = i32::from(info.grid.y) - bounds.uly;
            let distance = dx * dx + dy * dy;

            if best_task.is_none() || distance < minimum_distance {
                best_task = Some(task.id);
                minimum_distance = distance;
            }
        }

        if let Some(task_id) = best_task {
            self.task_add_unit(world, task_id, unit);
        }
    }

    // -----------------------------------------------------------------------
    // Делегирование в классы задач (`ai/tasks/*`)
    // -----------------------------------------------------------------------

    fn with_task_kind<R>(
        &mut self,
        world: &mut GameState,
        task_id: u32,
        f: impl FnOnce(&mut TaskKind, &mut TaskCtx<'_>, super::task::TaskHeader) -> R,
    ) -> Option<R> {
        let task = self.task(task_id).cloned()?;
        let header = task.header();

        let mut task = task;
        let result = {
            let mut ctx = TaskCtx { tm: self, world };
            f(&mut task.kind, &mut ctx, header)
        };

        self.write_kind(task);

        Some(result)
    }

    fn task_init(&mut self, world: &mut GameState, task_id: u32) {
        self.with_task_kind(world, task_id, |kind, ctx, header| kind.init(ctx, header));
    }

    /// `Task::BeginTurn`.
    pub fn task_begin_turn(&mut self, world: &mut GameState, task_id: u32) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.begin_turn(ctx, header)
        });
    }

    /// `Task::EndTurn`.
    pub fn task_end_turn(&mut self, world: &mut GameState, task_id: u32) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.end_turn(ctx, header)
        });
    }

    /// `Task::Execute(unit)` — возвращает `true`, если задача что-то сделала.
    pub fn task_execute(&mut self, world: &mut GameState, task_id: u32, unit: UnitId) -> bool {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.execute(ctx, header, unit)
        })
        .unwrap_or(false)
    }

    /// `Task::AddUnit`.
    pub fn task_add_unit(&mut self, world: &mut GameState, task_id: u32, unit: UnitId) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.add_unit(ctx, header, unit)
        });
    }

    /// `Task::RemoveUnit`.
    pub fn task_remove_unit(&mut self, world: &mut GameState, task_id: u32, unit: UnitId) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.remove_unit(ctx, header, unit)
        });
    }

    /// `Task::RemoveSelf`.
    pub fn task_remove_self(&mut self, world: &mut GameState, task_id: u32) {
        let _ = self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.remove_self(ctx, header)
        });
        self.remove_task(world, task_id);
    }

    /// `Task::ChildComplete`.
    pub fn task_child_complete(&mut self, world: &mut GameState, task_id: u32, child: u32) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.child_complete(ctx, header, child)
        });
    }

    /// Уведомление родителя о завершении `TaskMove`.
    pub fn task_move_finished(
        &mut self,
        world: &mut GameState,
        task_id: u32,
        unit: UnitId,
        result: i8,
    ) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.move_finished(ctx, header, unit, result)
        });
    }

    /// `Task::CheckReactions`.
    pub fn task_check_reactions(&mut self, world: &mut GameState, task_id: u32) -> bool {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.check_reactions(ctx, header)
        })
        .unwrap_or(false)
    }

    /// `Task::ExchangeOperator`.
    pub fn task_exchange_operator(
        &mut self,
        world: &mut GameState,
        task_id: u32,
        unit: UnitId,
    ) -> bool {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.exchange_operator(ctx, header, unit)
        })
        .unwrap_or(false)
    }

    /// `Task::EventUnitDestroyed`.
    pub fn task_event_unit_destroyed(&mut self, world: &mut GameState, task_id: u32, unit: UnitId) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.event_unit_destroyed(ctx, header, unit)
        });
    }

    /// `Task::EventEnemyUnitSpotted`.
    pub fn task_event_enemy_unit_spotted(
        &mut self,
        world: &mut GameState,
        task_id: u32,
        unit: UnitId,
    ) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.event_enemy_unit_spotted(ctx, header, unit)
        });
    }

    /// `Task::EventCargoTransfer`.
    pub fn task_event_cargo_transfer(&mut self, world: &mut GameState, task_id: u32, unit: UnitId) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.event_cargo_transfer(ctx, header, unit)
        });
    }

    /// `Task::EventUnitLoaded`.
    pub fn task_event_unit_loaded(
        &mut self,
        world: &mut GameState,
        task_id: u32,
        unit1: UnitId,
        unit2: UnitId,
    ) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.event_unit_loaded(ctx, header, unit1, unit2)
        });
    }

    /// `Task::EventUnitUnloaded`.
    pub fn task_event_unit_unloaded(
        &mut self,
        world: &mut GameState,
        task_id: u32,
        unit1: UnitId,
        unit2: UnitId,
    ) {
        self.with_task_kind(world, task_id, |kind, ctx, header| {
            kind.event_unit_unloaded(ctx, header, unit1, unit2)
        });
    }

    /// `Task::IsUnitUsable`.
    pub fn task_is_unit_usable(&self, world: &GameState, task_id: u32, unit: UnitId) -> bool {
        match self.task(task_id) {
            Some(task) => {
                let header = task.header();
                task.kind.is_unit_usable(world, header, unit)
            }
            None => false,
        }
    }

    /// `Task::GetBounds`.
    pub fn task_get_bounds(&self, world: &GameState, task_id: u32) -> crate::unit::Rect {
        match self.task(task_id) {
            Some(task) => task.kind.get_bounds(world, task.header()),
            None => crate::unit::Rect::default(),
        }
    }

    /// Дайджест состояния ИИ для тестов детерминизма (DESIGN §6, заготовка `crc.rs`).
    pub fn digest_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(self.tasks.len() as u32).to_le_bytes());
        for task in &self.tasks {
            out.extend_from_slice(&task.id.to_le_bytes());
            out.push(task.get_type());
            out.push(task.team.0);
            out.extend_from_slice(&task.base_priority.to_le_bytes());
            out.extend_from_slice(&task.parent.unwrap_or(u32::MAX).to_le_bytes());
            out.push(u8::from(task.needs_processing));
            out.push(u8::from(task.scheduled_for_turn_start));
            out.push(u8::from(task.scheduled_for_turn_end));
        }

        for queue in [
            &self.priority_reminders,
            &self.normal_reminders,
            &self.deferred_reminders,
        ] {
            out.extend_from_slice(&(queue.len() as u32).to_le_bytes());
            for reminder in queue {
                out.push(reminder.get_type());
                match reminder {
                    Reminder::TurnStart { task } | Reminder::TurnEnd { task } => {
                        out.extend_from_slice(&task.to_le_bytes());
                    }
                    Reminder::Available { unit }
                    | Reminder::MoveFinished { unit }
                    | Reminder::Attack { unit }
                    | Reminder::Retreat { unit } => {
                        out.extend_from_slice(&unit.0.to_le_bytes());
                    }
                }
            }
        }

        out.extend_from_slice(&(self.unit_tasks.len() as u32).to_le_bytes());
        for (unit, tasks) in &self.unit_tasks {
            out.extend_from_slice(&unit.0.to_le_bytes());
            out.extend_from_slice(&(tasks.len() as u32).to_le_bytes());
            for task in tasks {
                out.extend_from_slice(&task.to_le_bytes());
            }
        }

        out.push(self.reminder_counter as u8);
        out.extend_from_slice(&self.next_task_id.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
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

        // `append_task` вызывает `Task::Init` → `RemindTurnStart(true)`.
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

        // 4 priority + 3 normal: ожидаемая последовательность P,P,N,P,P,N,N.
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
}
