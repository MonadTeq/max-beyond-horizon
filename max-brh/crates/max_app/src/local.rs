//! M-SP: local match (1 human + 1–3 computers) without the network.
//!
//! # What this is a port of
//!
//! The loop repeats `GameManager_GameLoop` + `GameManager_TeamTurnTurnBased`
//! (`max-cpp-port/src/game_manager.cpp:463`):
//!
//! 1. a round goes over the teams `RED → GREEN → BLUE → GRAY`; `None` and
//!    `Eliminated` teams are skipped;
//! 2. at the start of a team's turn `GameManager_ManageEconomy(team)` is called
//!    ([`max_core::GameState::manage_economy`]) and `GameManager_ProgressBuildState`
//!    (in max_core only the economy exists so far — production is P3/TODO);
//! 3. a computer team: `Ai_BeginTurn(team)` and the turn "spins" with ticks until
//!    `AiPlayer::CheckEndTurn` reaches `end_turn_retry_counter > 120` — in
//!    C++ it is at this threshold that `GameManager_RefreshOrders` sets
//!    `finished_turn` and `GAME_STATE_9_END_TURN`;
//! 4. a human team: wait for input; **End Turn** sends packet `06` and
//!    closes the turn;
//! 5. after the last team of the round — `GameState::finish_turn()`
//!    (`GameManager_TeamTurnFinish`): victory/defeat conditions, flag reset,
//!    `turn_counter += 1`.
//!
//! # Layer split (refactor #6)
//!
//! Steps 1, 2, 3 and 5 are the **deterministic** team-turn machine and live in
//! [`max_core::GameState`]: [`max_core::GameState::begin_round`] resets the round
//! cursor, `next_team` performs one step, `end_round` closes the round
//! (`finish_turn` + the next round). The machine is driven **only by the facade**
//! ([`max_core::universe::GameUniverse::end_local_turn`] /
//! [`max_core::universe::GameUniverse::begin_local_round`]), which reports every
//! step as [`max_core::universe::GameEvent::Turn`] with the pre-transition peek
//! values (`previous_player_team`, `status`). This module only consumes the
//! notices: the hot-seat «Begin turn» window, the autosave, the HUD log, the camera
//! focus, the turn timer and [`LocalPhase`] — it never writes simulation fields
//! itself and never calls `next_team`.
//!
//! Turn timer — `settings["timer"]` (`GameManager_UpdateTurnTimer`); `0` —
//! no limit. Pause (`PauseMenu`) stops both the ticks and the timer.
//!
//! Victory/defeat — [`max_core::victory::evaluate_status`]
//! (`WinLoss_EvaluateStatus`) at the moment the round is closed; the screen is drawn from it
//! [`crate::winloss`].

use max_core::enums::{GameStateType, PlayMode, WinLossState};
use max_core::game::TurnEvent;
use max_core::geo::Point;
use max_core::setup::{ResourceLevel, TeamStart};
use max_core::team::{TeamId, TeamKind};
use max_core::universe::GameEvent;
use max_core::victory::{self, MissionCategory, WinLossStatus};

use crate::hotseat::TurnNotice;
use crate::{App, NewGameConfig};

/// How many game teams are in a match (`PLAYER_TEAM_COUNT`).
pub const LOCAL_TEAM_COUNT: usize = 4;

/// Tick limit of one AI turn — a guard against a "hung" AI.
///
/// C++ ends the turn on `end_turn_retry_counter > 120` **only when the AI has no
/// unfinished work** (`tasks_pending`/`has_pending_work`), so in a
/// match with live tasks the turn may last long. The limit is like
/// `while (Access_AreTaskEventsPending())` + the emergency `force ENDTURN` from
/// `Ai_CheckEndTurn` (concurrent branch): after it the turn is closed forcibly.
pub const LOCAL_AI_TICK_LIMIT: u32 = 12_000;

/// Starting positions of the teams: the four quarters of the 112×112 map.
///
/// In C++ the positions come from the mission (`UnitsManager_TeamMissionSupplies`);
/// a generated custom match has no mission, so the map quarters are taken,
/// and `setup::find_valid_starting_position` moves them to the nearest land.
pub const START_POSITIONS: [Point; LOCAL_TEAM_COUNT] = [
    Point::new(22, 22),
    Point::new(88, 88),
    Point::new(88, 22),
    Point::new(22, 88),
];

/// Local match parameters — what the setup screen returns.
///
/// Default values — `settings.cpp` (`timer=180`, `endturn=45`,
/// `start_gold=150`, `play_mode=1`, `victory_type=0`, `victory_limit=50`,
/// `opponent=1`, resource levels `1/1/1/0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSetup {
    /// Human name (the player team).
    pub player_name: String,
    /// Planet `0..=23` (`settings["world"]`).
    pub world: i64,
    /// AI difficulty (`settings["opponent"]`, 0..=5).
    pub opponent: u8,
    /// Seconds per human turn (0 — no timer).
    pub timer: u32,
    /// Seconds to finish the AI turn (`settings["endturn"]`).
    pub endturn: u32,
    /// Game mode (`settings["play_mode"]`).
    pub play_mode: max_core::enums::PlayMode,
    /// Starting credits (`settings["start_gold"]`, 0..=250).
    pub start_gold: u32,
    /// Raw material level (`raw_resource` 0–2).
    pub raw: u8,
    /// Fuel level.
    pub fuel: u8,
    /// Gold level.
    pub gold: u8,
    /// Alien debris level.
    pub derelicts: u8,
    /// Victory condition (`victory_type`).
    pub victory_type: max_core::enums::VictoryType,
    /// Victory limit (`victory_limit`).
    pub victory_limit: i32,
    /// Who controls whom: 4 slots (`menu_team_player_setting`).
    pub team_types: [TeamKind; LOCAL_TEAM_COUNT],
    /// Clan of each team (`menu_team_clan_setting`); `Random` — 0.
    pub team_clans: [max_core::enums::TeamClan; LOCAL_TEAM_COUNT],
    /// M-HS: **hot-seat** match (`MISSION_CATEGORY_HOT_SEAT`).
    ///
    /// In C++ the mode is set not by a match setting but by the chosen menu item:
    /// `menu_custom_game_menu(is_single_player)` (`menu.cpp:2115`) loads
    /// a mission of the `MISSION_CATEGORY_HOT_SEAT` category, and the rules of
    /// `ChoosePlayerMenu` (how many humans are allowed) and the team names depend on it. Here the
    /// flag is stored next to `team_types`, so that the same rules can be checked
    /// without the UI (`LocalSetup::validate`).
    pub hot_seat: bool,
    /// Map generation seed (`Remote_RngSeed`).
    pub seed: u32,
    /// Landing points chosen in the site select phase (`GameManager_SelectSite`).
    ///
    /// `None` — the team did not go through the selection (AI or the old path):
    /// [`START_POSITIONS`] and `setup::find_valid_starting_position` are used.
    pub start_positions: [Option<Point>; LOCAL_TEAM_COUNT],
    /// Loadout after mobilization (`CargoMenu`): the free `SC_L0001` plus
    /// the purchased units. `None` — [`max_core::deploy::DEFAULT_MISSION_LOADOUT`].
    pub team_loadouts: [Option<Vec<max_core::deploy::TeamSupply>>; LOCAL_TEAM_COUNT],
    /// Credits left after mobilization (`TeamMissionSupplies::team_gold`).
    /// `None` — `start_gold`.
    pub team_gold: [Option<u32>; LOCAL_TEAM_COUNT],
    /// Attribute upgrades purchased in `CargoMenu`
    /// (`unitvalues_actual` → `TeamUnits.current_values` before landing).
    pub team_upgrades: [Option<Vec<(max_core::enums::UnitType, max_core::unit_values::UnitValues)>>;
        LOCAL_TEAM_COUNT],
    /// The team's `stats_gold_spent_on_upgrades` (`CargoMenu::CommitUpgradeChanges`).
    pub team_gold_spent: [Option<u32>; LOCAL_TEAM_COUNT],
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
            // `menu_choose_player_menu_loop`: RED — human, GREEN — computer,
            // BLUE/GRAY — empty.
            team_types: [
                TeamKind::Player,
                TeamKind::Computer,
                TeamKind::None,
                TeamKind::None,
            ],
            team_clans: [max_core::enums::TeamClan::Random; LOCAL_TEAM_COUNT],
            hot_seat: false,
            seed: 0,
            start_positions: [None; LOCAL_TEAM_COUNT],
            team_loadouts: std::array::from_fn(|_| None),
            team_gold: [None; LOCAL_TEAM_COUNT],
            team_upgrades: std::array::from_fn(|_| None),
            team_gold_spent: [None; LOCAL_TEAM_COUNT],
        }
    }
}

