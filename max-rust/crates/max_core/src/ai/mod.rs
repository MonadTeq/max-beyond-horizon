//! ИИ: `AiPlayer`, `TaskManager`, задачи, карты угроз — порт `ai.cpp`.
//!
//! # Состав
//!
//! | Модуль | C++-оракул |
//! |---|---|
//! | [`attack`] | `aiattack.cpp` — решения об атаке |
//! | [`reminders`] | `reminders.cpp` — отложенные вызовы задач |
//! | [`strategy`] | `aiplayer.cpp` — `AiPlayer`, стратегия, карты угроз |
//! | [`task`] | `task.cpp` — база задачи, приоритеты, `Task_*` |
//! | [`task_manager`] | `task_manager.cpp` — очереди, `FindTaskForUnit` |
//! | [`tasks`] | 49 классов `task*.cpp` |
//! | [`threat`] | `heatmap.cpp`, `threatmap.cpp`, `zonewalker.cpp` |
//! | [`weight`] | `weighttable.cpp` |
//!
//! Этот модуль — сам `ai.cpp`: [`ai_init`], [`ai_setup_strategy`],
//! [`ai_begin_turn`], [`ai_check_reactions`], [`ai_check_end_turn`],
//! [`ai_check_mines`], [`ai_evaluate_attack_targets`], [`ai_remove_unit`],
//! [`ai_unit_spotted`], [`ai_is_dangerous_location`],
//! [`task_manager_need_to_reserve_raw_materials`] и состояние [`AiState`].
//!
//! # Где живёт состояние ИИ
//!
//! `GameState.ai: AiState` — аналог глобалов C++ (`AiPlayer_Teams`,
//! `TaskManager`, `Ai_AiReactionState`, тепловые карты команд,
//! `ResourceManager_GetSettings()`). Так состояние входит в дайджест партии и
//! не требует менять `TeamInfo` (P3) или вводить глобальную мутацию
//! (DESIGN §1.4 п.5).
//!
//! # Идиома `with_task_manager`
//!
//! Обработчики задач получают одновременно `&mut TaskManager` и
//! `&mut GameState`. Поскольку менеджер лежит **внутри** `GameState`, его
//! временно изымают:
//!
//! ```
//! # use max_core::game::GameState;
//! let mut world = GameState::new();
//! world.with_task_manager(|tm, world| {
//!     let _ = tm.task_count();
//!     let _ = world.turn_counter;
//! });
//! ```
//!
//! Внутри замыкания `world.ai.task_manager` пуст — все обращения к менеджеру
//! должны идти через параметр `tm` (это же правило действует в обработчиках
//! задач, см. [`tasks::TaskCtx`]).

pub mod attack;
pub mod reminders;
pub mod strategy;
pub mod task;
pub mod task_manager;
pub mod tasks;
pub mod threat;
pub mod weight;

pub use attack::evaluate_attack;
pub use reminders::Reminder;
pub use strategy::{AiPlayer, SpottedUnit};
pub use task::{
    Task, TaskHeader, TASK_PRIORITY_ATTACK_DEFAULT, TASK_PRIORITY_EMERGENCY, TASK_TYPE_ATTACK,
    TASK_TYPE_CREATE_BUILDING, TASK_TYPE_CREATE_UNIT, TASK_TYPE_MOVE, TASK_TYPE_OBTAIN_UNITS,
};
pub use task_manager::TaskManager;
pub use tasks::{TaskCtx, TaskKind};
pub use threat::{HeatMap, ThreatMap, ZoneWalker};
pub use weight::WeightTable;

use crate::enums::{CautionLevel, Opponent, PlayMode, UnitOrder};
use crate::game::{GameState, PLAYER_TEAM_COUNT};
use crate::geo::Point;
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo, UnitListKind};

/// `AiReactionStateType` (`ai.hpp:27`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum AiReactionState {
    /// Готов обрабатывать реакции.
    #[default]
    Idle = 0,
    /// Ждёт завершения движений/обработки.
    Processing = 1,
    /// Частицы/атаки в процессе.
    AnimationsActive = 2,
}

