//! `.qa` script parser (QA layer 2): lines → [`Command`].
//!
//! The format is described in `max-brh-qa/scripts/README.md` and `max-brh-qa/plans/qa-plan.md`. A line —
//! one command; `#` starts a comment (inside `"…"` it does not);
//! empty lines are ignored. Coordinates are in **640×480** frame pixels
//! (the original window), so the same point means the same thing on
//! any host and in CI.
//!
//! The parser executes nothing: it only checks syntax and ranges,
//! so that a typo in the script fails with a line number instead of "silently clicking into
//! the void". Execution — [`crate::qa::runner`].

use max_core::team::TeamKind;

use crate::ui::Key;

/// QA frame width (the original window).
pub const FRAME_WIDTH: i32 = 640;
/// QA frame height.
pub const FRAME_HEIGHT: i32 = 480;

/// Comparison operator in `assert`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    /// `=`
    Eq,
    /// `>=`
    Ge,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `<`
    Lt,
}

impl Cmp {
    /// Check `left <op> right`.
    pub fn test(self, left: i64, right: i64) -> bool {
        match self {
            Self::Eq => left == right,
            Self::Ge => left >= right,
            Self::Le => left <= right,
            Self::Gt => left > right,
            Self::Lt => left < right,
        }
    }

    /// Operator character (for the log/error messages).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ge => ">=",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Lt => "<",
        }
    }
}

/// Screen name in `assert screen=…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenName {
    /// `Screen::MainMenu`.
    MainMenu,
    /// `Screen::JoinList`.
    JoinList,
    /// `Screen::CreateForm`.
    CreateForm,
    /// `Screen::MultiplayerMenu`.
    MultiplayerMenu,
    /// `Screen::NewGameMenu` — the `New Game` submenu (`menu_new_game_menu_loop`).
    NewGameMenu,
    /// `Screen::MissionSelect` — the `GameSetupMenu` mission list.
    MissionSelect,
    /// `Screen::Introduction` (MVE video placeholder).
    Introduction,
    /// `Screen::Credits` (credits scrolling).
    Credits,
    /// `Screen::LocalSetup` on the Options screen.
    Options,
    /// `Screen::LocalSetup` on the Planet screen.
    Planet,
    /// `Screen::LocalSetup` on the Players screen.
    Players,
    /// `Screen::LocalSetup` on the Clan screen.
    Clan,
    /// Any `Screen::LocalSetup` screen (whichever of the three).
    LocalSetup,
    /// `Screen::PreGame` — mobilization (`CargoMenu`).
    Mobilize,
    /// `Screen::PreGame` — landing point selection (`GameManager_SelectSite`).
    Landing,
    /// `Screen::GameOver`.
    GameOver,
    /// `Screen::MissionStory` — the campaign intro/victory/defeat story screen
    /// (`menu_draw_mission_story_screen`).
    Story,
    /// `Screen::Preferences` — the `OptionsMenu` dialog in the menu (`Setup`).
    Preferences,
    /// `Screen::InSession` (a match is in progress).
    InGame,
    /// `Screen::Help` — the modal help window.
    Help,
}

impl ScreenName {
    /// Name parsing (case does not matter).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "mainmenu" | "main_menu" | "main" => Some(Self::MainMenu),
            "joinlist" | "join_list" | "join" => Some(Self::JoinList),
            "createform" | "create_form" | "create" => Some(Self::CreateForm),
            "multiplayermenu" | "multiplayer_menu" | "multiplayer" => Some(Self::MultiplayerMenu),
            "newgamemenu" | "new_game_menu" | "newgame" | "new_game" => Some(Self::NewGameMenu),
            "missionselect" | "mission_select" | "missions" | "gamesetup" | "game_setup" => {
                Some(Self::MissionSelect)
            }
            "introduction" | "intro" => Some(Self::Introduction),
            "credits" | "titres" => Some(Self::Credits),
            "options" => Some(Self::Options),
            "planet" | "planets" => Some(Self::Planet),
            "players" => Some(Self::Players),
            "clan" | "clans" => Some(Self::Clan),
            "localsetup" | "local_setup" | "setup" => Some(Self::LocalSetup),
            "mobilize" | "mobilization" | "cargo" => Some(Self::Mobilize),
            "landing" | "siteselect" | "site_select" => Some(Self::Landing),
            "gameover" | "game_over" | "winloss" => Some(Self::GameOver),
            "story" | "missionstory" | "mission_story" | "storyscreen" => Some(Self::Story),
            "preferences" | "prefs" => Some(Self::Preferences),
            "ingame" | "in_game" | "session" => Some(Self::InGame),
            "help" | "helpmode" | "help_mode" => Some(Self::Help),
            _ => None,
        }
    }

    /// Canonical name (as in the log).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MainMenu => "MainMenu",
            Self::JoinList => "JoinList",
            Self::CreateForm => "CreateForm",
            Self::MultiplayerMenu => "MultiplayerMenu",
            Self::NewGameMenu => "NewGameMenu",
            Self::MissionSelect => "MissionSelect",
            Self::Introduction => "Introduction",
            Self::Credits => "Credits",
            Self::Options => "Options",
            Self::Planet => "Planet",
            Self::Players => "Players",
            Self::Clan => "Clan",
            Self::LocalSetup => "LocalSetup",
            Self::Mobilize => "Mobilize",
            Self::Landing => "Landing",
            Self::GameOver => "GameOver",
            Self::Story => "MissionStory",
            Self::Preferences => "Preferences",
            Self::InGame => "InGame",
            Self::Help => "Help",
        }
    }
}

/// Local match phase in `assert phase=…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseName {
    /// Human turn.
    PlayerTurn,
    /// Computer turn (whichever team).
    AiTurn,
    /// Transition to the next team.
    Advancing,
    /// The match is over.
    Finished,
}

impl PhaseName {
    /// Name parsing (case does not matter).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "playerturn" | "player_turn" | "player" => Some(Self::PlayerTurn),
            "aiturn" | "ai_turn" | "ai" => Some(Self::AiTurn),
            "advancing" | "next" => Some(Self::Advancing),
            "finished" | "over" => Some(Self::Finished),
            _ => None,
        }
    }

    /// Canonical name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlayerTurn => "PlayerTurn",
            Self::AiTurn => "AiTurn",
            Self::Advancing => "Advancing",
            Self::Finished => "Finished",
        }
    }
}

/// State check in `assert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assertion {
    /// `assert turn=N` / `turn>=N`: the turn counter.
    Turn(Cmp, i32),
    /// `assert team=N|RED|GREEN|BLUE|GRAY`: whose turn it is (`active_turn_team`).
    Team(u8),
    /// `assert units>=N`: the number of units in the state.
    Units(Cmp, u32),
    /// `assert screen=NAME`: the current screen.
    Screen(ScreenName),
    /// `assert phase=NAME`: the local match phase.
    Phase(PhaseName),
    /// `assert notice=TEAM|none`: the «Begin turn» window (M-HS) is open for the team.
    Notice(Option<u8>),
    /// `assert victory`: the match ended with a human victory.
    Victory,
    /// `assert defeat`: the match ended with a defeat.
    Defeat,
    /// `assert selected`: a mobile player unit is selected.
    Selected,
    /// `assert preview`: the selected unit has a non-empty path preview
    /// (the green HUD route after RMB).
    Preview,
    /// `assert missile-line`: a flying projectile (`MISSILE_UNIT` with `AirPath`) stands
    /// on the **pixel line** from the launch point to the target
    /// (`AirPath::Execute`, `airpath.cpp:219-242`; deviation ≤ 1.5 px).
    /// Catches the "projectile goes by cells in an 8-direction route" regression.
    MissileLine,
    /// `assert log=TEXT`: the HUD log has a line with `TEXT` (e.g.
    /// `assert log=Move:` after an RMB order).
    Log(String),
    /// `assert unit-pos=X,Y`: the selected unit stands in cell `(X, Y)`.
    UnitPos(i16, i16),
    /// `assert angle=N`: the hull angle of the selected unit (0..7, 0 = north).
    ///
    /// This is a simulation field (`UnitInfo::angle`): when moving it changes
    /// at route steps via `Paths_GetAngle` (see `GroundPath::Execute` /
    /// `GroundPath::UpdateUnitAngle`).
    Angle(u8),
    /// `assert build=open|closed`: whether `BuildMenu` is open (P2).
    BuildOpen(bool),
    /// `assert build-site=open|closed`: whether build site selection is in progress
    /// (`ORDER_STATE_SELECT_SITE`).
    BuildSiteOpen(bool),
    /// `assert build-site=valid|invalid`: whether the cell under the cursor is valid in
    /// the site selection mode (`CURSOR_MAP2`/`CURSOR_UNIT_NO_GO`).
    BuildSiteValid(bool),
    /// `assert build-center=yes|no`: whether the builder sprite stands exactly in the center
    /// of the 2×2 tape (`PositionInTape` + `OffsetDrawZones`, 32 px).
    BuildCenter(bool),
    /// `assert site-pos=X,Y`: the cell of the construction site tape (`LRG/SMLTAPE`).
    SitePos(i16, i16),
    /// `assert builder=inside|outside`: whether the builder (its cell) stands inside
    /// the bounds of its parent building. `inside` — the site is still occupied
    /// by the builder, `outside` — it has left it (`PositionInTape` LEAVE +
    /// `ActivateEngineer` → `ORDER_MOVE`).
    BuilderInside(bool),
    /// `assert build-ready=yes|no`: whether the player has a completed building
    /// waiting for the builder to leave (`ORDER_IDLE`/`ORDER_STATE_BUILDING_READY`).
    BuildReady(bool),
    /// `assert factory-ready=yes|no`: the selected factory waits for a ready
    /// unit to be brought out (`ORDER_BUILD`/`ORDER_STATE_UNIT_READY`, `SpawnNewUnit`,
    /// `unitinfo.cpp:3489-3520`).
    FactoryReady(bool),
    /// `assert unit-stored=NAME|none`: the selected factory has a unit of this
    /// type **inside** (`ORDER_IDLE`/`ORDER_STATE_STORE`, `parent` — the factory);
    /// `none` — there is no ready unit in the factory.
    UnitStored(Option<String>),
    /// `assert endturn=pressed|normal`: the look of the End Turn button.
    ///
    /// C++ `GameManager_UpdateGui` (`game_manager.cpp:2437-2481`) calls
    /// `GameManager_MenuClickEndTurnButton(rest_state)` by the active team type:
    /// `TEAM_TYPE_PLAYER` → released, `TEAM_TYPE_COMPUTER` → pressed
    /// (`SetRestState(true)`) until the end of the opponents' turns.
    EndTurnPressed(bool),
    /// `assert endturn-frames<N`: how many frames the last `end_turn` took
    /// (turn transition + AI turns). A hung AI turn hits
    /// `LOCAL_AI_TICK_LIMIT` (12 000) — the threshold catches such a regression.
    EndTurnFrames(Cmp, u32),
    /// `assert hint=TEXT|none`: the hint above the map
    /// (`MessageManager_DrawMessage` for the selected builder); the comparison is a
    /// substring, case-insensitive.
    Hint(Option<String>),
    /// `assert upgrade=open|closed`: whether the upgrades screen is open (P6).
    UpgradeOpen(bool),
    /// `assert research-menu=open|closed`: whether the research screen is open (P7).
    ResearchMenuOpen(bool),
    /// `assert unit-menu=open|closed`: whether the unit command menu is open (C++
    /// `GameManager_PopupButtons`).
    UnitMenuOpen(bool),
    /// `assert unit-menu-item=NAME`: the item is in the menu and enabled.
    UnitMenuItem(String),
    /// `assert unit-menu-disabled=NAME`: the item exists but is disabled (the mechanics are
    /// not ported).
    UnitMenuDisabled(String),
    /// `assert unit-menu-enabled=NAME`: the menu item exists and is enabled.
    UnitMenuEnabled(String),
    /// `assert cursor=load|steal|disable|repair|reload|transfer|none`: the cursor
    /// mode of the selected unit (C++ `UnitInfo::cursor`).
    CursorMode(String),
    /// `assert targeting=on|off`: the targeting mode (`targeting_mode`).
    Targeting(bool),
    /// `assert self-destruct=open|closed`: the `UnitsManager_SelfDestructMenu` dialog.
    SelfDestructOpen(bool),
    /// `assert pause=open|closed`: the `Alt+P` pause dialog (`PauseMenu_Menu`).
    PauseOpen(bool),
    /// `assert unit-order=NAME`: the selected unit's order (`ORDER_*`).
    UnitOrder(max_core::enums::UnitOrder),
    /// `assert research-alloc attack=N`: how many centers are assigned to the topic.
    ResearchAlloc(max_core::enums::ResearchTopic, Cmp, u32),
    /// `assert help=open|closed`: whether the help window is open (P7).
    HelpOpen(bool),
    /// `assert help-section=cargo|planet|newgame|options|…`: the window section.
    HelpSection(crate::helpmenu::HelpSection),
    /// `assert help-entry=NAME`: the `help.json` entry shown in the window.
    HelpEntry(String),
    /// `assert help-row>=N`: the first visible line of the help window.
    HelpRow(Cmp, u32),
    /// `assert gold>=N`: the local team's gold.
    Gold(Cmp, u32),
    /// `assert research attack>=1`: the research topic level (P6).
    ResearchLevel(max_core::enums::ResearchTopic, Cmp, u32),
    /// `assert unit=NAME`: the player has a unit with this name (P2: the unit/building
    /// appeared after production turns).
    UnitType(String),
    /// `assert unit-count NAME>=N`: units of this type on the map (the name is as in
    /// `spawn`: `RocketSmoke`, `Rocket`, `TorpedoBubble`, …).
    UnitCount(String, Cmp, u32),
    /// `assert firing=on|off`: the selected unit shows the firing frame
    /// (`UnitInfo::PrepareFire`/`ProgressFire`, `firing_recoil_frames > 0`).
    Firing(bool),
    /// `assert building=on|off`: the selected mobile builder has construction
    /// in progress (`ORDER_BUILD`, a site on the map).
    Building(bool),
    /// `assert trail=on|off`: the map has projectile trail particle units
    /// (`RKTSMOKE`/`TRPBUBLE`, `Paths_DrawMissile`).
    Trail(bool),
    /// `assert site=large|small|none`: the construction site in the world — tape+cones
    /// (+slab) in one cell; `none` — neither tape nor cones for the player
    /// team (after completion the slab under the building remains and does not interfere).
    ConstructionSite(Option<bool>),
    /// `assert site=drawn`: the site decals **are drawn** in the current frame —
    /// each (tape, cones, slab) yields its pixels. The `IsRelevant` check
    /// (`unitinfogroup.cpp:59`): a unit with `ORDER_IDLE` is not drawn.
    ConstructionSiteDrawn,
    /// `assert digest=HEX`: CRC16 of the state snapshot (`App::digest`).
    Digest(u16),
    /// `assert resource-icons=N`: how many cells with a survey marker are drawn
    /// (`Survey_RenderMarkers`): every surveyed cell — a known deposit (`*MK1..16`)
    /// or the `*MK0` circle of an explored cell without a resource.
    ResourceIcons(Cmp, u32),
    /// `assert survey-overlay=yes|no`: whether showing resource icons is enabled
    /// (`GameManager_RenderSurveyIndicator`: the SURVEY button or the selected
    /// scout).
    SurveyOverlay(bool),
    /// `assert unit-selector=yes|no`: whether the selected unit indicator is drawn
    /// (`GameManager_DrawUnitSelector`).
    UnitSelector(bool),
    /// `assert panel=NAME:on|off`: a 3×3 toggle of the left panel
    /// (`CTInfo::display_button_*`, `GameManager_MenuClick*Button`,
    /// `game_manager.cpp:3800-3990`). `NAME` — `survey`, `status`, `colors`,
    /// `hits`, `ammo`, `range`, `scan`, `grid`, `names`.
    PanelToggle(crate::panels::PanelButton, bool),
    /// `assert overlay>=N`: how many pixels of the 3×3 button overlays are drawn in
    /// the current frame (`crate::overlays`, `RenderStats::overlay_pixels`).
    OverlayPixels(Cmp, u32),
    /// `assert cargo-mode=buy|upgrade`: the `CargoMenu` list mode.
    CargoMode(bool),
    /// `assert cargo-types=N`: the length of the type list (15 in buy mode).
    CargoTypes(Cmp, u32),
    /// `assert cargo-credits>=N`: the remaining credits in `CargoMenu`.
    CargoCredits(Cmp, u32),
    /// `assert cargo-description>=N`: the description length of the selected type
    /// (`Unit::GetDescription()`, characters).
    CargoDescription(Cmp, u32),
    /// `assert cargo-stat-icons>=N`: the number of pips icons in the statistics table
    /// of the selected type (`UnitStats_DrawStats`).
    CargoStatIcons(Cmp, u32),
    /// `assert cargo-portrait=yes|no`: whether the selected type has a big image
    /// portrait `P_*` (`Unit::GetPortrait()`).
    CargoPortrait(bool),
    /// `assert alloc=open|closed`: whether the extraction allocation dialog is open
    /// (C++ `AllocMenu`).
    AllocOpen(bool),
    /// `assert alloc-raw=N`: raw extraction in the mine complex (`production.raw`).
    AllocRaw(Cmp, i32),
    /// `assert alloc-fuel=N`.
    AllocFuel(Cmp, i32),
    /// `assert alloc-gold=N`.
    AllocGold(Cmp, i32),
    /// `assert alloc-free=N`: free extraction units
    /// (`production.free_capacity`).
    AllocFree(Cmp, i32),
    /// `assert complex-raw=N`: the complex raw stock (`Complex::raw`).
    ComplexRaw(Cmp, i32),
    /// `assert complex-fuel=N`: the complex fuel stock.
    ComplexFuel(Cmp, i32),
    /// `assert complex-gold=N`: the complex gold stock.
    ComplexGold(Cmp, i32),
    /// `assert transfer=open|closed`: whether the X-FER window is open (`TransferMenu`).
    TransferOpen(bool),
    /// `assert transfer-amount=N`: the current bar value (signed; plus —
    /// from the unit into the complex, minus — back).
    TransferAmount(Cmp, i32),
    /// `assert unit-storage=N`: the `storage` of the selected unit (cargo by its type).
    UnitStorage(Cmp, i32),
    /// `assert prefs=open|closed`: the `Preferences`/`Setup` dialog is open (in the menu —
    /// `Screen::Preferences`, in game — the modal window `App::preferences_dialog`).
    PrefsOpen(bool),
    /// `assert prefs-value NAME=N`: the option value in the open dialog.
    PrefsValue(crate::preferences::PrefKey, Cmp, i32),
    /// `assert prefs-text NAME=VALUE`: the string option value in the dialog
    /// (`player_name`, `host_address`).
    PrefsText(crate::preferences::PrefKey, String),
    /// `assert prefs-effects=on|off`: the application `settings["effects"]`.
    PrefsEffects(bool),
    /// `assert prefs-sound=on|off`: the sound flag (`!disable_fx`).
    PrefsSound(bool),
    /// `assert prefs-audio=on|off`: the actual state of the audio layer
    /// (`AudioPlayer::enabled`), i.e. the flag reached the audio.
    PrefsAudio(bool),
}

