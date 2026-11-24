//! `max_app` — the M.A.X. application as a library: `max_core` + `max_net` +
//! `max_assets` + `max_render` in one game loop.
//!
//! The crate separates the **testable logic** (this file and the [`assets`],
//! [`render`], [`net`] modules) from the **binary** (`src/main.rs`): SDL3 lives only in
//! `main.rs` under the `sdl` feature, so `cargo test -p max_app` requires no
//! window, display or SDL.
//!
//! # What [`App`] can do
//!
//! | Method | Purpose |
//! |---|---|
//! | [`App::init_assets`] | reads `MAX.RES`, `PATCHES.RES`, `COLOR.PAL`, the planet map (`SNOW_1`…`DESERT_6`) and the [`assets::UNIT_SPRITES`] sprites |
//! | [`App::load_game`] / [`App::load_save`] | V70 save → [`max_core::game::GameState`] (`SaveLoad_LoadFormatV70` normalizations) |
//! | [`App::new_game`] / [`App::new_game_on_surface`] | `setup::generate_map` + settings + alien debris |
//! | [`App::tick`] | one simulation tick (`GameManager_ProcessTick`) |
//! | [`App::submit`] | the single lockstep entry: queues a `max_core::input::Command` (network packet or local input) |
//! | [`App::render_frame`] | a [`max_render::buffer::RgbaBuffer`] frame without SDL |
//! | [`hud::render_hud`] | HUD (M4.2): minimap, unit panel, log, resources, buttons |
//! | [`audio::AudioPlayer`] | sound (M6.2): WAV clips, `play_once`/`play_loop`; output — behind the `sound` feature |
//! | [`hud::build_move_order`] | packet 38 from the selection/click (`Paths_RequestPath`) |
//! | [`hud::build_attack_order`] | packet 08 (`P3`, `ORDER_MOVE_TO_ATTACK`) on an enemy |
//! | [`App::digest`] | CRC/state snapshot for the desync detector and tests |
//! | [`App::run_demo`] | headless scenario "load save → N ticks → digest" |
//!
//! # Determinism
//!
//! Everything that affects the simulation comes from `max_core`: the tick is
//! [`GameState::tick`], the command — [`max_core::events::apply_command`]. The
//! application itself stores no game state other than diagnostics
//! counters (`ticks`, `applied_commands`), so two runs of the same save
//! give the same [`AppDigest`].

// `deny`, not `forbid`: the only exception is reading the process RSS for
// P4 soak runs (`mem.rs`, Windows: `GetProcessMemoryInfo`). It is marked
// `#[allow(unsafe_code)]` locally; the rest of the crate is still unsafe-free.
#![deny(unsafe_code)]

pub mod allocmenu;
pub mod assets;
pub mod audio;
pub mod buildmenu;
pub mod buildsite;
pub mod cargo_unit_data;
pub mod chrome;
pub mod credits;
pub mod cursor;
pub mod dialog;
pub mod helpmenu;
pub mod host;
pub mod hotseat;
pub mod hud;
pub mod intro;
pub mod landing;
pub mod local;
pub mod mem;
pub mod menu;
pub mod mobilize;
pub mod net;
pub mod overlays;
pub mod panels;
pub mod portrait;
pub mod preferences;
pub mod pregame;
pub mod profile;
pub mod qa;
pub mod render;
pub mod researchmenu;
pub mod saveload;
pub mod scroll;
pub mod selfdestruct;
pub mod setup;
pub mod sfx;
pub mod soak;
pub mod transfermenu;
pub mod ui;
pub mod unit_names;
pub mod unitmenu;
pub mod upgrademenu;
pub mod winloss;

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use max_core::crc;
use max_core::deploy::TeamDeployment;
use max_core::enums::{GameStateType, Planet, TeamClan, UnitOrder, UnitOrderState};
use max_core::events::ApplyOutcome;
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::geo::Point;
use max_core::input::Command;
use max_core::rng::Randomizer;
use max_core::save::SaveFile;
use max_core::setup::{
    generate_map, AlienUnitCosts, GameSetup, GenerationSettings, ResourceLevel, TeamStart,
};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::unit_values::{base_unit_values, ATTRIB_TURNS};
use max_core::universe::{
    Dispatcher, GameEvent, GameUniverse, LocalDispatcher, StateRef, SubmitOutcome,
};
use max_core::{SurfaceMap, UnitType};
use max_render::viewport::Viewport;

use crate::net::NetworkDispatcher;

pub use allocmenu::{AllocAction, AllocMenu, AllocMenuAssets};
pub use assets::{
    data_search_roots, resolve_color_pal, resolve_max_res, resolve_patches_res, Assets,
    AssetsReport, LoadedSprite, SpriteInfo, COLOR_PAL_NAME, DATA_DIR_NAME, MAX_RES_NAME,
    PATCHES_RES_FALLBACKS, PLANET_GROUP_PICTURES, UNIT_SPRITES, UNIT_TYPES_WITHOUT_SHADOW_DATA,
    UNIT_TYPES_WITHOUT_SPRITE_DATA, WORLD_ASSETS,
};
pub use audio::{AudioBackend, AudioPlayer, NoopBackend, PlayMode, MENU_CLICK};
pub use buildmenu::{BuildMenu, BuildMenuAction, BuildMenuAssets};
pub use cursor::{cursor_for_context, CursorContext, CursorHover, CursorId, CursorImage};
pub use hotseat::TurnNotice;
pub use hud::{
    build_attack_order, build_move_order, build_power_order, hud_layout, render_hud, team_color,
    unit_at_screen, HudButton, HudLayout, HudState, HudView, MessageLog, MoveOrder, PathPreview,
};
pub use landing::{LandingAction, LandingScreen};
pub use local::{LocalGame, LocalOutcome, LocalPhase, LocalSetup, LocalUpdate, LOCAL_TEAM_COUNT};
pub use mem::{memory_line, process_memory, ProcessMemory};
pub use mobilize::Mobilization;
pub use net::{
    connect_horizon, run_network, run_network_with_sink, DirectEndpoint, FrameSink, LobbyInfo,
    NetworkOptions, NetworkReport, PacketHistogram, SharedDigest,
};
pub use panels::{PanelButton, PanelState, Panels};
pub use preferences::{
    PrefKey, Preferences, PreferencesAction, PreferencesAssets, PreferencesDialog, PreferencesInfo,
    PreferencesVariant,
};
pub use pregame::{PreGameFlow, PreGameStage, PreGameStep};
pub use profile::{format_ns, Meter, MeterStats, ProfileSnapshot, FRAME_BUDGET_NS};
pub use render::{
    draw_unit_sprites, draw_unit_sprites_with_angle, draw_unit_sprites_with_base, unit_frame_index,
    unit_frame_index_for_angle, write_ppm, RenderStats, UnitSpriteStats,
};
pub use saveload::{
    save_meta_for_local_game, save_meta_for_local_game_with, SaveLoadAction, SaveLoadAssets,
    SaveLoadMenu, SaveLoadMode, AUTOSAVE_SLOT, LAST_SLOT, SLOTS_PER_PAGE,
};
pub use soak::{run_soak, SoakOptions, SoakReport, SoakTurn};
pub use transfermenu::{TransferAction, TransferMenu, TransferMenuAssets};
pub use ui::{
    handle_session_input, planet_cell_at, planet_cell_rect, planet_grid_rect, planet_name,
    CreateField, CreateForm, HorizonMenuClient, InputEvent, Key, MenuClient, Screen, ScreenView,
    SessionRequest, Transition, UiContext, DEFAULT_GAME_NAME, MAX_WORLD, PLANET_COLUMNS,
    PLANET_COUNT, PLANET_GRID_MIN_HEIGHT, PLANET_NAMES, PLANET_ROWS,
};
pub use unitmenu::{
    MenuEvent, UnitMenu, UnitMenuAssets, UnitMenuItem, CURSOR_LOAD, CURSOR_RELOAD, CURSOR_REPAIR,
    CURSOR_TRANSFER,
};
pub use upgrademenu::{UpgradeAction, UpgradeFilter, UpgradeMenu, UpgradeMenuAssets};

/// Application mode (affects only the CLI and diagnostics, not the simulation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// `--demo`: load a save, run N ticks, print the digest.
    Demo,
    /// `--new-game`: a local match from `setup::generate_map`.
    NewGame,
    /// `--connect`: Horizon + ENet session.
    Connect,
    /// `--host`: ENet server with the server-side lobby (P2P host mode).
    Host,
    /// `--ui`: interactive menu (create/select a game) and session.
    Ui,
}

impl fmt::Display for AppMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Demo => "demo",
            Self::NewGame => "new-game",
            Self::Connect => "connect",
            Self::Host => "host",
            Self::Ui => "ui",
        })
    }
}

/// Application configuration: game data paths, network, player name, mode.
///
/// Default paths — from the repository root (`max-original/MAX.RES`, etc.);
/// the CLI overrides them with flags, and tests — [`AppConfig::with_save`] and
/// [`App::new_game_on_surface`] (without game files).
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// `MAX.RES` (maps `SNOW_*`…`DESERT_*`, sprites, data).
    pub max_res: PathBuf,
    /// `PATCHES.RES` (mission rules, `SC_*`).
    pub patches_res: PathBuf,
    /// `PATCHES.RES` from which **only** the handshake `data_hash` is computed.
    ///
    /// `None` — the same file as [`Self::patches_res`] (old behavior).
    /// A separate path is needed when the server `game_data` does not contain
    /// `PATCHES.RES` (its hash part is empty), while the client must keep
    /// full assets: then `--patches` points to the real archive,
    /// and `--handshake-patches` — to a nonexistent file, repeating the server
    /// semantics (`max-brh-docs/protocol.md` §7).
    pub handshake_patches: Option<PathBuf>,
    /// `COLOR.PAL` (256 × RGB).
    pub color_pal: PathBuf,
    /// `.WRL` map directory; `None` — next to [`Self::max_res`].
    pub world_dir: Option<PathBuf>,
    /// Save for demo mode (usually `SAVE1.CAM`).
    pub save: Option<PathBuf>,
    /// Horizon gRPC address (`--connect`).
    pub horizon_host: String,
    /// Horizon gRPC port (`max-brh-docs/protocol-grpc.md` §4.4: default 8082).
    pub horizon_port: u16,
    /// Player name in the lobby (packet 36).
    pub player_name: String,
    /// Preferred team slot 0..=3 (packet 35).
    pub team_slot: u8,
    /// Team clan (packet 26).
    pub clan: TeamClan,
    /// Planet for [`App::init_assets`] and [`App::new_game`].
    pub planet: Planet,
    /// Frame width (render window / SDL).
    pub screen_width: u32,
    /// Frame height.
    pub screen_height: u32,
    /// Application mode.
    pub mode: AppMode,
    /// How many ticks to run in demo mode.
    pub demo_turns: u32,
    /// V70 save directory (`save1.cam`, autosave `save10.*`). `None` — next to
    /// `MAX.RES` (like `FilePathGameData`/`FilePathGamePref` in C++).
    pub save_dir: Option<PathBuf>,
    /// Autosave at the end of the turn (`SETUP.auto_save`, enabled by default).
    pub autosave: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_res: PathBuf::from("max-original/MAX.RES"),
            patches_res: PathBuf::from("assets/PATCHES.RES"),
            handshake_patches: None,
            color_pal: PathBuf::from("max-original/COLOR.PAL"),
            world_dir: None,
            save: None,
            horizon_host: "127.0.0.1".to_string(),
            horizon_port: 8082,
            player_name: "max_app".to_string(),
            team_slot: 0,
            clan: TeamClan::Random,
            planet: Planet::Snowcrab,
            screen_width: max_render::layout::WINDOW_WIDTH as u32,
            screen_height: max_render::layout::WINDOW_HEIGHT as u32,
            mode: AppMode::Demo,
            demo_turns: 5,
            save_dir: None,
            autosave: true,
        }
    }
}

impl AppConfig {
    /// Configuration with a save.
    pub fn with_save(mut self, save: impl Into<PathBuf>) -> Self {
        self.save = Some(save.into());
        self
    }

    /// Save `dir/SAVE1.CAM` (convenient for `--data-dir`).
    pub fn with_save_in(mut self, dir: impl AsRef<Path>) -> Self {
        self.save = Some(dir.as_ref().join("SAVE1.CAM"));
        self
    }

    /// Game data directory (`MAX.RES`, `COLOR.PAL`) — `max-original/`.
    pub fn with_data_dir(mut self, dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        self.max_res = dir.join("MAX.RES");
        self.color_pal = dir.join("COLOR.PAL");
        self
    }

    /// `.WRL` map directory (if it does not coincide with the `MAX.RES` directory).
    pub fn with_world_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.world_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// `assets/` directory with `PATCHES.RES`.
    pub fn with_assets_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.patches_res = dir.as_ref().join("PATCHES.RES");
        self
    }

    /// A separate `PATCHES.RES` **only** for the handshake `data_hash`.
    ///
    /// Assets continue to load from [`Self::patches_res`] (or its
    /// fallback), so the path may not exist — like a server whose
    /// `game_data` does not contain `PATCHES.RES` (`max-brh-docs/protocol.md` §7).
    pub fn with_handshake_patches(mut self, path: impl Into<PathBuf>) -> Self {
        self.handshake_patches = Some(path.into());
        self
    }

    /// `PATCHES.RES` path for `data_hash` (`max-brh-docs/protocol.md` §7).
    ///
    /// `handshake_patches == None` → the assets path [`Self::patches_res`]
    /// (behavior before the paths were separated).
    pub fn handshake_patches_path(&self) -> &Path {
        self.handshake_patches
            .as_deref()
            .unwrap_or(&self.patches_res)
    }

    /// Frame size.
    pub fn with_screen(mut self, width: u32, height: u32) -> Self {
        self.screen_width = width;
        self.screen_height = height;
        self
    }

    /// Horizon gRPC address/port.
    pub fn with_horizon(mut self, host: impl Into<String>, port: u16) -> Self {
        self.horizon_host = host.into();
        self.horizon_port = port;
        self
    }

    /// Player name, slot and clan.
    pub fn with_player(mut self, name: impl Into<String>, team_slot: u8, clan: TeamClan) -> Self {
        self.player_name = name.into();
        self.team_slot = team_slot;
        self.clan = clan;
        self
    }

    /// Planet (`settings["world"]`).
    pub fn with_planet(mut self, planet: Planet) -> Self {
        self.planet = planet;
        self
    }

    /// Mode.
    pub fn with_mode(mut self, mode: AppMode) -> Self {
        self.mode = mode;
        self
    }

    /// Number of demo mode ticks.
    pub fn with_demo_turns(mut self, turns: u32) -> Self {
        self.demo_turns = turns;
        self
    }

    /// V70 save directory (save/load, autosave).
    pub fn with_save_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.save_dir = Some(dir.into());
        self
    }

    /// Enables/disables autosave (`SETUP.auto_save`).
    pub fn with_autosave(mut self, enabled: bool) -> Self {
        self.autosave = enabled;
        self
    }
}

/// New local match parameters.
///
/// [`Self::p2_reference`] — exactly the input of the P2 reference vector
/// `p00_l111_d0_s0` (`max-brh/tools/map_vectors/vectors.txt`): planet 0,
/// levels 1/1/1/0, seed 0, starts `(RED, 20, 20)` and `(BLUE, 70, 70)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGameConfig {
    /// Planet.
    pub planet: Planet,
    /// RNG seed (`Remote_RngSeed`).
    pub seed: u32,
    /// Raw material level (`raw_resource` 0–2).
    pub raw: ResourceLevel,
    /// Fuel level.
    pub fuel: ResourceLevel,
    /// Gold level.
    pub gold: ResourceLevel,
    /// Alien debris level.
    pub derelicts: ResourceLevel,
    /// Starting positions of the teams (mutated by `FindValidStartingPosition`).
    pub teams: Vec<TeamStart>,
    /// Clan of each of the four game teams (`menu_team_clan_setting`).
    /// `TeamClan::Random` — C++ will pick a clan at random.
    pub clans: [TeamClan; PLAYER_TEAM_COUNT],
    /// Which team is the local player.
    pub player_team: TeamId,
    /// `start_gold` — team gold at start (`settings.cpp:118`, 150).
    ///
    /// `team_gold` after landing is computed from it too: in a custom game
    /// the starting `SC_L0001` loadout is free, there are no upgrades.
    pub start_gold: u32,
    /// Teams ready for landing after the start phases (`CargoMenu` + site select).
    ///
    /// Empty — the old path: everyone gets [`Self::start_gold`] and
    /// [`max_core::deploy::DEFAULT_MISSION_LOADOUT`] per [`Self::teams`].
    pub deployments: Vec<TeamDeployment>,
}

impl Default for NewGameConfig {
    fn default() -> Self {
        Self::p2_reference()
    }
}

impl NewGameConfig {
    /// Input of the P2 reference vector `p00_l111_d0_s0`.
    pub fn p2_reference() -> Self {
        Self {
            planet: Planet::Snowcrab,
            seed: 0,
            raw: ResourceLevel::NORMAL,
            fuel: ResourceLevel::NORMAL,
            gold: ResourceLevel::NORMAL,
            derelicts: ResourceLevel::LOW,
            teams: vec![
                TeamStart::new(TeamId::RED, Point::new(20, 20)),
                TeamStart::new(TeamId::BLUE, Point::new(70, 70)),
            ],
            clans: [TeamClan::Random; PLAYER_TEAM_COUNT],
            player_team: TeamId::RED,
            start_gold: max_core::deploy::DEFAULT_START_GOLD,
            deployments: Vec::new(),
        }
    }
}

/// Result of [`App::new_game`] / [`App::new_game_on_surface`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewGameReport {
    /// Planet.
    pub planet: Planet,
    /// RNG seed.
    pub seed: u32,
    /// How many teams participate.
    pub teams: usize,
    /// How many alien debris `setup::generate_map` returned (the
    /// `UnitsManager_DeployUnit` order; all of them are deployed into `GameState` under
    /// `TeamId::ALIEN`).
    pub aliens: usize,
    /// How many starting units `deploy::deploy_starting_units` landed
    /// (`GameManager_ProcessTeamMissionSupplyUnits` for each team).
    pub start_units: usize,
    /// How many units are in the state after generation (`aliens` + `start_units`).
    pub units: usize,
    /// CRC16 of the map surface.
    pub surface_crc: u16,
    /// CRC16 of the resource map.
    pub resources_crc: u16,
}

