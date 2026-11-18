//! Диалог `Preferences`/`Setup` — порт C++ `OptionsMenu` (P8).
//!
//! # Оракул
//!
//! * `menu.cpp:2325` `menu_preferences_window(team)` → `OptionsMenu(team,
//!   PREFSPIC).Run()` (кнопка `Preferences` игровой панели,
//!   `GameManager_MenuClickPreferencesButton`, `game_manager.cpp:3918`);
//! * `menu.cpp:2331` `menu_setup_window()` → `OptionsMenu(PLAYER_TEAM_RED,
//!   SETUPPIC).Run()` (пункт `Setup` главного меню);
//! * `optionsmenu.hpp/.cpp` — таблица опций, раскладка и поведение;
//! * `settings.cpp` — значения по умолчанию и секции `SETUP`/`PREFERENCES`/
//!   `OPTIONS`; `IniPreferences` (7 × `signed int`) — `save.md`.
//!
//! # Окно
//!
//! Обе версии рисуются **одной** панелью `PREFSPIC` 400×422 (`Window(id)` в C++
//! центрирует её: `ulx = (640−400)/2 = 120`, `uly = (480−422)/2 = 29`).
//! Заголовок — `GNW_TEXT_FONT_5` в (108,12) 184×17 зелёным (`COLOR_GREEN`), текст
//! `Setup` (`7013`) или `Preferences` (`f70f`). Кнопки в нижней строке
//! (`PRFDNE_U/D` 62×23 в (215,383), `PRFCAN_U/D` 62×23 в (125,383),
//! `PRFHLP_U/D` 25×23 в (188,383)); подписи `Done`/`Cancel` — `FONT5`,
//! `Fonts_GoldColor (165,175,199)`, как `Button::SetCaption` (в конструкторе
//! `Text_SetFont(GNW_TEXT_FONT_5)` стоит до создания кнопок).
//!
//! # Раскладка (дословно `OptionsMenu::Init`)
//!
//! `window_uly` стартует с 20; каждая секция даёт `+20`, каждая опция с
//! `ulx == 25` — `+20`; `LABEL` после отрисовки отнимает
//! `20 − (Text_GetHeight() + 3) = 7` (`FONT5` — 10 px). Скрытые контролы
//! (`IsControlVisible`) не занимают строку. Для `SETUPPIC` `Init` выходит на
//! секции `preferences`, поэтому главное меню показывает только `SETUP`-часть
//! (звук/видео/автосейв/сеть/имя); для `PREFSPIC` — все секции до `invalid`
//! (`debug`-контролы не портированы).
//!
//! | Секция | Контролы |
//! |---|---|
//! | `setup` | `Volume:` (label), `Enhanced graphics` (только Setup), громкости Music/FX/Voice/Video + `Disable…`, `Auto-Save Enabled`, `Host Address:`/`Host Port:` (только Setup), `Player Name:` |
//! | `preferences` | `Animate Effects`, `Click to Scroll`, `Double Unit Steps`, `Track Selected Unit`, `Halt movement when enemy is detected`, `Auto-Select Next Unit`, `Scroll Speed` |
//! | `options` | `Turn Time:`/`End Turn Time:` (edit int), `Play Mode: %s`, `Computer Player(s): %s`, `Game ends at %i %s.` (read-only) |
//!
//! Слайдер: подпись `Text_TextBox(ulx, uly, 185, 20)`, жёлоб `PRFSLIT` 57×3 в
//! `x = ulx + Text_GetWidth(caption) + 10`, `y = uly + 8`; ползунок `PRFSLIDE`
//! 14×17 в `x = track_x + value·57/range_max − 7`, `y = uly + 1`; клик по
//! области `x + width(caption), uly, 77×20` задаёт
//! `value = (x − track_x)·range_max / 57` (`OptionsMenu::UpdateSlider`).
//! Чекбокс — `UNCHKED`/`CHECKED` 18×17 в `(ulx, uly)`, подпись в
//! `(ulx+25, uly−2) 155×24`. Поле — `PREFEDIT` 60×19 (`PREFNAME` 187×17 для
//! имени) в `(ulx + width(caption) + 10, uly + (20−h)/2)`, значение по центру
//! цветом `0xA2`.
//!
//! # Поведение
//!
//! * Мышь: клик по чекбоксу переключает значение, по слайдеру — ставит его по
//!   позиции, по полю — начинает ввод; `Done`/`Cancel`/`Help`.
//! * Клавиатура: `Enter` — Done, `Esc` — Cancel, `/`/`?` — Help (как
//!   `GNW_KB_KEY_SHIFT_DIVIDE`), `Tab`/`Up`/`Down` — фокус, `Space` —
//!   переключить чекбокс, `Left`/`Right` — слайдер. В C++ клавиатурной
//!   навигации нет; она добавлена как доступность и не меняет вид.
//!   У поля ввода `Enter` фиксирует значение, `Esc` — тоже (как
//!   `ProcessTextEditKeyPress`: первое нажатие Esc при активном поле только
//!   выходит из редактирования), `Backspace`/печатные символы — правка.
//! * `Done` пишет значения в `App::preferences`, сохраняет их в
//!   `<save_dir>/preferences.ini` и в `SaveMeta::ini_preferences` (V70);
//!   `Cancel` возвращает снимок значений на момент открытия (как
//!   `last_rest_state` в C++). Изменения эффектов/звука применяются сразу —
//!   как `SetVolume`/`Halt*Playback` в C++.
//!
//! # Что реально действует
//!
//! * `Animate Effects` (`effects`) — выключает покадровые анимации:
//!   `App::update_unit_animations` не листает кадры строителя, `ANIMATED`-зданий
//!   (`RADAR`) и работающих заводов, `render::unit_frame_index_with_animation`
//!   получает `effects = false` (кадр статичен), как `settings["effects"]` в
//!   `units_manager.cpp:2857/3030` и `unitinfo.cpp:5057`;
//! * `FX Disabled` (`disable_fx`) — **звук (SFX) вкл/выкл**: `AudioPlayer::
//!   set_enabled(!disable_fx)`; `FX` — громкость `AudioPlayer::set_volume`
//!   (`fx_sound_level/100`), как `AUDIO_TYPE_SFX2` в C++;
//! * `Auto-Save Enabled` (`auto_save`) — `App::autosave` (слот 10).
//!
//! Остальные опции сохраняются и попадают в `IniPreferences`/`preferences.ini`,
//! но в текущем клиенте не действуют (помечено в `docs/analysis/visual-1to1-status.md`):
//! `disable_music`/`disable_voice`/`disable_movie` и уровни Music/Voice/Video
//! (музыки/голоса/видео в звуковом слое нет), `click_scroll`, `quick_scroll`,
//! `fast_movement`, `follow_unit`, `auto_select`, `enemy_halt` (управление
//! камерой/выделением ещё не портировано), `enhanced_graphics`, `host_address`,
//! `host_port` (сеть задаётся CLI), `Turn Time`/`End Turn Time` (таймеры партии
//! берутся с экрана настройки), `Player Name` (в игре C++ правит имя команды —
//! у нас общее имя игрока для следующей партии).