impl LocalSetup {
    /// The human team (the first `Player`); `None` — an invalid setup.
    pub fn human_team(&self) -> Option<TeamId> {
        self.team_types
            .iter()
            .position(|kind| *kind == TeamKind::Player)
            .map(|index| TeamId(index as u8))
    }

    /// All human teams in slot order (`GameManager_UpdateHumanPlayerCount`).
    pub fn human_teams(&self) -> Vec<TeamId> {
        crate::hotseat::human_teams(&self.team_types)
    }

    /// How many humans play (`GameManager_HumanPlayerCount`).
    pub fn human_count(&self) -> usize {
        self.team_types
            .iter()
            .filter(|kind| **kind == TeamKind::Player)
            .count()
    }

    /// Hot-seat match: more than one human
    /// (`GameManager_HumanPlayerCount > 1`, `game_manager.cpp:3276`).
    ///
    /// It is this count that makes C++ enable turn-based, show the
    /// «Begin turn» window and take the names `Red Team`/`Green Team`/…
    pub fn is_hot_seat(&self) -> bool {
        self.human_count() > 1
    }

    /// The setup was made via `Hot Seat Game`
    /// (`menu_custom_game_menu(false)`), even if there are fewer than two humans yet.
    ///
    /// Needed by the `ChoosePlayerMenu::EventDone` rules: hot-seat requires two
    /// humans, a single game — one human and a computer.
    pub fn hot_seat_settings(&self) -> bool {
        self.hot_seat || self.is_hot_seat()
    }

    /// Game mode taking `GameManager_UpdateHumanPlayerCount` into account: hot-seat is always
    /// turn-based (`GameManager_PlayMode = PLAY_MODE_TURN_BASED`), the
    /// "Simultaneous Moves" setting does not apply in it.
    pub fn effective_play_mode(&self) -> PlayMode {
        if self.is_hot_seat() {
            PlayMode::TurnBased
        } else {
            self.play_mode
        }
    }

    /// Team name in the match (`menu_team_name_setting`).
    ///
    /// In a single game the human gets `player_name` (`menu.cpp:1944`), in
    /// hot-seat all teams are called `Red Team`/`Green Team`/… — the player name
    /// there is one for all and is not substituted.
    pub fn display_name(&self, team: TeamId) -> String {
        if !self.is_hot_seat() && Some(team) == self.human_team() {
            self.player_name.clone()
        } else {
            crate::hotseat::team_display_name(team).to_string()
        }
    }

    /// Computer teams in slot order.
    pub fn ai_teams(&self) -> Vec<TeamId> {
        self.team_types
            .iter()
            .enumerate()
            .filter(|(_, kind)| **kind == TeamKind::Computer)
            .map(|(index, _)| TeamId(index as u8))
            .collect()
    }

    /// Playing teams (not `None`) in slot order.
    pub fn playing_teams(&self) -> Vec<TeamId> {
        self.team_types
            .iter()
            .enumerate()
            .filter(|(_, kind)| **kind != TeamKind::None)
            .map(|(index, _)| TeamId(index as u8))
            .collect()
    }