/// Result of one [`App::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// Application tick number (1-based).
    pub tick: u64,
    /// `GameManager_TurnCounter`.
    pub turn_counter: i32,
    /// `GameManager_GameState`.
    pub game_state: GameStateType,
    /// Number of units after the tick.
    pub units: usize,
}

/// Canonical state digest — the application desync detector.
///
/// Three independent `max_core::crc` values:
///
/// * `state_digest` — CRC16 of the full [`GameState::snapshot_bytes`] (all
///   simulation fields);
/// * `state_checksum` — CRC16 of packet 45 (`GameManager_CheckDesync`), the same
///   one the server compares in a lockstep session;
/// * `replay_crc` — CRC16 of the replay digest (`crc::replay_digest_bytes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppDigest {
    /// How many ticks the application ran.
    pub ticks: u64,
    /// Turn number.
    pub turn_counter: i32,
    /// Match phase.
    pub game_state: GameStateType,
    /// Number of units.
    pub units: usize,
    /// CRC16 of the state snapshot.
    pub state_digest: u16,
    /// CRC16 of packet 45.
    pub state_checksum: u16,
    /// CRC16 of the replay digest.
    pub replay_crc: u16,
    /// Snapshot length in bytes.
    pub snapshot_len: usize,
}

impl AppDigest {
    /// Takes the digest of the state.
    pub fn capture(state: StateRef<'_>, ticks: u64) -> Self {
        let snapshot = state.snapshot_bytes();

        Self {
            ticks,
            turn_counter: state.turn_counter(),
            game_state: state.game_state(),
            units: state.unit_count(),
            state_digest: crc::crc16(&snapshot),
            state_checksum: state.state_checksum(),
            replay_crc: state.replay_digest_crc(),
            snapshot_len: snapshot.len(),
        }
    }

    /// One-line representation for CLI logs.
    pub fn to_line(self) -> String {
        format!(
            "ticks={} turn={} state={:?} units={} state_digest=0x{:04X} state_checksum=0x{:04X} replay_crc=0x{:04X} snapshot={}B",
            self.ticks,
            self.turn_counter,
            self.game_state,
            self.units,
            self.state_digest,
            self.state_checksum,
            self.replay_crc,
            self.snapshot_len
        )
    }
}

impl fmt::Display for AppDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_line())
    }
}

/// Application: match state + assets + camera + counters.
///
/// `D` is the transport policy of the facade ([`Dispatcher`]); the default
/// `LocalDispatcher` keeps every existing call-site — `App` means
/// `App<LocalDispatcher>` unless a network driver names the parameter explicitly
/// (`game-universe-stage-c.md` §2). Switching the mode means creating another
/// `App<D>`, not mutating this one (ADR §7.8).
///
/// Created without game files ([`App::new`]); assets are loaded explicitly
/// ([`App::init_assets`]), and the state — from a save ([`App::load_game`]) or by
/// generation ([`App::new_game`]). Everything else (tick, commands, frame,
/// digest) works without assets too: the map is rendered in a simplified way, units —
/// with team markers.
#[derive(Debug, Clone)]
pub struct App<D: Dispatcher = LocalDispatcher> {
    config: AppConfig,
    /// The owning facade over the match state (stage A).
    ///
    /// The `GameState` is not a field of `App`: the facade owns it and is the only
    /// mutable entry to the world. Stage C §5 removed the accessor bridge of
    /// stage B, so the facade is reached as a field: the driver uses it in-crate
    /// (`self.universe…`), while integration tests use its documented config/test
    /// layer. The dispatcher type is the `App` parameter: the universe and the
    /// application share the same transport policy.
    pub universe: GameUniverse<D>,
    /// The application event cursor (stage A: the combat sounds of a tick are read
    /// from the facade event stream, see [`App::tick`]).
    universe_events: max_core::universe::EventStream,
    /// The turn-machine event cursor (stage A: `local.rs` consumes
    /// `GameEvent::Turn` notices instead of calling `next_team` itself).
    ///
    /// A dedicated cursor is an independent position in the same buffer (owner
    /// decision 6): consuming turn notices cannot steal the combat sounds from
    /// [`App::universe_events`].
    turn_events: max_core::universe::EventStream,
    assets: Option<Assets>,
    viewport: Viewport,
    ticks: u64,
    applied_commands: u64,
    /// Session UI state (M4.2): the selected unit, log, route preview
    /// and orders from input. **Not part of** the snapshot/CRC — selection
    /// must not change the simulation determinism ([`AppDigest`] does not see it).
    hud: hud::HudState,
    /// M-SP local match state (`None` — network/demo/menu).
    pub(crate) local: Option<local::LocalGame>,
    /// Open `BuildMenu` (P2). Not part of the snapshot/CRC — this is UI.
    build_menu: Option<buildmenu::BuildMenu>,
    /// Build site selection mode (`ORDER_STATE_SELECT_SITE`, C++
    /// `GameManager_TempTape` + `GameManager_GetBuilderUnitCursor`). Not part of
    /// the snapshot/CRC — the order is not issued before confirmation.
    pub(crate) build_site: Option<buildsite::BuildSiteSelect>,
    /// Open V70 save/load window (P3). Not part of the snapshot/CRC.
    pub(crate) save_load: Option<saveload::SaveLoadMenu>,
    /// Open purchasable upgrades screen (P6). Not part of the snapshot/CRC.
    upgrade_menu: Option<upgrademenu::UpgradeMenu>,
    /// Open research screen (P7, `ResearchMenu`). Not part of the snapshot/CRC.
    research_menu: Option<researchmenu::ResearchMenu>,
    /// Open selected unit command menu (C++ `GameManager_PopupButtons`).
    /// Not part of the snapshot/CRC — this is UI.
    unit_menu: Option<unitmenu::UnitMenu>,
    /// Open building self-destruct dialog (`UnitsManager_SelfDestructMenu`,
    /// windows `SELFDSTR`/`SLFDOPN`/`SLFDOK`). Not part of the snapshot/CRC.
    self_destruct: Option<selfdestruct::SelfDestructMenu>,
    /// UI sound queue for the current frame ([`sfx::Sfx`]): played by `main.rs`
    /// (`AudioPlayer`), tests check events against it. Not part of the snapshot/CRC.
    sounds: Vec<sfx::Sfx>,
    /// Combat sounds for the frame ([`sfx::CombatSfx`]): a unit's shot/hit/explosion
    /// (`max_core::UnitSound` → `SC_UNITS`). Played by `main.rs`; not part of
    /// the snapshot/CRC.
    unit_sounds: Vec<sfx::CombatSfx>,
    /// Open pause dialog (`PauseMenu_Menu` → `DialogMenu`, `Alt+P`).
    pause_dialog: Option<dialog::DialogMenu>,
    /// What the pause was before the dialog was opened (to restore after `OK`).
    pause_was_paused: bool,
    /// Open mine extraction allocation dialog (C++ `AllocMenu`, window
    /// `ALLOCFRM`). Not part of the snapshot/CRC — this is UI; the values live in the unit
    /// fields (`raw_mining`/`fuel_mining`/`gold_mining`).
    alloc_menu: Option<allocmenu::AllocMenu>,
    /// Open X-FER window (C++ `TransferMenu`, `XFERPIC`). Not part of the snapshot/CRC:
    /// the amount goes as order **08/P3** (`transfer_cargo`).
    transfer_menu: Option<transfermenu::TransferMenu>,
    /// Open help window over the game (P7, `HelpMenu`). Not part of the snapshot/CRC.
    help: Option<helpmenu::HelpScreen>,
    /// C++ help mode (`HelpMenu_Menu`, `helpmenu.cpp:315`): cursor
    /// `CURSOR_HELP`, the window opens on a click on an element. Not part of the snapshot/CRC.
    help_mode: Option<helpmenu::HelpMode>,
    /// Visual unit positions in cells (fractional) — drawing only.
    ///
    /// The simulation still moves a unit a whole cell per tick; the frame follows
    /// the actual route cell by cell with real time
    /// ([`App::update_visual_units`]). These values are not part of the CRC/snapshot.
    visual_units: std::collections::HashMap<UnitId, (f32, f32)>,
    /// Last known simulation cell of a unit: from its changes
    /// the visual route is built. UI state, not part of the snapshot.
    visual_goals: std::collections::HashMap<UnitId, (i16, i16)>,
    /// Queue of route cells the frame still has to pass. The simulation
    /// executes the order in ticks (fast), while the frame passes the same cells at
    /// the real original speed (`UnitInfo::Move`: 8 px/tick ~ 3.75 cells/s).
    /// Each point stores the movement direction toward it (`VisualStep.angle`),
    /// so that the sprite turns at the turn cell, not earlier.
    visual_paths: std::collections::HashMap<UnitId, std::collections::VecDeque<VisualStep>>,
    /// Visual frame clock of units (`FrameAnimation`).
    ///
    /// Two mechanisms: the builder cycle (`ProcessOrderBuild`) and the animation of
    /// `ANIMATED` buildings (`ProcessOrderSentry`, e.g. RADAR). Factory and mine
    /// frames are **not advanced** by time (DOS capture §7.2): they have state
    /// frames, and the visible blinking comes from palette cycles
    /// [`App::color_cycles`]. They are not part of the snapshot/CRC: the simulation does not store
    /// a "frame number" — it stores only `moved`/`image_index`, which C++
    /// uses as a counter. `dt = 0` (QA/headless) — the static C++ frame,
    /// deterministically.
    frame_anims: std::collections::HashMap<UnitId, FrameAnim>,
    /// Palette cycle clock (`World::InitColorCycles` +
    /// `World::UpdateColorAnimations`): 11 DAC slot ranges that
    /// rotate at 2/6/8/9/10 fps. It is they that make the building beacons blink
    /// and the drill indicator — the sprite frames are not advanced
    /// (`max-original-dasm/formats/unit-animation.md` §7.3). They are not part of the
    /// snapshot/CRC; `dt = 0` (QA/headless) leaves the palette original.
    color_cycles: max_render::palette::ColorCycles,
    /// Snapshot of the system palette with rotated ranges (frame cache).
    ///
    /// `None` — the cycles have not stepped yet (or `dt = 0`): the unchanged
    /// `Assets::system_palette` is drawn.
    cycle_palette: Option<max_render::palette::Palette>,
    /// "Growth" of a just-placed unit (`UnitsManager_ScaleUnit`).
    ///
    /// `scaler_adjust` — a **simulation** field (`UnitInfo::RenderSprite`:
    /// scale `2^-scaler`); here — only the visual transition to the full
    /// size by real `dt`. `dt = 0` (QA/headless) — full size.
    growth_anims: std::collections::HashMap<UnitId, GrowthAnim>,
    /// FLC animation of the selected unit's portrait (`WINDOW_CORNER_FLIC`).
    ///
    /// UI state: not part of the snapshot/CRC. `None` — no unit selected, the type
    /// has no `F_*` resource or the FLC did not load (then the portrait — sprite ×2,
    /// see `panels::draw_portrait`).
    portrait: Option<portrait::PortraitAnim>,
    /// User settings (`SETUP`/`PREFERENCES`/`OPTIONS` from
    /// `settings.cpp`). Not part of the snapshot/CRC: only `effects` (frames)
    /// and sound affect the simulation.
    pub(crate) preferences: Preferences,
    /// Open `Preferences` dialog (panel button, C++ `OptionsMenu`).
    /// Not part of the snapshot/CRC — this is UI.
    pub(crate) preferences_dialog: Option<PreferencesDialog>,
    /// Frame number of the `BLDMRK1..5` construction markers blinking
    /// (`DrawMap_BuildMarkImageIndex`) and seconds until the next one.
    ///
    /// C++ changes the frame every three map redraws
    /// (`DrawMap_BuildMMarkDelayCounter`), the port — by the visual clock
    /// ([`BUILD_MARKER_FRAME_SECONDS`], 10 frames/s, like the builder cycle); at
    /// `dt = 0` (QA/headless) the frame is static and the goldens deterministic.
    pub(crate) build_marker_step: u8,
    pub(crate) build_marker_elapsed: f32,
    /// Selection indicator color — `GameManager_MarkerColor`
    /// (`game_manager.cpp:222`): `0xFF` (white) or `0x00` (black).
    /// Toggled every [`render::UNIT_SELECTOR_BLINK_SECONDS`] (210 ms,
    /// `game_manager.cpp:1189-1192`); at `dt = 0` (QA/headless) — `0xFF`.
    pub(crate) marker_color_index: u8,
    pub(crate) marker_color_elapsed: f32,
    /// Mouse and the "held" LMB for edge camera scrolling
    /// (`GameManager_ProcessInput`/`GameManager_SetGridOffset`). Not part of the
    /// snapshot/CRC — pure input.
    pub(crate) scroll: scroll::ScrollState,
}

/// Visual frame clock of one unit.
///
/// Each counter lives independently: the builder advances frames at 10 Hz
/// ([`BUILDER_ANIM_FRAME_SECONDS`]), an `ANIMATED` building — on every C++ tick
/// ([`SENTRY_ANIM_FRAME_SECONDS`]). There are no factory frames: C++ does not
/// advance them by time, the palette provides the blinking. All of them — for the frame only and
/// not part of the snapshot/CRC.
#[derive(Debug, Clone, Copy, Default)]
struct FrameAnim {
    /// Seconds since the last builder frame shift.
    builder_elapsed: f32,
    /// `0` — `+16` (construction start), `1` — `+24`, `2` — `+32` (C++ `+8`).
    builder_step: u8,
    /// Seconds since the last `ProcessOrderSentry` tick.
    sentry_elapsed: f32,
    /// `ProcessOrderSentry` tick number (ANIMATED building).
    sentry_step: u32,
}

/// Visual "growth" transition from `scaler_adjust` to full size.
#[derive(Debug, Clone, Copy)]
struct GrowthAnim {
    /// Scale at the first frame (`render::scaler_scale`), 1/16..1/2.
    start: f32,
    /// Animation seconds; after [`GROWTH_ANIM_SECONDS`] — full size.
    elapsed: f32,
}

/// Seconds per frame of the builder working cycle.
///
/// C++ `UnitsManager_ProcessOrderBuild` advances the frame every second tick
/// (`moved & 1`), and the original tick is ~30 Hz; here the frames go by real
/// interface time, 10 frames/s — the `+16 → +24 → +32` cycle in 0.3 s.
pub const BUILDER_ANIM_FRAME_SECONDS: f32 = 0.1;

/// Number of frames in the construction marker cycle — `BLDMRK1..5`
/// (`DrawMap_BuildMarkImages`, `drawmap.cpp:49`).
pub const BUILD_MARKER_FRAMES: usize = 5;

/// Seconds per construction marker blinking frame.
///
/// C++ changes the frame every three map redraws (`DrawMap_BuildMMarkDelayCounter`
/// = 3), and the map is drawn ~30 times per second → ~10 frames/s. The port takes the same
/// frequency through the visual clock, because its frame rate is not 30 Hz.
pub const BUILD_MARKER_FRAME_SECONDS: f32 = 0.1;

/// Seconds per `ProcessOrderSentry` frame (`ANIMATED` building, RADAR).
///
/// C++ advances the frame on every order processing tick (`UnitsManager_ProcessTick`
/// → `UnitsManager_ProcessOrders` → `UnitsManager_ProcessOrderSentry`), and the tick —
/// ~30 Hz (`TickTimer`, `timer_get`). In the interactive mode this clock follows real
/// `dt`: 1/30 s per frame, 16 RADAR frames — a revolution in ~0.53 s.
pub const SENTRY_ANIM_FRAME_SECONDS: f32 = 1.0 / 30.0;

/// Speed and enablement of the mine drill frames.
///
/// The DOS capture (`max-original-dasm/formats/unit-animation.md` §7.2) showed
/// that when idle the mine body is static; the drill frames `1..15` are enabled only
/// after a separate capture of active extraction. Factories have no time-based frames
/// at all: the `image_base` ↔ `image_base + 1` change follows the order
/// (`UnitInfo::StartBuilding`/`SpawnNewUnit`), and the blinking comes from the palette
/// cycles [`render::MINING_DRILL_FRAMES_ENABLED`] / [`App::color_cycles`].
pub use crate::render::MINING_DRILL_FRAMES_ENABLED;

/// Duration of the visual unit "growth", seconds.
///
/// C++ `UnitsManager_ProcessOrderAwaitScaling` reduces `scaler_adjust` by 1
/// per order processing tick (5 steps: 1/16 → 1). In the interactive mode the steps
/// are smoothed by real time; `dt = 0` (QA/headless) immediately gives
/// full size, so that the frames remain deterministic.
pub const GROWTH_ANIM_SECONDS: f32 = 1.0;

/// Speed of the visible unit movement on the map — cells per second.
///
/// Original: `UnitInfo::Move` (`unitinfo.cpp:1730`) moves the unit by `velocity`
/// pixels per tick, and `Paths_CalculateStep` (`paths.cpp:292`) limits
/// `max_velocity` to eight pixels; the tick is about 30 Hz (`TickTimer`,
/// `timer_get`). One cell — `GFX_MAP_TILE_SIZE = 64` px: 8 × 30 / 64 ≈
/// **3.75 cells/s**. We take 4 — this way the frame passes the route in the same time
/// as the pixel movement in C++.
pub const VISUAL_CELLS_PER_SECOND: f32 = 4.0;

/// A jump larger than this number of cells is not a route but a teleport/state
/// synchronization: the visual position is set to the target without animation.
pub const VISUAL_TELEPORT_CELLS: i32 = 6;

/// One step of a unit visual route: the point in cells and the movement direction
/// to it (`Paths_GetAngle`, 0..7).
///
/// The angle is stored **per segment**, not read from `UnitInfo::angle` at
/// the frame moment. The simulation executes the route fast (a cell per tick) and has already
/// turned toward the next cell — or passed several cells ahead — while
/// the sprite still walks this segment at real speed. Without the angle
/// snapshot the frame would show a turn 45–90° before the sprite reached the turn
/// cell: the unit would "look" sideways while moving straight.
#[derive(Debug, Clone, Copy, PartialEq)]
struct VisualStep {
    /// Cell coordinate (fractional — at intermediate jump points).
    x: f32,
    /// Cell coordinate (fractional).
    y: f32,
    /// Movement direction toward this point: `Paths_GetAngle(dx, dy)`.
    angle: u8,
}