/// Where the high-level `menu` command goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    /// `New Game` of the main menu → M-SP setup.
    NewGame,
    /// `Multiplayer Game` → `Multiplayer Menu`.
    Multiplayer,
    /// `Hot Seat Game` → M-HS setup.
    HotSeat,
    /// `Join Network Game` → the lobby game list.
    Join,
    /// `Introduction` of the main menu (MVE placeholder).
    Introduction,
    /// `Credits` of the main menu (credits scrolling).
    Credits,
    /// `Setup` of the main menu (`OptionsMenu`, `SETUPPIC`).
    Setup,
}

/// Action on the setup screen (`setup`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupAction {
    /// `Done` button (Options → Planet → Players → match start).
    Done,
    /// `Cancel` button.
    Cancel,
    /// `Random` button (planets screen only).
    Random,
}

/// Action on the mobilization screen (`CargoMenu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobilizeAction {
    /// `buy` — buy the selected unit.
    Buy,
    /// `buy N` — select row N and buy it.
    BuyIndex(u32),
    /// `delete` — sell the selected purchased unit.
    Delete,
    /// `step N` — move the selection along the list (may be negative).
    Step(i32),
    /// `mode buy|upgrade` — the `CRGTOG` toggle.
    Mode(bool),
    /// `filter ground|air|sea|building|combat` — the `SEL*` button.
    Filter(FilterName),
    /// `select N` — row N of the type list.
    Select(u32),
    /// `loadout N` — row N of the "Purchased" list.
    Loadout(u32),
    /// `row N up|down` — the upgrade arrow of row N.
    Adjust { row: u32, increase: bool },
    /// `cargo N` — set the cargo slider to N blocks (5·N units).
    Cargo(u32),
    /// `done` — `Done` (close mobilization).
    Done,
    /// `cancel` — `Cancel`.
    Cancel,
}

/// Domain filter of the upgrade list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterName {
    /// Ground (`SELGRD`).
    Ground,
    /// Air (`SELAIR`).
    Air,
    /// Sea (`SELSEA`).
    Sea,
    /// Buildings (`SELBLD`).
    Building,
    /// Combat only (`SELCBT`).
    Combat,
}

impl FilterName {
    /// Index in `Mobilization::filters`.
    pub const fn index(self) -> usize {
        match self {
            Self::Ground => 0,
            Self::Air => 1,
            Self::Sea => 2,
            Self::Building => 3,
            Self::Combat => 4,
        }
    }

    /// Name parsing (case does not matter).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "ground" | "grd" | "land" => Some(Self::Ground),
            "air" => Some(Self::Air),
            "sea" | "naval" => Some(Self::Sea),
            "building" | "build" | "bld" => Some(Self::Building),
            "combat" | "cbt" => Some(Self::Combat),
            _ => None,
        }
    }
}

/// `BuildMenu` screen action (P2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildCommand {
    /// `open` — open the menu for the selected builder (the `B` key).
    Open,
    /// `select N` — select a list row.
    Select(u32),
    /// `add` — factory `Build`: add the selected type to the queue.
    Add,
    /// `delete` — `Delete`: remove the selected queue element.
    Delete,
    /// `rate N` — speed 1/2/4.
    Rate(u16),
    /// `repeat` — the `Repeat` button (factory only).
    Repeat,
    /// `step N` — list arrows (N > 0 — down).
    Step(i32),
    /// `queue step N` — queue arrows.
    QueueStep(i32),
    /// `done` — `Done`/Enter: the order (`Build()`).
    Done,
    /// `path` — the engineer's `Path` button (`BLDPTH_U`, `buildmenu.cpp:343-346`):
    /// `Build()` with a forced 1×1 site selection (`event_click_path_build`).
    Path,
    /// `cancel` — `Cancel`/Esc: close without an order.
    Cancel,
    /// `place X Y` — confirm the build site in cell `X,Y` with a left click on
    /// the map (`GameManager_PathBuild`); `X Y` — the cell the player points to.
    Place(i16, i16),
    /// `place near` — confirm the site in the selected builder's cell.
    PlaceNear,
    /// `cursor X Y` — hover the mouse over a cell without a click: the ghost moves,
    /// no order is issued (`GameManager_GetBuilderUnitCursor`).
    Cursor(i16, i16),
}

/// Upgrades screen action (P6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeCommand {
    /// `open` — open for the selected unit (the `U` key).
    Open,
    /// `select N` — select a row of the type list.
    Select(u32),
    /// `row N up|down` — press the upgrade arrow of row N.
    Adjust { row: u32, increase: bool },
    /// `done` — `CommitUpgradeChanges`.
    Done,
    /// `cancel` — close without changes.
    Cancel,
}

/// Research screen action (P7, `ResearchMenu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearchCommand {
    /// `open` — open for the selected powered-on `RESEARCH` center.
    Open,
    /// `select N` — select a topic (keyboard).
    Select(u32),
    /// `row N up|down` — the `UPGRGT`/`UPGLFT` arrow of row N.
    Row { row: u32, increase: bool },
    /// `done` — `ResearchMenu_OnClick_Done` (the allocation is applied).
    Done,
    /// `cancel` — close without changes.
    Cancel,
}

/// `Preferences`/`Setup` dialog action (`OptionsMenu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefsCommand {
    /// `open` — open the dialog like a player: in game by clicking the panel
    /// `Preferences` button, in the main menu — by the `Setup` item.
    Open,
    /// `toggle NAME` — a click on a checkbox (`effects`, `disable_fx`, …).
    Toggle(String),
    /// `set NAME VALUE` — a slider (click at the position) or an input field
    /// (`timer`/`endturn`/`player_name`): click on the field, clear, type, Enter.
    Set {
        /// Option name (`music_level`, `quick_scroll`, `timer`, `player_name`, …).
        name: String,
        /// Value as text.
        value: String,
    },
    /// `done` — the `Done` button (saves).
    Done,
    /// `cancel` — the `Cancel` button (restores the snapshot).
    Cancel,
    /// `help` — the `Help` button (`?`).
    Help,
}

/// Help window action (`HelpMenu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelpCommand {
    /// `open SECTION` — open with the `?`/`Help` button of the current screen: it enters
    /// the help mode (`HelpMenu_Menu`), there is no window yet.
    Open(crate::helpmenu::HelpSection),
    /// `entry NAME` — a click on the center of the `NAME` entry from `help.json` (the real
    /// path of choosing an entry by coordinates).
    Entry(String),
    /// `click X Y` — a click on a frame point in help mode.
    Click(i32, i32),
    /// `page up|down` — `PageUp`/`PageDown`.
    PageUp,
    /// `page down`.
    PageDown,
    /// `up`/`down` — one line.
    Up,
    /// `down` — one line.
    Down,
    /// `keys` — the `Keys` button (`GAME_SCREEN_SETUP`).
    Keys,
    /// `done`/`ok` — `Done`/`Enter`: close the window, return to the mode.
    Done,
    /// `close`/`cancel` — exit the help mode (`Esc` in the mode/`Ctrl-Esc`).
    Close,
}

/// Mine extraction allocation dialog action (C++ `AllocMenu`, `ALLOCFRM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocateCommand {
    /// `open` — open the dialog for the selected mine (the `allocate` menu item).
    Open,
    /// `select raw|fuel|gold` — select a section (Rust client keyboard).
    Select(max_core::alloc::MiningKind),
    /// `set N` / `set raw|fuel|gold N` — the absolute extraction level (a click on
    /// the bar, `OnClick*Bar`).
    Set {
        /// Resource; `None` — the one selected in the dialog.
        kind: Option<max_core::alloc::MiningKind>,
        /// Target number of extraction units in the complex.
        level: i32,
    },
    /// `row raw|fuel|gold up|down` (or `up`/`down` for the selected section) —
    /// the `LFARO_OF`/`RTARO_OF` arrows.
    Adjust {
        /// Resource; `None` — the one selected in the dialog.
        kind: Option<max_core::alloc::MiningKind>,
        /// `+1` (right arrow) or `−1` (left).
        increase: bool,
    },
    /// `done` — `Done`/`Enter`: close (the values are already applied) and broadcast
    /// `ORDER_NEW_ALLOCATE` (08/P5).
    Done,
    /// `cancel`/`close` — `Esc`: the same as `done` (in C++ there is no separate `Cancel`,
    /// `Esc` and `KP_Enter` are the same exit of `AllocMenu::Run`).
    Cancel,
}

/// X-FER window action (C++ `TransferMenu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferCommand {
    /// `amount N` — set the bar value (like a click on the bar).
    Amount(i32),
    /// `right`/`inc` — `+1` (the `XFRRGT` button).
    Right,
    /// `left`/`dec` — `−1` (the `XFRLFT` button).
    Left,
    /// `done` — `Done`/`Enter` (order 08/P3 `ORDER_TRANSFER`).
    Done,
    /// `cancel` — `Esc` (without an order).
    Cancel,
    /// `help` — `?`/`Shift+/` (`TRANSFER_SETUP`).
    Help,
}

/// Action of the selected unit command menu (C++ `GameManager_PopupButtons`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitMenuCommand {
    /// `open` — a repeated click on the selected unit (the real trigger).
    Open,
    /// `item NAME` — a click on the item button (label or position character).
    Item(String),
    /// `key CHAR` — the item hotkey (`GameManager_ProcessPopupMenuInput`).
    Key(char),
    /// `close` — `Esc`.
    Close,
}

/// Action of bringing a ready unit out of a factory/transport (`launch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchCommand {
    /// `near` — a cell next to the container (`TaskActivate` iteration order).
    Near,
    /// `grid X Y` — exactly the specified map cell.
    Grid(i16, i16),
}

/// Building self-destruct dialog action (`UnitsManager_SelfDestructMenu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfDestructCommand {
    /// `arm` — press `Arm` (start `SLFDOPN1..6`).
    Arm,
    /// `ok` — press `OK` after the animation (`ORDER_EXPLODE`).
    Ok,
    /// `cancel` — `Cancel`/`Esc` (no order).
    Cancel,
    /// `advance N` — advance the visual clock by N ms.
    Advance(u32),
}

/// Landing point selection screen action (`GameManager_SelectSite`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingCommand {
    /// `click X Y` — a click on the frame (moves the cursor).
    Click(i32, i32),
    /// `grid X Y` — a click on a cell of the 112×112 map.
    Grid(i16, i16),
    /// `done` — confirm the point (Enter/`Done`).
    Done,
    /// `cancel` — cancel the selection (Esc).
    Cancel,
}

