//! M-SP: локальная партия (1 человек + 1–3 компьютера) без сети.
//!
//! # Что это порт чего
//!
//! Цикл повторяет `GameManager_GameLoop` + `GameManager_TeamTurnTurnBased`
//! (`max-cpp-port/src/game_manager.cpp:463`):
//!
//! 1. раунд идёт по командам `RED → GREEN → BLUE → GRAY`; команды `None` и
//!    `Eliminated` пропускаются;
//! 2. на начале хода команды вызывается `GameManager_ManageEconomy(team)`
//!    ([`max_core::GameState::manage_economy`]) и `GameManager_ProgressBuildState`
//!    (в max_core от него пока только экономика — производство это P3/TODO);
//! 3. компьютерная команда: `Ai_BeginTurn(team)` и ход «крутится» тиками, пока
//!    `AiPlayer::CheckEndTurn` не дойдёт до `end_turn_retry_counter > 120` — в
//!    C++ именно на этом пороге `GameManager_RefreshOrders` выставляет
//!    `finished_turn` и `GAME_STATE_9_END_TURN`;
//! 4. человеческая команда: ожидание ввода; **End Turn** шлёт пакет `06` и
//!    закрывает ход;
//! 5. после последней команды раунда — `GameState::finish_turn()`
//!    (`GameManager_TeamTurnFinish`): условия победы/поражения, сброс флагов,
//!    `turn_counter += 1`.
//!
//! Таймер хода — `settings["timer"]` (`GameManager_UpdateTurnTimer`); `0` —
//! без ограничения. Пауза (`PauseMenu`) останавливает и тики, и таймер.
//!
//! Победа/поражение — [`max_core::victory::evaluate_status`]
//! (`WinLoss_EvaluateStatus`) в момент закрытия раунда; по нему рисуется экран
//! [`crate::winloss`].

use max_core::enums::{GameStateType, WinLossState};
use max_core::events::command_finished_turn;
use max_core::geo::Point;
use max_core::setup::{ResourceLevel, TeamStart};
use max_core::team::{TeamId, TeamKind};
use max_core::victory::{self, WinLossStatus};

use crate::{App, NewGameConfig};

/// Сколько игровых команд в партии (`PLAYER_TEAM_COUNT`).
pub const LOCAL_TEAM_COUNT: usize = 4;

/// Предел тиков одного хода ИИ — страховка от «зависшего» ИИ.
///
/// C++ завершает ход по `end_turn_retry_counter > 120` **только когда у ИИ нет
/// незавершённой работы** (`tasks_pending`/`has_pending_work`), поэтому в
/// партии с живыми задачами ход может длиться долго. Предел — как
/// `while (Access_AreTaskEventsPending())` + аварийный `force ENDTURN` из
/// `Ai_CheckEndTurn` (concurrent-ветка): после него ход закрывается принудительно.
pub const LOCAL_AI_TICK_LIMIT: u32 = 12_000;

/// Стартовые позиции команд: четыре четверти карты 112×112.
///
/// В C++ позиции приходят из миссии (`UnitsManager_TeamMissionSupplies`);
/// у сгенерированной custom-партии миссии нет, поэтому берутся четверти карты,
/// а `setup::find_valid_starting_position` двигает их на ближайшую сушу.
pub const START_POSITIONS: [Point; LOCAL_TEAM_COUNT] = [
    Point::new(22, 22),
    Point::new(88, 88),
    Point::new(88, 22),
    Point::new(22, 88),
];

/// Параметры локальной партии — то, что отдаёт экран настройки.
///
/// Значения по умолчанию — `settings.cpp` (`timer=180`, `endturn=45`,
/// `start_gold=150`, `play_mode=1`, `victory_type=0`, `victory_limit=50`,
/// `opponent=1`, уровни ресурсов `1/1/1/0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSetup {
    /// Имя человека (команда-игрок).
    pub player_name: String,
    /// Планета `0..=23` (`settings["world"]`).
    pub world: i64,
    /// Сложность ИИ (`settings["opponent"]`, 0..=5).
    pub opponent: u8,
    /// Секунды на ход человека (0 — без таймера).
    pub timer: u32,
    /// Секунды на завершение хода ИИ (`settings["endturn"]`).
    pub endturn: u32,
    /// Режим игры (`settings["play_mode"]`).
    pub play_mode: max_core::enums::PlayMode,
    /// Стартовые кредиты (`settings["start_gold"]`, 0..=250).
    pub start_gold: u32,
    /// Уровень материалов (`raw_resource` 0–2).
    pub raw: u8,
    /// Уровень топлива.
    pub fuel: u8,
    /// Уровень золота.
    pub gold: u8,
    /// Уровень обломков пришельцев.
    pub derelicts: u8,
    /// Условие победы (`victory_type`).
    pub victory_type: max_core::enums::VictoryType,
    /// Лимит победы (`victory_limit`).
    pub victory_limit: i32,
    /// Кто кем управляет: 4 слота (`menu_team_player_setting`).
    pub team_types: [TeamKind; LOCAL_TEAM_COUNT],
    /// Seed генерации карты (`Remote_RngSeed`).
    pub seed: u32,
}

