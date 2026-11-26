//! `Preferences`/`Setup` dialog — a port of C++ `OptionsMenu` (P8).
//!
//! # Oracle
//!
//! * `menu.cpp:2325` `menu_preferences_window(team)` → `OptionsMenu(team,
//!   PREFSPIC).Run()` (the game panel `Preferences` button,
//!   `GameManager_MenuClickPreferencesButton`, `game_manager.cpp:3918`);
//! * `menu.cpp:2331` `menu_setup_window()` → `OptionsMenu(PLAYER_TEAM_RED,
//!   SETUPPIC).Run() (the Setup item of the main menu);
//! * `optionsmenu.hpp/.cpp` — the options table, layout and behavior;
//! * `settings.cpp` — default values and the `SETUP`/`PREFERENCES`/
//!   `OPTIONS`; `IniPreferences` (7 × `signed int`) — `save.md`.
//!
//! # Window
//!
//! Both variants are drawn by **one** `PREFSPIC` panel 400×422 (`Window(id)` in C++
//! centers it: `ulx = (640−400)/2 = 120`, `uly = (480−422)/2 = 29`).
//! Title — `GNW_TEXT_FONT_5` at (108,12) 184×17 in green (`COLOR_GREEN`), the text
//! `Setup` (`7013`) or `Preferences` (`f70f`). Buttons in the bottom row
//! (`PRFDNE_U/D` 62×23 at (215,383), `PRFCAN_U/D` 62×23 at (125,383),
//! `PRFHLP_U/D` 25×23 at (188,383)); the `Done`/`Cancel` labels — `FONT5`,
//! `Fonts_GoldColor (165,175,199)`, like `Button::SetCaption` (in the constructor
//! `Text_SetFont(GNW_TEXT_FONT_5)` comes before the buttons are created).
//!
//! # Layout (verbatim `OptionsMenu::Init`)
//!
//! `window_uly` starts at 20; each section gives `+20`, each option with
//! `ulx == 25` — `+20`; a `LABEL` after drawing subtracts
//! `20 − (Text_GetHeight() + 3) = 7` (`FONT5` — 10 px). Hidden controls
//! (`IsControlVisible`) do not take a row. For `SETUPPIC` `Init` stops at
//! the `preferences` section, so the main menu shows only the `SETUP` part
//! (sound/video/autosave/network/name); for `PREFSPIC` — all sections, the debug
//! one (`invalid`) included when it is enabled (see below).
//!
//! | Section | Controls |
//! |---|---|
//! | `setup` | `Volume:` (label), `Enhanced graphics` (Setup only), Music/FX/Voice/Video volumes + `Disable…`, `Auto-Save Enabled`, `Host Address:`/`Host Port:` (Setup only), `Player Name:` |
//! | `preferences` | `Animate Effects`, `Click to Scroll`, `Double Unit Steps`, `Track Selected Unit`, `Halt movement when enemy is detected`, `Auto-Select Next Unit`, `Scroll Speed` |
//! | `options` | `Turn Time:`/`End Turn Time:` (edit int), `Play Mode: %s`, `Computer Player(s): %s`, `Game ends at %i %s.` (read-only) |
//! | `invalid` | debug only: `Disable Fire`, `Real Time`, `Red Team`/`Green Team`/`Blue Team`/`Gray Team` (edit int 0..3) |
//!
//! # Debug section (`invalid`, B6)
//!
//! `optionsmenu.hpp` keeps six controls after the `invalid` section marker:
//! `disable_fire` (`Disable Fire`, `d5fa`) and `real_time` (`Real Time`, `6a65`)
//! checkboxes plus the four team-control fields `red_team_player` (`3bda`),
//! `green_team_player` (`3bb7`), `blue_team_player` (`11a8`), `gray_team_player`
//! (`75a0`), each `0..=3` (C++ `TEAM_TYPE_NONE..TEAM_TYPE_REMOTE`). This is a
//! developer tool, not a player setting: `OptionsMenu::Init` builds it only in a
//! debug build (`#if !defined(NDEBUG)`) and only while `settings["debug"]` is set.
//! The same switch starts the list 20 px higher (`window_uly = -20`) and drops the
//! three read-only `options` labels, which is what makes the six extra rows fit
//! above the dialog buttons (`OptionsMenu::Init`).
//!
//! Effect split (rule C3 — a toggle never writes `GameState` from the UI):
//!
//! * `debug` — the gate itself: UI state, outside the snapshot;
//! * `disable_fire` — **simulation**: reaction fire off (`game.rs:357`,
//!   `unit_order::blocked_on_path_request`, `combat`), installed through
//!   `GameUniverse::set_disable_fire`;
//! * `real_time` — **simulation**: a path step does not spend `speed`
//!   (`game.rs:360`, `pathfinding`), installed through `GameUniverse::set_real_time`;
//! * `red/green/blue/gray_team_player` — **simulation**: the team-control slot
//!   (`TeamInfo::team_type`, `game_manager.cpp:3940-3943`), installed through
//!   `GameUniverse::set_team_type`.
//!
//! The values reach the match when the dialog closes (`Done`/`Cancel`), the point
//! where C++ `GameManager_MenuClickPreferencesButton` (`game_manager.cpp:3925-3943`)
//! re-reads them; the dialog is modal, so no simulation step runs while it is open
//! (for `disable_fire` C++ reads the setting live, which the modal window makes
//! indistinguishable). `debug` off — the default — installs nothing, so the
//! existing dialog behavior and the canonical digest are unchanged.
//!
//! Enabling: `debug=1` in `<save_dir>/preferences.ini` (the C++ `settings.json` in
//! `FilePathGamePref`), which the interactive launch loads before the game
//! (`App::load_preferences_from_disk`); the QA driver keeps the defaults and never
//! sees the section unless a scenario sets the setting.
//!
//! # Port-only observation switch: `no_fow`
//!
//! No control row (the `PREFSPIC` layout stays 1:1 with the oracle), a debug
//! flag instead: `--no-fow` on the command line or `no_fow=1` in
//! `preferences.ini`. It lifts the fog of war for the **presentation** — every
//! unit is drawn (`render`, `overlays`) and the minimap shows all cells and all
//! unit dots (`panels::draw_minimap`), i.e. the `GameManager_AllVisible`
//! observation mode — so the AI can be watched "in peace". Unlike
//! `AiState::all_visible` it does not touch `GameState`: the snapshot and the
//! CRC are unchanged.
//!
//! Slider: the label `Text_TextBox(ulx, uly, 185, 20)`, the `PRFSLIT` groove 57×3 at
//! `x = ulx + Text_GetWidth(caption) + 10`, `y = uly + 8`; the `PRFSLIDE` thumb
//! 14×17 at `x = track_x + value·57/range_max − 7`, `y = uly + 1`; a click on
//! the area `x + width(caption), uly, 77×20` sets
//! `value = (x − track_x)·range_max / 57` (`OptionsMenu::UpdateSlider`).
//! Checkbox — `UNCHKED`/`CHECKED` 18×17 at `(ulx, uly)`, the label at
//! `(ulx+25, uly−2) 155×24`. Field — `PREFEDIT` 60×19 (`PREFNAME` 187×17 for
//! the name) at `(ulx + width(caption) + 10, uly + (20−h)/2)`, the value centered
//! in color `0xA2`.
//!
//! # Behavior
//!
//! * Mouse: a click on a checkbox toggles the value, on a slider sets it by
//!   position, on a field starts editing; `Done`/`Cancel`/`Help`.
//! * Keyboard: `Enter` — Done, `Esc` — Cancel, `/`/`?` — Help (like
//!   `GNW_KB_KEY_SHIFT_DIVIDE`), `Tab`/`Up`/`Down` — focus, `Space` —
//!   toggle a checkbox, `Left`/`Right` — slider. In C++ there is no keyboard
//!   navigation; it was added for accessibility and does not change the look.
//!   For the input field `Enter` commits the value, `Esc` — too (like
//!   `ProcessTextEditKeyPress`: the first Esc while a field is active only
//!   leaves editing), `Backspace`/printable characters — editing.
//! * `Done` writes the values into `App::preferences`, saves them to
//!   `<save_dir>/preferences.ini` and into `SaveMeta::ini_preferences` (V70);
//!   `Cancel` restores the snapshot of the values at the moment of opening (like
//!   `last_rest_state` in C++). Changes to effects/sound are applied immediately —
//!   like `SetVolume`/`Halt*Playback` in C++.
//!
//! # What actually works (B4)
//!
//! * `Animate Effects` (`effects`) — disables frame-by-frame animations:
//!   `App::update_unit_animations` does not advance the builder frames, `ANIMATED` buildings
//!   (`RADAR`) and working factories, `render::unit_frame_index_with_animation`
//!   gets `effects = false` (the frame is static), like `settings["effects"]` in
//!   `units_manager.cpp:2857/3030` and `unitinfo.cpp:5057`;
//! * `FX Disabled` (`disable_fx`) — **SFX sound on/off**: `AudioPlayer::
//!   set_enabled(!disable_fx)`; `FX` — the `AudioPlayer::set_volume` volume
//!   (`fx_sound_level/100`), like `AUDIO_TYPE_SFX2` in C++;
//! * `Auto-Save Enabled` (`auto_save`) — `App::autosave` (slot 10);
//! * `Click to Scroll` (`click_scroll`) — `App::update_scroll` builds the offset
//!   only while the LMB is "held", otherwise scrolling goes by hover
//!   (`GameManager_SetGridOffset`, `game_manager.cpp:6351`);
//! * `Scroll Speed` (`quick_scroll`) — the camera step in screen pixels per 30 Hz
//!   frame (`GameManager_ZoomMainMapView`/`ScrollMainMapView`,
//!   `game_manager.cpp:1711/1652`);
//! * `Double Unit Steps` (`fast_movement`) — the unit sprite speed ×2
//!   (`UnitInfo::Move`: `velocity += 2`, `unitinfo.cpp:1742`);
//! * `Track Selected Unit` (`follow_unit`) — the camera centers on the selected
//!   unit when it leaves the map window (`UnitInfo::FollowUnit`,
//!   `unitinfo.cpp:3597`);
//! * `Auto-Select Next Unit` (`auto_select`) — after an order or construction
//!   completes, the next unit of the team is selected (`GameManager_AutoSelectNext`,
//!   `game_manager.cpp:1779`);
//! * `Halt movement when enemy is detected` (`enemy_halt`) — the simulation sets
//!   `UnitEventEmergencyStop` when an enemy is detected (`access.cpp:827`);
//! * `Turn Time`/`End Turn Time` (`timer`/`endturn`) — the same `settings` that
//!   `GameManager_UpdateGui` reads at the start of each turn; the match start takes
//!   them from the setup screen, `Preferences` changes them from the next turn;
//! * `Player Name` (`player_name`) — in a single game the team name
//!   (`menu.cpp:1944`); in game `PREFSPIC` edits the current team name
//!   (`menu_team_name_setting`, `optionsmenu.cpp:177/552`) — it is visible in the
//!   HUD log, on the victory screen and in the V70 save.
//!
//! # Campaign progression (`game_file_number`, `last_campaign`)
//!
//! Two `SETUP` settings of the oracle (`settings.cpp:101-105`, defaults `1`) hold
//! the campaign progression (plan M5, `campaign-missions.md`):
//!
//! * `game_file_number` — 1-based index of the mission selected last
//!   (`GameSetupMenu_Menu`, `menu.cpp:1979/2058`; the victory wrap-up writes the
//!   next mission, `menu.cpp:532`);
//! * `last_campaign` — how many campaign missions are unlocked (1-based count;
//!   `DrawMissionList` disables a row while its 0-based index is `>=` it,
//!   `gamesetupmenu.cpp:281`), raised on a win (`menu.cpp:534-536`) and clamped
//!   to the mission count when the list opens (`menu.cpp:1970-1982`).
//!
//! They are **persistent user state**, not UI flags: they live in this struct and
//! in `preferences.ini` (the port of the C++ settings file), survive a restart and
//! round-trip the file byte for byte. The simulation never sees them —
//! `snapshot_bytes`/CRC do not change. The menu layer only reads them; the
//! progression itself is applied by [`crate::campaign`] from the mission start and
//! the victory wrap-up.
//!
//! Saved but not active yet: `disable_music`/`disable_voice`/
//! `disable_movie` and the Music/Voice/Video levels (there is no music/voice/video in the
//! sound layer — D2/C3), `enhanced_graphics`, `host_address`/`host_port` (the network
//! is set by the CLI). Input differences: the window layer has no `MouseButtonUp`, so
//! the LMB "hold" for `click_scroll` — a pulse of
//! [`crate::scroll::CLICK_HOLD_SECONDS`].