/// One script command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `click X Y` — the left button in frame pixels.
    Click(i32, i32),
    /// `click unit NAME` — a left click on a player unit with this name **without**
    /// checking the selection (cursor modes x-fer/repair/reload).
    ClickUnit(String),
    /// `click enemy NAME` — a left click on a foreign unit with this name: with
    /// an own unit selected it is an attack order (`GameManager_ClickUnit`).
    ClickEnemy(String),
    /// `rclick X Y` — the right button (move order/cancel).
    RightClick(i32, i32),
    /// `key NAME` — a key (`ENTER`, `ESC`, `T`, …).
    Key(Key),
    /// `type "text"` — printable characters into the current screen/line.
    Type(String),
    /// `wait N` — up to N frames until the game is ready for input.
    Wait(u32),
    /// `wait firing` — run frames until the selected unit shows the firing
    /// frame (`firing_recoil_frames > 0`, `UnitInfo::PrepareFire`), otherwise
    /// an error. Removes the scenario's dependence on the exact number of aiming ticks:
    /// the recoil lasts only 3 frames (`firing_recoil_frames`), and a fixed
    /// `tick N` hits it only with an unchanged number of `AimAtTarget` steps.
    WaitFiring,
    /// `tick N` — exactly N game loop frames.
    Tick(u32),
    /// `anim N` — advance only the **visual** clock by N ms of real
    /// time (frame-by-frame builder animation, building "growth") without
    /// simulation ticks. Needed to take deterministic animation frames
    /// (`dt > 0`), while `tick`/`wait` always go with `dt = 0`.
    Anim(u32),
    /// `mem` — write the current process RSS into the log (P4 soak, TC-1108).
    Mem,
    /// `shot NAME` — a PNG of the frame into the output directory.
    Shot(String),
    /// `assert …`.
    Assert(Assertion),
    /// `save FILE` — the `MXLS` container (`App::write_local_save`).
    Save(String),
    /// `save_v70 FILE` — a real V70 save of the current match
    /// (`GameState::to_save_bytes`, P3).
    SaveV70(String),
    /// `load FILE` — a local match from a V70 save (`App::start_local_game_from_save`).
    Load(String),
    /// `exit` — stop the script (success).
    Exit,
    /// `menu new_game|multiplayer|hot_seat|join`.
    Menu(MenuTarget),
    /// `setup done|cancel|random`.
    Setup(SetupAction),
    /// `players SLOT human|computer|none`.
    Players {
        /// Team slot `0..=3`.
        slot: u8,
        /// Who controls it.
        kind: TeamKind,
    },
    /// `select X Y` — a left click at a frame point + selection check.
    Select(i32, i32),
    /// `select unit N` — a left click on the N-th mobile player unit.
    SelectUnit(u32),
    /// `select last` — a click on the newest player unit (e.g. a factory just
    /// completed — it is not "mobile", so `select unit` does not
    /// fit).
    SelectLast,
    /// `select type NAME` — a click on a player unit with this name (`BuildMenu`
    /// shows the same names: «Training Hall», «Mining Station», …).
    SelectType(String),
    /// `move X Y` — a right click at a frame point + order check.
    Move(i32, i32),
    /// `move grid X Y` — a right click on a map cell.
    MoveGrid(i16, i16),
    /// `move near` — an order to an adjacent free cell of the selected unit.
    MoveNear,
    /// `move near refused` — a right click on an adjacent cell **without** an order:
    /// a refusal is expected (`CURSOR_UNIT_NO_GO`, a builder in `BUILD_IN_PROGRESS`).
    MoveNearRefused,
    /// `launch near` / `launch grid X Y` — bringing a ready unit out of a factory:
    /// a right click on the adjacent cell gives `ORDER_ACTIVATE` **to the factory**
    /// (`CURSOR_ACTIVATE`, `game_manager.cpp:6918-6926`), and
    /// `UnitsManager_ActivateUnit` launches the unit.
    Launch(LaunchCommand),
    /// `end_turn` — the End Turn button + AI turns until ready for input.
    EndTurn,
    /// `mobilize buy|buy N|delete|step N|done|cancel`.
    Mobilize(MobilizeAction),
    /// `landing click X Y|grid X Y|done|cancel`.
    Landing(LandingCommand),
    /// `build open|select N|add|delete|rate N|repeat|step N|queue step N|done|cancel`.
    Build(BuildCommand),
    /// `upgrade open|select N|row N up|down|done|cancel`.
    Upgrade(UpgradeCommand),
    /// `research open|select N|row N up|down|done|cancel`.
    Research(ResearchCommand),
    /// `help SECTION|page up|page down|up|down|keys|done|close`.
    Help(HelpCommand),
    /// `unit-menu open|item NAME|key CHAR|close` — the unit command menu.
    UnitMenu(UnitMenuCommand),
    /// `allocate open|select R|set N|row R up|down|done|cancel` — the mine
    /// extraction allocation dialog (C++ `AllocMenu`).
    Allocate(AllocateCommand),
    /// `transfer amount N|right|left|done|cancel|help` — the X-FER window
    /// (C++ `TransferMenu`).
    Transfer(TransferCommand),
    /// `prefs open|toggle NAME|set NAME N|done|cancel|help` — the
    /// `Preferences`/`Setup` (C++ `OptionsMenu`).
    Prefs(PrefsCommand),
    /// `spawn NAME` / `spawn enemy NAME` — place a unit on a free cell
    /// next to the selected one (or in the map center). QA-only: checking menu items
    /// (`load`/`steal`/`disable`/`remove`/bulldozer) that require units
    /// unavailable in mobilization.
    Spawn {
        /// Foreign team (by default — the player team).
        enemy: bool,
        /// Type name — the UFN (`Tank`, `Armoured Personnel Carrier`, …) or the
        /// variant's `Debug` name (`Tank`, `ArmouredPersonnelCarrier`).
        name: String,
    },
    /// `spawn at X Y NAME` — QA-only: place a unit **in the specified cell**
    /// (to check complex merging by connectors: `ADUMP` right next to
    /// a mine).
    SpawnAt {
        /// Map cell.
        x: i16,
        /// Map cell.
        y: i16,
        /// Type name.
        name: String,
    },
    /// `selfdestruct arm|ok|cancel` — the `UnitsManager_SelfDestructMenu` dialog.
    SelfDestruct(SelfDestructCommand),
    /// `complex raw N` — QA-only: raw stock of the selected unit's complex
    /// (needed to check `Upg. All`).
    ComplexRaw(i32),
    /// `complex fuel N` — QA-only: fuel stock of the selected unit's complex
    /// (C++ `Complex::Transfer(0, N, 0)`), e.g. to check the
    /// power supply of `POWGEN`/`POWERSTN`.
    ComplexFuel(i32),
    /// `complex gold N` — QA-only: the complex gold stock
    /// (`Complex::Transfer(0, 0, N)`).
    ComplexGold(i32),
    /// `teamvalues NAME ATTR VALUE` — QA-only: the current value of a team attribute
    /// (the upgrade is available when `current` is above `base`).
    TeamValues {
        /// Type name (`Power Station`, …).
        name: String,
        /// Attribute name (`hits`, `attack`, …).
        attribute: String,
        /// New value.
        value: i32,
    },
}

/// A script line with the parsed command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Line number in the file (1-based).
    pub line: usize,
    /// Source text without the comment.
    pub source: String,
    /// Command.
    pub command: Command,
}

/// Parses the whole script.
pub fn parse_script(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let source = strip_comment(raw);
        let source = source.trim();

        if source.is_empty() {
            continue;
        }

        let command = parse_command(source)
            .map_err(|error| format!("строка {line}: {error} — в «{source}»"))?;

        steps.push(Step {
            line,
            source: source.to_string(),
            command,
        });
    }

    Ok(steps)
}

/// Cuts the `#…` comment without touching the contents of quotes.
fn strip_comment(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quoted = false;

    for character in line.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                out.push(character);
            }
            '#' if !quoted => break,
            _ => out.push(character),
        }
    }

    out
}

fn parse_command(source: &str) -> Result<Command, String> {
    let (name, rest) = split_word(source);
    let name = name.to_ascii_lowercase();
    let rest = rest.trim();

    match name.as_str() {
        "click" => {
            // `click unit NAME` — a click on a specific unit (cursor modes).
            if let Some(name) = rest
                .strip_prefix("unit ")
                .or_else(|| rest.strip_prefix("UNIT "))
            {
                let name = name.trim();

                if name.is_empty() {
                    return Err(format!(
                        "click unit: ожидалось «click unit NAME», получено «{rest}»"
                    ));
                }

                return Ok(Command::ClickUnit(name.to_string()));
            }
            // `click enemy NAME` — a click on a foreign unit (attack order).
            if let Some(name) = rest
                .strip_prefix("enemy ")
                .or_else(|| rest.strip_prefix("ENEMY "))
            {
                let name = name.trim();

                if name.is_empty() {
                    return Err(format!(
                        "click enemy: ожидалось «click enemy NAME», получено «{rest}»"
                    ));
                }

                return Ok(Command::ClickEnemy(name.to_string()));
            }
            let (x, y) = parse_point(rest)?;
            Ok(Command::Click(x, y))
        }
        "rclick" | "right_click" => {
            let (x, y) = parse_point(rest)?;
            Ok(Command::RightClick(x, y))
        }
        "key" => Ok(Command::Key(parse_key(rest)?)),
        "type" => Ok(Command::Type(parse_text(rest)?)),
        "wait" => {
            // `wait firing` — frames until the firing frame of the selected unit.
            if rest.eq_ignore_ascii_case("firing") {
                Ok(Command::WaitFiring)
            } else {
                Ok(Command::Wait(parse_count(rest, "wait")?))
            }
        }
        "tick" => Ok(Command::Tick(parse_count(rest, "tick")?)),
        "anim" => Ok(Command::Anim(parse_count(rest, "anim")?)),
        "mem" => {
            if rest.is_empty() {
                Ok(Command::Mem)
            } else {
                Err(format!("mem не принимает аргументов: «{rest}»"))
            }
        }
        "shot" => Ok(Command::Shot(parse_name(rest, "shot")?)),
        "assert" => Ok(Command::Assert(parse_assertion(rest)?)),
        "save" => Ok(Command::Save(parse_name(rest, "save")?)),
        "save_v70" | "savev70" => Ok(Command::SaveV70(parse_name(rest, "save_v70")?)),
        "load" => Ok(Command::Load(parse_name(rest, "load")?)),
        "exit" => {
            if rest.is_empty() {
                Ok(Command::Exit)
            } else {
                Err(format!("exit не принимает аргументов: «{rest}»"))
            }
        }
        "menu" => Ok(Command::Menu(parse_menu(rest)?)),
        "setup" => Ok(Command::Setup(parse_setup(rest)?)),
        "players" => parse_players(rest),
        "select" => parse_select(rest),
        "move" => parse_move(rest),
        "mobilize" | "cargo" => parse_mobilize(rest),
        "landing" => parse_landing(rest),
        "build" => parse_build(rest),
        "upgrade" => parse_upgrade(rest),
        "research" => parse_research(rest),
        "help" => parse_help(rest),
        "unit-menu" | "unit_menu" => parse_unit_menu(rest),
        "allocate" | "alloc" => parse_allocate(rest),
        "transfer" | "xfer" => parse_transfer(rest),
        "prefs" | "preferences" => parse_prefs(rest),
        "launch" => parse_launch(rest),
        "spawn" => parse_spawn(rest),
        "selfdestruct" | "self_destruct" => parse_selfdestruct(rest),
        "complex" => parse_complex(rest),
        "teamvalues" | "team_values" => parse_teamvalues(rest),
        "end_turn" | "endturn" => {
            if rest.is_empty() {
                Ok(Command::EndTurn)
            } else {
                Err(format!("end_turn не принимает аргументов: «{rest}»"))
            }
        }
        other => Err(format!(
            "неизвестная команда «{other}» (см. max-brh-qa/scripts/README.md)"
        )),
    }
}

/// The first word and the rest of the line.
fn split_word(source: &str) -> (&str, &str) {
    match source.find(char::is_whitespace) {
        Some(index) => (&source[..index], &source[index..]),
        None => (source, ""),
    }
}

pub(crate) fn parse_i32(text: &str, what: &str) -> Result<i32, String> {
    text.parse::<i32>()
        .map_err(|_| format!("{what}: ожидалось целое число, получено «{text}»"))
}

/// Cell coordinate (`i16`) with a range check.
fn parse_i16(text: &str, what: &str) -> Result<i16, String> {
    let value = parse_i32(text, what)?;

    i16::try_from(value).map_err(|_| format!("{what}: вне диапазона i16: «{text}»"))
}

fn parse_u32(text: &str, what: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .map_err(|_| format!("{what}: ожидалось неотрицательное число, получено «{text}»"))
}

/// Map cell pair `X,Y` / `X Y` (for `assert unit-pos`).
fn parse_grid_pair(text: &str, what: &str) -> Result<(i16, i16), String> {
    let parts: Vec<&str> = text
        .split(|character: char| character == ',' || character == ':' || character.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();

    if parts.len() != 2 {
        return Err(format!(
            "{what}: ожидалось «X,Y» (две координаты клетки), получено «{text}»"
        ));
    }

    let x = parts[0]
        .parse::<i16>()
        .map_err(|_| format!("{what}: X не число: «{}»", parts[0]))?;
    let y = parts[1]
        .parse::<i16>()
        .map_err(|_| format!("{what}: Y не число: «{}»", parts[1]))?;

    if x < 0 || y < 0 || x >= max_core::geo::MAP_WIDTH || y >= max_core::geo::MAP_HEIGHT {
        return Err(format!(
            "{what}: клетка ({x}, {y}) вне карты {}×{}",
            max_core::geo::MAP_WIDTH,
            max_core::geo::MAP_HEIGHT
        ));
    }

    Ok((x, y))
}

/// 640×480 frame coordinate pair.
fn parse_point(rest: &str) -> Result<(i32, i32), String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.len() != 2 {
        return Err(format!(
            "ожидалось «X Y» (две координаты кадра), получено «{rest}»"
        ));
    }

    let x = parse_i32(parts[0], "X")?;
    let y = parse_i32(parts[1], "Y")?;

    if !(0..FRAME_WIDTH).contains(&x) || !(0..FRAME_HEIGHT).contains(&y) {
        return Err(format!(
            "точка ({x}, {y}) вне кадра {FRAME_WIDTH}×{FRAME_HEIGHT}"
        ));
    }

    Ok((x, y))
}

fn parse_count(rest: &str, what: &str) -> Result<u32, String> {
    if rest.is_empty() {
        return Err(format!("{what}: требуется число кадров"));
    }

    parse_u32(rest, what)
}

/// Frame/save file name: no paths and no spaces.
fn parse_name(rest: &str, what: &str) -> Result<String, String> {
    let rest = rest.trim();

    if rest.is_empty() {
        return Err(format!("{what}: требуется имя файла"));
    }
    if rest.contains('/') || rest.contains('\\') || rest.contains("..") {
        return Err(format!(
            "{what}: имя «{rest}» не должно содержать путь (файлы пишутся в каталог вывода)"
        ));
    }
    if !rest
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(format!(
            "{what}: в имени «{rest}» допустимы только A-Z a-z 0-9 . _ -"
        ));
    }

    Ok(rest.to_string())
}