/// Selected unit order at the tick start — for auto-selection
/// (`Preferences::auto_select`) after the order completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderSnapshot {
    /// Unit id at the snapshot moment.
    pub id: UnitId,
    /// Order (`UnitInfo::orders`).
    pub orders: UnitOrder,
    /// Execution state (`UnitInfo::state`).
    pub state: UnitOrderState,
    /// Whether there was a route (`UnitInfo::path`).
    pub has_path: bool,
    /// Movement points at the tick start (`UnitInfo::speed`).
    pub speed: u16,
}

// Dispatcher-agnostic surface (`game-universe-stage-c.md` §2): simulation, the
// HUD/menu/UI layers and rendering. The temporary network bridge of §2 is removed
// in §5 — the driver reaches the facade through the `universe` field.
impl<D: Dispatcher> App<D> {
    /// Configuration.
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Editable configuration (e.g. before `init_assets`).
    pub fn config_mut(&mut self) -> &mut AppConfig {
        &mut self.config
    }

    /// Assembles the application around a world built by `make_universe` (stage C §4.2).
    ///
    /// The single constructor body of both surfaces: [`App::new`] passes an empty
    /// `LocalDispatcher` world, [`App::new_network`] a `NetworkDispatcher` one.
    /// Everything around the universe is dispatcher-agnostic. The world arrives from
    /// a closure so that the large `GameUniverse` is built once, in place, instead
    /// of being copied through a by-value argument (the state is ~75 KB and debug
    /// builds memcpy it on every move).
    fn with_universe(config: AppConfig, make_universe: impl FnOnce() -> GameUniverse<D>) -> Self {
        let viewport = Viewport::new(config.screen_width, config.screen_height);
        let universe = make_universe();
        let universe_events = universe.subscribe();
        let turn_events = universe.subscribe();

        Self {
            config,
            universe,
            universe_events,
            turn_events,
            assets: None,
            viewport,
            ticks: 0,
            applied_commands: 0,
            hud: hud::HudState::new(),
            local: None,
            build_menu: None,
            build_site: None,
            save_load: None,
            upgrade_menu: None,
            research_menu: None,
            unit_menu: None,
            self_destruct: None,
            sounds: Vec::new(),
            unit_sounds: Vec::new(),
            pause_dialog: None,
            pause_was_paused: false,
            alloc_menu: None,
            transfer_menu: None,
            help: None,
            help_mode: None,
            visual_units: std::collections::HashMap::new(),
            visual_goals: std::collections::HashMap::new(),
            visual_paths: std::collections::HashMap::new(),
            frame_anims: std::collections::HashMap::new(),
            color_cycles: max_render::palette::ColorCycles::new(),
            cycle_palette: None,
            growth_anims: std::collections::HashMap::new(),
            portrait: None,
            preferences: Preferences::default(),
            preferences_dialog: None,
            build_marker_step: 0,
            build_marker_elapsed: 0.0,
            marker_color_index: 0xFF,
            marker_color_elapsed: 0.0,
            scroll: scroll::ScrollState::new(),
        }
    }

    /// Resets the per-match UI/frame state after a new world is installed (stage C A3).
    ///
    /// Shared by the local generator (`new_game_on_surface`) and the network
    /// `initialize_game`: around the world the application state is the same, only
    /// the dispatcher differs. The caller assigns `self.universe` first — the field
    /// assignment is the only move of the large state.
    fn reset_after_world_install(&mut self) {
        self.universe_events = self.universe.subscribe();
        self.turn_events = self.universe.subscribe();
        self.ticks = 0;
        self.applied_commands = 0;
        self.hud.clear();
        self.local = None;
        self.build_menu = None;
        self.save_load = None;
        self.upgrade_menu = None;
        self.research_menu = None;
        self.unit_menu = None;
        self.self_destruct = None;
        self.alloc_menu = None;
        self.transfer_menu = None;
        self.help = None;
        self.help_mode = None;
        self.reset_visual_state();
        self.focus_player();
        // Starting snapshot of the visual positions (see `load_game`).
        self.update_visual_units(0.0);
    }

    /// Loads the game data for `planet` and returns its surface map.
    ///
    /// The `.WRL` map is part of the game data, not of the match state: a match on
    /// another planet re-reads [`AppConfig`] (the former body of `new_game`).
    fn ensure_surface(&mut self, planet: Planet) -> Result<SurfaceMap> {
        self.config.planet = planet;

        if self.assets.as_ref().map(|assets| assets.planet) != Some(planet) {
            self.init_assets()?;
        }

        Ok(self
            .assets
            .as_ref()
            .expect("assets loaded above")
            .world
            .surface_map())
    }

    /// Carries the user settings of the lobby application into the network one
    /// (stage C §4.2).
    ///
    /// The interactive client may already have loaded `settings.json` (`--ui`) on
    /// the `App<LocalDispatcher>` that ran the lobby; the match must keep them —
    /// `enemy_halt` is simulation state, not only a UI preference.
    pub(crate) fn adopt_user_settings(&mut self, other: &App<LocalDispatcher>) {
        self.preferences = other.preferences.clone();
    }

