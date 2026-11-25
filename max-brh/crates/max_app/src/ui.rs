//! Client UI framework: menu screens, transitions and drawing.
//!
//! The module is a **pure core** over [`max_render::ui`]:
//!
//! * [`Screen`] — screen state: main menu, game list, game creation form
//!   and the active session;
//! * [`InputEvent`] — what the SDL layer turns into a key/click;
//! * [`Screen::handle_input`] — the transition machine: `MainMenu → CreateForm`,
//!   `MainMenu → JoinList` (via `list_games`), `CreateForm → create_game →
//!   InSession`, `JoinList → InSession`;
//! * [`UiContext`] — screen size, mouse position, status line, the Horizon client
//!   behind the [`MenuClient`] trait and [`AudioPlayer`] for clicks (M6.2);
//! * [`Screen::render`] — the frame without SDL: `render_ui` for menus and the map
//!   (`App::render_frame` → `max_render::map_render`) + the status line for
//!   [`Screen::InSession`].
//!
//! # Testability without the network
//!
//! The network is hidden behind the [`MenuClient`] trait. Tests pass a mock client with
//! predefined [`GameInfo`] and check the transitions; the real client —
//! [`HorizonMenuClient`] — is a synchronous wrapper over the asynchronous
//! [`max_net::HorizonClient`] (each call brings up a short-lived
//! `tokio` runtime). Entering the game — [`Transition::StartSession`]: the network
//! loop execution remains in `main.rs` and reuses
//! [`crate::run_network`].
//!
//! # Keys
//!
//! | Screen | Controls |
//! |---|---|
//! | MainMenu | Up/Down/Tab, Enter, mouse; `c`/`j`/`i`/`r`/`q`; Esc — exit |
//! | JoinList | Up/Down, Enter — join, `r` — refresh, Esc — back, mouse |
//! | CreateForm | Tab/Up/Down — field, input, Left/Right — numbers, Enter — create, Esc — back |
//! | InSession | LMB — select a unit, RMB — move order, LMB on an enemy — attack, Enter/T — chat (packet 18), End Turn/Menu, Esc — cancel (twice — exit) |
//!
//! # Session input (M4.3)
//!
//! The menu lives in [`Screen::handle_input`], and the input inside a match — in
//! [`handle_session_input`]: it works with [`App`] (selection and orders in
//! [`crate::hud::HudState`], not in `GameState`), so selecting a unit does not
//! get into the snapshot/CRC. Orders (`38`/`08`) are assembled by
//! [`crate::hud::build_move_order`]/[`crate::hud::build_attack_order`] and put
//! into `App::hud().pending`; the session channel decides what happens next
//! (SDL/UI Stage C2, [`crate::local::SessionChannel`]): a local match applies them
//! through `apply_now` and ticks them in the same frame, a lockstep match leaves
//! them in the queue for `net::flush_hud_orders` → `submit`.

use std::fmt;
use std::sync::Arc;

use max_assets::MissionRegistry;
use max_core::enums::Planet;
use max_core::geo::{Point as GridPoint, MAP_HEIGHT, MAP_WIDTH};
use max_core::input::{Command, Packet18Chat};
use max_core::unit::UnitInfo;
use max_core::universe::{Dispatcher, LocalDispatcher};
use max_core::victory::MissionCategory;
use max_net::{CreateGameParams, GameInfo, HorizonClient, CLIENT_NAME, HORIZON_PROTOCOL_VERSION};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer, BYTES_PER_PIXEL};
use max_render::text::{draw_text_gnw, draw_text_line, GnwFontId, GLYPH_HEIGHT};
use max_render::ui::{
    hit_test, layout, render_ui, Button, Label, ListBox, Point, Rect, TextBox, UiState, UiTheme,
    Widget,
};
use max_render::viewport::Viewport;

use crate::audio::{AudioPlayer, MENU_CLICK};
use crate::credits;
use crate::gamesetupmenu::{self, MissionSelectAction, MissionSelectForm};
use crate::helpmenu::{self, HelpAction, HelpPlacement, HelpSection};
use crate::hud::{self, HudButton};
use crate::intro;
use crate::landing::LandingAction;
use crate::local::{LocalSetup, SessionLocal};
use crate::menu;
use crate::mission::MissionStartRequest;
use crate::mobilize::CargoAction;
use crate::newgame::{self, NewGameAction, TipsWindow};
use crate::panels::{self, PanelButton};
use crate::preferences::{self, Preferences, PreferencesDialog, PreferencesVariant};
use crate::pregame::{PreGameFlow, PreGameStage};
use crate::researchmenu::ResearchAction;
use crate::setup::{self, GameSetupForm, SetupTransition};
use crate::sfx::Sfx;
use crate::story::{self, StoryAction};
use crate::winloss;
use crate::App;

/// Default game name if the «Name» field is empty.
pub const DEFAULT_GAME_NAME: &str = "M.A.X. Rust game";
/// Last valid planet index (`0..=23`, like `--world`).
pub const MAX_WORLD: i64 = 23;

/// Widget levels/indices the mouse handling relies on.
///
/// The order as in `menu.cpp::main_menu`: `New Game`, `Load Game`,
/// `Multiplayer Game`, `Setup`, `Introduction`, `Credits`, `Exit`.
const MAIN_CREATE: usize = 0;
const MAIN_JOIN: usize = 2;
const MAIN_SETUP: usize = 3;
const MAIN_INTRO: usize = 4;
const MAIN_CREDITS: usize = 5;
const MAIN_QUIT: usize = 6;

/// Rust client actions in focus order (`ctx.focus`).
///
/// `Load Game` remains unported (`Unported`); `Setup` opens the
/// `OptionsMenu(SETUPPIC)` dialog, the other actions — as in C++.
const MAIN_ACTIONS: [usize; 6] = [
    MAIN_CREATE,
    MAIN_JOIN,
    MAIN_SETUP,
    MAIN_INTRO,
    MAIN_CREDITS,
    MAIN_QUIT,
];

const JOIN_LIST: usize = 1;
const JOIN_BACK: usize = 2;
const JOIN_REFRESH: usize = 3;
const JOIN_JOIN: usize = 4;
const JOIN_HIDE: usize = 5;
const JOIN_HELP: usize = 6;

/// Game creation form buttons (widget indices).
const CREATE_BACK: usize = 7;
const CREATE_SUBMIT: usize = 8;
const CREATE_RANDOM: usize = 9;
const CREATE_HELP: usize = 10;

/// UI key (the SDL layer translates `WindowKey` into it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Esc.
    Escape,
    /// Enter.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Page Up (list pages, like `GNW_KB_KEY_PAGEUP`).
    PageUp,
    /// Page Down (`GNW_KB_KEY_PAGEDOWN`).
    PageDown,
    /// Printable character.
    Char(char),
    /// Printable character with Alt held (`Alt+P` — pause, `GNW_KB_KEY_LALT_P`).
    Alt(char),
    /// Other key (ignored).
    Other,
}

/// UI input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// Key.
    Key(Key),
    /// Left click in window pixels.
    MouseClick(i32, i32),
    /// Right click in window pixels (RMB — move order/cancel).
    RightClick(i32, i32),
    /// Mouse move in window pixels.
    MouseMove(i32, i32),
}

/// Network session entry parameters — the result of the menu choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequest {
    /// `game_id` from Horizon.
    pub game_id: String,
    /// Game password, if it was set/known.
    pub password: Option<String>,
}

/// The transition the application must execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// The screen changed/the status updated — just redraw.
    None,
    /// Enter the game: `main.rs` launches the existing `run_network`.
    StartSession(SessionRequest),
    /// Start a local match (M-SP) with these settings.
    StartLocal(Box<LocalSetup>),
    /// Start a mission of a category through the stage-1 flow
    /// (`App::start_mission_from_registry`): the menu only chose the mission.
    StartMission(Box<MissionStartRequest>),
    /// Exit the session (Esc in the SDL loop).
    LeaveSession,
    /// The campaign story screen (intro/victory/defeat) was closed: the driver
    /// resumes the action it kept aside (enter the already started match, or leave
    /// the session after the game-over screen).
    StoryFinished,
    /// Apply the settings from the `Preferences`/`Setup` dialog (main menu).
    ApplyPreferences(Box<Preferences>),
    /// Close the application.
    Quit,
}

/// Game creation form field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateField {
    /// Game name.
    Name,
    /// Planet (`0..=23`).
    World,
    /// Seconds per turn.
    Timer,
    /// Seconds to finish the turn.
    EndTurn,
    /// Number of opponents (`1..=3`).
    Opponent,
    /// Password (empty — open game).
    Password,
}

impl CreateField {
    /// All fields in Tab order.
    pub const ALL: [Self; 6] = [
        Self::Name,
        Self::World,
        Self::Timer,
        Self::EndTurn,
        Self::Opponent,
        Self::Password,
    ];

    /// Field index in [`CreateField::ALL`].
    pub const fn index(self) -> usize {
        match self {
            Self::Name => 0,
            Self::World => 1,
            Self::Timer => 2,
            Self::EndTurn => 3,
            Self::Opponent => 4,
            Self::Password => 5,
        }
    }

    /// Next field (cyclic).
    pub const fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// Previous field (cyclic).
    pub const fn prev(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Field label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::World => "World",
            Self::Timer => "Turn timer",
            Self::EndTurn => "End turn",
            Self::Opponent => "Opponents",
            Self::Password => "Password",
        }
    }

    /// The field is numeric (edited with digits and arrows).
    pub const fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::World | Self::Timer | Self::EndTurn | Self::Opponent
        )
    }
}

/// Game creation form state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateForm {
    /// Game name.
    pub name: String,
    /// Planet.
    pub world: i64,
    /// Seconds per turn.
    pub timer: i64,
    /// Seconds to finish the turn.
    pub endturn: i64,
    /// Number of opponents.
    pub opponent: i64,
    /// Password.
    pub password: String,
    /// Focused field.
    pub focus: CreateField,
}

impl Default for CreateForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            world: 0,
            timer: 60,
            endturn: 30,
            opponent: 1,
            password: String::new(),
            focus: CreateField::Name,
        }
    }
}

impl CreateForm {
    /// Current field value as a string (for drawing).
    pub fn value(&self, field: CreateField) -> String {
        match field {
            CreateField::Name => self.name.clone(),
            CreateField::World => self.world.to_string(),
            CreateField::Timer => self.timer.to_string(),
            CreateField::EndTurn => self.endturn.to_string(),
            CreateField::Opponent => self.opponent.to_string(),
            CreateField::Password => self.password.clone(),
        }
    }

    /// Enters a character into the focused field.
    pub fn push_char(&mut self, ch: char) {
        match self.focus {
            CreateField::Name => {
                if self.name.chars().count() < 24 && (ch.is_ascii_graphic() || ch == ' ') {
                    self.name.push(ch);
                }
            }
            CreateField::Password => {
                if self.password.chars().count() < 16 && (ch.is_ascii_graphic() || ch == ' ') {
                    self.password.push(ch);
                }
            }
            field => {
                if let Some(digit) = ch.to_digit(10) {
                    let value = self
                        .numeric(field)
                        .saturating_mul(10)
                        .saturating_add(i64::from(digit));
                    self.set_numeric(field, value);
                }
            }
        }
    }

    /// Backspace in the current field.
    pub fn backspace(&mut self) {
        match self.focus {
            CreateField::Name => {
                self.name.pop();
            }
            CreateField::Password => {
                self.password.pop();
            }
            field => {
                let value = self.numeric(field) / 10;
                self.set_numeric(field, value);
            }
        }
    }

    /// Changes the numeric field by `delta` (Left/Right arrows).
    pub fn adjust(&mut self, delta: i64) {
        if self.focus.is_numeric() {
            let value = self.numeric(self.focus).saturating_add(delta);
            self.set_numeric(self.focus, value);
        }
    }

    /// `CreateGame` parameters.
    pub fn create_params(&self) -> CreateGameParams {
        let name = self.name.trim();

        CreateGameParams {
            name: Some(if name.is_empty() {
                DEFAULT_GAME_NAME.to_string()
            } else {
                name.to_string()
            }),
            world: Some(self.world.clamp(0, MAX_WORLD)),
            timer: Some(self.timer.clamp(0, 3600)),
            endturn: Some(self.endturn.clamp(0, 600)),
            play_mode: None,
            start_gold: None,
            victory_type: None,
            victory_limit: None,
            min_players: Some((self.opponent.clamp(1, 3) + 1) as u32),
            password: if self.password.is_empty() {
                None
            } else {
                Some(self.password.clone())
            },
        }
    }

    /// Password for `JoinGame`, if the game is protected.
    pub fn password_value(&self) -> Option<String> {
        (!self.password.is_empty()).then(|| self.password.clone())
    }

    fn numeric(&self, field: CreateField) -> i64 {
        match field {
            CreateField::World => self.world,
            CreateField::Timer => self.timer,
            CreateField::EndTurn => self.endturn,
            CreateField::Opponent => self.opponent,
            _ => 0,
        }
    }

    fn set_numeric(&mut self, field: CreateField, value: i64) {
        match field {
            CreateField::World => self.world = value.clamp(0, MAX_WORLD),
            CreateField::Timer => self.timer = value.clamp(0, 3600),
            CreateField::EndTurn => self.endturn = value.clamp(0, 600),
            CreateField::Opponent => self.opponent = value.clamp(1, 3),
            _ => {}
        }
    }
}

/// Client screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// Main menu.
    MainMenu,
    /// Game list from `list_games`.
    JoinList {
        /// Lobby games.
        games: Vec<GameInfo>,
        /// Selected game.
        selected: usize,
    },
    /// Game creation form.
    CreateForm(CreateForm),
    /// `New Game` submenu (`menu_new_game_menu_loop`, `menu.cpp:2171`):
    /// Training/Scenario/Custom/Multiplayer Scenario/Campaign/Tips/Cancel.
    ///
    /// `tips` — the tips window state (`menu_draw_tips`), toggled by the `Tips`
    /// button.
    NewGameMenu {
        /// Open tips window; `None` — closed.
        tips: Option<TipsWindow>,
    },
    /// `GameSetupMenu` — the mission list of one category (`gamesetupmenu.cpp`).
    MissionSelect(MissionSelectForm),
    /// `Multiplayer Menu` (`menu_multiplayer_menu_loop`): host/join/hot-seat.
    MultiplayerMenu,
    /// `Introduction` (`Movie_PlayIntro`): MVE video `INTROFLC` (`maxint.mve`).
    Introduction(intro::IntroScreen),
    /// `Credits` (`menu_credits_menu_loop`): credits scrolling over `CREDITS`.
    Credits(credits::CreditsScreen),
    /// Local match setup (M-SP/M-HS): Options → Planet → Players.
    LocalSetup(GameSetupForm),
    /// `Preferences`/`Setup` dialog (`OptionsMenu`, `PREFSPIC`/`SETUPPIC`).
    ///
    /// Modal, like `OptionsMenu::Run` in C++: while open, the other screens
    /// receive no input. Closing (`Done`/`Cancel`) returns to the main menu.
    Preferences(Box<PreferencesDialog>),
    /// Start phases after `Players`: mobilization (`CargoMenu`) and landing point
    /// selection (`GameManager_SelectSite`).
    PreGame(PreGameFlow),
    /// Victory/defeat screen of the local match.
    GameOver,
    /// Campaign story screen (intro/victory/defeat), `menu.cpp:766-864`.
    ///
    /// Presentation only: the screen sequence of an already resolved mission. The
    /// driver keeps the action to resume (enter the session / leave to the menu) and
    /// executes it on [`Transition::StoryFinished`]; see [`crate::story`].
    MissionStory(Box<story::StoryScreen>),
    /// Modal help window over the previous screen (`HelpMenu`).
    ///
    /// In C++ `HelpMenu` is modal too: it is drawn over the menu/`CargoMenu`, and
    /// after closing control returns to the previous screen. The nested
    /// `screen` stores exactly it.
    Help {
        /// The screen under the help window.
        screen: Box<Screen>,
        /// Open help window; `None` — the element selection mode is active
        /// (`HelpMenu_Menu`, `helpmenu.cpp:315`).
        help: Option<helpmenu::HelpScreen>,
        /// Help mode section.
        mode: helpmenu::HelpMode,
    },
    /// The game is running: map + status; the network loop is launched by `main.rs`.
    InSession {
        /// Where to connect.
        request: SessionRequest,
    },
}

/// Minimal Horizon client the menu needs.
///
/// Synchronous, so that [`Screen::handle_input`] remains a pure function without
/// `async`: the network is called only on an explicit action (Join/Refresh/Create).
pub trait MenuClient {
    /// `ListGames` of the lobby.
    fn list_games(&mut self) -> Result<Vec<GameInfo>, String>;
    /// `CreateGame`; returns the created game.
    fn create_game(&mut self, params: CreateGameParams) -> Result<GameInfo, String>;
}

/// Real Horizon client: a synchronous wrapper over `max_net::HorizonClient`.
///
/// Each call brings up a short-lived `tokio` runtime and closes the gRPC
/// channel after the response — that is enough for menu operations (list/create),
/// while the long session goes through [`crate::run_network`].
#[derive(Debug, Clone)]
pub struct HorizonMenuClient {
    host: String,
    port: u16,
}

impl HorizonMenuClient {
    /// Client for Horizon `host:port`.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    fn runtime() -> Result<tokio::runtime::Runtime, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("tokio runtime: {error}"))
    }

    async fn connect(host: &str, port: u16) -> Result<HorizonClient, String> {
        let mut client = HorizonClient::connect(host, port)
            .await
            .map_err(|error| format!("Horizon {host}:{port}: {error}"))?;

        client
            .hello(HORIZON_PROTOCOL_VERSION, CLIENT_NAME)
            .await
            .map_err(|error| format!("Horizon Hello: {error}"))?;

        Ok(client)
    }
}

impl MenuClient for HorizonMenuClient {
    fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
        let runtime = Self::runtime()?;

        runtime.block_on(async {
            let mut client = Self::connect(&self.host, self.port).await?;
            client
                .list_games()
                .await
                .map_err(|error| format!("Horizon ListGames: {error}"))
        })
    }

    fn create_game(&mut self, params: CreateGameParams) -> Result<GameInfo, String> {
        let runtime = Self::runtime()?;

        runtime.block_on(async {
            let mut client = Self::connect(&self.host, self.port).await?;
            let created = client
                .create_game(params)
                .await
                .map_err(|error| format!("Horizon CreateGame: {error}"))?;
            Ok(created.game)
        })
    }
}

/// UI context: screen sizes, mouse, status and the Horizon client.
pub struct UiContext {
    /// Lobby client (a mock in tests).
    pub client: Box<dyn MenuClient>,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Status/error line under the button row.
    pub status: String,
    /// Mouse position (for hover).
    pub mouse: Option<Point>,
    /// Main menu focus (`0..=2`).
    pub focus: usize,
    /// Default player name (`--player`) for M-SP setup.
    pub player_name: String,
    /// `.WRL` map directory for the planet screen minimaps.
    pub world_dir: std::path::PathBuf,
    /// Sound (M6.2): menu clicks. Without the `sound` feature — a silent no-op.
    pub audio: AudioPlayer,
    /// Intro video path (`INTROFLC` → `maxint.mve`); `None` — placeholder.
    pub intro_mve_path: Option<std::path::PathBuf>,
    /// Intro subtitles (`INTROSUB` → JSON next to `MAX.RES`); `None` — no captions.
    pub intro_subtitles: Option<crate::subtitles::SubtitleTrack>,
    /// Presentation media diagnostics of the launch (video/audio/subtitles/music):
    /// shown in the menu status line and printed by the client, so a missing layer is
    /// visible instead of silent (owner condition).
    pub media_status: Option<String>,
    /// Current user settings (for the `Setup` dialog).
    pub preferences: Preferences,
    /// Mission registry for the `GameSetupMenu` (`GameSetupMenu_Menu` reads
    /// `MissionManager::GetMissions`); built once from `PATCHES.RES` + the game
    /// data directory. `None` — the menu shows the visible fallback.
    pub missions: Option<Arc<MissionRegistry>>,
    /// Campaign clamp reported by the campaign list (`GameSetupMenu_Menu`,
    /// `menu.cpp:1972-1977`): the persistent `last_campaign` was above the mission
    /// count, so the oracle writes the clamped value back and saves the settings.
    ///
    /// The menu layer never writes the application (rule C3); the driver executes
    /// this through [`crate::App::apply_campaign_clamp`] and clears the field.
    pub campaign_clamp: Option<i32>,
}

impl UiContext {
    /// Window size context with an empty status.
    pub fn new(client: Box<dyn MenuClient>, width: u32, height: u32) -> Self {
        Self {
            client,
            width,
            height,
            status: String::new(),
            mouse: None,
            focus: 0,
            player_name: "Player".to_string(),
            world_dir: std::path::PathBuf::from("max-original"),
            audio: AudioPlayer::new(),
            intro_mve_path: None,
            intro_subtitles: None,
            media_status: None,
            preferences: Preferences::default(),
            missions: None,
            campaign_clamp: None,
        }
    }

    /// Context with a ready status.
    pub fn with_status(
        client: Box<dyn MenuClient>,
        width: u32,
        height: u32,
        status: impl Into<String>,
    ) -> Self {
        Self {
            status: status.into(),
            ..Self::new(client, width, height)
        }
    }

    /// Drawing parameters (without borrowing the client).
    pub fn view<'a>(&'a self, app: Option<&'a App>) -> ScreenView<'a> {
        ScreenView {
            width: self.width,
            height: self.height,
            status: &self.status,
            mouse: self.mouse,
            focus: self.focus,
            app,
            missions: self.missions.as_deref(),
        }
    }

    /// Menu click sound (`KBUY0` → `kbuy0.spw`).
    ///
    /// Called on button/row activation in the menu. An error/missing file do not
    /// change the UI state: sound is not a blocker (M6.2).
    pub fn click(&mut self) {
        let _ = self.audio.play_once(MENU_CLICK);
    }
}

impl fmt::Debug for UiContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiContext")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("status", &self.status)
            .field("mouse", &self.mouse)
            .field("focus", &self.focus)
            .field("audio", &self.audio)
            .finish_non_exhaustive()
    }
}

/// Everything [`Screen::render`] needs besides the screen itself.
///
/// `D` is the dispatcher of the session application (stage C §4.2): menus use the
/// `LocalDispatcher` default, the network sink renders `App<NetworkDispatcher>`.
pub struct ScreenView<'a, D: Dispatcher = LocalDispatcher> {
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Status line.
    pub status: &'a str,
    /// Mouse position.
    pub mouse: Option<Point>,
    /// Main menu focus.
    pub focus: usize,
    /// Match state for [`Screen::InSession`] — the dispatcher of the match.
    pub app: Option<&'a App<D>>,
    /// Mission registry of the `GameSetupMenu` (`PATCHES.RES` descriptors).
    ///
    /// `None` — the archive is unavailable: the mission screen shows a visible
    /// "registry unavailable" fallback and refuses to start anything.
    pub missions: Option<&'a MissionRegistry>,
}