use std::path::{Path, PathBuf};

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::{Opponent, PlayMode, VictoryType};
use max_core::save::IniPreferences;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, draw_text_gnw_shadowed, GnwFontId};

use crate::assets::Assets;
use crate::menu::{blit_simple, draw_big_image_at, wrap_gnw_text, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// Ширина панели `PREFSPIC`.
pub const WINDOW_WIDTH: i32 = 400;
/// Высота панели `PREFSPIC`.
pub const WINDOW_HEIGHT: i32 = 422;
/// Позиция окна в кадре 640×480 (`Window(id)` центрирует ресурс).
pub const WINDOW_ULX: i32 = (640 - WINDOW_WIDTH) / 2;
/// Позиция окна в кадре 640×480.
pub const WINDOW_ULY: i32 = (480 - WINDOW_HEIGHT) / 2;

/// Заголовок: `Text_TextBox(..., 108, 12, 184, 17, COLOR_GREEN, true, true)`.
pub const TITLE: PixelRect = PixelRect::new(108, 12, 184, 17);
/// Кнопка `Done` (`PRFDNE_U/D`, 62×23).
pub const DONE_BUTTON: PixelRect = PixelRect::new(215, 383, 62, 23);
/// Кнопка `Cancel` (`PRFCAN_U/D`, 62×23).
pub const CANCEL_BUTTON: PixelRect = PixelRect::new(125, 383, 62, 23);
/// Кнопка `Help` (`PRFHLP_U/D`, 25×23).
pub const HELP_BUTTON: PixelRect = PixelRect::new(188, 383, 25, 23);

/// `FONT5` (`Text_SetFont(GNW_TEXT_FONT_5)` в конструкторе).
const LABEL_FONT: GnwFontId = GnwFontId::Font5;
/// Высота строки подписи в `Text_TextBox` (`Text_GetHeight() + 1`).
const LINE_HEIGHT: i32 = 11;
/// Ширина жёлоба слайдера (`PRFSLIT`, 57×3).
pub const SLIDER_SLIT_WIDTH: i32 = 57;
/// Высота области слайдера (`Text_TextBox` 20 px).
const SLIDER_ROW_HEIGHT: i32 = 20;

/// `COLOR_GREEN` заголовка.
const GREEN: u8 = 0x02;
/// `Fonts_GoldColor` — подписи/кнопки.
const GOLD: (u8, u8, u8) = (165, 175, 199);
/// `Fonts_BrightYellowColor` — read-only значения секции `options`.
const BRIGHT_YELLOW: (u8, u8, u8) = (162, 8, 199);
/// Цвет значения в поле ввода (`0xA2`).
const EDIT_TEXT: u8 = 0xA2;

/// Размер поля ввода по ключу контрола.
const EDIT_W: i32 = 60;
/// Высота `PREFEDIT`.
const EDIT_H: i32 = 19;
/// Ширина `PREFNAME`.
const NAME_W: i32 = 187;
/// Высота `PREFNAME`.
const NAME_H: i32 = 17;

/// Вариант диалога: `SETUPPIC` (главное меню) или `PREFSPIC` (игра).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesVariant {
    /// `menu_setup_window()`: только секция `setup`.
    Setup,
    /// `menu_preferences_window()`: все секции (кроме `debug`).
    InGame,
}