    /// Setup validation by `ChoosePlayerMenu::EventDone` (`chooseplayermenu.cpp:212`).
    ///
    /// Single game (`is_single_player = true`): at least two participants
    /// (human + computer), while «Human» on another slot in the UI copies
    /// the flag — there are never two humans there.
    ///
    /// Hot-seat (`is_single_player = false`): `human_player_count < 2` —
    /// error `_(06ec)`; computers in the mix are allowed (in C++ `EventDone`
    /// counts humans and computers separately, and `GameManager_UpdateHumanPlayerCount`
    /// simply force-enables turn-based).
    pub fn validate(&self) -> Result<(), String> {
        let humans = self.human_count();
        let playing = self.playing_teams().len();

        if playing < 2 {
            return Err(crate::hotseat::MSG_TWO_PLAYERS.to_string());
        }

        if humans == 0 {
            return Err("There must be at least one human player.".to_string());
        }

        if self.hot_seat_settings() {
            if humans < 2 {
                return Err(crate::hotseat::MSG_TWO_HUMANS.to_string());
            }

            return Ok(());
        }

        if humans > 1 {
            return Err("single player game allows only one human team".to_string());
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

    /// `NewGameConfig` for [`App::start_local_game`].
    ///
    /// If the start phases have already run (mobilization and/or site selection), their
    /// result goes into [`NewGameConfig::deployments`]: landing points — from
    /// [`Self::start_positions`], credits — from [`Self::team_gold`], loadout —
    /// from [`Self::team_loadouts`]. Without the phases the behavior is the former
    /// (one `SC_L0001` loadout and `start_gold` for all).
    pub fn new_game_config(&self) -> NewGameConfig {
        let level = |value: u8| ResourceLevel::new(value).unwrap_or(ResourceLevel::NORMAL);
        let human = self.human_team().unwrap_or(TeamId::RED);
        let phases_ran = self.start_positions.iter().any(Option::is_some)
            || self.team_loadouts.iter().any(Option::is_some)
            || self.team_gold.iter().any(Option::is_some)
            || self.team_upgrades.iter().any(Option::is_some);
        let teams: Vec<TeamStart> = self
            .playing_teams()
            .into_iter()
            .map(|team| {
                let index = team.0 as usize % LOCAL_TEAM_COUNT;

                TeamStart::new(
                    team,
                    self.start_positions[index].unwrap_or(START_POSITIONS[index]),
                )
            })
            .collect();
        let deployments = if phases_ran {
            teams
                .iter()
                .map(|start| {
                    let index = start.team.0 as usize % LOCAL_TEAM_COUNT;

                    max_core::deploy::TeamDeployment {
                        team: start.team,
                        position: start.position,
                        gold: self.team_gold[index].unwrap_or(self.start_gold),
                        supplies: self.team_loadouts[index]
                            .clone()
                            .unwrap_or_else(|| max_core::deploy::DEFAULT_MISSION_LOADOUT.to_vec()),
                        upgrades: self.team_upgrades[index].clone().unwrap_or_default(),
                        gold_spent_on_upgrades: self.team_gold_spent[index].unwrap_or(0),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        NewGameConfig {
            planet: planet_for_world(self.world),
            seed: self.seed,
            raw: level(self.raw),
            fuel: level(self.fuel),
            gold: level(self.gold),
            derelicts: level(self.derelicts),
            teams,
            player_team: human,
            start_gold: self.start_gold,
            clans: self.team_clans,
            deployments,
        }
    }
}

/// Planet by index `0..=23` (like `Planet::try_from`, with clamping).
pub fn planet_for_world(world: i64) -> max_core::enums::Planet {
    max_core::enums::Planet::try_from(world.clamp(0, 23) as u8)
        .unwrap_or(max_core::enums::Planet::Snowcrab)
}

/// Local match phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPhase {
    /// Human turn: waiting for input (or the timer).
    PlayerTurn,
    /// Computer turn of `team`.
    AiTurn(TeamId),
    /// The round is over, a transition is in progress (`finish_turn`/the next team).
    Advancing,
    /// The match is over (victory/defeat).
    Finished,
}

impl LocalPhase {
    /// Phase caption for the status line.
    pub fn caption(self) -> String {
        match self {
            Self::PlayerTurn => "Your turn".to_string(),
            Self::AiTurn(team) => format!("Computer turn: {}", team_name(team)),
            Self::Advancing => "Next turn".to_string(),
            Self::Finished => "Game over".to_string(),
        }
    }
}
/// Match result — data of the victory/defeat screen.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalOutcome {
    /// Whether the human team won (`menu_wrap_up_game`: `is_winner`).
    pub victory: bool,
    /// The team on whose turn the round closed (`GameManager_PlayerTeam`).
    ///
    /// In a single game this is the only human; in hot-seat — the one who moved
    /// last (C++ also passes `GameManager_PlayerTeam` to the screen).
    pub team: TeamId,
    /// `WinLoss_EvaluateStatus` at the moment the round is closed.
    pub status: WinLossStatus,
    /// The turn number at which the match ended.
    pub turn_counter: i32,
    /// Human team name (for the screen title).
    pub player_name: String,
}

/// Local match state.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalGame {
    /// Settings the match was started with.
    pub setup: LocalSetup,
    /// Phase.
    pub phase: LocalPhase,
    /// The team that is moving now.
    pub active_team: TeamId,
    /// Remaining turn timer, seconds (`GameManager_TurnTimerValue`).
    pub timer_left: f32,
    /// Pause (`PauseMenu`): ticks and the timer are stopped.
    pub paused: bool,
    /// Ticks in the current AI turn.
    pub ai_ticks: u32,
    /// Match result.
    pub outcome: Option<LocalOutcome>,
    /// M-HS: human teams in slot order (`GameManager_UpdateHumanPlayerCount`).
    pub human_teams: Vec<TeamId>,
    /// M-HS: the turn-transition window (`GameManager_UpdateGuiControl`); while it is
    /// open, the turn timer is stopped and input accepts only confirmation.
    pub notice: Option<TurnNotice>,
    /// Team names renamed by the player in game (`Preferences` →
    /// «Player Name»). C++ `OptionsMenu` in `PREFSPIC` edits
    /// `menu_team_name_setting[team]` (`optionsmenu.cpp:177,552`), not only
    /// the global `player_name`; `None` — the default name from
    /// [`LocalSetup::display_name`]. Not part of the simulation CRC: it is a UI
    /// name (like `Settings` in C++).
    pub team_names: [Option<String>; LOCAL_TEAM_COUNT],
}

impl LocalGame {
    /// A new match by the settings.
    pub fn new(setup: LocalSetup) -> Self {
        let human = setup.human_team().unwrap_or(TeamId::RED);
        let human_teams = setup.human_teams();

        Self {
            timer_left: setup.timer as f32,
            setup,
            phase: LocalPhase::PlayerTurn,
            active_team: human,
            paused: false,
            ai_ticks: 0,
            outcome: None,
            human_teams,
            notice: None,
            team_names: std::array::from_fn(|_| None),
        }
    }

    /// Team name in game: the rename from Preferences or
    /// [`LocalSetup::display_name`] (`menu_team_name_setting` C++).
    pub fn display_name(&self, team: TeamId) -> String {
        self.team_names
            .get(usize::from(team.0))
            .and_then(|name| name.clone())
            .unwrap_or_else(|| self.setup.display_name(team))
    }

    /// Renames a team (`OptionsMenu` `PREFSPIC` → `player_name`).
    ///
    /// In a single game the human team name coincides with `player_name`
    /// (`menu.cpp:1944`), so `setup.player_name` is updated too — otherwise
    /// the name would diverge between the victory screen and the V70 save.
    pub fn rename_team(&mut self, team: TeamId, name: impl Into<String>) {
        let name = name.into();

        if let Some(slot) = self.team_names.get_mut(usize::from(team.0)) {
            *slot = Some(name.clone());
        }

        if !self.is_hot_seat() && Some(team) == self.setup.human_team() {
            self.setup.player_name = name;
        }
    }

    /// The match is over.
    pub fn finished(&self) -> bool {
        self.outcome.is_some()
    }

    /// Whether the human won (for the screen).
    pub fn victory(&self) -> bool {
        self.outcome.as_ref().is_some_and(|outcome| outcome.victory)
    }

    /// Hot-seat match (`GameManager_HumanPlayerCount > 1`).
    pub fn is_hot_seat(&self) -> bool {
        self.human_teams.len() > 1
    }

    /// The turn-transition window is open.
    pub fn notice(&self) -> Option<&TurnNotice> {
        self.notice.as_ref()
    }

    /// Confirmation of the turn-transition window (`DialogMenu`: a click on `OK`, Enter, Esc).
    ///
    /// Returns `true` if the window actually closed.
    pub fn dismiss_notice(&mut self) -> bool {
        self.notice.take().is_some()
    }

    /// The human turn is in progress (`LocalPhase::PlayerTurn`).
    pub fn is_human_turn(&self) -> bool {
        self.phase == LocalPhase::PlayerTurn
    }

    /// Order input is blocked: AI turn, transition or the open transition window.
    ///
    /// In C++ during the computer turn the map is not redrawn
    /// (`game_manager.cpp:1179`) and orders to foreign/inactive teams are
    /// discarded (`GameManager_ActiveTurnTeam`), and while `DialogMenu` is open,
    /// the loop sits in `Run()` at all.
    pub fn input_blocked(&self) -> bool {
        self.notice.is_some() || self.phase != LocalPhase::PlayerTurn
    }

    /// Phase caption taking the team into account (hot-seat: whose turn).
    pub fn phase_caption(&self) -> String {
        match self.phase {
            LocalPhase::PlayerTurn => {
                if self.is_hot_seat() {
                    format!(
                        "{}'s turn",
                        crate::hotseat::team_display_name(self.active_team)
                    )
                } else {
                    "Your turn".to_string()
                }
            }
            LocalPhase::AiTurn(team) => format!("Computer turn: {}", team_name(team)),
            LocalPhase::Advancing => "Next turn".to_string(),
            LocalPhase::Finished => "Game over".to_string(),
        }
    }

    /// Match status line (phase, turn, timer).
    pub fn status_line(&self, turn_counter: i32) -> String {
        let timer = if self.setup.timer == 0 {
            String::new()
        } else if self.phase == LocalPhase::PlayerTurn {
            format!(" · {}s", self.timer_left.ceil().max(0.0) as i32)
        } else {
            String::new()
        };

        let paused = if self.paused { " · PAUSED" } else { "" };
        let notice = match self.notice.as_ref() {
            Some(notice) => format!(" · {} begins turn", team_name(notice.team)),
            None => String::new(),
        };

        format!(
            "LOCAL · turn {} · {}{}{}{} · LMB select · RMB move · End Turn",
            turn_counter,
            self.phase_caption(),
            timer,
            notice,
            paused
        )
    }
}

/// What [`App::local_update`] did per call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalUpdate {
    /// The simulation tick ran.
    pub ticked: bool,
    /// A turn was closed (human or AI).
    pub turn_closed: bool,
    /// The computer turn started.
    pub ai_started: bool,
    /// The match ended in this call.
    pub finished: bool,
}

/// Team name for logs/the screen (`menu_team_names`).
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
    /// Starts a local match (`menu_custom_game_menu` → `GameManager_InitGame`).
    ///
    /// Sets `team_types` from the settings (`TeamKind`), `network_game = false`
    /// (`Remote_IsNetworkGame`), the game mode, timers and the victory condition, then
    /// chooses AI strategies (`Ai_SetupStrategy` for each computer
    /// team — `GameManager_InitGame`) and passes the first turn.
    pub fn start_local_game(&mut self, setup: &LocalSetup) -> Result<(), String> {
        setup.validate()?;

        // `GameConfigMenu` `Done` writes the same `settings["timer"]`/`["endturn"]`
        // (`gameconfigmenu.cpp:134-141,273-274`) that `OptionsMenu` shows and edits;
        // afterwards the match reads them at every turn start
        // (`GameManager_UpdateGui`, `game_manager.cpp:2416-2471`).
        self.preferences.turn_timer = setup.timer.min(i32::MAX as u32) as i32;
        self.preferences.endturn = setup.endturn.min(i32::MAX as u32) as i32;
        // The name also comes from the settings (`settings["player_name"]`,
        // `menu.cpp:1944`): the field on the setup screen writes it back the same
        // way as `GameConfigMenu` — into `settings`.
        if !setup.player_name.is_empty() {
            self.preferences.player_name = setup.player_name.clone();
        }

        let human = setup.human_team().unwrap_or(TeamId::RED);
        let config = setup.new_game_config();
        let report = self
            .new_game(&config)
            .map_err(|error| format!("local game: {error:#}"))?;

        let enemy_halt = self.preferences.enemy_halt;
        let state = self.state_mut();

        // `Remote_IsNetworkGame = false` — a local match, not lockstep.
        state.network_game = false;
        // `GameManager_InitGame`: `GameManager_FastMovement`/`enemy_halt` —
        // live settings; in the port simulation the second is a state field.
        state.enemy_halt = enemy_halt;
        // `GameManager_UpdateHumanPlayerCount`: hot-seat is always turn-based.
        state.play_mode = setup.effective_play_mode();
        state.victory_type = setup.victory_type;
        state.victory_limit = setup.victory_limit;
        // `menu_custom_game_menu`: `MISSION_CATEGORY_CUSTOM` / `MISSION_CATEGORY_HOT_SEAT`.
        state.mission_category = if setup.is_hot_seat() {
            MissionCategory::HotSeat
        } else {
            MissionCategory::Custom
        };
        state.mission_index = 0;
        state.player_team = human;
        // `GameManager_InitGame`: `GameManager_PlayerTeam = PLAYER_TEAM_ALIEN`
        // (or the first computer team). In hot-seat it follows that the
        // «Begin turn» window is shown to the first human team too —
        // `GameManager_PlayerTeam != team` (`game_manager.cpp:3076`, `:2440`).
        if setup.is_hot_seat() {
            state.player_team = TeamId::ALIEN;
        }
        state.active_turn_team = human;
        state.game_state = GameStateType::InGame;
        state.turn_counter = 1;
        // `new_game` sets `Player` only for the first team; in hot-seat there are
        // several humans (`GameManager_InitGame`: each team's type from the settings).
        for team in setup.human_teams() {
            state.teams[team.0 as usize].team_type = TeamKind::Player;
        }
        state.teams[human.0 as usize].team_type = TeamKind::Player;
        for index in 0..LOCAL_TEAM_COUNT {
            state.teams[index].team_clan = setup.team_clans[index];
        }
        state.ai.settings.opponent = setup.opponent;
        state.ai.all_visible = false;

        // `Ai_Init` + `Ai_SetupStrategy` for computer teams
        // (`GameManager_InitGame`).
        max_core::ai_init(state);
        for team in setup.ai_teams() {
            let _ = max_core::ai_setup_strategy(state, team);
        }

        let local = LocalGame::new(setup.clone());

        tracing::info!(
            "local game: planet={:?} teams={} aliens={} units={} player={:?} humans={:?} ai={:?} mode={:?} hot_seat={}",
            report.planet,
            report.teams,
            report.aliens,
            report.units,
            human,
            setup.human_teams(),
            setup.ai_teams(),
            setup.play_mode,
            setup.is_hot_seat()
        );

        self.local = Some(local);
        // P3: match settings for the V70 write (`SaveFormatHeader`/`IniOptions`),
        // plus the player's current `IniPreferences` (7 × `signed int`).
        let meta =
            crate::saveload::save_meta_for_local_game_with(setup, &self.preferences.to_ini());
        self.state_mut().set_save_meta(meta);
        self.hud.clear();
        self.close_unit_menu();
        if setup.is_hot_seat() {
            self.hud.push_log(format!(
                "Hot seat: {} human players, {} computer(s)",
                setup.human_count(),
                setup.ai_teams().len()
            ));
        } else {
            self.hud.push_log(format!(
                "Local game: {} vs {} computer(s)",
                setup.player_name,
                setup.ai_teams().len()
            ));
        }
        self.local_begin_round();

        Ok(())
    }

    /// Continues a local match from a save container (see
    /// [`max_app::session_save`]).
    pub fn set_local_game(&mut self, local: LocalGame) {
        self.local = Some(local);
    }

    /// Current local match.
    pub fn local(&self) -> Option<&LocalGame> {
        self.local.as_ref()
    }

    /// Editable local match.
    pub fn local_mut(&mut self) -> Option<&mut LocalGame> {
        self.local.as_mut()
    }

    /// Whether a local match is in progress.
    pub fn local_active(&self) -> bool {
        self.local.is_some()
    }

    /// Exit to the menu: the local match is forgotten.
    pub fn leave_local_game(&mut self) {
        self.local = None;
        self.close_build_menu();
        self.close_build_site();
        self.close_research_menu();
        self.close_unit_menu();
        self.close_help();
        self.hud.clear();
    }

    /// `GameManager_TeamTurnTurnBased` round entry: the deterministic round
    /// cursor is reset and the machine advances to the first playing team — both
    /// inside the facade ([`max_core::universe::GameUniverse::begin_local_round`]).
    /// The application only sets the [`LocalPhase`] and consumes the notices.
    fn local_begin_round(&mut self) {
        {
            let mut local = self.local.take().expect("local game must exist");
            local.phase = LocalPhase::Advancing;
            self.local = Some(local);
        }
        self.universe.begin_local_round();
        self.local_next_team();
    }

    /// Round entry of a **loaded** match — `GAME_STATE_10_LOAD_GAME`
    /// (`game_manager.cpp:458-480`).
    ///
    /// The facade resumes at the saved `active_turn_team` and skips the economy of
    /// that team ([`max_core::universe::GameUniverse::resume_local_round`]): it was
    /// already applied before the save. C++ starts exactly one team here and breaks
    /// out of the turn loop (`:476-480`), so the application does not run the round
    /// machine beyond that first notice.
    ///
    /// If the saved active slot is not a playing team any more (a team
    /// `TEAM_TYPE_NONE`/`TEAM_TYPE_ELIMINATED`, or a slot after the last playing
    /// one), C++ never enters the loop body and the game loop falls through; a local
    /// match in the port cannot exit that way, so it falls back to the normal round
    /// entry — the only path that can make progress (economy applied once, there is
    /// no saved turn to duplicate).
    fn local_resume_round(&mut self) {
        {
            let mut local = self.local.take().expect("local game must exist");
            local.phase = LocalPhase::Advancing;
            self.local = Some(local);
        }
        if !self.universe.resume_local_round() {
            self.universe.begin_local_round();
        }
        self.local_next_team();
    }

    /// Consumes [`max_core::universe::GameEvent::Turn`] notices until a team has to
    /// act (a human turn or a computer turn) or the match is over.
    ///
    /// The deterministic transitions are performed by the facade
    /// (`GameUniverse::end_local_turn` / `begin_local_round`); this layer only
    /// reacts to the notices: the hot-seat «Begin turn» window, the autosave, the
    /// HUD log, the camera focus, the turn timer and [`LocalPhase`].
    ///
    /// The two values the UI needs *before* the step (`previous_player_team`,
    /// `WinLoss_Status`) are peeked by the facade and carried in
    /// [`max_core::universe::TurnNotice`] — in C++ `GameLoop` evaluates
    /// `WinLoss_Status` before `GameManager_TeamTurnFinish`, and
    /// `GameManager_UpdateGui` compares `GameManager_PlayerTeam` before switching
    /// it to the new human team (`local.rs:790-804` before the move).
    ///
    /// The cursor is the dedicated [`App::turn_events`] position in the shared
    /// event buffer (owner decision 6): draining turn notices does not consume the
    /// combat sounds of the other cursor.
    fn local_next_team(&mut self) {
        if self.local.is_none() {
            return;
        }

        // Separate field borrows: the cursor and the facade are disjoint fields.
        let events = self.turn_events.drain(&mut self.universe);

        for event in events {
            let GameEvent::Turn(notice) = event else {
                // Stage A: the local match still reads its UI reports from the
                // state (`local_finish_round`); other stream events have no
                // application-side consumer here.
                continue;
            };

            match notice.event {
                TurnEvent::Human(team) => {
                    self.local_start_human_turn(team, notice.previous_player_team);
                    return;
                }
                TurnEvent::Computer(team) => {
                    self.local_start_ai_turn(team);
                    return;
                }
                TurnEvent::RoundClosed => self.local_finish_round(),
                TurnEvent::GameOver => {
                    // `GameOver` is only reported by the closing step, so the
                    // pre-step status is the one C++ passes to the screen.
                    let status = notice.status.unwrap_or_else(|| {
                        victory::evaluate_status(self.state(), self.state().turn_counter)
                    });
                    self.local_game_over(status);
                    return;
                }
            }
        }
    }

    /// Closes `team`'s turn through the facade and consumes the resulting notices.
    ///
    /// The whole transition — `06` (immediately), `enter_end_turn`, the team-turn
    /// machine and the next-turn entry — is one facade operation
    /// ([`max_core::universe::GameUniverse::end_local_turn`]); `next_team` is not
    /// called from the application any more (stage-A task §6 item 3).
    ///
    /// P4 profiling: `GameManager_TeamTurnFinish` runs inside the facade, so the
    /// `finish_turn` scope covers the whole facade transition. The peek that only
    /// the closing step is measured is exact: `upcoming_team()` does not change
    /// before the machine runs (the 06 apply does not move the round cursor).
    fn local_close_turn(&mut self, team: TeamId) {
        let closing = self.state().upcoming_team().is_none();
        let turn_counter = self.state().turn_counter;
        let _profile = closing.then(|| crate::profile::finish_turn_scope(turn_counter));

        self.universe.end_local_turn(team);
        self.local_next_team();
    }

    /// Human team turn start (`GameManager_UpdateGui`, `TEAM_TYPE_PLAYER`): the
    /// timer, the hot-seat «Begin turn» window, the selection reset, the camera
    /// focus and the HUD log.
    fn local_start_human_turn(&mut self, team: TeamId, previous_player_team: TeamId) {
        let (setup_timer, hot_seat) = self
            .local
            .as_ref()
            .map(|local| (local.setup.timer, local.is_hot_seat()))
            .unwrap_or((0, false));
        let timer_left = if setup_timer == 0 {
            0.0
        } else {
            // C++: `ini_timer_setting == 0 && player is computer → 45`,
            // for a human 0 means "no timer".
            setup_timer as f32
        };

        // `GameManager_UpdateGui`: the «Begin turn» window is shown if
        // the human team changed (`GameManager_PlayerTeam != team`)
        // and there is more than one human (`GameManager_HumanPlayerCount`).
        let notice = (hot_seat && previous_player_team != team).then(|| TurnNotice::new(team));

        {
            let mut local = self.local.take().expect("local game");
            local.phase = LocalPhase::PlayerTurn;
            local.active_team = team;
            local.timer_left = timer_left;
            local.ai_ticks = 0;
            local.notice = notice;
            self.local = Some(local);
        }

        // The selection does not move between players: otherwise the new player
        // would order a foreign unit (`GameManager_UpdateGuiControl`).
        self.hud.clear_interaction();
        // The power message (`ProductionManager::DrawResourceMessage`)
        // is re-established after the turn change: `clear_interaction`
        // removes `hint` together with the selection.
        if let Some(message) = self.state().industry_message.clone() {
            self.hud.set_hint(message);
        }
        // The command popup belongs to the previous selection
        // (`GameManager_DeinitPopupButtons` on the turn change).
        self.close_unit_menu();
        self.focus_player();

        if let Some(notice) = notice {
            self.hud.push_log(crate::hotseat::notice_log_line(&notice));
        } else {
            self.hud.push_log(if hot_seat {
                format!(
                    "Turn {}: {} moves",
                    self.state().turn_counter,
                    team_name(team)
                )
            } else {
                format!("Turn {}: your move", self.state().turn_counter)
            });
        }
    }

    /// Computer team turn start (`GameManager_UpdateGui`, `TEAM_TYPE_COMPUTER`):
    /// the core already ran `Ai_BeginTurn(team)`, the application starts the AI
    /// timer and the [`LocalPhase::AiTurn`].
    fn local_start_ai_turn(&mut self, team: TeamId) {
        let mut local = self.local.take().expect("local game");
        local.phase = LocalPhase::AiTurn(team);
        local.active_team = team;
        local.ai_ticks = 0;
        // `GameManager_UpdateGui` for `TEAM_TYPE_COMPUTER`:
        // `ini_timer_setting = settings["endturn"]; if (< 45) = 45;` —
        // the AI turn timer starts from this value (the panel shows it
        // with the same `GameManager_DrawTurnTimer`), so it is visible also during the
        // computer turn (in QA — 00:45).
        local.timer_left = local.setup.endturn.max(45) as f32;
        self.local = Some(local);
        self.hud.push_log(format!(
            "Turn {}: {} (computer)",
            self.state().turn_counter,
            team_name(team)
        ));
    }

    /// `GameManager_TeamTurnFinish`, non-final branch: the production/research
    /// reports of the closed round, the next round entry, the autosave.
    ///
    /// The core already ran `finish_turn` (victory check, `finished_turn` reset,
    /// score graph, `turn_counter += 1`, production/research turns) and entered
    /// the next round; the caller continues with [`Self::local_next_team`].
    ///
    /// The production/research event queues are UI/QA reports (they are not part
    /// of the snapshot/CRC): taking them is the application's consumption of the
    /// core's report, not a simulation transition.
    fn local_finish_round(&mut self) {
        // Production turn events (`GameManager_ReportNewUnitsMessage`):
        // only for the local player, so that the log is not flooded with foreign
        // factories.
        let player = self.state().player_team;
        let events = self.state_mut().take_production_events();

        for event in events.iter().filter(|event| event.builder.team() == player) {
            let name = crate::buildmenu::unit_name(event.unit_type);

            match event.kind {
                max_core::ProductionEventKind::Spawned => {
                    self.hud.push_log(format!("{name} ready"))
                }
                max_core::ProductionEventKind::BuildingDone => {
                    self.hud.push_log(format!("{name} built"))
                }
                max_core::ProductionEventKind::PausedNoMaterials => self
                    .hud
                    .push_log("Production paused: not enough raw materials"),
                max_core::ProductionEventKind::PausedNoResources => self
                    .hud
                    .push_log("Production paused: not enough raw materials"),
                max_core::ProductionEventKind::PausedNoPower => {
                    self.hud.push_log("Production paused: not enough power")
                }
                max_core::ProductionEventKind::PausedNoFuel => {
                    self.hud.push_log("Production paused: not enough fuel")
                }
                max_core::ProductionEventKind::PausedNoWorkers => {
                    self.hud.push_log("Production paused: not enough workers")
                }
                max_core::ProductionEventKind::PausedNoGold => {
                    self.hud.push_log("Production paused: not enough gold")
                }
                max_core::ProductionEventKind::NoSpawnCell => self
                    .hud
                    .push_log("Production: no free cell near the factory"),
                max_core::ProductionEventKind::RubbleCleared => {
                    self.hud.push_log("Rubble cleared")
                }
            }
        }

        // `ResearchMenu_NewTurn`: completed topics (P6).
        let research = self.state_mut().take_research_events();
        for event in research.iter().filter(|event| event.team == player) {
            self.hud.push_log(format!(
                "Research complete: {:?} level {}",
                event.topic, event.level
            ));
        }

        // `ProductionManager::DrawResourceMessage` (`production_manager.cpp:792-804`):
        // «Adjustments made: …» over the map (`MESSAGE_BOX_NOTICE`).
        // The message lives in the state until the end of the round and is re-established
        // on entering the human turn (the turn change clears `hint`).
        if let Some(message) = self.state().industry_message.clone() {
            self.hud.set_hint(message);
        }

        {
            let mut local = self.local.take().expect("local game");
            local.phase = LocalPhase::Advancing;
            self.local = Some(local);
        }

        self.hud
            .push_log(format!("Turn {} begins", self.state().turn_counter));
        // C++ `GameManager_UpdateGui(team, …, enable_autosave = true)` for the first
        // team of the round: autosave (slot 10) with `SETUP.auto_save` enabled.
        self.autosave();
    }

    /// `GameManager_TeamTurnFinish`, final branch: the round close reported the
    /// end of the match (`menu_check_end_game_conditions`); the screen data is
    /// assembled from the `WinLoss_Status` evaluated **before** the close.
    fn local_game_over(&mut self, status: WinLossStatus) {
        let player_team = self.state().player_team;
        let is_winner = if status.team_status[player_team.0 as usize] == WinLossState::Generic {
            status.team_rank[player_team.0 as usize] == 1
        } else {
            status.team_status[player_team.0 as usize] == WinLossState::Won
        };
        // `finish_turn` does not advance the counter when the game is over.
        let turn_counter = self.state().turn_counter;

        let mut local = self.local.take().expect("local game");
        local.phase = LocalPhase::Finished;
        local.notice = None;
        local.outcome = Some(LocalOutcome {
            victory: is_winner,
            team: player_team,
            status,
            turn_counter,
            player_name: local.display_name(player_team),
        });
        self.local = Some(local);
        self.hud.push_log(if is_winner {
            "Game over: you win"
        } else {
            "Game over: you lose"
        });
    }

    /// Human End Turn: packet `06` (`GameManager_RefreshOrders` →
    /// `Remote_SendNetPacket_06`) and the transition to the next team of the round.
    ///
    /// While the turn-transition window is open (`notice`), the turn is not closed: first
    /// the player confirms «Begin turn» (`DialogMenu::Run` in C++ loops).
    pub fn local_end_turn(&mut self) {
        let Some(local) = self.local.as_ref() else {
            return;
        };

        if local.phase != LocalPhase::PlayerTurn || local.notice.is_some() {
            return;
        }

        let team = local.active_team;

        self.hud.push_log(format!("{} ended turn", team_name(team)));

        if let Some(local) = self.local.as_mut() {
            local.phase = LocalPhase::Advancing;
        }

        // `GameManager_RefreshOrders`: the team's turn is over, the phase becomes
        // `GAME_STATE_9_END_TURN` and the machine advances. Both happen inside the
        // facade: `06` is applied immediately (not queued — the simultaneous mode
        // needs a synchronous `finished_turn`), then `enter_end_turn` + the turn
        // machine (stage-A task §6 item 2/3).
        self.local_close_turn(team);
    }

    /// Confirmation of the M-HS turn-transition window (`DialogMenu`: a click on `OK`,
    /// Enter or Esc). Returns `true` if the window was open.
    ///
    /// After closing, the real team turn begins: the timer
    /// (`GameManager_UpdateTurnTimer`) starts only now — exactly as in
    /// `GameManager_UpdateGui`, where the window is shown before `IsTurnTimerActive`.
    pub fn local_acknowledge_turn(&mut self) -> bool {
        let Some(local) = self.local.as_mut() else {
            return false;
        };

        if !local.dismiss_notice() {
            return false;
        }

        let team = local.active_team;
        self.hud
            .push_log(format!("{}: begin turn", team_name(team)));
        true
    }

    /// The turn-transition window, if it is open.
    pub fn local_notice(&self) -> Option<&TurnNotice> {
        self.local.as_ref().and_then(|local| local.notice())
    }

    /// Orders/selection are blocked (AI turn, transition, transition window).
    pub fn local_input_blocked(&self) -> bool {
        self.local
            .as_ref()
            .is_some_and(|local| local.input_blocked())
    }

    /// Pause/resume. Returns the new state (`true` — paused).
    pub fn local_toggle_pause(&mut self) -> bool {
        let Some(local) = self.local.as_mut() else {
            return false;
        };
        local.paused = !local.paused;
        let paused = local.paused;
        self.hud.push_log(if paused { "Paused" } else { "Resumed" });
        paused
    }

    /// One frame of the local match: simulation tick, timer, AI turns.
    ///
    /// `dt` — seconds since the last frame (the turn timer and the visible unit movement).
    /// Simulation determinism does not depend on `dt`: ticks go one per frame,
    /// as in `GameManager_ProcessTick`.
    ///
    /// While the turn-transition window is open (M-HS), the simulation does not tick (in C++
    /// the loop sits in `DialogMenu::Run()`), but the visual route of the already moved units
    /// finishes — otherwise movement would break on the turn change.
    pub fn local_update(&mut self, dt: f32) -> LocalUpdate {
        let mut update = LocalUpdate::default();

        let Some(local) = self.local.as_ref() else {
            return update;
        };
        // `OptionsMenu::Run` in C++ blocks the game loop: while the `Preferences`
        // dialog is open, the simulation and the turn timer are stopped.
        if self.preferences_dialog.is_some() {
            return update;
        }
        let paused = local.paused;
        let phase = local.phase;
        let notice_open = local.notice.is_some();

        if paused {
            return update;
        }
        if phase == LocalPhase::Finished || notice_open {
            self.update_visual_units(dt);
            return update;
        }
        // P4: frame time of the local match (`MAX_TICK_PROFILE=1`). `dt` does not
        // affect the simulation — still one tick per frame.
        let _profile = crate::profile::local_update_scope();

        // Edge camera scrolling (`Preferences`: `click_scroll`/`quick_scroll`)
        // goes in the same frame as the game: `GameManager_ProcessInput` →
        // `GameManager_ScrollMainMapView`.
        self.update_scroll(dt);

        match phase {
            LocalPhase::PlayerTurn => {
                // `GameManager_AutoSelectNext` after an order completes
                // (`UnitInfo::MoveFinished`): the order snapshot — before the tick.
                let before = self.selected_order_snapshot();
                self.state_mut().tick();
                // Combat sounds of this tick (`UnitInfo::PrepareFire`/
                // `StartExplosion`): the queue is taken by `main.rs` (`AudioPlayer`).
                self.collect_combat_sounds();
                // `App::tick` counts ticks for diagnostics/the digest; the local
                // loop ticks right here, so the counter must grow in the window too,
                // otherwise the `MAX_DEBUG_INPUT` trace shows an eternal `tick 0`.
                self.ticks += 1;
                self.auto_select_after_tick(before);
                self.follow_selected_unit();
                self.update_visual_units(dt);
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
                let before = self.selected_order_snapshot();
                self.state_mut().tick();
                // AI combat is heard by the player the same as own (`PlaySfx` in C++ does not
                // depend on the unit owner) — the frame queue.
                self.collect_combat_sounds();
                self.ticks += 1;
                self.auto_select_after_tick(before);
                self.follow_selected_unit();
                self.update_visual_units(dt);
                update.ticked = true;

                let timed_out = {
                    let local = self.local.as_ref().expect("local game");
                    local.ai_ticks + 1 >= LOCAL_AI_TICK_LIMIT
                };

                // `AiPlayer::CheckEndTurn` ticks itself (`GameState::tick` →
                // `ai_check_end_turn`); here we read its retry counter:
                // C++ `GameManager_RefreshOrders` closes the turn at `> 120`.
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
                    // `GameManager_SyncTurnTimer` ticks during the AI turn too —
                    // `GameManager_TurnTimerValue` decreases by seconds
                    // (`game_manager.cpp:5033-5063`). Timer expiry does not close the AI turn:
                    // it is closed by `AiPlayer::CheckEndTurn`
                    // (`GameManager_RefreshOrders`).
                    local.timer_left = (local.timer_left - dt.max(0.0)).max(0.0);
                }

                if timed_out {
                    self.hud.push_log(format!(
                        "{}: force end turn ({} ticks)",
                        team_name(team),
                        LOCAL_AI_TICK_LIMIT
                    ));
                    // Diagnostics of a "hung" turn: `orders_pending`/
                    // `tasks_pending` explain why `Ai_CheckEndTurn` was not
                    // called (a projectile/explosion in `Destroy`, a moving unit).
                    tracing::warn!(
                        "AI turn timeout: team={team:?} orders_pending={} \
                         tasks_pending={} counter={} finished_turn={}",
                        self.state().orders_pending,
                        self.state().ai.players[team.0 as usize].tasks_pending,
                        self.state().ai.players[team.0 as usize].end_turn_retry_counter,
                        self.state().teams[team.0 as usize].finished_turn,
                    );
                }

                if finished || timed_out {
                    update.turn_closed = true;

                    if let Some(local) = self.local.as_mut() {
                        local.phase = LocalPhase::Advancing;
                    }

                    // `GameManager_RefreshOrders` for the AI turn: `06` immediately,
                    // `GAME_STATE_9_END_TURN` and the machine — all inside the
                    // facade (same path as the human End Turn, stage-A task §6 item 3).
                    self.local_close_turn(team);

                    let started_ai = matches!(
                        self.local.as_ref().map(|local| local.phase),
                        Some(LocalPhase::AiTurn(_))
                    );
                    update.ai_started = started_ai;
                }
            }
            LocalPhase::Advancing => {
                // The visual route also runs in the transition phase between
                // teams, until the turn-transition window opens.
                self.update_visual_units(dt);
                // A pure consumer in stage A: the machine has already run inside
                // the facade, so this only applies notices that are still unread.
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
// Local match saving
// ---------------------------------------------------------------------------

/// The magic of the local session container (`M.A.X. Local Session`).
pub const LOCAL_SAVE_MAGIC: [u8; 4] = *b"MXLS";
/// Container version.
///
/// `2` — added the `LocalSetup::hot_seat` flag (M-HS); `3` — team clans;
/// `4` — team names renamed in game (`LocalGame::team_names`).
/// Version 1 files are rejected; the container is local and survives only one
/// build.
pub const LOCAL_SAVE_VERSION: u16 = 4;
/// Local match quick-save file name.
pub const LOCAL_SAVE_FILE: &str = "max-local.mxs";

/// Local match save container: settings + phase + state snapshot.
///
/// # Why not V70
///
/// V70 **is readable** (`max_core::save::SaveFile::parse` → `GameState::from_save`,
/// including in a local match — [`App::start_local_game_from_save`]), but
/// there is no `GameState → SaveFile` write in the port: V70 stores an object tree
/// (`SaveUnit` + `HeatMap` + `AiPlayerState` + hashes) that `GameState`
/// does not have in that form. The `MXLS` container stores `GameState::snapshot_bytes()`
/// (the canonical digest of the whole simulation) and the match metadata; it checks
/// the save integrity (snapshot CRC), but restoring the simulation from it
/// requires a state deserializer — an open item (`max-brh-docs/plan-modes.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSave {
    /// Match settings.
    pub setup: LocalSetup,
    /// Phase.
    pub phase: LocalPhase,
    /// Active team.
    pub active_team: TeamId,
    /// Remaining timer in milliseconds.
    pub timer_left_ms: u32,
    /// Pause.
    pub paused: bool,
    /// Round cursor of the deterministic turn machine
    /// (`GameState::round_team`). Kept in the container byte layout for
    /// compatibility; the simulation value lives in `max_core`.
    pub round_team: u8,
    /// Ticks in the current AI turn.
    pub ai_ticks: u32,
    /// The match is over.
    pub finished: bool,
    /// Human victory.
    pub victory: bool,
    /// Turn number.
    pub turn_counter: i32,
    /// State snapshot (`GameState::snapshot_bytes`).
    pub snapshot: Vec<u8>,
    /// Snapshot CRC16 (`max_core::crc::crc16`).
    pub snapshot_crc: u16,
    /// Team names from Preferences (`LocalGame::team_names`).
    pub team_names: [Option<String>; LOCAL_TEAM_COUNT],
}

impl LocalSave {
    /// Assembles the container from the current application state.
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
            round_team: app.state().round_team.min(u8::MAX as usize) as u8,
            ai_ticks: local.ai_ticks,
            finished: local.finished(),
            victory: local.victory(),
            turn_counter: app.state().turn_counter,
            snapshot,
            snapshot_crc,
            team_names: local.team_names.clone(),
        })
    }