// `Copy`/`Clone` are structural here; the derived bounds would wrongly require
// `D: Copy` (a dispatcher holds an FSM and is not `Copy`).
impl<D: Dispatcher> Clone for ScreenView<'_, D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D: Dispatcher> Copy for ScreenView<'_, D> {}

/// Input action on the start phase screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreGameInput {
    /// Nothing.
    None,
    /// Cancel the whole chain (back to `Players`).
    Cancel,
    /// Click on the mobilization screen (`CargoMenu`).
    MobilizeClick(i32, i32),
    /// Key on the mobilization screen.
    MobilizeKey(Key),
    /// Confirm the landing point.
    LandingConfirm,
    /// Move the landing cursor.
    LandingStep(i32, i32),
    /// Click on the landing screen.
    LandingClick(i32, i32),
}

impl Screen {
    /// Handles input and advances the state machine.
    ///
    /// Returns [`Transition::StartSession`] when the game is created/selected:
    /// the caller (`main.rs`) executes the network loop, not the UI core. Network
    /// errors remain in [`UiContext::status`], the screen does not change.
    pub fn handle_input(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        if let InputEvent::MouseMove(x, y) = event {
            ctx.mouse = Some(Point::new(x, y));
        }

        // Help mode/window (`HelpMenu_Menu` + `HelpMenu`): only it receives
        // input. A click on an element with a `help.json` entry opens the window;
        // `Esc`/RMB/`?` close the window (return to the mode) or the mode itself.
        if matches!(self, Self::Help { .. }) {
            let mut exit = false;

            if let Self::Help { help, mode, .. } = self {
                match event {
                    InputEvent::MouseMove(..) => {}
                    InputEvent::MouseClick(x, y) => {
                        if let Some(window) = help.as_mut() {
                            if window.input(InputEvent::MouseClick(x, y)) == HelpAction::Close {
                                *help = None;
                            }
                        } else if let Some(window) = mode.click(x, y) {
                            *help = Some(window);
                            ctx.click();
                        }
                    }
                    InputEvent::RightClick(..) => {
                        if help.is_some() {
                            *help = None;
                        } else {
                            exit = true;
                        }
                    }
                    InputEvent::Key(Key::Escape) | InputEvent::Key(Key::Char('?')) => {
                        if help.is_some() {
                            *help = None;
                        } else {
                            exit = true;
                        }
                    }
                    InputEvent::Key(key) => {
                        if let Some(window) = help.as_mut() {
                            if window.input(InputEvent::Key(key)) == HelpAction::Close {
                                *help = None;
                            }
                        }
                    }
                }
            }

            if exit {
                if let Self::Help { screen, .. } = std::mem::replace(self, Self::MainMenu) {
                    *self = *screen;
                }
            }

            return Transition::None;
        }

        if matches!(self, Self::MainMenu) {
            return self.handle_main_menu(ctx, event);
        }
        if matches!(self, Self::Preferences(_)) {
            return self.handle_preferences(ctx, event);
        }
        if matches!(self, Self::JoinList { .. }) {
            return self.handle_join_list(ctx, event);
        }
        if matches!(self, Self::CreateForm(_)) {
            return self.handle_create_form(ctx, event);
        }
        if matches!(self, Self::MultiplayerMenu) {
            return self.handle_multiplayer_menu(ctx, event);
        }
        if matches!(self, Self::NewGameMenu { .. }) {
            return self.handle_new_game_menu(ctx, event);
        }
        if matches!(self, Self::MissionSelect(_)) {
            return self.handle_mission_select(ctx, event);
        }
        if matches!(self, Self::LocalSetup(_)) {
            return self.handle_local_setup(ctx, event);
        }
        if matches!(self, Self::PreGame(_)) {
            return self.handle_pregame(ctx, event);
        }
        if matches!(self, Self::GameOver) {
            return match event {
                InputEvent::Key(Key::Enter) | InputEvent::Key(Key::Escape) => {
                    Transition::LeaveSession
                }
                InputEvent::MouseClick(x, y) if winloss::ok_at(x, y) => Transition::LeaveSession,
                _ => Transition::None,
            };
        }
        if matches!(self, Self::MissionStory(_)) {
            // `menu_draw_mission_story_screen`: Enter/Esc close the text window, the
            // videos play, any key/click aborts a movie. The driver resumes its own
            // action (enter the match / leave to the menu) on `StoryFinished`.
            if let Self::MissionStory(screen) = self {
                return match screen.input(event) {
                    StoryAction::None => Transition::None,
                    StoryAction::Finished => Transition::StoryFinished,
                };
            }
        }
        if matches!(self, Self::Introduction(_)) {
            // MVE: like `mve_cb_ctl` in the original — any key/click
            // interrupts and returns to the main menu.
            return match event {
                InputEvent::MouseMove(..) => Transition::None,
                _ => {
                    *self = Self::MainMenu;
                    ctx.status = "Introduction closed".to_string();
                    Transition::None
                }
            };
        }
        if matches!(self, Self::Credits(_)) {
            return self.handle_credits(ctx, event);
        }

        match event {
            InputEvent::Key(Key::Escape) => Transition::LeaveSession,
            _ => Transition::None,
        }
    }

    /// One screen animation step (`dt` seconds; `dt <= 0` — the screen stands still).
    ///
    /// [`Screen::Credits`] scrolls, [`Screen::Introduction`] and
    /// [`Screen::MissionStory`] advance their MVE movies. Returns `true` if the show
    /// is over and the screen should close (in C++ `menu_credits_menu_loop` exits on
    /// `height_offset <= bounds.uly`; `Movie_Play` returns when the movie ends).
    pub fn advance(&mut self, dt: f32) -> bool {
        if let Self::Credits(screen) = self {
            screen.advance(dt);

            return screen.finished();
        }

        if let Self::Introduction(screen) = self {
            return screen.advance(dt);
        }

        if let Self::MissionStory(screen) = self {
            // `true` — the last `Movie_Play` of the story has ended.
            return screen.advance(dt);
        }

        false
    }

    /// Input on the credits screen: `KP-`/`KP+` change the speed (in the client — `-`/`+`),
    /// any other key or click closes the screen (`menu_credits_menu_loop`).
    fn handle_credits(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let Self::Credits(screen) = self else {
            return Transition::None;
        };
        let mut close = false;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::Key(Key::Char('+')) | InputEvent::Key(Key::Char('=')) => screen.faster(),
            InputEvent::Key(Key::Char('-')) | InputEvent::Key(Key::Char('_')) => screen.slower(),
            _ => close = true,
        }

        if close {
            // Return to the main menu (in C++ the credits loop simply ends).
            ctx.status = "Credits closed".to_string();
            *self = Self::MainMenu;
        }