impl PreferencesVariant {
    /// Заголовок окна (`_(7013)`/`_(f70f)`).
    pub const fn title(self) -> &'static str {
        match self {
            Self::Setup => "Setup",
            Self::InGame => "Preferences",
        }
    }

    /// Секция помощи (`SETUP_MENU_SETUP`/`PREFS_MENU_SETUP`).
    pub const fn help_section(self) -> crate::helpmenu::HelpSection {
        match self {
            Self::Setup => crate::helpmenu::HelpSection::Setup,
            Self::InGame => crate::helpmenu::HelpSection::Preferences,
        }
    }
}

/// Ключ опции (имя настройки C++ / наше поле `Preferences`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefKey {
    /// Секция `setup`.
    SectionSetup,
    /// Подпись `Volume:` (label, `music_level`).
    MusicVolumeLabel,
    /// `enhanced_graphics` (только `SETUPPIC`).
    EnhancedGraphics,
    /// `music_level` (слайдер 0..100).
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
    /// `host_address` (только `SETUPPIC`).
    HostAddress,
    /// `host_port` (только `SETUPPIC`).
    HostPort,
    /// `player_name`.
    PlayerName,
    /// Секция `preferences`.
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
    /// `quick_scroll` (слайдер 4..128).
    QuickScroll,
    /// Секция `options`.
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
    /// Секция `invalid` (debug-контролы; порт останавливается).
    SectionInvalid,
}

impl PrefKey {
    /// Имя настройки для QA/логов (`effects`, `disable_fx`, …).
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
        }
    }

    /// Разбор имени (QA `prefs toggle …`); регистр и `-`/`_` не важны.
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
            _ => return None,
        };

        Some(key)
    }

    /// Контрол переключается кликом/`Space` (чекбокс).
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
        )
    }

    /// Контрол — слайдер.
    pub const fn is_slider(self) -> bool {
        matches!(
            self,
            Self::MusicLevel | Self::FxSoundLevel | Self::VoiceLevel | Self::MovieLevel | Self::QuickScroll
        )
    }

    /// Контрол — поле ввода.
    pub const fn is_edit(self) -> bool {
        matches!(
            self,
            Self::HostAddress | Self::HostPort | Self::PlayerName | Self::TurnTimer | Self::EndTurn
        )
    }

    /// Границы слайдера/числового поля (`range_min`, `range_max` из C++).
    pub const fn range(self) -> (i32, i32) {
        match self {
            Self::QuickScroll => (4, 128),
            Self::HostPort => (1024, 65535),
            Self::TurnTimer | Self::EndTurn => (0, 32767),
            _ => (0, 100),
        }
    }
}

/// Тип контрола (`OptionsType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    /// Секция-разделитель (не рисуется).
    Section,
    /// Только подпись/значение.
    Label,
    /// Чекбокс `UNCHKED`/`CHECKED`.
    Checkbox,
    /// Слайдер с жёлобом `PRFSLIT` и ползунком `PRFSLIDE`.
    Slider,
    /// Числовое поле `PREFEDIT`.
    EditInt,
    /// Строковое поле `PREFEDIT`/`PREFNAME`.
    EditStr,
}

/// Опция таблицы `options_menu_buttons` (в порядке C++).
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

/// Дословная таблица `OptionsMenu::options_menu_buttons` (36 строк).
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
    // debug-секция (`invalid`) в порт не входит: 6 контролов ниже.
    def(PrefKey::SectionInvalid, ControlKind::Checkbox, "", 25),
    def(PrefKey::SectionInvalid, ControlKind::Checkbox, "", 210),
    def(PrefKey::SectionInvalid, ControlKind::EditInt, "", 25),
    def(PrefKey::SectionInvalid, ControlKind::EditInt, "", 210),
    def(PrefKey::SectionInvalid, ControlKind::EditInt, "", 25),
    def(PrefKey::SectionInvalid, ControlKind::EditInt, "", 210),
];

/// Значения `play_mode`/`opponent`/`victory_*` для read-only подписей.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreferencesInfo {
    /// Режим игры (`PlayMode`).
    pub play_mode: PlayMode,
    /// Сложность ИИ (`Opponent`).
    pub opponent: Opponent,
    /// Лимит победы (`victory_limit`).
    pub victory_limit: i32,
    /// Тип победы (`victory_type`).
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
    /// Подпись `Play Mode: %s` (`d0a7`/`ee25`).
    pub fn play_mode_text(self) -> String {
        let value = match self.play_mode {
            PlayMode::TurnBased => "Turn Based",
            PlayMode::SimultaneousMoves => "Simultaneous Moves",
            PlayMode::Unknown => "Unknown",
        };

        format!("Play Mode: {value}")
    }

    /// Подпись `Computer Player(s): %s` (`a24d`…`c7e3`).
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

    /// Подпись `Game ends at %i %s.` (`b542`/`c303`).
    pub fn victory_text(self) -> String {
        let unit = match self.victory_type {
            VictoryType::Duration => "turns",
            VictoryType::Score => "points",
        };

        format!("Game ends at {} {unit}.", self.victory_limit)
    }
}