    /// The facade view of the match state (pull model, stage A).
    ///
    /// This is the renderer-facing read path: a borrowed snapshot without a clone.
    /// The name is `state_ref`, not `snapshot`, because [`App::snapshot`] already
    /// returns the canonical state bytes and the stage-A task keeps it as is.
    pub fn state_ref(&self) -> StateRef<'_> {
        self.universe.snapshot()
    }

    /// Takes the outgoing transport DTOs (`drain_outgoing`, stage B §6b).
    ///
    /// `submit(Broadcast)` accumulates `NetworkCommand::Command` in the facade; the
    /// network driver is the only owner of the transport, so it drains them after
    /// [`App::tick`] and broadcasts them through the session.
    pub fn drain_outgoing(&mut self) -> Vec<max_core::universe::NetworkCommand> {
        self.universe.drain_outgoing()
    }

    /// Submits a command through the facade (the single point of change).
    ///
    /// This is the lockstep entry: the facade routes it (`Local`/`Broadcast`/
    /// `Drop`), queues it and plans the outgoing DTOs. The immediate barrier/FSM
    /// packets do not come here (stage C §5; decisions A5/A6).
    pub fn submit(&mut self, command: Command) -> SubmitOutcome {
        self.universe.submit(command)
    }

    /// Applies a command immediately and counts it (the FSM/immediate channel).
    ///
    /// Stage C §5 removed the public `App::apply` bridge: the lockstep path is
    /// [`App::submit`]. The HUD order flush, the build menus and the network
    /// pacing packet (`01`) still need the effect before the next tick, so they
    /// call the facade FSM channel [`GameUniverse::apply_now`] here, and the
    /// diagnostic counter [`App::applied_commands`] keeps counting only the
    /// commands that the application applies itself.
    fn apply_immediate(&mut self, command: Command) -> ApplyOutcome {
        let outcome = self.universe.apply_now(command);

        if outcome == ApplyOutcome::Applied {
            self.applied_commands += 1;
        }

        outcome
    }

    /// Open `BuildMenu` (P2); `None` — the screen is closed.
    pub fn build_menu(&self) -> Option<&buildmenu::BuildMenu> {
        self.build_menu.as_ref()
    }

    /// Open `BuildMenu` for input (editable).
    pub fn build_menu_mut(&mut self) -> Option<&mut buildmenu::BuildMenu> {
        self.build_menu.as_mut()
    }

    /// Opens `BuildMenu` for the selected unit — the `Build` button in C++
    /// (`GameManager_MenuClickBuildButton` → `BuildMenu_Menu`).
    ///
    /// `false` if no unit is selected or it cannot build.
    pub fn open_build_menu(&mut self) -> bool {
        let Some(id) = self.hud.selected else {
            return false;
        };

        let Some(menu) = buildmenu::BuildMenu::new(self.state_ref(), id) else {
            return false;
        };

        if menu.team != self.state_ref().player_team() {
            return false;
        }

        self.build_menu = Some(menu);
        true
    }

    /// Closes `BuildMenu` (Esc/`Cancel`/after an order).
    pub fn close_build_menu(&mut self) {
        self.build_menu = None;
    }

    /// Open upgrades screen (P6); `None` — closed.
    pub fn upgrade_menu(&self) -> Option<&upgrademenu::UpgradeMenu> {
        self.upgrade_menu.as_ref()
    }

    /// Open upgrades screen for input.
    pub fn upgrade_menu_mut(&mut self) -> Option<&mut upgrademenu::UpgradeMenu> {
        self.upgrade_menu.as_mut()
    }

    /// Opens the upgrades screen — C++ `UpgradeMenu(team, complex).Run()`
    /// (the `buy upg` item of the command menu, `units_manager.cpp:956`).
    ///
    /// Called by the popup `buy upg` item ([`unitmenu::ItemId::BuyUpgrade`]) and
    /// the `U` key (shortcut) — for a selected unit of own team.
    pub fn open_upgrade_menu(&mut self) -> bool {
        let Some(team) = self.hud.selected.and_then(|id| self.state_ref().unit(id)) else {
            return false;
        };
        let team = team.team;
        if team != self.state_ref().player_team() {
            return false;
        }
        let Some(menu) = upgrademenu::UpgradeMenu::new(self.state_ref(), team) else {
            return false;
        };
        self.upgrade_menu = Some(menu);
        true
    }

    /// Closes the upgrades screen without committing.
    pub fn close_upgrade_menu(&mut self) {
        self.upgrade_menu = None;
    }

    /// Commits the changes (`CommitUpgradeChanges`) and closes the screen.
    ///
    /// In a local game the changes are already applied to `TeamUnits`; in the network each
    /// changed type goes as packet **10** (`Command::UpgradeUnitType`).
    /// Packet 09 (team gold) is absent from the Rust protocol — the limitation is
    /// documented in `upgrademenu` (module header).
    pub fn commit_upgrade_menu(&mut self, online: bool) -> Option<String> {
        let mut menu = self.upgrade_menu.take()?;
        let changed = menu.commit(&mut self.universe);

        if online {
            for (unit_type, values) in &changed {
                self.hud.pending.push(Command::UpgradeUnitType(
                    max_core::Packet10UpgradeUnitType {
                        team: menu.team,
                        unit_type: *unit_type,
                        values: *values,
                    },
                ));
            }
        }

        let gold = self
            .state_ref()
            .team_units()
            .get(menu.team.0 as usize)
            .map(|units| units.gold)
            .unwrap_or(0);

        Some(if changed.is_empty() {
            format!("Upgrades: no changes (gold {gold})")
        } else {
            format!(
                "Upgrades: {} unit type(s) changed (gold {gold})",
                changed.len()
            )
        })
    }

    /// Input in the open upgrades screen — like `AbstractUpgradeMenu::ProcessKey`.
    pub fn upgrade_menu_input(&mut self, event: InputEvent) -> Option<upgrademenu::UpgradeAction> {
        let Some(mut menu) = self.upgrade_menu.take() else {
            return None;
        };
        let action = match event {
            InputEvent::MouseClick(x, y) => menu.click(self.state_ref(), x, y),
            InputEvent::Key(key) => menu.key(self.state_ref(), key),
            _ => upgrademenu::UpgradeAction::None,
        };
        self.upgrade_menu = Some(menu);
        Some(action)
    }

    /// Selection of a type list row in the open upgrades screen (QA/keyboard).
    pub fn upgrade_menu_select(&mut self, index: usize) -> bool {
        let Some(mut menu) = self.upgrade_menu.take() else {
            return false;
        };
        if index >= menu.unit_types.len() {
            self.upgrade_menu = Some(menu);
            return false;
        }
        menu.selected = index;
        menu.page_min = menu.page_min.min(
            menu.unit_types
                .len()
                .saturating_sub(crate::upgrademenu::LIST_ROWS as usize),
        );
        menu.rebuild_controls(self.state_ref());
        self.upgrade_menu = Some(menu);
        true
    }

    /// Upgrade arrow of row `row` in the open screen (QA).
    pub fn upgrade_menu_adjust(&mut self, row: usize, increase: bool) -> bool {
        let Some(mut menu) = self.upgrade_menu.take() else {
            return false;
        };
        let changed = menu.adjust(self.state_ref(), row, increase);
        self.upgrade_menu = Some(menu);
        changed
    }

    // -----------------------------------------------------------------------
    // Research screen (`ResearchMenu`, P7)
    // -----------------------------------------------------------------------

    /// Open research screen; `None` — closed.
    pub fn research_menu(&self) -> Option<&researchmenu::ResearchMenu> {
        self.research_menu.as_ref()
    }

    /// Open research screen for input.
    pub fn research_menu_mut(&mut self) -> Option<&mut researchmenu::ResearchMenu> {
        self.research_menu.as_mut()
    }

    /// Opens the research screen for a selected powered-on
    /// `RESEARCH` center of own team — C++ popup `Research`
    /// (`UnitsManager_Popup_OnClick_Research`, the `resrch` command menu item);
    /// called by the popup item and the `R` key (shortcut).
    pub fn open_research_menu(&mut self) -> bool {
        let Some(id) = self.hud.selected else {
            return false;
        };
        let Some(unit) = self.state_ref().unit(id) else {
            return false;
        };
        if unit.team != self.state_ref().player_team()
            || unit.unit_type != UnitType::ResearchCentre
            || unit.orders != max_core::enums::UnitOrder::PowerOn
        {
            return false;
        }
        let Some(menu) = researchmenu::ResearchMenu::new(self.state_ref(), unit.team) else {
            return false;
        };

        self.research_menu = Some(menu);
        true
    }

    /// Closes the research screen without applying the allocation.
    pub fn close_research_menu(&mut self) {
        self.research_menu = None;
    }

    /// `ResearchMenu_OnClick_Done`: apply the center allocation to
    /// `GameState` (`set_research_allocation`) and close the screen.
    pub fn commit_research_menu(&mut self) -> Option<String> {
        let mut menu = self.research_menu.take()?;
        let actual = menu.commit(&mut self.universe);
        let total: u32 = actual.iter().sum();

        Some(format!("Research: {} center(s) assigned (Done)", total))
    }

    /// Input in the open research screen — like `ResearchMenu::Run`.
    pub fn research_menu_input(
        &mut self,
        event: InputEvent,
    ) -> Option<researchmenu::ResearchAction> {
        let Some(mut menu) = self.research_menu.take() else {
            return None;
        };
        let action = menu.input(event);
        self.research_menu = Some(menu);
        Some(action)
    }

    /// Topic selection (keyboard/QA).
    pub fn research_menu_select(&mut self, index: usize) -> bool {
        let Some(menu) = self.research_menu.as_mut() else {
            return false;
        };
        if index >= max_core::enums::RESEARCH_TOPIC_COUNT {
            return false;
        }
        menu.selected = index;
        true
    }

    /// Topic `row` arrow (QA): `+1`/`−1` center, like a click on `UPGRGT`/`UPGLFT`.
    pub fn research_menu_adjust(&mut self, row: usize, increase: bool) -> bool {
        let Some(menu) = self.research_menu.as_mut() else {
            return false;
        };
        menu.adjust(row, increase)
    }

    // -----------------------------------------------------------------------
    // Selected unit command menu (C++ `GameManager_PopupButtons`)
    // -----------------------------------------------------------------------

    /// Open unit command menu; `None` — closed.
    pub fn unit_menu(&self) -> Option<&unitmenu::UnitMenu> {
        self.unit_menu.as_ref()
    }

    /// Open command menu for input.
    pub fn unit_menu_mut(&mut self) -> Option<&mut unitmenu::UnitMenu> {
        self.unit_menu.as_mut()
    }

    /// Opens the selected unit command menu — C++ `GameManager_InitPopupButtons`.
    ///
    /// `false` if there is no unit, it is foreign, or no items were collected (C++
    /// `popup_count == 0`, `unit->GetOrder() == ORDER_DISABLE`, `UNIT_READY`
    /// for a ground unit — see [`unitmenu::can_open`]).
    pub fn open_unit_menu(&mut self) -> bool {
        let Some(id) = self.hud.selected else {
            return false;
        };
        let Some(unit) = self.state_ref().unit(id).cloned() else {
            return false;
        };

        if unit.team != self.state_ref().player_team() {
            return false;
        }

        if unit.team != self.state_ref().active_turn_team() {
            return false;
        }

        // `UnitsManager_Popup_InitFactories` (`units_manager.cpp:869-872`): for
        // a factory in `ORDER_STATE_UNIT_READY` the menu does not open —
        // `UnitsManager_Popup_PlaceNewUnit` (`:833`) immediately selects the **ready
        // unit** (`GameManager_EnableMainMenu(parent)`, `parent` — the one whose
        // parent the factory is) and waits for a click on the adjacent cell: the same click
        // later gives `CURSOR_ACTIVATE` → `ORDER_ACTIVATE`
        // (`game_manager.cpp:6918-6926`).
        if unit.orders == UnitOrder::Build && unit.state == UnitOrderState::UnitReady {
            if let Some(stored) = unit.parent_unit {
                if let Some(stored_unit) = self.state_ref().unit(stored) {
                    if stored_unit.orders == UnitOrder::Idle {
                        self.hud.select(Some(stored));
                        self.hud.preview = None;
                        // `GameManager_MenuUnitSelect` → `DrawUnitStatusMessage`:
                        // a ready unit has no hint of its own, the previous
                        // («Finished building …») is cleared.
                        self.hud.clear_hint();
                        self.hud.push_log(format!(
                            "Unit ready in {}: click a cell next to the plant to launch it",
                            crate::buildmenu::unit_name(unit.unit_type)
                        ));
                        self.unit_menu = None;

                        return true;
                    }
                }
            }
        }

        let area = if self.chrome_active() {
            max_render::layout::MAIN_MAP
        } else {
            let (width, height) = self.viewport.screen();
            max_render::buffer::PixelRect::new(0, 0, width as i32, height as i32)
        };

        // Popup anchor — exactly like C++ `GameManager_InitPopupButtons`:
        // `ulx = (unit->x + 32)`, `uly = unit->y - height / 2`, where `unit->x/y`
        // — the cell center (`UnitInfo::SetPosition`: `grid*64 + 32`), i.e.
        // the menu stands to the right of the unit cell and centered on its height.
        let viewport = self.map_viewport();
        let (center_x, center_y) = crate::hud::tile_center(&viewport, unit.grid);
        let zoom = viewport.zoom();
        let anchor = (center_x + zoom as i32 / 2 + area.ulx, center_y + area.uly);

        let Some(mut menu) = unitmenu::UnitMenu::new(self.state_ref(), &unit, anchor, area, zoom)
        else {
            return false;
        };

        // C++ `state[i]`: the item is pressed if its mode is on
        // (`unit->cursor == 3/4/6/7`, `unit->enter_mode`).
        for item in &mut menu.items {
            item.pressed = match item.id {
                unitmenu::ItemId::Transfer => self.hud.cursor_mode == unitmenu::CURSOR_TRANSFER,
                unitmenu::ItemId::Repair => self.hud.cursor_mode == unitmenu::CURSOR_REPAIR,
                unitmenu::ItemId::Reload => self.hud.cursor_mode == unitmenu::CURSOR_RELOAD,
                unitmenu::ItemId::Load => self.hud.cursor_mode == unitmenu::CURSOR_LOAD,
                unitmenu::ItemId::Disable => self.hud.cursor_mode == unitmenu::CURSOR_DISABLE,
                unitmenu::ItemId::Steal => self.hud.cursor_mode == unitmenu::CURSOR_STEAL,
                unitmenu::ItemId::Attack => self.hud.targeting_mode,
                unitmenu::ItemId::Enter => self.hud.enter_mode,
                _ => item.pressed,
            };
        }

        self.unit_menu = Some(menu);
        true
    }

    /// Closes the command menu (`GameManager_DeinitPopupButtons`).
    pub fn close_unit_menu(&mut self) {
        self.unit_menu = None;
    }

    /// Input in the open command menu — [`unitmenu::menu_event`].
    pub fn unit_menu_input(&mut self, event: InputEvent) -> Option<unitmenu::MenuEvent> {
        let Some(mut menu) = self.unit_menu.take() else {
            return None;
        };
        let outcome = unitmenu::menu_event(&menu, event);
        menu.hover = match outcome {
            unitmenu::MenuEvent::Hover(index) => index,
            _ => menu.hover,
        };
        self.unit_menu = Some(menu);
        Some(outcome)
    }

    // -----------------------------------------------------------------------
    // Mine extraction allocation (C++ `AllocMenu`, window `ALLOCFRM`)
    // -----------------------------------------------------------------------

    /// Open extraction allocation dialog; `None` — closed.
    pub fn alloc_menu(&self) -> Option<&allocmenu::AllocMenu> {
        self.alloc_menu.as_ref()
    }

    /// Complex of the selected unit (`unit->GetComplex()`), for QA/log.
    pub fn selected_complex(&self) -> Option<u16> {
        let id = self.hud.selected?;

        Some(self.state_ref().unit(id)?.complex?.0 as u16)
    }

    /// Complex for QA economic checks: of the selected unit, and if it has no
    /// complex (or nothing is selected) — the first complex of the player team
    /// (the smallest id, like `TeamUnits::complexes[0]` in the C++ list).
    pub fn economy_complex(&self) -> Option<u16> {
        if let Some(complex) = self.selected_complex() {
            return Some(complex);
        }

        self.state_ref()
            .team_units()
            .get(self.state_ref().player_team().0 as usize)
            .and_then(|units| units.complexes().first())
            .map(|complex| complex.id as u16)
    }

    /// Open allocation dialog for input/QA.
    pub fn alloc_menu_mut(&mut self) -> Option<&mut allocmenu::AllocMenu> {
        self.alloc_menu.as_mut()
    }

    /// Opens the dialog for the selected mine — C++ `AllocMenu_Menu(unit)`.
    ///
    /// The command menu `allocate` item (`UnitsManager_Popup_OnClick_Allocate`)
    /// calls this. `false` — the unit is not a mine, not own team, disabled or
    /// has no complex.
    pub fn open_alloc_menu(&mut self) -> bool {
        let Some(id) = self.hud.selected else {
            return false;
        };
        let Some(unit) = self.state_ref().unit(id) else {
            return false;
        };
        if unit.team != self.state_ref().player_team() {
            return false;
        }

        let Some(menu) = allocmenu::AllocMenu::new(&mut self.universe, id) else {
            return false;
        };

        self.unit_menu = None;
        self.alloc_menu = Some(menu);
        true
    }

    /// Closes the dialog (`AllocMenu::Run` → `GameManager_EnableMainMenu`).
    ///
    /// The values are already applied by clicks (as in C++): `Done`/`Esc` only
    /// close the window. Returns a log line.
    pub fn close_alloc_menu(&mut self) -> Option<String> {
        let menu = self.alloc_menu.take()?;
        let [raw, fuel, gold] = allocmenu::totals(&menu);

        Some(format!("allocate: raw {raw}, fuel {fuel}, gold {gold}"))
    }

    /// Input in the open dialog — [`allocmenu::AllocMenu::input`].
    pub fn alloc_menu_input(&mut self, event: InputEvent) -> Option<allocmenu::AllocAction> {
        let Some(mut menu) = self.alloc_menu.take() else {
            return None;
        };
        let action = menu.input(&mut self.universe, event);
        self.alloc_menu = Some(menu);
        Some(action)
    }

    /// `allocate set N`/QA: the resource extraction level in the complex.
    pub fn alloc_menu_set(&mut self, kind: max_core::alloc::MiningKind, target: i32) -> bool {
        let Some(mut menu) = self.alloc_menu.take() else {
            return false;
        };
        menu.set_level(&mut self.universe, kind, target);
        self.alloc_menu = Some(menu);
        true
    }

    /// `allocate row …`/QA: `+1`/`−1` resource unit.
    pub fn alloc_menu_adjust(&mut self, kind: max_core::alloc::MiningKind, increase: bool) -> bool {
        let Some(mut menu) = self.alloc_menu.take() else {
            return false;
        };
        let changed = if increase {
            menu.increase(&mut self.universe, kind)
        } else {
            menu.decrease(&mut self.universe, kind)
        };
        self.alloc_menu = Some(menu);
        changed
    }

    /// `ORDER_NEW_ALLOCATE` commands (packet 08, **P5**) for all mines of the complex —
    /// `Remote_SendNetPacket_11` (`remote.cpp:2195`) at the end of `AllocMenu::Run`.
    ///
    /// C++ sends them only in the network; here the orders are always put into
    /// [`hud::HudState::pending`] — offline they are applied by
    /// [`App::flush_orders`] (the fields are already set by the dialog, the order is only
    /// a transient `ORDER_NEW_ALLOCATE`, as in C++), in the network they go
    /// as packet 08/P5 and deliver the extraction to the other clients. Returns the number
    /// commands.
    ///
    /// Call **before** [`Self::close_alloc_menu`] — the complex is taken from
    /// the open dialog.
    pub fn queue_alloc_orders(&mut self) -> usize {
        let Some(menu) = self.alloc_menu.as_ref() else {
            return 0;
        };
        let complex_id = menu.complex.0;
        let team = menu.team;
        let ids = self
            .state_ref()
            .unit_list(max_core::unit::UnitListKind::Stationary)
            .to_vec();
        let mut packets = Vec::new();

        for id in ids {
            let Some(unit) = self.state_ref().unit(id) else {
                continue;
            };
            if unit.team != team
                || unit.complex.map(|complex| complex.0) != Some(complex_id)
                || !max_core::alloc::is_active_mine(unit)
            {
                continue;
            }
            // `UnitsManager_SetNewOrderInt` does not touch `prior_order` if
            // the current order is already `ORDER_NEW_ALLOCATE`.
            let (prior_order, prior_state) =
                if unit.orders == max_core::enums::UnitOrder::NewAllocate {
                    (unit.prior_orders, unit.prior_state)
                } else {
                    (unit.orders, unit.state)
                };
            let fire_on = self.state_ref().order_data(id).fire_on;

            packets.push(max_core::input::Command::Order(
                max_core::input::Packet08Order {
                    unit: id,
                    payload: max_core::input::OrderPayload::P5(max_core::input::PayloadP5 {
                        p3: max_core::input::PayloadP3 {
                            p2: max_core::input::PayloadP2 {
                                p1: max_core::input::PayloadP1 {
                                    order: max_core::enums::UnitOrder::NewAllocate,
                                    state: max_core::enums::UnitOrderState::Init,
                                    prior_order,
                                    prior_state,
                                    disabled_reaction_fire: unit.disabled_reaction_fire,
                                },
                                parent_unit: unit.parent_unit,
                                experience: unit.experience,
                            },
                            move_to: unit.target_grid,
                            fire_on,
                            enemy_unit: None,
                            transfer_cargo: unit.transfer_cargo,
                            stealth_dice_roll: unit.stealth_dice_roll,
                        },
                        total_mining: unit.total_mining,
                        raw_mining: unit.raw_mining,
                        fuel_mining: unit.fuel_mining,
                        gold_mining: unit.gold_mining,
                    }),
                },
            ));
        }

        let count = packets.len();
        self.hud.pending.extend(packets);
        count
    }

    // -----------------------------------------------------------------------
    // X-FER (C++ `TransferMenu`, window `XFERPIC`)
    // -----------------------------------------------------------------------

    /// Open X-FER window; `None` — closed.
    pub fn transfer_menu(&self) -> Option<&transfermenu::TransferMenu> {
        self.transfer_menu.as_ref()
    }

    /// X-FER window for input/QA.
    pub fn transfer_menu_mut(&mut self) -> Option<&mut transfermenu::TransferMenu> {
        self.transfer_menu.as_mut()
    }

    /// Opens the X-FER window between `source` and `target` —
    /// `GameManager_TransferCargo` (`game_manager.cpp:4634`).
    ///
    /// C++ first does `unit1->SetParent(unit2)`; this field travels in packet
    /// 08/P3, so it is set immediately (as in the original — cancelling the dialog does not
    /// restore the parent). `false` — the window did not open (no
    /// complex/cargo type).
    pub fn open_transfer_menu(&mut self, source: UnitId, target: UnitId) -> bool {
        let Some(menu) = transfermenu::TransferMenu::new(self.state_ref(), source, target) else {
            return false;
        };

        if let Some(unit) = self.universe.unit_mut(source) {
            unit.parent_unit = Some(target);
        }

        self.unit_menu = None;
        self.transfer_menu = Some(menu);
        true
    }

    /// Closes the X-FER window without an order (C++ `Esc`/`Cancel`).
    pub fn cancel_transfer(&mut self) -> Option<String> {
        let menu = self.transfer_menu.take()?;
        // `TransferMenu::~TransferMenu` → `GameManager_EnableMainMenu(&*SelectedUnit)`
        // → `GameManager_MenuUnitSelect` resets `cursor`/`enter_mode`.
        self.hud.cursor_mode = 0;
        self.hud.enter_mode = false;

        Some(format!(
            "x-fer: отменено (было {})",
            transfermenu::amount_to_transfer(&menu)
        ))
    }

    /// `Done`/`Enter` — `unit1->transfer_cargo = cargo_transferred;
    /// ORDER_TRANSFER` (packet 08/**P3**).
    ///
    /// A zero amount, as in C++, gives no order (the menu simply
    /// closes). Returns a log line and the flag "the order was put into
    /// [`hud::HudState::pending`]».
    pub fn confirm_transfer(&mut self) -> Option<(String, bool)> {
        let menu = self.transfer_menu.take()?;
        // As in `~TransferMenu`: `EnableMainMenu` → `MenuUnitSelect` resets
        // the cursor mode of the selected unit.
        self.hud.cursor_mode = 0;
        self.hud.enter_mode = false;
        let amount = transfermenu::amount_to_transfer(&menu);

        if amount == 0 {
            return Some(("x-fer: 0 — приказ не отдан".to_string(), false));
        }

        // `unit1->transfer_cargo = cargo_transferred` before `SendNetPacket_08`.
        if let Some(unit) = self.universe.unit_mut(menu.unit) {
            unit.transfer_cargo = amount;
            unit.parent_unit = Some(menu.target);
        }

        let Some(command) =
            transfermenu::transfer_order(self.state_ref(), menu.unit, menu.target, amount)
        else {
            return Some(("x-fer: юнит исчез".to_string(), false));
        };

        self.hud.pending.push(command);
        // `GameManager_TransferCargo` (`game_manager.cpp:4675`): voice `V_M224`
        // after a successful transfer.
        self.play_sfx(sfx::Sfx::Transfer);

        Some((
            format!("x-fer: {amount} ({:?} → {:?})", menu.cargo, menu.target),
            true,
        ))
    }

    /// `transfer amount N`/QA: the bar value with clipping (like a click on the bar).
    pub fn transfer_set_amount(&mut self, amount: i32) -> bool {
        let Some(menu) = self.transfer_menu.as_mut() else {
            return false;
        };
        menu.set_amount(amount);
        true
    }

    /// `transfer right|left`/QA: one bar step.
    pub fn transfer_adjust(&mut self, increase: bool) -> bool {
        let Some(menu) = self.transfer_menu.as_mut() else {
            return false;
        };
        menu.adjust(increase)
    }

    /// Input in the open X-FER window — [`transfermenu::TransferMenu::input`].
    pub fn transfer_menu_input(
        &mut self,
        event: InputEvent,
    ) -> Option<transfermenu::TransferAction> {
        let Some(mut menu) = self.transfer_menu.take() else {
            return None;
        };
        let action = menu.input(event);
        self.transfer_menu = Some(menu);
        Some(action)
    }

    /// Activates a menu item — C++ `GameManager_PopupButtons.r_func[i]`.
    ///
    /// The menu closes (like `GameManager_DeinitPopupButtons(true)` in each
    /// C++ handler). The order, if the item gives one, is put into
    /// [`hud::HudState::pending`] — then [`App::flush_orders`] (offline) or
    /// the network layer. Returns a log line.
    ///
    /// `None` — the item is absent/disabled (the mechanics are not ported, see
    /// [`unitmenu::UnitMenuItem::note`]).
    pub fn activate_unit_menu_item(&mut self, index: usize) -> Option<String> {
        let menu = self.unit_menu.take()?;
        let item = menu.items.get(index)?.clone();
        let unit_id = menu.unit;

        if !item.enabled {
            // A disabled item remains in the menu (like a C++ button without a handler).
            self.unit_menu = Some(menu);
            return Some(unitmenu::disabled_line(&item));
        }

        let line = match item.id {
            unitmenu::ItemId::Build => {
                if self.open_build_menu() {
                    format!("{} — BuildMenu", item.caption)
                } else {
                    format!("{}: юнит не умеет строить", item.caption)
                }
            }
            unitmenu::ItemId::BuyUpgrade => {
                if self.open_upgrade_menu() {
                    "buy upg — UpgradeMenu".to_string()
                } else {
                    "buy upg: экран апгрейдов недоступен".to_string()
                }
            }
            unitmenu::ItemId::Research => {
                if self.open_research_menu() {
                    "resrch — ResearchMenu".to_string()
                } else {
                    "resrch: экран исследований недоступен".to_string()
                }
            }
            unitmenu::ItemId::PowerOn | unitmenu::ItemId::PowerOff => {
                match crate::hud::build_power_order(self.state_ref(), unit_id) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("{} — {}", item.caption, "Power toggled")
                    }
                    None => format!("{}: питание недоступно", item.caption),
                }
            }
            unitmenu::ItemId::Sentry => {
                let sentry = self
                    .state_ref()
                    .unit(unit_id)
                    .is_some_and(|unit| unit.orders == max_core::enums::UnitOrder::Sentry);

                let (order, order_state) = if sentry {
                    (
                        max_core::enums::UnitOrder::Await,
                        max_core::enums::UnitOrderState::ExecutingOrder,
                    )
                } else {
                    (
                        max_core::enums::UnitOrder::Sentry,
                        max_core::enums::UnitOrderState::ExecutingOrder,
                    )
                };

                match unitmenu::p1_order(self.state_ref(), unit_id, order, order_state) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        if sentry {
                            "sentry off — Await".to_string()
                        } else {
                            "sentry — ORDER_SENTRY".to_string()
                        }
                    }
                    None => "sentry: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::Stop => {
                let move_to = self
                    .state_ref()
                    .unit(unit_id)
                    .map(|unit| unit.target_grid)
                    .unwrap_or(Point::ZERO);

                match unitmenu::p3_order(
                    self.state_ref(),
                    unit_id,
                    max_core::enums::UnitOrder::Await,
                    max_core::enums::UnitOrderState::ClearPath,
                    move_to,
                    None,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        "stop — ORDER_AWAIT/CLEAR_PATH".to_string()
                    }
                    None => "stop: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::Upgrade => {
                match unitmenu::p2_order(
                    self.state_ref(),
                    unit_id,
                    max_core::enums::UnitOrder::Upgrade,
                    max_core::enums::UnitOrderState::Init,
                    Some(unit_id),
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        "upgrade — ORDER_UPGRADE".to_string()
                    }
                    None => "upgrade: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::Done => {
                // `UnitsManager_PerformAction` to the extent present in our
                // stack: `Await` + `STATIONARY` → `READY_TO_EXECUTE_ORDER`
                // (by an order, so that the state goes to the network too), then the selection
                // is cleared (`auto_select` in C++).
                let ready = self.state_ref().unit(unit_id).is_some_and(|unit| {
                    unit.orders == max_core::enums::UnitOrder::Await
                        && !unit.is_mobile()
                        && unit.state != max_core::enums::UnitOrderState::ReadyToExecuteOrder
                });

                if ready {
                    if let Some(command) = unitmenu::p3_order(
                        self.state_ref(),
                        unit_id,
                        max_core::enums::UnitOrder::Await,
                        max_core::enums::UnitOrderState::ReadyToExecuteOrder,
                        self.state_ref()
                            .unit(unit_id)
                            .map(|unit| unit.target_grid)
                            .unwrap_or(Point::ZERO),
                        None,
                    ) {
                        self.hud.pending.push(command);
                    }
                }

                self.hud.clear_interaction();
                "Done".to_string()
            }
            unitmenu::ItemId::Manual => {
                let unit = self.state_ref().unit(unit_id).cloned()?;
                let order = unit.orders;
                let order_state = if order == max_core::enums::UnitOrder::Sentry {
                    max_core::enums::UnitOrderState::ExecutingOrder
                } else {
                    unit.state
                };

                // P1 carries `disabled_reaction_fire` — the same way as the network.
                let command = unitmenu::p1_order(self.state_ref(), unit_id, order, order_state)?;
                if let Command::Order(packet) = command {
                    let mut packet = packet;
                    if let max_core::input::OrderPayload::P1(p1) = &mut packet.payload {
                        p1.disabled_reaction_fire = !unit.disabled_reaction_fire;
                    }
                    self.hud.pending.push(Command::Order(packet));
                }

                if unit.disabled_reaction_fire {
                    "Manual: ответный огонь включён".to_string()
                } else {
                    "Manual: ответный огонь выключен".to_string()
                }
            }
            unitmenu::ItemId::Auto => {
                let Some(unit) = self.universe.unit_mut(unit_id) else {
                    return Some("Auto: юнит исчез".to_string());
                };
                unit.auto_survey = !unit.auto_survey;
                let enabled = unit.auto_survey;

                if enabled {
                    "Auto: авто-разведка включена".to_string()
                } else {
                    "Auto: авто-разведка выключена".to_string()
                }
            }
            unitmenu::ItemId::Enter => {
                // C++ `UnitsManager_Popup_OnClick_Enter`: the `enter_mode` toggle.
                self.hud.enter_mode = !self.hud.enter_mode;
                if self.hud.enter_mode {
                    "enter: клик по своему транспорту — посадка".to_string()
                } else {
                    "enter: выключено".to_string()
                }
            }
            unitmenu::ItemId::Allocate => {
                // C++ `UnitsManager_Popup_OnClick_Allocate` → `AllocMenu_Menu(unit)`.
                if self.open_alloc_menu() {
                    "allocate — Allocation Menu".to_string()
                } else {
                    "allocate: нужна включённая шахта с комплексом".to_string()
                }
            }
            unitmenu::ItemId::Transfer
            | unitmenu::ItemId::Reload
            | unitmenu::ItemId::Repair
            | unitmenu::ItemId::Load
            | unitmenu::ItemId::Disable
            | unitmenu::ItemId::Steal => {
                // C++ cursor modes (`unit->cursor = 3/6/4/7/9/8`).
                let (cursor, label) = match item.id {
                    unitmenu::ItemId::Transfer => (unitmenu::CURSOR_TRANSFER, "x-fer"),
                    unitmenu::ItemId::Reload => (unitmenu::CURSOR_RELOAD, "reload"),
                    unitmenu::ItemId::Repair => (unitmenu::CURSOR_REPAIR, "repair"),
                    unitmenu::ItemId::Disable => (unitmenu::CURSOR_DISABLE, "disable"),
                    unitmenu::ItemId::Steal => (unitmenu::CURSOR_STEAL, "steal"),
                    _ => (unitmenu::CURSOR_LOAD, "load"),
                };

                self.hud.cursor_mode = if self.hud.cursor_mode == cursor {
                    0
                } else {
                    cursor
                };

                if self.hud.cursor_mode == cursor {
                    format!("{label}: выберите цель кликом")
                } else {
                    format!("{label}: режим выключен")
                }
            }
            unitmenu::ItemId::Attack => {
                // `UnitsManager_Popup_OnClick_TargetingMode`
                // (`units_manager.cpp:617`): the `targeting_mode` toggle, the cursor and
                // `enter_mode` are reset.
                self.hud.targeting_mode = !self.hud.targeting_mode;
                self.hud.cursor_mode = 0;
                self.hud.enter_mode = false;

                if self.hud.targeting_mode {
                    "attack: наведение включено — клик по врагу или клетке в радиусе".to_string()
                } else {
                    "attack: наведение выключено".to_string()
                }
            }
            unitmenu::ItemId::RemoveStationary => {
                // `UnitsManager_Popup_OnClick_Remove` → `UnitsManager_SelfDestructMenu`.
                if self.open_self_destruct(unit_id) {
                    format!("{}: подтвердите уничтожение здания", item.caption)
                } else {
                    format!("{}: здание сейчас нельзя уничтожить", item.caption)
                }
            }
            unitmenu::ItemId::RemoveRubble => {
                // `UnitsManager_Popup_OnClick_StopRemove` (`units_manager.cpp:1082`):
                // `Access_GetRemovableRubble` → `parent = rubble`,
                // `build_time` 4/1 and `ORDER_CLEAR`/`INIT`.
                let Some(unit) = self.state_ref().unit(unit_id).cloned() else {
                    return Some("remove: юнит исчез".to_string());
                };
                let team = unit.team;
                let grid = unit.grid;

                let Some(rubble) = unitmenu::removable_rubble(self.state_ref(), team, grid) else {
                    return Some("Unable to clear at current location.".to_string());
                };

                let rubble_is_building = self
                    .state_ref()
                    .unit(rubble)
                    .is_some_and(|unit| unit.is_quad());
                let clearing_time = if rubble_is_building { 4 } else { 1 };

                if let Some(unit) = self.universe.unit_mut(unit_id) {
                    unit.parent_unit = Some(rubble);
                    unit.build_time = clearing_time;
                }

                match unitmenu::clear_order(self.state_ref(), unit_id, rubble) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("Number of turns to clear site: {clearing_time}.")
                    }
                    None => "remove: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::StopRemove => {
                // `GameManager_SelectBuildSite` (`game_manager.cpp:3549-3552`):
                // `move_to = own cell` + `ORDER_BUILD`/`BUILD_CANCEL`;
                // `CancelBuilding` of a non-stationary unit returns `AWAIT`.
                match unitmenu::p1_order(
                    self.state_ref(),
                    unit_id,
                    max_core::enums::UnitOrder::Build,
                    max_core::enums::UnitOrderState::BuildCancel,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        "stop: расчистка отменена".to_string()
                    }
                    None => "stop: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::UpgradeAll => {
                // `UnitsManager_Popup_OnClick_UpgradeAll` (`units_manager.cpp:414`).
                match unitmenu::upgrade_all_orders(self.state_ref(), unit_id) {
                    Some(report) => {
                        self.hud.pending.extend(report.commands);
                        report.message
                    }
                    None => "Upg. All: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::StartBuild => {
                // C++ `UnitsManager_Popup_OnClick_BuildOrder` →
                // `UnitInfo::BuildOrder`: the queue and `build_time` remained from
                // `CancelBuilding` (factory), so `ORDER_BUILD` +
                // `ORDER_STATE_INIT` is enough — `start_building` resumes the construction.
                match unitmenu::p1_order(
                    self.state_ref(),
                    unit_id,
                    max_core::enums::UnitOrder::Build,
                    max_core::enums::UnitOrderState::Init,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("{} — ORDER_BUILD/INIT (продолжение)", item.caption)
                    }
                    None => format!("{}: юнит исчез", item.caption),
                }
            }
            unitmenu::ItemId::StopBuild => {
                // C++ `GameManager_SelectBuildSite` (`game_manager.cpp:3529`):
                // `ORDER_BUILD` + `ORDER_STATE_BUILD_CANCEL` — `CancelBuilding`
                // `ORDER_AWAIT` for a mobile builder, `HaltBuilding` for a factory (the construction can be
                // resumed by the `start` item).
                // resume with the `start` item).
                match unitmenu::p1_order(
                    self.state_ref(),
                    unit_id,
                    max_core::enums::UnitOrder::Build,
                    max_core::enums::UnitOrderState::BuildCancel,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("{} — ORDER_BUILD/BUILD_CANCEL", item.caption)
                    }
                    None => format!("{}: юнит исчез", item.caption),
                }
            }
            unitmenu::ItemId::PlaceMine | unitmenu::ItemId::RemoveMine => {
                let state_order = if item.id == unitmenu::ItemId::PlaceMine {
                    max_core::enums::UnitOrderState::PlacingMines
                } else {
                    max_core::enums::UnitOrderState::RemovingMines
                };

                match unitmenu::p1_order(
                    self.state_ref(),
                    unit_id,
                    max_core::enums::UnitOrder::LayMine,
                    state_order,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("{} — ORDER_LAY_MINE", item.caption)
                    }
                    None => format!("{}: юнит исчез", item.caption),
                }
            }
            // The other items are disabled when the list is built.
            _ => unitmenu::disabled_line(&item),
        };

        Some(line)
    }

    // -----------------------------------------------------------------------
    // Help window (`HelpMenu`, P7)
    // -----------------------------------------------------------------------

    /// Open help window over the game.
    pub fn help_screen(&self) -> Option<&helpmenu::HelpScreen> {
        self.help.as_ref()
    }

    /// Open help window for input.
    pub fn help_screen_mut(&mut self) -> Option<&mut helpmenu::HelpScreen> {
        self.help.as_mut()
    }

    /// Enables help mode — C++ `HelpMenu_Menu(section, WINDOW_MAIN_MAP)`
    /// (`helpmenu.cpp:315`): cursor `CURSOR_HELP`, the window opens on a click on
    /// an element with a `help.json` entry under the point.
    pub fn open_help(&mut self, section: helpmenu::HelpSection) -> bool {
        self.open_help_with(section, helpmenu::HelpPlacement::Game)
    }

    /// Help mode with an explicit window position.
    pub fn open_help_with(
        &mut self,
        section: helpmenu::HelpSection,
        placement: helpmenu::HelpPlacement,
    ) -> bool {
        self.help = None;
        self.help_mode = Some(helpmenu::HelpMode::new(section, placement));
        true
    }

    /// Opens the help window directly on entry `name` (QA/tests; in C++ the window
    /// opens only on a click on an element).
    pub fn open_help_entry(
        &mut self,
        section: helpmenu::HelpSection,
        placement: helpmenu::HelpPlacement,
        name: &str,
    ) -> bool {
        let Some(index) = helpmenu::entry_by_name(section, name) else {
            return false;
        };

        self.help = Some(helpmenu::HelpScreen::for_entry(section, placement, index));
        true
    }

    /// Window for the first section entry (without the mode; a safeguard for tests).
    pub fn open_help_first(
        &mut self,
        section: helpmenu::HelpSection,
        placement: helpmenu::HelpPlacement,
    ) -> bool {
        self.help = Some(helpmenu::HelpScreen::new(section, placement));
        true
    }

    /// Active help mode.
    pub fn help_mode(&self) -> Option<helpmenu::HelpMode> {
        self.help_mode
    }

    /// A click in help mode: `true` — the entry was found and the window opened.
    ///
    /// C++ `HelpMenu_Menu`: a click on an empty place does not open the window and does not
    /// disable the mode; `Esc`/right button/`Shift-/` disable the mode.
    pub fn help_mode_click(&mut self, x: i32, y: i32) -> bool {
        let Some(mode) = self.help_mode else {
            return false;
        };
        let Some(screen) = mode.click(x, y) else {
            return false;
        };

        self.help = Some(screen);
        true
    }

    /// Exit from help mode (`Esc`/RMB/`Shift-/`).
    pub fn exit_help_mode(&mut self) {
        self.help = None;
        self.help_mode = None;
    }

    /// Closes the help window (like `Done`/`Enter`/`Esc`); the mode remains
    /// active — C++ `HelpMenu::Run` returns control to `HelpMenu_Menu`.
    pub fn close_help(&mut self) {
        self.help = None;
    }

    /// Input in the help window; returns the action (close/no).
    pub fn help_input(&mut self, event: InputEvent) -> Option<helpmenu::HelpAction> {
        let Some(mut help) = self.help.take() else {
            return None;
        };
        let action = help.input(event);
        self.help = Some(help);
        Some(action)
    }

    // -----------------------------------------------------------------------
    // Building self-destruct (C++ `UnitsManager_SelfDestructMenu`)
    // -----------------------------------------------------------------------

    /// Open self-destruct dialog; `None` — closed.
    pub fn self_destruct(&self) -> Option<&selfdestruct::SelfDestructMenu> {
        self.self_destruct.as_ref()
    }

    /// Opens the dialog for a building — `UnitsManager_Popup_OnClick_Remove`
    /// (`units_manager.cpp:506`): only for a stationary own unit.
    pub fn open_self_destruct(&mut self, unit_id: UnitId) -> bool {
        let Some(unit) = self.state_ref().unit(unit_id) else {
            return false;
        };

        if unit.team != self.state_ref().player_team() || unit.is_mobile() {
            return false;
        }

        self.unit_menu = None;
        self.self_destruct = Some(selfdestruct::SelfDestructMenu::new(unit_id));
        true
    }

    /// Input in the dialog (`SelfDestructMenu`).
    ///
    /// `Explode` — `ORDER_EXPLODE`/`ORDER_STATE_EXPLODE` into the order queue,
    /// like `UnitsManager_SetNewOrder(unit, ORDER_EXPLODE, ORDER_STATE_EXPLODE)`.
    pub fn self_destruct_input(
        &mut self,
        event: InputEvent,
    ) -> Option<selfdestruct::SelfDestructAction> {
        let mut menu = self.self_destruct.take()?;
        let action = menu.input(event);

        match action {
            selfdestruct::SelfDestructAction::None => {
                self.self_destruct = Some(menu);
            }
            selfdestruct::SelfDestructAction::Cancel => {}
            selfdestruct::SelfDestructAction::Explode => {
                if let Some(command) = unitmenu::explode_order(self.state_ref(), menu.unit) {
                    self.hud.pending.push(command);
                }
            }
        }

        Some(action)
    }

    /// Advances the `SLFDOPN1..6` frames by the visual clock.
    pub fn advance_self_destruct(&mut self, dt: f32) {
        if let Some(menu) = self.self_destruct.as_mut() {
            menu.advance(dt);
        }
    }

    // -----------------------------------------------------------------------
    // UI sounds (C++ `SoundManager::PlaySfx`)
    // -----------------------------------------------------------------------

    /// Puts a sound into the frame queue (played by `main.rs` via `AudioPlayer`).
    pub fn play_sfx(&mut self, sfx: sfx::Sfx) {
        self.sounds.push(sfx);
    }

    /// Sounds accumulated since the previous call.
    pub fn take_sounds(&mut self) -> Vec<sfx::Sfx> {
        std::mem::take(&mut self.sounds)
    }

    /// Current sound queue (for tests/QA).
    pub fn sounds(&self) -> &[sfx::Sfx] {
        &self.sounds
    }

    /// Takes combat events from the simulation and turns them into frame sounds.
    ///
    /// Called right after `GameState::tick` (in [`App::tick`] and
    /// `App::local_update`): the simulation accumulates `max_core::UnitSound`
    /// (`PrepareFire`/`StartExplosion`), while the resource and volume are chosen by the
    /// `SC_UNITS` table ([`sfx::combat_sound`]). The queue is read by `main.rs` and played
    /// via `AudioPlayer` (a mock player in tests).
    pub fn collect_combat_sounds(&mut self) -> usize {
        // The frame queue: `main.rs` takes it every frame. QA/headless runs
        // do not play sound, so old events are trimmed so that the queue
        // does not grow over tens of thousands of soak frames.
        const MAX_QUEUED: usize = 64;

        let events = self.universe.take_unit_sounds();
        let mut added = 0;

        for event in events {
            if let Some(sound) = sfx::combat_sound(event) {
                self.unit_sounds.push(sound);
                added += 1;
            }
        }

        if self.unit_sounds.len() > MAX_QUEUED {
            let excess = self.unit_sounds.len() - MAX_QUEUED;
            self.unit_sounds.drain(0..excess);
        }

        added
    }

    /// Takes the combat sounds of the facade event stream ([`App::tick`], stage A).
    ///
    /// The facade publishes `GameEvent::Sound` from `GameState::unit_sounds`, so the
    /// frame queue stays exactly the same as with [`App::collect_combat_sounds`].
    /// Other events are ignored in stage A: the local match still reads its UI
    /// reports from the state (`local.rs`, the `local_update` tick path).
    fn collect_facade_sounds(&mut self) -> usize {
        const MAX_QUEUED: usize = 64;

        // Separate field borrows: the cursor and the facade are disjoint fields.
        let events = self.universe_events.drain(&mut self.universe);
        let mut added = 0;

        for event in events {
            if let GameEvent::Sound(event) = event {
                if let Some(sound) = sfx::combat_sound(event) {
                    self.unit_sounds.push(sound);
                    added += 1;
                }
            }
        }

        if self.unit_sounds.len() > MAX_QUEUED {
            let excess = self.unit_sounds.len() - MAX_QUEUED;
            self.unit_sounds.drain(0..excess);
        }

        added
    }

    /// Takes the combat sounds accumulated since the previous call.
    pub fn take_unit_sounds(&mut self) -> Vec<sfx::CombatSfx> {
        std::mem::take(&mut self.unit_sounds)
    }

    /// Current combat sound queue (for tests/QA).
    pub fn unit_sounds(&self) -> &[sfx::CombatSfx] {
        &self.unit_sounds
    }

    // -----------------------------------------------------------------------
    // Pause (`PauseMenu_Menu` → `DialogMenu`, `Alt+P`)
    // -----------------------------------------------------------------------

    /// Whether the pause dialog is open.
    pub fn pause_dialog(&self) -> Option<&dialog::DialogMenu> {
        self.pause_dialog.as_ref()
    }

    /// Opens the pause — `PauseMenu_Menu` (`menu.cpp:1747`): only in a match,
    /// ticks and the turn timer stop (C++ adjusts
    /// `menu_turn_timer_value` for the duration of the dialog).
    pub fn open_pause_dialog(&mut self) -> bool {
        if self.local.is_none() || self.pause_dialog.is_some() {
            return false;
        }

        self.pause_was_paused = self.local.as_ref().is_some_and(|local| local.paused);

        if let Some(local) = self.local.as_mut() {
            local.paused = true;
        }

        self.pause_dialog = Some(dialog::DialogMenu::pause());
        self.play_sfx(sfx::Sfx::Button);
        true
    }

    /// Input in the pause dialog; `OK`/`Enter`/`Esc` close it.
    pub fn pause_dialog_input(&mut self, event: InputEvent) -> Option<dialog::DialogAction> {
        let mut dialog = self.pause_dialog.take()?;
        let action = dialog.input(event);

        match action {
            dialog::DialogAction::None => {
                self.pause_dialog = Some(dialog);
            }
            dialog::DialogAction::Close => {
                if let Some(local) = self.local.as_mut() {
                    local.paused = self.pause_was_paused;
                }
                self.play_sfx(if matches!(event, InputEvent::Key(ui::Key::Escape)) {
                    sfx::Sfx::Cancel
                } else {
                    sfx::Sfx::Done
                });
            }
        }

        Some(action)
    }

    /// Game assets (`None` before [`App::init_assets`]).
    pub fn assets(&self) -> Option<&Assets> {
        self.assets.as_ref()
    }

    /// Camera/frame scale.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Whether the 1:1 window chrome is active (`FRAMEPIC` and the frame exactly 640×480).
    pub fn chrome_active(&self) -> bool {
        self.viewport.screen()
            == (
                max_render::layout::WINDOW_WIDTH as u32,
                max_render::layout::WINDOW_HEIGHT as u32,
            )
            && self
                .assets
                .as_ref()
                .and_then(|assets| assets.chrome.as_ref())
                .is_some_and(|chrome| chrome.is_ready())
    }

    /// Camera of the window map area: 448×448, the same center and zoom.
    ///
    /// With active chrome the map is drawn into its own 448×448 buffer
    /// (`WINDOW_MAIN_MAP`), so the mouse is also converted by this camera and not
    /// by the camera of the whole window.
    pub fn map_viewport(&self) -> Viewport {
        if self.chrome_active() {
            crate::panels::map_viewport(&self.viewport)
        } else {
            self.viewport
        }
    }

    /// Editable camera.
    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }

    // -----------------------------------------------------------------------
    // Camera scrolling (Preferences: click_scroll / quick_scroll)
    // -----------------------------------------------------------------------

    /// Remembers the mouse position for edge scrolling (`MouseMove`).
    pub fn note_mouse_move(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 {
            self.scroll.clear_pointer();
        } else {
            self.scroll.set_pointer(x, y);
        }
    }

    /// LMB click: with `click_scroll` enables the "hold" for scrolling.
    pub fn note_mouse_click(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 {
            self.scroll.press(x, y);
        }
    }

    /// Scroll state (pointer/hold) — for tests and the live UI.
    pub fn scroll_state(&self) -> &scroll::ScrollState {
        &self.scroll
    }

    /// One frame of edge scrolling — a port of `GameManager_ProcessInput` →
    /// `GameManager_SetGridOffset` → `GameManager_ScrollMainMapView`.
    ///
    /// Returns the camera shift in screen pixels, if any. The shift
    /// is applied to [`App::viewport`] by the same function
    /// [`Viewport::scroll_by_screen`] as the rest of the mouse input, so clicks
    /// after scrolling hit the same cells.
    ///
    /// `click_scroll`: with `true` the map moves only while the LMB is "held"
    /// (`GameManager_SetGridOffset`, `game_manager.cpp:6352`); with `false` — on
    /// hover. `quick_scroll` — "Scroll Speed" (`GridStepLevel`,
    /// `game_manager.cpp:1711`).
    pub fn update_scroll(&mut self, dt: f32) -> Option<(i32, i32)> {
        self.scroll.tick(dt);

        let (dx, dy) = self.scroll.step(
            dt,
            self.preferences.click_scroll,
            self.preferences.quick_scroll,
        )?;

        if dx == 0 && dy == 0 {
            return None;
        }

        let before = self.viewport.center();
        self.viewport.scroll_by_screen(dx, dy);

        // `GameManager_ScrollMainMapView` returns `false` if the map boundaries
        // did not change; at the map edge the step is not counted as scrolling.
        (self.viewport.center() != before).then_some((dx, dy))
    }

    // -----------------------------------------------------------------------
    // Camera following the selected unit (Preferences: follow_unit)
    // -----------------------------------------------------------------------

    /// A port of `UnitInfo::FollowUnit` (`unitinfo.cpp:3597`).
    ///
    /// With `follow_unit` the camera centers on the selected unit as soon as it
    /// leaves the map window. The visibility conditions — literally C++:
    /// turn-based, or the unit's team turn, or it has already finished the turn; the unit
    /// is visible to the local team.
    ///
    /// Returns `true` if the camera moved.
    pub fn follow_selected_unit(&mut self) -> bool {
        if !self.preferences.follow_unit {
            return false;
        }

        let Some(id) = self.hud.selected else {
            return false;
        };
        let player = self.state_ref().player_team();
        let Some(unit) = self.state_ref().unit(id) else {
            return false;
        };

        if !unit.is_visible_to_team(player) {
            return false;
        }

        let finished_turn = self.state_ref().teams()[player.0 as usize].finished_turn;
        let allowed = self.state_ref().play_mode() == max_core::enums::PlayMode::TurnBased
            || unit.team == player
            || finished_turn;

        if !allowed {
            return false;
        }

        let grid = unit.grid;

        if self.map_viewport().is_tile_visible(grid.x, grid.y) {
            return false;
        }

        self.viewport.center_on_grid(grid.x, grid.y);

        true
    }

    // -----------------------------------------------------------------------
    // Auto-selection of the next unit (Preferences: auto_select)
    // -----------------------------------------------------------------------

    /// Snapshot of the selected unit's order before the tick — to distinguish
    /// "the order has just completed" from "the unit stands and waits".
    fn selected_order_snapshot(&self) -> Option<OrderSnapshot> {
        let id = self.hud.selected?;
        let unit = self.state_ref().unit(id)?;

        Some(OrderSnapshot {
            id,
            orders: unit.orders,
            state: unit.state,
            has_path: unit.path.is_some(),
            speed: unit.speed,
        })
    }

    /// Reacts to the completion of the selected unit's order, like
    /// `UnitsManager_PerformAction`/`UnitInfo::MoveFinished`
    /// (`unitinfo.cpp:3654` → `GameManager_AutoSelectNext`, `game_manager.cpp:1779`).
    ///
    /// * the move order (`ORDER_MOVE`/`MOVE_TO_UNIT`/`MOVE_TO_ATTACK`) was in
    ///   progress and completed (`AWAIT` without a path) or the unit was destroyed;
    /// * if `auto_select` is off or the unit is still "interactive"
    ///   (`GameManager_IsInteractable`) — the selection remains;
    /// * otherwise the next unit of the team is selected (`GameManager_SelectNextUnit`).
    pub fn auto_select_after_tick(&mut self, before: Option<OrderSnapshot>) {
        if !self.preferences.auto_select {
            return;
        }

        let Some(before) = before else {
            return;
        };

        // The player managed to select another unit — do not override it.
        if self.hud.selected != Some(before.id) {
            return;
        }

        let moved_order = matches!(
            before.orders,
            UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
        );

        if !moved_order {
            return;
        }

        let was_moving = before.has_path
            || matches!(
                before.state,
                UnitOrderState::InProgress
                    | UnitOrderState::InTransition
                    | UnitOrderState::Init
                    | UnitOrderState::MoveInit
                    | UnitOrderState::IssuingPath
                    | UnitOrderState::ExecutingOrder
            );

        if !was_moving {
            return;
        }

        let finished = match self.state_ref().unit(before.id) {
            // The unit was destroyed on this tick (`CheckIfDestroyed` in C++ calls
            // `GameManager_AutoSelectNext` for the selected one).
            None => true,
            Some(unit) => {
                unit.hits == 0
                    || (matches!(unit.orders, UnitOrder::Await | UnitOrder::Idle)
                        && unit.path.is_none())
            }
        };

        if !finished {
            return;
        }

        if let Some(unit) = self.state_ref().unit(before.id) {
            if unit.hits > 0 && self.is_interactable(unit) {
                return;
            }
        }

        self.select_next_unit();
    }

    /// After `BuildMenu` (`UnitsManager_Popup_OnClick_Build` →
    /// `GameManager_AutoSelectNext`) — auto-select the next unit if
    /// the current one is no longer interactive.
    pub fn auto_select_after_order(&mut self) {
        if !self.preferences.auto_select {
            return;
        }

        let Some(id) = self.hud.selected else {
            return;
        };

        if let Some(unit) = self.state_ref().unit(id) {
            if unit.hits > 0 && self.is_interactable(unit) {
                return;
            }
        }

        self.select_next_unit();
    }

    /// `GameManager_IsInteractable` (`game_manager.cpp:3230`): the unit can still
    /// act, so auto-selection does not skip it.
    ///
    /// An exact port of `Access_FindReachableSpot` is absent in max_core; "there is
    /// something to shoot at" is estimated as `shots > 0` and an enemy within `RANGE`
    /// (the squared distance, as in [`max_core::access::is_within_attack_range`]).
    fn is_interactable(&self, unit: &max_core::unit::UnitInfo) -> bool {
        if matches!(
            unit.orders,
            UnitOrder::Sentry
                | UnitOrder::Idle
                | UnitOrder::Disable
                | UnitOrder::Explode
                | UnitOrder::AwaitScaling
        ) || matches!(
            unit.state,
            UnitOrderState::Destroy | UnitOrderState::ReadyToExecuteOrder
        ) {
            return false;
        }

        if matches!(unit.orders, UnitOrder::Build | UnitOrder::Clear) {
            return unit.state == UnitOrderState::SelectSite;
        }

        if unit.speed > 0 {
            return true;
        }

        if unit.shots > 0 {
            let range = i32::from(
                unit.base_values
                    .get_attribute(max_core::unit_values::ATTRIB_RANGE),
            );
            let squared = range * range;

            for id in self.state_ref().unit_ids_in_order() {
                let Some(enemy) = self.state_ref().unit(id) else {
                    continue;
                };

                if enemy.team == unit.team
                    || enemy.hits == 0
                    || !enemy.is_visible_to_team(unit.team)
                {
                    continue;
                }

                if max_core::access::squared_distance(unit.grid, enemy.grid) <= squared {
                    return true;
                }
            }
        }

        false
    }

    /// `GameManager_SelectNextUnit(1)` (`game_manager.cpp:1544`): the next
    /// valid team unit in `Access_UnitsLists` order, wrapping to
    /// the start; the camera centers if it is outside the window.
    pub fn select_next_unit(&mut self) -> Option<UnitId> {
        let player = self.state_ref().player_team();
        let current = self.hud.selected;
        let next = self.next_valid_unit(player, current)?;

        self.close_unit_menu();
        self.select_unit(Some(next));

        let grid = self.state_ref().unit(next).map(|unit| unit.grid);

        if let Some(grid) = grid {
            if !self.map_viewport().is_tile_visible(grid.x, grid.y) {
                self.viewport.center_on_grid(grid.x, grid.y);
            }
        }

        Some(next)
    }

    /// `Access_SeekNextUnit(team, unit, +1)` over the five lists in
    /// `Access_UnitsLists` (mobile land/sea → air → stationary → ground cover →
    /// order), then `GameManager_GetFirstRelevantUnit`.
    fn next_valid_unit(&self, team: TeamId, current: Option<UnitId>) -> Option<UnitId> {
        const ORDER_: [max_core::unit::UnitListKind; 5] = [
            max_core::unit::UnitListKind::MobileLandSea,
            max_core::unit::UnitListKind::MobileAir,
            max_core::unit::UnitListKind::Stationary,
            max_core::unit::UnitListKind::GroundCover,
            max_core::unit::UnitListKind::Particle,
        ];

        // Flat traversal order + the position of the current unit in it.
        let mut flat: Vec<UnitId> = Vec::new();

        for kind in ORDER_ {
            flat.extend_from_slice(self.state_ref().unit_list(kind));
        }

        let start = current
            .and_then(|id| flat.iter().position(|candidate| *candidate == id))
            .map(|index| index + 1)
            .unwrap_or(0);

        for step in 0..flat.len() {
            let id = flat[(start + step) % flat.len()];
            let Some(unit) = self.state_ref().unit(id) else {
                continue;
            };

            // C++ `Access_SeekNextUnit` does not return the current unit itself.
            if Some(id) == current {
                continue;
            }

            if unit.team == team && Self::is_valid_next_unit(unit) {
                return Some(id);
            }
        }

        // `GameManager_GetFirstRelevantUnit`: a mine, otherwise the first active unit.
        for kind in ORDER_ {
            for id in self.state_ref().unit_list(kind) {
                let Some(unit) = self.state_ref().unit(*id) else {
                    continue;
                };

                if unit.team == team && unit.hits > 0 && unit.unit_type == UnitType::MiningStation {
                    return Some(*id);
                }
            }
        }

        for kind in ORDER_ {
            for id in self.state_ref().unit_list(kind) {
                let Some(unit) = self.state_ref().unit(*id) else {
                    continue;
                };

                if unit.team == team && unit.hits > 0 && Self::is_valid_next_unit(unit) {
                    return Some(*id);
                }
            }
        }

        None
    }

    /// `Access_IsValidNextUnit` (`access.cpp:1377`).
    fn is_valid_next_unit(unit: &max_core::unit::UnitInfo) -> bool {
        if matches!(
            unit.orders,
            UnitOrder::Sentry | UnitOrder::Idle | UnitOrder::Disable | UnitOrder::Explode
        ) || matches!(
            unit.state,
            UnitOrderState::Destroy | UnitOrderState::ReadyToExecuteOrder
        ) || (unit.orders == UnitOrder::Build && unit.state != UnitOrderState::UnitReady)
        {
            return false;
        }

        if unit.speed > 0 || unit.shots > 0 || unit.storage != 0 {
            return true;
        }

        unit.orders != UnitOrder::Await
    }

    /// How many ticks have run since loading/generation.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// How many commands were applied successfully.
    pub fn applied_commands(&self) -> u64 {
        self.applied_commands
    }

    /// Loads game data from the [`AppConfig`] paths.
    ///
    /// Returns a report; a repeated call re-reads the files.
    pub fn init_assets(&mut self) -> Result<AssetsReport> {
        let assets = Assets::load(&self.config)?;
        let report = assets.report();

        for warning in &report.warnings {
            tracing::warn!("assets: {warning}");
        }
        tracing::info!(
            "assets: MAX.RES={} PATCHES.RES={} world={} ({}) {}x{} sprites={}",
            report.max_res_entries,
            report.patches_res_entries,
            report.world,
            report.world_source,
            report.world_size.0,
            report.world_size.1,
            report.sprite_count()
        );

        self.assets = Some(assets);

        Ok(report)
    }

    /// One simulation tick (`GameManager_ProcessTick`).
    ///
    /// It also repeats the UI hook `UnitInfo::MoveFinished` → `AutoSelectNext`
    /// (`auto_select`) for the network loop: selection is local UI state,
    /// so the setting works in a lockstep match too.
    pub fn tick(&mut self) -> TickReport {
        let before = self.selected_order_snapshot();
        // Stage A: the tick goes through the facade (the single time-advance point).
        self.universe.tick();
        // Combat sounds of this tick (`UnitInfo::PrepareFire`/`StartExplosion`):
        // the facade moved them from the state into the event stream.
        self.collect_facade_sounds();
        self.auto_select_after_tick(before);
        self.ticks += 1;

        TickReport {
            tick: self.ticks,
            turn_counter: self.state_ref().turn_counter(),
            game_state: self.state_ref().game_state(),
            units: self.state_ref().unit_count(),
        }
    }

    /// Resets the visual state (positions, targets and unit routes).
    ///
    /// Called when loading a save and starting a new match: otherwise the frame could
    /// animate the "move" of units from the old match into the new one.
    fn reset_visual_state(&mut self) {
        self.visual_units.clear();
        self.visual_goals.clear();
        self.visual_paths.clear();
        self.frame_anims.clear();
        self.growth_anims.clear();
        // The site selection mode belongs to the previous match: the ghost must not
        // move into the new one.
        self.build_site = None;
    }

    /// Visual unit position in cells (fractional) for the frame.
    ///
    /// If [`App::update_visual_units`] has not been called yet,
    /// the simulation cell is returned — headless frames and tests see exactly `grid`.
    pub fn visual_position(&self, id: UnitId) -> Option<(f32, f32)> {
        let unit = self.state_ref().unit(id)?;
        Some(
            self.visual_units
                .get(&id)
                .copied()
                .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y))),
        )
    }

    /// Body angle for the frame: the direction of the current visual segment.
    ///
    /// While the sprite walks the route, this is the direction of the segment it is
    /// on (the `angle` field of the queue step); when the queue is empty (the unit stands
    /// or the frame caught up with the simulation) — the current `UnitInfo::angle`. This way the frame
    /// turns the unit exactly at the turn cell and does not run ahead of
    /// the simulation.
    ///
    /// The value is for drawing only: not part of the snapshot/CRC and does not
    /// change the simulation.
    pub fn visual_angle(&self, id: UnitId) -> Option<u8> {
        let unit = self.state_ref().unit(id)?;

        Some(
            match self.visual_paths.get(&id).and_then(|queue| queue.front()) {
                Some(step) => step.angle,
                None => unit.angle,
            },
        )
    }

    /// Builder working cycle frame number for drawing (`0..3`).
    ///
    /// `0` — frame `+16` (`DrawSpriteFrame(image_index + 16)` in C++
    /// `StartBuilding`); then `+24`, `+32` — the `UnitsManager_ProcessOrderBuild` cycle
    /// (`+8`, and if `image_index + 8 >= 40` — `-16`). The value is advanced by
    /// [`App::update_unit_animations`] by real `dt`; `dt = 0` leaves
    /// `0`, so QA frames are deterministic.
    pub fn build_animation_step(&self, id: UnitId) -> u8 {
        self.frame_anims
            .get(&id)
            .map(|anim| anim.builder_step)
            .unwrap_or(0)
    }

    /// Unit frame clock for drawing ([`render::FrameAnimation`]).
    ///
    /// Contains the builder cycle and the `ProcessOrderSentry` tick
    /// (ANIMATED buildings). There are no factory/mine frames: C++ does not
    /// advance them by time — buildings have two state frames, a powered-on factory blinks
    /// with palette cycles. The value is advanced by [`App::update_unit_animations`]
    /// by real `dt`; `dt = 0` — all counters zero, i.e. the static
    /// C++ frame (the goldens do not drift).
    pub fn unit_animation(&self, id: UnitId) -> crate::render::FrameAnimation {
        self.frame_anims
            .get(&id)
            .map(|anim| crate::render::FrameAnimation {
                builder_step: anim.builder_step,
                sentry_step: anim.sentry_step,
            })
            .unwrap_or_default()
    }

    /// Snapshot of the system palette with palette cycles (`None` — no changes).
    ///
    /// `Some` only after the [`App::color_cycles`] clock has stepped at least
    /// once (i.e. at `dt > 0`). Unit drawing takes this snapshot, otherwise —
    /// `Assets::system_palette`. The palette is frame-only: not part of the snapshot/CRC.
    pub fn animated_palette(&self) -> Option<&max_render::palette::Palette> {
        self.cycle_palette.as_ref()
    }

    /// Palette cycle clock (for tests/diagnostics).
    pub fn color_cycles(&self) -> &max_render::palette::ColorCycles {
        &self.color_cycles
    }

    /// Advances the palette cycles by real `dt` and updates the palette snapshot.
    ///
    /// A port of `World::UpdateColorAnimations` (`world.cpp:190`): each cycle
    /// rotates its slot range when `time_limit` ms have accumulated. `dt <= 0`
    /// (QA/headless) resets the clock — the palette is original again, golden frames
    /// are deterministic.
    fn update_color_cycles(&mut self, dt: f32) {
        // Clamp to 0.5 s: a real frame is ~16 ms, while a big jump (window pause)
        // must not rotate the palette by hundreds of steps.
        let dt = dt.clamp(0.0, 0.5);

        if !(dt > 0.0) {
            self.color_cycles.reset();
            self.cycle_palette = None;
            return;
        }

        if !self.color_cycles.advance(dt) {
            return;
        }

        let palette = self.assets.as_ref().map(|assets| {
            max_render::palette::rotate_palette(&assets.system_palette, &self.color_cycles)
        });

        self.cycle_palette = palette;
    }

    /// Visual unit scale (`1.0` — full size).
    /// `2^-scaler_adjust`: `1/16 … 1` as `UnitsManager_ProcessOrderAwaitScaling` runs.
    /// In the interactive mode the transition is smoothed by real `dt`
    /// ([`GROWTH_ANIM_SECONDS`]); `dt = 0` and units without `scaler_adjust` — `1.0`.
    pub fn unit_growth_scale(&self, id: UnitId) -> f32 {
        self.growth_anims.get(&id).map_or(1.0, |anim| {
            let progress = (anim.elapsed / GROWTH_ANIM_SECONDS).clamp(0.0, 1.0);
            anim.start + (1.0 - anim.start) * progress
        })
    }

    /// Camera scale for drawing a unit taking the "growth" into account.
    ///
    /// `UnitInfo::RenderSprite` does not change `Gfx_ZoomLevel`: it divides the frame
    /// size by `2^scaler_adjust`. In the port the same is expressed by a reduced
    /// sprite `zoom` (1:1 — `64`), the shadow/turret are scaled by the same code.
    pub fn unit_render_zoom(&self, id: UnitId, zoom: u32) -> u32 {
        let scale = self.unit_growth_scale(id);
        if scale >= 1.0 {
            return zoom.max(1);
        }
        ((zoom as f32) * scale).round().max(1.0) as u32
    }

    /// Visual frame animation clock: builder, `ANIMATED` buildings,
    /// the intermediate cycle of working factories and "growth".
    ///
    /// Like [`App::update_visual_units`], this is **frame only**: `frame_anims`/
    /// `growth_anims` are not part of the snapshot/CRC and do not affect the simulation.
    /// `dt <= 0` (QA/headless) resets them: builder — pose `+16`, building —
    /// the static C++ frame, any "growing" construction — full size,
    /// so the golden frames are deterministic.
    /// Construction marker blink clock (`DrawMap_BuildMMarkDelayCounter`,
    /// `drawmap.cpp:47`): the `BLDMRK1..5` frame changes every three map
    /// redraws. The port counts by visual time: `dt = 0` (QA/headless) —
    /// frame 0, deterministically.
    fn update_build_marker(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);

        if !(dt > 0.0) {
            self.build_marker_step = 0;
            self.build_marker_elapsed = 0.0;
            return;
        }

        self.build_marker_elapsed += dt;

        while self.build_marker_elapsed >= BUILD_MARKER_FRAME_SECONDS {
            self.build_marker_elapsed -= BUILD_MARKER_FRAME_SECONDS;
            self.build_marker_step = (self.build_marker_step + 1) % BUILD_MARKER_FRAMES as u8;
        }
    }

    /// Selection indicator color (`GameManager_MarkerColor`,
    /// `game_manager.cpp:1189-1192`): `0xFF ^= 0xFF` every
    /// [`render::UNIT_SELECTOR_BLINK_SECONDS`] = 210 ms (`TIMER_FPS_TO_MS(24) * 5`).
    /// `dt = 0` (QA/headless) returns `0xFF` — the goldens are deterministic.
    fn update_marker_color(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);

        if !(dt > 0.0) {
            self.marker_color_index = 0xFF;
            self.marker_color_elapsed = 0.0;
            return;
        }

        self.marker_color_elapsed += dt;

        while self.marker_color_elapsed >= crate::render::UNIT_SELECTOR_BLINK_SECONDS {
            self.marker_color_elapsed -= crate::render::UNIT_SELECTOR_BLINK_SECONDS;
            self.marker_color_index ^= 0xFF;
        }
    }

    fn update_unit_animations(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);

        if !(dt > 0.0) {
            self.frame_anims.clear();
            self.growth_anims.clear();
            return;
        }

        let ids = self.state_ref().unit_ids_in_order();
        // C++ `settings["effects"]` (`settings.cpp:128`): with effects
        // disabled `UnitsManager_ProcessOrderBuild`/`ProcessOrderSentry`
        // (`units_manager.cpp:2857/3030`) do not advance frames, so our
        // frame clock does not run either — the frame stays static.
        let effects = self.preferences.effects;
        let state = self.universe.snapshot();

        self.frame_anims.retain(|id, _| state.unit(*id).is_some());

        for id in &ids {
            let Some(unit) = state.unit(*id) else {
                continue;
            };

            let builder = crate::render::builder_animation_active(unit);
            let sentry = effects && crate::render::sentry_animation_active(unit);

            if !(builder || sentry) {
                self.frame_anims.remove(id);
                continue;
            }

            let anim = self.frame_anims.entry(*id).or_default();

            if builder {
                anim.builder_elapsed += dt;

                while anim.builder_elapsed >= BUILDER_ANIM_FRAME_SECONDS {
                    anim.builder_elapsed -= BUILDER_ANIM_FRAME_SECONDS;
                    anim.builder_step =
                        (anim.builder_step + 1) % crate::render::BUILDER_WORK_FRAME_STEPS;
                }
            } else {
                anim.builder_elapsed = 0.0;
                anim.builder_step = 0;
            }

            if sentry {
                anim.sentry_elapsed += dt;

                while anim.sentry_elapsed >= SENTRY_ANIM_FRAME_SECONDS {
                    anim.sentry_elapsed -= SENTRY_ANIM_FRAME_SECONDS;
                    anim.sentry_step = anim.sentry_step.wrapping_add(1);
                }
            } else {
                anim.sentry_elapsed = 0.0;
                anim.sentry_step = 0;
            }
        }

        // "Growth": start from the simulation `scaler_adjust` (C++ 2^-n).
        // A field borrow of the facade keeps `growth_anims` disjoint.
        let universe = &self.universe;
        for id in &ids {
            let Some(unit) = universe.snapshot().unit(*id) else {
                continue;
            };

            if unit.scaler_adjust == 0 {
                continue;
            }

            self.growth_anims.entry(*id).or_insert(GrowthAnim {
                start: crate::render::scaler_scale(unit.scaler_adjust),
                elapsed: 0.0,
            });
        }

        for anim in self.growth_anims.values_mut() {
            anim.elapsed = (anim.elapsed + dt).min(GROWTH_ANIM_SECONDS);
        }

        // Ready animations live until the unit is removed: otherwise a unit with a not yet
        // reset `scaler_adjust` (in QA the simulation does not tick between frames)
        // would start "growing" anew on every frame.
        self.growth_anims
            .retain(|id, _| universe.snapshot().unit(*id).is_some());
    }

    /// Pulls the visual positions toward the simulation ones.
    ///
    /// The simulation moves a unit a whole cell per tick; the frame passes **the same
    /// sequence of cells** at the real original speed
    /// ([`VISUAL_CELLS_PER_SECOND`]). This is the visible "cell by cell"
    /// movement: with `dt > 0` the sprite position changes between frames, and after
    /// passing the route it coincides with the target cell.
    ///
    /// `dt <= 0` — snapshot without interpolation: headless/QA frames remain
    /// deterministic and match the references. The same rule applies to the
    /// visual animation clock: the builder shows the working pose `+16`, and
    /// a "growing" unit — full size.
    ///
    /// P4: the map is updated in place (`retain` + `insert`), not rebuilt
    /// every frame. This removes the `HashMap` allocation of `ids.len()`
    /// cells per 60 Hz frame; the values/traversal order are not used
    /// by the simulation or the CRC (visual positions are frame-only).
    pub fn update_visual_units(&mut self, dt: f32) {
        // The portrait goes by the same frames as the map: `dt = 0` (QA) does not move it.
        self.update_portrait(dt);
        // The builder frame-by-frame animation and "growth" — also by real dt.
        self.update_unit_animations(dt);
        // Construction marker blinking (`DrawMap_RenderBuildMarker`).
        self.update_build_marker(dt);
        // Selection indicator blinking (`GameManager_MarkerColor`).
        self.update_marker_color(dt);
        // Palette cycles (building beacons, drill indicator) — frame only too.
        self.update_color_cycles(dt);
        // `SLFDOPN1..6` frames of the self-destruct dialog (`TIMER_FPS_TO_MS(48)`).
        self.advance_self_destruct(dt);

        let ids = self.state_ref().unit_ids_in_order();
        let dt = dt.clamp(0.0, 0.1);
        // `fast_movement` ("Double Unit Steps"): C++ doubles the
        // movement speed (`UnitInfo::Move`: `velocity += FastMovement ? 2 : 1`,
        // `unitinfo.cpp:1742`; `Paths_CalculateStep`: `max_velocity` 16 instead of
        // 8, `paths.cpp:297`), so the sprite passes a cell twice as fast.
        // The port simulation moves the unit a cell per tick; the frame speed —
        // the only place where the setting is visible, exactly as the visual effect
        // of the original.
        let speed_scale = if self.preferences.fast_movement {
            2.0
        } else {
            1.0
        };

        // Separate field borrows: `retain` reads the state.
        let state = self.universe.snapshot();
        let visual = &mut self.visual_units;
        let goals = &mut self.visual_goals;
        let paths = &mut self.visual_paths;

        visual.retain(|id, _| state.unit(*id).is_some());
        goals.retain(|id, _| state.unit(*id).is_some());
        paths.retain(|id, _| state.unit(*id).is_some());

        // `dt = 0` (headless/QA): the snapshot — the frame reads exactly the simulation
        // cell. The route queue is reset, so the goldens do not drift.
        if dt <= 0.0 {
            for id in ids {
                let Some(unit) = state.unit(id) else {
                    continue;
                };
                let target = (f32::from(unit.grid.x), f32::from(unit.grid.y));
                visual.insert(id, target);
                goals.insert(id, (unit.grid.x, unit.grid.y));
                paths.remove(&id);
            }
            return;
        }

        let budget = VISUAL_CELLS_PER_SECOND * speed_scale * dt;

        for id in ids {
            let Some(unit) = state.unit(id) else {
                continue;
            };
            let grid = (unit.grid.x, unit.grid.y);
            let target = (f32::from(grid.0), f32::from(grid.1));
            let current = visual.entry(id).or_insert(target);
            let goal = goals.entry(id).or_insert(grid);
            let queue = paths.entry(id).or_default();

            // The simulation moved the unit (usually one cell per tick).
            // We append the actual route cells so that the frame walks them,
            // not cutting the path in a straight line; a big jump — a teleport/sync.
            if *goal != grid {
                let dx = i32::from(grid.0) - i32::from(goal.0);
                let dy = i32::from(grid.1) - i32::from(goal.1);
                let distance = dx.abs().max(dy.abs());

                if distance > VISUAL_TELEPORT_CELLS {
                    queue.clear();
                    *current = target;
                } else {
                    let divisor = distance.max(1);
                    let mut previous = *goal;

                    for step in 1..=divisor {
                        let x = goal.0 + (dx * step / divisor) as i16;
                        let y = goal.1 + (dy * step / divisor) as i16;
                        let step_dx = i32::from(x) - i32::from(previous.0);
                        let step_dy = i32::from(y) - i32::from(previous.1);
                        // The segment direction — from the actual cell displacement,
                        // not from the current `unit.angle`: by the frame moment
                        // the simulation may have already turned toward the next step
                        // (`GroundPath::UpdateUnitAngle`) or gone further.
                        let angle = if step_dx == 0 && step_dy == 0 {
                            unit.angle
                        } else {
                            max_core::unit_order::paths_get_angle(step_dx, step_dy) as u8
                        };

                        queue.push_back(VisualStep {
                            x: f32::from(x),
                            y: f32::from(y),
                            angle,
                        });
                        previous = (x, y);
                    }
                }

                *goal = grid;
            }

            // We pass no more than `budget` cells per frame; the remaining budget
            // is carried over to the next route points — the speed is constant.
            let mut left = budget;

            while left > 0.0 {
                let Some(&waypoint) = queue.front() else {
                    break;
                };
                let dx = waypoint.x - current.0;
                let dy = waypoint.y - current.1;
                let distance = dx.abs().max(dy.abs());

                if distance <= left {
                    *current = (waypoint.x, waypoint.y);
                    queue.pop_front();
                    left -= distance;
                } else {
                    let factor = left / distance;
                    current.0 += dx * factor;
                    current.1 += dy * factor;
                    left = 0.0;
                }
            }
        }
    }

    /// Canonical state digest.
    pub fn digest(&self) -> AppDigest {
        AppDigest::capture(self.state_ref(), self.ticks)
    }

    /// Canonical state bytes ([`GameState::snapshot_bytes`]).
    ///
    /// The same snapshot whose CRC is part of [`AppDigest::state_digest`];
    /// convenient to save to a file and compare two runs byte by byte.
    pub fn snapshot(&self) -> Vec<u8> {
        self.state_ref().snapshot_bytes()
    }

    /// Centers the camera on the first significant unit of the local player.
    ///
    /// This is `GameManager_GetFirstRelevantUnit` (`game_manager.cpp:3140`), not
    /// "the first unit in the list": C++ looks for a **mine** (`Access_GetFirstMiningStation`),
    /// then the first active ground/sea, air and, finally,
    /// stationary unit. Earlier the camera stood on the first unit of the canonical
    /// traversal — which is `GROUND_COVER` (`SMLSLAB`/`LRGSLAB`), one cell
    /// down-left of the base. Because of the shift, clicks computed from the base hit
    /// the neighboring cell with the stationary mine: it was selected, RMB answered
    /// «Unit cannot move (air/stationary)», and the unit did not move.
    pub fn focus_player(&mut self) {
        let team = self.state_ref().player_team();
        let mut grid = None;

        // 1. `Access_GetFirstMiningStation`: a team mine in `StationaryUnits`.
        for id in self
            .state_ref()
            .unit_list(max_core::unit::UnitListKind::Stationary)
        {
            if let Some(unit) = self.state_ref().unit(*id) {
                if unit.team == team && unit.unit_type == UnitType::MiningStation {
                    grid = Some(unit.grid);
                    break;
                }
            }
        }

        // 2. `Access_GetFirstActiveUnit` over three lists in C++ order.
        if grid.is_none() {
            for kind in [
                max_core::unit::UnitListKind::MobileLandSea,
                max_core::unit::UnitListKind::MobileAir,
                max_core::unit::UnitListKind::Stationary,
            ] {
                for id in self.state_ref().unit_list(kind) {
                    if let Some(unit) = self.state_ref().unit(*id) {
                        if unit.team == team && unit.orders != max_core::enums::UnitOrder::Idle {
                            grid = Some(unit.grid);
                            break;
                        }
                    }
                }

                if grid.is_some() {
                    break;
                }
            }
        }

        if let Some(grid) = grid {
            self.viewport.center_on_grid(grid.x, grid.y);
        }
    }
}