        Transition::None
    }

    /// Input on the M-SP setup screen (`Options` → `Planet` → `Players`).
    fn handle_local_setup(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let Self::LocalSetup(form) = self else {
            return Transition::None;
        };

        let transition = form.handle_input(event, ctx.width, ctx.height);
        let hot_seat = form.hot_seat;

        match transition {
            SetupTransition::None => Transition::None,
            SetupTransition::Back => {
                // Esc/Cancel returns where it came from: to `Multiplayer
                // Menu` for hot-seat (`menu_custom_game_menu`), otherwise to the
                // `New Game` submenu — the custom game is opened from there
                // (`menu_new_game_menu_loop` continues after a cancelled custom game).
                let back = if hot_seat {
                    Self::MultiplayerMenu
                } else {
                    Self::NewGameMenu { tips: None }
                };
                *self = back;
                ctx.status = "New game cancelled".to_string();
                Transition::None
            }
            SetupTransition::Start(setup) => {
                // Start phases: mobilization and landing point selection — before the match,
                // like `GameManager_PlayerMissionSetup` in C++.
                let flow = PreGameFlow::start(form.clone(), *setup, &ctx.world_dir);
                let finished = flow.finished();
                let ready = flow.setup.clone();

                *self = Self::PreGame(flow);
                ctx.status.clear();

                if finished {
                    Transition::StartLocal(Box::new(ready))
                } else {
                    Transition::None
                }
            }
            SetupTransition::Help(section) => {
                // The setup menu `?` button: the `HELPFRAM` window over the screen
                // (`HelpMenu_Menu`); the position — the center of the 640×480 window.
                let inner = std::mem::replace(self, Self::MainMenu);
                *self = Self::Help {
                    screen: Box::new(inner),
                    help: None,
                    mode: helpmenu::HelpMode::new(section, HelpPlacement::Menu),
                };
                ctx.click();
                Transition::None
            }
        }
    }

    /// Input on the start phase screens (`CargoMenu` and landing point selection).
    fn handle_pregame(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let Self::PreGame(flow) = self else {
            return Transition::None;
        };

        // The action is computed from the current screen, applied already without borrow.
        let action = match &mut flow.stage {
            PreGameStage::Mobilize(_) => match event {
                InputEvent::MouseMove(..) | InputEvent::RightClick(..) => PreGameInput::None,
                InputEvent::MouseClick(x, y) => PreGameInput::MobilizeClick(x, y),
                InputEvent::Key(key) => PreGameInput::MobilizeKey(key),
            },
            PreGameStage::Landing(_) => match event {
                InputEvent::MouseMove(..) | InputEvent::RightClick(..) => PreGameInput::None,
                InputEvent::Key(Key::Escape) => PreGameInput::Cancel,
                InputEvent::Key(Key::Enter) => PreGameInput::LandingConfirm,
                InputEvent::Key(Key::Left) => PreGameInput::LandingStep(-1, 0),
                InputEvent::Key(Key::Right) => PreGameInput::LandingStep(1, 0),
                InputEvent::Key(Key::Up) => PreGameInput::LandingStep(0, -1),
                InputEvent::Key(Key::Down) => PreGameInput::LandingStep(0, 1),
                InputEvent::Key(_) => PreGameInput::None,
                InputEvent::MouseClick(x, y) => PreGameInput::LandingClick(x, y),
            },
            PreGameStage::Finished => PreGameInput::None,
        };

        let mut back_to_form = false;
        let mut confirmed = false;
        let mut open_help = false;

        match action {
            PreGameInput::None => {}
            PreGameInput::Cancel => back_to_form = true,
            PreGameInput::MobilizeClick(x, y) => {
                if let PreGameStage::Mobilize(mobilization) = &mut flow.stage {
                    match mobilization.handle_click(x, y) {
                        CargoAction::None => {}
                        CargoAction::Cancel => back_to_form = true,
                        CargoAction::Done => {
                            flow.commit_mobilization();
                            confirmed = true;
                        }
                        CargoAction::Help => open_help = true,
                    }
                }
            }
            PreGameInput::MobilizeKey(key) => {
                if let PreGameStage::Mobilize(mobilization) = &mut flow.stage {
                    match mobilization.handle_key(key) {
                        CargoAction::None => {}
                        CargoAction::Cancel => back_to_form = true,
                        CargoAction::Done => {
                            flow.commit_mobilization();
                            confirmed = true;
                        }
                        CargoAction::Help => open_help = true,
                    }
                }
            }
            PreGameInput::LandingConfirm => {
                let ok = if let PreGameStage::Landing(landing) = &mut flow.stage {
                    landing.confirm() == LandingAction::Confirm
                } else {
                    false
                };

                if ok {
                    flow.commit_landing();
                    confirmed = true;
                }
            }
            PreGameInput::LandingStep(dx, dy) => {
                if let PreGameStage::Landing(landing) = &mut flow.stage {
                    landing.step(dx, dy);
                }
            }
            PreGameInput::LandingClick(x, y) => {
                let result = if let PreGameStage::Landing(landing) = &mut flow.stage {
                    landing.click(x, y)
                } else {
                    LandingAction::None
                };

                match result {
                    LandingAction::Confirm => {
                        flow.commit_landing();
                        confirmed = true;
                    }
                    LandingAction::Cancel => back_to_form = true,
                    LandingAction::None => {}
                }
            }
        }

        if back_to_form {
            *self = Self::LocalSetup(flow.form.clone());
            ctx.status = "Start cancelled".to_string();

            return Transition::None;
        }

        if confirmed && flow.finished() {
            return Transition::StartLocal(Box::new(flow.setup.clone()));
        }

        if open_help {
            // `CargoMenu` of the start phase — state `GAME_STATE_6_GAME_SETUP`:
            // `GameManager_GetDialogWindowCenterMode()` gives `WINDOW_MAIN_MAP`,
            // so the window stands at (253,126), like `HelpMenu` in game.
            let inner = std::mem::replace(self, Self::MainMenu);
            *self = Self::Help {
                screen: Box::new(inner),
                help: None,
                mode: helpmenu::HelpMode::new(HelpSection::Cargo, HelpPlacement::Game),
            };
            ctx.click();
        }

        Transition::None
    }

    /// `Multiplayer Menu` input (`menu_multiplayer_menu_loop`).
    ///
    /// Working: `Join Network Game` (Horizon game list), `Hot Seat Game`
    /// (M-HS — the same three setup screens, but `is_single_player = false`) and
    /// `Cancel`. `Host Network Game`, `Hot Seat Scenarios` and
    /// `Load Hot Seat Game` are not ported yet — they write a status line.
    fn handle_multiplayer_menu(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let mut action = None;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                action = menu::multiplayer_button_at(x, y);
            }
            InputEvent::Key(Key::Escape) => action = Some(menu::MultiplayerAction::Cancel),
            // Original hotkeys (`menu_multiplayer_menu_loop`):
            // Shift+H/J/O/T/L/C — host, join, hot seat, scenarios, load, cancel.
            InputEvent::Key(Key::Char('h' | 'H')) => action = Some(menu::MultiplayerAction::Host),
            InputEvent::Key(Key::Char('j' | 'J')) => action = Some(menu::MultiplayerAction::Join),
            InputEvent::Key(Key::Char('o' | 'O')) => {
                action = Some(menu::MultiplayerAction::HotSeat)
            }
            InputEvent::Key(Key::Char('t' | 'T')) => {
                action = Some(menu::MultiplayerAction::HotSeatScenarios)
            }
            InputEvent::Key(Key::Char('l' | 'L')) => {
                action = Some(menu::MultiplayerAction::LoadHotSeat)
            }
            InputEvent::Key(Key::Char('c' | 'C')) => action = Some(menu::MultiplayerAction::Cancel),
            _ => {}
        }

        if action.is_some() {
            ctx.click();
        }

        match action {
            Some(menu::MultiplayerAction::Host) => {
                // Game creation in Horizon (in the original — `Host Network Game`
                // via `Remote_Lobby`).
                *self = Self::CreateForm(CreateForm::default());
                ctx.status = "Create a network game (Enter creates, Esc back)".to_string();
                Transition::None
            }
            Some(menu::MultiplayerAction::Join) => {
                self.refresh_games(ctx);
                Transition::None
            }
            Some(menu::MultiplayerAction::HotSeat) => {
                let mut form = GameSetupForm::hot_seat();
                // C++ `menu_choose_player_menu_loop`: the human name is taken from
                // `settings["player_name"]` (`menu.cpp:1944`), not from the CLI.
                form.setup.player_name = ctx.preferences.player_name.clone();
                form.world_dir = ctx.world_dir.clone();
                *self = Self::LocalSetup(form);
                ctx.status = "Hot seat: 2-4 humans, Options -> Planet -> Players".to_string();
                Transition::None
            }
            Some(menu::MultiplayerAction::HotSeatScenarios) => {
                ctx.status = "Hot Seat Scenarios: screen is not ported yet".to_string();
                Transition::None
            }
            Some(menu::MultiplayerAction::LoadHotSeat) => {
                ctx.status = "Load Hot Seat Game: screen is not ported yet".to_string();
                Transition::None
            }
            Some(menu::MultiplayerAction::Cancel) => {
                *self = Self::MainMenu;
                ctx.status.clear();
                Transition::None
            }
            _ => Transition::None,
        }
    }

    /// `New Game` submenu input (`menu_new_game_menu_loop`, `menu.cpp:2171`).
    ///
    /// The seven buttons of the oracle plus the original hotkeys
    /// (`Shift+T/S/U/M/A/I/C`). `Tips` toggles the tips window
    /// (`menu_draw_tips`), `Cancel` returns to the main menu; the mission
    /// categories only open [`Self::MissionSelect`] — no match is touched here.
    fn handle_new_game_menu(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let mut action = None;
        let mut scroll = None;

        if let Self::NewGameMenu { tips } = self {
            match event {
                InputEvent::MouseMove(..) | InputEvent::RightClick(..) => {}
                InputEvent::MouseClick(x, y) => {
                    if tips.is_some() {
                        scroll = newgame::tips_scroll_at(x, y);
                    }

                    if scroll.is_none() {
                        action = newgame::new_game_button_at(x, y);
                    }
                }
                InputEvent::Key(Key::Escape) => action = Some(NewGameAction::Cancel),
                InputEvent::Key(Key::Char(character)) => {
                    action = match character.to_ascii_lowercase() {
                        't' => Some(NewGameAction::Training),
                        's' => Some(NewGameAction::Scenario),
                        'u' => Some(NewGameAction::Custom),
                        'm' => Some(NewGameAction::MultiplayerScenario),
                        'a' => Some(NewGameAction::Campaign),
                        'i' => Some(NewGameAction::Tips),
                        'c' => Some(NewGameAction::Cancel),
                        _ => None,
                    };
                }
                InputEvent::Key(_) => {}
            }
        }

        if let Some(down) = scroll {
            if let Self::NewGameMenu {
                tips: Some(window), ..
            } = self
            {
                window.scroll(down);
            }
            ctx.click();

            return Transition::None;
        }

        if action.is_some() {
            ctx.click();
        }

        match action {
            Some(NewGameAction::Training) => {
                *self = Self::MissionSelect(MissionSelectForm::new(MissionCategory::Training));
                ctx.status = "Training Missions: choose a mission, Enter starts".to_string();
                Transition::None
            }
            Some(NewGameAction::Scenario) => {
                *self = Self::MissionSelect(MissionSelectForm::new(MissionCategory::Scenario));
                ctx.status = "Stand Alone Missions: choose a mission, Enter starts".to_string();
                Transition::None
            }
            Some(NewGameAction::MultiplayerScenario) => {
                *self = Self::MissionSelect(MissionSelectForm::new(
                    MissionCategory::MultiPlayerScenario,
                ));
                ctx.status = "Custom Scenarios: choose a mission, Enter starts".to_string();
                Transition::None
            }
            Some(NewGameAction::Campaign) => {
                // `GameSetupMenu_Menu` (`menu.cpp:1968-1983`): the campaign list
                // opens on the persistent `last_campaign`, clamped to the mission
                // count. The clamp is only *reported* here — the driver writes it
                // back through `App::apply_campaign_clamp` (rule C3).
                let mut form = MissionSelectForm::new(MissionCategory::Campaign);

                if let Some(open) =
                    form.open_campaign(ctx.preferences.last_campaign, ctx.missions.as_deref())
                {
                    // `settings["game_file_number"] = last_campaign` (`menu.cpp:1979`).
                    ctx.preferences.game_file_number = open.last_campaign;

                    if open.clamped {
                        ctx.preferences.last_campaign = open.last_campaign;
                        ctx.campaign_clamp = Some(open.last_campaign);
                    }
                }

                *self = Self::MissionSelect(form);
                ctx.status = "Campaign Game: choose a mission, Enter starts".to_string();
                Transition::None
            }
            Some(NewGameAction::Custom) => {
                // `menu_custom_game_menu(true)` (`menu.cpp:2115`): the planet and
                // player setup screens, unchanged.
                let mut form = GameSetupForm::default();
                form.setup.player_name = ctx.preferences.player_name.clone();
                form.world_dir = ctx.world_dir.clone();
                *self = Self::LocalSetup(form);
                ctx.status = "Custom game: options, Enter — next screen, Esc — back".to_string();
                Transition::None
            }
            Some(NewGameAction::Tips) => {
                let mut opened = false;

                if let Self::NewGameMenu { tips } = self {
                    if tips.is_some() {
                        *tips = None;
                    } else {
                        *tips = Some(TipsWindow::new());
                        opened = true;
                    }
                }

                ctx.status = if opened {
                    "Tips: page arrows, Tips closes the window".to_string()
                } else {
                    "New game: choose a mission category".to_string()
                };
                Transition::None
            }
            Some(NewGameAction::Cancel) => {
                *self = Self::MainMenu;
                ctx.status.clear();
                Transition::None
            }
            None => Transition::None,
        }
    }

    /// `GameSetupMenu` input (`gamesetupmenu.cpp`).
    ///
    /// Rows select a mission; a second click on the selected row (or `Start`/Enter)
    /// emits [`Transition::StartMission`]. `Start` on a mission whose file is missing
    /// stays on the screen and explains why — the visible fallback of owner
    /// condition 5 — instead of starting an empty world.
    fn handle_mission_select(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let native_frame =
            ctx.width == menu::MENU_WIDTH as u32 && ctx.height == menu::MENU_HEIGHT as u32;
        let mut action = MissionSelectAction::None;

        {
            let Self::MissionSelect(form) = self else {
                return Transition::None;
            };
            let registry = ctx.missions.as_deref();

            match event {
                InputEvent::MouseMove(..) | InputEvent::RightClick(..) => {}
                InputEvent::MouseClick(x, y) if native_frame => {
                    action = gamesetupmenu::mission_select_at(x, y);

                    if let MissionSelectAction::Select(row) = action {
                        if form.select_row(registry, row) {
                            // `EventSelectItem`: a click on the selected row starts.
                            action = MissionSelectAction::Start;
                        }
                    }
                }
                InputEvent::Key(Key::Up) => form.step(registry, -1),
                InputEvent::Key(Key::Down) => form.step(registry, 1),
                InputEvent::Key(Key::PageUp) => action = MissionSelectAction::PageUp,
                InputEvent::Key(Key::PageDown) => action = MissionSelectAction::PageDown,
                InputEvent::Key(Key::Enter) => action = MissionSelectAction::Start,
                InputEvent::Key(Key::Escape) => action = MissionSelectAction::Cancel,
                InputEvent::Key(Key::Char('?')) => action = MissionSelectAction::Help,
                InputEvent::Key(_) => {}
                InputEvent::MouseClick(..) => {}
            }

            match action {
                MissionSelectAction::PageUp => form.scroll_page(registry, -1),
                MissionSelectAction::PageDown => form.scroll_page(registry, 1),
                MissionSelectAction::BriefUp => form.scroll_briefing(registry, -1),
                MissionSelectAction::BriefDown => form.scroll_briefing(registry, 1),
                _ => {}
            }
        }

        if action != MissionSelectAction::None {
            ctx.click();
        }
        match action {
            MissionSelectAction::Cancel => {
                *self = Self::NewGameMenu { tips: None };
                ctx.status = "New game: choose a mission category".to_string();
                Transition::None
            }
            MissionSelectAction::Help => {
                let section = match self {
                    Self::MissionSelect(form) => form.help_section(),
                    _ => HelpSection::NewGame,
                };
                let inner = std::mem::replace(self, Self::MainMenu);
                *self = Self::Help {
                    screen: Box::new(inner),
                    help: None,
                    mode: helpmenu::HelpMode::new(section, HelpPlacement::Menu),
                };
                Transition::None
            }
            MissionSelectAction::Start => {
                let Self::MissionSelect(form) = self else {
                    return Transition::None;
                };
                let registry = ctx.missions.as_deref();

                if !form.can_start(registry) {
                    ctx.status = form.start_error(registry);
                    return Transition::None;
                }

                ctx.status = String::new();
                Transition::StartMission(Box::new(MissionStartRequest {
                    category: form.category,
                    index: form.selected,
                }))
            }
            _ => Transition::None,
        }
    }

    /// Draws the screen into the frame.
    ///
    /// For [`Screen::InSession`] the map is taken from [`crate::App::render_frame`]
    /// (which draws `max_render::map_render`), with the status line on top.
    ///
    /// `D` is the dispatcher of the session application: after the lobby the
    /// network sink renders `App<NetworkDispatcher>` (stage C §4.2).
    pub fn render<D: Dispatcher>(&self, buffer: &mut RgbaBuffer, view: ScreenView<'_, D>)
    where
        App<D>: SessionLocal,
    {
        // The menu on the original resources (item E) — only in a 640×480 frame:
        // all coordinates and pictures are taken from the C++ port for the original window.
        let menu_assets = view
            .app
            .and_then(|app| app.assets())
            .and_then(|assets| assets.menu.as_ref());
        let native_frame =
            view.width as i32 == menu::MENU_WIDTH && view.height as i32 == menu::MENU_HEIGHT;
        let mouse = view.mouse.map(|point| (point.x, point.y));

        match self {
            Self::Help { screen, help, .. } => {
                // The help window is modal: first the screen under it, then `HELPFRAM`.
                screen.render(buffer, view);

                let assets = view.app.and_then(|app| app.assets());
                let fallback = assets.and_then(|assets| {
                    assets
                        .menu
                        .as_ref()
                        .map(|menu| menu.palette)
                        .or_else(|| panels::UiPalette::from_assets(assets))
                });

                if let Some(help) = help {
                    help.draw(
                        buffer,
                        assets.and_then(|assets| assets.help.as_ref()),
                        fallback,
                    );
                }
            }
            Self::MainMenu => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    menu::draw_main_menu(buffer, menu_assets, view.status, mouse);
                    return;
                }

                let ui =
                    build_main_menu(view.width, view.height, view.status, view.mouse, view.focus);
                render_ui(buffer, &ui);
            }
            Self::JoinList { games, selected } => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    menu::draw_join_list(buffer, menu_assets, games, *selected, view.status, mouse);
                    return;
                }

                let ui = build_join_list(
                    view.width,
                    view.height,
                    view.status,
                    view.mouse,
                    games,
                    *selected,
                );
                render_ui(buffer, &ui);
            }
            Self::CreateForm(form) => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    menu::draw_create_form(
                        buffer,
                        menu_assets,
                        view.app.and_then(|app| app.assets()),
                        form,
                        view.status,
                        mouse,
                    );
                    return;
                }

                let ui = build_create_form(view.width, view.height, view.status, view.mouse, form);
                render_ui(buffer, &ui);
                draw_planet_picker(
                    buffer,
                    view.app.and_then(|app| app.assets()),
                    form,
                    view.width,
                    view.height,
                );
            }
            Self::NewGameMenu { tips } => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    newgame::draw_new_game_menu(
                        buffer,
                        menu_assets,
                        tips.as_ref(),
                        view.status,
                        mouse,
                    );
                    return;
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "New Game Menu uses the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::MissionSelect(form) => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    gamesetupmenu::draw_mission_select(
                        buffer,
                        menu_assets,
                        view.missions,
                        form,
                        view.status,
                        mouse,
                    );
                    return;
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Mission selection uses the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::MultiplayerMenu => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    menu::draw_multiplayer_menu(buffer, menu_assets, view.status, mouse);
                    return;
                }
                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Multiplayer Menu uses the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::Introduction(screen) => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    screen.draw(buffer, menu_assets);
                    return;
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Introduction uses the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::Credits(screen) => {
                let background = menu_assets.and_then(|menu| menu.credits.as_ref());

                if let (Some(background), true) = (background, native_frame) {
                    screen.draw(buffer, background);
                    return;
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Credits need the original 640x480 frame and the CREDITS image",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::PreGame(flow) => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    match &flow.stage {
                        PreGameStage::Mobilize(mobilization) => {
                            // `CargoMenu` draws `I_*` icons and bars — a
                            // full asset set is needed, not just the menu.
                            if let Some(assets) = view.app.and_then(|app| app.assets()) {
                                mobilization.draw(buffer, assets, view.status, mouse);
                                return;
                            }
                        }
                        PreGameStage::Landing(landing) => {
                            let assets = view.app.and_then(|app| app.assets());
                            let chrome = assets.and_then(|assets| assets.chrome.as_ref());
                            let cursors = assets.and_then(|assets| assets.cursors.as_ref());
                            landing.draw(buffer, menu_assets, cursors, chrome, view.status, mouse);
                            return;
                        }
                        PreGameStage::Finished => {
                            buffer.fill(Rgba::rgb(12, 14, 18));
                        }
                    }
                    return;
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Start phases use the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::LocalSetup(form) => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    setup::draw_setup(buffer, menu_assets, form, view.status, mouse);
                    return;
                }

                // The frame is not 640×480: the setup screens are drawn in the original
                // coordinates, so we show an honest hint.
                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Setup screens use the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::Preferences(dialog) => {
                // `OptionsMenu` — a separate window over the main menu: in C++
                // the menu background remains visible around `PREFSPIC`.
                if let (Some(menu_assets), Some(app), true) = (menu_assets, view.app, native_frame)
                {
                    menu::draw_main_menu(buffer, menu_assets, view.status, mouse);

                    if let Some(assets) = app.assets() {
                        dialog.draw(buffer, assets);
                        return;
                    }
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Preferences use the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::GameOver => {
                if let (Some(menu_assets), Some(app)) = (menu_assets, view.app) {
                    if let Some(local) = app.local() {
                        winloss::draw_game_over(buffer, menu_assets, app.state_ref(), local, mouse);
                        return;
                    }
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(buffer, 12, 12, view.status, Rgba::rgb(232, 236, 244));
            }
            Self::MissionStory(screen) => {
                if native_frame {
                    screen.draw(buffer, menu_assets, mouse);
                    return;
                }

                // The story layout is in the original 640×480 coordinates.
                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Mission story uses the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::InSession { .. } => render_session(buffer, view),
        }
    }

    fn handle_main_menu(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let ui = build_main_menu(ctx.width, ctx.height, &ctx.status, ctx.mouse, ctx.focus);
        let mut activate: Option<usize> = None;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                let point = Point::new(x, y);

                if let Some(index) = hit_test(&ui, point) {
                    if let Some(action_index) = MAIN_ACTIONS.iter().position(|&item| item == index)
                    {
                        ctx.focus = action_index;
                        activate = Some(index);
                    } else if let Some(button) = menu::MAIN_MENU_BUTTONS.get(index) {
                        // Original buttons the Rust client does not have yet
                        // (`Load Game`, `Setup`, …): we do not stay silent, we write the status.
                        ctx.status = format!("{}: screen is not ported yet", button.label);
                        ctx.click();
                    }
                }
            }
            InputEvent::Key(key) => match key {
                Key::Up => {
                    let count = MAIN_ACTIONS.len();
                    ctx.focus = (ctx.focus + count - 1) % count;
                }
                Key::Down | Key::Tab => ctx.focus = (ctx.focus + 1) % MAIN_ACTIONS.len(),
                Key::Enter => {
                    activate = Some(MAIN_ACTIONS[ctx.focus.min(MAIN_ACTIONS.len() - 1)]);
                }
                Key::Escape => return Transition::Quit,
                Key::Char(character) => match character.to_ascii_lowercase() {
                    'c' => activate = Some(MAIN_CREATE),
                    'j' => activate = Some(MAIN_JOIN),
                    's' => activate = Some(MAIN_SETUP),
                    'i' => activate = Some(MAIN_INTRO),
                    'r' => activate = Some(MAIN_CREDITS),
                    'q' => return Transition::Quit,
                    _ => {}
                },
                _ => {}
            },
        }

        if activate.is_some() {
            ctx.click();
        }

        match activate {
            Some(MAIN_CREATE) => {
                // M-SP: `New Game` opens the submenu (`menu_new_game_menu_loop`,
                // `menu.cpp:2171`) — Training/Scenario/Custom/Multiplayer
                // Scenario/Campaign/Tips/Cancel. `Custom` leads to the local match
                // setup (`menu_custom_game_menu`: Options → Planet → Players).
                *self = Self::NewGameMenu { tips: None };
                ctx.status = "New game: choose a mission category".to_string();
                Transition::None
            }
            Some(MAIN_JOIN) => {
                // `Multiplayer Game` → `Multiplayer Menu` (`menu.cpp:2361`):
                // Host/Join/Hot Seat Game/Hot Seat Scenarios/Load Hot Seat/Cancel.
                *self = Self::MultiplayerMenu;
                ctx.status = "Multiplayer: Host, Join or Hot Seat Game".to_string();
                Transition::None
            }
            Some(MAIN_SETUP) => {
                // `menu_setup_window()` (`menu.cpp:2331`) → `OptionsMenu(SETUPPIC)`:
                // only the `setup` section (sound/video/autosave/network/name), over
                // the main menu, as a separate `PREFSPIC` window.
                let dialog = PreferencesDialog::new(
                    PreferencesVariant::Setup,
                    ctx.preferences.clone(),
                    preferences::PreferencesInfo::default(),
                );
                *self = Self::Preferences(Box::new(dialog));
                ctx.status = "Setup: Done saves, Cancel reverts".to_string();
                Transition::None
            }
            Some(MAIN_INTRO) => {
                // `Movie_PlayIntro` (`menu.cpp:2666`): the MVE video `INTROFLC`
                // (file `maxint.mve`). Frame 0 is decoded immediately so that
                // `shot`/`dt = 0` are deterministic; Esc/click — back to the menu.
                // C3: the audio track and the `INTROSUB` captions are optional layers.
                let screen = intro::IntroScreen::with_media(
                    ctx.intro_mve_path.clone(),
                    ctx.intro_subtitles.clone(),
                );
                let subtitles = match screen.subtitles() {
                    Some(track) => format!(", субтитры {} ({} шт.)", track.language(), track.len()),
                    None => ", субтитров нет".to_string(),
                };
                let audio = match screen.audio_format() {
                    Some(format) => format!(
                        ", звук {} Гц/{}/{} бит",
                        format.sample_rate,
                        if format.channels == 2 { "стерео" } else { "моно" },
                        format.bits
                    ),
                    None => ", без звука".to_string(),
                };
                ctx.status = if screen.is_loaded() {
                    format!(
                        "Introduction: maxint.mve — кадр {}{subtitles}{audio} (Esc)",
                        screen.frame_index()
                    )
                } else if let Some(error) = screen.error() {
                    format!("Introduction: {error} — Esc returns")
                } else {
                    "Introduction: maxint.mve не найден — Esc returns".to_string()
                };
                if let Some(note) = ctx.media_status.clone() {
                    ctx.status.push_str(&format!("; {note}"));
                }
                *self = Self::Introduction(screen);
                Transition::None
            }
            Some(MAIN_CREDITS) => {
                // `menu_credits_menu_loop` (`menu.cpp:1332`) — credits scrolling
                // over `CREDITS` 640×480; Esc/any key closes the screen.
                *self = Self::Credits(credits::CreditsScreen::new());
                ctx.status = "Credits: Esc returns to the menu".to_string();
                Transition::None
            }
            Some(MAIN_QUIT) => Transition::Quit,
            _ => Transition::None,
        }
    }

    /// Input in the `Preferences`/`Setup` dialog (`OptionsMenu`).
    ///
    /// `Done` saves the values and returns to the main menu; `Cancel` discards
    /// them; `?` opens the `SETUP_MENU_SETUP` help window over the dialog
    /// (`HelpMenu_Menu` in `OptionsMenu::ProcessKeyPress`).
    fn handle_preferences(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let Self::Preferences(dialog) = self else {
            return Transition::None;
        };

        match dialog.input(event) {
            preferences::PreferencesAction::None => Transition::None,
            preferences::PreferencesAction::Changed => {
                ctx.preferences = dialog.draft.clone();
                Transition::None
            }
            preferences::PreferencesAction::Help => {
                let section = dialog.variant().help_section();
                let inner = std::mem::replace(self, Self::MainMenu);
                *self = Self::Help {
                    screen: Box::new(inner),
                    help: None,
                    mode: helpmenu::HelpMode::new(section, HelpPlacement::Menu),
                };
                ctx.click();
                Transition::None
            }
            preferences::PreferencesAction::Done => {
                let prefs = dialog.draft.clone();
                ctx.preferences = prefs.clone();
                // The next match will start with this name (`menu.cpp:1944`).
                ctx.player_name = prefs.player_name.clone();
                ctx.status = "Preferences saved".to_string();
                *self = Self::MainMenu;
                ctx.click();
                Transition::ApplyPreferences(Box::new(prefs))
            }
            preferences::PreferencesAction::Cancel => {
                ctx.preferences = dialog.initial.clone();
                ctx.status = "Preferences cancelled".to_string();
                *self = Self::MainMenu;
                Transition::None
            }
        }
    }

    fn refresh_games(&mut self, ctx: &mut UiContext) {
        match ctx.client.list_games() {
            Ok(games) => {
                let count = games.len();
                *self = Self::JoinList { games, selected: 0 };
                ctx.status = if count == 0 {
                    "No games found — create one or refresh".to_string()
                } else {
                    format!("{count} game(s) — Enter joins, R refreshes")
                };
            }
            Err(error) => {
                ctx.status = format!("List games failed: {error}");
            }
        }
    }

    fn handle_join_list(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let (games, selected) = match self {
            Self::JoinList { games, selected } => (games.clone(), *selected),
            _ => return Transition::None,
        };
        let ui = build_join_list(
            ctx.width,
            ctx.height,
            &ctx.status,
            ctx.mouse,
            &games,
            selected,
        );
        let native_frame =
            ctx.width == menu::MENU_WIDTH as u32 && ctx.height == menu::MENU_HEIGHT as u32;

        enum Action {
            Select(usize),
            Back,
            Refresh,
            Join,
            Help,
            Create,
        }

        let mut action = None;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                if native_frame {
                    if let Some(index) = menu::join_row_at(x, y, games.len(), selected) {
                        action = Some(Action::Select(index));
                    } else if let Some(button) = menu::join_button_at(x, y) {
                        action = Some(match button {
                            menu::JoinButton::Refresh => Action::Refresh,
                            menu::JoinButton::Back | menu::JoinButton::Hide => Action::Back,
                            menu::JoinButton::Help => Action::Help,
                            menu::JoinButton::Join => Action::Join,
                        });
                    }
                } else {
                    let point = Point::new(x, y);

                    match hit_test(&ui, point) {
                        Some(JOIN_LIST) => {
                            if let Some(list) = ui.list_box(JOIN_LIST) {
                                if let Some(index) = list.row_at(point) {
                                    action = Some(Action::Select(index));
                                }
                            }
                        }
                        Some(JOIN_BACK | JOIN_HIDE) => action = Some(Action::Back),
                        Some(JOIN_REFRESH) => action = Some(Action::Refresh),
                        Some(JOIN_JOIN) => action = Some(Action::Join),
                        Some(JOIN_HELP) => action = Some(Action::Help),
                        _ => {}
                    }
                }
            }
            InputEvent::Key(Key::Up) => {
                action = Some(Action::Select(selected.saturating_sub(1)));
            }
            InputEvent::Key(Key::Down) => {
                action = Some(Action::Select(
                    (selected + 1).min(games.len().saturating_sub(1)),
                ));
            }
            InputEvent::Key(Key::Enter) => action = Some(Action::Join),
            InputEvent::Key(Key::Escape) => action = Some(Action::Back),
            InputEvent::Key(Key::Char('r' | 'R')) => action = Some(Action::Refresh),
            // Network game creation (the `CreateForm`) — from the game list screen:
            // in the main menu this button now opens the M-SP setup.
            InputEvent::Key(Key::Char('c' | 'C')) => action = Some(Action::Create),
            _ => {}
        }

        if action.is_some() {
            ctx.click();
        }

        match action {
            Some(Action::Select(index)) => {
                if let Self::JoinList { selected, .. } = self {
                    *selected = index;
                }
                Transition::None
            }
            Some(Action::Back) => {
                *self = Self::MainMenu;
                ctx.status.clear();
                Transition::None
            }
            Some(Action::Refresh) => {
                self.refresh_games(ctx);
                Transition::None
            }
            Some(Action::Help) => {
                ctx.status = "Help: screen is not ported yet".to_string();
                Transition::None
            }
            Some(Action::Create) => {
                *self = Self::CreateForm(CreateForm::default());
                ctx.status = "Create a network game (Enter creates, Esc back)".to_string();
                Transition::None
            }
            Some(Action::Join) => {
                let Some(game) = games.get(selected) else {
                    ctx.status = "No game selected — refresh the list".to_string();
                    return Transition::None;
                };
                let request = SessionRequest {
                    game_id: game.id.clone(),
                    password: None,
                };

                ctx.status = format!("Joining {} ({})", game.name, game.id);
                *self = Self::InSession {
                    request: request.clone(),
                };
                Transition::StartSession(request)
            }
            None => Transition::None,
        }
    }

    fn handle_create_form(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let ui = match self {
            Self::CreateForm(form) => {
                build_create_form(ctx.width, ctx.height, &ctx.status, ctx.mouse, form)
            }
            _ => return Transition::None,
        };
        let native_frame =
            ctx.width == menu::MENU_WIDTH as u32 && ctx.height == menu::MENU_HEIGHT as u32;

        let mut back = false;
        let mut submit = false;
        let mut clicked = false;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                // Planet grid: a click on the picture/name selects the planet. This is
                // done before `hit_test`, because the planet labels are regular
                // Labels, while the numeric field remains a Tab fallback.
                let planet = (ctx.height >= PLANET_GRID_MIN_HEIGHT)
                    .then(|| planet_cell_at(ctx.width, ctx.height, x, y))
                    .flatten();

                if let Some(index) = planet {
                    if let Self::CreateForm(form) = self {
                        form.world = index as i64;
                        form.focus = CreateField::World;
                    }
                    clicked = true;
                } else if native_frame {
                    // Original PLANETSE rectangles / MNUBTN buttons.
                    if let Some(button) = menu::create_button_at(x, y) {
                        clicked = true;

                        match button.action {
                            menu::MenuButtonAction::Cancel => back = true,
                            menu::MenuButtonAction::Done => submit = true,
                            menu::MenuButtonAction::Random => {
                                if let Self::CreateForm(form) = self {
                                    form.world = random_planet(form.world);
                                    form.focus = CreateField::World;
                                }
                            }
                            menu::MenuButtonAction::Help => {
                                ctx.status = "Help: screen is not ported yet".to_string();
                            }
                        }
                    } else if let Some(field) = menu::create_field_at(x, y) {
                        clicked = true;

                        if let Self::CreateForm(form) = self {
                            form.focus = field;
                        }
                    }
                } else {
                    let point = Point::new(x, y);

                    match hit_test(&ui, point) {
                        Some(CREATE_BACK) => {
                            back = true;
                            clicked = true;
                        }
                        Some(CREATE_SUBMIT) => {
                            submit = true;
                            clicked = true;
                        }
                        Some(CREATE_RANDOM) => {
                            clicked = true;
                            if let Self::CreateForm(form) = self {
                                form.world = random_planet(form.world);
                                form.focus = CreateField::World;
                            }
                        }
                        Some(CREATE_HELP) => {
                            clicked = true;
                            ctx.status = "Help: screen is not ported yet".to_string();
                        }
                        Some(index @ 1..=6) => {
                            clicked = true;
                            if let Self::CreateForm(form) = self {
                                form.focus = CreateField::ALL[index - 1];
                            }
                        }
                        _ => {}
                    }
                }
            }
            InputEvent::Key(key) => {
                if let Self::CreateForm(form) = self {
                    match key {
                        Key::Escape => back = true,
                        Key::Enter => submit = true,
                        Key::Tab | Key::Down => form.focus = form.focus.next(),
                        Key::Up => form.focus = form.focus.prev(),
                        Key::Backspace => form.backspace(),
                        Key::Left => form.adjust(-1),
                        Key::Right => form.adjust(1),
                        Key::Char(character) => form.push_char(character),
                        Key::PageUp | Key::PageDown | Key::Alt(_) | Key::Other => {}
                    }
                }
            }
        }

        if clicked || back || submit {
            ctx.click();
        }

        if back {
            *self = Self::MainMenu;
            ctx.status.clear();
            return Transition::None;
        }

        if submit {
            return self.submit_create(ctx);
        }

        Transition::None
    }

    fn submit_create(&mut self, ctx: &mut UiContext) -> Transition {
        let (params, password) = match self {
            Self::CreateForm(form) => {
                if !(0..=MAX_WORLD).contains(&form.world) {
                    ctx.status = format!("World must be 0..={MAX_WORLD}");
                    return Transition::None;
                }

                (form.create_params(), form.password_value())
            }
            _ => return Transition::None,
        };

        match ctx.client.create_game(params) {
            Ok(game) => {
                let request = SessionRequest {
                    game_id: game.id.clone(),
                    password,
                };
                ctx.status = format!("Created \"{}\" ({})", game.name, game.id);
                *self = Self::InSession {
                    request: request.clone(),
                };
                Transition::StartSession(request)
            }
            Err(error) => {
                ctx.status = format!("Create failed: {error}");
                Transition::None
            }
        }
    }
}