use std::path::{Path, PathBuf};

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::{Opponent, PlayMode, VictoryType};
use max_core::save::IniPreferences;
use max_core::team::{TeamId, TeamKind};
use max_core::universe::Dispatcher;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, draw_text_gnw_shadowed, GnwFontId};

use crate::assets::Assets;
use crate::menu::{blit_simple, draw_big_image_at, wrap_gnw_text, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// `PREFSPIC` panel width.
pub const WINDOW_WIDTH: i32 = 400;
/// `PREFSPIC` panel height.
pub const WINDOW_HEIGHT: i32 = 422;
/// Window position in the 640×480 frame (`Window(id)` centers the resource).
pub const WINDOW_ULX: i32 = (640 - WINDOW_WIDTH) / 2;
/// Window position in the 640×480 frame.
pub const WINDOW_ULY: i32 = (480 - WINDOW_HEIGHT) / 2;

/// Title: `Text_TextBox(..., 108, 12, 184, 17, COLOR_GREEN, true, true)`.
pub const TITLE: PixelRect = PixelRect::new(108, 12, 184, 17);
/// `Done` button (`PRFDNE_U/D`, 62×23).
pub const DONE_BUTTON: PixelRect = PixelRect::new(215, 383, 62, 23);
/// `Cancel` button (`PRFCAN_U/D`, 62×23).
pub const CANCEL_BUTTON: PixelRect = PixelRect::new(125, 383, 62, 23);
/// `Help` button (`PRFHLP_U/D`, 25×23).
pub const HELP_BUTTON: PixelRect = PixelRect::new(188, 383, 25, 23);

/// `FONT5` (`Text_SetFont(GNW_TEXT_FONT_5)` in the constructor).
const LABEL_FONT: GnwFontId = GnwFontId::Font5;
/// Label row height in `Text_TextBox` (`Text_GetHeight() + 1`).
const LINE_HEIGHT: i32 = 11;
/// Slider groove width (`PRFSLIT`, 57×3).
pub const SLIDER_SLIT_WIDTH: i32 = 57;
/// Slider area height (`Text_TextBox` 20 px).
const SLIDER_ROW_HEIGHT: i32 = 20;

/// Title `COLOR_GREEN`.
const GREEN: u8 = 0x02;
/// `Fonts_GoldColor` — labels/buttons.
const GOLD: (u8, u8, u8) = (165, 175, 199);
/// `Fonts_BrightYellowColor` — read-only values of the `options` section.
const BRIGHT_YELLOW: (u8, u8, u8) = (162, 8, 199);
/// Value color in the input field (`0xA2`).
const EDIT_TEXT: u8 = 0xA2;

/// Input field size by control key.
const EDIT_W: i32 = 60;
/// `PREFEDIT` height.
const EDIT_H: i32 = 19;
/// `PREFNAME` width.
const NAME_W: i32 = 187;
/// `PREFNAME` height.
const NAME_H: i32 = 17;

/// Dialog variant: `SETUPPIC` (main menu) or `PREFSPIC` (game).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesVariant {
    /// `menu_setup_window()`: only the `setup` section.
    Setup,
    /// `menu_preferences_window()`: all sections, plus the debug one (`invalid`)
    /// when it is enabled.
    InGame,
}

impl PreferencesVariant {
    /// Window title (`_(7013)`/`_(f70f)`).
    pub const fn title(self) -> &'static str {
        match self {
            Self::Setup => "Setup",
            Self::InGame => "Preferences",
        }
    }

    /// Help section (`SETUP_MENU_SETUP`/`PREFS_MENU_SETUP`).
    pub const fn help_section(self) -> crate::helpmenu::HelpSection {
        match self {
            Self::Setup => crate::helpmenu::HelpSection::Setup,
            Self::InGame => crate::helpmenu::HelpSection::Preferences,
        }
    }
}

/// Option key (the C++ setting name / our `Preferences` field).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefKey {
    /// `setup` section.
    SectionSetup,
    /// `Volume:` label (label, `music_level`).
    MusicVolumeLabel,
    /// `enhanced_graphics` (only `SETUPPIC`).
    EnhancedGraphics,
    /// `music_level` (slider 0..100).
    MusicLevel,
    /// `disable_music`.
    DisableMusic,
    /// `fx_sound_level`.
    FxSoundLevel,
    /// `disable_fx`.
    DisableFx,
    /// `voice_level`.
    VoiceLevel,
    /// `disable_voice`.
    DisableVoice,
    /// `movie_level`.
    MovieLevel,
    /// `disable_movie`.
    DisableMovie,
    /// `auto_save`.
    AutoSave,
    /// `host_address` (only `SETUPPIC`).
    HostAddress,
    /// `host_port` (only `SETUPPIC`).
    HostPort,
    /// `player_name`.
    PlayerName,
    /// `preferences` section.
    SectionPreferences,
    /// `effects`.
    Effects,
    /// `click_scroll`.
    ClickScroll,
    /// `fast_movement`.
    FastMovement,
    /// `follow_unit`.
    FollowUnit,
    /// `enemy_halt`.
    EnemyHalt,
    /// `auto_select`.
    AutoSelect,
    /// `quick_scroll` (slider 4..128).
    QuickScroll,
    /// `options` section.
    SectionOptions,
    /// `timer`.
    TurnTimer,
    /// `endturn`.
    EndTurn,
    /// `play_mode` (label).
    PlayModeLabel,
    /// `opponent` (label).
    OpponentLabel,
    /// `victory_limit` (label).
    VictoryLabel,
    /// `invalid` section: the debug controls start here.
    SectionInvalid,
    /// `disable_fire` (debug section).
    DisableFire,
    /// `real_time` (debug section).
    RealTime,
    /// `red_team_player` (debug section, `0..=3`).
    RedTeamPlayer,
    /// `green_team_player` (debug section, `0..=3`).
    GreenTeamPlayer,
    /// `blue_team_player` (debug section, `0..=3`).
    BlueTeamPlayer,
    /// `gray_team_player` (debug section, `0..=3`).
    GrayTeamPlayer,
}

impl PrefKey {
    /// Setting name for QA/logs (`effects`, `disable_fx`, …).
    pub const fn name(self) -> &'static str {
        match self {
            Self::SectionSetup => "setup",
            Self::MusicVolumeLabel => "music_label",
            Self::EnhancedGraphics => "enhanced_graphics",
            Self::MusicLevel => "music_level",
            Self::DisableMusic => "disable_music",
            Self::FxSoundLevel => "fx_sound_level",
            Self::DisableFx => "disable_fx",
            Self::VoiceLevel => "voice_level",
            Self::DisableVoice => "disable_voice",
            Self::MovieLevel => "movie_level",
            Self::DisableMovie => "disable_movie",
            Self::AutoSave => "auto_save",
            Self::HostAddress => "host_address",
            Self::HostPort => "host_port",
            Self::PlayerName => "player_name",
            Self::SectionPreferences => "preferences",
            Self::Effects => "effects",
            Self::ClickScroll => "click_scroll",
            Self::FastMovement => "fast_movement",
            Self::FollowUnit => "follow_unit",
            Self::EnemyHalt => "enemy_halt",
            Self::AutoSelect => "auto_select",
            Self::QuickScroll => "quick_scroll",
            Self::SectionOptions => "options",
            Self::TurnTimer => "timer",
            Self::EndTurn => "endturn",
            Self::PlayModeLabel => "play_mode",
            Self::OpponentLabel => "opponent",
            Self::VictoryLabel => "victory_limit",
            Self::SectionInvalid => "invalid",
            Self::DisableFire => "disable_fire",
            Self::RealTime => "real_time",
            Self::RedTeamPlayer => "red_team_player",
            Self::GreenTeamPlayer => "green_team_player",
            Self::BlueTeamPlayer => "blue_team_player",
            Self::GrayTeamPlayer => "gray_team_player",
        }
    }

    /// Parsing of the name (QA `prefs toggle …`); case and `-`/`_` do not matter.
    pub fn parse(text: &str) -> Option<Self> {
        let name = text.trim().to_ascii_lowercase().replace('-', "_");
        let key = match name.as_str() {
            "music" | "music_level" | "volume" => Self::MusicLevel,
            "enhanced_graphics" | "enhanced" | "graphics" => Self::EnhancedGraphics,
            "disable_music" | "music_off" => Self::DisableMusic,
            "fx" | "fx_sound_level" | "fx_level" => Self::FxSoundLevel,
            "disable_fx" | "fx_off" | "sound" => Self::DisableFx,
            "voice" | "voice_level" => Self::VoiceLevel,
            "disable_voice" | "voice_off" => Self::DisableVoice,
            "video" | "movie" | "movie_level" => Self::MovieLevel,
            "disable_movie" | "disable_video" | "movie_off" | "video_off" => Self::DisableMovie,
            "auto_save" | "autosave" => Self::AutoSave,
            "host_address" | "host" => Self::HostAddress,
            "host_port" | "port" => Self::HostPort,
            "player_name" | "name" => Self::PlayerName,
            "effects" | "animate_effects" => Self::Effects,
            "click_scroll" | "click_to_scroll" => Self::ClickScroll,
            "fast_movement" | "double_steps" | "double_unit_steps" => Self::FastMovement,
            "follow_unit" | "track_unit" | "track_selected_unit" => Self::FollowUnit,
            "enemy_halt" | "halt_enemy" => Self::EnemyHalt,
            "auto_select" | "auto_select_next_unit" => Self::AutoSelect,
            "quick_scroll" | "scroll_speed" => Self::QuickScroll,
            "timer" | "turn_time" | "turn_timer" => Self::TurnTimer,
            "endturn" | "end_turn" | "end_turn_time" => Self::EndTurn,
            "play_mode" => Self::PlayModeLabel,
            "opponent" | "computer_players" => Self::OpponentLabel,
            "victory" | "victory_limit" => Self::VictoryLabel,
            "disable_fire" | "no_fire" | "fire_off" => Self::DisableFire,
            "real_time" | "realtime" => Self::RealTime,
            "red_team_player" | "red_team" => Self::RedTeamPlayer,
            "green_team_player" | "green_team" => Self::GreenTeamPlayer,
            "blue_team_player" | "blue_team" => Self::BlueTeamPlayer,
            "gray_team_player" | "gray_team" => Self::GrayTeamPlayer,
            _ => return None,
        };

        Some(key)
    }

    /// The control toggles on click/`Space` (checkbox).
    pub const fn is_checkbox(self) -> bool {
        matches!(
            self,
            Self::EnhancedGraphics
                | Self::DisableMusic
                | Self::DisableFx
                | Self::DisableVoice
                | Self::DisableMovie
                | Self::AutoSave
                | Self::Effects
                | Self::ClickScroll
                | Self::FastMovement
                | Self::FollowUnit
                | Self::EnemyHalt
                | Self::AutoSelect
                | Self::DisableFire
                | Self::RealTime
        )
    }

    /// The control is a slider.
    pub const fn is_slider(self) -> bool {
        matches!(
            self,
            Self::MusicLevel | Self::FxSoundLevel | Self::VoiceLevel | Self::MovieLevel | Self::QuickScroll
        )
    }

    /// The control is an input field.
    pub const fn is_edit(self) -> bool {
        matches!(
            self,
            Self::HostAddress
                | Self::HostPort
                | Self::PlayerName
                | Self::TurnTimer
                | Self::EndTurn
                | Self::RedTeamPlayer
                | Self::GreenTeamPlayer
                | Self::BlueTeamPlayer
                | Self::GrayTeamPlayer
        )
    }

    /// The team-control slot of a debug field (`red_team_player` — 0 … `gray_team_player` — 3).
    pub const fn team_slot(self) -> Option<usize> {
        match self {
            Self::RedTeamPlayer => Some(0),
            Self::GreenTeamPlayer => Some(1),
            Self::BlueTeamPlayer => Some(2),
            Self::GrayTeamPlayer => Some(3),
            _ => None,
        }
    }

    /// Slider/number field bounds (`range_min`, `range_max` from C++).
    pub const fn range(self) -> (i32, i32) {
        match self {
            Self::QuickScroll => (4, 128),
            Self::HostPort => (1024, 65535),
            Self::TurnTimer | Self::EndTurn => (0, 32767),
            // `red_team_player` … `gray_team_player`: `TEAM_TYPE_NONE..=TEAM_TYPE_REMOTE`.
            Self::RedTeamPlayer
            | Self::GreenTeamPlayer
            | Self::BlueTeamPlayer
            | Self::GrayTeamPlayer => (0, 3),
            _ => (0, 100),
        }
    }
}