/// Пользовательские настройки (`SETUP` + `PREFERENCES` + часть `OPTIONS`).
///
/// Значения по умолчанию — `settings.cpp`: `effects=1`, `click_scroll=1`,
/// `quick_scroll=16`, `fast_movement=1`, `follow_unit=0`, `auto_select=0`,
/// `enemy_halt=1`; громкости 100, `auto_save=1`, `enhanced_graphics=1`,
/// `player_name="Player 1"`, `timer=180`, `endturn=45`.
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
    /// `enhanced_graphics`.
    pub enhanced_graphics: bool,
    /// `player_name`.
    pub player_name: String,
    /// `host_address`.
    pub host_address: String,
    /// `host_port`, 1024..=65535.
    pub host_port: i32,
    /// `timer` (сек).
    pub turn_timer: i32,
    /// `endturn` (сек).
    pub endturn: i32,
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
            enhanced_graphics: true,
            player_name: "Player 1".to_string(),
            host_address: "127.0.0.1".to_string(),
            host_port: 31554,
            turn_timer: 180,
            endturn: 45,
        }
    }
}

impl Preferences {
    /// `IniPreferences` (7 полей V70: `effects`, `click_scroll`,
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

    /// Применяет `IniPreferences` из сейва (как `SaveLoad_Load`:
    /// `settings->SetNumericValue(...)` для семи полей).
    pub fn apply_ini(&mut self, ini: IniPreferences) {
        self.effects = ini.effects != 0;
        self.click_scroll = ini.click_scroll != 0;
        self.quick_scroll = ini.quick_scroll.clamp(4, 128);
        self.fast_movement = ini.fast_movement != 0;
        self.follow_unit = ini.follow_unit != 0;
        self.auto_select = ini.auto_select != 0;
        self.enemy_halt = ini.enemy_halt != 0;
    }

    /// Звук SFX включён (`!disable_fx`).
    pub fn sound_enabled(&self) -> bool {
        !self.disable_fx
    }

    /// Громкость SFX 0.0..=1.0 (`fx_sound_level / 100`).
    pub fn sound_volume(&self) -> f32 {
        self.fx_sound_level.clamp(0, 100) as f32 / 100.0
    }

    /// Значение чекбокса по ключу.
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
            _ => return None,
        })
    }

    /// Числовое значение по ключу.
    pub fn int_value(&self, key: PrefKey) -> Option<i32> {
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

    /// Строковое значение по ключу.
    pub fn string_value(&self, key: PrefKey) -> Option<&str> {
        match key {
            PrefKey::PlayerName => Some(&self.player_name),
            PrefKey::HostAddress => Some(&self.host_address),
            _ => None,
        }
    }

    /// Устанавливает чекбокс; `false` — ключ не чекбокс.
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
            _ => return false,
        }

        true
    }

    /// Устанавливает число с приведением к границам контрола.
    pub fn set_int(&mut self, key: PrefKey, value: i32) -> bool {
        let (min, max) = key.range();
        let value = value.clamp(min, max);

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

    /// Строковое значение по ключу (для commit поля ввода).
    pub fn set_string(&mut self, key: PrefKey, value: &str) -> bool {
        match key {
            PrefKey::PlayerName => self.player_name = value.to_string(),
            PrefKey::HostAddress => self.host_address = value.to_string(),
            _ => return false,
        }

        true
    }

    /// Читает `preferences.ini` (неизвестные ключи игнорируются); `None` —
    /// файла нет/не читается.
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
                _ => {}
            }
        }

        Some(prefs)
    }

    /// Пишет `preferences.ini` (создаёт каталог).
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
        push_bool(&mut out, "enhanced_graphics", self.enhanced_graphics);
        out.push_str(&format!("player_name={}\n", self.player_name));
        out.push_str(&format!("host_address={}\n", self.host_address));
        out.push_str(&format!("host_port={}\n", self.host_port));
        out.push_str(&format!("timer={}\n", self.turn_timer));
        out.push_str(&format!("endturn={}\n", self.endturn));

        std::fs::write(path, out)
    }
}

fn push_bool(out: &mut String, name: &str, value: bool) {
    out.push_str(&format!("{name}={}\n", i32::from(value)));
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

/// Одна строка диалога (координаты — локальные, внутри `PREFSPIC`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Ключ настройки.
    pub key: PrefKey,
    /// Тип контрола.
    pub kind: ControlKind,
    /// Подпись (с `%s` для read-only подписей).
    pub caption: String,
    /// Локальный `x` контрола (`options_menu_buttons[i].ulx`).
    pub x: i32,
    /// Локальный `y` контрола (`window_uly`).
    pub y: i32,
    /// Прямоугольник подписи (`Text_TextBox`).
    pub label: PixelRect,
    /// Прямоугольник клика/контрола.
    pub rect: PixelRect,
    /// Жёлоб слайдера (`PRFSLIT`), если это слайдер.
    pub slider: Option<PixelRect>,
    /// Поле ввода (`PREFEDIT`/`PREFNAME`), если это edit.
    pub field: Option<PixelRect>,
}