/// Input inside a session: map, HUD, orders (M4.3).
///
/// Works with [`App`] directly (not via [`Screen`]): selection and orders
/// live in [`crate::hud::HudState`], which needs access to `GameState` and
/// the camera. A pure function: no network, no SDL, no time — everything is tested
/// headless.
///
/// * **LMB on an own unit** — selection (`App::hud().selected`);
/// * **LMB on an enemy** — attack order (packet `08`, processor `P3`);
/// * **RMB on a free cell** — the `Paths_RequestPath` route +
///   the line preview + packet `38` (`Command::Move`);
/// * **RMB on a unit / Esc** — cancel (a second Esc — exit the session);
/// * **End Turn** — the same path as the timer (`06` + `52`/`finish_turn`);
/// * **Menu** — exit to the main menu.
///
/// # Chat (M4.4)
///
/// **Enter** or **T** open the input line (`ChatInput`): printable ASCII
/// characters go into the buffer, **Backspace** deletes the last one, **Enter** sends
/// `Command::Chat` (packet **18**), **Esc** closes the input without sending. While
/// the line is open, the mouse and other keys are ignored.
///
/// The immediate/queued difference is not an argument (SDL/UI Stage C2): it is the
/// session capability [`crate::local::SessionLocal::session_channel`], derived from
/// the dispatcher — a local match applies through `apply_now` and ticks in the same
/// frame, a lockstep match queues for `net::flush_hud_orders` → `submit`.
pub fn handle_session_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let (width, height) = app.viewport().screen();
    let chrome = app.chrome_active();

    // Mouse for edge camera scrolling (`Preferences`: `click_scroll`):
    // `GameManager_ProcessInput` remembers the position and buttons between frames.
    match event {
        InputEvent::MouseMove(x, y) => app.note_mouse_move(x, y),
        InputEvent::MouseClick(x, y) => app.note_mouse_click(x, y),
        _ => {}
    }

    // `Alt+P` — `PauseMenu_Menu` (`game_manager.cpp:5542`): the "Game Paused" dialog
    // over everything, local match ticks stop.
    if matches!(event, InputEvent::Key(Key::Alt('p' | 'P'))) {
        if app.pause_dialog().is_some() {
            return Transition::None;
        }
        if app.local_active() && app.open_pause_dialog() {
            app.hud_mut().push_log("Paused");
        }
        return Transition::None;
    }

    if app.pause_dialog().is_some() {
        return session_pause_input(app, event);
    }

    // Quick save/load `OKCancelMenu` (`okcancelmenu.cpp`, C++ `GameManager_SaveLoadGame`):
    // modal over the game; `Enter` — confirm, `Esc` — cancel.
    if app.save_confirm().is_some() {
        return session_save_confirm_input(app, event);
    }

    // M-HS: the turn-transition window is modal — like `DialogMenu::Run()` in C++, it
    // accepts only confirmation (a click on `OK`, Enter, Esc). The mouse moves
    // (button highlight), everything else is ignored, including chat and orders.
    if app.local_notice().is_some() {
        return match event {
            InputEvent::Key(Key::Enter) | InputEvent::Key(Key::Escape) => {
                app.local_acknowledge_turn();
                Transition::None
            }
            InputEvent::MouseClick(x, y) if crate::hotseat::ok_at(x, y) => {
                app.local_acknowledge_turn();
                Transition::None
            }
            _ => Transition::None,
        };
    }

    // Modal windows: help (`HelpMenu`) — topmost, then the research
    // screen, then `BuildMenu` (like `GameManager_DisableMainMenu` in C++):
    // the map/panels/chat receive no order input while they are open.
    if app.help_screen().is_some() {
        return session_help_input(app, event);
    }

    // C++ help mode (`HelpMenu_Menu`): modal, a click on an element with a
    // `help.json` entry opens the window; `Esc`/RMB/`?` disable the mode.
    if app.help_mode().is_some() {
        return session_help_mode_input(app, event);
    }

    // Building self-destruct (`UnitsManager_SelfDestructMenu`) — a modal window
    // over everything, including the command menu (`GameManager_DeinitPopupButtons`
    // is called before opening).
    if app.self_destruct().is_some() {
        return session_self_destruct_input(app, event);
    }

    // `Preferences` (`OptionsMenu::Run`, `PREFSPIC`) — a modal dialog: the map and
    // panels receive no input (`GameManager_DisableMainMenu`).
    if app.preferences_dialog().is_some() {
        return session_preferences_input(app, event);
    }

    // Research screen (P7) — modal, like `ResearchMenu::Run`.
    if app.research_menu().is_some() {
        return session_research_menu_input(app, event);
    }

    // Mine extraction allocation (C++ `AllocMenu`, window `ALLOCFRM`) —
    // a modal dialog: the map/panels receive no input.
    if app.alloc_menu().is_some() {
        return session_alloc_menu_input(app, event);
    }

    // X-FER (C++ `TransferMenu`, window `XFERPIC`) — a modal quantity window:
    // `GameManager_DisableMainMenu` around `TransferMenu::Run`.
    if app.transfer_menu().is_some() {
        return session_transfer_menu_input(app, event);
    }

    if app.build_menu().is_some() {
        return session_build_menu_input(app, event);
    }

    // Purchasable upgrades screen (P6) — modal, like `UpgradeMenu::Run`.
    if app.upgrade_menu().is_some() {
        return session_upgrade_menu_input(app, event);
    }

    // The V70 save/load window (P3) is modal too: while it is open,
    // the map/panels/order chat receive no input (`GameManager_DisableMainMenu`).
    if app.save_load().is_some() {
        return session_saveload_input(app, event);
    }

    // Selected unit command menu (C++ `GameManager_PopupButtons`) — modal:
    // while it is open, the map/panels/chat receive no orders, and a click outside the menu
    // or Esc closes it (like `GameManager_DeinitPopupButtons`).
    if app.unit_menu().is_some() {
        return session_unit_menu_input(app, event);
    }

    // While the computer moves/a transition runs, orders and selection are impossible:
    // in C++ the map is not redrawn at this time (`game_manager.cpp:1179`),
    // and orders are discarded by `GameManager_ActiveTurnTeam`.
    let blocked = app.local_active() && app.local_input_blocked();

    // While the chat line is open (M4.4), the keyboard belongs to it: printable
    // ASCII characters go into the buffer, Backspace/Enter/Esc control the input. The mouse
    // is ignored so that a click during typing does not give an order.
    if app.hud().chat.is_active() {
        return match event {
            InputEvent::Key(key) => session_chat_key(app, key),
            _ => Transition::None,
        };
    }

    // Build site selection mode (`ORDER_STATE_SELECT_SITE`) — a port of
    // `GameManager_GetBuilderUnitCursor`/`PathBuild`/`ManagePlayerAction`:
    // LMB places the site in the cell under the cursor, Esc/RMB cancel. The C++ panels
    // remain enabled at this time, so clicks outside the map go
    // the usual way (`None`). Modal screens and the chat above take precedence.
    if app.build_site().is_some() && !blocked {
        if let Some(transition) = session_build_site_input(app, event, chrome) {
            return transition;
        }
    }

    match event {
        InputEvent::MouseMove(x, y) => {
            if chrome {
                // Panels: label/highlight and the cell under the cursor for the bottom
                // coordinates line.
                app.hud_mut().hover_button = None;
                app.hud_mut().hover_panel = panels::panel_button_at(x, y);
                let grid = panels::screen_to_map(x, y).and_then(|(map_x, map_y)| {
                    let (grid_x, grid_y) = app.map_viewport().screen_to_grid(map_x, map_y);

                    (grid_x >= 0 && grid_y >= 0 && grid_x < MAP_WIDTH && grid_y < MAP_HEIGHT)
                        .then_some((grid_x, grid_y))
                });

                app.hud_mut().panels.cursor_grid = grid;
            } else {
                app.hud_mut().hover_button = hud::hud_button_at(width, height, x, y);
            }

            Transition::None
        }
        InputEvent::MouseClick(x, y) => {
            if chrome {
                // Panels/minimap — view only when it is not our turn; the map
                // (selection/attack) is blocked.
                if blocked && panels::panel_button_at(x, y).is_none() {
                    return Transition::None;
                }
                session_chrome_left_click(app, x, y)
            } else if blocked {
                Transition::None
            } else {
                session_left_click(app, width, height, x, y)
            }
        }
        InputEvent::RightClick(x, y) => {
            if blocked {
                return Transition::None;
            }
            if chrome {
                session_chrome_right_click(app, x, y)
            } else {
                session_right_click(app, x, y)
            }
        }
        InputEvent::Key(Key::Char('p' | 'P')) if app.local_active() => {
            // Local match pause (`PauseMenu`): ticks and the timer are stopped.
            app.local_toggle_pause();
            Transition::None
        }
        // `B` — the unit panel `Build` button (`GameManager_MenuClickBuildButton`
        // → `BuildMenu_Menu`): opens the order screen for the selected builder.
        InputEvent::Key(Key::Char('b' | 'B')) if !blocked => {
            if app.open_build_menu() {
                app.hud_mut().mouse_position = None;
                app.hud_mut().push_log("Build menu");
            } else {
                app.hud_mut()
                    .push_log("Build: select a constructor, engineer or factory first");
            }
            Transition::None
        }
        // `O` — power of the selected unit (the `Power On`/`Power Off` popup in C++;
        // for `RESEARCH` the default topic research is started).
        InputEvent::Key(Key::Char('o' | 'O')) if !blocked => {
            let Some(id) = app.hud().selected else {
                app.hud_mut().push_log("Power: select a unit first");
                return Transition::None;
            };
            let Some(command) = hud::build_power_order(app.state_ref(), id) else {
                app.hud_mut().push_log("Power: unit cannot be powered");
                return Transition::None;
            };
            let label = "Power toggled";
            app.hud_mut().pending.push(command);
            app.hud_mut().push_log(label);
            apply_session_orders(app);
            Transition::None
        }
        // `U` — upgrades screen (`UpgradeMenu`): the `buy upg` item of the command menu
        // (C++ `UnitsManager_Popup_OnClick_BuyUpgrade`); the key is a shortcut.
        InputEvent::Key(Key::Char('u' | 'U')) if !blocked => {
            if app.open_upgrade_menu() {
                app.hud_mut().mouse_position = None;
                app.hud_mut().push_log("Upgrade menu");
            } else {
                app.hud_mut()
                    .push_log("Upgrade: select your unit (or gold refinery) first");
            }
            Transition::None
        }
        // `R` — research screen (`ResearchMenu`): the `resrch` item of the command menu
        // (C++ `UnitsManager_Popup_OnClick_Research`); the key is a shortcut.
        InputEvent::Key(Key::Char('r' | 'R')) if !blocked => {
            if app.open_research_menu() {
                app.hud_mut().mouse_position = None;
                app.hud_mut().push_log("Research menu");
            } else {
                app.hud_mut().push_log(
                    "Research: select a powered Research Centre of your team (O to power)",
                );
            }
            Transition::None
        }
        // `?` — the C++ help mode (`GNW_KB_KEY_SHIFT_DIVIDE`): the `GAME_SCREEN_SETUP`
        // window over the game (the `HELP_OF` panel does the same).
        InputEvent::Key(Key::Char('?')) if !blocked => {
            app.open_help(HelpSection::GameScreen);
            app.hud_mut().push_log("Help: game screen");
            Transition::None
        }
        InputEvent::Key(Key::Enter) | InputEvent::Key(Key::Char('t' | 'T')) if !blocked => {
            app.hud_mut().chat.open();
            app.hud_mut()
                .push_log("Chat: type a message, Enter sends, Esc cancels");
            Transition::None
        }
        InputEvent::Key(Key::Escape) => {
            if app.hud().selected.is_some() || app.hud().preview.is_some() {
                app.hud_mut().clear_interaction();
                app.hud_mut().push_log("Cancelled");
                Transition::None
            } else {
                Transition::LeaveSession
            }
        }
        // `S` — save the match, `L` — load (C++ `Alt+F`/`Alt+L`,
        // `GameManager_MenuClickFileButton`). In the network save/load are
        // unavailable (packet 16 is not ported) — `App::open_*_menu` writes
        // the explanation to the log.
        InputEvent::Key(Key::Char('s' | 'S')) if !blocked => {
            app.open_save_menu(app.session_channel());
            Transition::None
        }
        InputEvent::Key(Key::Char('l' | 'L')) if !blocked => {
            app.open_load_menu(app.session_channel());
            Transition::None
        }
        // `Alt+S`/`Alt+L` — quick save/load into the last slot: the `OKCancelMenu`
        // confirmation of `GameManager_SaveLoadGame` (`game_manager.cpp:5546-5550`).
        InputEvent::Key(Key::Alt('s' | 'S')) if !blocked => {
            app.quick_save(app.session_channel());
            Transition::None
        }
        InputEvent::Key(Key::Alt('l' | 'L')) if !blocked => {
            app.quick_load(app.session_channel());
            Transition::None
        }
        InputEvent::Key(_) => Transition::None,
    }
}

/// Input in the self-destruct dialog (`UnitsManager_SelfDestructMenu`).
///
/// The window is modal: `Arm`/`Enter` starts the `SLFDOPN1..6` animation, after it
/// `OK`/`Enter` gives `ORDER_EXPLODE`, `Esc`/`Cancel` closes without an order
/// (C++ `UnitsManager_SelfDestructMenu` returns `false`).
fn session_self_destruct_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let unit = app.self_destruct().map(|menu| menu.unit);
    let stage_before = app.self_destruct().map(|menu| menu.stage);

    match app.self_destruct_input(event) {
        Some(crate::selfdestruct::SelfDestructAction::None) => {
            // `Arm` (`units_manager.cpp:312`): sfx `MBUTT0`.
            if stage_before == Some(crate::selfdestruct::Stage::Confirm)
                && app
                    .self_destruct()
                    .is_some_and(|menu| menu.stage == crate::selfdestruct::Stage::Armed)
            {
                app.play_sfx(Sfx::Button);
            }
        }
        Some(crate::selfdestruct::SelfDestructAction::Cancel) => {
            // `NCANC0` (`units_manager.cpp:278/319`).
            app.play_sfx(Sfx::Cancel);
            app.hud_mut().push_log("Self destruct: cancelled");
        }
        Some(crate::selfdestruct::SelfDestructAction::Explode) => {
            // `SLFDOK` sfx `NDONE0` (`units_manager.cpp:254`).
            app.play_sfx(Sfx::Done);

            apply_session_orders(app);
            let name = unit
                .and_then(|unit| app.state_ref().unit(unit))
                .map(unit_label)
                .unwrap_or_else(|| "unit".to_string());
            app.hud_mut()
                .push_log(format!("Self destruct: {name} — ORDER_EXPLODE"));
        }
        None => {}
    }

    Transition::None
}

/// Input in the pause dialog (`PauseMenu_Menu` → `DialogMenu`, `Alt+P`).
fn session_pause_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    match app.pause_dialog_input(event) {
        Some(crate::dialog::DialogAction::Close) => {
            app.hud_mut().push_log("Resumed");
        }
        _ => {}
    }

    Transition::None
}

/// Input in the open selected unit command menu (C++ `GameManager_PopupButtons`).
///
/// The menu is modal: `Esc`/a click outside/a right click close it
/// (`GameManager_DeinitPopupButtons`), a click on an item and its hotkey
/// (`GameManager_ProcessPopupMenuInput`) invoke the item handler
/// (`App::activate_unit_menu_item`). Arrows and Enter do not work in the C++ menu —
/// neither here (they go to `MenuEvent::None`).
fn session_unit_menu_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let Some(outcome) = app.unit_menu_input(event) else {
        return Transition::None;
    };

    match outcome {
        crate::unitmenu::MenuEvent::None | crate::unitmenu::MenuEvent::Hover(_) => {}
        crate::unitmenu::MenuEvent::Close => {
            app.close_unit_menu();
            app.hud_mut().push_log("Unit menu closed");
        }
        crate::unitmenu::MenuEvent::Item(index) => {
            // `GameManager_ProcessPopupMenuInput` (`game_manager.cpp:2689`):
            // a popup item — `PlaySfx(KCARG0)`.
            app.play_sfx(Sfx::Click);

            if let Some(line) = app.activate_unit_menu_item(index) {
                app.hud_mut().push_log(line);

                apply_session_orders(app);
            }
        }
    }

    Transition::None
}

/// Input in the open mine extraction allocation dialog (C++ `AllocMenu`).
///
/// The dialog is modal (C++ `GameManager_DisableMainMenu` around `AllocMenu::Run`):
/// clicks on the map/panels give no orders. `Done`/`Esc`/`KP_Enter` close
/// the window — the values are already applied by clicks (`UpdateRawAllocation`), and on exit
/// C++ broadcasts `Remote_SendNetPacket_11` → `ORDER_NEW_ALLOCATE` (08/P5) to
/// each mine of the complex ([`App::queue_alloc_orders`]).
fn session_alloc_menu_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let Some(action) = app.alloc_menu_input(event) else {
        return Transition::None;
    };

    match action {
        crate::allocmenu::AllocAction::None => {}
        crate::allocmenu::AllocAction::Help => {
            app.open_help(HelpSection::Allocate);
            app.hud_mut().push_log("Allocate help");
        }
        crate::allocmenu::AllocAction::Close => {
            let queued = app.queue_alloc_orders();

            if let Some(line) = app.close_alloc_menu() {
                app.hud_mut().push_log(line);
            }
            if queued > 0 && app.session_channel().is_local() {
                apply_pending_orders(app);
            }
        }
    }

    Transition::None
}

/// Input in the open X-FER window (C++ `TransferMenu::Run`).
///
/// The window is modal (`GameManager_DisableMainMenu`): the map receives no order.
/// `Done`/`Enter` gives `ORDER_TRANSFER` (08/**P3**, `transfer_cargo`),
/// `Esc`/`Cancel` closes without an order, `?` opens `TRANSFER_SETUP`
/// over the window (in C++ `HelpMenu_Menu` returns to `TransferMenu`).
fn session_transfer_menu_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let Some(action) = app.transfer_menu_input(event) else {
        return Transition::None;
    };

    match action {
        crate::transfermenu::TransferAction::None => {}
        crate::transfermenu::TransferAction::Help => {
            app.open_help(HelpSection::Transfer);
            app.hud_mut().push_log("Transfer help");
        }
        crate::transfermenu::TransferAction::Cancel => {
            if let Some(line) = app.cancel_transfer() {
                app.hud_mut().push_log(line);
            }
        }
        crate::transfermenu::TransferAction::Confirm => {
            if let Some((line, queued)) = app.confirm_transfer() {
                app.hud_mut().push_log(line);

                // Offline the order is executed immediately (like the other orders from
                // the menu); in the network it goes as packet 08/P3.
                if queued && app.session_channel().is_local() {
                    apply_pending_orders(app);
                }
            }
        }
    }

    Transition::None
}
///
/// Window actions — [`crate::SaveLoadAction`]: write a slot, load a slot or
/// close. File I/O errors remain in the window status line so that the player
/// sees the reason instead of a silent close.
fn session_saveload_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let Some(menu) = app.save_load_mut() else {
        return Transition::None;
    };
    let action = menu.input(event);

    match action {
        crate::SaveLoadAction::None => Transition::None,
        crate::SaveLoadAction::Cancel => {
            app.close_save_load();
            app.hud_mut().push_log("Save/Load cancelled");
            Transition::None
        }
        crate::SaveLoadAction::Save { slot, name } => {
            match app.save_game_to_slot(slot, &name) {
                Ok(path) => {
                    // C++ `saveloadmenu.cpp:440`: a successful write becomes the
                    // quick save/load target.
                    app.set_last_save_slot(slot);
                    app.close_save_load();
                    app.hud_mut().push_log(format!("Saved {}", path.display()));
                }
                Err(error) => {
                    if let Some(menu) = app.save_load_mut() {
                        menu.status = format!("Save failed: {error}");
                    }
                }
            }
            Transition::None
        }
        crate::SaveLoadAction::Load { slot } => {
            match app.load_game_slot(slot) {
                Ok(path) => {
                    app.close_save_load();
                    app.hud_mut().push_log(format!("Loaded {}", path.display()));
                }
                Err(error) => {
                    if let Some(menu) = app.save_load_mut() {
                        menu.status = format!("Load failed: {error}");
                    }
                }
            }
            Transition::None
        }
    }
}

/// Input in the quick save/load `OKCancelMenu` (`okcancelmenu.cpp`).
///
/// `OK` (`Enter`/the `OK` button) performs the action captured by
/// [`crate::App::quick_save`]/[`crate::App::quick_load`]: write the last slot with
/// the name already stored in it (`GameManager_SaveLoadGame(true)`) or load it
/// (`...false`). `Cancel` (`Esc`/the `Cancel` button) closes the window and changes
/// nothing. The written format is the same as the window's save
/// (both go through [`crate::App::save_game_to_slot`]).
fn session_save_confirm_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    match app.save_confirm_input(event) {
        crate::saveload::ConfirmChoice::None => Transition::None,
        crate::saveload::ConfirmChoice::Cancel => {
            app.close_save_confirm();
            app.hud_mut().push_log("Save/Load cancelled");
            Transition::None
        }
        crate::saveload::ConfirmChoice::Ok => {
            let Some(action) = app.take_save_confirm_action() else {
                return Transition::None;
            };
            match action {
                crate::saveload::SaveConfirmAction::Save { slot, name } => {
                    match app.save_game_to_slot(slot, &name) {
                        Ok(_) => {
                            // C++ `MessageManager_DrawMessage(_(f640), …)`.
                            app.hud_mut().push_log("Game saved.");
                        }
                        Err(error) => {
                            app.hud_mut().push_log(format!("Save failed: {error}"));
                        }
                    }
                }
                crate::saveload::SaveConfirmAction::Load { slot } => {
                    match app.load_game_slot(slot) {
                        Ok(path) => {
                            app.hud_mut().push_log(format!("Loaded {}", path.display()));
                        }
                        Err(error) => {
                            app.hud_mut().push_log(format!("Load failed: {error}"));
                        }
                    }
                }
            }
            Transition::None
        }
    }
}

/// Input in the open `BuildMenu` (modal screen P2).
///
/// `Done`/Enter — `Build()`: the order goes as `Command::Order` with `PayloadP4`
/// (packet 08) — applied locally immediately, in the network it goes to the HUD queue.
/// `Cancel`/Esc — close without an order.
fn session_build_menu_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let action = app.build_menu_input(event);

    match action {
        Some(crate::buildmenu::BuildMenuAction::Commit) => {
            if let Some(line) = app.commit_build_menu(app.session_channel()) {
                app.hud_mut().push_log(line);
            }
            // `UnitsManager_Popup_OnClick_Build` → `GameManager_AutoSelectNext`
            // (`units_manager.cpp:825-826`): after a construction order C++ clears
            // the selection from the builder if it is no longer interactive.
            app.auto_select_after_order();
        }
        Some(crate::buildmenu::BuildMenuAction::PathBuild) => {
            if let Some(line) = app.commit_build_menu_path(app.session_channel()) {
                app.hud_mut().push_log(line);
            }
            app.auto_select_after_order();
        }
        Some(crate::buildmenu::BuildMenuAction::Close) => {
            app.close_build_menu();
            app.hud_mut().push_log("Build cancelled");
        }
        _ => {}
    }

    Transition::None
}