/// Control type (`OptionsType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    /// Section separator (not drawn).
    Section,
    /// Label/value only.
    Label,
    /// Checkbox `UNCHKED`/`CHECKED`.
    Checkbox,
    /// Slider with the `PRFSLIT` groove and `PRFSLIDE` thumb.
    Slider,
    /// Number field `PREFEDIT`.
    EditInt,
    /// String field `PREFEDIT`/`PREFNAME`.
    EditStr,
}

/// An option of the `options_menu_buttons` table (in C++ order).
#[derive(Debug, Clone, Copy)]
struct Def {
    key: PrefKey,
    kind: ControlKind,
    caption: &'static str,
    ulx: i32,
}

const fn def(key: PrefKey, kind: ControlKind, caption: &'static str, ulx: i32) -> Def {
    Def {
        key,
        kind,
        caption,
        ulx,
    }
}

/// Verbatim `OptionsMenu::options_menu_buttons` table (36 rows).
const DEFS: [Def; 36] = [
    def(PrefKey::SectionSetup, ControlKind::Section, "", 0),
    def(PrefKey::MusicVolumeLabel, ControlKind::Label, "Volume:", 25),
    def(
        PrefKey::EnhancedGraphics,
        ControlKind::Checkbox,
        "Enhanced graphics (requires 16MB)",
        210,
    ),
    def(PrefKey::MusicLevel, ControlKind::Slider, "Music", 25),
    def(PrefKey::DisableMusic, ControlKind::Checkbox, "Disable Music", 210),
    def(PrefKey::FxSoundLevel, ControlKind::Slider, "FX", 25),
    def(PrefKey::DisableFx, ControlKind::Checkbox, "FX Disabled", 210),
    def(PrefKey::VoiceLevel, ControlKind::Slider, "Voice", 25),
    def(PrefKey::DisableVoice, ControlKind::Checkbox, "Voice Disabled", 210),
    def(PrefKey::MovieLevel, ControlKind::Slider, "Video", 25),
    def(PrefKey::DisableMovie, ControlKind::Checkbox, "Disable Video", 210),
    def(PrefKey::AutoSave, ControlKind::Checkbox, "Auto-Save Enabled", 25),
    def(PrefKey::HostAddress, ControlKind::EditStr, "Host Address:", 25),
    def(PrefKey::HostPort, ControlKind::EditInt, "Host Port:", 25),
    def(PrefKey::PlayerName, ControlKind::EditStr, "Player Name:", 25),
    def(PrefKey::SectionPreferences, ControlKind::Section, "", 0),
    def(PrefKey::Effects, ControlKind::Checkbox, "Animate Effects", 25),
    def(PrefKey::ClickScroll, ControlKind::Checkbox, "Click to Scroll", 210),
    def(PrefKey::FastMovement, ControlKind::Checkbox, "Double Unit Steps", 25),
    def(PrefKey::FollowUnit, ControlKind::Checkbox, "Track Selected Unit", 210),
    def(
        PrefKey::EnemyHalt,
        ControlKind::Checkbox,
        "Halt movement when enemy is detected",
        25,
    ),
    def(PrefKey::AutoSelect, ControlKind::Checkbox, "Auto-Select Next Unit", 210),
    def(PrefKey::QuickScroll, ControlKind::Slider, "Scroll Speed", 25),
    def(PrefKey::SectionOptions, ControlKind::Section, "", 0),
    def(PrefKey::TurnTimer, ControlKind::EditInt, "Turn Time:", 25),
    def(PrefKey::EndTurn, ControlKind::EditInt, "End Turn Time:", 210),
    def(PrefKey::PlayModeLabel, ControlKind::Label, "Play Mode: %s", 25),
    def(
        PrefKey::OpponentLabel,
        ControlKind::Label,
        "Computer Player(s): %s",
        25,
    ),
    def(PrefKey::VictoryLabel, ControlKind::Label, "Game ends at %i %s.", 25),
    def(PrefKey::SectionInvalid, ControlKind::Section, "", 0),
    // Debug section (`invalid`, `options_menu_buttons` 30..35). Built only in a
    // debug build with `settings["debug"]` set (`OptionsMenu::Init`): two checkboxes
    // and the four team-control fields, captions `d5fa`, `6a65`, `3bda`, `3bb7`,
    // `11a8`, `75a0`.
    def(PrefKey::DisableFire, ControlKind::Checkbox, "Disable Fire", 25),
    def(PrefKey::RealTime, ControlKind::Checkbox, "Real Time", 210),
    def(PrefKey::RedTeamPlayer, ControlKind::EditInt, "Red Team", 25),
    def(PrefKey::GreenTeamPlayer, ControlKind::EditInt, "Green Team", 210),
    def(PrefKey::BlueTeamPlayer, ControlKind::EditInt, "Blue Team", 25),
    def(PrefKey::GrayTeamPlayer, ControlKind::EditInt, "Gray Team", 210),
];

/// `play_mode`/`opponent`/`victory_*` values for the read-only labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreferencesInfo {
    /// Game mode (`PlayMode`).
    pub play_mode: PlayMode,
    /// AI difficulty (`Opponent`).
    pub opponent: Opponent,
    /// Victory limit (`victory_limit`).
    pub victory_limit: i32,
    /// Victory type (`victory_type`).
    pub victory_type: VictoryType,
}

impl Default for PreferencesInfo {
    fn default() -> Self {
        Self {
            play_mode: PlayMode::SimultaneousMoves,
            opponent: Opponent::Apprentice,
            victory_limit: 50,
            victory_type: VictoryType::Duration,
        }
    }
}

impl PreferencesInfo {
    /// `Play Mode: %s` label (`d0a7`/`ee25`).
    pub fn play_mode_text(self) -> String {
        let value = match self.play_mode {
            PlayMode::TurnBased => "Turn Based",
            PlayMode::SimultaneousMoves => "Simultaneous Moves",
            PlayMode::Unknown => "Unknown",
        };

        format!("Play Mode: {value}")
    }

    /// `Computer Player(s): %s` label (`a24d`…`c7e3`).
    pub fn opponent_text(self) -> String {
        let value = match self.opponent {
            Opponent::Clueless => "Clueless",
            Opponent::Apprentice => "Apprentice",
            Opponent::Average => "Average",
            Opponent::Expert => "Expert",
            Opponent::Master => "Master",
            Opponent::God => "God",
        };

        format!("Computer Player(s): {value}")
    }

    /// `Game ends at %i %s.` label (`b542`/`c303`).
    pub fn victory_text(self) -> String {
        let unit = match self.victory_type {
            VictoryType::Duration => "turns",
            VictoryType::Score => "points",
        };

        format!("Game ends at {} {unit}.", self.victory_limit)
    }
}

/// `PLAYER_TEAM_MAX - 1` — the four playing teams the debug section can re-assign.
pub const DEBUG_TEAM_COUNT: usize = 4;

/// User settings (`SETUP` + `PREFERENCES` + part of `OPTIONS` + `DEBUG`).
///
/// Default values — `settings.cpp`: `effects=1`, `click_scroll=1`,
/// `quick_scroll=16`, `fast_movement=1`, `follow_unit=0`, `auto_select=0`,
/// `enemy_halt=1`; volumes 100, `auto_save=1`, `enhanced_graphics=1`,
/// `game_file_number=1`, `last_campaign=1`, `player_name="Player 1"`,
/// `timer=180`, `endturn=45`. Debug (`settings.cpp`
/// `DEBUG`/`TEAMS`): `debug=0`, `disable_fire=0`, `real_time=0`, team control
/// `red=1` (player), `green=2` (computer), `blue=0`, `gray=0` (`TEAM_TYPE_NONE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    /// `effects` — «Animate Effects».
    pub effects: bool,
    /// `click_scroll` — «Click to Scroll».
    pub click_scroll: bool,
    /// `quick_scroll` — «Scroll Speed», 4..=128.
    pub quick_scroll: i32,
    /// `fast_movement` — «Double Unit Steps».
    pub fast_movement: bool,
    /// `follow_unit` — «Track Selected Unit».
    pub follow_unit: bool,
    /// `auto_select` — «Auto-Select Next Unit».
    pub auto_select: bool,
    /// `enemy_halt` — «Halt movement when enemy is detected».
    pub enemy_halt: bool,
    /// `music_level`, 0..=100.
    pub music_level: i32,
    /// `fx_sound_level`, 0..=100.
    pub fx_sound_level: i32,
    /// `voice_level`, 0..=100.
    pub voice_level: i32,
    /// `movie_level`, 0..=100.
    pub movie_level: i32,
    /// `disable_music`.
    pub disable_music: bool,
    /// `disable_fx`.
    pub disable_fx: bool,
    /// `disable_voice`.
    pub disable_voice: bool,
    /// `disable_movie`.
    pub disable_movie: bool,
    /// `auto_save`.
    pub auto_save: bool,
    /// `game_file_number` — 1-based mission selected last (`settings.cpp:102`).
    ///
    /// Persistent campaign progression state (`campaign::`): the mission start sets
    /// it (`menu.cpp:2058`) and the victory wrap-up moves it to the next mission
    /// (`menu.cpp:532`). Not a UI flag and not part of the snapshot.
    pub game_file_number: i32,
    /// `enhanced_graphics`.
    pub enhanced_graphics: bool,
    /// `last_campaign` — number of unlocked campaign missions (`settings.cpp:105`),
    /// a 1-based count (`gamesetupmenu.cpp:281`). Persistent progression state
    /// raised on a campaign win (`menu.cpp:534-536`) and clamped to the mission
    /// list when the campaign menu opens (`menu.cpp:1970-1982`).
    pub last_campaign: i32,
    /// `player_name`.
    pub player_name: String,
    /// `host_address`.
    pub host_address: String,
    /// `host_port`, 1024..=65535.
    pub host_port: i32,
    /// `timer` (sec).
    pub turn_timer: i32,
    /// `endturn` (sec).
    pub endturn: i32,
    /// `debug` — the `invalid` debug section gate (`settings.cpp` `DEBUG`).
    ///
    /// Pure UI state: it decides whether the six debug controls are laid out and
    /// installed; it is not part of the snapshot. The section itself is built into
    /// debug builds only ([`DEBUG_SECTION_BUILT`]).
    pub debug: bool,
    /// `disable_fire` — reaction fire off (simulation, `game.rs:357`).
    pub disable_fire: bool,
    /// `real_time` — a path step does not spend `speed` (`GameManager_RealTime`,
    /// `game.rs:360`).
    pub real_time: bool,
    /// `red_team_player` … `gray_team_player` — who controls each team
    /// (`menu_team_player_setting`, `settings.cpp` `TEAMS`), `0..=3`.
    pub team_players: [TeamKind; DEBUG_TEAM_COUNT],
    /// `no_fow` — **debug/presentation only**: draw every unit and the whole
    /// minimap without the fog of war (the `GameManager_AllVisible` observation
    /// mode of the oracle). Not a player setting and not part of the dialog: it
    /// is switched by the `--no-fow` command line flag or the
    /// `no_fow=1` line of `preferences.ini`, and it never writes the
    /// simulation state (`GameState`/CRC unchanged).
    pub no_fow: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            effects: true,
            click_scroll: true,
            quick_scroll: 16,
            fast_movement: true,
            follow_unit: false,
            auto_select: false,
            enemy_halt: true,
            music_level: 100,
            fx_sound_level: 100,
            voice_level: 100,
            movie_level: 100,
            disable_music: false,
            disable_fx: false,
            disable_voice: false,
            disable_movie: false,
            auto_save: true,
            game_file_number: 1,
            enhanced_graphics: true,
            last_campaign: 1,
            player_name: "Player 1".to_string(),
            host_address: "127.0.0.1".to_string(),
            host_port: 31554,
            turn_timer: 180,
            endturn: 45,
            debug: false,
            disable_fire: false,
            real_time: false,
            team_players: [
                TeamKind::Player,
                TeamKind::Computer,
                TeamKind::None,
                TeamKind::None,
            ],
            no_fow: false,
        }
    }
}