impl Default for LocalSetup {
    fn default() -> Self {
        Self {
            player_name: "Player".to_string(),
            world: 0,
            opponent: max_core::enums::Opponent::Average as u8,
            timer: 180,
            endturn: 45,
            play_mode: max_core::enums::PlayMode::SimultaneousMoves,
            start_gold: 150,
            raw: 1,
            fuel: 1,
            gold: 1,
            derelicts: 0,
            victory_type: max_core::enums::VictoryType::Duration,
            victory_limit: 50,
            // `menu_choose_player_menu_loop`: RED — человек, GREEN — компьютер,
            // BLUE/GRAY — пусто.
            team_types: [
                TeamKind::Player,
                TeamKind::Computer,
                TeamKind::None,
                TeamKind::None,
            ],
            seed: 0,
        }
    }
}

impl LocalSetup {
    /// Команда-человек (первая `Player`); `None` — недопустимая настройка.
    pub fn human_team(&self) -> Option<TeamId> {
        self.team_types
            .iter()
            .position(|kind| *kind == TeamKind::Player)
            .map(|index| TeamId(index as u8))
    }

    /// Компьютерные команды в порядке слотов.
    pub fn ai_teams(&self) -> Vec<TeamId> {
        self.team_types
            .iter()
            .enumerate()
            .filter(|(_, kind)| **kind == TeamKind::Computer)
            .map(|(index, _)| TeamId(index as u8))
            .collect()
    }

    /// Играющие команды (не `None`) в порядке слотов.
    pub fn playing_teams(&self) -> Vec<TeamId> {
        self.team_types
            .iter()
            .enumerate()
            .filter(|(_, kind)| **kind != TeamKind::None)
            .map(|(index, _)| TeamId(index as u8))
            .collect()
    }

    /// Проверка настроек по `ChoosePlayerMenu::EventDone` (`chooseplayermenu.cpp:212`):
    /// в одиночной игре нужно минимум два участника (человек + ИИ).
    pub fn validate(&self) -> Result<(), String> {
        let humans = self
            .team_types
            .iter()
            .filter(|kind| **kind == TeamKind::Player)
            .count();
        let playing = self.playing_teams().len();

        // В C++ одиночный режим оставляет ровно одного человека: выбор
        // «Human» на другом слоте переносит флаг.
        if humans > 1 {
            return Err("single player game allows only one human team".to_string());
        }

        if playing < 2 {
            return Err("There must be at least two players to begin a new game.".to_string());
        }

        if !self
            .team_types
            .iter()
            .any(|kind| *kind == TeamKind::Computer)
        {
            return Err("There must be at least one computer opponent.".to_string());
        }

        Ok(())
    }

    /// `NewGameConfig` для [`App::start_local_game`].
    pub fn new_game_config(&self) -> NewGameConfig {
        let level = |value: u8| ResourceLevel::new(value).unwrap_or(ResourceLevel::NORMAL);
        let human = self.human_team().unwrap_or(TeamId::RED);

        NewGameConfig {
            planet: planet_for_world(self.world),
            seed: self.seed,
            raw: level(self.raw),
            fuel: level(self.fuel),
            gold: level(self.gold),
            derelicts: level(self.derelicts),
            teams: self
                .playing_teams()
                .into_iter()
                .map(|team| {
                    TeamStart::new(team, START_POSITIONS[team.0 as usize % LOCAL_TEAM_COUNT])
                })
                .collect(),
            player_team: human,
            start_gold: self.start_gold,
        }
    }
}

/// Планета по индексу `0..=23` (как `Planet::try_from`, с клампом).
pub fn planet_for_world(world: i64) -> max_core::enums::Planet {
    max_core::enums::Planet::try_from(world.clamp(0, 23) as u8)
        .unwrap_or(max_core::enums::Planet::Snowcrab)
}

/// Фаза локальной партии.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPhase {
    /// Ход человека: ждём ввод (или таймер).
    PlayerTurn,
    /// Ход компьютера `team`.
    AiTurn(TeamId),
    /// Раунд закончился, идёт переход (`finish_turn`/следующая команда).
    Advancing,
    /// Партия окончена (победа/поражение).
    Finished,
}

impl LocalPhase {
    /// Подпись фазы для строки статуса.
    pub fn caption(self) -> String {
        match self {
            Self::PlayerTurn => "Your turn".to_string(),
            Self::AiTurn(team) => format!("Computer turn: {}", team_name(team)),
            Self::Advancing => "Next turn".to_string(),
            Self::Finished => "Game over".to_string(),
        }
    }
}

/// Итог партии — данные экрана победы/поражения.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalOutcome {
    /// Победила ли команда человека (`menu_wrap_up_game`: `is_winner`).
    pub victory: bool,
    /// `WinLoss_EvaluateStatus` на момент закрытия раунда.
    pub status: WinLossStatus,
    /// Номер хода, на котором закончилась партия.
    pub turn_counter: i32,
    /// Имя команды человека (для заголовка экрана).
    pub player_name: String,
}

/// Состояние локальной партии.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalGame {
    /// Настройки, с которыми начата партия.
    pub setup: LocalSetup,
    /// Фаза.
    pub phase: LocalPhase,
    /// Команда, которая ходит сейчас.
    pub active_team: TeamId,
    /// Остаток таймера хода, секунды (`GameManager_TurnTimerValue`).
    pub timer_left: f32,
    /// Пауза (`PauseMenu`): тики и таймер стоят.
    pub paused: bool,
    /// Тиков в текущем ходу ИИ.
    pub ai_ticks: u32,
    /// Индекс команды в раунде (`0..LOCAL_TEAM_COUNT`).
    pub round_team: usize,
    /// Итог партии.
    pub outcome: Option<LocalOutcome>,
}