/// Input in the build site selection mode (`ORDER_STATE_SELECT_SITE`).
///
/// Repeats C++:
/// * **mouse move** — `GameManager_GetBuilderUnitCursor`: the tape ghost
///   is moved under the cursor (the 2×2 anchor is bound to the builder), the window cursor
///   becomes `CURSOR_MAP2`/`CURSOR_UNIT_NO_GO`;
/// * **LMB on the map** — `GameManager_PathBuild`: the `ORDER_BUILD` order with
///   the site cell; on an invalid cell C++ (`CURSOR_UNIT_NO_GO` →
///   `ManagePlayerAction`) cancels the selection;
/// * **RMB on the map / Esc** — cancel (`GameManager_SelectBuildSite`): the order does not
///   go, the builder queue does not change.
///
/// `None` — the event is not ours (a click on a panel/a key other than Esc): the caller
/// will handle it the usual way, like C++ with the main menu enabled.
fn session_build_site_input<D: Dispatcher>(
    app: &mut App<D>,
    event: InputEvent,
    chrome: bool,
) -> Option<Transition>
where
    App<D>: SessionLocal,
{
    // The map cell under the frame point — the same projection as for regular clicks
    // (`session_map_left_click`: `screen_to_map` with chrome).
    let grid_at = |app: &App<D>, x: i32, y: i32| -> Option<max_core::geo::Point> {
        let (gx, gy) = if chrome {
            let (map_x, map_y) = panels::screen_to_map(x, y)?;
            app.map_viewport().screen_to_grid(map_x, map_y)
        } else {
            app.viewport().screen_to_grid(x, y)
        };

        Some(max_core::geo::Point::new(gx as i16, gy as i16))
    };

    match event {
        InputEvent::MouseMove(x, y) => {
            app.hud_mut().mouse_position = Some((x, y));

            if let Some(grid) = grid_at(app, x, y) {
                if let Some(mut site) = app.build_site.take() {
                    site.update_cursor(app.state_ref(), grid);
                    app.build_site = Some(site);
                }
            }

            // Then the usual movement handling (panel label, coordinates).
            None
        }
        InputEvent::MouseClick(x, y) => {
            let Some(grid) = grid_at(app, x, y) else {
                return None;
            };
            let valid = match app.build_site.take() {
                Some(mut site) => {
                    site.update_cursor(app.state_ref(), grid);
                    let valid = site.valid(app.state_ref());
                    app.build_site = Some(site);
                    valid
                }
                None => false,
            };

            let line = if valid {
                // `GameManager_PathBuild` (`game_manager.cpp:4569`): the voice
                // `V_M049`, then `UnitsManager_StartBuild`.
                app.play_sfx(Sfx::Done);
                app.play_sfx(Sfx::BuildSitePlaced);
                app.confirm_build_site(app.session_channel())
            } else {
                // C++: `CURSOR_UNIT_NO_GO` → `GameManager_ManagePlayerAction` —
                // the selection is canceled entirely.
                app.play_sfx(Sfx::Cancel);
                app.cancel_build_site()
                    .map(|line| format!("{line} (invalid cell {},{} )", grid.x, grid.y))
            };

            if let Some(line) = line {
                app.hud_mut().push_log(line);
            }
            apply_session_orders(app);

            Some(Transition::None)
        }
        InputEvent::RightClick(x, y) => {
            // RMB on the map cancels the selection; on a panel — the usual handling.
            grid_at(app, x, y)?;

            if let Some(line) = app.cancel_build_site() {
                app.hud_mut().push_log(line);
            }

            Some(Transition::None)
        }
        InputEvent::Key(Key::Escape) => {
            if let Some(line) = app.cancel_build_site() {
                app.hud_mut().push_log(line);
            }

            Some(Transition::None)
        }
        _ => None,
    }
}

/// Input in the open upgrades screen (modal screen P6).
///
/// `Done`/Enter — `CommitUpgradeChanges`: the changed types are written into
/// `TeamUnits` (packets 10 in the network), gold is debited. `Cancel`/Esc — close
/// without changes (C++ `UpgradeMenu::Run` returns `false`). `Help` —
/// `UPGRADES_SETUP` over the screen (`HelpMenu_Menu`).
fn session_upgrade_menu_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let action = app.upgrade_menu_input(event);

    match action {
        Some(crate::upgrademenu::UpgradeAction::Commit) => {
            if let Some(line) = app.commit_upgrade_menu(app.session_channel()) {
                app.hud_mut().push_log(line);
            }
        }
        Some(crate::upgrademenu::UpgradeAction::Cancel) => {
            app.close_upgrade_menu();
            app.hud_mut().push_log("Upgrades cancelled");
        }
        Some(crate::upgrademenu::UpgradeAction::Help) => {
            app.open_help(HelpSection::Upgrades);
            app.hud_mut().push_log("Upgrades help");
        }
        _ => {}
    }

    Transition::None
}

/// Input in the help window over the game (`HelpMenu`).
///
/// The window is modal; `Done`/`Enter`/`Esc` close it (`event_click_cancel`), and
/// the screen under it (game/`ResearchMenu`/`UpgradeMenu`) keeps working with the same
/// state.
fn session_help_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    // Window buttons: `Done` — `NDONE0`, scrolling/`Keys` — `KCARG0`
    // (`helpmenu.cpp:91/131/135`).
    match event {
        InputEvent::MouseClick(x, y) => {
            if app
                .help_screen()
                .is_some_and(|help| help.done_rect().contains(x, y))
            {
                app.play_sfx(Sfx::Done);
            } else {
                app.play_sfx(Sfx::Click);
            }
        }
        InputEvent::Key(Key::Enter) => app.play_sfx(Sfx::Done),
        InputEvent::Key(Key::PageUp)
        | InputEvent::Key(Key::PageDown)
        | InputEvent::Key(Key::Up)
        | InputEvent::Key(Key::Down) => app.play_sfx(Sfx::Click),
        InputEvent::Key(Key::Char('k' | 'K')) => app.play_sfx(Sfx::Done),
        _ => {}
    }

    if app.help_input(event) == Some(HelpAction::Close) {
        app.close_help();
        app.hud_mut().push_log("Help closed");
    }

    Transition::None
}

/// Input in help mode (`HelpMenu_Menu`, `helpmenu.cpp:315`).
///
/// A click on an element with a `help.json` entry opens the window (`HelpMenu`); a click on
/// an empty place does not open the window and does not disable the mode (C++: empty text —
/// the window is not created). `Esc`, the right button or a repeated `?` disable the mode
/// (`MOUSE_RELEASE_RIGHT`/`GNW_KB_KEY_CTRL_ESCAPE`/`GNW_KB_KEY_SHIFT_DIVIDE`).
fn session_help_mode_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    match event {
        InputEvent::MouseClick(x, y) => {
            if app.help_mode_click(x, y) {
                // `HelpMenu_Menu` (`helpmenu.cpp:331`): choosing an element — `KCARG0`.
                app.play_sfx(Sfx::Click);
                let entry = app
                    .help_screen()
                    .and_then(|help| help.entry_name())
                    .unwrap_or("?");
                app.hud_mut().push_log(format!("Help: {entry}"));
            } else {
                app.hud_mut()
                    .push_log("Help: здесь нет подсказки — выберите элемент");
            }
        }
        InputEvent::RightClick(..)
        | InputEvent::Key(Key::Escape)
        | InputEvent::Key(Key::Char('?')) => {
            app.exit_help_mode();
            app.hud_mut().push_log("Help closed");
        }
        _ => {}
    }

    Transition::None
}

/// Input in the `Preferences` dialog over the game (C++ `OptionsMenu::Run`, `PREFSPIC`).
///
/// `Done`/`Enter` apply and save the settings, `Esc` restores the opening
/// snapshot, `?` opens the `PREFS_MENU_SETUP` help window. While the dialog is open,
/// `App::local_update` does not tick (the C++ loop sits in `Run`).
fn session_preferences_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    match app.preferences_input(event) {
        Some(preferences::PreferencesAction::Done) => {
            // The values are already in `App::preferences`; for the next frame the frame
            // clock reads the new `effects`.
        }
        Some(preferences::PreferencesAction::Cancel) => {
            app.hud_mut().push_log("Preferences cancelled");
        }
        Some(preferences::PreferencesAction::Help) => {
            let section = app
                .preferences_dialog()
                .map(|dialog| dialog.variant().help_section())
                .unwrap_or(HelpSection::Preferences);
            app.open_help(section);
            app.hud_mut().push_log("Preferences help");
        }
        _ => {}
    }

    Transition::None
}

/// Input in the open research screen (modal screen P7).
///
/// `Done`/`Enter` — `ResearchMenu_OnClick_Done`: the commit becomes packet **09**
/// ([`max_app::App::commit_research_menu`]) — a broadcast command applied by the
/// simulation (`events::apply_packet_09`), not a direct facade write. `Cancel`/`Esc` —
/// close without changes. `Help` — `RESEARCH_SETUP`.
fn session_research_menu_input<D: Dispatcher>(app: &mut App<D>, event: InputEvent) -> Transition
where
    App<D>: SessionLocal,
{
    let action = app.research_menu_input(event);

    match action {
        Some(ResearchAction::Done) => {
            if let Some(line) = app.commit_research_menu(app.session_channel()) {
                app.hud_mut().push_log(line);
            }
        }
        Some(ResearchAction::Cancel) => {
            app.close_research_menu();
            app.hud_mut().push_log("Research cancelled");
        }
        Some(ResearchAction::Help) => {
            app.open_help(HelpSection::Research);
            app.hud_mut().push_log("Research help");
        }
        _ => {}
    }

    Transition::None
}

/// Key in the open chat input line.
///
/// * printable ASCII (including space) — into the buffer (up to [`hud::CHAT_MAX_CHARS`]);
/// * Backspace — delete the last character;
/// * Enter — send [`Packet18Chat`] (`Command::Chat`): in the network — to the
///   `App::hud().pending` queue (sent by `net::flush_hud_orders`), in demo —
///   immediately to `apply_command`; the log line is written in both modes;
/// * Esc — close the input without sending.
///
/// The other keys (arrows/Tab) are ignored in the chat so as not to
/// accidentally open a menu/order.
fn session_chat_key<D: Dispatcher>(app: &mut App<D>, key: Key) -> Transition
where
    App<D>: SessionLocal,
{
    match key {
        Key::Char(character) => {
            app.hud_mut().chat.push_char(character);
        }
        Key::Backspace => {
            app.hud_mut().chat.backspace();
        }
        Key::Enter => return submit_chat(app),
        Key::Escape => {
            app.hud_mut().chat.close();
            app.hud_mut().push_log("Chat cancelled");
        }
        Key::Tab
        | Key::Up
        | Key::Down
        | Key::Left
        | Key::Right
        | Key::PageUp
        | Key::PageDown
        | Key::Alt(_)
        | Key::Other => {}
    }

    Transition::None
}

/// Sends the chat line: log + [`Command::Chat`] (packet **18**).
///
/// An empty line is not sent (like `Remote_SendNetPacket_18`, which
/// returns on `message[0] == '\0'`), the input is simply closed.
fn submit_chat<D: Dispatcher>(app: &mut App<D>) -> Transition
where
    App<D>: SessionLocal,
{
    let Some(message) = app.hud_mut().chat.take_message() else {
        return Transition::None;
    };

    let sender = app.player_team();
    let line = hud::chat_line(sender, &message);
    let command = Command::Chat(Packet18Chat { sender, message });

    app.hud_mut().push_log(line);

    // SDL/UI Stage C2: the channel decides — a lockstep match queues packet 18 for
    // `net::flush_hud_orders`, a local match applies it in this frame.
    if app.session_channel().is_lockstep() {
        app.hud_mut().pending.push(command);
    } else {
        // B2 audit — the same command-channel flush as `App::flush_orders`: the
        // command is already built and applied through `apply_now` only because the
        // demo has no session driver; it is not a UI mutation of the simulation.
        app.apply_immediate(command);
    }

    Transition::None
}

/// Left button without chrome: procedural HUD buttons → minimap → map.
fn session_left_click<D: Dispatcher>(
    app: &mut App<D>,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
) -> Transition
where
    App<D>: SessionLocal,
{
    if let Some(button) = hud::hud_button_at(width, height, x, y) {
        return match button {
            HudButton::EndTurn => {
                session_end_turn(app);
                Transition::None
            }
            HudButton::Menu => Transition::LeaveSession,
        };
    }

    // A click on the minimap — camera centering (as in the original).
    let layout = hud::hud_layout(width, height);

    if let Some(grid) = hud::minimap_grid_at(&layout, x, y) {
        app.viewport_mut().center_on_grid(grid.x, grid.y);
        return Transition::None;
    }

    let viewport = *app.viewport();

    session_map_left_click(app, &viewport, x, y)
}

/// Left button with 1:1 chrome: panels → minimap → map.
///
/// The window screen coordinates are converted into map area coordinates
/// ([`panels::screen_to_map`]) before `screen_to_grid` — otherwise the click drifts by
/// (180,18) (risk 4 in `max-brh-docs/visual-1to1-status.md`).
fn session_chrome_left_click<D: Dispatcher>(app: &mut App<D>, x: i32, y: i32) -> Transition
where
    App<D>: SessionLocal,
{
    if let Some(button) = panels::panel_button_at(x, y) {
        return session_panel_button(app, button, x);
    }

    // Minimap: 112×112 world cells in the window (15,356). In 2x mode the click
    // is converted via `GameManager_GetMinimapPosition` (the 56×56 area around
    // the camera center), otherwise the click drifts.
    let minimap = max_render::layout::MINIMAP;

    if minimap.contains(x, y) {
        let state = app.state_ref();
        let map_width = i32::from(state.map().size.width);
        let map_height = i32::from(state.map().size.height);
        let two_x = app.hud().panels.minimap_2x;

        if let Some((grid_x, grid_y)) =
            panels::minimap_grid_at(x, y, map_width, map_height, app.viewport(), two_x)
        {
            app.viewport_mut().center_on_grid(grid_x, grid_y);
        }

        return Transition::None;
    }

    // A click outside the map (frame/scrolling) — not an order.
    let Some((map_x, map_y)) = panels::screen_to_map(x, y) else {
        return Transition::None;
    };
    let viewport = app.map_viewport();

    session_map_left_click(app, &viewport, map_x, map_y)
}

/// 1:1 panel button: End Turn, toggles, zoom, minimap, menu.
fn session_panel_button<D: Dispatcher>(app: &mut App<D>, button: PanelButton, x: i32) -> Transition
where
    App<D>: SessionLocal,
{
    // Panel buttons — `PlaySfx(MBUTT0)` (`game_manager.cpp:5446/5692/5713`).
    app.play_sfx(Sfx::Button);

    match button {
        PanelButton::EndTurn => {
            session_end_turn(app);
            return Transition::None;
        }
        PanelButton::Center => {
            if let Some(grid) = app.selected_unit().map(|unit| unit.grid) {
                app.viewport_mut().center_on_grid(grid.x, grid.y);
                app.hud_mut().push_log("Center on selected unit");
            }
            return Transition::None;
        }
        PanelButton::ZoomPlus => {
            let zoom = (app.viewport().zoom() + 8).clamp(8, 64);
            app.viewport_mut().set_zoom(zoom);
            return Transition::None;
        }
        PanelButton::ZoomMinus => {
            let zoom = app.viewport().zoom().saturating_sub(8).max(8);
            app.viewport_mut().set_zoom(zoom);
            return Transition::None;
        }
        PanelButton::ZoomSlider => {
            let zoom = panels::zoom_from_slider_x(x);
            app.viewport_mut().set_zoom(zoom);
            return Transition::None;
        }
        PanelButton::Minimap2x => {
            let current = app.hud().panels.minimap_2x;
            app.hud_mut().panels.minimap_2x = !current;
            return Transition::None;
        }
        PanelButton::MinimapTnt => {
            let current = app.hud().panels.minimap_tnt;
            app.hud_mut().panels.minimap_tnt = !current;
            return Transition::None;
        }
        PanelButton::Pause if app.local_active() => {
            // M-SP pause: `PauseMenu` — ticks and the turn timer stop.
            app.local_toggle_pause();
            return Transition::None;
        }
        PanelButton::Help => {
            // `GameManager_MenuClickHelpButton` → `HelpMenu_Menu("GAME_SCREEN_SETUP")`
            // (`WINDOW_MAIN_MAP`); `?`/Shift+/ in game does the same.
            app.open_help(HelpSection::GameScreen);
            app.hud_mut().push_log("Help: game screen");
            return Transition::None;
        }
        PanelButton::Preferences => {
            // `GameManager_MenuClickPreferencesButton` (`game_manager.cpp:3918`)
            // → `menu_preferences_window` (`OptionsMenu`, `PREFSPIC`).
            if app.open_preferences() {
                app.hud_mut().push_log("Preferences");
            } else {
                app.hud_mut()
                    .push_log("Preferences: close the open dialog first");
            }
            return Transition::None;
        }
        PanelButton::Files => {
            // `GameManager_MenuClickFileButton` (`Alt+F`) — the V70 window.
            app.open_save_menu(app.session_channel());
            return Transition::None;
        }
        PanelButton::Play
        | PanelButton::Pause
        | PanelButton::Lock
        | PanelButton::Prev
        | PanelButton::Done
        | PanelButton::Next
        | PanelButton::Reports
        | PanelButton::Chat => {
            // The screens of these buttons are not ported yet (items C/E of the plan).
            app.hud_mut()
                .push_log(format!("{button:?}: screen is not ported yet"));
            return Transition::None;
        }
        _ if button.is_toggle() => {
            let enabled = app.hud_mut().panels.toggle(button);
            let caption = button.caption().unwrap_or("toggle");
            app.hud_mut()
                .push_log(format!("{caption}: {}", if enabled { "on" } else { "off" }));
            return Transition::None;
        }
        _ => {}
    }

    Transition::None
}

/// `MAX_DEBUG_INPUT=1` — trace session input to stderr (diagnostics).
///
/// Without the environment variable no branch prints anything: the output does not
/// get into a normal run or QA/golden.
fn debug_input() -> bool {
    std::env::var_os("MAX_DEBUG_INPUT").is_some()
}

/// Selection/attack by a **map camera** point (448×448 with chrome).
fn session_map_left_click<D: Dispatcher>(
    app: &mut App<D>,
    viewport: &Viewport,
    x: i32,
    y: i32,
) -> Transition
where
    App<D>: SessionLocal,
{
    // `GameManager_ProcessPlayerInput` (`game_manager.cpp:6711`): releasing LMB
    // on the map — `PlaySfx(KCARG0)`.
    app.play_sfx(Sfx::Click);

    let team = app.player_team();
    let grid = viewport.screen_to_grid(x, y);
    if debug_input() {
        let at = app
            .state_ref()
            .units_at(max_core::geo::Point::new(grid.0, grid.1))
            .len();
        let origin = viewport.origin();
        eprintln!(
            "[click] map=({x},{y}) grid=({},{}) units_at={at} viewport_origin=({},{})",
            grid.0, grid.1, origin.0, origin.1
        );
    }
    let Some(id) = hud::unit_at_screen(app.state_ref(), viewport, team, x, y) else {
        // `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5832-5843`):
        // the selected builder in `ORDER_STATE_UNIT_READY` + a cell next to the
        // completed building (the `BLDMRK*` ring) — cursor `CURSOR_UNIT_GO`, and
        // LMB gives `ORDER_ACTIVATE`/`IN_TRANSITION` (`:6879-6890`) instead of
        // clearing the selection. The hint `_(276c)` asks to click exactly "the cell
        // with the symbol".
        let exit_cell = max_core::geo::Point::new(grid.0, grid.1);
        if let Some(selected) = app.hud().selected {
            if let Some(command) =
                hud::build_builder_exit_click_order(app.state_ref(), selected, exit_cell)
            {
                app.hud_mut().preview = None;
                app.hud_mut().pending.push(command);
                app.hud_mut().push_log(format!(
                    "Leave construction site: click ({}, {})",
                    exit_cell.x, exit_cell.y
                ));

                apply_session_orders(app);

                return Transition::None;
            }
        }

        // `GameManager_ProcessPlayerInput`: a click on the map closes the message
        // window (`MessageManager_ClearMessageBox`).
        app.hud_mut().clear_hint();

        // `targeting_mode` and an empty cell: `GameManager_SetUnitOrder`
        // (`game_manager.cpp:6360`) — if the target is not found and there are shots,
        // `fire_on = (x,y)` and `ORDER_FIRE`/`INIT` (area fire).
        if app.hud().targeting_mode {
            if let Some(selected) = app.hud().selected {
                let cell = max_core::geo::Point::new(grid.0, grid.1);
                let in_range = app.state_ref().unit(selected).is_some_and(|unit| {
                    app.state_ref().is_within_attack_range(
                        unit,
                        cell,
                        i32::from(
                            unit.base_values
                                .get_attribute(max_core::unit_values::ATTRIB_RANGE),
                        ),
                    )
                });

                if in_range {
                    match hud::build_ground_fire_order(app.state_ref(), selected, cell) {
                        Some(command) => {
                            app.hud_mut().pending.push(command);
                            app.hud_mut()
                                .push_log(format!("Attack ground: ({}, {})", grid.0, grid.1));
                            apply_session_orders(app);
                            return Transition::None;
                        }
                        None => {
                            app.hud_mut().push_log("Attack: нет выстрелов");
                            return Transition::None;
                        }
                    }
                }
            }
        }

        if app.hud().selected.is_some() {
            app.hud_mut().clear_interaction();
            app.hud_mut().push_log("Deselected");
        }
        return Transition::None;
    };

    let (unit_team, label) = match app.state_ref().unit(id) {
        Some(unit) => (unit.team, unit_label(unit)),
        None => return Transition::None,
    };

    if unit_team == team {
        let selected = app.hud().selected;

        // Cursor mode of the selected unit (C++ `UnitInfo::cursor` 4/6, command
        // menu): a click on an own unit gives `ORDER_REPAIR`/`ORDER_RELOAD`
        // (`GameManager_RepairUnit`/`GameManager_ReloadUnit`).
        let cursor = app.hud().cursor_mode;
        if cursor != 0 {
            if let Some(selected) = selected.filter(|selected| *selected != id) {
                // X-FER: `GameManager_ProcessMouse` → CURSOR_TRANSFER →
                // `GameManager_TransferCargo` → `TransferMenu_Menu`.
                if cursor == crate::unitmenu::CURSOR_TRANSFER {
                    match crate::transfermenu::resolve_target(app.state_ref(), selected, id) {
                        Ok(Some(target)) => {
                            if app.open_transfer_menu(selected, target) {
                                let label = app
                                    .state_ref()
                                    .unit(target)
                                    .map(unit_label)
                                    .unwrap_or_else(|| format!("unit {target:?}"));
                                app.hud_mut()
                                    .push_log(format!("x-fer: цель {label} — выберите количество"));
                            } else {
                                app.hud_mut().push_log("x-fer: окно не открылось");
                            }
                        }
                        Ok(None) => {
                            // C++ silently does nothing: the target has no
                            // compatible storage/cargo type.
                        }
                        Err(message) => app.hud_mut().push_log(message),
                    }
                    return Transition::None;
                }

                match crate::unitmenu::cursor_mode_click(app.state_ref(), selected, id, cursor) {
                    Ok(Some(command)) => {
                        app.hud_mut().pending.push(command);
                        let label = match cursor {
                            crate::unitmenu::CURSOR_REPAIR => "Repair",
                            _ => "Reload",
                        };
                        // `GameManager_RepairUnit`/`ReloadUnit`
                        // (`game_manager.cpp:4609/4626`): `V_M210`/`V_M085`.
                        app.play_sfx(if cursor == crate::unitmenu::CURSOR_REPAIR {
                            Sfx::Repair
                        } else {
                            Sfx::Reload
                        });
                        app.hud_mut().push_log(format!("{label}: приказ отдан"));
                        apply_session_orders(app);
                        return Transition::None;
                    }
                    Ok(None) => {}
                    Err(message) => {
                        // Unavailable action — `NCANC0` (`game_manager.cpp:4612`).
                        app.play_sfx(Sfx::Cancel);
                        app.hud_mut().push_log(message);
                        return Transition::None;
                    }
                }

                // `load` (`cursor = 7`) — `GameManager_ProcessMouse`,
                // `CURSOR_LOAD` branch (`game_manager.cpp:6807`).
                if cursor == crate::unitmenu::CURSOR_LOAD {
                    // C++ gives the order only when the cursor is `CURSOR_LOAD`
                    // (`load_mode_applies`); otherwise the target is not accepted.
                    let applies = match (app.state_ref().unit(selected), app.state_ref().unit(id)) {
                        (Some(transport), Some(passenger)) => {
                            crate::unitmenu::load_mode_applies(transport, passenger)
                        }
                        _ => false,
                    };

                    if !applies {
                        app.play_sfx(Sfx::Cancel);
                        app.hud_mut()
                            .push_log("load: цель не принимается транспортом");
                        return Transition::None;
                    }

                    match crate::unitmenu::load_mode_click(app.state_ref(), selected, id) {
                        Ok(Some(command)) => {
                            app.hud_mut().pending.push(command);
                            app.hud_mut()
                                .push_log(format!("load: погрузка ({label}) — приказ отдан"));
                            apply_session_orders(app);
                            return Transition::None;
                        }
                        Ok(None) => {}
                        Err(message) => {
                            app.hud_mut().push_log(message);
                            return Transition::None;
                        }
                    }
                }
            }
        }

        // `enter_mode` (C++ `UnitsManager_Popup_OnClick_Enter`): a click on an own
        // unit — `ORDER_MOVE_TO_UNIT` into its cell (boarding a transport).
        if app.hud().enter_mode {
            if let Some(selected) = selected.filter(|selected| *selected != id) {
                if let Some(command) =
                    crate::unitmenu::enter_mode_click(app.state_ref(), selected, id)
                {
                    app.hud_mut().pending.push(command);
                    app.hud_mut().enter_mode = false;
                    app.hud_mut()
                        .push_log(format!("Enter: move to unit ({label})"));
                    apply_session_orders(app);
                    return Transition::None;
                }
            }
            app.hud_mut().enter_mode = false;
        }

        // A repeated click on the already selected unit opens the command menu —
        // C++ `GameManager_ClickUnit` (`unit1 == unit2` → `InitPopupButtons`).
        if selected == Some(id) {
            app.hud_mut().preview = None;
            session_show_unit_status(app, id);

            let line = if app.open_unit_menu() {
                match app.unit_menu() {
                    Some(menu) => format!("Unit menu: {label} ({})", menu.items.len()),
                    // `Popup_InitFactories`: for a factory with a ready unit the menu does not
                    // open — `PlaceNewUnit` selected the ready unit.
                    None => format!("Launch {label}: click a cell next to it"),
                }
            } else {
                format!("Unit menu: no commands for {label}")
            };
            app.hud_mut().push_log(line);

            return Transition::None;
        }

        app.hud_mut().select(Some(id));
        app.hud_mut().preview = None;
        app.hud_mut().push_log(format!("Selected {label}"));
        // `GameManager_UnitSelect` → `GameManager_DrawUnitStatusMessage`:
        // the hint above the map about the current construction of the selected builder.
        session_show_unit_status(app, id);
        return Transition::None;
    }

    // Enemy: commando (cursor modes 8/9) or attack (packet 08, P3).
    let Some(selected) = app.hud().selected else {
        app.hud_mut()
            .push_log("Enemy unit — select your unit first");
        return Transition::None;
    };

    let cursor = app.hud().cursor_mode;
    if matches!(
        cursor,
        crate::unitmenu::CURSOR_STEAL | crate::unitmenu::CURSOR_DISABLE
    ) {
        // `GameManager_StealUnit`/`GameManager_DisableUnit`
        // (`game_manager.cpp:4680/4696`): `Randomizer_Generate(101)` goes into
        // packet 08/P3 (`stealth_dice_roll`), so the roll is generated here.
        //
        // B2 audit — command-payload generation, not a UI write: the value is
        // consumed once by the simulation RNG stream exactly as the C++ sender
        // consumes it, and the roll itself travels in the P3 command below (the
        // receiver applies `stealth_dice_roll` from the payload, it does not roll).
        let roll = app.universe.generate_rng_i32(101) as u8;

        match crate::unitmenu::stealth_mode_click(app.state_ref(), selected, id, cursor, roll) {
            Ok(Some(command)) => {
                app.hud_mut().pending.push(command);
                let label = if cursor == crate::unitmenu::CURSOR_STEAL {
                    "steal"
                } else {
                    "disable"
                };
                // `GameManager_StealUnit`/`DisableUnit` — target selection like KCARG0.
                app.play_sfx(Sfx::Click);
                app.hud_mut()
                    .push_log(format!("{label}: действие назначено"));
                apply_session_orders(app);
                return Transition::None;
            }
            Ok(None) => {
                app.play_sfx(Sfx::Cancel);
                app.hud_mut().push_log("Infiltrator: цель не подходит");
                return Transition::None;
            }
            Err(message) => {
                app.play_sfx(Sfx::Cancel);
                app.hud_mut().push_log(message);
                return Transition::None;
            }
        }
    }

    match hud::build_attack_order(app.state_ref(), selected, id) {
        Some(command) => {
            app.hud_mut().preview = None;
            app.hud_mut().pending.push(command);
            app.hud_mut().push_log(format!("Attack: {label}"));
            apply_session_orders(app);
        }
        None => app.hud_mut().push_log("Cannot attack this unit"),
    }

    Transition::None
}