fn parse_key(rest: &str) -> Result<Key, String> {
    let name = rest.trim();

    if name.is_empty() {
        return Err("key: требуется имя клавиши".to_string());
    }

    let key = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" | "bs" => Key::Backspace,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "pageup" | "page_up" | "pgup" => Key::PageUp,
        "pagedown" | "page_down" | "pgdn" => Key::PageDown,
        // `Alt+P` — `PauseMenu_Menu` (`GNW_KB_KEY_LALT_P`).
        "alt_p" | "altp" => Key::Alt('p'),
        "alt_f" | "altf" => Key::Alt('f'),
        "alt_l" | "altl" => Key::Alt('l'),
        // `Alt+S` — quick save into the last slot (`GNW_KB_KEY_LALT_S`).
        "alt_s" | "alts" => Key::Alt('s'),
        _ => {
            let mut characters = name.chars();

            match (characters.next(), characters.next()) {
                (Some(character), None) if character.is_ascii_graphic() => Key::Char(character),
                _ => {
                    return Err(format!(
                        "key: неизвестная клавиша «{name}» (ENTER/ESC/TAB/BACKSPACE/UP/DOWN/LEFT/RIGHT/PAGEUP/PAGEDOWN или один символ)"
                    ))
                }
            }
        }
    };

    Ok(key)
}

/// Text for `type`: in quotes — as is (spaces allowed), otherwise a word.
fn parse_text(rest: &str) -> Result<String, String> {
    let rest = rest.trim();

    if rest.is_empty() {
        return Err("type: требуется текст".to_string());
    }

    if let Some(inner) = rest.strip_prefix('"') {
        let Some(inner) = inner.strip_suffix('"') else {
            return Err("type: незакрытая кавычка".to_string());
        };

        Ok(inner.to_string())
    } else if rest.contains(char::is_whitespace) {
        Err("type: текст с пробелами заключается в кавычки".to_string())
    } else {
        Ok(rest.to_string())
    }
}

fn parse_team(text: &str) -> Result<u8, String> {
    let lowered = text.trim().to_ascii_lowercase();

    let team = match lowered.as_str() {
        "red" => 0,
        "green" => 1,
        "blue" => 2,
        "gray" | "grey" => 3,
        _ => {
            let value = parse_u32(&lowered, "команда")?;
            if value > 3 {
                return Err(format!("команда {value}: слотов всего 4 (0..=3)"));
            }
            value as u8
        }
    };

    Ok(team)
}

fn parse_team_kind(text: &str) -> Result<TeamKind, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "human" | "player" => Ok(TeamKind::Player),
        "computer" | "ai" | "cpu" => Ok(TeamKind::Computer),
        "none" | "empty" => Ok(TeamKind::None),
        other => Err(format!(
            "players: тип «{other}» (ожидалось human|computer|none)"
        )),
    }
}

fn parse_research_topic(text: &str) -> Result<max_core::enums::ResearchTopic, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "attack" => Ok(max_core::enums::ResearchTopic::Attack),
        "shots" => Ok(max_core::enums::ResearchTopic::Shots),
        "range" => Ok(max_core::enums::ResearchTopic::Range),
        "armor" => Ok(max_core::enums::ResearchTopic::Armor),
        "hits" => Ok(max_core::enums::ResearchTopic::Hits),
        "speed" => Ok(max_core::enums::ResearchTopic::Speed),
        "scan" => Ok(max_core::enums::ResearchTopic::Scan),
        "cost" => Ok(max_core::enums::ResearchTopic::Cost),
        other => Err(format!(
            "assert research: неизвестная тема «{other}» (attack|shots|range|armor|hits|speed|scan|cost)"
        )),
    }
}

fn parse_assertion(rest: &str) -> Result<Assertion, String> {
    let rest = rest.trim();

    if rest.is_empty() {
        return Err("assert: требуется проверка".to_string());
    }

    let lowered = rest.to_ascii_lowercase();

    match lowered.as_str() {
        "victory" | "win" => return Ok(Assertion::Victory),
        "defeat" | "lose" => return Ok(Assertion::Defeat),
        "selected" => return Ok(Assertion::Selected),
        "preview" => return Ok(Assertion::Preview),
        "missile-line" | "missile_line" => return Ok(Assertion::MissileLine),
        _ => {}
    }

    // `prefs-value NAME=N` contains its own `=`; otherwise the generic operator
    // parsing would take "prefs-value NAME" for the check name.
    if lowered.starts_with("prefs-value ") {
        let body = &rest["prefs-value ".len()..];
        let (name, cmp, expected) = if let Some((name, value)) = body.split_once(">=") {
            (name, Cmp::Ge, value)
        } else if let Some((name, value)) = body.split_once("<=") {
            (name, Cmp::Le, value)
        } else if let Some((name, value)) = body.split_once('=') {
            (name, Cmp::Eq, value)
        } else {
            return Err(format!(
                "assert prefs-value: ожидалось NAME=N или NAME>=N, получено «{rest}»"
            ));
        };
        let key = crate::preferences::PrefKey::parse(name.trim())
            .ok_or_else(|| format!("assert prefs-value: неизвестная опция «{}»", name.trim()))?;

        return Ok(Assertion::PrefsValue(
            key,
            cmp,
            parse_i32(expected.trim(), "assert prefs-value")?,
        ));
    }

    // `prefs-text NAME=VALUE` — a string option (`player_name`).
    if lowered.starts_with("prefs-text ") {
        // The prefix is the same length in lowercase, so we take the body from
        // the source line — the name value is case-sensitive.
        let body = &rest["prefs-text ".len()..];
        let (name, expected) = body
            .split_once('=')
            .ok_or_else(|| format!("assert prefs-text: ожидалось NAME=VALUE, получено «{rest}»"))?;
        let key = crate::preferences::PrefKey::parse(name.trim())
            .ok_or_else(|| format!("assert prefs-text: неизвестная опция «{}»", name.trim()))?;

        if !key.is_edit() {
            return Err(format!(
                "assert prefs-text: «{}» — не поле ввода",
                name.trim()
            ));
        }

        return Ok(Assertion::PrefsText(key, expected.trim().to_string()));
    }

    // We search the operator from the longest, otherwise ">=" would split into ">" and "=".
    let (key, operator, value) = if let Some(index) = rest.find(">=") {
        (&rest[..index], Cmp::Ge, &rest[index + 2..])
    } else if let Some(index) = rest.find("<=") {
        (&rest[..index], Cmp::Le, &rest[index + 2..])
    } else if let Some(index) = rest.find('=') {
        (&rest[..index], Cmp::Eq, &rest[index + 1..])
    } else if let Some(index) = rest.find('>') {
        (&rest[..index], Cmp::Gt, &rest[index + 1..])
    } else if let Some(index) = rest.find('<') {
        (&rest[..index], Cmp::Lt, &rest[index + 1..])
    } else {
        return Err(format!(
            "assert: нет оператора в «{rest}» (например turn>=3, screen=InGame, victory)"
        ));
    };

    let key = key.trim().to_ascii_lowercase();
    let value = value.trim();

    if value.is_empty() {
        return Err(format!("assert {key}: пустое значение"));
    }

    match key.as_str() {
        "turn" => Ok(Assertion::Turn(operator, parse_i32(value, "assert turn")?)),
        "endturn-frames" | "end_turn_frames" | "ai-turn-frames" => Ok(
            Assertion::EndTurnFrames(operator, parse_u32(value, "assert endturn-frames")?),
        ),
        "units" => Ok(Assertion::Units(
            operator,
            parse_u32(value, "assert units")?,
        )),
        k if k.starts_with("unit-count ") => {
            let name = k["unit-count ".len()..].trim();

            if name.is_empty() {
                return Err("assert unit-count: пустое имя типа".to_string());
            }

            Ok(Assertion::UnitCount(
                name.to_string(),
                operator,
                parse_u32(value, "assert unit-count")?,
            ))
        }
        "firing" => parse_on_off(value, "assert firing").map(Assertion::Firing),
        "building" => parse_on_off(value, "assert building").map(Assertion::Building),
        "trail" => parse_on_off(value, "assert trail").map(Assertion::Trail),
        "team" => Ok(Assertion::Team(parse_team(value)?)),
        "screen" => {
            let screen = ScreenName::parse(value)
                .ok_or_else(|| format!("assert screen: неизвестный экран «{value}»"))?;
            Ok(Assertion::Screen(screen))
        }
        "phase" => {
            let phase = PhaseName::parse(value)
                .ok_or_else(|| format!("assert phase: неизвестная фаза «{value}»"))?;
            Ok(Assertion::Phase(phase))
        }
        "notice" => {
            if value.eq_ignore_ascii_case("none") {
                Ok(Assertion::Notice(None))
            } else {
                Ok(Assertion::Notice(Some(parse_team(value)?)))
            }
        }
        "build" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::BuildOpen(true)),
            "closed" | "none" => Ok(Assertion::BuildOpen(false)),
            other => Err(format!(
                "assert build: ожидалось open|closed, получено «{other}»"
            )),
        },
        "build-site" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" | "selecting" => Ok(Assertion::BuildSiteOpen(true)),
            "closed" | "none" => Ok(Assertion::BuildSiteOpen(false)),
            "valid" => Ok(Assertion::BuildSiteValid(true)),
            "invalid" => Ok(Assertion::BuildSiteValid(false)),
            other => Err(format!(
                "assert build-site: ожидалось open|closed|valid|invalid, получено «{other}»"
            )),
        },
        "build-center" => match value.to_ascii_lowercase().as_str() {
            "yes" | "true" | "on" => Ok(Assertion::BuildCenter(true)),
            "no" | "false" | "off" => Ok(Assertion::BuildCenter(false)),
            other => Err(format!(
                "assert build-center: ожидалось yes|no, получено «{other}»"
            )),
        },
        "site-pos" => {
            let (x, y) = parse_grid_pair(value, "assert site-pos")?;
            Ok(Assertion::SitePos(x, y))
        }
        "builder" => match value.to_ascii_lowercase().as_str() {
            "inside" | "on-site" | "on_site" => Ok(Assertion::BuilderInside(true)),
            "outside" | "off-site" | "off_site" => Ok(Assertion::BuilderInside(false)),
            other => Err(format!(
                "assert builder: ожидалось inside|outside, получено «{other}»"
            )),
        },
        "build-ready" => match value.to_ascii_lowercase().as_str() {
            "yes" | "true" | "on" | "waiting" => Ok(Assertion::BuildReady(true)),
            "no" | "false" | "off" | "ready" => Ok(Assertion::BuildReady(false)),
            other => Err(format!(
                "assert build-ready: ожидалось yes|no, получено «{other}»"
            )),
        },
        "factory-ready" => match value.to_ascii_lowercase().as_str() {
            "yes" | "true" | "on" | "waiting" => Ok(Assertion::FactoryReady(true)),
            "no" | "false" | "off" | "ready" => Ok(Assertion::FactoryReady(false)),
            other => Err(format!(
                "assert factory-ready: ожидалось yes|no, получено «{other}»"
            )),
        },
        "unit-stored" => {
            if value.is_empty() {
                return Err("assert unit-stored: пустое имя типа".to_string());
            }

            if value.eq_ignore_ascii_case("none") {
                Ok(Assertion::UnitStored(None))
            } else {
                Ok(Assertion::UnitStored(Some(value.to_string())))
            }
        }
        "resource-icons" => Ok(Assertion::ResourceIcons(
            operator,
            parse_u32(value, "assert resource-icons")?,
        )),
        "survey-overlay" => match value.to_ascii_lowercase().as_str() {
            "yes" | "true" | "on" => Ok(Assertion::SurveyOverlay(true)),
            "no" | "false" | "off" => Ok(Assertion::SurveyOverlay(false)),
            other => Err(format!(
                "assert survey-overlay: ожидалось yes|no, получено «{other}»"
            )),
        },
        "unit-selector" => match value.to_ascii_lowercase().as_str() {
            "yes" | "true" | "on" => Ok(Assertion::UnitSelector(true)),
            "no" | "false" | "off" => Ok(Assertion::UnitSelector(false)),
            other => Err(format!(
                "assert unit-selector: ожидалось yes|no, получено «{other}»"
            )),
        },
        "panel" => {
            let (name, state) = value
                .split_once(':')
                .ok_or_else(|| "assert panel: ожидалось NAME:on|off".to_string())?;
            let button = parse_panel_button(name.trim())?;

            match state.trim().to_ascii_lowercase().as_str() {
                "on" | "yes" | "true" | "pressed" => Ok(Assertion::PanelToggle(button, true)),
                "off" | "no" | "false" | "released" => Ok(Assertion::PanelToggle(button, false)),
                other => Err(format!(
                    "assert panel={name}: ожидалось on|off, получено «{other}»"
                )),
            }
        }
        "overlay" => Ok(Assertion::OverlayPixels(
            operator,
            parse_u32(value, "assert overlay")?,
        )),
        "endturn" | "end-turn" => match value.to_ascii_lowercase().as_str() {
            "pressed" | "down" | "on" => Ok(Assertion::EndTurnPressed(true)),
            "normal" | "up" | "released" | "off" => Ok(Assertion::EndTurnPressed(false)),
            other => Err(format!(
                "assert endturn: ожидалось pressed|normal, получено «{other}»"
            )),
        },
        "hint" => {
            if value.eq_ignore_ascii_case("none") {
                Ok(Assertion::Hint(None))
            } else {
                Ok(Assertion::Hint(Some(value.to_string())))
            }
        }
        "upgrade" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::UpgradeOpen(true)),
            "closed" | "none" => Ok(Assertion::UpgradeOpen(false)),
            other => Err(format!(
                "assert upgrade: ожидалось open|closed, получено «{other}»"
            )),
        },
        "research-menu" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::ResearchMenuOpen(true)),
            "closed" | "none" => Ok(Assertion::ResearchMenuOpen(false)),
            other => Err(format!(
                "assert research-menu: ожидалось open|closed, получено «{other}»"
            )),
        },
        "unit-menu" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::UnitMenuOpen(true)),
            "closed" | "none" => Ok(Assertion::UnitMenuOpen(false)),
            other => Err(format!(
                "assert unit-menu: ожидалось open|closed, получено «{other}»"
            )),
        },
        "unit-menu-item" => Ok(Assertion::UnitMenuItem(value.to_string())),
        "unit-menu-disabled" => Ok(Assertion::UnitMenuDisabled(value.to_string())),
        "unit-menu-enabled" => Ok(Assertion::UnitMenuEnabled(value.to_string())),
        "cursor" => Ok(Assertion::CursorMode(value.to_ascii_lowercase())),
        "targeting" => match value.to_ascii_lowercase().as_str() {
            "on" | "yes" | "1" => Ok(Assertion::Targeting(true)),
            "off" | "no" | "0" => Ok(Assertion::Targeting(false)),
            other => Err(format!(
                "assert targeting: ожидалось on|off, получено «{other}»"
            )),
        },
        "self-destruct" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::SelfDestructOpen(true)),
            "closed" | "none" => Ok(Assertion::SelfDestructOpen(false)),
            other => Err(format!(
                "assert self-destruct: ожидалось open|closed, получено «{other}»"
            )),
        },
        "pause" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::PauseOpen(true)),
            "closed" | "none" => Ok(Assertion::PauseOpen(false)),
            other => Err(format!(
                "assert pause: ожидалось open|closed, получено «{other}»"
            )),
        },
        "unit-order" => Ok(Assertion::UnitOrder(parse_unit_order(value)?)),
        "help-entry" => {
            if value.trim().is_empty() {
                return Err("assert help-entry: пустое имя записи".to_string());
            }

            Ok(Assertion::HelpEntry(value.trim().to_string()))
        }
        "help" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::HelpOpen(true)),
            "closed" | "none" => Ok(Assertion::HelpOpen(false)),
            other => Err(format!(
                "assert help: ожидалось open|closed, получено «{other}»"
            )),
        },
        "help-section" => {
            let section = crate::helpmenu::HelpSection::parse(value).ok_or_else(|| {
                format!("assert help-section: неизвестная секция «{value}»")
            })?;
            Ok(Assertion::HelpSection(section))
        }
        "help-row" => Ok(Assertion::HelpRow(
            operator,
            parse_u32(value, "assert help-row")?,
        )),
        other if other.starts_with("research-alloc ") => {
            let topic = parse_research_topic(other["research-alloc ".len()..].trim())?;
            Ok(Assertion::ResearchAlloc(
                topic,
                operator,
                parse_u32(value, "assert research-alloc")?,
            ))
        }
        "gold" => Ok(Assertion::Gold(
            operator,
            parse_u32(value, "assert gold")?,
        )),
        "unit" => Ok(Assertion::UnitType(value.to_string())),
        "site" => match value.to_ascii_lowercase().as_str() {
            "large" | "big" => Ok(Assertion::ConstructionSite(Some(true))),
            "small" => Ok(Assertion::ConstructionSite(Some(false))),
            "none" | "closed" | "absent" => Ok(Assertion::ConstructionSite(None)),
            "drawn" | "visible" => Ok(Assertion::ConstructionSiteDrawn),
            other => Err(format!(
                "assert site: ожидалось large|small|none|drawn, получено «{other}»"
            )),
        },
        "unit-pos" => {
            let (x, y) = parse_grid_pair(value, "assert unit-pos")?;
            Ok(Assertion::UnitPos(x, y))
        }
        "angle" => {
            let angle = parse_u32(value, "assert angle")?;
            if angle > 7 {
                return Err(format!(
                    "assert angle: угол 0..7 (0 = север), получено «{value}»"
                ));
            }
            Ok(Assertion::Angle(angle as u8))
        }
        "log" => Ok(Assertion::Log(value.to_string())),
        "cargo-mode" => match value.to_ascii_lowercase().as_str() {
            "buy" | "purchase" => Ok(Assertion::CargoMode(true)),
            "upgrade" | "upgrades" => Ok(Assertion::CargoMode(false)),
            other => Err(format!(
                "assert cargo-mode: ожидалось buy|upgrade, получено «{other}»"
            )),
        },
        "cargo-types" => Ok(Assertion::CargoTypes(
            operator,
            parse_u32(value, "assert cargo-types")?,
        )),
        "cargo-credits" => Ok(Assertion::CargoCredits(
            operator,
            parse_u32(value, "assert cargo-credits")?,
        )),
        "cargo-description" => Ok(Assertion::CargoDescription(
            operator,
            parse_u32(value, "assert cargo-description")?,
        )),
        "cargo-stat-icons" => Ok(Assertion::CargoStatIcons(
            operator,
            parse_u32(value, "assert cargo-stat-icons")?,
        )),
        "cargo-portrait" => match value.to_ascii_lowercase().as_str() {
            "yes" | "true" | "1" => Ok(Assertion::CargoPortrait(true)),
            "no" | "false" | "0" => Ok(Assertion::CargoPortrait(false)),
            other => Err(format!(
                "assert cargo-portrait: ожидалось yes|no, получено «{other}»"
            )),
        },
        "alloc" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::AllocOpen(true)),
            "closed" | "none" => Ok(Assertion::AllocOpen(false)),
            other => Err(format!(
                "assert alloc: ожидалось open|closed, получено «{other}»"
            )),
        },
        "alloc-raw" => Ok(Assertion::AllocRaw(
            operator,
            parse_i32(value, "assert alloc-raw")?,
        )),
        "alloc-fuel" => Ok(Assertion::AllocFuel(
            operator,
            parse_i32(value, "assert alloc-fuel")?,
        )),
        "alloc-gold" => Ok(Assertion::AllocGold(
            operator,
            parse_i32(value, "assert alloc-gold")?,
        )),
        "alloc-free" => Ok(Assertion::AllocFree(
            operator,
            parse_i32(value, "assert alloc-free")?,
        )),
        "complex-raw" => Ok(Assertion::ComplexRaw(
            operator,
            parse_i32(value, "assert complex-raw")?,
        )),
        "complex-fuel" => Ok(Assertion::ComplexFuel(
            operator,
            parse_i32(value, "assert complex-fuel")?,
        )),
        "complex-gold" => Ok(Assertion::ComplexGold(
            operator,
            parse_i32(value, "assert complex-gold")?,
        )),
        "transfer" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::TransferOpen(true)),
            "closed" | "none" => Ok(Assertion::TransferOpen(false)),
            other => Err(format!(
                "assert transfer: ожидалось open|closed, получено «{other}»"
            )),
        },
        "transfer-amount" => Ok(Assertion::TransferAmount(
            operator,
            parse_i32(value, "assert transfer-amount")?,
        )),
        "unit-storage" => Ok(Assertion::UnitStorage(
            operator,
            parse_i32(value, "assert unit-storage")?,
        )),
        "digest" => {            let digits = value.trim_start_matches("0x").trim_start_matches("0X");
            let digest = u16::from_str_radix(digits, 16)
                .map_err(|_| format!("assert digest: ожидалось hex16, получено «{value}»"))?;
            Ok(Assertion::Digest(digest))
        }
        other if other.starts_with("research ") => {
            let topic = match other["research ".len()..].trim() {
                "attack" => max_core::enums::ResearchTopic::Attack,
                "shots" => max_core::enums::ResearchTopic::Shots,
                "range" => max_core::enums::ResearchTopic::Range,
                "armor" => max_core::enums::ResearchTopic::Armor,
                "hits" => max_core::enums::ResearchTopic::Hits,
                "speed" => max_core::enums::ResearchTopic::Speed,
                "scan" => max_core::enums::ResearchTopic::Scan,
                "cost" => max_core::enums::ResearchTopic::Cost,
                other => {
                    return Err(format!(
                        "assert research: неизвестная тема «{other}» (attack|shots|range|armor|hits|speed|scan|cost)"
                    ))
                }
            };
            Ok(Assertion::ResearchLevel(
                topic,
                operator,
                parse_u32(value, "assert research")?,
            ))
        }
        "prefs" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::PrefsOpen(true)),
            "closed" | "none" => Ok(Assertion::PrefsOpen(false)),
            other => Err(format!(
                "assert prefs: ожидалось open|closed, получено «{other}»"
            )),
        },
        "prefs-effects" => parse_on_off(value, "assert prefs-effects").map(Assertion::PrefsEffects),
        "prefs-sound" => parse_on_off(value, "assert prefs-sound").map(Assertion::PrefsSound),
        "prefs-audio" => parse_on_off(value, "assert prefs-audio").map(Assertion::PrefsAudio),
        "prefs-value" => {
            let (name, expected) = value
                .split_once('=')
                .ok_or_else(|| format!("assert prefs-value: ожидалось NAME=N, получено «{value}»"))?;
            let key = crate::preferences::PrefKey::parse(name).ok_or_else(|| {
                format!("assert prefs-value: неизвестная опция «{name}»")
            })?;
            Ok(Assertion::PrefsValue(
                key,
                operator,
                parse_i32(expected, "assert prefs-value")?,
            ))
        }
        other => Err(format!(
            "assert: неизвестная проверка «{other}» (turn|team|units|screen|phase|notice|digest|victory|defeat|selected)"
        )),
    }
}