// Local (single-player) surface: constructors and loading on `LocalDispatcher`.
impl App<LocalDispatcher> {
    /// Empty application (no assets; the state — an empty `GameState::new` match).
    pub fn new(config: AppConfig) -> Self {
        Self::with_universe(config, GameUniverse::default)
    }

    /// Loads the save from [`AppConfig::save`].
    ///
    /// # Errors
    ///
    /// The save is not set or cannot be read/parsed as V70.
    pub fn load_game(&mut self) -> Result<()> {
        let save = self
            .config
            .save
            .clone()
            .context("в AppConfig не задан путь к сейву (--demo <save>)")?;

        self.load_save(save)
    }

    /// Loads a V70 save by path.
    ///
    /// Then the state is normalized ([`GameState::from_save`]) and ready for
    /// [`App::tick`]; the tick/command counters are reset.
    pub fn load_save(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .with_context(|| format!("не удалось прочитать сейв {}", path.display()))?;

        self.load_save_bytes(&bytes)
            .with_context(|| format!("не удалось разобрать сейв {}", path.display()))
    }

    /// Loads a V70 save from bytes (tests and embedded fixtures).
    pub fn load_save_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let save = SaveFile::parse(bytes).map_err(|error| anyhow::anyhow!("{error}"))?;

        // Stage A: the facade owns the state, so a load replaces the universe
        // (`GameUniverse::from_save` counts the load as a state version).
        self.universe = max_core::universe::GameUniverse::from_save(&save);
        self.universe_events = self.universe.subscribe();
        self.turn_events = self.universe.subscribe();
        // `SaveLoad_Load` leaves the player `settings` as is: `enemy_halt`
        // is taken from `Preferences`, not from the file (`saveload.cpp:1175` writes only
        // the timers into the settings).
        let enemy_halt = self.preferences.enemy_halt;
        self.universe.set_enemy_halt(enemy_halt);
        self.ticks = 0;
        self.applied_commands = 0;
        self.hud.clear();
        self.local = None;
        self.build_menu = None;
        self.save_load = None;
        self.upgrade_menu = None;
        self.research_menu = None;
        self.unit_menu = None;
        self.self_destruct = None;
        self.alloc_menu = None;
        self.transfer_menu = None;
        self.help = None;
        self.help_mode = None;
        self.reset_visual_state();
        self.focus_player();
        // Starting snapshot: the first tick with `dt > 0` will see from which cell
        // the unit started moving, otherwise the first route step would "teleport".
        self.update_visual_units(0.0);