impl LocalGame {
    /// Новая партия по настройкам.
    pub fn new(setup: LocalSetup) -> Self {
        let human = setup.human_team().unwrap_or(TeamId::RED);

        Self {
            timer_left: setup.timer as f32,
            setup,
            phase: LocalPhase::PlayerTurn,
            active_team: human,
            paused: false,
            ai_ticks: 0,
            round_team: 0,
            outcome: None,
        }
    }

    /// Партия окончена.
    pub fn finished(&self) -> bool {
        self.outcome.is_some()
    }

    /// Победил ли человек (для экрана).
    pub fn victory(&self) -> bool {
        self.outcome.as_ref().is_some_and(|outcome| outcome.victory)
    }

    /// Строка статуса партии (фаза, ход, таймер).
    pub fn status_line(&self, turn_counter: i32) -> String {
        let timer = if self.setup.timer == 0 {
            String::new()
        } else if self.phase == LocalPhase::PlayerTurn {
            format!(" · {}s", self.timer_left.ceil().max(0.0) as i32)
        } else {
            String::new()
        };

        let paused = if self.paused { " · PAUSED" } else { "" };

        format!(
            "LOCAL · turn {} · {}{}{} · LMB select · RMB move · End Turn",
            turn_counter,
            self.phase.caption(),
            timer,
            paused
        )
    }
}

/// Что сделал [`App::local_update`] за вызов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalUpdate {
    /// Тик симуляции прошёл.
    pub ticked: bool,
    /// Закрылся ход (человека или ИИ).
    pub turn_closed: bool,
    /// Начался ход компьютера.
    pub ai_started: bool,
    /// Партия закончилась в этом вызове.
    pub finished: bool,
}

/// Имя команды для логов/экрана (`menu_team_names`).
pub fn team_name(team: TeamId) -> &'static str {
    match team.0 {
        0 => "Red",
        1 => "Green",
        2 => "Blue",
        3 => "Gray",
        _ => "Alien",
    }
}

impl App {
    /// Начинает локальную партию (`menu_custom_game_menu` → `GameManager_InitGame`).
    ///
    /// Ставит `team_types` из настроек (`TeamKind`), `network_game = false`
    /// (`Remote_IsNetworkGame`), режим игры, таймеры и условие победы, затем
    /// выбирает стратегии ИИ (`Ai_SetupStrategy` для каждой компьютерной
    /// команды — `GameManager_InitGame`) и передаёт первый ход.
    pub fn start_local_game(&mut self, setup: &LocalSetup) -> Result<(), String> {
        setup.validate()?;

        let human = setup.human_team().unwrap_or(TeamId::RED);
        let config = setup.new_game_config();
        let report = self
            .new_game(&config)
            .map_err(|error| format!("local game: {error:#}"))?;

        let state = self.state_mut();

        // `Remote_IsNetworkGame = false` — локальная партия, а не lockstep.
        state.network_game = false;
        state.play_mode = setup.play_mode;
        state.victory_type = setup.victory_type;
        state.victory_limit = setup.victory_limit;
        state.mission_category = victory::MissionCategory::Custom;
        state.mission_index = 0;
        state.player_team = human;
        state.active_turn_team = human;
        state.game_state = GameStateType::InGame;
        state.turn_counter = 1;
        state.teams[human.0 as usize].team_type = TeamKind::Player;
        state.ai.settings.opponent = setup.opponent;
        state.ai.all_visible = false;

        // `Ai_Init` + `Ai_SetupStrategy` для компьютерных команд
        // (`GameManager_InitGame`).
        max_core::ai_init(state);
        for team in setup.ai_teams() {
            let _ = max_core::ai_setup_strategy(state, team);
        }

        let local = LocalGame::new(setup.clone());

        tracing::info!(
            "local game: planet={:?} teams={} aliens={} units={} player={:?} ai={:?} mode={:?}",
            report.planet,
            report.teams,
            report.aliens,
            report.units,
            human,
            setup.ai_teams(),
            setup.play_mode
        );

        self.local = Some(local);
        self.hud.clear();
        self.hud.push_log(format!(
            "Local game: {} vs {} computer(s)",
            setup.player_name,
            setup.ai_teams().len()
        ));
        self.local_begin_round();

        Ok(())
    }

    /// Продолжает локальную партию из контейнера сохранения (см.
    /// [`max_app::session_save`]).
    pub fn set_local_game(&mut self, local: LocalGame) {
        self.local = Some(local);
    }

    /// Текущая локальная партия.
    pub fn local(&self) -> Option<&LocalGame> {
        self.local.as_ref()
    }

    /// Изменяемая локальная партия.
    pub fn local_mut(&mut self) -> Option<&mut LocalGame> {
        self.local.as_mut()
    }

    /// Идёт ли локальная партия.
    pub fn local_active(&self) -> bool {
        self.local.is_some()
    }

    /// Выход в меню: локальная партия забывается.
    pub fn leave_local_game(&mut self) {
        self.local = None;
        self.hud.clear();
    }