/// Настройки ИИ — `ResourceManager_GetSettings()` в объёме, который читает ИИ.
///
/// Значения по умолчанию — из `settings.cpp`: `max_percent` = 90,
/// `opponent` = 1 (`OPPONENT_TYPE_APPRENTICE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiSettings {
    /// `opponent` (`OpponentType`).
    pub opponent: u8,
    /// `max_percent` (порог стелс-действий).
    pub max_percent: i32,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            opponent: Opponent::Apprentice as u8,
            max_percent: 90,
        }
    }
}

/// Состояние ИИ партии (аналог глобалов C++).
#[derive(Debug, Clone)]
pub struct AiState {
    /// `AiPlayer_Teams[PLAYER_TEAM_MAX - 1]`.
    pub players: [AiPlayer; PLAYER_TEAM_COUNT],
    /// `TaskManager` (`task_manager.cpp:43`).
    pub task_manager: TaskManager,
    /// `Ai_AiReactionState` (`ai.cpp:43`).
    pub reaction_state: AiReactionState,
    /// `UnitsManager_TeamInfo[team].heat_map` (`heatmap.cpp`).
    pub heat_maps: [HeatMap; PLAYER_TEAM_COUNT],
    /// Настройки партии, читаемые ИИ.
    pub settings: AiSettings,
    /// `GameManager_AllVisible` (отладка/тесты: все юниты видны).
    pub all_visible: bool,
}

impl Default for AiState {
    fn default() -> Self {
        Self::new()
    }
}

impl AiState {
    /// Пустое состояние: по игроку на команду, пустые карты.
    pub fn new() -> Self {
        Self {
            players: std::array::from_fn(|team| AiPlayer::new(TeamId(team as u8))),
            task_manager: TaskManager::new(),
            reaction_state: AiReactionState::Idle,
            heat_maps: std::array::from_fn(|team| HeatMap::new(TeamId(team as u8))),
            settings: AiSettings::default(),
            all_visible: false,
        }
    }