/// `GameManager_DrawUnitStatusMessage` (`game_manager.cpp:8038`): the hint
/// above the map for the selected builder.
///
/// For a building unit (`ORDER_BUILD`) — «Currently building: X. / Turns to
/// completion: N.» (`ee24`), for a completed one (`ORDER_STATE_UNIT_READY`) —
/// «Finished building X. …» (`276c`). For other units the message window
/// is closed, like `MessageManager_ClearMessageBox` in `GameManager_UnitSelect`.
fn session_show_unit_status<D: Dispatcher>(app: &mut App<D>, id: max_core::unit::UnitId)
where
    App<D>: SessionLocal,
{
    let message = app
        .state_ref()
        .unit(id)
        .and_then(|unit| crate::panels::builder_status_message(app.state_ref(), unit));

    match message {
        Some(text) => app.hud_mut().set_hint(text),
        None => app.hud_mut().clear_hint(),
    }
}

/// Right button without chrome.
fn session_right_click<D: Dispatcher>(app: &mut App<D>, x: i32, y: i32) -> Transition
where
    App<D>: SessionLocal,
{
    let viewport = *app.viewport();

    session_map_right_click(app, &viewport, x, y)
}

/// Right button with chrome: first the panels (they give no order), then the map.
fn session_chrome_right_click<D: Dispatcher>(app: &mut App<D>, x: i32, y: i32) -> Transition
where
    App<D>: SessionLocal,
{
    if panels::panel_button_at(x, y).is_some() || max_render::layout::MINIMAP.contains(x, y) {
        return Transition::None;
    }
    let Some((map_x, map_y)) = panels::screen_to_map(x, y) else {
        return Transition::None;
    };
    let viewport = app.map_viewport();

    session_map_right_click(app, &viewport, map_x, map_y)
}

/// Right click on the map: cancel on a unit, otherwise a move order to the cell.
fn session_map_right_click<D: Dispatcher>(
    app: &mut App<D>,
    viewport: &Viewport,
    x: i32,
    y: i32,
) -> Transition
where
    App<D>: SessionLocal,
{
    let team = app.player_team();
    let (grid_x, grid_y) = viewport.screen_to_grid(x, y);
    let debug = debug_input();

    if debug {
        let origin = viewport.origin();
        eprintln!(
            "[rclick] map=({x},{y}) grid=({grid_x},{grid_y}) viewport_origin=({},{}) selected={:?}",
            origin.0,
            origin.1,
            app.hud().selected
        );
    }

    // «Cancel — Esc/right click on the unit».
    if let Some(id) = hud::unit_at_screen(app.state_ref(), viewport, team, x, y) {
        let label = app
            .state_ref()
            .unit(id)
            .map(unit_label)
            .unwrap_or_else(|| "unit".to_string());

        if debug {
            eprintln!("[rclick] result=cancel unit={} ({label})", id.0);
        }

        app.hud_mut().clear_interaction();
        app.hud_mut().push_log(format!("Cancelled ({label})"));
        return Transition::None;
    }

    let Some(selected) = app.hud().selected else {
        if debug {
            eprintln!("[rclick] result=no-selection");
        }
        app.hud_mut().push_log("No unit selected — LMB selects");
        return Transition::None;
    };

    let destination = GridPoint::new(grid_x, grid_y);

    if grid_x < 0
        || grid_y < 0
        || grid_x >= max_core::geo::MAP_WIDTH
        || grid_y >= max_core::geo::MAP_HEIGHT
    {
        if debug {
            eprintln!("[rclick] result=outside-map grid=({grid_x},{grid_y})");
        }
        app.hud_mut().push_log("Outside the map");
        return Transition::None;
    }

    // A ready unit in a container (factory/transport: `ORDER_IDLE`, the parent — the
    // container): a click on the adjacent cell — `CURSOR_ACTIVATE`, the `ORDER_ACTIVATE`
    // order goes to the container (`game_manager.cpp:6918-6926`),
    // `UnitsManager_ActivateUnit` brings the unit out. Checked **before**
    // `unit_can_move`: a ready aircraft in AIRPLT is also `ORDER_IDLE`.
    if let Some(command) = hud::build_activate_order(app.state_ref(), selected, destination) {
        app.hud_mut().preview = None;
        app.hud_mut().pending.push(command);
        app.hud_mut().push_log(format!(
            "Launch: click ({}, {})",
            destination.x, destination.y
        ));

        if debug {
            eprintln!(
                "[rclick] result=activate selected={} to=({},{})",
                selected.0, destination.x, destination.y
            );
        }

        apply_session_orders(app);

        return Transition::None;
    }

    // Aviation moves via `ORDER_MOVE`/`INIT` (packet 08/P3): the `AirPath` is built
    // by the simulation (`Paths_GetAirPath`), and `unit_can_move` deliberately
    // excludes `MOBILE_AIR_UNIT` (the ground `Packet38Move` does not suit it).
    if app
        .state_ref()
        .unit(selected)
        .is_some_and(|unit| unit.is_air())
    {
        match hud::build_air_move_order(app.state_ref(), selected, destination) {
            Some(command) => {
                app.hud_mut().preview = None;
                app.hud_mut().pending.push(command);
                app.hud_mut().push_log(format!(
                    "Move (air): ({}, {})",
                    destination.x, destination.y
                ));

                apply_session_orders(app);
            }
            None => {
                app.hud_mut().push_log("Air move refused");
            }
        }

        return Transition::None;
    }

    let can_move = app
        .state_ref()
        .unit(selected)
        .is_some_and(hud::unit_can_move);

    if !can_move {
        if debug {
            eprintln!("[rclick] result=cannot-move selected={}", selected.0);
        }
        app.hud_mut().push_log("Unit cannot move (air/stationary)");
        return Transition::None;
    }

    // A completed builder stands on the site (`ORDER_BUILD`/`UNIT_READY`,
    // building `ORDER_IDLE`/`BUILDING_READY`): the click gives `ORDER_ACTIVATE`/
    // `IN_TRANSITION` — exactly `game_manager.cpp:6879-6890`. The path is not built
    // (C++ sets only `move_to`), so `build_move_order` is not called
    // here.
    if let Some(command) = hud::build_builder_exit_order(app.state_ref(), selected, destination) {
        app.hud_mut().preview = None;
        app.hud_mut().pending.push(command);
        app.hud_mut().push_log(format!(
            "Leave construction site: click ({}, {})",
            destination.x, destination.y
        ));

        if debug {
            eprintln!(
                "[rclick] result=leave-site selected={} to=({},{})",
                selected.0, destination.x, destination.y
            );
        }

        apply_session_orders(app);

        return Transition::None;
    }

    // A builder on an unfinished site: C++ gives no move order
    // (`GameManager_GetBuilderUnitCursor`, `game_manager.cpp:5927-5931` —
    // `CURSOR_UNIT_NO_GO` for all states except `UNIT_READY`/`SELECT_SITE`).
    // It can only be taken away by an explicit `stop` in the popup
    // (`UnitsManager_Popup_OnClick_BuildStop` → `ORDER_BUILD`/`BUILD_CANCEL`).
    if let Some(site) = app
        .state_ref()
        .unit(selected)
        .and_then(max_core::unit_order::active_build_site)
    {
        app.hud_mut().preview = None;
        app.play_sfx(Sfx::Cancel);
        app.hud_mut().push_log(format!(
            "Cannot move while building at ({}, {}) — use stop",
            site.x, site.y
        ));

        if debug {
            eprintln!(
                "[rclick] result=no-go-building selected={} site=({},{})",
                selected.0, site.x, site.y
            );
        }

        return Transition::None;
    }

    match hud::build_move_order(app.state_ref(), selected, destination) {
        Some(order) => {
            let steps = order.preview.len();
            app.hud_mut().preview = Some(order.preview);
            app.hud_mut().pending.push(order.command);
            app.hud_mut().push_log(format!(
                "Move: {steps} steps to ({}, {})",
                destination.x, destination.y
            ));

            if debug {
                eprintln!(
                    "[rclick] result=move selected={} steps={steps} to=({},{})",
                    selected.0, destination.x, destination.y
                );
            }

            apply_session_orders(app);
        }
        None => {
            if debug {
                eprintln!(
                    "[rclick] result=no-path selected={} to=({},{})",
                    selected.0, destination.x, destination.y
                );
            }
            app.hud_mut().preview = None;
            app.hud_mut()
                .push_log(format!("No path to ({}, {})", destination.x, destination.y));
        }
    }

    Transition::None
}

/// End Turn: in a local match — the AI turn, in a lockstep session — a flag for the
/// game loop (`06`+`52` there), in demo — a local pass of [`App::end_turn`].
fn session_end_turn<D: Dispatcher>(app: &mut App<D>)
where
    App<D>: SessionLocal,
{
    if app.local_active() {
        app.local_end_turn();
    } else if app.session_channel().is_lockstep() {
        app.hud_mut().request_end_turn();
        app.hud_mut().push_log("End Turn requested");
    } else {
        app.end_turn();
    }
}

/// Applies this frame's orders through the session capability (SDL/UI Stage C2).
///
/// A local match applies them now (`apply_now`) and ticks them in the same frame; a
/// lockstep match leaves them in [`crate::hud::HudState::pending`] for the session
/// driver (`net::flush_hud_orders` → `submit`).
fn apply_session_orders<D: Dispatcher>(app: &mut App<D>)
where
    App<D>: SessionLocal,
{
    if app.session_channel().is_local() {
        apply_pending_orders(app);
    }
}

/// Applies the accumulated orders locally (local match/demo) and logs it.
fn apply_pending_orders<D: Dispatcher>(app: &mut App<D>)
where
    App<D>: SessionLocal,
{
    let applied = app.flush_orders();

    if applied > 0 {
        let line = format!("{applied} order(s) applied locally");
        app.hud_mut().push_log(line);
    }
}

/// Unit caption for the log: the name (if set) and the type.
///
/// Without an own name (`unit.name`) the caption is the type UFN
/// ([`crate::unit_names::unit_user_friendly_name`]), not the variant's `Debug` name.
fn unit_label(unit: &UnitInfo) -> String {
    if unit.name.is_empty() {
        crate::unit_names::unit_user_friendly_name(unit.unit_type).to_string()
    } else {
        format!("{} [{:?}]", unit.name, unit.unit_type)
    }
}

/// Session frame: the map from [`App::render_frame`] + the HUD (M4.2) over it.
///
/// If the 1:1 window chrome (`FRAMEPIC`) is loaded and the frame is 640×480, the original HUD is already
/// drawn in the picture itself — the procedural HUD is not drawn, only the status line is output
/// over the map area. Without chrome — the former look.
///
/// Without `App` (headless rendering before the network) the former background with the status line remains.
fn render_session<D: Dispatcher>(buffer: &mut RgbaBuffer, view: ScreenView<'_, D>)
where
    App<D>: SessionLocal,
{
    if let Some(app) = view.app {
        let frame = app.render_frame();
        blit(buffer, &frame);

        let chrome_active = app.chrome_active();

        if chrome_active {
            // The map area already contains tiles, units and panels; we write only
            // the message line in the top-left corner of the map — in the GNW font, like
            // `MessageManager_DrawMessage`.
            let area = max_render::layout::MAIN_MAP;
            let status: String = view.status.chars().take(70).collect();

            // Open `BuildMenu` — a modal screen over the frame: the match status
            // line does not cover it (like `GameManager_DisableMainMenu`).
            // The save/load window (P3) — the same modal.
            // The extraction allocation dialog (`AllocMenu`) occupies the whole 640×480
            // frame (`ALLOCFRM`), so the status line is not drawn over
            // it — like a modal C++ window.
            if app.build_menu().is_none() && app.save_load().is_none() && app.alloc_menu().is_none()
            {
                if let Some(assets) = app.assets() {
                    max_render::text::draw_text_gnw_shadowed(
                        buffer,
                        area.ulx + 4,
                        area.uly + 4,
                        max_render::text::GnwFontId::Font5.font(),
                        &status,
                        assets.palette.rgba(0xA0),
                        assets.palette.rgba(0xE6),
                        assets.palette.rgba(0x3F),
                    );
                }
            }

            // M-HS: the turn-transition window over the frame — modal, like `DialogMenu`.
            draw_local_notice(buffer, app, view.mouse);
            return;
        }

        let hud_view = hud::HudView {
            state: app.state_ref(),
            hud: app.hud(),
            viewport: app.viewport(),
            team: app.player_team(),
            status: view.status,
        };

        if app.build_menu().is_none() && app.save_load().is_none() {
            hud::render_hud(buffer, &hud_view);
        }
        draw_local_notice(buffer, app, view.mouse);
        return;
    }

    buffer.fill(Rgba::rgb(8, 12, 20));

    let bar_height = (GLYPH_HEIGHT + 8).max(16);
    let y = view.height as i32 - bar_height;

    if y >= 0 {
        buffer.fill_rect(
            PixelRect::new(0, y, view.width as i32, bar_height),
            Rgba::rgb(8, 12, 20),
        );
        draw_text_line(buffer, 4, y + 4, view.status, Rgba::rgb(232, 236, 244));
    }
}

/// M-HS turn-transition window over the session frame (`DialogMenu`).
///
/// Drawn only when `LocalGame::notice` is open — i.e. in hot-seat on
/// a human team change (`GameManager_UpdateGuiControl`).
fn draw_local_notice<D: Dispatcher>(buffer: &mut RgbaBuffer, app: &App<D>, mouse: Option<Point>)
where
    App<D>: SessionLocal,
{
    let Some(notice) = app.local_notice() else {
        return;
    };
    let Some(assets) = app.assets() else {
        return;
    };
    let Some(menu) = assets.menu.as_ref() else {
        return;
    };
    let Some(palette) = crate::panels::UiPalette::from_assets(assets) else {
        return;
    };

    crate::hotseat::draw_turn_notice(
        buffer,
        menu,
        &palette,
        notice,
        mouse.map(|point| (point.x, point.y)),
    );
}

/// Copies the `src` frame into `dst` clipped by the smaller size.
fn blit(dst: &mut RgbaBuffer, src: &RgbaBuffer) {
    let width = dst.width().min(src.width()) as usize;
    let height = dst.height().min(src.height()) as usize;

    if width == 0 || height == 0 {
        return;
    }

    let src_stride = src.stride();
    let dst_stride = dst.stride();
    let row_bytes = width * BYTES_PER_PIXEL;
    let src_pixels = src.pixels();
    let dst_pixels = dst.pixels_mut();

    for row in 0..height {
        let src_start = row * src_stride;
        let dst_start = row * dst_stride;
        dst_pixels[dst_start..dst_start + row_bytes]
            .copy_from_slice(&src_pixels[src_start..src_start + row_bytes]);
    }
}

// ---------------------------------------------------------------------------
// Screen layout
// ---------------------------------------------------------------------------

fn horizontal_margin(width: i32) -> i32 {
    (width / 12).clamp(12, 48)
}

fn build_main_menu(
    width: u32,
    height: u32,
    status: &str,
    mouse: Option<Point>,
    focus: usize,
) -> UiState {
    let width = width as i32;
    let height = height as i32;
    let theme = UiTheme::default();
    let native_frame = width == menu::MENU_WIDTH && height == menu::MENU_HEIGHT;
    let mut widgets = Vec::with_capacity(12);

    if native_frame {
        // Seven original buttons: indices 0..=6, of which the Rust client actions are —
        // 0 (New Game), 2 (Multiplayer Game), 6 (Exit).
        for button in &menu::MAIN_MENU_BUTTONS {
            widgets.push(Widget::Button(Button::new(
                button.label,
                Rect::new(
                    button.rect.ulx,
                    button.rect.uly,
                    button.rect.width,
                    button.rect.height,
                ),
            )));
        }

        widgets.push(Widget::Label(Label::colored(
            "Main Menu",
            Rect::new(236, 145, 172, 15),
            theme.accent,
        )));
        widgets.push(Widget::Label(Label::colored(
            status,
            Rect::new(10, 452, 370, 12),
            theme.accent,
        )));
        widgets.push(Widget::Label(Label::colored(
            "Up/Down + Enter or mouse; C/J/I/R/Q; Esc — quit",
            Rect::new(10, 436, 370, 12),
            theme.text_disabled,
        )));
    } else {
        let margin = horizontal_margin(width);
        let content = (width - margin * 2).max(60);
        let x = (width - content) / 2;
        let heights = [24, 12, 28, 28, 28, 12];
        let gap = 8;
        let total = heights.iter().sum::<i32>() + gap * (heights.len() as i32 - 1);
        let top = ((height - total) / 2).max(8);
        let rects = layout(Rect::new(x, top, content, total), &heights, gap);
        let empty = Rect::new(0, 0, 0, 0);

        // The indices must match the original order (`MAIN_*`):
        // 0 — Create, 1 — Load, 2 — Join, 3..=5 — not ported, 6 — Exit.
        let buttons = [
            ("New Game", rects[2]),
            ("Load Game", empty),
            ("Multiplayer Game", rects[3]),
            ("Setup", empty),
            ("Introduction", empty),
            ("Credits", empty),
            ("Exit", rects[4]),
        ];

        for (label, rect) in buttons {
            let enabled = !rect.is_empty();
            widgets.push(Widget::Button(
                Button::new(label, rect).with_enabled(enabled),
            ));
        }

        widgets.push(Widget::Label(Label::colored(
            "M.A.X.",
            rects[0],
            theme.accent,
        )));
        widgets.push(Widget::Label(Label::colored(
            "Rust client — M4.1 UI",
            rects[1],
            theme.text_disabled,
        )));
        widgets.push(Widget::Label(Label::colored(
            "Up/Down + Enter or mouse; C/J/I/R/Q; Esc — quit",
            rects[5],
            theme.text_disabled,
        )));
        widgets.push(Widget::Label(Label::colored(
            status,
            Rect::new(margin, (height - 18).max(0), content, 12),
            theme.accent,
        )));
    }

    let mut ui = UiState::new(widgets);
    ui.focus = Some(MAIN_ACTIONS[focus.min(MAIN_ACTIONS.len() - 1)]);
    ui.set_hover(mouse);
    ui
}

fn build_join_list(
    width: u32,
    height: u32,
    status: &str,
    mouse: Option<Point>,
    games: &[GameInfo],
    selected: usize,
) -> UiState {
    let width = width as i32;
    let height = height as i32;
    let theme = UiTheme::default();
    let native_frame = width == menu::MENU_WIDTH && height == menu::MENU_HEIGHT;
    let items: Vec<String> = games
        .iter()
        .map(|game| {
            let lock = if game.password_protected { " *" } else { "" };
            format!(
                "{}  [{}]  {}/{}{}",
                game.name,
                game.status.as_ws_str(),
                game.players,
                game.min_players,
                lock
            )
        })
        .collect();
    let empty = Rect::new(0, 0, 0, 0);

    let (list_rect, row_height, back, refresh, join, hide, help) = if native_frame {
        (
            Rect::new(
                menu::JOIN_LIST.ulx,
                menu::JOIN_LIST.uly,
                menu::JOIN_LIST.width,
                menu::JOIN_LIST.height,
            ),
            menu::JOIN_ROW_HEIGHT,
            Rect::new(354, 438, 109, 40),
            Rect::new(243, 438, 109, 40),
            Rect::new(514, 438, 109, 40),
            Rect::new(
                menu::JOIN_HIDE.ulx,
                menu::JOIN_HIDE.uly,
                menu::JOIN_HIDE.width,
                menu::JOIN_HIDE.height,
            ),
            Rect::new(465, 438, 48, 40),
        )
    } else {
        let margin = horizontal_margin(width);
        let content = (width - margin * 2).max(60);
        let x = (width - content) / 2;
        let buttons_y = (height - 72).max(44);
        let list_height = (buttons_y - 6 - 30).max(20);

        (
            Rect::new(x, 30, content, list_height),
            14,
            Rect::new(x, buttons_y, 100, 26),
            Rect::new(x + 108, buttons_y, 100, 26),
            Rect::new(x + content - 100, buttons_y, 100, 26),
            empty,
            empty,
        )
    };

    let mut list = ListBox::with_items(list_rect, items);
    list.row_height = row_height;
    list.selected = (!games.is_empty()).then_some(selected.min(games.len().saturating_sub(1)));
    list.scroll = menu::join_scroll(selected);
    list.ensure_visible();

    let title = if native_frame {
        Rect::new(
            menu::JOIN_TITLE.ulx,
            menu::JOIN_TITLE.uly,
            menu::JOIN_TITLE.width,
            menu::JOIN_TITLE.height,
        )
    } else {
        Rect::new(back.x, 12, back.width.max(100) * 4, 12)
    };
    let hint = if native_frame {
        Rect::new(150, 418, 462, 12)
    } else {
        Rect::new(back.x, (height - 38).max(0), back.width.max(100) * 4, 12)
    };
    let status_rect = if native_frame {
        Rect::new(
            menu::JOIN_STATUS.ulx,
            menu::JOIN_STATUS.uly,
            menu::JOIN_STATUS.width,
            menu::JOIN_STATUS.height,
        )
    } else {
        Rect::new(back.x, (height - 18).max(0), back.width.max(100) * 4, 12)
    };

    let widgets = vec![
        Widget::Label(Label::colored("JOIN GAME", title, theme.accent)),
        Widget::ListBox(list),
        Widget::Button(Button::new("Cancel", back)),
        Widget::Button(Button::new("Options", refresh)),
        Widget::Button(Button::new("Ready", join).with_enabled(!games.is_empty())),
        Widget::Button(Button::new("Hide", hide).with_enabled(!hide.is_empty())),
        Widget::Button(Button::new("?", help).with_enabled(!help.is_empty())),
        Widget::Label(Label::colored(
            "Up/Down, Enter — join; R — refresh; Esc — back",
            hint,
            theme.text_disabled,
        )),
        Widget::Label(Label::colored(status, status_rect, theme.accent)),
    ];

    let mut ui = UiState::new(widgets);
    ui.focus = Some(JOIN_LIST);
    ui.set_hover(mouse);
    ui
}

/// Number of planets in the original (`PlanetSelectMenu` — 24, `PlanetType`).
pub const PLANET_COUNT: usize = 24;
/// Columns in the planet grid.
pub const PLANET_COLUMNS: i32 = 8;
/// Rows in the planet grid.
pub const PLANET_ROWS: i32 = 3;
/// Minimum window height at which the planet grid is shown.
pub const PLANET_GRID_MIN_HEIGHT: u32 = 320;

/// Names of the 24 planets — `_(e43b)`…`_(ba99)` from `planetselectmenu.cpp:120-122`
/// (`language.json`, en-US).
pub const PLANET_NAMES: [&str; PLANET_COUNT] = [
    "Snowcrab",
    "Frigia",
    "Ice Berg",
    "The Cooler",
    "Ultima Thule",
    "Long Floes",
    "Iron Cross",
    "Splatterscape",
    "Peak-a-boo",
    "Valentine's Planet",
    "Three Rings",
    "Great divide",
    "New Luzon",
    "Middle Sea",
    "High Impact",
    "Sanctuary",
    "Islandia",
    "Hammerhead",
    "Freckles",
    "Sandspit",
    "Great Circle",
    "Long Passage",
    "Flash Point",
    "Bottleneck",
];

/// Planet name (`planetselectmenu.cpp:138`).
pub fn planet_name(planet: Planet) -> &'static str {
    PLANET_NAMES
        .get(planet as usize)
        .copied()
        .unwrap_or("Unknown")
}