impl Preferences {
    /// `IniPreferences` (7 V70 fields: `effects`, `click_scroll`,
    /// `quick_scroll`, `fast_movement`, `follow_unit`, `auto_select`,
    /// `enemy_halt`).
    pub fn to_ini(&self) -> IniPreferences {
        IniPreferences {
            effects: i32::from(self.effects),
            click_scroll: i32::from(self.click_scroll),
            quick_scroll: self.quick_scroll,
            fast_movement: i32::from(self.fast_movement),
            follow_unit: i32::from(self.follow_unit),
            auto_select: i32::from(self.auto_select),
            enemy_halt: i32::from(self.enemy_halt),
        }
    }

    /// Applies `IniPreferences` from a save (like `SaveLoad_Load`:
    /// `settings->SetNumericValue(...)` for the seven fields).
    pub fn apply_ini(&mut self, ini: IniPreferences) {
        self.effects = ini.effects != 0;
        self.click_scroll = ini.click_scroll != 0;
        self.quick_scroll = ini.quick_scroll.clamp(4, 128);
        self.fast_movement = ini.fast_movement != 0;
        self.follow_unit = ini.follow_unit != 0;
        self.auto_select = ini.auto_select != 0;
        self.enemy_halt = ini.enemy_halt != 0;
    }

    /// SFX sound enabled (`!disable_fx`).
    pub fn sound_enabled(&self) -> bool {
        !self.disable_fx
    }

    /// Voices enabled (`!disable_voice`) — C++ `AUDIO_TYPE_VOICE`.
    pub fn voice_enabled(&self) -> bool {
        !self.disable_voice
    }

    /// Music enabled (`!disable_music`) — C++ `AUDIO_TYPE_MUSIC` (`D2`).
    pub fn music_enabled(&self) -> bool {
        !self.disable_music
    }

    /// Music volume 0.0..=1.0 (`music_level / 100`, `SoundManager::PlayMusic` gain).
    pub fn music_volume(&self) -> f32 {
        self.music_level.clamp(0, 100) as f32 / 100.0
    }

    /// Movie sound enabled (`!disable_movie`) — C++ `movie_run` (`movie_level`).
    pub fn movie_enabled(&self) -> bool {
        !self.disable_movie
    }

    /// Movie sound volume 0.0..=1.0 (`movie_level / 100`, `MVE_sndVolume`).
    pub fn movie_volume(&self) -> f32 {
        self.movie_level.clamp(0, 100) as f32 / 100.0
    }

    /// SFX volume 0.0..=1.0 (`fx_sound_level / 100`).
    pub fn sound_volume(&self) -> f32 {
        self.fx_sound_level.clamp(0, 100) as f32 / 100.0
    }

    /// Checkbox value by key.
    pub fn bool_value(&self, key: PrefKey) -> Option<bool> {
        Some(match key {
            PrefKey::EnhancedGraphics => self.enhanced_graphics,
            PrefKey::DisableMusic => self.disable_music,
            PrefKey::DisableFx => self.disable_fx,
            PrefKey::DisableVoice => self.disable_voice,
            PrefKey::DisableMovie => self.disable_movie,
            PrefKey::AutoSave => self.auto_save,
            PrefKey::Effects => self.effects,
            PrefKey::ClickScroll => self.click_scroll,
            PrefKey::FastMovement => self.fast_movement,
            PrefKey::FollowUnit => self.follow_unit,
            PrefKey::EnemyHalt => self.enemy_halt,
            PrefKey::AutoSelect => self.auto_select,
            PrefKey::DisableFire => self.disable_fire,
            PrefKey::RealTime => self.real_time,
            _ => return None,
        })
    }

    /// Numeric value by key.
    pub fn int_value(&self, key: PrefKey) -> Option<i32> {
        if let Some(slot) = key.team_slot() {
            return Some(team_kind_value(self.team_players[slot]));
        }

        Some(match key {
            PrefKey::MusicLevel => self.music_level,
            PrefKey::FxSoundLevel => self.fx_sound_level,
            PrefKey::VoiceLevel => self.voice_level,
            PrefKey::MovieLevel => self.movie_level,
            PrefKey::QuickScroll => self.quick_scroll,
            PrefKey::HostPort => self.host_port,
            PrefKey::TurnTimer => self.turn_timer,
            PrefKey::EndTurn => self.endturn,
            _ => return None,
        })
    }

    /// String value by key.
    pub fn string_value(&self, key: PrefKey) -> Option<&str> {
        match key {
            PrefKey::PlayerName => Some(&self.player_name),
            PrefKey::HostAddress => Some(&self.host_address),
            _ => None,
        }
    }

    /// Sets a checkbox; `false` — the key is not a checkbox.
    pub fn set_bool(&mut self, key: PrefKey, value: bool) -> bool {
        match key {
            PrefKey::EnhancedGraphics => self.enhanced_graphics = value,
            PrefKey::DisableMusic => self.disable_music = value,
            PrefKey::DisableFx => self.disable_fx = value,
            PrefKey::DisableVoice => self.disable_voice = value,
            PrefKey::DisableMovie => self.disable_movie = value,
            PrefKey::AutoSave => self.auto_save = value,
            PrefKey::Effects => self.effects = value,
            PrefKey::ClickScroll => self.click_scroll = value,
            PrefKey::FastMovement => self.fast_movement = value,
            PrefKey::FollowUnit => self.follow_unit = value,
            PrefKey::EnemyHalt => self.enemy_halt = value,
            PrefKey::AutoSelect => self.auto_select = value,
            PrefKey::DisableFire => self.disable_fire = value,
            PrefKey::RealTime => self.real_time = value,
            _ => return false,
        }

        true
    }

    /// Sets a number clamped to the control bounds.
    pub fn set_int(&mut self, key: PrefKey, value: i32) -> bool {
        let (min, max) = key.range();
        let value = value.clamp(min, max);

        if let Some(slot) = key.team_slot() {
            self.team_players[slot] = team_kind_from_value(value);
            return true;
        }

        match key {
            PrefKey::MusicLevel => self.music_level = value,
            PrefKey::FxSoundLevel => self.fx_sound_level = value,
            PrefKey::VoiceLevel => self.voice_level = value,
            PrefKey::MovieLevel => self.movie_level = value,
            PrefKey::QuickScroll => self.quick_scroll = value,
            PrefKey::HostPort => self.host_port = value,
            PrefKey::TurnTimer => self.turn_timer = value,
            PrefKey::EndTurn => self.endturn = value,
            _ => return false,
        }

        true
    }

    /// String value by key (for committing the input field).
    pub fn set_string(&mut self, key: PrefKey, value: &str) -> bool {
        match key {
            PrefKey::PlayerName => self.player_name = value.to_string(),
            PrefKey::HostAddress => self.host_address = value.to_string(),
            _ => return false,
        }

        true
    }

    /// Reads `preferences.ini` (unknown keys are ignored); `None` —
    /// no file/not readable.
    pub fn load_file(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut prefs = Self::default();

        for line in text.lines() {
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();

            match key {
                "effects" => prefs.effects = parse_bool(value, prefs.effects),
                "click_scroll" => prefs.click_scroll = parse_bool(value, prefs.click_scroll),
                "quick_scroll" => {
                    prefs.quick_scroll = parse_int(value).unwrap_or(prefs.quick_scroll).clamp(4, 128)
                }
                "fast_movement" => prefs.fast_movement = parse_bool(value, prefs.fast_movement),
                "follow_unit" => prefs.follow_unit = parse_bool(value, prefs.follow_unit),
                "auto_select" => prefs.auto_select = parse_bool(value, prefs.auto_select),
                "enemy_halt" => prefs.enemy_halt = parse_bool(value, prefs.enemy_halt),
                "music_level" => {
                    prefs.music_level = parse_int(value).unwrap_or(prefs.music_level).clamp(0, 100)
                }
                "fx_sound_level" => {
                    prefs.fx_sound_level =
                        parse_int(value).unwrap_or(prefs.fx_sound_level).clamp(0, 100)
                }
                "voice_level" => {
                    prefs.voice_level = parse_int(value).unwrap_or(prefs.voice_level).clamp(0, 100)
                }
                "movie_level" => {
                    prefs.movie_level = parse_int(value).unwrap_or(prefs.movie_level).clamp(0, 100)
                }
                "disable_music" => prefs.disable_music = parse_bool(value, prefs.disable_music),
                "disable_fx" => prefs.disable_fx = parse_bool(value, prefs.disable_fx),
                "disable_voice" => prefs.disable_voice = parse_bool(value, prefs.disable_voice),
                "disable_movie" => prefs.disable_movie = parse_bool(value, prefs.disable_movie),
                "auto_save" => prefs.auto_save = parse_bool(value, prefs.auto_save),
                // Campaign progression (`settings.cpp` SETUP): raw integers, no
                // clamping on load — the clamp is the campaign menu's rule
                // (`campaign::menu_open`, `menu.cpp:1970-1982`).
                "game_file_number" => {
                    prefs.game_file_number = parse_int(value).unwrap_or(prefs.game_file_number)
                }
                "last_campaign" => {
                    prefs.last_campaign = parse_int(value).unwrap_or(prefs.last_campaign)
                }
                "enhanced_graphics" => {
                    prefs.enhanced_graphics = parse_bool(value, prefs.enhanced_graphics)
                }
                "player_name" => prefs.player_name = value.to_string(),
                "host_address" => prefs.host_address = value.to_string(),
                "host_port" => {
                    prefs.host_port = parse_int(value).unwrap_or(prefs.host_port).clamp(1024, 65535)
                }
                "timer" => prefs.turn_timer = parse_int(value).unwrap_or(prefs.turn_timer),
                "endturn" => prefs.endturn = parse_int(value).unwrap_or(prefs.endturn),
                // DEBUG/TEAMS section (`settings.cpp`): the debug dialog section.
                "debug" => prefs.debug = parse_bool(value, prefs.debug),
                "disable_fire" => prefs.disable_fire = parse_bool(value, prefs.disable_fire),
                "real_time" => prefs.real_time = parse_bool(value, prefs.real_time),
                // Debug observation mode: draw the world without the fog of war.
                "no_fow" => prefs.no_fow = parse_bool(value, prefs.no_fow),
                "red_team_player" => {
                    prefs.team_players[0] = parse_team_kind(value, prefs.team_players[0])
                }
                "green_team_player" => {
                    prefs.team_players[1] = parse_team_kind(value, prefs.team_players[1])
                }
                "blue_team_player" => {
                    prefs.team_players[2] = parse_team_kind(value, prefs.team_players[2])
                }
                "gray_team_player" => {
                    prefs.team_players[3] = parse_team_kind(value, prefs.team_players[3])
                }
                _ => {}
            }
        }

        Some(prefs)
    }