    /// `GameManager_TeamTurnTurnBased` для начала раунда: идём по командам
    /// от `RED`, пропуская пустые/выбитые; человек — ждём ввод, компьютер —
    /// `Ai_BeginTurn`.
    fn local_begin_round(&mut self) {
        let mut local = self.local.take().expect("local game must exist");
        local.round_team = 0;
        local.phase = LocalPhase::Advancing;
        self.local = Some(local);
        self.local_next_team();
    }

    /// Следующая команда раунда (или закрытие раунда).
    fn local_next_team(&mut self) {
        loop {
            let Some(local) = self.local.as_ref() else {
                return;
            };
            let round_team = local.round_team;

            if round_team >= LOCAL_TEAM_COUNT {
                self.local_finish_round();
                return;
            }

            let team = TeamId(round_team as u8);
            let playing = {
                let state = self.state();
                let kind = state.teams[round_team].team_type;
                kind != TeamKind::None && kind != TeamKind::Eliminated
            };

            if !playing {
                if let Some(local) = self.local.as_mut() {
                    local.round_team += 1;
                }
                continue;
            }

            // `GameManager_ManageEconomy(team)` в начале хода команды.
            self.state_mut().manage_economy(team);
            self.state_mut().active_turn_team = team;
            self.state_mut().game_state = GameStateType::InGame;

            let kind = self.state().teams[round_team].team_type;

            if kind == TeamKind::Player {
                let setup_timer = self
                    .local
                    .as_ref()
                    .map(|local| local.setup.timer)
                    .unwrap_or(0);
                let timer_left = if setup_timer == 0 {
                    0.0
                } else {
                    // C++: `ini_timer_setting == 0 && player is computer → 45`,
                    // для человека 0 значит «без таймера».
                    setup_timer as f32
                };

                {
                    let state = self.state_mut();
                    state.player_team = team;
                    state.game_state = GameStateType::InGame;
                }

                let mut local = self.local.take().expect("local game");
                local.phase = LocalPhase::PlayerTurn;
                local.active_team = team;
                local.timer_left = timer_left;
                local.ai_ticks = 0;
                self.local = Some(local);
                self.focus_player();
                self.hud
                    .push_log(format!("Turn {}: your move", self.state().turn_counter));
                return;
            }

            // Компьютерная команда: `Ai_BeginTurn(team)`.
            self.state_mut().ai_begin_turn(team);

            let mut local = self.local.take().expect("local game");
            local.phase = LocalPhase::AiTurn(team);
            local.active_team = team;
            local.ai_ticks = 0;
            self.local = Some(local);
            self.hud.push_log(format!(
                "Turn {}: {} (computer)",
                self.state().turn_counter,
                team_name(team)
            ));
            return;
        }
    }

    /// `GameManager_TeamTurnFinish`: проверка победы, следующий раунд.
    fn local_finish_round(&mut self) {
        // C++ считает `WinLoss_Status` **до** `menu_check_end_game_conditions` и
        // передаёт его на экран; `finish_turn` внутри делает ту же проверку.
        let status = victory::evaluate_status(self.state(), self.state().turn_counter);
        let turn_counter = self.state().turn_counter;
        let over = self.state_mut().finish_turn();

        if over {
            let player_team = self.state().player_team;
            let is_winner = if status.team_status[player_team.0 as usize] == WinLossState::Generic {
                status.team_rank[player_team.0 as usize] == 1
            } else {
                status.team_status[player_team.0 as usize] == WinLossState::Won
            };

            let mut local = self.local.take().expect("local game");
            local.phase = LocalPhase::Finished;
            local.outcome = Some(LocalOutcome {
                victory: is_winner,
                status,
                turn_counter,
                player_name: local.setup.player_name.clone(),
            });
            self.local = Some(local);
            self.hud.push_log(if is_winner {
                "Game over: you win"
            } else {
                "Game over: you lose"
            });
            return;
        }

        {
            let mut local = self.local.take().expect("local game");
            local.round_team = 0;
            local.phase = LocalPhase::Advancing;
            self.local = Some(local);
        }

        self.hud
            .push_log(format!("Turn {} begins", self.state().turn_counter));
        self.local_next_team();
    }

    /// End Turn человека: пакет `06` (`GameManager_RefreshOrders` →
    /// `Remote_SendNetPacket_06`) и переход к следующей команде раунда.
    pub fn local_end_turn(&mut self) {
        let Some(local) = self.local.as_ref() else {
            return;
        };

        if local.phase != LocalPhase::PlayerTurn {
            return;
        }

        let team = local.active_team;
        let _ = self.apply(command_finished_turn(team));

        if self.state().game_state != GameStateType::EndTurn {
            self.state_mut().game_state = GameStateType::EndTurn;
        }

        self.hud.push_log(format!("{} ended turn", team_name(team)));

        if let Some(local) = self.local.as_mut() {
            local.phase = LocalPhase::Advancing;
            local.round_team = team.0 as usize + 1;
        }

        self.local_next_team();
    }

    /// Пауза/продолжение. Возвращает новое состояние (`true` — на паузе).
    pub fn local_toggle_pause(&mut self) -> bool {
        let Some(local) = self.local.as_mut() else {
            return false;
        };
        local.paused = !local.paused;
        let paused = local.paused;
        self.hud.push_log(if paused { "Paused" } else { "Resumed" });
        paused
    }