        Ok(())
    }

    /// Creates a local match from [`NewGameConfig`] on the planet map from the assets.
    ///
    /// If the assets are not loaded yet or were loaded for another planet, they
    /// are re-read from [`AppConfig`] with `planet = config.planet`: the map
    /// (`.WRL`) is part of the game data, not of the match state.
    pub fn new_game(&mut self, config: &NewGameConfig) -> Result<NewGameReport> {
        let surface = self.ensure_surface(config.planet)?;
        Ok(self.new_game_on_surface(config, &surface))
    }

    /// Creates a local match on a ready surface map.
    ///
    /// The world itself is built by the shared generator ([`build_world_into`],
    /// stage C A3): only the dispatcher differs between the local and the network
    /// constructor. The world is written in place, so the 75 KB state is not moved.
    pub fn new_game_on_surface(
        &mut self,
        config: &NewGameConfig,
        surface: &SurfaceMap,
    ) -> NewGameReport {
        self.universe = GameUniverse::with_dispatcher(LocalDispatcher);
        let report = build_world_into(config, surface, &mut self.universe);
        // `GameManager_InitGame`: the live `enemy_halt` setting. Generation has no
        // armed unit in range (`Access_UpdateMapStatus` reads the flag per unit
        // step), so applying it right after the world is built is the same world.
        self.universe.set_enemy_halt(self.preferences.enemy_halt);
        self.reset_after_world_install();
        report
    }

    /// Headless demo: load a save, run `turns` ticks, return the digest.
    ///
    /// This is exactly the `--demo` scenario, so the test and the CLI check the same code.
    pub fn run_demo(&mut self, save: impl AsRef<Path>, turns: u32) -> Result<AppDigest> {
        self.load_save(save)?;

        for _ in 0..turns {
            self.tick();
        }

        Ok(self.digest())
    }
}

