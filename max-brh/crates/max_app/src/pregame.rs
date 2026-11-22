//! Two match-start phases between the Players screen and the match itself.
//!
//! # C++ chain
//!
//! `menu_custom_game_menu` (`menu.cpp:2115`) after `ChoosePlayerMenu` calls
//! `GameManager_GameLoop(GAME_STATE_6_GAME_SETUP)` → `GameManager_GameSetup` →
//! `GameManager_InitGame` (`game_manager.cpp:3056`). There, **before** generating
//! resources and landing, for each human team in descending
//! index order (`for (i = PLAYER_TEAM_MAX - 1; i >= PLAYER_TEAM_RED; --i)`)
//! it runs `GameManager_PlayerMissionSetup(team)`:
//!
//! 1. `GameManager_CargoSelection(team)` — `CargoMenu`, mobilization: purchase of
//!    starting units for `start_gold` (if the budget is not zero);
//! 2. `GameManager_SelectSite(team)` — choosing a landing point on the map
//!    (`GAME_STATE_7_SITE_SELECT`, `Enter`/click — confirm, `Esc` — cancel);
//! 3. landing proximity check (`GameManager_HandleProximityOverlaps`).
//!
//! Then the computer teams choose their points themselves (`Ai_SelectStartingPosition`)
//! and buy their roster (`AiPlayer::RollTeamMissionSupplies`), after which
//! `GameManager_PopulateMapWithResources` generates resources and
//! `GameManager_ProcessTeamMissionSupplyUnits` lands the armies.
//!
//! # What this module does
//!
//! It keeps the phase queue for humans (mobilization → landing point for each
//! team), accumulates the results in [`LocalSetup`] and hands the ready settings to
//! [`crate::App::start_local_game`]. AI teams do not go through the phases: their points
//! are chosen by `App::new_game_on_surface` (`Ai_SelectStartingPosition`), the roster —
//! by the standard loading (see the simplifications in [`crate::mobilize`]).
//!
//! # Differences from C++
//!
//! * Mobilization is minimal (purchase list + credits), see [`crate::mobilize`].
//! * `Esc`/`Cancel` returns to the `Players` screen (in C++ cancelling a phase interrupts
//!   the start and returns to the main menu) — this way the settings are not lost.
//! * `GameManager_HandleProximityOverlaps` and the close-landing warnings
//!   are not ported.

use std::path::Path;

use max_core::map::SurfaceMap;
use max_core::team::{TeamId, TeamKind};

use crate::landing::LandingScreen;
use crate::local::LocalSetup;
use crate::mobilize::Mobilization;
use crate::setup::{GameSetupForm, SetupScreen};

/// Phase of one team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreGameStep {
    /// `CargoMenu` — purchase of starting units.
    Mobilize(TeamId),
    /// `GameManager_SelectSite` — landing point.
    Landing(TeamId),
}

/// Current phase screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreGameStage {
    /// Team mobilization is in progress.
    Mobilize(Mobilization),
    /// Landing point selection is in progress.
    Landing(LandingScreen),
    /// All phases are completed — the match can start.
    Finished,
}

/// Start phase queue and accumulated results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreGameFlow {
    /// Match settings (phase results are written here as well).
    pub setup: LocalSetup,
    /// Settings form — so that `Cancel` returns to `Players`.
    pub form: GameSetupForm,
    /// Remaining steps (the first one is current).
    pub pending: Vec<PreGameStep>,
    /// Current screen.
    pub stage: PreGameStage,
    /// Planet surface (one for all point-selection phases).
    pub surface: Option<SurfaceMap>,
    /// Real `.WRL` tiles for rendering the landing screen.
    pub terrain: Option<max_render::terrain::TerrainAtlas>,
    /// Status line between screens.
    pub status: String,
}

/// Start phase plan for humans: slot descending order (as in C++), for each —
/// mobilization (if there are credits), then the landing point.
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
    /// Creates the phase queue: humans in slot descending order (as in C++), for
    /// each — mobilization (if there are credits), then the landing point.
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

    /// Moves to the next step of the queue (or to [`PreGameStage::Finished`]).
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
                            // No planet data — the point remains the default one
                            // (`START_POSITIONS` + FindValidStartingPosition during
                            // generation). The phase is skipped honestly.
                            self.status = "No planet data: default landing site used".to_string();
                        }
                    }
                }
            }
        }

        self.stage = PreGameStage::Finished;
    }

    /// All phases are completed.
    pub fn finished(&self) -> bool {
        matches!(self.stage, PreGameStage::Finished)
    }

    /// Who is passing the phase now.
    pub fn current_team(&self) -> Option<TeamId> {
        match &self.stage {
            PreGameStage::Mobilize(mobilization) => Some(mobilization.team),
            PreGameStage::Landing(landing) => Some(landing.team),
            PreGameStage::Finished => None,
        }
    }

    /// Completes mobilization: credits, loading and purchased upgrades go into
    /// the match settings (`CargoMenu::CommitUpgradeChanges`).
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

    /// Completes point selection: the cell goes into the match settings.
    pub fn commit_landing(&mut self) {
        if let PreGameStage::Landing(landing) = &self.stage {
            let index = usize::from(landing.team.0) % 4;

            self.setup.start_positions[index] = Some(landing.cursor);
        }

        self.advance();
    }

    /// Cancels the whole chain: returns the form to the `Players` screen.
    pub fn cancel(&mut self) {
        self.form.screen = SetupScreen::Players;
        self.form.status.clear();
    }
}

/// How many humans pass the phases (for logs/tests).
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