    /// Один кадр локальной партии: тик симуляции, таймер, ходы ИИ.
    ///
    /// `dt` — секунды с прошлого кадра (таймер хода). Детерминизм симуляции от
    /// `dt` не зависит: тики идут по одному за кадр, как в
    /// `GameManager_ProcessTick`.
    pub fn local_update(&mut self, dt: f32) -> LocalUpdate {
        let mut update = LocalUpdate::default();

        let Some(local) = self.local.as_ref() else {
            return update;
        };
        if local.paused || local.phase == LocalPhase::Finished {
            return update;
        }
        let phase = local.phase;

        match phase {
            LocalPhase::PlayerTurn => {
                self.state.tick();
                update.ticked = true;

                let timer = self
                    .local
                    .as_ref()
                    .map(|local| local.setup.timer)
                    .unwrap_or(0);
                if timer > 0 {
                    let mut expired = false;
                    if let Some(local) = self.local.as_mut() {
                        local.timer_left -= dt.max(0.0);
                        expired = local.timer_left <= 0.0;
                    }
                    if expired {
                        self.hud.push_log("Turn timer expired");
                        self.local_end_turn();
                        update.turn_closed = true;
                    }
                }
            }
            LocalPhase::AiTurn(team) => {
                self.state.tick();
                update.ticked = true;

                let timed_out = {
                    let local = self.local.as_ref().expect("local game");
                    local.ai_ticks + 1 >= LOCAL_AI_TICK_LIMIT
                };

                // `AiPlayer::CheckEndTurn` тикает сам (`GameState::tick` →
                // `ai_check_end_turn`); здесь читаем его счётчик повторов:
                // C++ `GameManager_RefreshOrders` на `> 120` закрывает ход.
                let finished = self
                    .state()
                    .ai
                    .players
                    .get(team.0 as usize)
                    .map(|player| player.end_turn_retry_counter > 120)
                    .unwrap_or(true)
                    || self.state().teams[team.0 as usize].finished_turn;

                if let Some(local) = self.local.as_mut() {
                    local.ai_ticks += 1;
                }

                if timed_out {
                    self.hud.push_log(format!(
                        "{}: force end turn ({} ticks)",
                        team_name(team),
                        LOCAL_AI_TICK_LIMIT
                    ));
                }

                if finished || timed_out {
                    let _ = self.apply(command_finished_turn(team));
                    if self.state().game_state != GameStateType::EndTurn {
                        self.state_mut().game_state = GameStateType::EndTurn;
                    }

                    update.turn_closed = true;

                    if let Some(local) = self.local.as_mut() {
                        local.phase = LocalPhase::Advancing;
                        local.round_team = team.0 as usize + 1;
                    }

                    self.local_next_team();

                    let started_ai = matches!(
                        self.local.as_ref().map(|local| local.phase),
                        Some(LocalPhase::AiTurn(_))
                    );
                    update.ai_started = started_ai;
                }
            }
            LocalPhase::Advancing => {
                self.local_next_team();
                update.ai_started = matches!(
                    self.local.as_ref().map(|local| local.phase),
                    Some(LocalPhase::AiTurn(_))
                );
            }
            LocalPhase::Finished => {}
        }

        update.finished = self
            .local
            .as_ref()
            .is_some_and(|local| local.phase == LocalPhase::Finished);
        update
    }
}

// ---------------------------------------------------------------------------
// РЎРѕС…СЂР°РЅРµРЅРёРµ Р»РѕРєР°Р»СЊРЅРѕР№ РїР°СЂС‚РёРё
// ---------------------------------------------------------------------------

/// РњР°РіРёСЏ РєРѕРЅС‚РµР№РЅРµСЂР° Р»РѕРєР°Р»СЊРЅРѕРіРѕ СЃРѕС…СЂР°РЅРµРЅРёСЏ (`M.A.X. Local Session`).
pub const LOCAL_SAVE_MAGIC: [u8; 4] = *b"MXLS";
/// Р’РµСЂСЃРёСЏ РєРѕРЅС‚РµР№РЅРµСЂР°.
pub const LOCAL_SAVE_VERSION: u16 = 1;
/// РРјСЏ С„Р°Р№Р»Р° Р±С‹СЃС‚СЂРѕРіРѕ СЃРѕС…СЂР°РЅРµРЅРёСЏ Р»РѕРєР°Р»СЊРЅРѕР№ РїР°СЂС‚РёРё.
pub const LOCAL_SAVE_FILE: &str = "max-local.mxs";