/// The common world generator of the local and the network constructors (stage C A3).
///
/// The body is the former `App::new_game_on_surface` without the application state:
/// only the dispatcher differs between `App<LocalDispatcher>` and
/// `App<NetworkDispatcher>`, so the canonical world bytes are identical
/// (`game-universe-stage-c.md` §4.1). The caller installs the universe through
/// `App::adopt_universe` and applies its own live settings.
///
/// A port of `GameManager_InitGame` to the extent of P2 + M3.2: `setup::generate_map`
/// (resources + alien debris), then `GameState::with_setup`, deployment of
/// alien debris (`UnitsManager_DeployUnit` with `team = PLAYER_TEAM_ALIEN`),
/// landing of the starting armies
/// ([`max_core::deploy::deploy_starting_units`] —
/// `GameManager_ProcessTeamMissionSupplyUnits`) and continuation of the RNG stream
/// (as in C++, where `Randomizer_SetSeed` stands at the start of the match generation).
///
/// # Starting armies
///
/// Each playing team gets a mine, a generator, two slabs and the `SC_L0001`
/// loadout (`CONSTRCT` 40, `ENGINEER` 20, `SURVEYOR` 0), gold —
/// [`NewGameConfig::start_gold`]. Without this the victory/defeat conditions
/// (`SC_L0002`) consider the team eliminated already on the first turn.
///
/// # Alien debris
///
/// The [`NewGameReport::aliens`] order is the `setup::generate_map` order
/// (slab → building → bridges → units), in which C++
/// `GameManager_SpawnAlienDerelicts` calls `UnitsManager_DeployUnit`.
/// The units are created in `GameState` on behalf of `TeamId::ALIEN`; heat maps
/// and team statistics/victory conditions do not see them (as in C++:
/// `PLAYER_TEAM_ALIEN` has `team_type == TEAM_TYPE_NONE` and `heat_map == null`).
pub(crate) fn build_world<D: Dispatcher>(
    config: &NewGameConfig,
    surface: &SurfaceMap,
    dispatcher: D,
) -> (GameUniverse<D>, NewGameReport) {
    let mut universe = GameUniverse::with_dispatcher(dispatcher);
    let report = build_world_into(config, surface, &mut universe);
    (universe, report)
}