    /// Writes `preferences.ini` (creates the directory).
    pub fn save_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut out = String::new();
        out.push_str("# M.A.X. Rust client preferences (C++ settings.cpp SETUP/PREFERENCES/OPTIONS)\n");
        push_bool(&mut out, "effects", self.effects);
        push_bool(&mut out, "click_scroll", self.click_scroll);
        out.push_str(&format!("quick_scroll={}\n", self.quick_scroll));
        push_bool(&mut out, "fast_movement", self.fast_movement);
        push_bool(&mut out, "follow_unit", self.follow_unit);
        push_bool(&mut out, "auto_select", self.auto_select);
        push_bool(&mut out, "enemy_halt", self.enemy_halt);
        out.push_str(&format!("music_level={}\n", self.music_level));
        out.push_str(&format!("fx_sound_level={}\n", self.fx_sound_level));
        out.push_str(&format!("voice_level={}\n", self.voice_level));
        out.push_str(&format!("movie_level={}\n", self.movie_level));
        push_bool(&mut out, "disable_music", self.disable_music);
        push_bool(&mut out, "disable_fx", self.disable_fx);
        push_bool(&mut out, "disable_voice", self.disable_voice);
        push_bool(&mut out, "disable_movie", self.disable_movie);
        push_bool(&mut out, "auto_save", self.auto_save);
        out.push_str(&format!("game_file_number={}\n", self.game_file_number));
        push_bool(&mut out, "enhanced_graphics", self.enhanced_graphics);
        out.push_str(&format!("last_campaign={}\n", self.last_campaign));
        out.push_str(&format!("player_name={}\n", self.player_name));
        out.push_str(&format!("host_address={}\n", self.host_address));
        out.push_str(&format!("host_port={}\n", self.host_port));
        out.push_str(&format!("timer={}\n", self.turn_timer));
        out.push_str(&format!("endturn={}\n", self.endturn));
        // DEBUG/TEAMS (`settings.cpp`): the `invalid` debug dialog section.
        push_bool(&mut out, "debug", self.debug);
        push_bool(&mut out, "disable_fire", self.disable_fire);
        push_bool(&mut out, "real_time", self.real_time);
        push_bool(&mut out, "no_fow", self.no_fow);
        for (slot, name) in [
            (0, "red_team_player"),
            (1, "green_team_player"),
            (2, "blue_team_player"),
            (3, "gray_team_player"),
        ] {
            out.push_str(&format!(
                "{name}={}\n",
                team_kind_value(self.team_players[slot])
            ));
        }

        std::fs::write(path, out)
    }
}

fn push_bool(out: &mut String, name: &str, value: bool) {
    out.push_str(&format!("{name}={}\n", i32::from(value)));
}

/// The debug edit value of a team-control slot (`TEAM_TYPE_*`, `0..=3`).
const fn team_kind_value(kind: TeamKind) -> i32 {
    kind as i32
}

/// `TeamKind` by the value typed into `red_team_player` … `gray_team_player`.
///
/// C++ stores the raw number and the range is `0..=3`, so `Eliminated` (4) is not
/// reachable from the dialog; anything else falls back to `None` like the empty slot.
const fn team_kind_from_value(value: i32) -> TeamKind {
    match value {
        1 => TeamKind::Player,
        2 => TeamKind::Computer,
        3 => TeamKind::Remote,
        _ => TeamKind::None,
    }
}

/// Parses the settings-file value of a team-control slot; `0..=3`, otherwise the fallback.
fn parse_team_kind(text: &str, fallback: TeamKind) -> TeamKind {
    match parse_int(text) {
        Some(value @ 0..=3) => team_kind_from_value(value),
        _ => fallback,
    }
}

fn parse_bool(text: &str, fallback: bool) -> bool {
    match text.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => true,
        "0" | "false" | "off" | "no" => false,
        _ => fallback,
    }
}

fn parse_int(text: &str) -> Option<i32> {
    text.trim().parse::<i32>().ok()
}

/// One dialog row (coordinates are local, inside `PREFSPIC`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Setting key.
    pub key: PrefKey,
    /// Control type.
    pub kind: ControlKind,
    /// Label (with `%s` for read-only labels).
    pub caption: String,
    /// Local `x` of the control (`options_menu_buttons[i].ulx`).
    pub x: i32,
    /// Local `y` of the control (`window_uly`).
    pub y: i32,
    /// Label rectangle (`Text_TextBox`).
    pub label: PixelRect,
    /// Click/control rectangle.
    pub rect: PixelRect,
    /// Slider groove (`PRFSLIT`), if this is a slider.
    pub slider: Option<PixelRect>,
    /// Input field (`PREFEDIT`/`PREFNAME`), if this is an edit.
    pub field: Option<PixelRect>,
}

/// Dialog action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesAction {
    /// Nothing changed.
    None,
    /// The value changed: apply to `App::preferences` immediately (effects/sound).
    Changed,
    /// `Done`/`Enter`: save.
    Done,
    /// `Cancel`/`Esc`: restore the values at the moment of opening.
    Cancel,
    /// `?`: help window.
    Help,
}

/// The debug section is compiled into debug builds only — C++ `#if !defined(NDEBUG)`.
///
/// A release build has no debug controls at all, exactly like the C++ release build,
/// so `settings["debug"]` cannot open a developer switch in a shipped client.
pub const DEBUG_SECTION_BUILT: bool = cfg!(debug_assertions);

/// Whether the `invalid` debug section is part of the dialog.
///
/// Mirrors `OptionsMenu::Init`: the section needs a debug build
/// ([`DEBUG_SECTION_BUILT`]) and `ResourceManager_GetSettings()->GetNumericValue("debug")`.
/// The gate is UI state — it only decides whether the six controls are laid out and
/// installed, it never reaches the simulation or the canonical digest.
pub fn debug_section_enabled(preferences: &Preferences) -> bool {
    DEBUG_SECTION_BUILT && preferences.debug
}

/// Open `Preferences` dialog with an editable copy of the values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferencesDialog {
    /// `SETUPPIC` or `PREFSPIC`.
    variant: PreferencesVariant,
    /// The `invalid` debug section is laid out (`settings["debug"]`, debug build).
    debug: bool,
    /// Editable values.
    pub draft: Preferences,
    /// Snapshot for `Cancel`.
    pub initial: Preferences,
    /// Values of the read-only labels.
    pub info: PreferencesInfo,
    rows: Vec<Row>,
    /// Index of the focused row (keyboard).
    pub focus: Option<usize>,
    /// Index of the row being edited.
    editing: Option<usize>,
    /// Edit buffer (`char text_buffer[64]`).
    edit_buffer: String,
    /// Original value of the edited row.
    edit_initial: String,
}

impl PreferencesDialog {
    /// Creates the dialog with the `OptionsMenu::Init` layout for the variant.
    pub fn new(variant: PreferencesVariant, prefs: Preferences, info: PreferencesInfo) -> Self {
        let debug = debug_section_enabled(&prefs);
        let rows = layout(variant, debug);

        Self {
            variant,
            debug,
            draft: prefs.clone(),
            initial: prefs,
            info,
            rows,
            focus: None,
            editing: None,
            edit_buffer: String::new(),
            edit_initial: String::new(),
        }
    }

    /// The debug section is part of this dialog (debug build with `settings["debug"]`).
    pub const fn has_debug_section(&self) -> bool {
        self.debug
    }

    /// Dialog variant.
    pub const fn variant(&self) -> PreferencesVariant {
        self.variant
    }

    /// Visible rows (in C++ order).
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Row by key.
    pub fn row(&self, key: PrefKey) -> Option<&Row> {
        self.rows.iter().find(|row| row.key == key)
    }

    /// Index of the row being edited (for tests/QA).
    pub fn editing(&self) -> Option<usize> {
        self.editing
    }

    /// Text in the input field (for tests).
    pub fn edit_buffer(&self) -> &str {
        &self.edit_buffer
    }

    /// Read-only label text (with the values substituted).
    pub fn label_text(&self, key: PrefKey) -> Option<String> {
        Some(match key {
            PrefKey::PlayModeLabel => self.info.play_mode_text(),
            PrefKey::OpponentLabel => self.info.opponent_text(),
            PrefKey::VictoryLabel => self.info.victory_text(),
            PrefKey::MusicVolumeLabel => "Volume:".to_string(),
            _ => return None,
        })
    }

    /// Control value as a number (checkbox — 0/1) for `assert prefs-value`.
    pub fn value(&self, key: PrefKey) -> Option<i32> {
        if let Some(value) = self.draft.bool_value(key) {
            return Some(i32::from(value));
        }
        self.draft.int_value(key)
    }

    /// String control value (for tests/logs).
    pub fn string_value(&self, key: PrefKey) -> Option<String> {
        if let Some(value) = self.draft.string_value(key) {
            return Some(value.to_string());
        }
        None
    }

    /// The input field text as `Enter` sees it
    /// (`timer`/`endturn` — a number, `player_name`/`host_address` — a string).
    pub fn edit_text(&self, key: PrefKey) -> Option<String> {
        if !key.is_edit() || !self.rows.iter().any(|row| row.key == key) {
            return None;
        }

        Some(self.current_string(key))
    }

    /// Toggles a checkbox (keyboard/QA test). `false` — the key is not a checkbox.
    pub fn toggle(&mut self, key: PrefKey) -> bool {
        let Some(row) = self.rows.iter().find(|row| row.key == key) else {
            return false;
        };
        if row.kind != ControlKind::Checkbox {
            return false;
        }
        let current = self.draft.bool_value(key).unwrap_or(false);

        self.draft.set_bool(key, !current)
    }

    /// Sets a slider (click/QA). `false` — the key is not a slider.
    pub fn set_slider(&mut self, key: PrefKey, value: i32) -> bool {
        let Some(row) = self.rows.iter().find(|row| row.key == key) else {
            return false;
        };
        if row.kind != ControlKind::Slider {
            return false;
        }

        self.draft.set_int(key, value)
    }

    /// Screen rectangle of the control (for QA clicks/tests).
    pub fn screen_rect(&self, key: PrefKey) -> Option<PixelRect> {
        let row = self.row(key)?;

        Some(PixelRect::new(
            WINDOW_ULX + row.rect.ulx,
            WINDOW_ULY + row.rect.uly,
            row.rect.width,
            row.rect.height,
        ))
    }

    /// Screen `x` of the slider for the value `value` (for QA).
    pub fn slider_screen_x(&self, key: PrefKey, value: i32) -> Option<i32> {
        let row = self.row(key)?;
        let slider = row.slider?;
        let (min, max) = key.range();
        let value = value.clamp(min, max);
        let x = slider.ulx + value * SLIDER_SLIT_WIDTH / max;

        Some(WINDOW_ULX + x)
    }

    /// The slider value that a click at the screen point `screen_x` gives
    /// (`OptionsMenu::UpdateSlider`: `((x − track)·max) / 57`).
    pub fn slider_value_at(&self, key: PrefKey, screen_x: i32) -> Option<i32> {
        let row = self.row(key)?;
        let slider = row.slider?;
        let (min, max) = key.range();
        let value = (screen_x - WINDOW_ULX - slider.ulx) * max / SLIDER_SLIT_WIDTH;

        Some(value.clamp(min, max))
    }