/// РљРѕРЅС‚РµР№РЅРµСЂ СЃРѕС…СЂР°РЅРµРЅРёСЏ Р»РѕРєР°Р»СЊРЅРѕР№ РїР°СЂС‚РёРё: РЅР°СЃС‚СЂРѕР№РєРё + С„Р°Р·Р° + СЃРЅРёРјРѕРє СЃРѕСЃС‚РѕСЏРЅРёСЏ.
///
/// # РџРѕС‡РµРјСѓ РЅРµ V70
///
/// V70 **С‡РёС‚Р°РµС‚СЃСЏ** (`max_core::save::SaveFile::parse` в†’ `GameState::from_save`,
/// РІ С‚РѕРј С‡РёСЃР»Рµ РІ Р»РѕРєР°Р»СЊРЅРѕР№ РїР°СЂС‚РёРё вЂ” [`App::start_local_game_from_save`]), РЅРѕ
/// Р·Р°РїРёСЃРё `GameState в†’ SaveFile` РІ РїРѕСЂС‚Рµ РЅРµС‚: V70 С…СЂР°РЅРёС‚ РѕР±СЉРµРєС‚РЅРѕРµ РґРµСЂРµРІРѕ
/// (`SaveUnit` + `HeatMap` + `AiPlayerState` + С…СЌС€Рё), РєРѕС‚РѕСЂРѕРіРѕ РІ `GameState`
/// РЅРµС‚ РІ С‚Р°РєРѕРј РІРёРґРµ. РљРѕРЅС‚РµР№РЅРµСЂ `MXLS` РєР»Р°РґС‘С‚ `GameState::snapshot_bytes()`
/// (РєР°РЅРѕРЅРёС‡РµСЃРєРёР№ РґР°Р№РґР¶РµСЃС‚ РІСЃРµР№ СЃРёРјСѓР»СЏС†РёРё) Рё РјРµС‚Р°РґР°РЅРЅС‹Рµ РїР°СЂС‚РёРё; РѕРЅ РїСЂРѕРІРµСЂСЏРµС‚
/// С†РµР»РѕСЃС‚РЅРѕСЃС‚СЊ СЃРѕС…СЂР°РЅРµРЅРёСЏ (CRC СЃРЅРёРјРєР°), РЅРѕ РІРѕСЃСЃС‚Р°РЅРѕРІР»РµРЅРёРµ СЃРёРјСѓР»СЏС†РёРё РёР· РЅРµРіРѕ
/// С‚СЂРµР±СѓРµС‚ РґРµСЃРµСЂРёР°Р»РёР·Р°С‚РѕСЂР° СЃРѕСЃС‚РѕСЏРЅРёСЏ вЂ” РѕС‚РєСЂС‹С‚С‹Р№ РїСѓРЅРєС‚ (`docs/plan-modes.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSave {
    /// РќР°СЃС‚СЂРѕР№РєРё РїР°СЂС‚РёРё.
    pub setup: LocalSetup,
    /// Р¤Р°Р·Р°.
    pub phase: LocalPhase,
    /// РђРєС‚РёРІРЅР°СЏ РєРѕРјР°РЅРґР°.
    pub active_team: TeamId,
    /// РћСЃС‚Р°С‚РѕРє С‚Р°Р№РјРµСЂР° РІ РјРёР»Р»РёСЃРµРєСѓРЅРґР°С….
    pub timer_left_ms: u32,
    /// РџР°СѓР·Р°.
    pub paused: bool,
    /// РРЅРґРµРєСЃ РєРѕРјР°РЅРґС‹ РІ СЂР°СѓРЅРґРµ.
    pub round_team: u8,
    /// РўРёРєРѕРІ РІ С‚РµРєСѓС‰РµРј С…РѕРґСѓ РР.
    pub ai_ticks: u32,
    /// РџР°СЂС‚РёСЏ РѕРєРѕРЅС‡РµРЅР°.
    pub finished: bool,
    /// РџРѕР±РµРґР° С‡РµР»РѕРІРµРєР°.
    pub victory: bool,
    /// РќРѕРјРµСЂ С…РѕРґР°.
    pub turn_counter: i32,
    /// РЎРЅРёРјРѕРє СЃРѕСЃС‚РѕСЏРЅРёСЏ (`GameState::snapshot_bytes`).
    pub snapshot: Vec<u8>,
    /// CRC16 СЃРЅРёРјРєР° (`max_core::crc::crc16`).
    pub snapshot_crc: u16,
}

impl LocalSave {
    /// РЎРѕР±РёСЂР°РµС‚ РєРѕРЅС‚РµР№РЅРµСЂ РёР· С‚РµРєСѓС‰РµРіРѕ СЃРѕСЃС‚РѕСЏРЅРёСЏ РїСЂРёР»РѕР¶РµРЅРёСЏ.
    pub fn capture(app: &App) -> Option<Self> {
        let local = app.local()?;
        let snapshot = app.state().snapshot_bytes();
        let snapshot_crc = max_core::crc::crc16(&snapshot);

        Some(Self {
            setup: local.setup.clone(),
            phase: local.phase,
            active_team: local.active_team,
            timer_left_ms: (local.timer_left.max(0.0) * 1000.0) as u32,
            paused: local.paused,
            round_team: local.round_team.min(u8::MAX as usize) as u8,
            ai_ticks: local.ai_ticks,
            finished: local.finished(),
            victory: local.victory(),
            turn_counter: app.state().turn_counter,
            snapshot,
            snapshot_crc,
        })
    }

    /// РЎРµСЂРёР°Р»РёР·СѓРµС‚ РєРѕРЅС‚РµР№РЅРµСЂ РІ Р±Р°Р№С‚С‹.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + self.snapshot.len());

        out.extend_from_slice(&LOCAL_SAVE_MAGIC);
        out.extend_from_slice(&LOCAL_SAVE_VERSION.to_le_bytes());