/// The same generator, writing the generated world **in place** (stage C §4.1).
///
/// `build_world` keeps the documented `(GameUniverse<D>, NewGameReport)` shape; the
/// two constructors use this core instead, because the state is ~75 KB and a
/// by-value return costs a full copy of it on the debug-build stack (the local
/// `new_game` path is deep: `start_local_game` → `new_game` → `new_game_on_surface`).
/// The resulting state is exactly [`GameUniverse::with_setup_and_dispatcher`] +
/// the deployment tail, so the bytes are identical.
pub(crate) fn build_world_into<D: Dispatcher>(
    config: &NewGameConfig,
    surface: &SurfaceMap,
    out: &mut GameUniverse<D>,
) -> NewGameReport {
    let settings = GenerationSettings::from_menu_levels(
        config.raw,
        config.fuel,
        config.gold,
        config.derelicts,
    );
    let alien_costs = AlienUnitCosts {
        alien_tank: base_unit_values(UnitType::AlienTank).get_attribute(ATTRIB_TURNS),
        juggernaut: base_unit_values(UnitType::Juggernaut).get_attribute(ATTRIB_TURNS),
        alien_assault_gun: base_unit_values(UnitType::AlienAssaultGun).get_attribute(ATTRIB_TURNS),
        alien_plane: base_unit_values(UnitType::AlienPlane).get_attribute(ATTRIB_TURNS),
    };
    let mut setup = GameSetup {
        planet: config.planet,
        settings,
        teams: config.teams.clone(),
        alien_costs,
    };
    let mut rng = Randomizer::new(config.seed);

    // The start phases (`CargoMenu` + site select) have already run: humans chose
    // the points themselves, the computers choose them like `Ai_SelectStartingPosition`
    // (`GameManager_InitGame`: a loop over computer teams after the humans, but
    // before `GameManager_PopulateMapWithResources`).
    //
    // C++ re-seeds the RNG in `GameManager_PopulateMapWithResources`, so the
    // AI choice goes in a separate stream from the same seed, while the map generation —
    // already from a clean seed (as before).
    let mut deployments = config.deployments.clone();

    if !deployments.is_empty() {
        let mut choice_rng = Randomizer::new(config.seed);
        let mut taken: Vec<Point> = deployments.iter().map(|entry| entry.position).collect();

        for start in setup.teams.iter_mut() {
            if deployments.iter().any(|entry| entry.team == start.team) {
                continue;
            }

            let position =
                max_core::setup::ai_choose_starting_position(surface, &mut choice_rng, &taken);

            start.position = position;
            taken.push(position);
            deployments.push(TeamDeployment::default_loadout(
                start.team,
                position,
                config.start_gold,
            ));
        }
    }

    let output = generate_map(&mut setup, surface, &mut rng);
    // Resource map snapshot **right after generation**
    // (`GameManager_PopulateMapWithResources`, `game_manager.cpp:4730`):
    // `NewGameReport::resources_crc` is checked against the P2 generation vector and must
    // not contain the team survey accumulated by the landed
    // scouts (`Survey_SurveyArea` → `CargoMap |= hash_team_id`).
    let generated_resources_crc = crc::crc16(&output.map.resources.to_le_bytes());

    // `GameManager_PopulateMapWithResources` (`game_manager.cpp:4730`) before
    // landing moves `TeamMissionSupplies[team].starting_position` of
    // **each** playing team: `FindValidStartingPosition`, then
    // `Optimize` of materials/fuel. Therefore `ProcessTeamMissionSupplyUnits`
    // (and `GetStartPositionMiningStation`) sees the final point — for
    // the AI and for a human who chose the point in the site select phase.
    //
    // Previously only the points added here for the AI were synchronized;
    // the interactive path (`start_positions` from the landing phase) went into
    // `deploy_starting_units_with` with its own original value. While the point
    // is valid (checked by `LandingScreen`), the result matches, but with
    // any future change of `Optimize` the layout would diverge from C++.
    for deployment in deployments.iter_mut() {
        if let Some(start) = setup
            .teams
            .iter()
            .find(|start| start.team == deployment.team)
        {
            deployment.position = start.position;
        }
    }

    // Teams from setup (positions could have been moved by `FindValidStartingPosition`).
    let mut team_kinds = [TeamKind::None; PLAYER_TEAM_COUNT];
    for start in &setup.teams {
        let index = start.team.0 as usize;
        if index < PLAYER_TEAM_COUNT {
            team_kinds[index] = if start.team == config.player_team {
                TeamKind::Player
            } else {
                TeamKind::Computer
            };
        }
    }

    // Stage B §7: the generated world enters the facade through its config
    // layer — `GameState` fields are crate-private now. Stage C A3: the world
    // generator is generic over the dispatcher (`build_world`), so the local and
    // the network constructor produce the same bytes.
    // A3: the generated world is written into the universe in place (the
    // dispatcher is kept; the facade counters start over).
    out.reset_world(output.map, team_kinds);
    let state = out;
    state.set_player_team(config.player_team);
    // Clans from the settings — before deploying units: `deploy_starting_units`
    // takes the attributes from `team_units` (`ResourceManager_InitClanUnitValues`).
    // The call rebuilds base and current values from `SC_ATTRI` and clears the
    // team complexes, exactly like the C++ initialization; the derelict slot
    // (`PLAYER_TEAM_ALIEN`) was already set by `GameState::with_setup`.
    for index in 0..PLAYER_TEAM_COUNT {
        let clan = config.clans.get(index).copied().unwrap_or(TeamClan::Random);
        let team = TeamId(index as u8);
        state.team_mut(team).expect("team slot").team_clan = clan;
        state
            .team_units_mut(team)
            .expect("team economy")
            .init_clan_unit_values(clan);
    }

    // `CargoMenu::CommitUpgradeChanges`: upgrades bought before landing
    // (`unitvalues_actual`) replace the team's current values before
    // `GameManager_ProcessTeamMissionSupplyUnits`, so the landed units
    // get the already improved attributes.
    for deployment in &deployments {
        let index = deployment.team.0 as usize;

        if index >= PLAYER_TEAM_COUNT {
            continue;
        }

        for (unit_type, values) in &deployment.upgrades {
            let slot = *unit_type as usize;
            if slot < state.snapshot().team_units()[index].current_values.len() {
                state
                    .team_units_mut(deployment.team)
                    .expect("team economy")
                    .current_values[slot] = *values;
            }
        }
        state
            .team_mut(deployment.team)
            .expect("team slot")
            .stats_gold_spent_on_upgrades = deployment.gold_spent_on_upgrades;
    }
    // The RNG stream continues after generation — as in the original.
    *state.rng_mut() = rng;

    // `GameManager_SpawnAlienDerelicts`: all placements — `DeployUnit` with
    // `team = PLAYER_TEAM_ALIEN`, in generation order. C++ passes
    // `skip_map_status_update = true`; in max_core `update_map_status`
    // ignores `ALIEN` itself (the team has no heat map), so
    // the result is the same — the team layers are not touched.
    for placement in &output.aliens {
        let _ = state.spawn_unit(placement.unit_type, TeamId::ALIEN, placement.position, 0);
    }

    // `GameManager_ProcessTeamMissionSupplyUnits` for each team
    // (`team_type != NONE`), in ascending team index order and already
    // after the alien debris — exactly as in `GameManager_InitGame`. Without this
    // `menu_check_end_game_conditions` ends the match right after the first
    // turn: the teams have neither infrastructure nor troops.
    //
    // If the start phases have already run (`Mobilization` + site select), each
    // team has its own landing point, credits and loadout
    // (`TeamMissionSupplies` in C++).
    let start_units = if deployments.is_empty() {
        state.deploy_starting_units(
            config.start_gold,
            setup.settings.max_resources,
            &setup.teams,
        )
    } else {
        state.deploy_starting_units_with(setup.settings.max_resources, &deployments)
    };

    // `Access_UpdateVisibilityStatus` after landing (`game_manager.cpp:1965`):
    // resets unit visibility by `InitStealthStatus` and rebuilds
    // the scanning coverage/heat maps. Without this the landing order
    // left "one-way" visibility (whoever landed later sees
    // those landed earlier, but not vice versa), and `visible_to_team` from spawn
    // made foreign bases visible to everyone without survey.
    state.update_visibility_status(true);

    let report = NewGameReport {
        planet: config.planet,
        seed: config.seed,
        teams: setup.teams.len(),
        aliens: output.aliens.len(),
        start_units,
        units: state.snapshot().unit_count(),
        surface_crc: crc::crc16(&state.snapshot().map().surface.to_bytes()),
        resources_crc: generated_resources_crc,
    };

    tracing::info!(
            "new game: planet={:?} seed={} teams={} aliens={} start_units={} units={} surface_crc=0x{:04X}",
            report.planet,
            report.seed,
            report.teams,
            report.aliens,
            report.start_units,
            report.units,
            report.surface_crc
        );

    // Stage A: the generated state becomes the new facade-owned state; the
    // caller installs it (`App::adopt_universe`), which also re-subscribes the
    // event cursors because the buffer starts empty.
    report
}

// Network surface: constructing the client after the lobby (stage C §4.2).
impl App<NetworkDispatcher> {
    /// A network client application for `local_team` in `play_mode` (stage C §4.2).
    ///
    /// The universe is still empty: [`App::initialize_game`] replaces it with the
    /// world built from the lobby settings on a [`NetworkDispatcher`] (A3/A4).
    /// Switching the mode is creating another `App<D>`, never mutating an existing
    /// one (ADR §7.8): the local and the network surfaces are different types.
    pub fn new_network(
        config: AppConfig,
        local_team: TeamId,
        play_mode: max_core::enums::PlayMode,
    ) -> Self {
        let mut app = Self::with_universe(config, || {
            GameUniverse::with_dispatcher(NetworkDispatcher::new(local_team, play_mode))
        });
        app.universe.set_player_team(local_team);
        app.universe.set_play_mode(play_mode);
        app
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_points_at_the_repository_layout() {
        let config = AppConfig::default();

        assert_eq!(config.mode, AppMode::Demo);
        assert_eq!(config.demo_turns, 5);
        assert_eq!(config.horizon_port, 8082);
        assert_eq!(config.max_res, PathBuf::from("max-original/MAX.RES"));
        assert_eq!(config.save, None);
    }

    #[test]
    fn p2_reference_matches_the_vector_case() {
        let config = NewGameConfig::p2_reference();

        assert_eq!(config.planet, Planet::Snowcrab);
        assert_eq!(config.seed, 0);
        assert_eq!(config.raw.value(), 1);
        assert_eq!(config.fuel.value(), 1);
        assert_eq!(config.gold.value(), 1);
        assert_eq!(config.derelicts.value(), 0);
        assert_eq!(
            config.teams,
            vec![
                TeamStart::new(TeamId::RED, Point::new(20, 20)),
                TeamStart::new(TeamId::BLUE, Point::new(70, 70)),
            ]
        );
    }

    #[test]
    fn digest_line_has_the_documented_fields() {
        let digest = AppDigest::capture(GameUniverse::default().snapshot(), 0);
        let line = digest.to_line();

        assert!(line.contains("ticks=0"));
        assert!(line.contains("state_digest=0x"));
        assert!(line.contains("replay_crc=0x"));
        assert_eq!(
            digest,
            AppDigest::capture(GameUniverse::default().snapshot(), 0)
        );
    }

    /// Unique temporary test directory (without external crates).
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "max_app_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn handshake_patches_default_to_the_assets_path() {
        let config = AppConfig::default();

        assert_eq!(config.handshake_patches, None);
        assert_eq!(
            config.handshake_patches_path(),
            config.patches_res.as_path(),
            "None → хэш из assets-пути patches_res, как раньше"
        );
    }

    #[test]
    fn handshake_patches_override_only_the_hash_path() {
        let config = AppConfig::default().with_handshake_patches("/tmp/nonexistent.res");

        assert_eq!(
            config.handshake_patches_path(),
            Path::new("/tmp/nonexistent.res")
        );
        assert_eq!(
            config.patches_res,
            PathBuf::from("assets/PATCHES.RES"),
            "ассетный путь не меняется"
        );
    }

    #[test]
    fn data_hash_uses_handshake_patches_and_leaves_the_patches_part_empty_when_missing() {
        let dir = temp_dir("handshake_hash");
        let max_res = dir.join("MAX.RES");
        let patches = dir.join("PATCHES.RES");
        std::fs::write(&max_res, b"max-res-bytes").expect("write MAX.RES");
        std::fs::write(&patches, b"patches-bytes").expect("write PATCHES.RES");

        // None → the PATCHES part of the hash is computed by the assets path.
        let assets_only = AppConfig {
            max_res: max_res.clone(),
            patches_res: patches.clone(),
            ..Default::default()
        };
        let hash = max_net::DataHash::from_files(
            &assets_only.max_res,
            assets_only.handshake_patches_path(),
        )
        .expect("hash files");
        assert!(hash.max_res.is_some());
        assert!(
            hash.patches_res.is_some(),
            "None → хэш из assets-пути patches_res"
        );

        // Some(nonexistent path) → an empty PATCHES part, but the assets remain.
        let split = AppConfig {
            max_res,
            patches_res: patches,
            handshake_patches: Some(dir.join("nonexistent.res")),
            ..Default::default()
        };
        let hash = max_net::DataHash::from_files(&split.max_res, split.handshake_patches_path())
            .expect("hash files");
        assert!(hash.max_res.is_some());
        assert!(
            hash.patches_res.is_none(),
            "путь хендшейка не существует → хэш без PATCHES-части (семантика сервера)"
        );
        assert_eq!(
            split.handshake_patches_path(),
            dir.join("nonexistent.res").as_path()
        );

        std::fs::remove_dir_all(dir).ok();
    }
}