fn parse_on_off(value: &str, what: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" | "enabled" => Ok(true),
        "off" | "false" | "0" | "no" | "disabled" => Ok(false),
        other => Err(format!("{what}: ожидалось on|off, получено «{other}»")),
    }
}

/// Left panel toggle name (`assert panel=NAME:on|off`).
fn parse_panel_button(name: &str) -> Result<crate::panels::PanelButton, String> {
    use crate::panels::PanelButton;

    match name.to_ascii_lowercase().as_str() {
        "survey" => Ok(PanelButton::Survey),
        "status" => Ok(PanelButton::Status),
        "colors" | "colours" => Ok(PanelButton::Colors),
        "hits" => Ok(PanelButton::Hits),
        "ammo" => Ok(PanelButton::Ammo),
        "range" => Ok(PanelButton::Range),
        "scan" => Ok(PanelButton::Scan),
        "grid" => Ok(PanelButton::Grid),
        "names" | "name" => Ok(PanelButton::Names),
        other => Err(format!(
            "assert panel: неизвестная кнопка «{other}» (survey|status|colors|hits|ammo|range|scan|grid|names)"
        )),
    }
}

fn parse_menu(rest: &str) -> Result<MenuTarget, String> {
    match rest.trim().to_ascii_lowercase().as_str() {
        "new_game" | "new" | "newgame" => Ok(MenuTarget::NewGame),
        "multiplayer" | "multi" => Ok(MenuTarget::Multiplayer),
        "hot_seat" | "hotseat" | "hot" => Ok(MenuTarget::HotSeat),
        "join" => Ok(MenuTarget::Join),
        "introduction" | "intro" => Ok(MenuTarget::Introduction),
        "credits" => Ok(MenuTarget::Credits),
        "setup" => Ok(MenuTarget::Setup),
        other => Err(format!(
            "menu: неизвестный пункт «{other}» (new_game|multiplayer|hot_seat|join|intro|credits|setup)"
        )),
    }
}

fn parse_setup(rest: &str) -> Result<SetupAction, String> {
    match rest.trim().to_ascii_lowercase().as_str() {
        "done" | "ok" => Ok(SetupAction::Done),
        "cancel" | "back" => Ok(SetupAction::Cancel),
        "random" => Ok(SetupAction::Random),
        other => Err(format!(
            "setup: неизвестное действие «{other}» (done|cancel|random)"
        )),
    }
}

fn parse_players(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.len() != 2 {
        return Err(format!(
            "players: ожидалось «SLOT human|computer|none», получено «{rest}»"
        ));
    }

    let slot = parse_u32(parts[0], "players: слот")?;
    if slot > 3 {
        return Err(format!("players: слот {slot} (всего 4: 0..=3)"));
    }

    Ok(Command::Players {
        slot: slot as u8,
        kind: parse_team_kind(parts[1])?,
    })
}

fn parse_mobilize(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.is_empty() {
        return Err("mobilize: требуется действие".to_string());
    }

    let action = match parts[0].to_ascii_lowercase().as_str() {
        "buy" => match parts.get(1) {
            Some(index) => {
                if parts.len() > 2 {
                    return Err(format!("mobilize buy: лишние аргументы «{rest}»"));
                }
                MobilizeAction::BuyIndex(parse_u32(index, "mobilize buy")?)
            }
            None => MobilizeAction::Buy,
        },
        "delete" => MobilizeAction::Delete,
        "done" => MobilizeAction::Done,
        "cancel" => MobilizeAction::Cancel,
        "step" => {
            let value = parts
                .get(1)
                .ok_or_else(|| "mobilize step: требуется число".to_string())?;

            MobilizeAction::Step(parse_i32(value, "mobilize step")?)
        }
        "mode" => match parts.get(1).map(|part| part.to_ascii_lowercase()) {
            Some(word) if word == "buy" || word == "purchase" => MobilizeAction::Mode(true),
            Some(word) if word == "upgrade" || word == "upgrades" => MobilizeAction::Mode(false),
            _ => {
                return Err(format!(
                    "mobilize mode: ожидалось buy|upgrade, получено «{rest}»"
                ))
            }
        },
        "filter" => {
            let name = parts
                .get(1)
                .and_then(|part| FilterName::parse(part))
                .ok_or_else(|| {
                    format!(
                        "mobilize filter: ожидалось ground|air|sea|building|combat, получено «{rest}»"
                    )
                })?;
            MobilizeAction::Filter(name)
        }
        "select" => MobilizeAction::Select(parse_u32(
            parts
                .get(1)
                .ok_or_else(|| "mobilize select: требуется индекс".to_string())?,
            "mobilize select",
        )?),
        "loadout" => MobilizeAction::Loadout(parse_u32(
            parts
                .get(1)
                .ok_or_else(|| "mobilize loadout: требуется индекс".to_string())?,
            "mobilize loadout",
        )?),
        "row" => {
            if parts.len() != 3 {
                return Err(format!(
                    "mobilize row: ожидалось «mobilize row N up|down», получено «{rest}»"
                ));
            }
            let row = parse_u32(parts[1], "mobilize row")?;
            let increase = match parts[2].to_ascii_lowercase().as_str() {
                "up" | "increase" | "+" => true,
                "down" | "decrease" | "-" => false,
                other => {
                    return Err(format!(
                        "mobilize row: ожидалось up|down, получено «{other}»"
                    ))
                }
            };
            MobilizeAction::Adjust { row, increase }
        }
        "cargo" => MobilizeAction::Cargo(parse_u32(
            parts
                .get(1)
                .ok_or_else(|| "mobilize cargo: требуется число блоков".to_string())?,
            "mobilize cargo",
        )?),
        other => {
            return Err(format!(
                "mobilize: ожидалось buy [N]|delete|step N|mode buy|upgrade|filter NAME|select N|loadout N|row N up|down|cargo N|done|cancel, получено «{other}»"
            ))
        }
    };

    Ok(Command::Mobilize(action))
}

fn parse_landing(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.is_empty() {
        return Err("landing: требуется действие".to_string());
    }

    let command = match parts[0].to_ascii_lowercase().as_str() {
        "click" => {
            if parts.len() != 3 {
                return Err(format!(
                    "landing click: ожидалось «landing click X Y», получено «{rest}»"
                ));
            }

            LandingCommand::Click(
                parse_i32(parts[1], "landing click X")?,
                parse_i32(parts[2], "landing click Y")?,
            )
        }
        "grid" => {
            if parts.len() != 3 {
                return Err(format!(
                    "landing grid: ожидалось «landing grid X Y», получено «{rest}»"
                ));
            }

            let x = parse_i32(parts[1], "landing grid X")?;
            let y = parse_i32(parts[2], "landing grid Y")?;

            if !(0..112).contains(&x) || !(0..112).contains(&y) {
                return Err(format!("landing grid: клетка ({x}, {y}) вне карты 112×112"));
            }

            LandingCommand::Grid(x as i16, y as i16)
        }
        "done" => LandingCommand::Done,
        "cancel" => LandingCommand::Cancel,
        other => {
            return Err(format!(
                "landing: неизвестное действие «{other}» (click|grid|done|cancel)"
            ))
        }
    };

    Ok(Command::Landing(command))
}