        // setup
        put_string(&mut out, &self.setup.player_name);
        out.extend_from_slice(&(self.setup.world as i32).to_le_bytes());
        out.push(self.setup.opponent);
        out.extend_from_slice(&self.setup.timer.to_le_bytes());
        out.extend_from_slice(&self.setup.endturn.to_le_bytes());
        out.push(self.setup.play_mode as u8);
        out.extend_from_slice(&self.setup.start_gold.to_le_bytes());
        out.extend_from_slice(&[
            self.setup.raw,
            self.setup.fuel,
            self.setup.gold,
            self.setup.derelicts,
        ]);
        out.push(self.setup.victory_type as u8);
        out.extend_from_slice(&self.setup.victory_limit.to_le_bytes());
        out.extend(self.setup.team_types.iter().map(|kind| *kind as u8));
        out.extend_from_slice(&self.setup.seed.to_le_bytes());

        // С„Р°Р·Р°
        match self.phase {
            LocalPhase::PlayerTurn => out.push(0),
            LocalPhase::AiTurn(team) => {
                out.push(1);
                out.push(team.0);
            }
            LocalPhase::Advancing => out.push(2),
            LocalPhase::Finished => out.push(3),
        }
        out.push(self.active_team.0);
        out.extend_from_slice(&self.timer_left_ms.to_le_bytes());
        out.push(u8::from(self.paused));
        out.push(self.round_team);
        out.extend_from_slice(&self.ai_ticks.to_le_bytes());
        out.push(u8::from(self.finished));
        out.push(u8::from(self.victory));
        out.extend_from_slice(&self.turn_counter.to_le_bytes());

        out.extend_from_slice(&self.snapshot_crc.to_le_bytes());
        out.extend_from_slice(&(self.snapshot.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.snapshot);

        out
    }

    /// Р Р°Р·Р±РёСЂР°РµС‚ РєРѕРЅС‚РµР№РЅРµСЂ Рё РїСЂРѕРІРµСЂСЏРµС‚ CRC СЃРЅРёРјРєР°.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = SaveReader::new(bytes);

        let magic = reader.bytes(4)?;
        if magic != LOCAL_SAVE_MAGIC {
            return Err("not a local session save (bad magic)".to_string());
        }
        let version = reader.u16()?;
        if version != LOCAL_SAVE_VERSION {
            return Err(format!("unsupported local save version {version}"));
        }

        let player_name = reader.string()?;
        let world = i64::from(reader.i32()?);
        let opponent = reader.u8()?;
        let timer = reader.u32()?;
        let endturn = reader.u32()?;
        let play_mode_raw = reader.u8()?;
        let play_mode = max_core::enums::PlayMode::try_from(play_mode_raw)
            .unwrap_or(max_core::enums::PlayMode::TurnBased);
        let start_gold = reader.u32()?;
        let raw = reader.u8()?;
        let fuel = reader.u8()?;
        let gold = reader.u8()?;
        let derelicts = reader.u8()?;
        let victory_type_raw = reader.u8()?;
        let victory_type = max_core::enums::VictoryType::try_from(victory_type_raw)
            .unwrap_or(max_core::enums::VictoryType::Duration);
        let victory_limit = reader.i32()?;
        let mut team_types = [TeamKind::None; LOCAL_TEAM_COUNT];
        for slot in team_types.iter_mut() {
            *slot = match reader.u8()? {
                0 => TeamKind::None,
                1 => TeamKind::Player,
                2 => TeamKind::Computer,
                3 => TeamKind::Remote,
                _ => TeamKind::Eliminated,
            };
        }
        let seed = reader.u32()?;

        let phase = match reader.u8()? {
            0 => LocalPhase::PlayerTurn,
            1 => LocalPhase::AiTurn(TeamId(reader.u8()?)),
            2 => LocalPhase::Advancing,
            _ => LocalPhase::Finished,
        };
        let active_team = TeamId(reader.u8()?);
        let timer_left_ms = reader.u32()?;
        let paused = reader.u8()? != 0;
        let round_team = reader.u8()?;
        let ai_ticks = reader.u32()?;
        let finished = reader.u8()? != 0;
        let victory = reader.u8()? != 0;
        let turn_counter = reader.i32()?;

        let snapshot_crc = reader.u16()?;
        let snapshot_len = reader.u32()? as usize;
        let snapshot = reader.bytes(snapshot_len)?.to_vec();

        if max_core::crc::crc16(&snapshot) != snapshot_crc {
            return Err("local save is corrupt (snapshot CRC mismatch)".to_string());
        }

