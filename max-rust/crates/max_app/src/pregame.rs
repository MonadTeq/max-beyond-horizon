//! Две фазы старта партии между экраном Players и собственно партией.
//!
//! # Цепочка C++
//!
//! `menu_custom_game_menu` (`menu.cpp:2115`) после `ChoosePlayerMenu` вызывает
//! `GameManager_GameLoop(GAME_STATE_6_GAME_SETUP)` → `GameManager_GameSetup` →
//! `GameManager_InitGame` (`game_manager.cpp:3056`). Там, **до** генерации
//! ресурсов и высадки, для каждой человеческой команды в порядке убывания
//! индекса (`for (i = PLAYER_TEAM_MAX - 1; i >= PLAYER_TEAM_RED; --i)`)
//! выполняется `GameManager_PlayerMissionSetup(team)`:
//!
//! 1. `GameManager_CargoSelection(team)` — `CargoMenu`, мобилизация: покупка
//!    стартовых юнитов за `start_gold` (если бюджет не ноль);
//! 2. `GameManager_SelectSite(team)` — выбор точки высадки на карте
//!    (`GAME_STATE_7_SITE_SELECT`, `Enter`/клик — подтвердить, `Esc` — отмена);
//! 3. проверка близости высадок (`GameManager_HandleProximityOverlaps`).
//!
//! Затем компьютерные команды выбирают точки сами (`Ai_SelectStartingPosition`)
//! и покупают состав (`AiPlayer::RollTeamMissionSupplies`), после чего
//! `GameManager_PopulateMapWithResources` генерирует ресурсы и
//! `GameManager_ProcessTeamMissionSupplyUnits` высаживает армии.
//!
//! # Что делает этот модуль
//!
//! Держит очередь фаз для людей (мобилизация → точка высадки на каждую
//! команду), копит результаты в [`LocalSetup`] и отдаёт готовые настройки в
//! [`crate::App::start_local_game`]. ИИ-команды фаз не проходят: их точки
//! выбирает `App::new_game_on_surface` (`Ai_SelectStartingPosition`), состав —
//! стандартная загрузка (см. упрощения в [`crate::mobilize`]).
//!
//! # Отличия от C++
//!
//! * Мобилизация — минимальная (список покупки + кредиты), см. [`crate::mobilize`].
//! * `Esc`/`Cancel` возвращает на экран `Players` (в C++ отмена фазы прерывает
//!   старт и возвращает в главное меню) — так настройки не теряются.
//! * `GameManager_HandleProximityOverlaps` и предупреждения о близких высадках
//!   не портированы.

use std::path::Path;

use max_core::map::SurfaceMap;
use max_core::team::{TeamId, TeamKind};

use crate::landing::LandingScreen;
use crate::local::LocalSetup;
use crate::mobilize::Mobilization;
use crate::setup::{GameSetupForm, SetupScreen};

/// Фаза одной команды.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreGameStep {
    /// `CargoMenu` — покупка стартовых юнитов.
    Mobilize(TeamId),
    /// `GameManager_SelectSite` — точка высадки.
    Landing(TeamId),
}

/// Текущий экран фазы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreGameStage {
    /// Идёт мобилизация команды.
    Mobilize(Mobilization),
    /// Идёт выбор точки высадки.
    Landing(LandingScreen),
    /// Все фазы пройдены — можно начинать партию.
    Finished,
}

/// Очередь фаз старта и накопленные результаты.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreGameFlow {
    /// Настройки партии (результаты фаз пишутся сюда же).
    pub setup: LocalSetup,
    /// Форма настройки — чтобы `Cancel` вернул на `Players`.
    pub form: GameSetupForm,
    /// Оставшиеся шаги (первый — текущий).
    pub pending: Vec<PreGameStep>,
    /// Текущий экран.
    pub stage: PreGameStage,
    /// Поверхность планеты (одна на все фазы выбора точки).
    pub surface: Option<SurfaceMap>,
    /// Настоящие тайлы `.WRL` для рендера экрана высадки.
    pub terrain: Option<max_render::terrain::TerrainAtlas>,
    /// Строка статуса между экранами.
    pub status: String,
}

/// План фаз старта для людей: порядок убывания слота (как C++), для каждого —
/// мобилизация (если есть кредиты), затем точка высадки.
pub fn phase_plan(setup: &LocalSetup) -> Vec<PreGameStep> {
    let mut pending = Vec::new();

    for team in setup.human_teams().into_iter().rev() {
        if Mobilization::needed(setup.start_gold) {
            pending.push(PreGameStep::Mobilize(team));
        }
        pending.push(PreGameStep::Landing(team));
    }

    pending
}