fn parse_build(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = parts.first().map(|part| part.to_ascii_lowercase()) else {
        return Err("build: ожидалось действие (open|select N|add|delete|rate N|repeat|step N|queue step N|done|cancel)".to_string());
    };

    let command = match (first.as_str(), parts.as_slice()) {
        ("open", [_]) => BuildCommand::Open,
        ("select", [_, index]) => BuildCommand::Select(parse_u32(index, "build select")?),
        ("add", [_]) => BuildCommand::Add,
        ("delete", [_]) => BuildCommand::Delete,
        ("rate", [_, rate]) => {
            let rate = parse_u32(rate, "build rate")?;

            if ![1, 2, 4].contains(&rate) {
                return Err(format!("build rate: скорость {rate} не из 1/2/4"));
            }

            BuildCommand::Rate(rate as u16)
        }
        ("repeat", [_]) => BuildCommand::Repeat,
        ("step", [_, delta]) => BuildCommand::Step(parse_i32(delta, "build step")?),
        ("queue", [_, step, delta]) if step.eq_ignore_ascii_case("step") => {
            BuildCommand::QueueStep(parse_i32(delta, "build queue step")?)
        }
        ("done", [_]) => BuildCommand::Done,
        ("path", [_]) => BuildCommand::Path,
        ("cancel", [_]) => BuildCommand::Cancel,
        ("place", [_, "near"]) => BuildCommand::PlaceNear,
        ("place", [_, x, y]) => BuildCommand::Place(
            x.parse::<i16>()
                .map_err(|_| format!("build place: X не число: «{x}»"))?,
            y.parse::<i16>()
                .map_err(|_| format!("build place: Y не число: «{y}»"))?,
        ),
        ("cursor", [_, x, y]) => BuildCommand::Cursor(
            x.parse::<i16>()
                .map_err(|_| format!("build cursor: X не число: «{x}»"))?,
            y.parse::<i16>()
                .map_err(|_| format!("build cursor: Y не число: «{y}»"))?,
        ),
        _ => {
            return Err(format!(
                "build: неизвестное действие «{rest}» (open|select N|add|delete|rate N|repeat|step N|queue step N|done|path|place X Y|place near|cursor X Y|cancel)"
            ))
        }
    };

    Ok(Command::Build(command))
}

/// `spawn [enemy] NAME` — QA-only unit on a free cell.
fn parse_spawn(rest: &str) -> Result<Command, String> {
    let rest = rest.trim();

    if rest.is_empty() {
        return Err("spawn: ожидалось «spawn [enemy] NAME» или «spawn at X Y NAME»".to_string());
    }

    if let Some(rest) = rest.strip_prefix("at ") {
        // `spawn at X Y [enemy] NAME` — the type name may consist of several
        // words («Missile Crawler», «Training Hall»), so we cut only the first
        // two numbers, and treat the rest as the name (with an optional `enemy`).
        let mut parts = rest.splitn(3, char::is_whitespace);
        let x = parts.next().unwrap_or_default();
        let y = parts.next().unwrap_or_default();
        let tail = parts.next().unwrap_or_default().trim();

        if x.is_empty() || y.is_empty() || tail.is_empty() {
            return Err(format!(
                "spawn at: ожидалось «spawn at X Y [enemy] NAME», получено «{rest}»"
            ));
        }

        let name = match tail.strip_prefix("enemy ") {
            Some(rest) => format!("enemy {}", rest.trim()),
            None => tail.to_string(),
        };

        return Ok(Command::SpawnAt {
            x: parse_i16(x, "spawn at x")?,
            y: parse_i16(y, "spawn at y")?,
            name,
        });
    }

    let (enemy, name) = match rest.strip_prefix("enemy ") {
        Some(name) => (true, name.trim()),
        None => (false, rest),
    };

    if name.is_empty() {
        return Err("spawn: пустое имя типа".to_string());
    }

    Ok(Command::Spawn {
        enemy,
        name: name.to_string(),
    })
}

/// `selfdestruct arm|ok|cancel|advance N`.
fn parse_selfdestruct(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    let command = match parts.as_slice() {
        ["arm"] => SelfDestructCommand::Arm,
        ["ok"] => SelfDestructCommand::Ok,
        ["cancel"] => SelfDestructCommand::Cancel,
        ["advance", millis] => {
            SelfDestructCommand::Advance(parse_count(millis, "selfdestruct advance")?)
        }
        _ => {
            return Err(format!(
                "selfdestruct: ожидалось arm|ok|cancel|advance N, получено «{rest}»"
            ))
        }
    };

    Ok(Command::SelfDestruct(command))
}

/// `complex raw N` — QA-only complex raw stock.
fn parse_complex(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    match parts.as_slice() {
        ["raw", value] => Ok(Command::ComplexRaw(parse_i32(value, "complex raw")?)),
        ["fuel", value] => Ok(Command::ComplexFuel(parse_i32(value, "complex fuel")?)),
        ["gold", value] => Ok(Command::ComplexGold(parse_i32(value, "complex gold")?)),
        _ => Err(format!(
            "complex: ожидалось «complex raw|fuel|gold N», получено «{rest}»"
        )),
    }
}

/// `teamvalues NAME ATTR VALUE` — QA-only current value of a team attribute.
fn parse_teamvalues(rest: &str) -> Result<Command, String> {
    let value_start = rest.trim_end().rsplit_once(' ').ok_or_else(|| {
        format!("teamvalues: ожидалось «teamvalues NAME ATTR VALUE», получено «{rest}»")
    })?;
    let (head, value) = value_start;
    let value = parse_i32(value.trim(), "teamvalues value")?;
    let (name, attribute) = head.trim_end().rsplit_once(' ').ok_or_else(|| {
        format!("teamvalues: ожидалось «teamvalues NAME ATTR VALUE», получено «{rest}»")
    })?;

    if name.trim().is_empty() || attribute.trim().is_empty() {
        return Err("teamvalues: пустое имя типа или атрибута".to_string());
    }

    Ok(Command::TeamValues {
        name: name.trim().to_string(),
        attribute: attribute.trim().to_ascii_lowercase(),
        value,
    })
}

fn parse_upgrade(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = parts.first().map(|part| part.to_ascii_lowercase()) else {
        return Err(
            "upgrade: ожидалось действие (open|select N|row N up|down|done|cancel)".to_string(),
        );
    };

    let command = match (first.as_str(), parts.as_slice()) {
        ("open", [_]) => UpgradeCommand::Open,
        ("select", [_, index]) => UpgradeCommand::Select(parse_u32(index, "upgrade select")?),
        ("row", [_, row, direction]) => {
            let row = parse_u32(row, "upgrade row")?;
            let increase = match direction.to_ascii_lowercase().as_str() {
                "up" | "inc" | "increase" => true,
                "down" | "dec" | "decrease" => false,
                other => {
                    return Err(format!(
                        "upgrade row: ожидалось up|down, получено «{other}»"
                    ))
                }
            };
            UpgradeCommand::Adjust { row, increase }
        }
        ("done", [_]) => UpgradeCommand::Done,
        ("cancel", [_]) => UpgradeCommand::Cancel,
        _ => {
            return Err(format!(
                "upgrade: неизвестное действие «{rest}» (open|select N|row N up|down|done|cancel)"
            ))
        }
    };

    Ok(Command::Upgrade(command))
}

fn parse_research(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = parts.first().map(|part| part.to_ascii_lowercase()) else {
        return Err(
            "research: ожидалось действие (open|select N|row N up|down|done|cancel)".to_string(),
        );
    };

    let command = match (first.as_str(), parts.as_slice()) {
        ("open", [_]) => ResearchCommand::Open,
        ("select", [_, index]) => ResearchCommand::Select(parse_u32(index, "research select")?),
        ("row", [_, row, direction]) => {
            let row = parse_u32(row, "research row")?;
            let increase = match direction.to_ascii_lowercase().as_str() {
                "up" | "inc" | "increase" | "+" => true,
                "down" | "dec" | "decrease" | "-" => false,
                other => {
                    return Err(format!(
                        "research row: ожидалось up|down, получено «{other}»"
                    ))
                }
            };
            ResearchCommand::Row { row, increase }
        }
        ("done", [_]) => ResearchCommand::Done,
        ("cancel", [_]) => ResearchCommand::Cancel,
        _ => {
            return Err(format!(
                "research: неизвестное действие «{rest}» (open|select N|row N up|down|done|cancel)"
            ))
        }
    };

    Ok(Command::Research(command))
}

fn parse_prefs(rest: &str) -> Result<Command, String> {
    let (action, args) = split_word(rest.trim());
    let action = action.to_ascii_lowercase();
    let args = args.trim();

    match action.as_str() {
        "open" => {
            if !args.is_empty() {
                return Err(format!("prefs open не принимает аргументов: «{args}»"));
            }
            Ok(Command::Prefs(PrefsCommand::Open))
        }
        "done" | "ok" => {
            if !args.is_empty() {
                return Err(format!("prefs done не принимает аргументов: «{args}»"));
            }
            Ok(Command::Prefs(PrefsCommand::Done))
        }
        "cancel" | "close" => {
            if !args.is_empty() {
                return Err(format!("prefs cancel не принимает аргументов: «{args}»"));
            }
            Ok(Command::Prefs(PrefsCommand::Cancel))
        }
        "help" => {
            if !args.is_empty() {
                return Err(format!("prefs help не принимает аргументов: «{args}»"));
            }
            Ok(Command::Prefs(PrefsCommand::Help))
        }
        "toggle" => {
            let name = args.trim();
            if name.is_empty() || name.split_whitespace().count() != 1 {
                return Err(format!(
                    "prefs toggle: требуется имя опции (effects|disable_fx|click_scroll|…), получено «{args}»"
                ));
            }
            let key = crate::preferences::PrefKey::parse(name)
                .ok_or_else(|| format!("prefs toggle: неизвестная опция «{name}»"))?;
            if !key.is_checkbox() {
                return Err(format!(
                    "prefs toggle: «{name}» не чекбокс (для слайдера — prefs set NAME N)"
                ));
            }
            Ok(Command::Prefs(PrefsCommand::Toggle(name.to_string())))
        }
        "set" => {
            let Some((name, value)) = args.trim().split_once(char::is_whitespace) else {
                return Err(format!(
                    "prefs set: ожидалось «NAME VALUE», получено «{args}»"
                ));
            };
            let name = name.trim();
            let value = value.trim();
            if value.is_empty() {
                return Err(format!(
                    "prefs set: ожидалось «NAME VALUE», получено «{args}»"
                ));
            }
            let key = crate::preferences::PrefKey::parse(name)
                .ok_or_else(|| format!("prefs set: неизвестная опция «{name}»"))?;
            if key.is_checkbox() {
                return Err(format!(
                    "prefs set: «{name}» — чекбокс (для него — prefs toggle NAME)"
                ));
            }
            if !key.is_slider() && !key.is_edit() {
                return Err(format!("prefs set: «{name}» не имеет значения"));
            }
            if key.is_slider() {
                parse_i32(value, "prefs set: значение")?;
            }
            Ok(Command::Prefs(PrefsCommand::Set {
                name: name.to_string(),
                value: value.to_string(),
            }))
        }
        other => Err(format!(
            "prefs: неизвестное действие «{other}» (open|toggle NAME|set NAME N|done|cancel|help)"
        )),
    }
}

fn parse_help(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = parts.first().map(|part| part.to_ascii_lowercase()) else {
        return Err(
            "help: ожидалось «help cargo|planet|newgame|options|research|upgrades|game» или page up|down|up|down|keys|done"
                .to_string(),
        );
    };

    let command = match (first.as_str(), parts.as_slice()) {
        ("page", [_, direction]) if direction.eq_ignore_ascii_case("up") => HelpCommand::PageUp,
        ("page", [_, direction]) if direction.eq_ignore_ascii_case("down") => HelpCommand::PageDown,
        ("row", [_, direction]) if direction.eq_ignore_ascii_case("up") => HelpCommand::Up,
        ("row", [_, direction]) if direction.eq_ignore_ascii_case("down") => HelpCommand::Down,
        ("up", [_]) => HelpCommand::Up,
        ("down", [_]) => HelpCommand::Down,
        ("keys", [_]) => HelpCommand::Keys,
        ("done", [_]) | ("ok", [_]) => HelpCommand::Done,
        ("close", [_]) | ("cancel", [_]) => HelpCommand::Close,
        ("entry", [_, _, ..]) => {
            let name = parts[1..].join(" ");

            if name.is_empty() {
                return Err("help entry: пустое имя записи".to_string());
            }

            HelpCommand::Entry(name)
        }
        ("click", [_, x, y]) => HelpCommand::Click(parse_i32(x, "help click")?, parse_i32(y, "help click")?),
        (section, [_]) => {
            let section = crate::helpmenu::HelpSection::parse(section).ok_or_else(|| {
                format!(
                    "help: неизвестная секция «{section}» (cargo|planet|newgame|options|research|upgrades|game|hotseat|clan)"
                )
            })?;
            HelpCommand::Open(section)
        }
        _ => {
            return Err(format!(
                "help: неизвестное действие «{rest}» (SECTION|page up|page down|up|down|keys|done|close)"
            ))
        }
    };

    Ok(Command::Help(command))
}

fn parse_unit_menu(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = parts.first().map(|part| part.to_ascii_lowercase()) else {
        return Err("unit-menu: ожидалось «unit-menu open|item NAME|key CHAR|close»".to_string());
    };

    let command = match (first.as_str(), parts.as_slice()) {
        ("open", [_]) => UnitMenuCommand::Open,
        ("close", [_]) | ("cancel", [_]) => UnitMenuCommand::Close,
        ("key", [_, character]) => {
            let mut chars = character.chars();
            match (chars.next(), chars.next()) {
                (Some(character), None) => UnitMenuCommand::Key(character),
                _ => {
                    return Err(format!(
                        "unit-menu key: ожидался один символ, получено «{character}»"
                    ))
                }
            }
        }
        ("item", [_, ..]) => {
            let name = rest
                .trim_start_matches(|character: char| character.is_ascii_alphabetic())
                .trim();

            if name.is_empty() {
                return Err("unit-menu item: нужна подпись пункта".to_string());
            }

            UnitMenuCommand::Item(name.to_string())
        }
        _ => {
            return Err(format!(
                "unit-menu: неизвестное действие «{rest}» (open|item NAME|key CHAR|close)"
            ))
        }
    };

    Ok(Command::UnitMenu(command))
}

/// `allocate open|select R|set N|row R up|down|done|cancel` — the
/// extraction allocation dialog (C++ `AllocMenu`).
fn parse_allocate(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = parts.first().map(|part| part.to_ascii_lowercase()) else {
        return Err(
            "allocate: ожидалось «allocate open|select R|set [R] N|row R up|down|done|cancel»"
                .to_string(),
        );
    };

    let mining = |text: &str| -> Result<max_core::alloc::MiningKind, String> {
        max_core::alloc::MiningKind::parse(text)
            .ok_or_else(|| format!("allocate: ожидался ресурс raw|fuel|gold, получено «{text}»"))
    };

    let command = match (first.as_str(), parts.as_slice()) {
        ("open", [_]) => AllocateCommand::Open,
        ("done", [_]) => AllocateCommand::Done,
        ("cancel", [_]) | ("close", [_]) => AllocateCommand::Cancel,
        ("select", [_, kind]) => AllocateCommand::Select(mining(kind)?),
        // `allocate set N` — the selected resource; `allocate set raw N` — explicit.
        ("set", [_, level]) => AllocateCommand::Set {
            kind: None,
            level: parse_i32(level, "allocate set")?.clamp(0, 16),
        },
        ("set", [_, kind, level]) => AllocateCommand::Set {
            kind: Some(mining(kind)?),
            level: parse_i32(level, "allocate set")?.clamp(0, 16),
        },
        ("row", [_, direction]) => {
            let increase = parse_direction(direction)?;
            AllocateCommand::Adjust {
                kind: None,
                increase,
            }
        }
        ("row", [_, kind, direction]) => AllocateCommand::Adjust {
            kind: Some(mining(kind)?),
            increase: parse_direction(direction)?,
        },
        ("up", [_]) => AllocateCommand::Adjust {
            kind: None,
            increase: true,
        },
        ("down", [_]) => AllocateCommand::Adjust {
            kind: None,
            increase: false,
        },
        _ => {
            return Err(format!(
                "allocate: неизвестное действие «{rest}» (open|select R|set [R] N|row [R] up|down|done|cancel)"
            ))
        }
    };

    Ok(Command::Allocate(command))
}