    /// Handles input.
    pub fn input(&mut self, event: InputEvent) -> PreferencesAction {
        match event {
            InputEvent::Key(key) => self.key(key),
            InputEvent::MouseClick(x, y) => self.click(x, y),
            InputEvent::MouseMove(..) | InputEvent::RightClick(..) | InputEvent::MouseRelease(..) => {
                PreferencesAction::None
            }
        }
    }

    fn click(&mut self, x: i32, y: i32) -> PreferencesAction {
        let lx = x - WINDOW_ULX;
        let ly = y - WINDOW_ULY;

        if DONE_BUTTON.contains(lx, ly) {
            self.commit_edit();
            return PreferencesAction::Done;
        }
        if CANCEL_BUTTON.contains(lx, ly) {
            self.cancel_edit();
            return PreferencesAction::Cancel;
        }
        if HELP_BUTTON.contains(lx, ly) {
            self.commit_edit();
            return PreferencesAction::Help;
        }

        let hit = self
            .rows
            .iter()
            .position(|row| row.rect.contains(lx, ly));

        // A click outside the active field commits its value — like
        // `ProcessTextEditKeyPress(GNW_KB_KEY_RETURN)` before processing the control.
        let mut changed = false;

        if let Some(editing) = self.editing {
            let inside = self
                .rows
                .get(editing)
                .and_then(|row| row.field)
                .is_some_and(|field| field.contains(lx, ly));

            if !inside {
                changed |= self.commit_edit();
            }
        }

        let Some(index) = hit else {
            return if changed {
                PreferencesAction::Changed
            } else {
                PreferencesAction::None
            };
        };
        let Some(row) = self.rows.get(index).cloned() else {
            return PreferencesAction::None;
        };

        match row.kind {
            ControlKind::Checkbox => {
                let current = self.draft.bool_value(row.key).unwrap_or(false);
                self.draft.set_bool(row.key, !current);
                self.focus = Some(index);
                PreferencesAction::Changed
            }
            ControlKind::Slider => {
                let slider = row.slider.expect("slider rect");
                let (min, max) = row.key.range();
                let value = (lx - slider.ulx) * max / SLIDER_SLIT_WIDTH;
                self.draft.set_int(row.key, value.clamp(min, max));
                self.focus = Some(index);
                PreferencesAction::Changed
            }
            ControlKind::EditInt | ControlKind::EditStr => {
                self.begin_edit(index);
                self.focus = Some(index);
                if changed {
                    PreferencesAction::Changed
                } else {
                    PreferencesAction::None
                }
            }
            ControlKind::Label | ControlKind::Section => {
                if changed {
                    PreferencesAction::Changed
                } else {
                    PreferencesAction::None
                }
            }
        }
    }

    fn key(&mut self, key: Key) -> PreferencesAction {
        if let Some(index) = self.editing {
            return match key {
                Key::Escape => {
                    // C++: the first Esc while a field is active only leaves
                    // editing (`ProcessTextEditKeyPress(ESC)`).
                    if self.commit_edit() {
                        PreferencesAction::Changed
                    } else {
                        PreferencesAction::None
                    }
                }
                Key::Enter => {
                    if self.commit_edit() {
                        PreferencesAction::Changed
                    } else {
                        PreferencesAction::None
                    }
                }
                Key::Backspace => {
                    self.edit_buffer.pop();
                    PreferencesAction::None
                }
                Key::Char(character) => {
                    let numeric = self
                        .rows
                        .get(index)
                        .is_some_and(|row| row.kind == ControlKind::EditInt);

                    if character.is_ascii_graphic() || character == ' ' {
                        if !numeric || character.is_ascii_digit() {
                            if self.edit_buffer.chars().count() < 63 {
                                self.edit_buffer.push(character);
                            }
                        }
                    }
                    PreferencesAction::None
                }
                _ => PreferencesAction::None,
            };
        }

        match key {
            Key::Escape => PreferencesAction::Cancel,
            Key::Enter => PreferencesAction::Done,
            Key::Char('/') | Key::Char('?') => PreferencesAction::Help,
            Key::Tab | Key::Down => {
                self.step_focus(1);
                PreferencesAction::None
            }
            Key::Up => {
                self.step_focus(-1);
                PreferencesAction::None
            }
            Key::Char(' ') => {
                let Some(index) = self.focus else {
                    return PreferencesAction::None;
                };
                let Some(row) = self.rows.get(index).cloned() else {
                    return PreferencesAction::None;
                };

                if row.kind == ControlKind::Checkbox {
                    let current = self.draft.bool_value(row.key).unwrap_or(false);
                    self.draft.set_bool(row.key, !current);
                    return PreferencesAction::Changed;
                }

                PreferencesAction::None
            }
            Key::Left | Key::Right => {
                let Some(index) = self.focus else {
                    return PreferencesAction::None;
                };
                let Some(row) = self.rows.get(index).cloned() else {
                    return PreferencesAction::None;
                };

                if row.kind != ControlKind::Slider {
                    return PreferencesAction::None;
                }

                let (min, max) = row.key.range();
                let step = if row.key == PrefKey::QuickScroll { 4 } else { 1 };
                let delta = if key == Key::Right { step } else { -step };
                let current = self.draft.int_value(row.key).unwrap_or(min);

                self.draft.set_int(row.key, (current + delta).clamp(min, max));
                PreferencesAction::Changed
            }
            _ => PreferencesAction::None,
        }
    }

    fn step_focus(&mut self, delta: i32) {
        let interactive: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.kind != ControlKind::Section)
            .map(|(index, _)| index)
            .collect();

        if interactive.is_empty() {
            self.focus = None;
            return;
        }

        let current = self
            .focus
            .and_then(|focus| interactive.iter().position(|index| *index == focus))
            .map(|position| position as i32)
            .unwrap_or(if delta > 0 { -1 } else { 0 });
        let position = (current + delta).rem_euclid(interactive.len() as i32) as usize;

        self.focus = Some(interactive[position]);
    }

    fn begin_edit(&mut self, index: usize) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let value = self.current_string(row.key);

        self.editing = Some(index);
        self.edit_buffer = value.clone();
        self.edit_initial = value;
    }

    fn current_string(&self, key: PrefKey) -> String {
        if let Some(value) = self.draft.int_value(key) {
            return value.to_string();
        }
        if let Some(value) = self.draft.string_value(key) {
            return value.to_string();
        }

        String::new()
    }

    /// Commits the input field value. `true` — the value changed.
    fn commit_edit(&mut self) -> bool {
        let Some(index) = self.editing.take() else {
            return false;
        };
        let Some(row) = self.rows.get(index).cloned() else {
            return false;
        };
        let text = self.edit_buffer.trim().to_string();
        let mut changed = false;

        match row.kind {
            ControlKind::EditInt => {
                if let Ok(value) = text.parse::<i32>() {
                    let (min, max) = row.key.range();
                    let value = value.clamp(min, max);

                    changed = self.draft.int_value(row.key) != Some(value);
                    self.draft.set_int(row.key, value);
                }
            }
            ControlKind::EditStr => {
                changed = self.draft.string_value(row.key) != Some(text.as_str());
                self.draft.set_string(row.key, &text);
            }
            _ => {}
        }

        changed
    }

    fn cancel_edit(&mut self) {
        if let Some(index) = self.editing.take() {
            let value = self.edit_initial.clone();
            if let Some(row) = self.rows.get(index).cloned() {
                if row.kind == ControlKind::EditStr {
                    self.draft.set_string(row.key, &value);
                } else if let Ok(value) = value.parse::<i32>() {
                    self.draft.set_int(row.key, value);
                }
            }
        }
    }

    /// Draws the dialog over the 640×480 frame.
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets) -> u32 {
        if frame.width() != 640 || frame.height() != 480 {
            return 0;
        }

        let prefs_assets = assets.preferences.as_ref();
        let palette = match self.variant {
            PreferencesVariant::Setup => assets.menu.as_ref().map(|menu| menu.palette),
            PreferencesVariant::InGame => UiPalette::from_assets(assets),
        }
        .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));

        let Some(palette) = palette else {
            return 0;
        };

        let screen = PixelRect::new(0, 0, 640, 480);
        let mut painted = 0;

        match prefs_assets.and_then(|assets| assets.background.as_ref()) {
            Some(background) => {
                painted += draw_big_image_at(
                    frame,
                    background,
                    &palette,
                    (WINDOW_ULX, WINDOW_ULY),
                    screen,
                );
            }
            None => {
                frame.fill_rect(
                    PixelRect::new(WINDOW_ULX, WINDOW_ULY, WINDOW_WIDTH, WINDOW_HEIGHT),
                    palette.rgba(0x0E),
                );
                painted += 1;
            }
        }

        // Title (`COLOR_GREEN`, FONT5, centered 184×17).
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(
                WINDOW_ULX + TITLE.ulx,
                WINDOW_ULY + TITLE.uly,
                TITLE.width,
                TITLE.height,
            ),
            LABEL_FONT.font(),
            self.variant.title(),
            palette.rgba(GREEN),
        )
        .max(0) as u32;

        for (index, row) in self.rows.iter().enumerate() {
            painted += self.draw_row(frame, &palette, prefs_assets, row, index);
        }

        painted += draw_dialog_button(
            frame,
            &palette,
            prefs_assets.and_then(|assets| assets.done.as_ref()),
            DONE_BUTTON,
            "Done",
        );
        painted += draw_dialog_button(
            frame,
            &palette,
            prefs_assets.and_then(|assets| assets.cancel.as_ref()),
            CANCEL_BUTTON,
            "Cancel",
        );
        painted += draw_dialog_button(
            frame,
            &palette,
            prefs_assets.and_then(|assets| assets.help.as_ref()),
            HELP_BUTTON,
            "",
        );

        painted
    }

    fn draw_row(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        assets: Option<&PreferencesAssets>,
        row: &Row,
        _index: usize,
    ) -> u32 {
        let label_rect = PixelRect::new(
            WINDOW_ULX + row.label.ulx,
            WINDOW_ULY + row.label.uly,
            row.label.width,
            row.label.height,
        );
        let control_rect = PixelRect::new(
            WINDOW_ULX + row.rect.ulx,
            WINDOW_ULY + row.rect.uly,
            row.rect.width,
            row.rect.height,
        );
        let mut painted = 0;

        match row.kind {
            ControlKind::Section => {}
            ControlKind::Label => {
                let text = self.label_text(row.key).unwrap_or_else(|| row.caption.clone());
                let color = if row.key == PrefKey::MusicVolumeLabel {
                    GOLD
                } else {
                    BRIGHT_YELLOW
                };

                painted += draw_label_box(frame, palette, label_rect, &text, color, false);
            }
            ControlKind::Checkbox => {
                let checked = self.draft.bool_value(row.key).unwrap_or(false);
                let image = if checked {
                    assets.and_then(|assets| assets.checked.as_ref())
                } else {
                    assets.and_then(|assets| assets.unchecked.as_ref())
                };

                if let Some(image) = image {
                    painted += blit_simple(
                        frame,
                        palette,
                        image,
                        control_rect.ulx,
                        control_rect.uly,
                        true,
                    );
                }

                painted += draw_label_box(frame, palette, label_rect, &row.caption, GOLD, false);
            }
            ControlKind::Slider => {
                painted += draw_label_box(frame, palette, label_rect, &row.caption, GOLD, false);

                let value = self.draft.int_value(row.key).unwrap_or(0);
                let (min, max) = row.key.range();
                let value = value.clamp(min, max);

                if let Some(slider) = row.slider {
                    let screen = PixelRect::new(
                        WINDOW_ULX + slider.ulx,
                        WINDOW_ULY + slider.uly,
                        slider.width,
                        slider.height,
                    );

                    if let Some(slit) = assets.and_then(|assets| assets.slit.as_ref()) {
                        painted += blit_simple(
                            frame,
                            palette,
                            slit,
                            screen.ulx,
                            screen.uly,
                            true,
                        );
                    } else {
                        frame.fill_rect(screen, palette.rgba(0x18));
                        painted += 1;
                    }

                    if let Some(slide) = assets.and_then(|assets| assets.slide.as_ref()) {
                        let x = screen.ulx + value * SLIDER_SLIT_WIDTH / max
                            - i32::from(slide.width) / 2;
                        let y = WINDOW_ULY + row.y + (SLIDER_ROW_HEIGHT - i32::from(slide.height)) / 2;

                        painted += blit_simple(frame, palette, slide, x, y, true);
                    }
                }
            }
            ControlKind::EditInt | ControlKind::EditStr => {
                painted += draw_label_box(frame, palette, label_rect, &row.caption, GOLD, false);

                let field = row.field.expect("edit field");
                let screen = PixelRect::new(
                    WINDOW_ULX + field.ulx,
                    WINDOW_ULY + field.uly,
                    field.width,
                    field.height,
                );
                let image = if row.key == PrefKey::PlayerName {
                    assets.and_then(|assets| assets.name.as_ref())
                } else {
                    assets.and_then(|assets| assets.edit.as_ref())
                };

                if let Some(image) = image {
                    painted += blit_simple(
                        frame,
                        palette,
                        image,
                        screen.ulx,
                        screen.uly,
                        true,
                    );
                } else {
                    frame.fill_rect(screen, palette.rgba(0x18));
                    painted += 1;
                }

                let editing = self
                    .editing
                    .and_then(|index| self.rows.get(index))
                    .is_some_and(|editing| editing.key == row.key);
                let text = if editing {
                    self.edit_buffer.clone()
                } else {
                    self.current_string(row.key)
                };
                let text = fit_text(&text, screen.width - 4, LABEL_FONT.font());

                painted += draw_text_gnw_centered(
                    frame,
                    screen,
                    LABEL_FONT.font(),
                    &text,
                    palette.rgba(EDIT_TEXT),
                )
                .max(0) as u32;

                if editing {
                    let width = LABEL_FONT.font().text_width(&text);
                    let x = screen.ulx + (screen.width - (width + 1)).max(0) / 2 + width + 1;
                    let y = screen.uly + 5;
                    frame.fill_rect(PixelRect::new(x, y, 1, 9), palette.rgba(EDIT_TEXT));
                    painted += 1;
                }
            }
        }

        painted
    }
}