impl PreGameFlow {
    /// Создаёт очередь фаз: люди в порядке убывания слота (как C++), для
    /// каждого — мобилизация (если есть кредиты), затем точка высадки.
    pub fn start(
        form: GameSetupForm,
        setup: LocalSetup,
        world_dir: &Path,
    ) -> Self {
        let parts = LandingScreen::load_parts(setup.world, world_dir);
        let surface = parts.as_ref().map(|(surface, _)| surface.clone());
        let terrain = parts.map(|(_, terrain)| terrain);
        let pending = phase_plan(&setup);

        let mut flow = Self {
            setup,
            form,
            pending,
            stage: PreGameStage::Finished,
            surface,
            terrain,
            status: String::new(),
        };

        flow.advance();
        flow
    }

    /// Переходит к следующему шагу очереди (или в [`PreGameStage::Finished`]).
    fn advance(&mut self) {
        while let Some(step) = self.pending.first().copied() {
            match step {
                PreGameStep::Mobilize(team) => {
                    let clan = self.setup.team_clans[usize::from(team.0) % 4];

                    self.pending.remove(0);
                    self.stage = PreGameStage::Mobilize(Mobilization::new(
                        team,
                        clan,
                        self.setup.start_gold,
                    ));
                    self.status = format!(
                        "{}: buy units, then Done",
                        crate::hotseat::team_display_name(team)
                    );
                    return;
                }
                PreGameStep::Landing(team) => {
                    self.pending.remove(0);

                    match self.surface.clone() {
                        Some(surface) => {
                            self.stage = PreGameStage::Landing(LandingScreen::new_with_terrain(
                                team,
                                self.setup.world,
                                surface,
                                self.terrain.clone(),
                            ));
                            self.status = format!(
                                "{}: select landing site",
                                crate::hotseat::team_display_name(team)
                            );
                            return;
                        }
                        None => {
                            // Нет данных планеты — точка остаётся стандартной
                            // (`START_POSITIONS` + FindValidStartingPosition при
                            // генерации). Фаза честно пропускается.
                            self.status = "No planet data: default landing site used".to_string();
                        }
                    }
                }
            }
        }

        self.stage = PreGameStage::Finished;
    }

    /// Все фазы пройдены.
    pub fn finished(&self) -> bool {
        matches!(self.stage, PreGameStage::Finished)
    }

    /// Кто проходит фазу сейчас.
    pub fn current_team(&self) -> Option<TeamId> {
        match &self.stage {
            PreGameStage::Mobilize(mobilization) => Some(mobilization.team),
            PreGameStage::Landing(landing) => Some(landing.team),
            PreGameStage::Finished => None,
        }
    }

    /// Завершает мобилизацию: кредиты, загрузка и купленные апгрейды уходят в
    /// настройки партии (`CargoMenu::CommitUpgradeChanges`).
    pub fn commit_mobilization(&mut self) {
        if let PreGameStage::Mobilize(mobilization) = &self.stage {
            let index = usize::from(mobilization.team.0) % 4;

            self.setup.team_gold[index] = Some(mobilization.credits);
            self.setup.team_loadouts[index] = Some(mobilization.loadout());
            self.setup.team_upgrades[index] = Some(mobilization.changed_upgrades());
            self.setup.team_gold_spent[index] = Some(mobilization.gold_spent_on_upgrades());
        }

        self.advance();
    }

    /// Завершает выбор точки: клетка уходит в настройки партии.
    pub fn commit_landing(&mut self) {
        if let PreGameStage::Landing(landing) = &self.stage {
            let index = usize::from(landing.team.0) % 4;

            self.setup.start_positions[index] = Some(landing.cursor);
        }

        self.advance();
    }

    /// Отмена всей цепочки: возвращает форму на экран `Players`.
    pub fn cancel(&mut self) {
        self.form.screen = SetupScreen::Players;
        self.form.status.clear();
    }
}

/// Сколько людей проходят фазы (для логов/тестов).
pub fn phase_team_order(setup: &LocalSetup) -> Vec<TeamId> {
    setup
        .team_types
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == TeamKind::Player)
        .map(|(index, _)| TeamId(index as u8))
        .rev()
        .collect()
}