/// `transfer amount N|right|left|done|cancel|help` — the X-FER window.
fn parse_transfer(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    let command = match parts.as_slice() {
        ["amount", value] | ["set", value] => {
            TransferCommand::Amount(parse_i32(value, "transfer amount")?)
        }
        ["amount"] | ["set"] => {
            return Err("transfer amount: требуется число".to_string());
        }
        ["right"] | ["inc"] | ["+"] => TransferCommand::Right,
        ["left"] | ["dec"] | ["-"] => TransferCommand::Left,
        ["done"] | ["confirm"] => TransferCommand::Done,
        ["cancel"] | ["close"] => TransferCommand::Cancel,
        ["help"] | ["?"] => TransferCommand::Help,
        [] => {
            return Err(
                "transfer: требуется действие (amount N|right|left|done|cancel|help)".to_string(),
            );
        }
        _ => {
            return Err(format!(
                "transfer: неизвестное действие «{rest}» (amount N|right|left|done|cancel|help)"
            ));
        }
    };

    Ok(Command::Transfer(command))
}

fn parse_direction(text: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "up" | "right" | "increase" | "+" | "inc" => Ok(true),
        "down" | "left" | "decrease" | "-" | "dec" => Ok(false),
        other => Err(format!(
            "allocate: ожидалось up|down (или right|left), получено «{other}»"
        )),
    }
}

/// Order name (`ORDER_SENTRY`, `Sentry`, `sentry`) → [`max_core::enums::UnitOrder`].
fn parse_unit_order(text: &str) -> Result<max_core::enums::UnitOrder, String> {
    use max_core::enums::UnitOrder;

    let name = text
        .trim()
        .trim_start_matches("ORDER_")
        .trim_start_matches("order_")
        .to_ascii_lowercase();

    let order = match name.as_str() {
        "await" | "awaiting" => UnitOrder::Await,
        "transform" | "transforming" => UnitOrder::Transform,
        "move" | "moving" => UnitOrder::Move,
        "fire" | "firing" | "attack" | "attacking" => UnitOrder::Fire,
        "build" | "building" => UnitOrder::Build,
        "activate" => UnitOrder::Activate,
        "newallocate" | "allocate" => UnitOrder::NewAllocate,
        "poweron" => UnitOrder::PowerOn,
        "poweroff" => UnitOrder::PowerOff,
        "explode" | "exploding" => UnitOrder::Explode,
        "unload" | "unloading" => UnitOrder::Unload,
        "clear" | "clearing" => UnitOrder::Clear,
        "sentry" => UnitOrder::Sentry,
        "land" | "landing" => UnitOrder::Land,
        "takeoff" => UnitOrder::TakeOff,
        "load" | "loading" => UnitOrder::Load,
        "idle" => UnitOrder::Idle,
        "repair" | "repairing" => UnitOrder::Repair,
        "reload" | "reloading" => UnitOrder::Reload,
        "transfer" | "transferring" => UnitOrder::Transfer,
        "haltbuilding" => UnitOrder::HaltBuilding,
        "movetounit" => UnitOrder::MoveToUnit,
        "upgrade" | "upgrading" => UnitOrder::Upgrade,
        "laymine" => UnitOrder::LayMine,
        "movetoattack" => UnitOrder::MoveToAttack,
        "disable" | "disabled" => UnitOrder::Disable,
        other => {
            return Err(format!(
                "assert unit-order: неизвестный приказ «{other}» (await|move|fire|build|sentry|poweron|…)"
            ))
        }
    };

    Ok(order)
}

fn parse_select(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("unit"))
    {
        if parts.len() != 2 {
            return Err(format!(
                "select unit: ожидалось «select unit N», получено «{rest}»"
            ));
        }

        return Ok(Command::SelectUnit(parse_u32(parts[1], "select unit")?));
    }

    if parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("last"))
    {
        if parts.len() != 1 {
            return Err(format!("select last: лишние аргументы «{rest}»"));
        }

        return Ok(Command::SelectLast);
    }

    if parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("type"))
    {
        let name = rest
            .trim_start_matches(|character: char| character.is_ascii_alphabetic())
            .trim();

        if name.is_empty() {
            return Err(format!(
                "select type: ожидалось «select type NAME», получено «{rest}»"
            ));
        }

        return Ok(Command::SelectType(name.to_string()));
    }

    let (x, y) = parse_point(rest)?;

    Ok(Command::Select(x, y))
}

/// `launch near` / `launch grid X Y` — bringing a ready unit out of a factory.
fn parse_launch(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    match parts.first().map(|part| part.to_ascii_lowercase()) {
        Some(word) if word == "near" => {
            if parts.len() != 1 {
                return Err(format!("launch near: лишние аргументы «{rest}»"));
            }

            Ok(Command::Launch(LaunchCommand::Near))
        }
        Some(word) if word == "grid" => {
            if parts.len() != 3 {
                return Err(format!(
                    "launch grid: ожидалось «launch grid X Y», получено «{rest}»"
                ));
            }

            let x = parse_i32(parts[1], "launch grid X")?;
            let y = parse_i32(parts[2], "launch grid Y")?;

            if !(0..112).contains(&x) || !(0..112).contains(&y) {
                return Err(format!("launch grid: клетка ({x}, {y}) вне карты 112×112"));
            }

            Ok(Command::Launch(LaunchCommand::Grid(x as i16, y as i16)))
        }
        _ => Err(format!(
            "launch: ожидалось «launch near» или «launch grid X Y», получено «{rest}»"
        )),
    }
}