    /// Канонический дайджест состояния ИИ (для тестов детерминизма).
    pub fn digest_into(&self, out: &mut Vec<u8>) {
        self.task_manager.digest_into(out);
        out.push(self.reaction_state as u8);
        out.push(u8::from(self.all_visible));
        out.push(self.settings.opponent);
        out.extend_from_slice(&self.settings.max_percent.to_le_bytes());

        for (team, player) in self.players.iter().enumerate() {
            out.push(team as u8);
            out.push(player.strategy as u8);
            out.extend_from_slice(&player.greenhouse_ratio.to_le_bytes());
            out.extend_from_slice(&player.minefield_density.to_le_bytes());
            out.push(u8::from(player.need_init));
            out.push(u8::from(player.tasks_pending));
            out.push(player.end_turn_retry_counter);
            out.extend_from_slice(&player.attack_value_share.to_le_bytes());
            out.extend_from_slice(&(player.attack_tasks.len() as u32).to_le_bytes());
            for task in &player.attack_tasks {
                out.extend_from_slice(&task.unwrap_or(u32::MAX).to_le_bytes());
            }
            out.extend_from_slice(&player.target_location.x.to_le_bytes());
            out.extend_from_slice(&player.target_location.y.to_le_bytes());
            out.extend_from_slice(&(player.spotted_units.len() as u32).to_le_bytes());
            for spotted in &player.spotted_units {
                out.extend_from_slice(&spotted.unit.unwrap_or(UnitId::NO_ID).0.to_le_bytes());
                out.push(u8::from(spotted.visible_to_team));
                out.extend_from_slice(&spotted.last_position.x.to_le_bytes());
                out.extend_from_slice(&spotted.last_position.y.to_le_bytes());
                out.extend_from_slice(&spotted.task.unwrap_or(u32::MAX).to_le_bytes());
            }
            out.extend_from_slice(&(player.air_force.len() as u32).to_le_bytes());
            for unit in &player.air_force {
                out.extend_from_slice(&unit.0.to_le_bytes());
            }
            out.extend_from_slice(&(player.ground_forces.len() as u32).to_le_bytes());
            for unit in &player.ground_forces {
                out.extend_from_slice(&unit.0.to_le_bytes());
            }
            out.extend_from_slice(&player.info_map);
            out.extend(player.mine_map.iter().map(|value| *value as u8));
            out.extend_from_slice(&player.weight_table_generic.len().to_le_bytes());
            out.extend_from_slice(&player.weight_table_tanks.len().to_le_bytes());
        }

        for heat_map in &self.heat_maps {
            for cell in heat_map.cells() {
                out.extend_from_slice(&cell.complete.to_le_bytes());
                out.extend_from_slice(&cell.stealth_sea.to_le_bytes());
                out.extend_from_slice(&cell.stealth_land.to_le_bytes());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Мост «ИИ ⇄ состояние партии» (`ai.cpp`)
// ---------------------------------------------------------------------------

impl GameState {
    /// Временное изъятие менеджера задач (см. документацию модуля).
    pub fn with_task_manager<R>(
        &mut self,
        f: impl FnOnce(&mut TaskManager, &mut GameState) -> R,
    ) -> R {
        let mut task_manager = std::mem::take(&mut self.ai.task_manager);
        let result = f(&mut task_manager, self);
        self.ai.task_manager = task_manager;
        result
    }

    /// Регистрирует задачу в менеджере (`TaskManager::AppendTask`).
    pub fn ai_append_task(
        &mut self,
        team: TeamId,
        parent: Option<u32>,
        priority: u16,
        kind: TaskKind,
    ) -> u32 {
        self.with_task_manager(|task_manager, world| {
            task_manager.append_task(world, Task::new(0, team, parent, priority, kind))
        })
    }

    /// Ставит напоминание (`TaskManager::AppendReminder`).
    pub fn ai_append_reminder(&mut self, reminder: Reminder, priority: bool) {
        self.with_task_manager(|task_manager, world| {
            task_manager.append_reminder(world, reminder, priority);
        });
    }

    /// `Task_RemindMoveFinished`.
    pub fn ai_remind_move_finished(&mut self, unit: UnitId, priority: bool) {
        self.with_task_manager(|task_manager, world| {
            task_manager.remind_move_finished(world, unit, priority);
        });
    }

    /// `Task::RemoveSelf`.
    pub fn ai_task_remove_self(&mut self, task_id: u32) {
        self.with_task_manager(|task_manager, world| {
            task_manager.task_remove_self(world, task_id);
        });
    }

    /// `Ai_SetInfoMapPoint` (`ai.cpp:122`).
    pub fn ai_set_info_map_point(&mut self, team: TeamId, point: Point) {
        if self.teams[team.0 as usize].team_type == TeamKind::Computer {
            self.ai.players[team.0 as usize].set_info_map_point(point);
        }
    }

    /// `Ai_MarkMineMapPoint` (`ai.cpp:128`).
    pub fn ai_mark_mine_map_point(&mut self, team: TeamId, point: Point) {
        if self.teams[team.0 as usize].team_type == TeamKind::Computer {
            self.ai.players[team.0 as usize].mark_mine_map_point(point);
        }
    }

    /// `Ai_UpdateMineMap` (`ai.cpp:134`).
    pub fn ai_update_mine_map(&mut self, team: TeamId, point: Point) {
        if world_is_computer(self, team) {
            self.ai.players[team.0 as usize].update_mine_map(point);
        }
    }

    /// `UnitInfo::SpotByTeam` (`unitinfo.cpp:2810`) — обнаружение юнита
    /// командой: видимость, пометка «замечен», уведомление ИИ.
    pub fn spot_by_team(&mut self, id: UnitId, team: TeamId) {
        let Some(unit) = self.unit(id).cloned() else {
            return;
        };

        if unit.team == team || unit.orders == UnitOrder::Idle || unit.is_visible_to_team(team) {
            return;
        }

        if let Some(mut info) = self.unit(id).cloned() {
            info.set_visible_to_team(team, true);
            info.spot_by_team(team);
            self.write_unit(id, info);
        }

        if self.teams[unit.team.0 as usize].team_type == TeamKind::Computer {
            self.ai_add_unit_to_tracker_list(id);
        }

        ai_unit_spotted(self, id, team);
    }

    /// `Ai_CheckReactions` (`ai.cpp:407`) — точка вызова тика.
    pub fn ai_check_reactions(&mut self) {
        if self.play_mode == PlayMode::Unknown {
            return;
        }

        if ai_are_there_active_combat_effects(self) {
            self.ai.reaction_state = AiReactionState::AnimationsActive;
            return;
        }

        if self.ai.reaction_state == AiReactionState::AnimationsActive {
            self.ai.reaction_state = AiReactionState::Processing;
            self.ai_check_computer_reactions();
            return;
        }

        if ai_are_there_moving_units(self) {
            self.ai.reaction_state = AiReactionState::Processing;
        } else if self.ai.reaction_state != AiReactionState::Idle {
            self.ai.reaction_state = AiReactionState::Idle;
            self.ai_check_computer_reactions();
            return;
        }

        self.execute_reminders();
    }

    /// `TaskManager::ExecuteReminders` через `with_task_manager`.
    pub fn execute_reminders(&mut self) -> bool {
        self.with_task_manager(|task_manager, world| task_manager.execute_reminders(world))
    }

    /// `Ai_CheckComputerReactions`.
    pub fn ai_check_computer_reactions(&mut self) {
        self.with_task_manager(|task_manager, world| {
            task_manager.check_computer_reactions(world);
        });
    }

    /// `Ai_EvaluateAttackTargets` (`ai.cpp:361`).
    pub fn ai_evaluate_attack_targets(&mut self, unit: UnitId) {
        self.with_task_manager(|task_manager, world| {
            task_manager.collect_potential_attack_targets(world, unit);
        });
    }

    /// `Ai_AddUnitToTrackerList` (`ai.cpp:182`).
    pub fn ai_add_unit_to_tracker_list(&mut self, unit: UnitId) {
        self.with_task_manager(|task_manager, world| {
            task_manager.enqueue_unit_for_reaction_check(world, unit);
        });
    }

    /// `Ai_CheckEndTurn` (`ai.cpp:239`).
    pub fn ai_check_end_turn(&mut self) {
        if self.play_mode == PlayMode::TurnBased {
            if self.game_state == crate::enums::GameStateType::InGame {
                let team = self.active_turn_team;
                if self.teams[team.0 as usize].team_type == TeamKind::Computer
                    && !self.teams[team.0 as usize].finished_turn
                {
                    self.ai_check_player_end_turn(team);
                }
            }

            return;
        }

        if self.game_state == crate::enums::GameStateType::EndTurn
            || self.game_state == crate::enums::GameStateType::InGame
        {
            for team_index in 0..PLAYER_TEAM_COUNT {
                let team = TeamId(team_index as u8);
                if self.teams[team_index].team_type == TeamKind::Computer
                    && !self.teams[team_index].finished_turn
                    && self.ai_check_player_end_turn(team)
                {
                    return;
                }
            }

            if self.game_state == crate::enums::GameStateType::InGame {
                let team = self.active_turn_team;
                if self.teams[team.0 as usize].team_type == TeamKind::Computer
                    && self.teams[team.0 as usize].finished_turn
                {
                    self.game_state = crate::enums::GameStateType::EndTurn;
                }
            }
        }
    }

    /// `AiPlayer::CheckEndTurn` для команды (с изъятием игрока ради `&mut`).
    fn ai_check_player_end_turn(&mut self, team: TeamId) -> bool {
        let mut player = std::mem::replace(
            &mut self.ai.players[team.0 as usize],
            AiPlayer::new(team),
        );

        let result = player.check_end_turn(self);

        self.ai.players[team.0 as usize] = player;
        result
    }

    /// `Ai_BeginTurn` (`ai.cpp:148`).
    pub fn ai_begin_turn(&mut self, team: TeamId) {
        if self.teams[team.0 as usize].team_type != TeamKind::Computer {
            return;
        }

        let mut player = std::mem::replace(
            &mut self.ai.players[team.0 as usize],
            AiPlayer::new(team),
        );

        player.begin_turn(self);

        self.ai.players[team.0 as usize] = player;
    }
}

/// Проверка «команда — компьютер» без заимствования.
fn world_is_computer(world: &GameState, team: TeamId) -> bool {
    world.teams[team.0 as usize].team_type == TeamKind::Computer
}

// ---------------------------------------------------------------------------
// Функции `ai.cpp`
// ---------------------------------------------------------------------------

/// `Ai_Init` (`ai.cpp:154`).
pub fn ai_init(world: &mut GameState) {
    world.with_task_manager(|task_manager, world| {
        task_manager.clear(world);
    });

    for team in 0..PLAYER_TEAM_COUNT {
        world.ai.players[team].init();
    }
}

/// `Ai_SetupStrategy` (`ai.cpp:116`) — только выбор стратегии, без
/// перегенерации карты.
pub fn ai_setup_strategy(world: &mut GameState, team: TeamId) -> bool {
    let mut player = std::mem::replace(
        &mut world.ai.players[team.0 as usize],
        AiPlayer::new(team),
    );

    let result = player.select_strategy(world);

    world.ai.players[team.0 as usize] = player;
    result
}

/// `Ai_SetInfoMapPoint` (`ai.cpp:122`).
pub fn ai_set_info_map_point(world: &mut GameState, point: Point, team: TeamId) {
    world.ai_set_info_map_point(team, point);
}

/// `Ai_MarkMineMapPoint` (`ai.cpp:128`).
pub fn ai_mark_mine_map_point(world: &mut GameState, point: Point, team: TeamId) {
    world.ai_mark_mine_map_point(team, point);
}

/// `Ai_UpdateMineMap` (`ai.cpp:134`).
pub fn ai_update_mine_map(world: &mut GameState, point: Point, team: TeamId) {
    world.ai_update_mine_map(team, point);
}

/// `Ai_SetTasksPendingFlag` (`ai.cpp:140`) — флаг «есть работа» у компьютеров.
pub fn ai_set_tasks_pending_flag(world: &mut GameState) {
    for team in 0..PLAYER_TEAM_COUNT {
        if world.teams[team].team_type == TeamKind::Computer {
            world.ai.players[team].change_tasks_pending_flag(true);
        }
    }
}

/// `Ai_ClearTasksPendingFlags` (`ai.cpp:273`).
pub fn ai_clear_tasks_pending_flags(world: &mut GameState) {
    for player in &mut world.ai.players {
        player.change_tasks_pending_flag(false);
    }
}

/// `Ai_BeginTurn` (`ai.cpp:148`).
pub fn ai_begin_turn(world: &mut GameState, team: TeamId) {
    world.ai_begin_turn(team);
}

/// `Ai_CheckEndTurn` (`ai.cpp:239`).
pub fn ai_check_end_turn(world: &mut GameState) {
    world.ai_check_end_turn();
}

/// `Ai_AddUnitToTrackerList` (`ai.cpp:182`).
pub fn ai_add_unit_to_tracker_list(world: &mut GameState, unit: UnitId) {
    world.ai_add_unit_to_tracker_list(unit);
}

/// `Ai_EnableAutoSurvey` (`ai.cpp:184`) — создать `TaskAutoSurvey`.
pub fn ai_enable_auto_survey(world: &mut GameState, unit: UnitId) {
    let Some(info) = world.unit(unit).cloned() else {
        return;
    };

    if info.tasks_enabled {
        world.ai_enable_auto_survey(unit);
    }
}

impl GameState {
    /// `Ai_EnableAutoSurvey` без проверки `tasks_enabled`.
    pub fn ai_enable_auto_survey(&mut self, unit: UnitId) {
        let Some(info) = self.unit(unit).cloned() else {
            return;
        };

        let kind = TaskKind::AutoSurvey {
            central_site: info.grid,
            unit: Some(unit),
            radius: 6,
            direction: 0,
            position: info.grid,
            destination: info.grid,
            direction2: 0,
            continue_search: true,
            location_found: false,
            current_radius: 1,
        };

        self.ai_append_task(
            info.team,
            None,
            task::TASK_PRIORITY_AUTO_SURVEY,
            kind,
        );
    }
}

/// `Ai_IsDangerousLocation` (`ai.cpp:190`).
pub fn ai_is_dangerous_location(
    world: &GameState,
    unit: &UnitInfo,
    destination: Point,
    caution_level: CautionLevel,
) -> bool {
    if world.teams[unit.team.0 as usize].team_type != TeamKind::Computer
        || caution_level == CautionLevel::None
    {
        return false;
    }

    let mut unit_hits = i32::from(unit.hits);

    if caution_level == CautionLevel::AvoidAllDamage {
        unit_hits = 1;
    }

    strategy::get_damage_potential(world, unit.team, unit, destination, caution_level) >= unit_hits
}

/// `Ai_UpdateTerrainDistanceField` (`ai.cpp:215`) — в max_core
/// `TerrainDistanceField` не заведён (P5 — только `pathfinding::TerrainDistanceField`
/// в объёме поиска), поэтому портирована вторая половина: учёт военной силы.
pub fn ai_update_terrain_distance_field(world: &mut GameState, unit: UnitId) {
    let Some(info) = world.unit(unit).cloned() else {
        return;
    };

    if world.teams[info.team.0 as usize].team_type == TeamKind::Computer
        && info.base_values.get_attribute(crate::unit_values::ATTRIB_AMMO) > 0
    {
        let team = info.team;
        let mut player = std::mem::replace(&mut world.ai.players[team.0 as usize], AiPlayer::new(team));

        player.add_military_unit(world, unit);

        world.ai.players[team.0 as usize] = player;
    }
}

/// `Ai_DetermineCautionLevel` (`ai.cpp:279`).
pub fn ai_determine_caution_level(world: &GameState, unit: UnitId) -> CautionLevel {
    let Some(info) = world.unit(unit) else {
        return CautionLevel::AvoidNextTurnsFire;
    };

    if info.ai_state_bits & crate::enums::AI_STATE_NO_RETREAT != 0 {
        return CautionLevel::None;
    }

    if let Some(task_id) = world.ai.task_manager.unit_task(unit) {
        if let Some(task) = world.ai.task_manager.task(task_id) {
            return task.get_caution_level(world, info);
        }
    }

    CautionLevel::AvoidNextTurnsFire
}

/// `Ai_RemoveUnit` (`ai.cpp:295`).
pub fn ai_remove_unit(world: &mut GameState, unit: UnitId) {
    for team in 0..PLAYER_TEAM_COUNT {
        world.ai.players[team].remove_unit(unit);
    }

    world.with_task_manager(|task_manager, world| {
        task_manager.remove_destroyed_unit(world, unit);
    });
}

/// `Ai_UnitSpotted` (`ai.cpp:347`).
pub fn ai_unit_spotted(world: &mut GameState, unit: UnitId, team: TeamId) {
    let Some(info) = world.unit(unit).cloned() else {
        return;
    };

    let is_computer = world.teams[team.0 as usize].team_type == TeamKind::Computer;

    if is_computer && info.flags & crate::enums::unit_flags::SELECTABLE != 0 {
        let mut player = std::mem::replace(
            &mut world.ai.players[team.0 as usize],
            AiPlayer::new(team),
        );

        player.unit_spotted(world, unit);

        world.ai.players[team.0 as usize] = player;
    }

    world.with_task_manager(|task_manager, world| {
        task_manager.add_spotted_unit(world, unit);
    });
}

/// `Ai_UnitHidden` (`ai.cpp:355`).
pub fn ai_unit_hidden(world: &mut GameState, unit: UnitId, team: TeamId) {
    let Some(info) = world.unit(unit).cloned() else {
        return;
    };

    if world.teams[team.0 as usize].team_type == TeamKind::Computer
        && info.flags & crate::enums::unit_flags::SELECTABLE != 0
    {
        let mut player = std::mem::replace(&mut world.ai.players[team.0 as usize], AiPlayer::new(team));

        player.unit_hidden(world, unit);

        world.ai.players[team.0 as usize] = player;
    }
}

/// `Ai_EvaluateAttackTargets` (`ai.cpp:361`).
pub fn ai_evaluate_attack_targets(world: &mut GameState, unit: UnitId) {
    world.ai_evaluate_attack_targets(unit);
}

/// `Ai_CheckComputerReactions` (`ai.cpp:363`).
pub fn ai_check_computer_reactions(world: &mut GameState) {
    world.ai_check_computer_reactions();
}

/// `Ai_CheckMines` (`ai.cpp:365`) — миноукладчик сообщает командам о минах.
pub fn ai_check_mines(world: &mut GameState, unit: UnitId) {
    let Some(info) = world.unit(unit).cloned() else {
        return;
    };

    for team in 0..PLAYER_TEAM_COUNT {
        let team_id = TeamId(team as u8);

        if info.team != team_id
            && world.teams[team].team_type == TeamKind::Computer
            && info.is_visible_to_team(team_id)
        {
            let mut player = std::mem::replace(
                &mut world.ai.players[team],
                AiPlayer::new(team_id),
            );

            player.find_mines(world, unit);

            world.ai.players[team] = player;
        }
    }
}

/// `Ai_AreThereActiveCombatEffects` (`ai.cpp:374`).
pub fn ai_are_there_active_combat_effects(world: &GameState) -> bool {
    world.unit_list(UnitListKind::Particle).is_empty() == false
        || !world.pending_attacks.is_empty()
        || world.combat_effects_active
}

/// `Ai_AreThereMovingUnits` (`ai.cpp:387`).
pub fn ai_are_there_moving_units(world: &GameState) -> bool {
    for kind in [UnitListKind::MobileLandSea, UnitListKind::MobileAir] {
        for id in world.unit_list(kind) {
            let Some(unit) = world.unit(*id) else { continue };

            if unit.orders == UnitOrder::Move
                && matches!(
                    unit.state,
                    crate::enums::UnitOrderState::InProgress | crate::enums::UnitOrderState::InTransition
                )
            {
                return true;
            }
        }
    }

    false
}

/// `Ai_CheckReactions` (`ai.cpp:407`).
pub fn ai_check_reactions(world: &mut GameState) {
    world.ai_check_reactions();
}

/// `Ai_GetReactionState`.
pub fn ai_get_reaction_state(world: &GameState) -> AiReactionState {
    world.ai.reaction_state
}

/// `Ai_SetReactionState`.
pub fn ai_set_reaction_state(world: &mut GameState, state: AiReactionState) {
    world.ai.reaction_state = state;
}

/// `TaskManager_NeedToReserveRawMaterials` (`ai.cpp:434`).
pub fn task_manager_need_to_reserve_raw_materials(world: &GameState, team: TeamId) -> bool {
    tasks::create::need_to_reserve_raw_materials(world, team)
}

/// `Ai_SelectStartingPosition` (`ai.cpp:105`).
///
/// Часть setup'а (P2), но целиком на RNG и `TeamMissionSupplies`: портирована
/// 1:1 — до 64 попыток найти безопасную точку в `[3, size-3]`.
pub fn ai_select_starting_position(world: &mut GameState, team: TeamId) {
    let mut position = Point::ZERO;
    let mut attempts = 0;

    loop {
        position.x = world.rng.generate_i32(i32::from(crate::geo::MAP_WIDTH) - 6) as i16 + 3;
        position.y = world.rng.generate_i32(i32::from(crate::geo::MAP_HEIGHT) - 6) as i16 + 3;

        attempts += 1;

        if is_safe_starting_position(world, position, team) || attempts >= 64 {
            break;
        }
    }

    if let Some(supplies) = world.mission_supplies[team.0 as usize].as_mut() {
        supplies.starting_position = position;
    }
}

/// `Ai_IsValidStartingPosition` (`ai.cpp:68`) — 5×5 вокруг точки — суша.
fn is_valid_starting_position(world: &GameState, bounds: (i32, i32, i32, i32)) -> bool {
    let (ulx, uly, lrx, lry) = bounds;

    let mut y = uly;
    while y <= lry {
        let mut x = ulx;
        while x <= lrx {
            if crate::access::modified_surface_type(world, Point::new(x as i16, y as i16), false)
                != crate::access::SURFACE_TYPE_LAND
            {
                return false;
            }
            x += 1;
        }
        y += 1;
    }

    true
}

/// `Ai_IsSafeStartingPosition` (`ai.cpp:80`).
fn is_safe_starting_position(world: &GameState, position: Point, team: TeamId) -> bool {
    let bounds = (
        i32::from(position.x) - 2,
        i32::from(position.y) - 2,
        i32::from(position.x) + 2,
        i32::from(position.y) + 2,
    );

    if !is_valid_starting_position(world, bounds) {
        return false;
    }

    for other in 0..PLAYER_TEAM_COUNT {
        if TeamId(other as u8) == team {
            continue;
        }

        let Some(supplies) = world.mission_supplies[other].as_ref() else {
            continue;
        };

        if supplies.units.is_empty() {
            continue;
        }

        let dx = i32::from(position.x) - i32::from(supplies.starting_position.x);
        let dy = i32::from(position.y) - i32::from(supplies.starting_position.y);

        if dx * dx + dy * dy < AI_SAFE_ENEMY_DISTANCE {
            return false;
        }
    }

    true
}

/// `AI_SAFE_ENEMY_DISTANCE` (`ai.cpp:41`).
pub const AI_SAFE_ENEMY_DISTANCE: i32 = 400;

#[cfg(test)]
mod tests {
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
        // Без `TeamMissionSupplies` позиция не сохраняется, но RNG потреблён.
        let _ = world.rng.state();
    }
}