/// Planet by grid index `0..24` (`Planet::try_from`).
pub fn planet_from_index(index: usize) -> Option<Planet> {
    (index < PLANET_COUNT)
        .then(|| Planet::try_from(index as u8).ok())
        .flatten()
}

/// Next "random" planet for the `Random` button.
///
/// The original takes `Randomizer_Generate(24)`; in the Rust client the choice is deterministic
/// (without a shared UI RNG), so that frames and tests are reproducible.
pub fn random_planet(current: i64) -> i64 {
    ((current.clamp(0, MAX_WORLD) * 7 + 5) % (MAX_WORLD + 1)).clamp(0, MAX_WORLD)
}

/// Horizontal form margins.
fn form_content(width: i32) -> (i32, i32) {
    let margin = horizontal_margin(width);
    let content = (width - margin * 2).max(60);
    let x = (width - content) / 2;

    (x, content)
}

/// Planet grid rectangle inside the form (24 cells 8×3).
///
/// In a 640×480 frame — the exact 3×2 `PLANETSE` slot area (`192,51 388×249`),
/// otherwise — the former adaptive layout for tests/small frames.
pub fn planet_grid_rect(width: u32, height: u32) -> Rect {
    if width as i32 == menu::MENU_WIDTH && height as i32 == menu::MENU_HEIGHT {
        return menu::planet_grid_rect();
    }

    let width = width as i32;
    let height = height as i32;
    let (x, content) = form_content(width);
    let cell_height = ((height - 220) / PLANET_ROWS).clamp(30, 56);

    Rect::new(x, 24, content, cell_height * PLANET_ROWS)
}

/// Form field rectangle (the original 3×2 layout in the PLANETSE description).
pub fn create_field_rect(field: CreateField) -> Rect {
    menu::CREATE_FIELD_RECTS
        .iter()
        .find(|(item, _)| *item == field)
        .map(|(_, rect)| Rect::new(rect.ulx, rect.uly, rect.width, rect.height))
        .unwrap_or(Rect::new(0, 0, 0, 0))
}

/// Rectangle of one planet in the grid.
pub fn planet_cell_rect(width: u32, height: u32, index: usize) -> Rect {
    let grid = planet_grid_rect(width, height);
    let cell_width = (grid.width / PLANET_COLUMNS).max(1);
    let cell_height = (grid.height / PLANET_ROWS).max(1);
    let column = index as i32 % PLANET_COLUMNS;
    let row = index as i32 / PLANET_COLUMNS;

    Rect::new(
        grid.x + column * cell_width,
        grid.y + row * cell_height,
        cell_width,
        cell_height,
    )
}

/// Planet under the point (`None` — the point is outside the grid).
pub fn planet_cell_at(width: u32, height: u32, x: i32, y: i32) -> Option<usize> {
    for index in 0..PLANET_COUNT {
        let rect = planet_cell_rect(width, height, index);

        if x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height {
            return Some(index);
        }
    }

    None
}

fn build_create_form(
    width: u32,
    height: u32,
    status: &str,
    mouse: Option<Point>,
    form: &CreateForm,
) -> UiState {
    let width_i = width as i32;
    let height_i = height as i32;
    let (x, content) = form_content(width_i);
    let native_frame = width_i == menu::MENU_WIDTH && height_i == menu::MENU_HEIGHT;
    let theme = UiTheme::default();
    let empty = Rect::new(0, 0, 0, 0);

    let values = [
        (CreateField::Name, 24usize),
        (CreateField::World, 2),
        (CreateField::Timer, 4),
        (CreateField::EndTurn, 4),
        (CreateField::Opponent, 1),
        (CreateField::Password, 16),
    ];

    let (title_text, field_rects, back, create, random, help, hint_rect, status_rect) =
        if native_frame {
            (
                "Planet Select Menu",
                CreateField::ALL
                    .iter()
                    .map(|field| create_field_rect(*field))
                    .collect::<Vec<_>>(),
                Rect::new(354, 438, 109, 40),
                Rect::new(514, 438, 109, 40),
                Rect::new(243, 438, 109, 40),
                Rect::new(465, 438, 48, 40),
                Rect::new(41, 424, 559, 12),
                Rect::new(
                    menu::PLANET_STATUS.ulx,
                    menu::PLANET_STATUS.uly,
                    menu::PLANET_STATUS.width,
                    menu::PLANET_STATUS.height,
                ),
            )
        } else {
            let show_planets = height_i >= PLANET_GRID_MIN_HEIGHT as i32;
            let grid = planet_grid_rect(width, height);
            let fields_top = if show_planets {
                grid.y + grid.height + 6
            } else {
                28
            };
            let field_gap = 4;
            let field_height = ((height_i - fields_top - 88) / 6).clamp(12, 22);
            let rects = layout(
                Rect::new(x, fields_top, content, 0),
                &[field_height; 6],
                field_gap,
            );
            let buttons_y = fields_top + 6 * (field_height + field_gap) + 2;

            (
                if show_planets {
                    "NEW GAME - choose a planet"
                } else {
                    "NEW GAME - settings"
                },
                rects,
                Rect::new(x, buttons_y, 100, 24),
                Rect::new(x + content - 100, buttons_y, 100, 24),
                empty,
                empty,
                Rect::new(x, buttons_y + 32, content, 12),
                Rect::new(x, (height_i - 18).max(0), content, 12),
            )
        };

    let mut widgets = vec![Widget::Label(Label::colored(
        title_text,
        if native_frame {
            Rect::new(230, 6, 180, 20)
        } else {
            Rect::new(x, 10, content, 12)
        },
        theme.accent,
    ))];

    for ((field, max_chars), rect) in values.into_iter().zip(field_rects.iter().copied()) {
        let placeholder = match field {
            CreateField::Name => DEFAULT_GAME_NAME,
            CreateField::World => "0..23 (click a planet)",
            CreateField::Timer => "seconds",
            CreateField::EndTurn => "seconds",
            CreateField::Opponent => "1..3",
            CreateField::Password => "(open game)",
        };
        let text_box = TextBox::with_value(field.label(), form.value(field), placeholder, rect)
            .with_max_chars(max_chars);

        widgets.push(Widget::TextBox(text_box));
    }

    widgets.push(Widget::Button(Button::new("Cancel", back)));
    widgets.push(Widget::Button(Button::new("Done", create)));
    widgets.push(Widget::Button(
        Button::new("Random", random).with_enabled(!random.is_empty()),
    ));
    widgets.push(Widget::Button(
        Button::new("?", help).with_enabled(!help.is_empty()),
    ));

    // Planet labels are drawn in `draw_planet_picker` with the GNW Font5
    // (in C++ the planet names are `GNW_TEXT_FONT_5`), not 8×8 Labels: otherwise
    // «Valentine's Planet» is truncated to 8 characters.

    widgets.push(Widget::Label(Label::colored(
        if native_frame {
            "Click a planet; Tab/World - number; Enter - create; Esc - back"
        } else if height_i >= PLANET_GRID_MIN_HEIGHT as i32 {
            "Click a planet; Tab/World - number; Enter - create; Esc - back"
        } else {
            "Tab/Up/Down - field; Left/Right - numbers; Enter - create; Esc - back"
        },
        hint_rect,
        theme.text_disabled,
    )));
    widgets.push(Widget::Label(Label::colored(
        status,
        status_rect,
        theme.accent,
    )));

    let mut ui = UiState::new(widgets);
    ui.focus = Some(1 + form.focus.index());
    ui.set_hover(mouse);
    ui
}

/// Draws planet pictures into the form grid (`SNOW_PIC + world/6`).
///
/// `render_ui` fills the background and labels, so the pictures are put on top — in
/// the area above the label that the widgets do not occupy. Without assets, placeholder
/// frames are drawn so that the grid is visible in headless tests too.
fn draw_planet_picker(
    buffer: &mut RgbaBuffer,
    assets: Option<&crate::Assets>,
    form: &CreateForm,
    width: u32,
    height: u32,
) {
    if height < PLANET_GRID_MIN_HEIGHT {
        return;
    }

    let theme = UiTheme::default();

    for index in 0..PLANET_COUNT {
        let cell = planet_cell_rect(width, height, index);
        // A square thumbnail: `SNOW_PIC` — 128×128, stretching it by the cell
        // width is impossible (the planet turns into a pancake).
        let size = (cell.width - 4).min(cell.height - 12).max(1);
        let image_rect = Rect::new(cell.x + (cell.width - size) / 2, cell.y + 1, size, size);
        let planet = Planet::try_from(index as u8).ok();
        let picture =
            planet.and_then(|planet| assets.and_then(|assets| assets.planet_picture(planet)));

        if let Some(picture) = picture {
            blit_big_image_scaled(buffer, picture, image_rect);
        } else {
            buffer.fill_rect(image_rect.into(), theme.panel);
            draw_text_line(
                buffer,
                image_rect.x + 3,
                image_rect.y + 3,
                &format!("{index}"),
                theme.text_disabled,
            );
        }

        // Planet name — GNW Font5 (like `planetselectmenu.cpp:124-138`),
        // centered under the thumbnail; a long name is clipped to the cell width.
        if let Some(planet) = planet {
            let font = GnwFontId::Font5.font();
            let max_width = (cell.width - 2).max(1);
            let mut name = planet_name(planet).to_string();

            while font.text_width(&name) > max_width {
                if name.pop().is_none() {
                    break;
                }
            }

            let x = cell.x + (cell.width - font.text_width(&name)).max(0) / 2;
            let y = cell.y + cell.height - font.height();

            draw_text_gnw(buffer, x, y, font, &name, theme.text);
        }

        let selected = form.world == index as i64;
        let border = if selected {
            Rgba::rgb(252, 0, 0)
        } else {
            theme.border
        };
        let thickness = if selected { 2 } else { 1 };

        draw_rect_border(buffer, cell, border, thickness);
    }
}

/// `thickness` frame inside `rect`.
fn draw_rect_border(buffer: &mut RgbaBuffer, rect: Rect, color: Rgba, thickness: i32) {
    let thickness = thickness.max(1);

    for inset in 0..thickness {
        let x = rect.x + inset;
        let y = rect.y + inset;
        let width = rect.width - inset * 2;
        let height = rect.height - inset * 2;

        if width <= 0 || height <= 0 {
            break;
        }

        for dx in 0..width {
            buffer.set(x + dx, y, color);
            buffer.set(x + dx, y + height - 1, color);
        }
        for dy in 0..height {
            buffer.set(x, y + dy, color);
            buffer.set(x + width - 1, y + dy, color);
        }
    }
}