fn parse_move(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    match parts.first().map(|part| part.to_ascii_lowercase()) {
        Some(word) if word == "near" => {
            if parts.len() == 1 {
                return Ok(Command::MoveNear);
            }
            if parts.len() == 2 && parts[1].eq_ignore_ascii_case("refused") {
                return Ok(Command::MoveNearRefused);
            }
            return Err(format!("move near: лишние аргументы «{rest}»"));
        }
        Some(word) if word == "grid" => {
            if parts.len() != 3 {
                return Err(format!(
                    "move grid: ожидалось «move grid X Y», получено «{rest}»"
                ));
            }

            let x = parse_i32(parts[1], "move grid X")?;
            let y = parse_i32(parts[2], "move grid Y")?;

            if !(0..112).contains(&x) || !(0..112).contains(&y) {
                return Err(format!("move grid: клетка ({x}, {y}) вне карты 112×112"));
            }

            Ok(Command::MoveGrid(x as i16, y as i16))
        }
        _ => {
            let (x, y) = parse_point(rest)?;
            Ok(Command::Move(x, y))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(source: &str) -> Command {
        let steps = parse_script(source).expect("скрипт разбирается");
        assert_eq!(steps.len(), 1, "ожидалась одна команда: {source}");
        steps.into_iter().next().expect("шаг").command
    }

    #[test]
    fn every_low_level_command_is_parsed() {
        assert_eq!(one("click 10 20"), Command::Click(10, 20));
        assert_eq!(one("rclick 639 479"), Command::RightClick(639, 479));
        assert_eq!(one("key ENTER"), Command::Key(Key::Enter));
        assert_eq!(one("key esc"), Command::Key(Key::Escape));
        assert_eq!(one("key t"), Command::Key(Key::Char('t')));
        assert_eq!(
            one("type \"Hello, world\""),
            Command::Type("Hello, world".to_string())
        );
        assert_eq!(one("wait 40"), Command::Wait(40));
        assert_eq!(one("tick 3"), Command::Tick(3));
        // `anim N` — visual clock only (dt > 0), without simulation ticks.
        assert_eq!(one("anim 150"), Command::Anim(150));
        assert_eq!(one("anim 0"), Command::Anim(0));
        assert!(parse_script("anim -5\n").is_err());
        assert_eq!(one("mem"), Command::Mem);
        assert_eq!(
            one("shot main-menu"),
            Command::Shot("main-menu".to_string())
        );
        assert_eq!(one("save sp.mxs"), Command::Save("sp.mxs".to_string()));
        assert_eq!(
            one("load SAVE1.CAM"),
            Command::Load("SAVE1.CAM".to_string())
        );
        assert_eq!(one("exit"), Command::Exit);
    }

    #[test]
    fn every_assertion_is_parsed() {
        assert_eq!(
            one("assert turn=3"),
            Command::Assert(Assertion::Turn(Cmp::Eq, 3))
        );
        assert_eq!(
            one("assert turn>=3"),
            Command::Assert(Assertion::Turn(Cmp::Ge, 3))
        );
        assert_eq!(
            one("assert turn<=3"),
            Command::Assert(Assertion::Turn(Cmp::Le, 3))
        );
        assert_eq!(
            one("assert units>4"),
            Command::Assert(Assertion::Units(Cmp::Gt, 4))
        );
        assert_eq!(
            one("assert unit-count RocketSmoke>=3"),
            Command::Assert(Assertion::UnitCount("rocketsmoke".to_string(), Cmp::Ge, 3))
        );
        assert_eq!(
            one("assert firing=on"),
            Command::Assert(Assertion::Firing(true))
        );
        assert_eq!(
            one("assert building=off"),
            Command::Assert(Assertion::Building(false))
        );
        assert_eq!(
            one("assert trail=on"),
            Command::Assert(Assertion::Trail(true))
        );
        assert_eq!(one("assert team=1"), Command::Assert(Assertion::Team(1)));
        assert_eq!(one("assert team=RED"), Command::Assert(Assertion::Team(0)));
        assert_eq!(
            one("assert screen=InGame"),
            Command::Assert(Assertion::Screen(ScreenName::InGame))
        );
        assert_eq!(
            one("assert phase=PlayerTurn"),
            Command::Assert(Assertion::Phase(PhaseName::PlayerTurn))
        );
        assert_eq!(
            one("assert notice=GREEN"),
            Command::Assert(Assertion::Notice(Some(1)))
        );
        assert_eq!(
            one("assert notice=none"),
            Command::Assert(Assertion::Notice(None))
        );
        assert_eq!(one("assert victory"), Command::Assert(Assertion::Victory));
        assert_eq!(one("assert defeat"), Command::Assert(Assertion::Defeat));
        assert_eq!(one("assert selected"), Command::Assert(Assertion::Selected));
        assert_eq!(
            one("assert digest=0xBEEF"),
            Command::Assert(Assertion::Digest(0xBEEF))
        );
        assert_eq!(one("assert angle=2"), Command::Assert(Assertion::Angle(2)));
        assert!(parse_script("assert angle=8\n").is_err(), "угол 0..7");
        assert!(parse_script("assert angle=восток\n").is_err());
        assert_eq!(
            one("assert endturn=pressed"),
            Command::Assert(Assertion::EndTurnPressed(true))
        );
        assert_eq!(
            one("assert endturn=normal"),
            Command::Assert(Assertion::EndTurnPressed(false))
        );
        assert_eq!(
            one("assert site=large"),
            Command::Assert(Assertion::ConstructionSite(Some(true)))
        );
        assert_eq!(
            one("assert site=small"),
            Command::Assert(Assertion::ConstructionSite(Some(false)))
        );
        assert_eq!(
            one("assert site=none"),
            Command::Assert(Assertion::ConstructionSite(None))
        );
        assert_eq!(
            one("assert site=drawn"),
            Command::Assert(Assertion::ConstructionSiteDrawn)
        );
        assert_eq!(
            one("assert site=visible"),
            Command::Assert(Assertion::ConstructionSiteDrawn)
        );
        assert!(parse_script("assert site=huge\n").is_err());
        // A4–A9: frame checks of the SURVEY overlay and the selector.
        assert_eq!(
            one("assert resource-icons>=1"),
            Command::Assert(Assertion::ResourceIcons(Cmp::Ge, 1))
        );
        assert_eq!(
            one("assert resource-icons=0"),
            Command::Assert(Assertion::ResourceIcons(Cmp::Eq, 0))
        );
        assert_eq!(
            one("assert survey-overlay=yes"),
            Command::Assert(Assertion::SurveyOverlay(true))
        );
        assert_eq!(
            one("assert survey-overlay=no"),
            Command::Assert(Assertion::SurveyOverlay(false))
        );
        assert_eq!(
            one("assert unit-selector=yes"),
            Command::Assert(Assertion::UnitSelector(true))
        );
        assert_eq!(
            one("assert unit-selector=no"),
            Command::Assert(Assertion::UnitSelector(false))
        );
        assert!(parse_script("assert survey-overlay=maybe\n").is_err());
        // P9: the Preferences/Setup dialog.
        assert_eq!(
            one("assert prefs=open"),
            Command::Assert(Assertion::PrefsOpen(true))
        );
        assert_eq!(
            one("assert prefs=closed"),
            Command::Assert(Assertion::PrefsOpen(false))
        );
        assert_eq!(
            one("assert prefs-value effects=0"),
            Command::Assert(Assertion::PrefsValue(
                crate::preferences::PrefKey::Effects,
                Cmp::Eq,
                0
            ))
        );
        assert_eq!(
            one("assert prefs-value quick_scroll=35"),
            Command::Assert(Assertion::PrefsValue(
                crate::preferences::PrefKey::QuickScroll,
                Cmp::Eq,
                35
            ))
        );
        assert_eq!(
            one("assert prefs-effects=off"),
            Command::Assert(Assertion::PrefsEffects(false))
        );
        assert_eq!(
            one("assert prefs-sound=on"),
            Command::Assert(Assertion::PrefsSound(true))
        );
        assert_eq!(
            one("assert prefs-audio=off"),
            Command::Assert(Assertion::PrefsAudio(false))
        );
        assert!(parse_script("assert prefs-value bogus=1\n").is_err());
        assert!(parse_script("assert prefs-value effects\n").is_err());
        assert!(parse_script("assert prefs-effects=maybe\n").is_err());
    }

    #[test]
    fn high_level_commands_are_parsed() {
        assert_eq!(one("menu new_game"), Command::Menu(MenuTarget::NewGame));
        assert_eq!(
            one("menu multiplayer"),
            Command::Menu(MenuTarget::Multiplayer)
        );
        assert_eq!(one("menu hot_seat"), Command::Menu(MenuTarget::HotSeat));
        assert_eq!(one("menu join"), Command::Menu(MenuTarget::Join));
        assert_eq!(one("menu credits"), Command::Menu(MenuTarget::Credits));
        assert_eq!(one("menu intro"), Command::Menu(MenuTarget::Introduction));
        assert_eq!(one("menu setup"), Command::Menu(MenuTarget::Setup));
        assert_eq!(one("setup done"), Command::Setup(SetupAction::Done));
        assert_eq!(one("setup cancel"), Command::Setup(SetupAction::Cancel));
        assert_eq!(one("setup random"), Command::Setup(SetupAction::Random));
        assert_eq!(
            one("players 2 none"),
            Command::Players {
                slot: 2,
                kind: TeamKind::None
            }
        );
        assert_eq!(
            one("players 1 computer"),
            Command::Players {
                slot: 1,
                kind: TeamKind::Computer
            }
        );
        assert_eq!(
            one("players 0 human"),
            Command::Players {
                slot: 0,
                kind: TeamKind::Player
            }
        );
        assert_eq!(one("select 404 242"), Command::Select(404, 242));
        assert_eq!(one("select unit 2"), Command::SelectUnit(2));
        assert_eq!(one("move 468 306"), Command::Move(468, 306));
        assert_eq!(one("move grid 24 25"), Command::MoveGrid(24, 25));
        assert_eq!(one("move near"), Command::MoveNear);
        assert_eq!(one("end_turn"), Command::EndTurn);
    }

    #[test]
    fn upgrade_and_research_commands_parse() {
        let steps = parse_script(
            "upgrade open\n\
             upgrade select 3\n\
             upgrade row 0 up\n\
             upgrade row 1 down\n\
             upgrade done\n\
             upgrade cancel\n\
             assert upgrade=open\n\
             assert gold<150\n\
             assert research attack>=1\n",
        )
        .expect("P6-команды разбираются");

        assert_eq!(steps[0].command, Command::Upgrade(UpgradeCommand::Open));
        assert_eq!(
            steps[1].command,
            Command::Upgrade(UpgradeCommand::Select(3))
        );
        assert_eq!(
            steps[2].command,
            Command::Upgrade(UpgradeCommand::Adjust {
                row: 0,
                increase: true
            })
        );
        assert_eq!(
            steps[3].command,
            Command::Upgrade(UpgradeCommand::Adjust {
                row: 1,
                increase: false
            })
        );
        assert_eq!(
            steps[6].command,
            Command::Assert(Assertion::UpgradeOpen(true))
        );
        assert_eq!(
            steps[7].command,
            Command::Assert(Assertion::Gold(Cmp::Lt, 150))
        );
        assert_eq!(
            steps[8].command,
            Command::Assert(Assertion::ResearchLevel(
                max_core::enums::ResearchTopic::Attack,
                Cmp::Ge,
                1
            ))
        );
    }

    #[test]
    fn cargo_menu_commands_and_assertions_parse() {
        let steps = parse_script(
            "mobilize mode upgrade\n\
             mobilize mode buy\n\
             mobilize filter ground\n\
             mobilize filter combat\n\
             mobilize select 3\n\
             mobilize loadout 0\n\
             mobilize row 0 up\n\
             mobilize row 1 down\n\
             mobilize cargo 4\n\
             mobilize buy 14\n\
             mobilize delete\n\
             assert cargo-mode=upgrade\n\
             assert cargo-types=15\n\
             assert cargo-credits>=100\n\
             assert cargo-description>=100\n\
             assert cargo-stat-icons>=10\n\
             assert cargo-portrait=yes\n",
        )
        .expect("P7-команды разбираются");

        assert_eq!(
            steps[0].command,
            Command::Mobilize(MobilizeAction::Mode(false))
        );
        assert_eq!(
            steps[1].command,
            Command::Mobilize(MobilizeAction::Mode(true))
        );
        assert_eq!(
            steps[2].command,
            Command::Mobilize(MobilizeAction::Filter(FilterName::Ground))
        );
        assert_eq!(
            steps[3].command,
            Command::Mobilize(MobilizeAction::Filter(FilterName::Combat))
        );
        assert_eq!(
            steps[4].command,
            Command::Mobilize(MobilizeAction::Select(3))
        );
        assert_eq!(
            steps[5].command,
            Command::Mobilize(MobilizeAction::Loadout(0))
        );
        assert_eq!(
            steps[6].command,
            Command::Mobilize(MobilizeAction::Adjust {
                row: 0,
                increase: true
            })
        );
        assert_eq!(
            steps[7].command,
            Command::Mobilize(MobilizeAction::Adjust {
                row: 1,
                increase: false
            })
        );
        assert_eq!(
            steps[8].command,
            Command::Mobilize(MobilizeAction::Cargo(4))
        );
        assert_eq!(
            steps[9].command,
            Command::Mobilize(MobilizeAction::BuyIndex(14))
        );
        assert_eq!(steps[10].command, Command::Mobilize(MobilizeAction::Delete));
        assert_eq!(
            steps[11].command,
            Command::Assert(Assertion::CargoMode(false))
        );
        assert_eq!(
            steps[12].command,
            Command::Assert(Assertion::CargoTypes(Cmp::Eq, 15))
        );
        assert_eq!(
            steps[13].command,
            Command::Assert(Assertion::CargoCredits(Cmp::Ge, 100))
        );
        assert_eq!(
            steps[14].command,
            Command::Assert(Assertion::CargoDescription(Cmp::Ge, 100))
        );
        assert_eq!(
            steps[15].command,
            Command::Assert(Assertion::CargoStatIcons(Cmp::Ge, 10))
        );
        assert_eq!(
            steps[16].command,
            Command::Assert(Assertion::CargoPortrait(true))
        );

        assert!(parse_script("mobilize filter water\n").is_err());
        assert!(parse_script("mobilize mode neither\n").is_err());
        assert!(parse_script("assert cargo-mode=neither\n").is_err());
        assert!(parse_script("assert cargo-portrait=maybe\n").is_err());
    }

    #[test]
    fn comments_and_empty_lines_are_skipped() {
        let script = "\
# заголовок
click 1 2  # клик по кнопке
\n
type \"a#b\"  # решётка в кавычках — часть текста
";

        let steps = parse_script(script).expect("разбирается");

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].line, 2);
        assert_eq!(steps[0].command, Command::Click(1, 2));
        assert_eq!(steps[1].command, Command::Type("a#b".to_string()));
    }

    #[test]
    fn step_numbers_point_at_the_source_line() {
        let script = "# c\n\nclick 1 2\n\nshot a\n";

        let steps = parse_script(script).expect("разбирается");

        assert_eq!(steps[0].line, 3);
        assert_eq!(steps[1].line, 5);
    }

    #[test]
    fn errors_name_the_line() {
        let error = parse_script("click 1 2\nbogus 3\n").expect_err("опечатка ловится");

        assert!(error.contains("строка 2"), "{error}");
        assert!(error.contains("bogus"), "{error}");
    }

    #[test]
    fn out_of_frame_and_bad_arguments_are_rejected() {
        for source in [
            "click 640 10",
            "click -1 10",
            "click 10 480",
            "click 10",
            "click 10 20 30",
            "key",
            "key F13",
            "type hello world",
            "type \"unterminated",
            "wait",
            "mem 1",
            "shot a/b",
            "shot ..",
            "save ../x.mxs",
            "load dir/SAVE.CAM",
            "assert",
            "assert turn",
            "assert turn=abc",
            "assert units>=-2",
            "assert team=9",
            "assert screen=Nope",
            "assert phase=Nope",
            "assert digest=zzz",
            "assert nope=1",
            "menu nope",
            "setup nope",
            "players 9 human",
            "players 0 alien",
            "players 0",
            "select 900 10",
            "select unit",
            "move grid 112 0",
            "move grid 1",
            "move near extra",
            "end_turn now",
            "exit now",
        ] {
            assert!(
                parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
            );
        }
    }

    #[test]
    fn research_menu_and_help_commands_parse() {
        assert_eq!(
            one("research open"),
            Command::Research(ResearchCommand::Open)
        );
        assert_eq!(
            one("research row 0 down"),
            Command::Research(ResearchCommand::Row {
                row: 0,
                increase: false
            })
        );
        assert_eq!(
            one("research row 1 up"),
            Command::Research(ResearchCommand::Row {
                row: 1,
                increase: true
            })
        );
        assert_eq!(
            one("research select 7"),
            Command::Research(ResearchCommand::Select(7))
        );
        assert_eq!(
            one("research done"),
            Command::Research(ResearchCommand::Done)
        );
        assert_eq!(
            one("research cancel"),
            Command::Research(ResearchCommand::Cancel)
        );
        assert_eq!(
            one("assert research-menu=open"),
            Command::Assert(Assertion::ResearchMenuOpen(true))
        );
        assert_eq!(
            one("assert research-alloc attack=0"),
            Command::Assert(Assertion::ResearchAlloc(
                max_core::enums::ResearchTopic::Attack,
                Cmp::Eq,
                0
            ))
        );

        assert_eq!(
            one("help cargo"),
            Command::Help(HelpCommand::Open(crate::helpmenu::HelpSection::Cargo))
        );
        assert_eq!(
            one("help newgame"),
            Command::Help(HelpCommand::Open(crate::helpmenu::HelpSection::NewGame))
        );
        assert_eq!(one("help page down"), Command::Help(HelpCommand::PageDown));
        assert_eq!(one("help page up"), Command::Help(HelpCommand::PageUp));
        assert_eq!(one("help keys"), Command::Help(HelpCommand::Keys));
        assert_eq!(one("help done"), Command::Help(HelpCommand::Done));
        assert_eq!(one("help close"), Command::Help(HelpCommand::Close));
        assert_eq!(
            one("assert help=open"),
            Command::Assert(Assertion::HelpOpen(true))
        );
        assert_eq!(
            one("assert help-section=CARGO_SETUP"),
            Command::Assert(Assertion::HelpSection(crate::helpmenu::HelpSection::Cargo))
        );
        assert_eq!(
            one("assert help-row>=16"),
            Command::Assert(Assertion::HelpRow(Cmp::Ge, 16))
        );

        // Typos fail with a line number instead of "silently".
        for source in [
            "research nope",
            "research row 0 sideways",
            "help nope",
            "help page sideways",
            "assert help-section=nope",
            "assert research-alloc nope=1",
        ] {
            assert!(
                parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
            );
        }
    }

    #[test]
    fn allocate_commands_parse() {
        use max_core::alloc::MiningKind;

        assert_eq!(
            one("allocate open"),
            Command::Allocate(AllocateCommand::Open)
        );
        assert_eq!(
            one("allocate select gold"),
            Command::Allocate(AllocateCommand::Select(MiningKind::Gold))
        );
        // `set N` — the selected resource; `set raw N` — explicit.
        assert_eq!(
            one("allocate set 4"),
            Command::Allocate(AllocateCommand::Set {
                kind: None,
                level: 4
            })
        );
        assert_eq!(
            one("allocate set fuel 2"),
            Command::Allocate(AllocateCommand::Set {
                kind: Some(MiningKind::Fuel),
                level: 2
            })
        );
        // The level is clipped by the mine capacity (16 units).
        assert_eq!(
            one("allocate set raw 99"),
            Command::Allocate(AllocateCommand::Set {
                kind: Some(MiningKind::Raw),
                level: 16
            })
        );
        assert_eq!(
            one("allocate row raw up"),
            Command::Allocate(AllocateCommand::Adjust {
                kind: Some(MiningKind::Raw),
                increase: true
            })
        );
        assert_eq!(
            one("allocate row down"),
            Command::Allocate(AllocateCommand::Adjust {
                kind: None,
                increase: false
            })
        );
        assert_eq!(
            one("allocate done"),
            Command::Allocate(AllocateCommand::Done)
        );
        assert_eq!(
            one("allocate cancel"),
            Command::Allocate(AllocateCommand::Cancel)
        );

        assert_eq!(
            one("assert alloc=open"),
            Command::Assert(Assertion::AllocOpen(true))
        );
        assert_eq!(
            one("assert alloc-raw=4"),
            Command::Assert(Assertion::AllocRaw(Cmp::Eq, 4))
        );
        assert_eq!(
            one("assert alloc-free>=0"),
            Command::Assert(Assertion::AllocFree(Cmp::Ge, 0))
        );
        assert_eq!(
            one("assert complex-raw>=4"),
            Command::Assert(Assertion::ComplexRaw(Cmp::Ge, 4))
        );
        assert_eq!(
            one("assert complex-gold=1"),
            Command::Assert(Assertion::ComplexGold(Cmp::Eq, 1))
        );
        assert_eq!(one("complex raw 60"), Command::ComplexRaw(60));
        assert_eq!(one("complex fuel 40"), Command::ComplexFuel(40));
        assert_eq!(one("complex gold -5"), Command::ComplexGold(-5));

        for source in [
            "allocate nope",
            "allocate select nope",
            "allocate set",
            "allocate set raw nope",
            "allocate row sideways",
            "assert alloc=nope",
            "assert alloc-raw=nope",
        ] {
            assert!(
                parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
            );
        }
    }

    #[test]
    fn prefs_commands_are_parsed() {
        assert_eq!(one("prefs open"), Command::Prefs(PrefsCommand::Open));
        assert_eq!(
            one("prefs toggle effects"),
            Command::Prefs(PrefsCommand::Toggle("effects".to_string()))
        );
        assert_eq!(
            one("prefs toggle sound"),
            Command::Prefs(PrefsCommand::Toggle("sound".to_string()))
        );
        assert_eq!(
            one("prefs set quick_scroll 36"),
            Command::Prefs(PrefsCommand::Set {
                name: "quick_scroll".to_string(),
                value: "36".to_string()
            })
        );
        assert_eq!(
            one("prefs set timer 90"),
            Command::Prefs(PrefsCommand::Set {
                name: "timer".to_string(),
                value: "90".to_string()
            })
        );
        assert_eq!(
            one("prefs set player_name Commander"),
            Command::Prefs(PrefsCommand::Set {
                name: "player_name".to_string(),
                value: "Commander".to_string()
            })
        );
        assert_eq!(
            one("prefs set player_name Jane Doe"),
            Command::Prefs(PrefsCommand::Set {
                name: "player_name".to_string(),
                value: "Jane Doe".to_string()
            })
        );
        assert_eq!(
            one("assert prefs-text player_name=Commander"),
            Command::Assert(Assertion::PrefsText(
                crate::preferences::PrefKey::PlayerName,
                "Commander".to_string()
            ))
        );
        assert_eq!(one("prefs done"), Command::Prefs(PrefsCommand::Done));
        assert_eq!(one("prefs cancel"), Command::Prefs(PrefsCommand::Cancel));
        assert_eq!(one("prefs help"), Command::Prefs(PrefsCommand::Help));

        for source in [
            "prefs",
            "prefs nope",
            "prefs toggle",
            "prefs toggle bogus",
            "prefs toggle quick_scroll", // slider, not a checkbox
            "prefs set effects 1",       // checkbox, not a slider
            "prefs set quick_scroll nope",
            "prefs set quick_scroll",
        ] {
            assert!(
                parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
            );
        }
    }

    #[test]
    fn cmp_tests_match_the_operator() {
        assert!(Cmp::Eq.test(3, 3));
        assert!(!Cmp::Eq.test(3, 4));
        assert!(Cmp::Ge.test(3, 3));
        assert!(!Cmp::Ge.test(2, 3));
        assert!(Cmp::Le.test(2, 3));
        assert!(Cmp::Gt.test(4, 3));
        assert!(Cmp::Lt.test(2, 3));
        assert_eq!(Cmp::Ge.as_str(), ">=");
    }
}