/// `OptionsMenu::Init` layout for the variant.
///
/// `debug` — the `invalid` debug section is enabled (debug build with
/// `settings["debug"]`): the in-game list starts 20 px higher (`window_uly = -20`),
/// the three read-only `options` labels are skipped and the `invalid` section is not
/// a stop, which is exactly how the six extra rows fit above the dialog buttons.
fn layout(variant: PreferencesVariant, debug: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut y = if debug && variant == PreferencesVariant::InGame {
        -20
    } else {
        20
    };
    let font_height = LABEL_FONT.height();

    for def in &DEFS {
        if !is_visible(variant, def.key) {
            continue;
        }

        if def.kind == ControlKind::Section {
            // `SETUPPIC` ends the list at the `preferences` section, `PREFSPIC` — at
            // `invalid` unless the debug section is enabled.
            let stop = match def.key {
                PrefKey::SectionPreferences => variant == PreferencesVariant::Setup,
                PrefKey::SectionInvalid => !debug,
                _ => false,
            };

            if stop {
                break;
            }

            y += 20;
            continue;
        }

        // With the debug section the three read-only `options` labels are dropped
        // (`OptionsMenu::Init`, `#if !defined(NDEBUG)`), which is where the room for
        // the debug rows comes from.
        if debug
            && matches!(
                def.key,
                PrefKey::PlayModeLabel | PrefKey::OpponentLabel | PrefKey::VictoryLabel
            )
        {
            continue;
        }

        if def.ulx == 25 {
            y += 20;
        }

        let row = build_row(def, y);

        if def.kind == ControlKind::Label {
            // `window_uly -= 20 - (Text_GetHeight() + 3)`.
            y -= 20 - (font_height + 3);
        }

        rows.push(row);
    }

    rows
}

fn build_row(def: &Def, y: i32) -> Row {
    let x = def.ulx;
    let caption = def.caption.to_string();
    let width = LABEL_FONT.font().text_width(def.caption);

    let label = match def.kind {
        ControlKind::Checkbox => PixelRect::new(x + 25, y - 2, 155, 24),
        ControlKind::Slider => PixelRect::new(x, y, 185, 20),
        ControlKind::EditInt | ControlKind::EditStr => PixelRect::new(x, y, 185, 20),
        ControlKind::Label => PixelRect::new(x, y, WINDOW_WIDTH - x, 20),
        ControlKind::Section => PixelRect::new(0, 0, 0, 0),
    };

    let (rect, slider, field) = match def.kind {
        ControlKind::Checkbox => (PixelRect::new(x, y, 18, 17), None, None),
        ControlKind::Slider => {
            let track_x = x + width + 10;
            let slider = PixelRect::new(track_x, y + 8, SLIDER_SLIT_WIDTH, 3);

            (PixelRect::new(x + width, y, 77, 20), Some(slider), None)
        }
        ControlKind::EditInt => {
            let field = PixelRect::new(x + width + 10, y + (20 - EDIT_H) / 2, EDIT_W, EDIT_H);

            (field, None, Some(field))
        }
        ControlKind::EditStr => {
            let (field_w, field_h) = if def.key == PrefKey::PlayerName {
                (NAME_W, NAME_H)
            } else {
                (EDIT_W, EDIT_H)
            };
            let field = PixelRect::new(x + width + 10, y + (20 - field_h) / 2, field_w, field_h);

            (field, None, Some(field))
        }
        ControlKind::Label => (label, None, None),
        ControlKind::Section => (PixelRect::new(0, 0, 0, 0), None, None),
    };

    Row {
        key: def.key,
        kind: def.kind,
        caption,
        x,
        y,
        label,
        rect,
        slider,
        field,
    }
}

/// `IsControlVisible`: graphics and network — only in `SETUPPIC`.
fn is_visible(variant: PreferencesVariant, key: PrefKey) -> bool {
    if matches!(
        key,
        PrefKey::EnhancedGraphics | PrefKey::HostAddress | PrefKey::HostPort
    ) {
        return variant == PreferencesVariant::Setup;
    }

    true
}

/// `Text_TextBox` label with `FontColor`: word wrap, vertical centering.
fn draw_label_box(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    rect: PixelRect,
    text: &str,
    color: (u8, u8, u8),
    centered: bool,
) -> u32 {
    let font = LABEL_FONT.font();
    let lines = wrap_gnw_text(text, rect.width);
    let max_lines = ((rect.height / LINE_HEIGHT).max(1)) as usize;
    let lines = &lines[..lines.len().min(max_lines)];
    let block = LINE_HEIGHT * lines.len() as i32;
    let mut y = rect.uly + ((rect.height + 1) - block).max(0) / 2;

    for line in lines {
        let x = if centered {
            rect.ulx + (rect.width - (font.text_width(line) + 1)).max(0) / 2
        } else {
            rect.ulx
        };

        draw_text_gnw_shadowed(
            frame,
            x,
            y,
            font,
            line,
            palette.rgba(color.0),
            palette.rgba(color.1),
            palette.rgba(color.2),
        );
        y += LINE_HEIGHT;
    }

    1
}

/// Dialog button: `up` picture + `FONT5` label (`Button::SetCaption`).
fn draw_dialog_button(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    pair: Option<&MenuButtonPair>,
    rect: PixelRect,
    label: &str,
) -> u32 {
    let screen = PixelRect::new(
        WINDOW_ULX + rect.ulx,
        WINDOW_ULY + rect.uly,
        rect.width,
        rect.height,
    );
    let mut painted = 0;

    if let Some(pair) = pair {
        painted += blit_simple(
            frame,
            palette,
            pair.image(false),
            screen.ulx,
            screen.uly,
            true,
        );
    } else {
        frame.fill_rect(screen, palette.rgba(0x12));
        painted += 1;
    }

    if !label.is_empty() {
        let font = LABEL_FONT.font();
        let width = font.text_width(label) + 1;
        let x = screen.ulx + (screen.width - width).max(0) / 2;
        let y = screen.uly + (screen.height - font.height()) / 2;

        painted += draw_text_gnw_shadowed(
            frame,
            x,
            y,
            font,
            label,
            palette.rgba(GOLD.0),
            palette.rgba(GOLD.1),
            palette.rgba(GOLD.2),
        )
        .max(0) as u32;
    }

    painted
}

/// Clipping of a long value by the field width.
fn fit_text(text: &str, max_width: i32, font: &max_render::text::GnwFont) -> String {
    if font.text_width(text) <= max_width {
        return text.to_string();
    }

    let mut out = text.to_string();

    while !out.is_empty() && font.text_width(&out) > max_width {
        out.pop();
    }

    out
}

/// Dialog resources from `MAX.RES`.
#[derive(Debug, Clone)]
pub struct PreferencesAssets {
    /// `PREFSPIC` 400×422.
    pub background: Option<BigImage>,
    /// `UNCHKED` 18×17.
    pub unchecked: Option<SimpleImage>,
    /// `CHECKED` 18×17.
    pub checked: Option<SimpleImage>,
    /// `PRFSLIT` 57×3.
    pub slit: Option<SimpleImage>,
    /// `PRFSLIDE` 14×17.
    pub slide: Option<SimpleImage>,
    /// `PREFEDIT` 60×19.
    pub edit: Option<SimpleImage>,
    /// `PREFNAME` 187×17.
    pub name: Option<SimpleImage>,
    /// `PRFDNE_U/D` 62×23.
    pub done: Option<MenuButtonPair>,
    /// `PRFCAN_U/D` 62×23.
    pub cancel: Option<MenuButtonPair>,
    /// `PRFHLP_U/D` 25×23.
    pub help: Option<MenuButtonPair>,
}

impl PreferencesAssets {
    /// Loads resources; a missing part is not an error (frames are drawn).
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            let up = SimpleImage::decode(archive.read(up).ok()?).ok()?;
            let down = SimpleImage::decode(archive.read(down).ok()?).ok()?;
            Some(MenuButtonPair::new(up, down))
        };
        let simple = |name: &str| -> Option<SimpleImage> {
            SimpleImage::decode(archive.read(name).ok()?).ok()
        };

        Self {
            background: archive
                .read("PREFSPIC")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            unchecked: simple("UNCHKED"),
            checked: simple("CHECKED"),
            slit: simple("PRFSLIT"),
            slide: simple("PRFSLIDE"),
            edit: simple("PREFEDIT"),
            name: simple("PREFNAME"),
            done: pair("PRFDNE_U", "PRFDNE_D"),
            cancel: pair("PRFCAN_U", "PRFCAN_D"),
            help: pair("PRFHLP_U", "PRFHLP_D"),
        }
    }

    /// All mandatory pictures were loaded.
    pub fn is_ready(&self) -> bool {
        self.background.is_some()
            && self.unchecked.is_some()
            && self.checked.is_some()
            && self.slit.is_some()
            && self.slide.is_some()
            && self.done.is_some()
            && self.cancel.is_some()
    }
}

// ---------------------------------------------------------------------------
// Integration with the application
// ---------------------------------------------------------------------------