/// Scales a big image into `rect` (nearest neighbor), skipping the transparent
/// index — planet pictures 128×128 into a 56×40 cell.
fn blit_big_image_scaled(buffer: &mut RgbaBuffer, image: &max_assets::BigImage, rect: Rect) {
    if rect.width <= 0 || rect.height <= 0 || image.width <= 0 || image.height <= 0 {
        return;
    }

    for y in 0..rect.height {
        let source_y = (y * i32::from(image.height) / rect.height) as u32;

        for x in 0..rect.width {
            let source_x = (x * i32::from(image.width) / rect.width) as u32;
            let Some(index) = image.pixel(source_x, source_y) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            let [r, g, b] = image.color(index);

            buffer.set(rect.x + x, rect.y + y, Rgba::rgb(r, g, b));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::rc::Rc;

    use max_net::GameStatus;

    use crate::AppConfig;

    #[derive(Default)]
    struct MockLog {
        list_calls: usize,
        create_calls: usize,
        last_params: Option<CreateGameParams>,
    }

    struct MockClient {
        log: Rc<RefCell<MockLog>>,
        games: Vec<GameInfo>,
        created: Option<GameInfo>,
        list_error: Option<String>,
        create_error: Option<String>,
    }

    impl MockClient {
        fn new(games: Vec<GameInfo>, created: Option<GameInfo>) -> (Self, Rc<RefCell<MockLog>>) {
            let log = Rc::new(RefCell::new(MockLog::default()));
            let client = Self {
                log: Rc::clone(&log),
                games,
                created,
                list_error: None,
                create_error: None,
            };

            (client, log)
        }
    }

    impl MenuClient for MockClient {
        fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
            self.log.borrow_mut().list_calls += 1;
            self.list_error
                .clone()
                .map_or_else(|| Ok(self.games.clone()), Err)
        }

        fn create_game(&mut self, params: CreateGameParams) -> Result<GameInfo, String> {
            self.log.borrow_mut().create_calls += 1;
            self.log.borrow_mut().last_params = Some(params);
            self.create_error.clone().map_or_else(
                || {
                    self.created
                        .clone()
                        .ok_or_else(|| "mock: no created game".to_string())
                },
                Err,
            )
        }
    }

    fn game(id: &str, name: &str) -> GameInfo {
        GameInfo {
            id: id.to_string(),
            name: name.to_string(),
            world: 3,
            status: GameStatus::Running,
            port: 31554,
            players: 1,
            min_players: 2,
            password_protected: false,
        }
    }

    /// Backend that counts `play` calls (sound in tests without a device).
    #[derive(Debug)]
    struct CountingBackend {
        calls: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl crate::audio::AudioBackend for CountingBackend {
        fn name(&self) -> &'static str {
            "counting"
        }

        fn play(
            &mut self,
            _clip: std::sync::Arc<max_assets::WavClip>,
            _mode: crate::audio::PlayMode,
        ) -> Result<(), String> {
            *self.calls.lock().expect("lock") += 1;

            Ok(())
        }
    }

    fn context(
        games: Vec<GameInfo>,
        created: Option<GameInfo>,
    ) -> (UiContext, Rc<RefCell<MockLog>>) {
        let (client, log) = MockClient::new(games, created);
        (UiContext::new(Box::new(client), 512, 384), log)
    }

    fn key(screen: &mut Screen, ctx: &mut UiContext, key: Key) -> Transition {
        screen.handle_input(ctx, InputEvent::Key(key))
    }

    fn char(screen: &mut Screen, ctx: &mut UiContext, c: char) -> Transition {
        key(screen, ctx, Key::Char(c))
    }

    #[test]
    fn main_menu_opens_the_local_setup_and_escape_goes_back() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::MainMenu;

        // M-SP: «New Game» opens the `New Game` submenu (`menu.cpp:2171`).
        assert_eq!(key(&mut screen, &mut ctx, Key::Enter), Transition::None);
        assert_eq!(screen, Screen::NewGameMenu { tips: None });

        // `Custom Game` (`_(3d29)`) opens the local match setup (Options).
        let custom = newgame::new_game_rect(NewGameAction::Custom).expect("Custom Game");
        screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(
                custom.ulx + custom.width / 2,
                custom.uly + custom.height / 2,
            ),
        );
        assert!(matches!(screen, Screen::LocalSetup(_)));
        if let Screen::LocalSetup(form) = &screen {
            assert_eq!(form.screen, crate::setup::SetupScreen::Options);
        }

        // Enter leads to the next screen (Planet).
        key(&mut screen, &mut ctx, Key::Enter);
        if let Screen::LocalSetup(form) = &screen {
            assert_eq!(form.screen, crate::setup::SetupScreen::Planet);
        }

        // Escape returns to the `New Game` submenu (`menu_custom_game_menu` cancel
        // continues the `menu_new_game_menu_loop`), the second Escape to the main menu.
        key(&mut screen, &mut ctx, Key::Escape);
        assert_eq!(screen, Screen::NewGameMenu { tips: None });
        key(&mut screen, &mut ctx, Key::Escape);
        assert_eq!(screen, Screen::MainMenu);
    }

    #[test]
    fn multiplayer_menu_opens_the_hot_seat_setup() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::MainMenu;

        // Main menu → «Multiplayer Game» (`menu.cpp:2361`).
        char(&mut screen, &mut ctx, 'j');
        assert_eq!(screen, Screen::MultiplayerMenu);

        // The `Hot Seat Game` button (385,280) 200×29.
        let button = menu::MULTIPLAYER_BUTTONS[2];
        let transition = screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(
                button.rect.ulx + button.rect.width / 2,
                button.rect.uly + button.rect.height / 2,
            ),
        );

        assert_eq!(transition, Transition::None);
        match &screen {
            Screen::LocalSetup(form) => {
                assert!(form.hot_seat);
                assert_eq!(form.players_title(), "Hot Seat Menu");
                assert_eq!(form.setup.human_count(), 2);
                assert_eq!(form.screen, crate::setup::SetupScreen::Options);
            }
            other => panic!("ожидалась настройка hot-seat, получено {other:?}"),
        }

        // Esc from the first screen returns to `Multiplayer Menu`, not to the main one.
        key(&mut screen, &mut ctx, Key::Escape);
        assert_eq!(screen, Screen::MultiplayerMenu);

        // Original hotkey `Shift+O` (`GNW_KB_KEY_SHIFT_O`).
        char(&mut screen, &mut ctx, 'o');
        assert!(matches!(screen, Screen::LocalSetup(_)));

        // `Cancel` (`Shift+C`) returns to the main menu.
        key(&mut screen, &mut ctx, Key::Escape);
        key(&mut screen, &mut ctx, Key::Escape);
        assert_eq!(screen, Screen::MainMenu);
    }

    #[test]
    fn planet_click_changes_world_and_keeps_numeric_fallback() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::CreateForm(CreateForm::default());

        // A click on the 13th planet (index 12) selects it.
        let cell = planet_cell_rect(ctx.width, ctx.height, 12);
        let transition = screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(cell.x + cell.width / 2, cell.y + cell.height / 2),
        );

        assert_eq!(transition, Transition::None);
        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.world, 12);
            assert_eq!(form.focus, CreateField::World);
        } else {
            panic!("ожидалась форма создания");
        }

        // Numeric fallback: Tab to the World field and digits still work.
        if let Screen::CreateForm(form) = &mut screen {
            form.focus = CreateField::Name;
            form.world = 0;
        }
        key(&mut screen, &mut ctx, Key::Tab);
        char(&mut screen, &mut ctx, '7');

        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.world, 7, "цифровой ввод — fallback");
        }

        // A click on the planet name (bottom of the cell) also selects it.
        let cell = planet_cell_rect(ctx.width, ctx.height, 23);
        screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(cell.x + 2, cell.y + cell.height - 2),
        );
        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.world, 23);
        }
    }

    #[test]
    fn planet_grid_is_eight_by_three_and_inside_the_form() {
        assert_eq!(PLANET_COLUMNS * PLANET_ROWS, PLANET_COUNT as i32);

        for (width, height) in [(512, 384), (640, 480), (320, 240)] {
            let grid = planet_grid_rect(width, height);
            assert!(grid.width > 0 && grid.height > 0);

            for index in 0..PLANET_COUNT {
                let cell = planet_cell_rect(width, height, index);
                assert!(cell.x >= grid.x && cell.x + cell.width <= grid.x + grid.width);
                assert!(cell.y >= grid.y && cell.y + cell.height <= grid.y + grid.height);
                assert_eq!(planet_cell_at(width, height, cell.x, cell.y), Some(index));
            }

            assert_eq!(planet_cell_at(width, height, 0, 0), None);
        }
    }

    #[test]
    fn create_form_paints_planet_pictures_from_max_res() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let max_res = root.join("max-original/MAX.RES");
        let color_pal = root.join("max-original/COLOR.PAL");
        let patches = root.join("assets/PATCHES.RES");

        if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
            return;
        }

        let config = AppConfig {
            max_res,
            color_pal,
            patches_res: patches,
            ..Default::default()
        };
        let mut app = crate::App::new(config);
        app.init_assets().expect("ассеты игры загружаются");

        let form = CreateForm {
            world: 12,
            ..Default::default()
        };
        let mut buffer = RgbaBuffer::filled(640, 480, Rgba::rgb(1, 2, 3));
        let view = ScreenView {
            width: 640,
            height: 480,
            status: "planets",
            mouse: None,
            focus: 0,
            app: Some(&app),
            missions: None,
        };
        Screen::CreateForm(form).render(&mut buffer, view);

        let cell = planet_cell_rect(640, 480, 12);
        let image = Rect::new(cell.x + 3, cell.y + 3, cell.width - 6, cell.height - 16);
        let mut colors = std::collections::BTreeSet::new();

        for y in image.y..image.y + image.height {
            for x in image.x..image.x + image.width {
                if let Some(pixel) = buffer.get(x, y) {
                    colors.insert([pixel.r, pixel.g, pixel.b]);
                }
            }
        }

        assert!(
            colors.len() > 8,
            "картинка планеты должна давать больше 8 цветов, а не заливку: {colors:?}"
        );
    }

    #[test]
    fn create_form_creates_a_game_and_enters_the_session_with_mock_client() {
        let created = game("created-1", "Duel");
        let (mut ctx, log) = context(Vec::new(), Some(created.clone()));
        let mut screen = Screen::MainMenu;

        // The network form is available from `Multiplayer Menu`: «Multiplayer Game» →
        // `Join Network Game` (game list) → `c` (create a game).
        char(&mut screen, &mut ctx, 'j');
        char(&mut screen, &mut ctx, 'j');
        char(&mut screen, &mut ctx, 'c');
        for c in "Duel".chars() {
            char(&mut screen, &mut ctx, c);
        }

        // World 0 by default; check field switching and numeric input.
        key(&mut screen, &mut ctx, Key::Tab);
        char(&mut screen, &mut ctx, '1');
        char(&mut screen, &mut ctx, '2');
        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.world, 12);
        }

        let transition = key(&mut screen, &mut ctx, Key::Enter);

        assert_eq!(
            transition,
            Transition::StartSession(SessionRequest {
                game_id: "created-1".to_string(),
                password: None,
            })
        );
        assert_eq!(
            screen,
            Screen::InSession {
                request: SessionRequest {
                    game_id: "created-1".to_string(),
                    password: None,
                }
            }
        );

        let log = log.borrow();
        assert_eq!(log.create_calls, 1);
        assert_eq!(
            log.list_calls, 1,
            "Create не возвращается в список: единственный ListGames — вход в список"
        );
        assert!(
            ctx.status.contains("Created \"Duel\""),
            "статус подтверждает создание: {}",
            ctx.status
        );
        let params = log.last_params.as_ref().expect("параметры CreateGame");
        assert_eq!(params.name.as_deref(), Some("Duel"));
        assert_eq!(params.world, Some(12));
        assert_eq!(params.min_players, Some(2));
        assert_eq!(params.password, None);
    }

    #[test]
    fn create_failure_keeps_the_form_and_reports_status() {
        let (client, _log) = MockClient::new(Vec::new(), None);
        let client = MockClient {
            create_error: Some("horizon down".to_string()),
            ..client
        };
        let mut ctx = UiContext::new(Box::new(client), 512, 384);
        let mut screen = Screen::CreateForm(CreateForm::default());

        let transition = key(&mut screen, &mut ctx, Key::Enter);

        assert_eq!(transition, Transition::None);
        assert!(matches!(screen, Screen::CreateForm(_)));
        assert!(ctx.status.contains("horizon down"));
    }

    #[test]
    fn join_list_lists_games_and_selects_with_keyboard() {
        let (mut ctx, log) = context(vec![game("g1", "Alpha"), game("g2", "Beta")], None);
        let mut screen = Screen::MainMenu;

        // MainMenu: «Multiplayer Game» opens `Multiplayer Menu` (`menu.cpp:2361`).
        key(&mut screen, &mut ctx, Key::Down);
        key(&mut screen, &mut ctx, Key::Enter);
        assert_eq!(screen, Screen::MultiplayerMenu);

        // `Join Network Game` — the Horizon game list.
        char(&mut screen, &mut ctx, 'j');

        assert_eq!(
            screen,
            Screen::JoinList {
                games: vec![game("g1", "Alpha"), game("g2", "Beta")],
                selected: 0,
            }
        );
        assert_eq!(log.borrow().list_calls, 1);

        key(&mut screen, &mut ctx, Key::Down);
        if let Screen::JoinList { selected, .. } = &screen {
            assert_eq!(*selected, 1);
        } else {
            panic!("ожидался список игр");
        }

        let transition = key(&mut screen, &mut ctx, Key::Enter);
        assert_eq!(
            transition,
            Transition::StartSession(SessionRequest {
                game_id: "g2".to_string(),
                password: None,
            })
        );
        assert!(matches!(screen, Screen::InSession { .. }));
    }

    #[test]
    fn join_list_mouse_click_selects_a_row() {
        let games = vec![game("g1", "Alpha"), game("g2", "Beta")];
        let (mut ctx, _log) = context(games.clone(), None);
        let mut screen = Screen::JoinList { games, selected: 0 };

        let ui = build_join_list(
            ctx.width,
            ctx.height,
            &ctx.status,
            None,
            match &screen {
                Screen::JoinList { games, .. } => games,
                _ => unreachable!(),
            },
            0,
        );
        let list = ui.list_box(JOIN_LIST).expect("ListBox");
        let point = Point::new(list.rect.x + 4, list.rect.y + list.row_height + 2);

        screen.handle_input(&mut ctx, InputEvent::MouseClick(point.x, point.y));

        if let Screen::JoinList { selected, .. } = &screen {
            assert_eq!(*selected, 1);
        } else {
            panic!("ожидался список игр");
        }
    }

    #[test]
    fn join_list_refresh_uses_list_games_again() {
        let (mut ctx, log) = context(vec![game("g1", "Alpha")], None);
        let mut screen = Screen::JoinList {
            games: vec![game("g1", "Alpha")],
            selected: 0,
        };

        key(&mut screen, &mut ctx, Key::Char('r'));

        assert_eq!(log.borrow().list_calls, 1);
        assert!(matches!(screen, Screen::JoinList { .. }));
    }

    #[test]
    fn list_games_failure_stays_in_the_menu() {
        let (client, _log) = MockClient::new(Vec::new(), None);
        let client = MockClient {
            list_error: Some("no horizon".to_string()),
            ..client
        };
        let mut ctx = UiContext::new(Box::new(client), 512, 384);
        let mut screen = Screen::MainMenu;

        key(&mut screen, &mut ctx, Key::Down);
        key(&mut screen, &mut ctx, Key::Enter);
        char(&mut screen, &mut ctx, 'j');

        // A network error does not leave the menu: we stay in `Multiplayer Menu` with
        // the status line (the game list is also available by a repeated `j`).
        assert_eq!(screen, Screen::MultiplayerMenu);
        assert!(ctx.status.contains("no horizon"));
    }

    /// Item E: in a 640×480 frame the menu is drawn from `MAX.RES` resources, not
    /// procedurally.
    #[test]
    fn native_frame_menus_are_drawn_from_max_res() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let max_res = root.join("max-original/MAX.RES");
        let color_pal = root.join("max-original/COLOR.PAL");
        let patches = root.join("assets/PATCHES.RES");

        if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
            return;
        }

        let mut app = crate::App::new(AppConfig {
            max_res,
            color_pal,
            patches_res: patches,
            ..Default::default()
        });
        app.init_assets().expect("ассеты игры");
        let menu = app
            .assets()
            .and_then(|assets| assets.menu.as_ref())
            .expect("MenuAssets из MAX.RES");

        let view = ScreenView {
            width: 640,
            height: 480,
            status: "status",
            mouse: Some(Point::new(390, 180)),
            focus: 0,
            app: Some(&app),
            missions: None,
        };

        // Main menu: background — MAINPIC.
        let mut buffer = RgbaBuffer::new(640, 480);
        Screen::MainMenu.render(&mut buffer, view);
        assert_eq!(
            buffer.get(0, 0),
            menu.mainpic.rgb(0, 0).map(Rgba::from_rgb),
            "фон главного меню — MAINPIC"
        );

        // Creation form: background — PLANETSE, MNUBTN buttons at the bottom.
        let mut buffer = RgbaBuffer::new(640, 480);
        Screen::CreateForm(CreateForm::default()).render(&mut buffer, view);
        assert_eq!(
            buffer.get(600, 300),
            menu.planet_background().rgb(600, 300).map(Rgba::from_rgb),
            "фон формы — PLANETSE"
        );

        // Game list: background — MULTGAME.
        let mut buffer = RgbaBuffer::new(640, 480);
        Screen::JoinList {
            games: vec![game("g1", "Alpha")],
            selected: 0,
        }
        .render(&mut buffer, view);
        assert_eq!(
            buffer.get(10, 10),
            menu.join_background().rgb(10, 10).map(Rgba::from_rgb),
            "фон списка игр — MULTGAME"
        );
    }

    /// Item E: clicks on the main menu buttons hit the original
    /// `menu.cpp` rectangles (not the procedural layout).
    #[test]
    fn main_menu_clicks_use_the_cpp_button_rectangles() {
        let (mut ctx, _log) = context(Vec::new(), None);
        ctx.width = 640;
        ctx.height = 480;
        let mut screen = Screen::MainMenu;

        let create = menu::main_menu_rect(menu::MainMenuAction::Create).expect("New Game");
        let transition = screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(
                create.ulx + create.width / 2,
                create.uly + create.height / 2,
            ),
        );

        assert_eq!(transition, Transition::None);
        assert_eq!(
            screen,
            Screen::NewGameMenu { tips: None },
            "New Game открывает подменю New Game Menu"
        );

        // The `Custom Game` button of the submenu opens the local match setup.
        let custom = newgame::new_game_rect(NewGameAction::Custom).expect("Custom Game");
        screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(custom.ulx + 4, custom.uly + 4),
        );
        assert!(
            matches!(screen, Screen::LocalSetup(_)),
            "Custom Game открывает настройку локальной партии"
        );

        let mut screen = Screen::MainMenu;
        let exit = menu::main_menu_rect(menu::MainMenuAction::Quit).expect("Exit");
        assert_eq!(
            screen.handle_input(&mut ctx, InputEvent::MouseClick(exit.ulx + 4, exit.uly + 4),),
            Transition::Quit
        );
    }

    /// Item F: a click in the 1280×960 window (scale 2) is converted to the 640×480 frame and
    /// hits the same map cell as a click in the frame.
    #[test]
    fn window_click_at_2x_lands_in_the_same_map_cell() {
        use max_render::frame_scale::FrameScale;
        use max_render::layout;

        let scale = FrameScale::native(1280, 960);
        assert_eq!(scale.scale(), 2);

        let viewport = panels::map_viewport(&Viewport::new(640, 480));
        // A point in the map area: (180,18) + 3.5 cells by x and 2.5 by y.
        let frame_click = (
            layout::MAIN_MAP.ulx + 3 * 64 + 32,
            layout::MAIN_MAP.uly + 2 * 64 + 32,
        );
        let local = (
            frame_click.0 - layout::MAIN_MAP.ulx,
            frame_click.1 - layout::MAIN_MAP.uly,
        );
        let expected_grid = viewport.screen_to_grid(local.0, local.1);

        let window_click = scale.frame_to_window(frame_click.0, frame_click.1);
        let mapped = scale
            .window_to_frame(window_click.0 + 1, window_click.1 + 1)
            .expect("клик внутри кадра");
        let (map_x, map_y) = panels::screen_to_map(mapped.0, mapped.1).expect("точка на карте");
        let grid = viewport.screen_to_grid(map_x, map_y);

        assert_eq!(grid, expected_grid);
        assert_eq!(grid, (56, 55), "клетка под точкой (180+224, 18+160)");

        // A click on the black field of the 1000×700 window (letterbox) does not reach the frame.
        let letterboxed = FrameScale::native(1000, 700);
        assert_eq!(letterboxed.window_to_frame(10, 10), None);
    }

    #[test]
    fn setup_help_button_opens_and_closes_the_help_window() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::MainMenu;

        key(&mut screen, &mut ctx, Key::Enter);
        assert_eq!(screen, Screen::NewGameMenu { tips: None });

        // `Custom Game` opens the local match setup (Options).
        let custom = newgame::new_game_rect(NewGameAction::Custom).expect("Custom Game");
        screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(custom.ulx + 4, custom.uly + 4),
        );
        assert!(matches!(screen, Screen::LocalSetup(_)));

        // The `?` button (465,438 48×40) on the Options screen.
        let transition = screen.handle_input(&mut ctx, InputEvent::MouseClick(489, 458));
        assert_eq!(transition, Transition::None);

        match &screen {
            Screen::Help {
                screen, help, mode, ..
            } => {
                // C++ `HelpMenu_Menu`: the `?` button enables help mode, the window
                // does not exist yet; an entry is chosen by clicking an element.
                assert!(help.is_none(), "режим помощи: окна ещё нет");
                assert_eq!(mode.section, HelpSection::Options);
                assert_eq!(mode.placement, HelpPlacement::Menu);
                assert!(matches!(
                    screen.as_ref(),
                    Screen::LocalSetup(form) if form.screen == crate::setup::SetupScreen::Options
                ));
            }
            other => panic!("ожидалось окно помощи, получено {other:?}"),
        }

        // A click on the `PLAY_MODE` element (426..632, 39..227) opens the entry.
        screen.handle_input(&mut ctx, InputEvent::MouseClick(529, 133));
        if let Screen::Help {
            help: Some(help), ..
        } = &screen
        {
            assert_eq!(help.section(), HelpSection::Options);
            assert_eq!(help.entry_name(), Some("PLAY_MODE"));
        } else {
            panic!("клик по элементу не открыл запись");
        }

        // `PageDown` scrolls the entry; `Esc` closes the window and returns to the
        // mode (C++ `HelpMenu::Run` returns to `HelpMenu_Menu`).
        let rows_before = match &screen {
            Screen::Help {
                help: Some(help), ..
            } => help.line_count(),
            _ => 0,
        };
        screen.handle_input(&mut ctx, InputEvent::Key(Key::PageDown));
        if let Screen::Help {
            help: Some(help), ..
        } = &screen
        {
            if rows_before > crate::helpmenu::ROWS_PER_PAGE {
                assert_eq!(help.row_index(), crate::helpmenu::ROWS_PER_PAGE);
            } else {
                assert_eq!(help.row_index(), 0);
            }
        }

        screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape));
        assert!(
            matches!(&screen, Screen::Help { help: None, .. }),
            "Esc закрывает окно, режим остаётся"
        );

        // A second `Esc` disables the mode and returns the Options screen.
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape));
        assert!(matches!(
            screen,
            Screen::LocalSetup(form) if form.screen == crate::setup::SetupScreen::Options
        ));
    }

    #[test]
    fn research_menu_opens_for_a_powered_center_and_commits() {
        use max_core::enums::{ResearchTopic, UnitOrder, UnitType};
        use max_core::geo::Point;
        use max_core::team::TeamId;

        let (_ctx, _log) = context(Vec::new(), None);
        let mut app = App::new(AppConfig::default());
        let id = app
            .universe
            .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(20, 20), 0)
            .expect("center");
        let mut unit = app.state_ref().unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.research_topic = ResearchTopic::Attack;
        app.universe.write_unit(id, unit);
        app.universe
            .update_research_progress(TeamId::RED, ResearchTopic::Attack, 1);
        app.hud_mut().select(Some(id));

        assert!(app.open_research_menu(), "меню открывается для центра");
        assert_eq!(app.research_menu().map(|menu| menu.active_centers), Some(1));

        // The `R` key does not open the menu again (already open).
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(Key::Char('r'))),
            Transition::None
        );

        // `←`/`→` and `Done` through the same input path as the player.
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(Key::Down)),
            Transition::None
        );
        handle_session_input(&mut app, InputEvent::Key(Key::Left));
        handle_session_input(&mut app, InputEvent::Key(Key::Right));
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(Key::Enter)),
            Transition::None
        );
        assert!(app.research_menu().is_none());
        assert_eq!(
            app.state_ref().research_allocation(TeamId::RED)[ResearchTopic::Attack as usize],
            1
        );
    }

    #[test]
    fn panel_help_opens_game_screen_help_and_escape_closes_it() {
        use max_core::enums::{UnitOrder, UnitType};
        use max_core::geo::Point;
        use max_core::team::TeamId;

        let mut app = App::new(AppConfig::default());
        let id = app
            .universe
            .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
            .expect("unit");
        let mut unit = app.state_ref().unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::Await;
        app.universe.write_unit(id, unit);

        // `?`/the HELP_OF button enables help mode (`HelpMenu_Menu`): there is no window,
        // an entry is chosen by clicking an element.
        assert!(app.open_help(HelpSection::GameScreen));
        assert_eq!(
            app.help_mode().map(|mode| mode.section),
            Some(HelpSection::GameScreen)
        );
        assert!(app.help_screen().is_none());

        // A click on the `FILES` panel — the `FILES_BUTTON` entry (17..83, 4..20).
        assert_eq!(
            handle_session_input(&mut app, InputEvent::MouseClick(50, 12)),
            Transition::None
        );
        assert_eq!(
            app.help_screen().and_then(|help| help.entry_name()),
            Some("FILES_BUTTON")
        );

        // `Esc` closes the window and returns to the mode (C++ `HelpMenu::Run`).
        handle_session_input(&mut app, InputEvent::Key(Key::Escape));
        assert!(app.help_screen().is_none());
        assert!(app.help_mode().is_some());

        // A second `Esc` disables the mode.
        handle_session_input(&mut app, InputEvent::Key(Key::Escape));
        assert!(app.help_mode().is_none());
    }

    #[test]
    fn all_screens_render_without_panics() {
        let app = crate::App::new(AppConfig::default().with_screen(320, 240));
        let mut buffer = RgbaBuffer::new(320, 240);

        let screens = [
            Screen::MainMenu,
            Screen::JoinList {
                games: vec![game("g1", "Alpha")],
                selected: 0,
            },
            Screen::CreateForm(CreateForm::default()),
            Screen::MultiplayerMenu,
            Screen::Help {
                screen: Box::new(Screen::LocalSetup(GameSetupForm::default())),
                help: Some(helpmenu::HelpScreen::new(
                    HelpSection::Options,
                    HelpPlacement::Menu,
                )),
                mode: helpmenu::HelpMode::new(HelpSection::Options, HelpPlacement::Menu),
            },
            Screen::InSession {
                request: SessionRequest {
                    game_id: "g1".to_string(),
                    password: None,
                },
            },
        ];

        for screen in &screens {
            let view = ScreenView {
                width: 320,
                height: 240,
                status: "status",
                mouse: Some(Point::new(10, 10)),
                focus: 0,
                app: Some(&app),
                missions: None,
            };
            screen.render(&mut buffer, view);

            assert_eq!(buffer.get(0, 0).map(|pixel| pixel.a), Some(255));
            assert!(
                buffer.pixels().chunks_exact(4).any(|pixel| pixel[3] != 0),
                "экран {screen:?} не нарисовал ни одного пикселя"
            );
        }

        // Without App (headless rendering before the network) the session still draws the status.
        let mut ctx2 = UiContext::new(Box::new(MockClient::new(Vec::new(), None).0), 64, 32);
        ctx2.status = "offline".to_string();
        let screen = Screen::InSession {
            request: SessionRequest {
                game_id: "g1".to_string(),
                password: None,
            },
        };
        let mut small = RgbaBuffer::new(64, 32);
        screen.render(&mut small, ctx2.view(None));
        assert!(small.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn create_params_carry_password_and_opponents() {
        let form = CreateForm {
            name: "  Password game  ".to_string(),
            world: 7,
            timer: 90,
            endturn: 15,
            opponent: 3,
            password: "sesame".to_string(),
            focus: CreateField::Name,
        };

        let params = form.create_params();
        assert_eq!(params.name.as_deref(), Some("Password game"));
        assert_eq!(params.world, Some(7));
        assert_eq!(params.min_players, Some(4));
        assert_eq!(params.password.as_deref(), Some("sesame"));
        assert_eq!(form.password_value().as_deref(), Some("sesame"));
    }

    /// M6.2: a click on a menu button calls `AudioPlayer::play_once(KBUY0)`.
    ///
    /// The clip is put into the cache (`preload`), so the test needs neither game
    /// data nor an audio device — exactly the UI hook is checked.
    #[test]
    fn menu_button_click_triggers_menu_sound() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let (mut ctx, _log) = context(Vec::new(), None);
        ctx.audio = AudioPlayer::with_backend(Box::new(CountingBackend {
            calls: std::sync::Arc::clone(&calls),
        }));
        ctx.audio.preload(
            MENU_CLICK,
            max_assets::WavClip {
                sample_rate: 22050,
                channels: 1,
                samples: vec![1, 2, 3],
            },
        );

        let ui = build_main_menu(ctx.width, ctx.height, &ctx.status, ctx.mouse, ctx.focus);
        let create = ui.widgets[MAIN_CREATE].rect().center();
        let mut screen = Screen::MainMenu;

        let transition = screen.handle_input(&mut ctx, InputEvent::MouseClick(create.x, create.y));

        assert_eq!(transition, Transition::None);
        assert_eq!(
            screen,
            Screen::NewGameMenu { tips: None },
            "клик по New Game открывает подменю"
        );
        assert_eq!(*calls.lock().expect("lock"), 1, "клик должен озвучиваться");
        assert_eq!(ctx.audio.plays(), 1);
    }
}