        Ok(Self {
            setup: LocalSetup {
                player_name,
                world,
                opponent,
                timer,
                endturn,
                play_mode,
                start_gold,
                raw,
                fuel,
                gold,
                derelicts,
                victory_type,
                victory_limit,
                team_types,
                seed,
            },
            phase,
            active_team,
            timer_left_ms,
            paused,
            round_team,
            ai_ticks,
            finished,
            victory,
            turn_counter,
            snapshot,
            snapshot_crc,
        })
    }

    /// РћСЃС‚Р°С‚РѕРє С‚Р°Р№РјРµСЂР° РІ СЃРµРєСѓРЅРґР°С….
    pub fn timer_left(&self) -> f32 {
        self.timer_left_ms as f32 / 1000.0
    }
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// РњРёРЅРёРјР°Р»СЊРЅС‹Р№ little-endian С‡РёС‚Р°С‚РµР»СЊ РєРѕРЅС‚РµР№РЅРµСЂР°.
struct SaveReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SaveReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn bytes(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self.pos + count;
        if end > self.data.len() {
            return Err("local save is truncated".to_string());
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    fn string(&mut self) -> Result<String, String> {
        let len = usize::from(self.u16()?);
        let bytes = self.bytes(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| "local save has non-UTF-8 name".to_string())
    }
}

impl App {
    /// РќР°С‡РёРЅР°РµС‚ Р»РѕРєР°Р»СЊРЅСѓСЋ РїР°СЂС‚РёСЋ **РёР· V70-СЃРµР№РІР°** (`SaveLoad_Load` + Р»РѕРєР°Р»СЊРЅС‹Р№ С†РёРєР»).
    ///
    /// Р—Р°РіСЂСѓР·РєР° V70 РІ Р»РѕРєР°Р»СЊРЅРѕРј СЂРµР¶РёРјРµ: СЃРѕСЃС‚РѕСЏРЅРёРµ РёР· С„Р°Р№Р»Р°, `network_game = false`,
    /// РєРѕРјР°РЅРґС‹ Рё `player_team` вЂ” РёР· СЃРµР№РІР°, СЂРµР¶РёРј РёРіСЂС‹ Рё С‚Р°Р№РјРµСЂС‹ вЂ” РёР· РЅР°СЃС‚СЂРѕРµРє
    /// СЌРєСЂР°РЅР°. РџРѕСЃР»Рµ Р·Р°РіСЂСѓР·РєРё РёРґС‘С‚ РѕР±С‹С‡РЅС‹Р№ С†РёРєР» M-SP (С…РѕРґС‹ РР, End Turn).
    pub fn start_local_game_from_save(
        &mut self,
        setup: &LocalSetup,
        bytes: &[u8],
    ) -> Result<(), String> {
        self.load_save_bytes(bytes)
            .map_err(|error| format!("V70 load: {error:#}"))?;

        let state = self.state_mut();
        state.network_game = false;
        state.play_mode = setup.play_mode;
        state.victory_type = setup.victory_type;
        state.victory_limit = setup.victory_limit;
        state.ai.settings.opponent = setup.opponent;

        // РРіСЂР°СЋС‰РёРµ РєРѕРјР°РЅРґС‹ вЂ” РёР· СЃРµР№РІР°; `player_team` СЃРµР№РІР° вЂ” С‡РµР»РѕРІРµРє.
        let mut team_types = [TeamKind::None; LOCAL_TEAM_COUNT];
        for (index, slot) in team_types.iter_mut().enumerate() {
            *slot = match state.teams[index].team_type {
                TeamKind::None | TeamKind::Eliminated => TeamKind::None,
                TeamKind::Player => TeamKind::Player,
                _ => TeamKind::Computer,
            };
        }

        if !team_types.contains(&TeamKind::Player) {
            let player = setup.human_team().unwrap_or(state.player_team);
            team_types[player.0 as usize] = TeamKind::Player;
            state.player_team = player;
        }

        for (index, kind) in team_types.iter().enumerate() {
            if *kind == TeamKind::Computer {
                state.teams[index].team_type = TeamKind::Computer;
            }
        }

        max_core::ai_init(state);
        for index in 0..LOCAL_TEAM_COUNT {
            if team_types[index] == TeamKind::Computer {
                let _ = max_core::ai_setup_strategy(state, TeamId(index as u8));
            }
        }

        let mut loaded = setup.clone();
        loaded.team_types = team_types;

        let mut local = LocalGame::new(loaded);
        local.phase = LocalPhase::Advancing;
        local.round_team = 0;
        self.local = Some(local);
        self.hud.clear();
        self.hud.push_log("Local game loaded from a V70 save");
        self.local_begin_round();

        Ok(())
    }

    /// РЎРѕС…СЂР°РЅРµРЅРёРµ Р»РѕРєР°Р»СЊРЅРѕР№ РїР°СЂС‚РёРё РІ Р±Р°Р№С‚С‹ РєРѕРЅС‚РµР№РЅРµСЂР° `MXLS`.
    pub fn local_save_bytes(&self) -> Option<Vec<u8>> {
        LocalSave::capture(self).map(|save| save.encode())
    }

    /// Р—Р°РїРёСЃСЊ Р»РѕРєР°Р»СЊРЅРѕР№ РїР°СЂС‚РёРё РІ С„Р°Р№Р» (`.mxs`).
    pub fn write_local_save(&self, path: impl AsRef<std::path::Path>) -> Result<usize, String> {
        let bytes = self
            .local_save_bytes()
            .ok_or_else(|| "no local game in progress".to_string())?;
        std::fs::write(path.as_ref(), &bytes)
            .map_err(|error| format!("write {}: {error}", path.as_ref().display()))?;
        Ok(bytes.len())
    }

    /// Р§С‚РµРЅРёРµ Рё РїСЂРѕРІРµСЂРєР° РєРѕРЅС‚РµР№РЅРµСЂР° Р»РѕРєР°Р»СЊРЅРѕРіРѕ СЃРѕС…СЂР°РЅРµРЅРёСЏ.
    pub fn read_local_save(path: impl AsRef<std::path::Path>) -> Result<LocalSave, String> {
        let bytes = std::fs::read(path.as_ref())
            .map_err(|error| format!("read {}: {error}", path.as_ref().display()))?;
        LocalSave::decode(&bytes)
    }
}