    /// Serializes the container into bytes.
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
        out.extend(self.setup.team_clans.iter().map(|clan| *clan as u8));
        out.push(u8::from(self.setup.hot_seat));
        out.extend_from_slice(&self.setup.seed.to_le_bytes());
        // v4: team names (`LocalGame::team_names`).
        for name in &self.team_names {
            match name {
                Some(name) => {
                    out.push(1);
                    put_string(&mut out, name);
                }
                None => out.push(0),
            }
        }

        // phase
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

    /// Parses the container and checks the snapshot CRC.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = SaveReader::new(bytes);

        let magic = reader.bytes(4)?;
        if magic != LOCAL_SAVE_MAGIC {
            return Err("not a local session save (bad magic)".to_string());
        }
        let version = reader.u16()?;
        if version < 2 || version > LOCAL_SAVE_VERSION {
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
        let mut team_clans = [max_core::enums::TeamClan::Random; LOCAL_TEAM_COUNT];
        if version >= 3 {
            for slot in team_clans.iter_mut() {
                *slot = max_core::enums::TeamClan::try_from(reader.u8()?)
                    .unwrap_or(max_core::enums::TeamClan::Random);
            }
        }
        let hot_seat = reader.u8()? != 0;
        let seed = reader.u32()?;
        let mut team_names: [Option<String>; LOCAL_TEAM_COUNT] = std::array::from_fn(|_| None);
        if version >= 4 {
            for slot in team_names.iter_mut() {
                if reader.u8()? != 0 {
                    *slot = Some(reader.string()?);
                }
            }
        }

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
                team_clans,
                hot_seat,
                seed,
                start_positions: [None; LOCAL_TEAM_COUNT],
                team_loadouts: std::array::from_fn(|_| None),
                team_gold: [None; LOCAL_TEAM_COUNT],
                team_upgrades: std::array::from_fn(|_| None),
                team_gold_spent: [None; LOCAL_TEAM_COUNT],
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
            team_names,
        })
    }

    /// Remaining timer in seconds.
    pub fn timer_left(&self) -> f32 {
        self.timer_left_ms as f32 / 1000.0
    }
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Minimal little-endian container reader.
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
    /// Starts a local match **from a V70 save** (`SaveLoad_Load` + the local loop).
    ///
    /// Loading V70 in local mode: the state from the file, `network_game = false`,
    /// the teams and `player_team` — from the save, the game mode and timers — from the setup
    /// screen. After loading the usual M-SP loop runs (AI turns, End Turn).
    pub fn start_local_game_from_save(
        &mut self,
        setup: &LocalSetup,
        bytes: &[u8],
    ) -> Result<(), String> {
        self.load_save_bytes(bytes)
            .map_err(|error| format!("V70 load: {error:#}"))?;

        // `SaveLoad_Load` (`saveload.cpp:920-934`): the file's `IniPreferences`
        // are applied to the player settings — `effects` controls the frames again.
        let saved_preferences = self.state().save_meta.ini_preferences;
        self.preferences.apply_ini(saved_preferences);
        // `SaveLoad_Load` (`saveload.cpp:1175-1176`): the timers from the file become
        // `settings["timer"]`/`["endturn"]` again.
        let saved_timer = self.state().save_meta.ini_options.turn_timer.max(0);
        let saved_endturn = self.state().save_meta.ini_options.endturn.max(0);
        self.preferences.turn_timer = saved_timer;
        self.preferences.endturn = saved_endturn;
        // `settings` settings the simulation reads (`enemy_halt`).
        self.state_mut().enemy_halt = self.preferences.enemy_halt;

        let state = self.state_mut();
        state.network_game = false;
        state.play_mode = setup.effective_play_mode();
        state.victory_type = setup.victory_type;
        state.victory_limit = setup.victory_limit;
        state.ai.settings.opponent = setup.opponent;

        // Playing teams — from the save; the save's `player_team` — human.
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
        loaded.timer = saved_timer as u32;
        loaded.endturn = saved_endturn as u32;
        // A save with several human teams is played as hot-seat.
        loaded.hot_seat = loaded.is_hot_seat();

        self.state_mut().mission_category = if loaded.is_hot_seat() {
            MissionCategory::HotSeat
        } else {
            MissionCategory::Custom
        };
        // P3: if the screen settings overrode the mode/victory from the file, we bring
        // `SaveMeta` to the same form — otherwise a repeated save would "drift"
        // relative to the just loaded one.
        {
            let state = self.state_mut();
            let meta = &mut state.save_meta;
            meta.save_game_type = if state.mission_category == MissionCategory::HotSeat {
                max_core::enums::FileType::HotSeat
            } else {
                max_core::enums::FileType::Custom
            };
            meta.ini_options.play_mode = state.play_mode;
            meta.ini_options.victory_type = state.victory_type;
            meta.ini_options.victory_limit = state.victory_limit;
            meta.ini_options.opponent = max_core::enums::Opponent::try_from(setup.opponent)
                .unwrap_or(max_core::enums::Opponent::Average);
            meta.opponent = meta.ini_options.opponent;
        }

        let mut local = LocalGame::new(loaded);
        local.phase = LocalPhase::Advancing;
        self.local = Some(local);
        self.hud.clear();
        self.hud.push_log("Local game loaded from a V70 save");
        // `GAME_STATE_10_LOAD_GAME` resumes at the saved active team without
        // re-running its economy (`game_manager.cpp:458-464`).
        self.local_resume_round();

        Ok(())
    }

    /// Saves the local match into the bytes of the `MXLS` container.
    pub fn local_save_bytes(&self) -> Option<Vec<u8>> {
        LocalSave::capture(self).map(|save| save.encode())
    }

    /// Writes the local match to a file (`.mxs`).
    pub fn write_local_save(&self, path: impl AsRef<std::path::Path>) -> Result<usize, String> {
        let bytes = self
            .local_save_bytes()
            .ok_or_else(|| "no local game in progress".to_string())?;
        std::fs::write(path.as_ref(), &bytes)
            .map_err(|error| format!("write {}: {error}", path.as_ref().display()))?;
        Ok(bytes.len())
    }

    /// Reading and checking the local save container.
    pub fn read_local_save(path: impl AsRef<std::path::Path>) -> Result<LocalSave, String> {
        let bytes = std::fs::read(path.as_ref())
            .map_err(|error| format!("read {}: {error}", path.as_ref().display()))?;
        LocalSave::decode(&bytes)
    }
}