// Preferences are carried by every application type: the session input path is
// generic over the dispatcher (stage C §4.2), and the network application must
// accept a settings change from the `Preferences` dialog too.
impl<D: Dispatcher> crate::App<D> {
    /// Replaces the settings (effects/sound are applied by rendering/the audio layer).
    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.preferences = preferences;
        self.sync_preferences_to_game();
    }

    /// Applies the settings that live in a match to the current session.
    ///
    /// * `timer`/`endturn` — `GameManager_UpdateGui` reads them from `settings` at
    ///   the start of each turn (`game_manager.cpp:2416-2471`), so a new
    ///   match gets them at start, and a change in game takes effect from the next
    ///   turn;
    /// * `enemy_halt` — `Access_UpdateMapStatus` reads the setting on every
    ///   unit step (`access.cpp:830`), so the state field is updated
    ///   immediately.
    ///
    /// B2 audit — `set_enemy_halt` is a **preference install**, not a UI
    /// simulation mutation: the flag is not part of `GameState::snapshot_bytes`
    /// (the snapshot carries `network_game`/`settings_effects`/`disable_fire`/
    /// `real_time` only), it is a local rules switch read by `Access_UpdateMapStatus`
    /// on every step, and C++ has no packet for it. `Preferences` is outside the
    /// snapshot by design (`sdl-ui-integration.md` §4, "debug-Preferences — чисто
    /// UI-настройки, вне снапшота").
    ///
    /// The debug section (`debug`, `disable_fire`, `real_time`, team slots) is **not**
    /// applied here: C++ re-reads it only after the dialog closes, so it goes through
    /// [`Self::install_debug_preferences`] from the dialog path.
    fn sync_preferences_to_game(&mut self) {
        let enemy_halt = self.preferences.enemy_halt;
        self.universe.set_enemy_halt(enemy_halt);

        if let Some(local) = self.local.as_mut() {
            local.setup.timer = self.preferences.turn_timer.max(0) as u32;
            local.setup.endturn = self.preferences.endturn.max(0) as u32;
        }
    }

    /// Installs the debug section (`invalid`) into the live match.
    ///
    /// Oracle: `GameManager_MenuClickPreferencesButton` (`game_manager.cpp:3925-3943`)
    /// runs right after `menu_preferences_window` returns and copies the settings the
    /// window can change into the running game — `GameManager_RealTime` (`real_time`)
    /// and `UnitsManager_TeamInfo[team].team_type` for the four playing teams. The
    /// port does the same here, and `disable_fire` too (C++ reads it live at every
    /// use; the dialog is modal, so installing it at close is indistinguishable).
    ///
    /// This is the **install** channel, not a UI mutation (rule C3, SDL/UI Stage B2
    /// item 7 "install/preferences"): the switches travel through the narrow facade
    /// API (`set_disable_fire`, `set_real_time`, `set_team_type`) exactly like
    /// `enemy_halt` does, never as a `GameState` write from the dialog.
    ///
    /// `disable_fire`, `real_time` and the team types are canonical digest fields
    /// ([`max_core::game::GameState::snapshot_bytes`]), so changing one moves
    /// `snapshot_bytes` by design: C++ carries `real_time`/`disable_fire` in the match
    /// setup packet (`remote.cpp:2469-2471`) and the team slots are part of
    /// `UnitsManager_TeamInfo`. With the gate off nothing is installed, so the digest
    /// does not move. In a network match a switch therefore desyncs the peers — a
    /// deliberate property of the developer tool, and C++ behaves the same way (no
    /// packet synchronizes the team slots after the dialog).
    fn install_debug_preferences(&mut self) {
        let (disable_fire, real_time, team_players) = (
            self.preferences.disable_fire,
            self.preferences.real_time,
            self.preferences.team_players,
        );

        self.universe.set_disable_fire(disable_fire);
        self.universe.set_real_time(real_time);

        for (slot, kind) in team_players.iter().enumerate() {
            self.universe.set_team_type(TeamId(slot as u8), *kind);
        }
    }

    /// Settings file: `<save_dir>/preferences.ini` (C++ `settings.json` in
    /// `FilePathGamePref`).
    pub fn preferences_path(&self) -> PathBuf {
        self.save_dir().join("preferences.ini")
    }

    /// Saves the settings to disk; an error goes to the HUD log (no crash).
    pub fn persist_preferences(&mut self) {
        let path = self.preferences_path();

        if let Err(error) = self.preferences.save_file(&path) {
            self.hud
                .push_log(format!("Preferences: не удалось сохранить {}: {error}", path.display()));
        }
    }
}

impl<D: Dispatcher> crate::App<D> {
    /// Current user settings.
    pub fn preferences(&self) -> &Preferences {
        &self.preferences
    }

    /// Debug observation mode: the fog of war is not drawn (`--no-fow` /
    /// `no_fow=1`). Presentation only — the simulation state and the digest do
    /// not see it.
    pub fn no_fow(&self) -> bool {
        self.preferences.no_fow
    }

    /// Switches the debug observation mode (see [`Self::no_fow`]).
    pub fn set_no_fow(&mut self, enabled: bool) {
        self.preferences.no_fow = enabled;
    }

    /// Whether visual effects are enabled (`settings["effects"]`).
    pub fn effects_enabled(&self) -> bool {
        self.preferences.effects
    }

    /// Whether SFX sound is enabled (`!disable_fx`).
    pub fn sound_enabled(&self) -> bool {
        self.preferences.sound_enabled()
    }

    /// Loads `preferences.ini`, if present (interactive launch only).
    pub fn load_preferences_from_disk(&mut self) -> bool {
        let path = self.preferences_path();
        let Some(preferences) = Preferences::load_file(&path) else {
            return false;
        };

        self.preferences = preferences;
        true
    }

    /// Open `Preferences` dialog; `None` — closed.
    pub fn preferences_dialog(&self) -> Option<&PreferencesDialog> {
        self.preferences_dialog.as_ref()
    }

    /// Open dialog for input/QA.
    pub fn preferences_dialog_mut(&mut self) -> Option<&mut PreferencesDialog> {
        self.preferences_dialog.as_mut()
    }

    /// Values of the read-only labels (`Play Mode`, `Computer Player(s)`, …).
    pub fn preferences_info(&self) -> PreferencesInfo {
        match self.local() {
            Some(local) => PreferencesInfo {
                play_mode: local.setup.play_mode,
                opponent: Opponent::try_from(local.setup.opponent).unwrap_or(Opponent::Average),
                victory_limit: local.setup.victory_limit,
                victory_type: local.setup.victory_type,
            },
            None => PreferencesInfo::default(),
        }
    }

    /// Opens `Preferences` (panel button, `PREFSPIC`).
    ///
    /// Returns `false` if the dialog is already open or another modal
    /// screen is open (in C++ the main menu is disabled at that time
    /// `GameManager_DisableMainMenu`).
    pub fn open_preferences(&mut self) -> bool {
        if self.preferences_dialog.is_some()
            || self.help.is_some()
            || self.build_menu.is_some()
            || self.save_load.is_some()
            || self.upgrade_menu.is_some()
            || self.research_menu.is_some()
            || self.unit_menu.is_some()
            || self.alloc_menu.is_some()
        {
            return false;
        }

        let info = self.preferences_info();
        // C++ `PREFSPIC`: the `Player Name:` field is bound to
        // `menu_team_name_setting[team]` — the name of the team the dialog
        // opener plays (`optionsmenu.cpp:172-181`), not to the global
        // `player_name` from the main menu.
        let mut draft = self.preferences.clone();

        if let Some(local) = self.local.as_ref() {
            draft.player_name = local.display_name(self.state_ref().player_team());
        }

        // The debug section: C++ reads these six values from `settings`, which the
        // `ChoosePlayerMenu` and the previous dialog wrote, so the window always shows
        // what is installed. The port keeps the authoritative copy in the live match
        // (`GameManager_RealTime`, `UnitsManager_TeamInfo`), therefore the draft starts
        // from the installed values. Reading the state is allowed for the UI; writing
        // happens only when the dialog closes, through the narrow install API
        // (`GameManager_MenuClickPreferencesButton`, `game_manager.cpp:3925-3943`).
        let debug_installed = debug_section_enabled(&draft);

        if debug_installed {
            let state = self.state_ref();
            draft.disable_fire = state.disable_fire();
            draft.real_time = state.real_time();

            for (slot, kind) in draft.team_players.iter_mut().enumerate() {
                if let Some(info) = state.team(TeamId(slot as u8)) {
                    *kind = info.team_type;
                }
            }
        }

        let mut dialog = PreferencesDialog::new(PreferencesVariant::InGame, draft, info);
        // `Cancel` restores the application values at the moment of opening: the name
        // field shows the team name, but the global `player_name` changes
        // only on `Done` (like `SetStringValue` in `OptionsMenu::Run`). The debug
        // section restores the values that are installed right now, so `Cancel`
        // leaves the match untouched.
        dialog.initial = self.preferences.clone();

        if dialog.has_debug_section() {
            dialog.initial.disable_fire = dialog.draft.disable_fire;
            dialog.initial.real_time = dialog.draft.real_time;
            dialog.initial.team_players = dialog.draft.team_players;
        }

        self.preferences_dialog = Some(dialog);
        true
    }

    /// Closes the dialog (without saving).
    pub fn close_preferences(&mut self) {
        self.preferences_dialog = None;
    }

    /// Input in the open dialog; `None` — the dialog is closed.
    ///
    /// `Changed` applies the draft immediately (like `SetNumericValue`/`SetVolume` in
    /// C++), `Done` saves to the file, `Cancel` restores the opening snapshot.
    pub fn preferences_input(&mut self, event: InputEvent) -> Option<PreferencesAction> {
        let mut dialog = self.preferences_dialog.take()?;
        let action = dialog.input(event);

        match action {
            PreferencesAction::Changed => {
                // C++ `OptionsMenu`: checkboxes and sliders write `settings` immediately
                // (`SetVolume`/`SetNumericValue`), while EDIT_STR/EDIT_INT — only
                // on `Done` (`optionsmenu.cpp:533-560`). Therefore the draft is
                // applied entirely except for the input fields, which keep the value
                // the dialog opened with (`dialog.initial`; for the debug team slots
                // that is the installed one, see `open_preferences`).
                let mut applied = dialog.draft.clone();
                applied.player_name = dialog.initial.player_name.clone();
                applied.host_address = dialog.initial.host_address.clone();
                applied.host_port = dialog.initial.host_port;
                applied.turn_timer = dialog.initial.turn_timer;
                applied.endturn = dialog.initial.endturn;
                applied.team_players = dialog.initial.team_players;

                self.preferences = applied;
                self.sync_preferences_to_game();
                self.preferences_dialog = Some(dialog);
            }
            PreferencesAction::None | PreferencesAction::Help => {
                self.preferences_dialog = Some(dialog);
            }
            PreferencesAction::Done => {
                self.preferences = dialog.draft.clone();
                // C++ `OptionsMenu` `PREFSPIC` on `Done` writes the entered name
                // both to `player_name` and to `menu_team_name_setting[team]`
                // (`optionsmenu.cpp:548-560`): the team name changes immediately.
                let player_team = self.state_ref().player_team();
                if let Some(local) = self.local.as_mut() {
                    local.rename_team(player_team, dialog.draft.player_name.clone());
                }
                self.sync_preferences_to_game();

                // `GameManager_MenuClickPreferencesButton` (`game_manager.cpp:3925`)
                // re-reads the settings once `OptionsMenu::Run` returns; the debug
                // switches are installed there, through the narrow facade API.
                if dialog.has_debug_section() {
                    self.install_debug_preferences();
                }

                self.persist_preferences();
                self.hud.push_log("Preferences saved");
            }
            PreferencesAction::Cancel => {
                self.preferences = dialog.initial.clone();
                self.sync_preferences_to_game();

                // The restored snapshot holds the values the dialog opened with, so this
                // install puts back exactly what was running before the dialog.
                if dialog.has_debug_section() {
                    self.install_debug_preferences();
                }
            }
        }

        Some(action)
    }
}

/// `PathBuf` for `Cargo.toml`/tests: the settings file path next to the saves.
pub fn preferences_path_for(save_dir: &Path) -> PathBuf {
    save_dir.join("preferences.ini")
}

#[cfg(test)]
#[path = "preferences_tests.rs"]
mod tests;