/// Действие диалога.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesAction {
    /// Ничего не изменилось.
    None,
    /// Значение изменилось: применить к `App::preferences` сразу (эффекты/звук).
    Changed,
    /// `Done`/`Enter`: сохранить.
    Done,
    /// `Cancel`/`Esc`: вернуть значения на момент открытия.
    Cancel,
    /// `?`: окно помощи.
    Help,
}

/// Открытый диалог `Preferences` с редактируемой копией значений.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferencesDialog {
    /// `SETUPPIC` или `PREFSPIC`.
    variant: PreferencesVariant,
    /// Редактируемые значения.
    pub draft: Preferences,
    /// Снимок для `Cancel`.
    pub initial: Preferences,
    /// Значения read-only подписей.
    pub info: PreferencesInfo,
    rows: Vec<Row>,
    /// Индекс строки в фокусе (клавиатура).
    pub focus: Option<usize>,
    /// Индекс редактируемой строки.
    editing: Option<usize>,
    /// Буфер редактирования (`char text_buffer[64]`).
    edit_buffer: String,
    /// Исходное значение редактируемой строки.
    edit_initial: String,
}

impl PreferencesDialog {
    /// Создаёт диалог с раскладкой `OptionsMenu::Init` для варианта.
    pub fn new(variant: PreferencesVariant, prefs: Preferences, info: PreferencesInfo) -> Self {
        let rows = layout(variant);

        Self {
            variant,
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

    /// Вариант диалога.
    pub const fn variant(&self) -> PreferencesVariant {
        self.variant
    }

    /// Видимые строки (в порядке C++).
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Строка по ключу.
    pub fn row(&self, key: PrefKey) -> Option<&Row> {
        self.rows.iter().find(|row| row.key == key)
    }

    /// Индекс редактируемой строки (для тестов/QA).
    pub fn editing(&self) -> Option<usize> {
        self.editing
    }

    /// Текст в поле ввода (для тестов).
    pub fn edit_buffer(&self) -> &str {
        &self.edit_buffer
    }

    /// Текст read-only подписи (с подставленными значениями).
    pub fn label_text(&self, key: PrefKey) -> Option<String> {
        Some(match key {
            PrefKey::PlayModeLabel => self.info.play_mode_text(),
            PrefKey::OpponentLabel => self.info.opponent_text(),
            PrefKey::VictoryLabel => self.info.victory_text(),
            PrefKey::MusicVolumeLabel => "Volume:".to_string(),
            _ => return None,
        })
    }

    /// Значение контрола числом (чекбокс — 0/1) для `assert prefs-value`.
    pub fn value(&self, key: PrefKey) -> Option<i32> {
        if let Some(value) = self.draft.bool_value(key) {
            return Some(i32::from(value));
        }
        self.draft.int_value(key)
    }

    /// Строковое значение контрола (для тестов/логов).
    pub fn string_value(&self, key: PrefKey) -> Option<String> {
        if let Some(value) = self.draft.string_value(key) {
            return Some(value.to_string());
        }
        None
    }

    /// Переключает чекбокс (клавиатура/QA-тест). `false` — ключ не чекбокс.
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

    /// Ставит слайдер (клик/QA). `false` — ключ не слайдер.
    pub fn set_slider(&mut self, key: PrefKey, value: i32) -> bool {
        let Some(row) = self.rows.iter().find(|row| row.key == key) else {
            return false;
        };
        if row.kind != ControlKind::Slider {
            return false;
        }

        self.draft.set_int(key, value)
    }

    /// Экранный прямоугольник контрола (для QA-кликов/тестов).
    pub fn screen_rect(&self, key: PrefKey) -> Option<PixelRect> {
        let row = self.row(key)?;

        Some(PixelRect::new(
            WINDOW_ULX + row.rect.ulx,
            WINDOW_ULY + row.rect.uly,
            row.rect.width,
            row.rect.height,
        ))
    }

    /// Экранная `x`-координата слайдера для значения `value` (для QA).
    pub fn slider_screen_x(&self, key: PrefKey, value: i32) -> Option<i32> {
        let row = self.row(key)?;
        let slider = row.slider?;
        let (min, max) = key.range();
        let value = value.clamp(min, max);
        let x = slider.ulx + value * SLIDER_SLIT_WIDTH / max;

        Some(WINDOW_ULX + x)
    }

    /// Значение слайдера, которое даст клик в экранную точку `screen_x`
    /// (`OptionsMenu::UpdateSlider`: `((x − track)·max) / 57`).
    pub fn slider_value_at(&self, key: PrefKey, screen_x: i32) -> Option<i32> {
        let row = self.row(key)?;
        let slider = row.slider?;
        let (min, max) = key.range();
        let value = (screen_x - WINDOW_ULX - slider.ulx) * max / SLIDER_SLIT_WIDTH;

        Some(value.clamp(min, max))
    }

    /// Обрабатывает ввод.
    pub fn input(&mut self, event: InputEvent) -> PreferencesAction {
        match event {
            InputEvent::Key(key) => self.key(key),
            InputEvent::MouseClick(x, y) => self.click(x, y),
            InputEvent::MouseMove(..) | InputEvent::RightClick(..) => PreferencesAction::None,
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

        // Клик мимо активного поля фиксирует его значение — как
        // `ProcessTextEditKeyPress(GNW_KB_KEY_RETURN)` перед обработкой контрола.
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
                    // C++: первое Esc при активном поле только выходит из
                    // редактирования (`ProcessTextEditKeyPress(ESC)`).
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

    /// Фиксирует значение поля ввода. `true` — значение изменилось.
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

    /// Рисует диалог поверх кадра 640×480.
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

        // Заголовок (`COLOR_GREEN`, FONT5, по центру 184×17).
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

/// Раскладка `OptionsMenu::Init` для варианта.
fn layout(variant: PreferencesVariant) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut y = 20;
    let font_height = LABEL_FONT.height();

    for def in &DEFS {
        if !is_visible(variant, def.key) {
            continue;
        }

        if def.kind == ControlKind::Section {
            // `SETUPPIC` завершает список на секции `preferences`,
            // `PREFSPIC` — на `invalid` (debug-секция не портирована).
            let stop = match def.key {
                PrefKey::SectionPreferences => variant == PreferencesVariant::Setup,
                PrefKey::SectionInvalid => true,
                _ => false,
            };

            if stop {
                break;
            }

            y += 20;
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

/// `IsControlVisible`: графика и сеть — только в `SETUPPIC`.
fn is_visible(variant: PreferencesVariant, key: PrefKey) -> bool {
    if matches!(
        key,
        PrefKey::EnhancedGraphics | PrefKey::HostAddress | PrefKey::HostPort
    ) {
        return variant == PreferencesVariant::Setup;
    }

    true
}

/// Подпись `Text_TextBox` с `FontColor`: перенос по словам, вертикальное центрирование.
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

/// Кнопка диалога: картинка `up` + подпись `FONT5` (`Button::SetCaption`).
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

/// Обрезка длинного значения по ширине поля.
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

/// Ресурсы диалога из `MAX.RES`.
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
    /// Грузит ресурсы; отсутствие части — не ошибка (рисуются рамки).
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

    /// Все обязательные картинки загрузились.
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
// Интеграция с приложением
// ---------------------------------------------------------------------------

impl crate::App {
    /// Текущие пользовательские настройки.
    pub fn preferences(&self) -> &Preferences {
        &self.preferences
    }

    /// Заменяет настройки (эффекты/звук применяются отрисовкой/аудио-слоем).
    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.preferences = preferences;
    }

    /// Включены ли визуальные эффекты (`settings["effects"]`).
    pub fn effects_enabled(&self) -> bool {
        self.preferences.effects
    }

    /// Включён ли звук SFX (`!disable_fx`).
    pub fn sound_enabled(&self) -> bool {
        self.preferences.sound_enabled()
    }

    /// Файл настроек: `<save_dir>/preferences.ini` (C++ `settings.json` в
    /// `FilePathGamePref`).
    pub fn preferences_path(&self) -> PathBuf {
        self.save_dir().join("preferences.ini")
    }

    /// Сохраняет настройки на диск; ошибка — в журнал HUD (не падение).
    pub fn persist_preferences(&mut self) {
        let path = self.preferences_path();

        if let Err(error) = self.preferences.save_file(&path) {
            self.hud
                .push_log(format!("Preferences: не удалось сохранить {}: {error}", path.display()));
        }
    }

    /// Загружает `preferences.ini`, если он есть (только интерактивный запуск).
    pub fn load_preferences_from_disk(&mut self) -> bool {
        let path = self.preferences_path();
        let Some(preferences) = Preferences::load_file(&path) else {
            return false;
        };

        self.preferences = preferences;
        true
    }

    /// Открытый диалог `Preferences`; `None` — закрыт.
    pub fn preferences_dialog(&self) -> Option<&PreferencesDialog> {
        self.preferences_dialog.as_ref()
    }

    /// Открытый диалог для ввода/QA.
    pub fn preferences_dialog_mut(&mut self) -> Option<&mut PreferencesDialog> {
        self.preferences_dialog.as_mut()
    }

    /// Значения read-only подписей (`Play Mode`, `Computer Player(s)`, …).
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

    /// Открывает `Preferences` (кнопка панели, `PREFSPIC`).
    ///
    /// Возвращает `false`, если диалог уже открыт или открыт другой модальный
    /// экран (в C++ главное меню в это время выключено
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
        self.preferences_dialog = Some(PreferencesDialog::new(
            PreferencesVariant::InGame,
            self.preferences.clone(),
            info,
        ));
        true
    }

    /// Закрывает диалог (без сохранения).
    pub fn close_preferences(&mut self) {
        self.preferences_dialog = None;
    }

    /// Ввод в открытом диалоге; `None` — диалог закрыт.
    ///
    /// `Changed` сразу применяет черновик (как `SetNumericValue`/`SetVolume` в
    /// C++), `Done` сохраняет в файл, `Cancel` возвращает снимок открытия.
    pub fn preferences_input(&mut self, event: InputEvent) -> Option<PreferencesAction> {
        let mut dialog = self.preferences_dialog.take()?;
        let action = dialog.input(event);

        match action {
            PreferencesAction::Changed => {
                self.preferences = dialog.draft.clone();
                self.preferences_dialog = Some(dialog);
            }
            PreferencesAction::None | PreferencesAction::Help => {
                self.preferences_dialog = Some(dialog);
            }
            PreferencesAction::Done => {
                self.preferences = dialog.draft.clone();
                self.persist_preferences();
                self.hud.push_log("Preferences saved");
            }
            PreferencesAction::Cancel => {
                self.preferences = dialog.initial.clone();
            }
        }

        Some(action)
    }
}

/// `PathBuf` для `Cargo.toml`/тестов: путь к файлу настроек рядом с сейвами.
pub fn preferences_path_for(save_dir: &Path) -> PathBuf {
    save_dir.join("preferences.ini")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(variant: PreferencesVariant) -> PreferencesDialog {
        PreferencesDialog::new(variant, Preferences::default(), PreferencesInfo::default())
    }

    /// Раскладка `OptionsMenu::Init` для `PREFSPIC` (без debug-секции).
    ///
    /// `window_uly` = 20; секция +20; каждая опция с `ulx == 25` +20; label
    /// после отрисовки `−7` (`20 − (10 + 3)`).
    #[test]
    fn in_game_layout_matches_the_cpp_window_uly_algorithm() {
        let dialog = dialog(PreferencesVariant::InGame);
        let y = |key| dialog.row(key).map(|row| row.y);

        assert_eq!(y(PrefKey::MusicVolumeLabel), Some(60));
        assert_eq!(y(PrefKey::MusicLevel), Some(73));
        assert_eq!(y(PrefKey::DisableMusic), Some(73));
        assert_eq!(y(PrefKey::FxSoundLevel), Some(93));
        assert_eq!(y(PrefKey::DisableFx), Some(93));
        assert_eq!(y(PrefKey::VoiceLevel), Some(113));
        assert_eq!(y(PrefKey::MovieLevel), Some(133));
        assert_eq!(y(PrefKey::AutoSave), Some(153));
        assert_eq!(y(PrefKey::PlayerName), Some(173));
        assert_eq!(y(PrefKey::Effects), Some(213));
        assert_eq!(y(PrefKey::ClickScroll), Some(213));
        assert_eq!(y(PrefKey::FastMovement), Some(233));
        assert_eq!(y(PrefKey::EnemyHalt), Some(253));
        assert_eq!(y(PrefKey::QuickScroll), Some(273));
        assert_eq!(y(PrefKey::TurnTimer), Some(313));
        assert_eq!(y(PrefKey::EndTurn), Some(313));
        assert_eq!(y(PrefKey::PlayModeLabel), Some(333));
        assert_eq!(y(PrefKey::OpponentLabel), Some(346));
        assert_eq!(y(PrefKey::VictoryLabel), Some(359));

        // `enhanced_graphics`/`host_address`/`host_port` — только `SETUPPIC`.
        assert!(dialog.row(PrefKey::EnhancedGraphics).is_none());
        assert!(dialog.row(PrefKey::HostAddress).is_none());
        assert!(dialog.row(PrefKey::HostPort).is_none());
        assert_eq!(dialog.rows().len(), 23);
    }

    /// `SETUPPIC`: `Init` выходит на секции `preferences`.
    #[test]
    fn setup_layout_stops_at_the_preferences_section() {
        let dialog = dialog(PreferencesVariant::Setup);
        let y = |key| dialog.row(key).map(|row| row.y);

        assert_eq!(y(PrefKey::MusicVolumeLabel), Some(60));
        // Label отнимает 7: чекбокс справа рисуется на 53.
        assert_eq!(y(PrefKey::EnhancedGraphics), Some(53));
        assert_eq!(y(PrefKey::HostAddress), Some(173));
        assert_eq!(y(PrefKey::HostPort), Some(193));
        assert_eq!(y(PrefKey::PlayerName), Some(213));
        assert!(dialog.row(PrefKey::Effects).is_none());
        assert_eq!(dialog.rows().len(), 14);
    }

    /// Клик по чекбоксу переключает значение (как `win_button_down` в C++),
    /// `Enter` — `Done`, `Esc` — `Cancel`.
    #[test]
    fn checkbox_click_toggles_and_enter_is_done() {
        let mut dialog = dialog(PreferencesVariant::InGame);
        let rect = dialog.screen_rect(PrefKey::Effects).expect("effects");
        let center = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

        assert!(dialog.draft.effects);
        assert_eq!(
            dialog.input(InputEvent::MouseClick(center.0, center.1)),
            PreferencesAction::Changed
        );
        assert!(!dialog.draft.effects);

        // `Tab`/`Space`: фокус начинается с первого контрола (`Volume:` label),
        // второй `Tab` — слайдер `Music`; `Space` на слайдере ничего не делает.
        dialog.focus = None;
        assert_eq!(dialog.input(InputEvent::Key(Key::Tab)), PreferencesAction::None);
        assert_eq!(dialog.input(InputEvent::Key(Key::Tab)), PreferencesAction::None);
        let focus = dialog.focus.expect("focus");
        assert_eq!(dialog.rows()[focus].key, PrefKey::MusicLevel);
        assert_eq!(
            dialog.input(InputEvent::Key(Key::Char(' '))),
            PreferencesAction::None,
            "Space на слайдере ничего не делает"
        );

        assert_eq!(dialog.input(InputEvent::Key(Key::Enter)), PreferencesAction::Done);
        assert_eq!(dialog.input(InputEvent::Key(Key::Escape)), PreferencesAction::Cancel);
    }

    /// Клик по слайдеру ставит значение по формуле `UpdateSlider`.
    #[test]
    fn slider_click_sets_the_value_by_track_position() {
        let mut dialog = dialog(PreferencesVariant::InGame);
        let x = dialog
            .slider_screen_x(PrefKey::QuickScroll, 36)
            .expect("slider");
        let expected = dialog.slider_value_at(PrefKey::QuickScroll, x).unwrap();
        let rect = dialog.screen_rect(PrefKey::QuickScroll).expect("row");
        let y = rect.uly + rect.height / 2;

        assert_eq!(
            dialog.input(InputEvent::MouseClick(x, y)),
            PreferencesAction::Changed
        );
        assert_eq!(dialog.draft.quick_scroll, expected);
        assert!((expected - 36).abs() <= 1, "квантование жёлоба: {expected}");

        // Клавиатура: Right/Left двигают слайдер в фокусе.
        dialog.focus = dialog
            .rows()
            .iter()
            .position(|row| row.key == PrefKey::QuickScroll);
        dialog.input(InputEvent::Key(Key::Right));
        assert_eq!(dialog.draft.quick_scroll, expected + 4);
    }

    /// Поле имени: клик, ввод, `Enter` — значение в черновике (`TextEdit` C++).
    #[test]
    fn player_name_edit_accepts_typing_and_enter() {
        let mut dialog = dialog(PreferencesVariant::Setup);
        let rect = dialog.screen_rect(PrefKey::PlayerName).expect("name");
        let (x, y) = (rect.ulx + 4, rect.uly + rect.height / 2);

        dialog.input(InputEvent::MouseClick(x, y));
        assert!(dialog.editing().is_some());

        for _ in 0..16 {
            dialog.input(InputEvent::Key(Key::Backspace));
        }
        for character in "Commander".chars() {
            dialog.input(InputEvent::Key(Key::Char(character)));
        }
        dialog.input(InputEvent::Key(Key::Enter));

        assert_eq!(dialog.draft.player_name, "Commander");
        assert!(dialog.editing().is_none());
    }

    /// `IniPreferences` — ровно 7 полей V70.
    #[test]
    fn ini_preferences_roundtrip() {
        let mut prefs = Preferences::default();
        prefs.effects = false;
        prefs.click_scroll = false;
        prefs.quick_scroll = 40;
        prefs.fast_movement = false;
        prefs.follow_unit = true;
        prefs.auto_select = true;
        prefs.enemy_halt = false;

        let ini = prefs.to_ini();
        assert_eq!(
            ini,
            IniPreferences {
                effects: 0,
                click_scroll: 0,
                quick_scroll: 40,
                fast_movement: 0,
                follow_unit: 1,
                auto_select: 1,
                enemy_halt: 0,
            }
        );

        let mut restored = Preferences::default();
        restored.apply_ini(ini);
        assert_eq!(
            (
                restored.effects,
                restored.click_scroll,
                restored.quick_scroll,
                restored.fast_movement,
                restored.follow_unit,
                restored.auto_select,
                restored.enemy_halt,
            ),
            (false, false, 40, false, true, true, false)
        );
    }

    /// Файл настроек переживает запись/чтение (C++ `settings.json`).
    #[test]
    fn preferences_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "max-rust-prefs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_nanos())
                .unwrap_or(0)
        ));
        let path = preferences_path_for(&dir);
        let mut prefs = Preferences::default();
        prefs.effects = false;
        prefs.quick_scroll = 40;
        prefs.disable_fx = true;
        prefs.player_name = "Cmdr".to_string();

        prefs.save_file(&path).expect("save");
        let loaded = Preferences::load_file(&path).expect("load");

        assert_eq!(loaded, prefs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// По умолчанию эффекты и звук включены (`settings.cpp`: `effects = 1`,
    /// `disable_fx = 0`).
    #[test]
    fn defaults_match_settings_cpp() {
        let prefs = Preferences::default();
        assert!(prefs.effects);
        assert!(prefs.sound_enabled());
        assert!((prefs.sound_volume() - 1.0).abs() < f32::EPSILON);
        assert_eq!(prefs.quick_scroll, 16);
        assert!(prefs.auto_save);
        assert_eq!(prefs.turn_timer, 180);
        assert_eq!(prefs.endturn, 45);
    }

    /// `PrefKey::parse` понимает имена QA-команд.
    #[test]
    fn prefs_key_names_roundtrip() {
        for key in [
            PrefKey::Effects,
            PrefKey::DisableFx,
            PrefKey::ClickScroll,
            PrefKey::QuickScroll,
            PrefKey::PlayerName,
            PrefKey::TurnTimer,
            PrefKey::EndTurn,
        ] {
            assert_eq!(PrefKey::parse(key.name()), Some(key), "{}", key.name());
        }
        assert_eq!(PrefKey::parse("sound"), Some(PrefKey::DisableFx));
        assert_eq!(PrefKey::parse("track_unit"), Some(PrefKey::FollowUnit));
        assert_eq!(PrefKey::parse("bogus"), None);
    }
}
