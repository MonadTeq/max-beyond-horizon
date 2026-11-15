//! Экран настройки локальной партии (M-SP) — 1:1 с экранами C++-порта.
//!
//! Три экрана, ровно в порядке `menu_custom_game_menu` (`menu.cpp:2115`):
//!
//! | # | Экран | Оракул |
//! |---|---|---|
//! | 1 | `Options` — `GameConfigMenu` (фон `OPTNFRM`) | `gameconfigmenu.cpp/.hpp` |
//! | 2 | `Planet` — `PlanetSelectMenu` (фон `PLANETSE`) | `planetselectmenu.cpp/.hpp` |
//! | 3 | `Players` — `ChoosePlayerMenu` (фон `CHOSPLYR`) | `chooseplayermenu.cpp/.hpp` |
//!
//! Раскладка (координаты, картинки кнопок, тексты) взята из C++-таблиц
//! `game_config_menu_items` / `game_config_menu_controls`,
//! `planet_select_menu_controls` и `choose_player_menu_controls`; английские
//! строки — из `assets/language.json` (`_(hash)` → `en-US`).
//!
//! # Отличие от оригинала
//!
//! Поле **Name** (имя игрока) в C++ задаётся в Preferences (`PREFSPIC`), а не в
//! настройке партии. В Rust-клиенте оно вынесено на свободное место нижней
//! строки экрана `Options` (слева от кнопок `Cancel`/`?`/`Done`): играть
//! локально без имени нельзя, а экран Preferences ещё не портирован. Это
//! единственное дополнение к раскладке оригинала; сами панели и кнопки —
//! 1:1.

use std::path::{Path, PathBuf};

use max_core::enums::{PlayMode, VictoryType};
use max_core::team::TeamKind;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, draw_text_gnw_shadowed, GnwFontId};

use crate::assets::WORLD_ASSETS;
use crate::hotseat::{MSG_TWO_HUMANS, TITLE_HOT_SEAT, TITLE_SINGLE};
use crate::local::{LocalSetup, LOCAL_TEAM_COUNT};
use crate::menu::{
    blit_simple, blit_simple_anchored, clean_status, draw_big_image, draw_image_button,
    truncate_to_width, wrap_gnw_text, MenuAssets,
};

/// Цвет текста (в палитре окна) — `COLOR_GREEN`.
pub const GREEN: u8 = 0x02;
/// `TEXT` — цвет обычного текста меню.
pub const TEXT: u8 = 0xA2;
/// `Fonts_BrightBrownColor` (`fonts.cpp`).
pub const BROWN: (u8, u8, u8) = (164, 173, 199);
/// `Fonts_BrightYellowColor` (`fonts.cpp`).
pub const YELLOW: (u8, u8, u8) = (162, 8, 199);
/// `Fonts_BrightSilverColor` (`fonts.cpp`).
pub const SILVER: (u8, u8, u8) = (162, 172, 199);

// ---------------------------------------------------------------------------
// GameConfigMenu: панели и заголовки
// ---------------------------------------------------------------------------

/// Фон экрана опций — `OPTNFRM`.
pub const OPTIONS_BACKGROUND: &str = "OPTNFRM";
/// Фон выбора команд — `CHOSPLYR`.
pub const PLAYERS_BACKGROUND: &str = "CHOSPLYR";

/// Шесть панелей `bg_panels` (`gameconfigmenu.cpp:93`): 200×190.
pub const CONFIG_PANELS: [PixelRect; 6] = [
    PixelRect::new(6, 38, 200, 190),
    PixelRect::new(216, 38, 200, 190),
    PixelRect::new(426, 38, 200, 190),
    PixelRect::new(6, 233, 200, 190),
    PixelRect::new(216, 233, 200, 190),
    PixelRect::new(426, 233, 200, 190),
];

/// Один заголовок `game_config_menu_items[i]` (`gameconfigmenu.hpp:61`).
#[derive(Debug, Clone, Copy)]
pub struct ConfigItem {
    /// Прямоугольник `bounds`.
    pub rect: PixelRect,
    /// Текст (английский `en-US`).
    pub text: &'static str,
}

macro_rules! ci {
    ($ulx:expr, $uly:expr, $lrx:expr, $lry:expr, $text:expr) => {
        ConfigItem {
            rect: PixelRect::new($ulx, $uly, $lrx - $ulx, $lry - $uly),
            text: $text,
        }
    };
}

/// 71 заголовок экрана опций — дословный перенос таблицы
/// `game_config_menu_items`.
pub const CONFIG_ITEMS: [ConfigItem; 71] = [
    ci!(230, 5, 409, 25, "Options Menu"),
    ci!(19, 50, 199, 70, "Computer Opponent"),
    ci!(230, 50, 409, 70, "Turn Timers"),
    ci!(439, 50, 619, 70, "Play Mode"),
    ci!(19, 245, 199, 265, "Starting Credit"),
    ci!(230, 245, 409, 265, "Resource Levels"),
    ci!(439, 245, 619, 265, "Victory Condition"),
    ci!(56, 85, 156, 100, "Clueless"),
    ci!(56, 100, 156, 115, "Apprentice"),
    ci!(56, 115, 156, 130, "Average"),
    ci!(56, 130, 156, 145, "Expert"),
    ci!(56, 145, 156, 160, "Master"),
    ci!(56, 160, 156, 175, "God"),
    ci!(227, 85, 307, 105, "Turn Limit:"),
    ci!(331, 85, 411, 105, "End Turn:"),
    ci!(227, 105, 307, 120, "No Limit"),
    ci!(227, 120, 307, 135, "60 seconds"),
    ci!(227, 135, 307, 150, "120"),
    ci!(227, 150, 307, 165, "180"),
    ci!(227, 165, 307, 180, "240"),
    ci!(227, 180, 307, 195, "300"),
    ci!(227, 195, 307, 210, "360"),
    ci!(331, 105, 411, 120, "None"),
    ci!(331, 120, 411, 135, "15 seconds"),
    ci!(331, 135, 411, 150, "30"),
    ci!(331, 150, 411, 165, "45"),
    ci!(331, 165, 411, 180, "60"),
    ci!(331, 180, 411, 195, "75"),
    ci!(331, 195, 411, 210, "90"),
    ci!(480, 90, 580, 110, "Turn Based"),
    ci!(480, 115, 580, 145, "Simultaneous Moves"),
    ci!(24, 285, 114, 300, "None"),
    ci!(24, 305, 114, 320, "Low"),
    ci!(24, 325, 114, 340, "Limited"),
    ci!(24, 345, 114, 360, "Normal"),
    ci!(24, 365, 114, 380, "High"),
    ci!(24, 385, 114, 400, "Too Much!"),
    ci!(144, 285, 194, 300, "0"),
    ci!(144, 305, 194, 320, "50"),
    ci!(144, 325, 194, 340, "100"),
    ci!(144, 345, 194, 360, "150"),
    ci!(144, 365, 194, 380, "200"),
    ci!(144, 385, 194, 400, "250"),
    ci!(229, 280, 409, 292, "Raw Materials:"),
    ci!(229, 315, 409, 327, "Fuel:"),
    ci!(229, 350, 409, 362, "Gold:"),
    ci!(229, 385, 409, 397, "Alien Derelicts:"),
    ci!(225, 292, 287, 304, "Poor"),
    ci!(288, 292, 352, 304, "Medium"),
    ci!(351, 292, 413, 304, "Rich"),
    ci!(225, 327, 287, 339, "Poor"),
    ci!(288, 327, 352, 339, "Medium"),
    ci!(351, 327, 413, 339, "Rich"),
    ci!(225, 362, 287, 374, "Poor"),
    ci!(288, 362, 352, 374, "Medium"),
    ci!(351, 362, 413, 374, "Rich"),
    ci!(225, 397, 287, 409, "None"),
    ci!(288, 397, 352, 409, "Rare"),
    ci!(351, 397, 413, 409, "Common"),
    ci!(427, 285, 529, 305, "Duration:"),
    ci!(529, 285, 633, 305, "Score:"),
    ci!(447, 305, 507, 320, "Short"),
    ci!(447, 325, 507, 340, "Medium"),
    ci!(447, 345, 507, 360, "Long"),
    ci!(551, 305, 611, 320, "Low"),
    ci!(551, 325, 611, 340, "Medium"),
    ci!(551, 345, 611, 360, "High"),
    ci!(447, 370, 507, 390, ""),
    ci!(551, 370, 611, 390, ""),
    ci!(447, 390, 507, 410, "turns"),
    ci!(551, 390, 611, 410, "points"),
];

/// Описания сложности — `game_config_menu_difficulty_descriptions`
/// (`gameconfigmenu.cpp:155`), рисуются в `(19,170)` 180×60.
pub const DIFFICULTY_DESCRIPTIONS: [&str; 6] = [
    "Very easy opponent with no advanced tactics or strategies, and a 25% penalty to building.",
    "Basic opponent with some advanced tactics, and no building penalty.",
    "Fairly clever opponent with some advanced strategies and nastier tactics.",
    "Smartest opponent available, with no advantages.",
    "Smartest opponent with a small (25%) bonus to production.",
    "Merciless opponent with a 50% bonus to production.",
];

/// Описание сложности: прямоугольник.
pub const DIFFICULTY_DESCRIPTION: PixelRect = PixelRect::new(19, 170, 180, 60);

/// Значения таймера хода (`game_config_menu_controls[6..13]`, строки 15..21).
pub const TIMER_VALUES: [u32; 7] = [0, 60, 120, 180, 240, 300, 360];
/// Значения таймера конца хода (строки 22..28).
pub const ENDTURN_VALUES: [u32; 7] = [0, 15, 30, 45, 60, 75, 90];
/// Стартовые кредиты (строки 37..42).
pub const CREDIT_VALUES: [u32; 6] = [0, 50, 100, 150, 200, 250];
/// Условие победы по ходам (строки 61..63) → (тип, лимит).
pub const VICTORY_DURATION_VALUES: [(VictoryType, i32); 3] = [
    (VictoryType::Duration, 100),
    (VictoryType::Duration, 200),
    (VictoryType::Duration, 400),
];
/// Условие победы по очкам (строки 64..66) → (тип, лимит).
pub const VICTORY_SCORE_VALUES: [(VictoryType, i32); 3] = [
    (VictoryType::Score, 200),
    (VictoryType::Score, 400),
    (VictoryType::Score, 800),
];

/// Поле имени игрока — свободное место нижней строки `OPTNFRM`.
pub const NAME_FIELD: PixelRect = PixelRect::new(16, 444, 320, 22);

// ---------------------------------------------------------------------------
// PlanetSelectMenu
// ---------------------------------------------------------------------------

/// Заголовок экрана (`planet_select_menu_screen_title`).
pub const PLANET_TITLE: PixelRect = PixelRect::new(230, 6, 180, 20);
/// Имя выбранной планеты (`planet_select_menu_planet_name`).
pub const PLANET_NAME: PixelRect = PixelRect::new(16, 61, 150, 20);
/// Описание планеты (`planet_select_menu_planet_description`).
pub const PLANET_DESCRIPTION: PixelRect = PixelRect::new(41, 350, 561, 60);
/// Большая картинка планеты — `WindowManager_LoadBigImage(SNOW_PIC + world/6)`,
/// у `SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC` origin (26, 98).
pub const PLANET_PICTURE: (i32, i32) = (26, 98);
/// Стрелки смены группы планет (`MNULAROU`/`MNURAROU`), (60,248)/(92,248).
pub const PLANET_ARROW_LEFT: PixelRect = PixelRect::new(60, 248, 30, 22);
/// Правая стрелка.
pub const PLANET_ARROW_RIGHT: PixelRect = PixelRect::new(92, 248, 30, 22);
/// Шесть слотов мини-карт (`planet_select_menu_controls[0..6]`, 112×112).
pub const PLANET_SLOTS: [PixelRect; 6] = [
    PixelRect::new(192, 51, 112, 112),
    PixelRect::new(330, 51, 112, 112),
    PixelRect::new(469, 51, 112, 112),
    PixelRect::new(192, 189, 112, 112),
    PixelRect::new(330, 189, 112, 112),
    PixelRect::new(469, 189, 112, 112),
];

/// Кнопки нижней строки экрана планет (`planet_select_menu_controls[8..12]`).
pub const PLANET_BUTTONS: [(PixelRect, &str, usize); 4] = [
    (PixelRect::new(243, 438, 109, 40), "Random", 4),
    (PixelRect::new(354, 438, 109, 40), "Cancel", 4),
    (PixelRect::new(465, 438, 48, 40), "?", 5),
    (PixelRect::new(514, 438, 109, 40), "Done", 6),
];

/// Имена 24 планет — `menu_planet_names` (`planetselectmenu.cpp:120`).
pub const PLANET_NAMES: [&str; 24] = [
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

/// Описания 24 планет — `menu_planet_descriptions` (`planetselectmenu.cpp:116`).
pub const PLANET_DESCRIPTIONS: [&str; 24] = [
    "This world is named for the shape of its only landmass. If participants on this world are placed too close together, a production strategy is unlikely to work.\nNumber of Participants: Two to three participants is preferable.\nGame Time: Games on this land mass can be very short unless the combatants are well separated.",
    "This area is likely to need a full deployment of land, sea, and air forces. A force starting on the same land mass as another might abandon their base camp and striking out for another land mass to start over.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: Games in this area are likely to be long.",
    "These chunks of ice poking up out of the frozen sea are widely enough separated that participants should have plenty of time to build up naval and air forces to pursue their goals.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: Games in this area are likely to be long.",
    "This single land mass with surrounding islands is so small that two participants starting on the mass makes immediate contact with the enemy very likely. Start on one of the outlying islands to gain plenty of time for force increase.\nNumber of Participants: Two to three participants is preferable.\nGame Time: Games in this area can be very short.",
    "Participants who land on one of the smaller land masses should have plenty of time to build production before encountering an opponent. Landing two on the same land mass will cause instant combat.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: Games in this area are likely to be long.",
    "The nature of this land mass makes construction of water platforms mandatory for a successful colonization. The chance of a preemptive strike by an opponent is high, though not as high as from other single-land areas.\nNumber of Participants: Two to three participants is preferable.\nGame Time: Games in this area should be of medium to long length.",
    "A starting position on one of the outlying islands almost ensures a long time for buildup before contact is made with an enemy.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: A long game unless the participants are all started in close proximity on the main continent.",
    "This archipelago of closely placed islands could be the scene of an entirely land and bridge based campaign, though sea and air assets would be useful.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: Likely to be long unless there are only two participants and they both land on the same island.",
    "Only these few islands peak up from the omnipresent muddy seas of this world. The islands are sufficiently scattered that sea and air elements are necessary for any successful endeavor.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: This is likely to be a long campaign.",
    "Named for the heart-shaped great lake in the center of the land mass, this world can be won by land and air forces alone, but the use of sea forces in the central lake could be crucial.\nNumber of Participants: Two to four can be easily accommodated.\nGame Time: This is likely to be a long campaign.",
    "The name of this world refers to the three lakes encompassed by the otherwise solid land mass of the world. There is a chance that a production-first strategy can run afoul of a preemptive strike by another participant.\nNumber of Participants: Two to three participants is preferable.\nGame Time: Probably short.",
    "This world has a lot of space and virtually every land mass has plenty of room for large armies. Depending on initial placements, the islands might see no activity at all. If every place is populated, expect intense seaborne combats.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: Most games in this location will be long.",
    "This archipelago is tightly grouped in the middle of the planetary sea. There are opportunities for extensive air and sea resources, though an entirely land-and-air-based campaign could be successful.\nNumber of Participants: Two to three participants is preferable.\nGame Time: varies greatly depending on the landing points of the participants.",
    "The useful parts of this world are split apart by a central sea. Control of the central sea with seaborne assets will be essential unless the participants all land on the same continent.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: will be long unless two participants land close together.",
    "This world is generally resourceless except for the region around the impact crater from a major asteroid strike. A combatant can win using just land and air forces. However, control of the inner sea could be important.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: the game can be very long.",
    "This archipelago with a slice of mainland is suited to a balanced combined arm force with extensive resource building. Taking sanctuary from the incessant naval action on the slice of mainland is a legitimate tactic, of course.\nNumber of Participants: Two to three participants is preferable.\nGame Time: The game will probably be long.",
    "This archipelago is best suited to a concentration on sea borne forces. Extensive engineering will be necessary to create areas for proper exploitation of the planetary resources.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: The necessary construction makes this a long game.",
    "The extensive land area on this planet makes it ideal for extensive land-based warfare. Sea borne assets may play a necessary but secondary role, depending on initial placement.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: this is likely to be a long game.",
    "This planet gets its name from its appearance when seen from space. The mountains jutting out of the desert give it a decidedly freckled appearance. There might be extensive use of naval assets, or none.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: Most games in this location will be long games.",
    "Very few lumps of sand poke out of the ocean on this planet. Its situation demands heavy use of water platforms and naval assets. Two participants are very unlikely to end up on the same land mass.\nNumber of Participants: Two to three participants is preferable.\nGame Time: Most games in this location will be long games.",
    "Like any other world where all the land is connected, this can be very risky for a high-production strategy. However, its size does give some protection against a preemptive strike by another participant.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: medium to long unless participants' landing points are close.",
    "The sea passage that separates the two main land masses makes naval action likely here. Those who wish to follow a production strategy might land on the island to the north east.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: this is likely to be a long game.",
    "The central island of this planet makes a natural battleground for all participants, whether they start there or not. Start on one of the corner lands to gain a reasonable amount of time for a strategic buildup.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: Games on this world are likely to be very long.",
    "This world gets its name from the land bottleneck controllable by sea forces and the sea bottleneck controllable by land forces. Both are in the center of the map. Extensive use of all forces is likely in this location.\nNumber of Participants: two to four can be easily accommodated.\nGame Time: Most games on this world are likely to be very long.",
];

// ---------------------------------------------------------------------------
// ChoosePlayerMenu
// ---------------------------------------------------------------------------

/// Заголовок экрана выбора команд (`choose_player_menu_titles[0]`).
pub const PLAYERS_TITLE: PixelRect = PixelRect::new(230, 6, 180, 20);

/// Заголовки колонок `choose_player_menu_titles[1..6]`.
pub const PLAYER_HEADERS: [(PixelRect, &str); 5] = [
    (PixelRect::new(61, 31, 80, 20), "Team"),
    (PixelRect::new(162, 31, 79, 20), "Human"),
    (PixelRect::new(272, 31, 79, 20), "Computer"),
    (PixelRect::new(380, 31, 79, 20), "None"),
    (PixelRect::new(497, 31, 79, 20), "Clan"),
];

/// Кнопки выбора типа команды: (x, y) — `choose_player_menu_controls[0..12]`,
/// `y` строк 67/159/251/343.
pub fn player_type_button_rect(row: usize, column: usize) -> PixelRect {
    let x = [175, 285, 394][column.min(2)];
    let y = 67 + 92 * row as i32;

    PixelRect::new(x, y, 55, 71)
}

/// Прямоугольники флагов команд (`CH_TM1_U`…`CH_TM4_U`).
pub fn player_banner_rect(row: usize) -> PixelRect {
    PixelRect::new(495, 61 + 92 * row as i32, 93, 89)
}

/// Кнопки нижней строки экрана команд (`choose_player_menu_controls[16..19]`).
pub const PLAYER_BUTTONS: [(PixelRect, &str, usize); 3] = [
    (PixelRect::new(354, 438, 109, 40), "Cancel", 4),
    (PixelRect::new(465, 438, 48, 40), "?", 5),
    (PixelRect::new(514, 438, 109, 40), "Done", 6),
];

// ---------------------------------------------------------------------------
// ClanSelectMenu
// ---------------------------------------------------------------------------

/// Фон экрана выбора клана (`CLANSEL`).
pub const CLAN_BACKGROUND: &str = "CLANSEL";
/// Заголовок `clan_select_menu_screen_title`.
pub const CLAN_TITLE: PixelRect = PixelRect::new(230, 6, 180, 20);
/// Восемь карточек `CH_CN1_U`…`CH_CN8_U` 91×86
/// (`clan_select_menu_controls[0..8]`; `lrx/lry` в C++ нулевые, размер берётся
/// из картинки — 91×86).
pub const CLAN_SLOTS: [PixelRect; 8] = [
    PixelRect::new(46, 44, 91, 86),
    PixelRect::new(201, 44, 91, 86),
    PixelRect::new(355, 44, 91, 86),
    PixelRect::new(510, 44, 91, 86),
    PixelRect::new(46, 194, 91, 86),
    PixelRect::new(201, 194, 91, 86),
    PixelRect::new(355, 194, 91, 86),
    PixelRect::new(510, 194, 91, 86),
];
/// Подписи кланов под карточками (`clan_select_menu_clan_icons`).
pub const CLAN_NAME_RECTS: [PixelRect; 8] = [
    PixelRect::new(32, 140, 110, 20),
    PixelRect::new(188, 140, 110, 20),
    PixelRect::new(344, 140, 110, 20),
    PixelRect::new(497, 140, 110, 20),
    PixelRect::new(32, 290, 110, 20),
    PixelRect::new(188, 290, 110, 20),
    PixelRect::new(344, 290, 110, 20),
    PixelRect::new(497, 290, 110, 20),
];
/// Имена кланов в порядке `TeamClan` (1 — The Chosen … 8 — Axis Inc.).
pub const CLAN_NAMES: [&str; 8] = [
    "The Chosen",
    "Crimson Path",
    "Von Griffin",
    "Ayer's Hand",
    "Musashi",
    "Sacred Eights",
    "Seven Knights",
    "Axis Inc.",
];
/// Нижняя строка `ClanSelectMenu` (`clan_select_menu_controls[8..12]`).
pub const CLAN_BUTTONS: [(PixelRect, &str, usize); 4] = [
    (PixelRect::new(243, 438, 109, 40), "Random", 3),
    (PixelRect::new(354, 438, 109, 40), "Cancel", 4),
    (PixelRect::new(465, 438, 48, 40), "?", 5),
    (PixelRect::new(514, 438, 109, 40), "Done", 6),
];

/// Сообщения `ChoosePlayerMenu::EventDone`.
///
/// `_(bc94)` — нужно два участника; `_(06ec)` — hot-seat требует двух людей.
pub const MSG_TWO_PLAYERS: &str = crate::hotseat::MSG_TWO_PLAYERS;
/// Сообщение hot-seat: нужно минимум два человека (`_(06ec)`).
pub const MSG_HOT_SEAT_TWO_HUMANS: &str = MSG_TWO_HUMANS;

/// Экран настройки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupScreen {
    /// `GameConfigMenu`.
    Options,
    /// `PlanetSelectMenu`.
    Planet,
    /// `ChoosePlayerMenu`.
    Players,
    /// `ClanSelectMenu` — выбор клана команды.
    Clan,
}

/// Клан по индексу карточки `0..8` (`CLN1`…`CLN8`).
pub fn clan_from_index(index: usize) -> max_core::enums::TeamClan {
    max_core::enums::TeamClan::try_from((index + 1).min(8) as u8)
        .unwrap_or(max_core::enums::TeamClan::Random)
}

/// Имя клана для подписи (`Random` — «No clan chosen»).
pub fn clan_name(clan: max_core::enums::TeamClan) -> &'static str {
    let index = clan as usize;
    if index == 0 {
        "Random"
    } else {
        CLAN_NAMES[(index - 1).min(CLAN_NAMES.len() - 1)]
    }
}

/// Стартовые типы команд hot-seat (`menu_choose_player_menu_loop`, `menu.cpp:1883`):
/// `menu_custom_game_menu(false)` ставит RED и GREEN людьми, BLUE/GRAY — пустыми.
pub fn hot_seat_default_teams() -> [TeamKind; LOCAL_TEAM_COUNT] {
    [
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]
}

/// Переход из формы настройки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupTransition {
    /// Просто перерисовать.
    None,
    /// Назад в меню/на предыдущий экран.
    Back,
    /// Начать партию с этими настройками.
    Start(Box<LocalSetup>),
}

/// Состояние формы настройки (все три экрана).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSetupForm {
    /// Текущий экран.
    pub screen: SetupScreen,
    /// Настройки партии (таймер, планета, типы команд…).
    pub setup: LocalSetup,
    /// M-HS: форма открыта пунктом `Hot Seat Game` (`is_single_player = false`).
    pub hot_seat: bool,
    /// Идёт редактирование имени.
    pub editing_name: bool,
    /// Редактируется поле условия победы (какой тип: `Duration`/`Score`).
    pub editing_victory: Option<VictoryType>,
    /// Буфер редактирования условия победы.
    pub victory_text: String,
    /// Строка статуса/ошибки.
    pub status: String,
    /// Кэш мини-карт планет (112×112 RGBA).
    pub minimaps: Vec<Option<Vec<u8>>>,
    /// Откуда грузить `.WRL` (каталог данных игры).
    pub world_dir: PathBuf,
    /// Команда, для которой открыт `ClanSelectMenu`.
    pub clan_team: usize,
    /// Выбранный в `ClanSelectMenu` клан (применяется по Done/Random).
    pub clan_selection: max_core::enums::TeamClan,
}

impl Default for GameSetupForm {
    fn default() -> Self {
        Self {
            screen: SetupScreen::Options,
            setup: LocalSetup::default(),
            hot_seat: false,
            editing_name: false,
            editing_victory: None,
            victory_text: String::new(),
            status: String::new(),
            minimaps: (0..24).map(|_| None).collect(),
            world_dir: PathBuf::from("max-original"),
            clan_team: 0,
            clan_selection: max_core::enums::TeamClan::Random,
        }
    }
}

impl GameSetupForm {
    /// Форма с каталогом данных (для мини-карт планет).
    pub fn with_world_dir(world_dir: impl Into<PathBuf>) -> Self {
        Self {
            world_dir: world_dir.into(),
            ..Self::default()
        }
    }

    /// Форма **hot-seat** (`menu_custom_game_menu(false)`).
    ///
    /// Стартовые значения `menu_choose_player_menu_loop` (`menu.cpp:1883`):
    /// RED — человек, GREEN — человек, BLUE/GRAY — пусто.
    pub fn hot_seat() -> Self {
        let mut form = Self {
            hot_seat: true,
            ..Self::default()
        };

        form.setup.hot_seat = true;
        form.setup.team_types = hot_seat_default_teams();
        form
    }

    /// Экраны какого режима настраиваются (`ChoosePlayerMenu::is_single_player`).
    pub fn is_single_player(&self) -> bool {
        !self.hot_seat
    }

    /// Заголовок экрана команд: `_(5512)` / `_(55c8)`.
    pub fn players_title(&self) -> &'static str {
        if self.hot_seat {
            TITLE_HOT_SEAT
        } else {
            TITLE_SINGLE
        }
    }

    /// Индекс выбранной планеты (`0..23`).
    pub fn world(&self) -> usize {
        self.setup.world.clamp(0, 23) as usize
    }

    /// Первая планета текущей группы (`(world / 6) * 6`).
    pub fn world_group_first(&self) -> usize {
        (self.world() / 6) * 6
    }

    /// Индекс выбранной строки таймера хода (`timer` → строка).
    pub fn timer_index(&self) -> usize {
        TIMER_VALUES
            .iter()
            .rposition(|value| self.setup.timer >= *value)
            .unwrap_or(0)
    }

    /// Индекс строки таймера конца хода.
    pub fn endturn_index(&self) -> usize {
        ENDTURN_VALUES
            .iter()
            .rposition(|value| self.setup.endturn >= *value)
            .unwrap_or(0)
    }

    /// Индекс строки стартовых кредитов.
    pub fn credit_index(&self) -> usize {
        CREDIT_VALUES
            .iter()
            .rposition(|value| self.setup.start_gold >= *value)
            .unwrap_or(0)
    }

    /// Индекс условия победы в группе `Duration` (0..2).
    pub fn duration_index(&self) -> usize {
        VICTORY_DURATION_VALUES
            .iter()
            .rposition(|(kind, limit)| {
                *kind == VictoryType::Duration
                    && self.setup.victory_type == VictoryType::Duration
                    && self.setup.victory_limit >= *limit
            })
            .unwrap_or(0)
    }

    /// Индекс условия победы в группе `Score` (0..2).
    pub fn score_index(&self) -> usize {
        VICTORY_SCORE_VALUES
            .iter()
            .rposition(|(kind, limit)| {
                *kind == VictoryType::Score
                    && self.setup.victory_type == VictoryType::Score
                    && self.setup.victory_limit >= *limit
            })
            .unwrap_or(0)
    }

    /// Применить настройки по умолчанию из `--player` и каталога данных.
    pub fn with_player_name(mut self, name: impl Into<String>) -> Self {
        self.setup.player_name = name.into();
        self
    }

    /// Ставит `team_types` (для тестов/повторного входа).
    pub fn with_team_types(mut self, team_types: [TeamKind; LOCAL_TEAM_COUNT]) -> Self {
        self.setup.team_types = team_types;
        self
    }

    /// Загружает мини-карты текущей группы планет (если `.WRL` есть на диске).
    ///
    /// Порядок рендера повторяет `Menu_LoadPlanetMinimap` (`menu.cpp:663`):
    /// палитра `.WRL` делится на 4, каждый индекс переводится в палитру окна
    /// (`Color_MapColor` с полным перебором).
    pub fn ensure_minimaps(&mut self, window_palette: Option<&crate::panels::UiPalette>) {
        let Some(palette) = window_palette else {
            return;
        };

        let first = self.world_group_first();

        for offset in 0..6 {
            let world = first + offset;

            if world >= WORLD_ASSETS.len() || self.minimaps[world].is_some() {
                continue;
            }

            if let Some(image) = load_minimap(&self.world_dir, world, palette) {
                self.minimaps[world] = Some(image);
            }
        }
    }

    /// Статус партии: обновляет поле и отдаёт переход.
    fn fail(&mut self, message: &str) -> SetupTransition {
        self.status = message.to_string();
        SetupTransition::None
    }

    /// Ввод (клавиша/клик). Координаты мыши — экрана оригинала 640×480.
    pub fn handle_input(
        &mut self,
        event: crate::ui::InputEvent,
        _width: u32,
        _height: u32,
    ) -> SetupTransition {
        match event {
            crate::ui::InputEvent::MouseMove(..) => SetupTransition::None,
            crate::ui::InputEvent::RightClick(..) => SetupTransition::None,
            crate::ui::InputEvent::MouseClick(x, y) => self.click(x, y),
            crate::ui::InputEvent::Key(key) => self.key(key),
        }
    }

    fn key(&mut self, key: crate::ui::Key) -> SetupTransition {
        use crate::ui::Key;

        if self.screen == SetupScreen::Clan {
            return self.key_clan(key);
        }

        match key {
            Key::Escape => {
                if self.editing_name {
                    self.editing_name = false;
                    SetupTransition::None
                } else if self.editing_victory.is_some() {
                    self.apply_victory_text();
                    SetupTransition::None
                } else {
                    SetupTransition::Back
                }
            }
            Key::Enter => {
                if self.editing_name {
                    self.editing_name = false;
                    SetupTransition::None
                } else if self.editing_victory.is_some() {
                    self.apply_victory_text();
                    SetupTransition::None
                } else {
                    self.next_screen()
                }
            }
            Key::Backspace => {
                if self.editing_name {
                    self.setup.player_name.pop();
                } else if self.editing_victory.is_some() {
                    self.victory_text.pop();
                }
                SetupTransition::None
            }
            Key::Char(character) => {
                if self.editing_name && (character.is_ascii_graphic() || character == ' ') {
                    if self.setup.player_name.chars().count() < 24 {
                        self.setup.player_name.push(character);
                    }
                } else if self.editing_victory.is_some() && character.is_ascii_digit() {
                    if self.victory_text.len() < 5 {
                        self.victory_text.push(character);
                    }
                }
                SetupTransition::None
            }
            Key::Tab | Key::Up | Key::Down | Key::Left | Key::Right | Key::Other => {
                SetupTransition::None
            }
        }
    }

    /// Переход на следующий экран (Enter без редактирования).
    fn next_screen(&mut self) -> SetupTransition {
        match self.screen {
            SetupScreen::Options => {
                self.screen = SetupScreen::Planet;
                SetupTransition::None
            }
            SetupScreen::Planet => {
                self.screen = SetupScreen::Players;
                SetupTransition::None
            }
            SetupScreen::Players => self.done(),
            SetupScreen::Clan => self.apply_clan(),
        }
    }

    fn key_clan(&mut self, key: crate::ui::Key) -> SetupTransition {
        use crate::ui::Key;

        match key {
            Key::Escape => {
                self.screen = SetupScreen::Players;
                self.status.clear();
                SetupTransition::None
            }
            Key::Enter => self.apply_clan(),
            _ => SetupTransition::None,
        }
    }

    /// `ClanSelectMenu::EventDone`: сохранить выбранный клан команде.
    fn apply_clan(&mut self) -> SetupTransition {
        let team = self.clan_team.min(LOCAL_TEAM_COUNT - 1);
        self.setup.team_clans[team] = self.clan_selection;
        self.status = format!("Clan: {}", clan_name(self.clan_selection));
        self.screen = SetupScreen::Players;
        SetupTransition::None
    }

    /// `ClanSelectMenu::EventRandom`.
    fn apply_random_clan(&mut self) -> SetupTransition {
        let team = self.clan_team.min(LOCAL_TEAM_COUNT - 1);
        self.setup.team_clans[team] = max_core::enums::TeamClan::Random;
        self.clan_selection = max_core::enums::TeamClan::Random;
        self.status = "Clan: random (chosen by the computer)".to_string();
        self.screen = SetupScreen::Players;
        SetupTransition::None
    }

    /// `ChoosePlayerMenu::EventDone` + запуск партии.
    fn done(&mut self) -> SetupTransition {
        // Режим — часть настроек партии (`MISSION_CATEGORY_HOT_SEAT`).
        self.setup.hot_seat = self.hot_seat || self.setup.human_count() > 1;

        if let Err(error) = self.setup.validate() {
            return self.fail(&error);
        }

        match self.setup.play_mode {
            PlayMode::TurnBased | PlayMode::SimultaneousMoves | PlayMode::Unknown => {}
        }

        SetupTransition::Start(Box::new(self.setup.clone()))
    }

    fn click(&mut self, x: i32, y: i32) -> SetupTransition {
        match self.screen {
            SetupScreen::Options => self.click_options(x, y),
            SetupScreen::Planet => self.click_planet(x, y),
            SetupScreen::Players => self.click_players(x, y),
            SetupScreen::Clan => self.click_clan(x, y),
        }
    }

    fn inside(rect: PixelRect, x: i32, y: i32) -> bool {
        rect.contains(x, y)
    }

    fn click_options(&mut self, x: i32, y: i32) -> SetupTransition {
        // Имя: клик по свободному полю — редактирование.
        if Self::inside(NAME_FIELD, x, y) {
            self.editing_name = true;
            self.editing_victory = None;
            return SetupTransition::None;
        }
        if self.editing_name || self.editing_victory.is_some() {
            if self.editing_victory.is_some() {
                self.apply_victory_text();
            }
            self.editing_name = false;
            self.editing_victory = None;
        }

        // Сложность (`game_config_menu_controls[0..6]`).
        for row in 0..6 {
            if Self::inside(CONFIG_ITEMS[7 + row].rect, x, y) {
                self.setup.opponent = row as u8;
                self.status = format!("Opponent: {}", DIFFICULTY_DESCRIPTIONS[row]);
                return SetupTransition::None;
            }
        }

        for (index, value) in TIMER_VALUES.iter().enumerate() {
            if Self::inside(CONFIG_ITEMS[15 + index].rect, x, y) {
                self.setup.timer = *value;
                return SetupTransition::None;
            }
        }
        for (index, value) in ENDTURN_VALUES.iter().enumerate() {
            if Self::inside(CONFIG_ITEMS[22 + index].rect, x, y) {
                self.setup.endturn = *value;
                return SetupTransition::None;
            }
        }

        if Self::inside(CONFIG_ITEMS[29].rect, x, y) {
            self.setup.play_mode = PlayMode::TurnBased;
            return SetupTransition::None;
        }
        if Self::inside(CONFIG_ITEMS[30].rect, x, y) {
            self.setup.play_mode = PlayMode::SimultaneousMoves;
            return SetupTransition::None;
        }

        for (index, value) in CREDIT_VALUES.iter().enumerate() {
            if Self::inside(CONFIG_ITEMS[37 + index].rect, x, y) {
                self.setup.start_gold = *value;
                return SetupTransition::None;
            }
        }

        // Уровни ресурсов: четыре строки по три значения.
        let resource_rows = [
            (47usize, 0usize), // raw
            (50, 1),           // fuel
            (53, 2),           // gold
            (56, 3),           // alien derelicts
        ];
        for (start, kind) in resource_rows {
            for column in 0..3 {
                if Self::inside(CONFIG_ITEMS[start + column].rect, x, y) {
                    match kind {
                        0 => self.setup.raw = column as u8,
                        1 => self.setup.fuel = column as u8,
                        2 => self.setup.gold = column as u8,
                        _ => self.setup.derelicts = column as u8,
                    }
                    return SetupTransition::None;
                }
            }
        }

        // Условие победы: Duration/Score + шесть вариантов.
        for (index, (kind, limit)) in VICTORY_DURATION_VALUES.iter().enumerate() {
            if Self::inside(CONFIG_ITEMS[61 + index].rect, x, y) {
                self.setup.victory_type = *kind;
                self.setup.victory_limit = *limit;
                return SetupTransition::None;
            }
        }
        for (index, (kind, limit)) in VICTORY_SCORE_VALUES.iter().enumerate() {
            if Self::inside(CONFIG_ITEMS[64 + index].rect, x, y) {
                self.setup.victory_type = *kind;
                self.setup.victory_limit = *limit;
                return SetupTransition::None;
            }
        }
        // Поля ввода лимита (строки 67/68): `EventVictoryConditionPrefs`.
        // Клик по полю активирует и тип условия: 67 — ходы, 68 — очки
        // (`active_text_edit_index == 46/47`).
        if Self::inside(CONFIG_ITEMS[67].rect, x, y) {
            self.editing_victory = Some(VictoryType::Duration);
            self.victory_text = self.setup.victory_limit.to_string();
            self.status = "Enter turn limit, Enter to apply".to_string();
            return SetupTransition::None;
        }
        if Self::inside(CONFIG_ITEMS[68].rect, x, y) {
            self.editing_victory = Some(VictoryType::Score);
            self.victory_text = self.setup.victory_limit.to_string();
            self.status = "Enter score limit, Enter to apply".to_string();
            return SetupTransition::None;
        }

        // Кнопки `Cancel`/`?`/`Done`.
        if Self::inside(PixelRect::new(354, 438, 109, 40), x, y) {
            return SetupTransition::Back;
        }
        if Self::inside(PixelRect::new(465, 438, 48, 40), x, y) {
            self.status = "Options help is not ported yet".to_string();
            return SetupTransition::None;
        }
        if Self::inside(PixelRect::new(514, 438, 109, 40), x, y) {
            self.screen = SetupScreen::Planet;
            self.status.clear();
        }

        SetupTransition::None
    }

    /// Текст → лимит победы (`GameConfigMenu::UpdateTextEdit`):
    /// `victory_type = active_text_edit_index - 46`, `victory_limit` — число.
    fn apply_victory_text(&mut self) {
        if let Some(kind) = self.editing_victory.take() {
            if let Ok(value) = self.victory_text.trim().parse::<i32>() {
                self.setup.victory_limit = value.clamp(0, 99_999);
            }
            self.setup.victory_type = kind;
        }
    }

    fn click_planet(&mut self, x: i32, y: i32) -> SetupTransition {
        let first = self.world_group_first();

        for (slot, rect) in PLANET_SLOTS.iter().enumerate() {
            if Self::inside(*rect, x, y) {
                let world = first + slot;
                if world < PLANET_NAMES.len() {
                    // `PlanetSelectMenu::EventPlanet`: `world = group*6 + key`.
                    self.setup.world = world as i64;
                }
                return SetupTransition::None;
            }
        }

        if Self::inside(PLANET_ARROW_LEFT, x, y) {
            // `EventWorld(direction=false)`: группа −6, <0 → 18.
            let group = (self.world() / 6) * 6;
            let world = if group < 6 { 18 } else { group - 6 };
            self.setup.world = world as i64;
            return SetupTransition::None;
        }
        if Self::inside(PLANET_ARROW_RIGHT, x, y) {
            let group = (self.world() / 6) * 6;
            let world = if group + 6 > 23 { 0 } else { group + 6 };
            self.setup.world = world as i64;
            return SetupTransition::None;
        }

        for (rect, label, _) in PLANET_BUTTONS {
            if !Self::inside(rect, x, y) {
                continue;
            }
            return match label {
                "Random" => {
                    // `EventRandom`: случайная планета и сразу Done.
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|value| value.subsec_nanos())
                        .unwrap_or(0);
                    self.setup.world = i64::from(seed % 24);
                    self.screen = SetupScreen::Players;
                    SetupTransition::None
                }
                "Cancel" => {
                    self.screen = SetupScreen::Options;
                    SetupTransition::None
                }
                "Done" => {
                    self.screen = SetupScreen::Players;
                    SetupTransition::None
                }
                _ => {
                    self.status = "Planet help is not ported yet".to_string();
                    SetupTransition::None
                }
            };
        }

        SetupTransition::None
    }

    fn click_players(&mut self, x: i32, y: i32) -> SetupTransition {
        for row in 0..LOCAL_TEAM_COUNT {
            for column in 0..3 {
                if !Self::inside(player_type_button_rect(row, column), x, y) {
                    continue;
                }
                let kind = match column {
                    0 => TeamKind::Player,
                    1 => TeamKind::Computer,
                    _ => TeamKind::None,
                };
                self.select_team_type(row, kind);
                return SetupTransition::None;
            }

            if Self::inside(player_banner_rect(row), x, y) {
                // `ChoosePlayerMenu::EventSelectClan`: открыть `ClanSelectMenu`
                // для этой команды, сохранив текущий выбор.
                self.clan_team = row;
                self.clan_selection = self.setup.team_clans[row];
                self.screen = SetupScreen::Clan;
                self.status.clear();
                return SetupTransition::None;
            }
        }

        for (rect, label, _) in PLAYER_BUTTONS {
            if !Self::inside(rect, x, y) {
                continue;
            }
            return match label {
                "Cancel" => {
                    self.screen = SetupScreen::Planet;
                    SetupTransition::None
                }
                "Done" => self.done(),
                _ => {
                    self.status = "Setup help is not ported yet".to_string();
                    SetupTransition::None
                }
            };
        }

        SetupTransition::None
    }

    /// `ClanSelectMenu`: восемь карточек, Random/Cancel/?/Done.
    fn click_clan(&mut self, x: i32, y: i32) -> SetupTransition {
        for (index, rect) in CLAN_SLOTS.iter().enumerate() {
            if Self::inside(*rect, x, y) {
                self.clan_selection = clan_from_index(index);
                self.status = format!("Clan: {}", CLAN_NAMES[index]);
                return SetupTransition::None;
            }
        }

        for (rect, label, _resource) in CLAN_BUTTONS {
            if !Self::inside(rect, x, y) {
                continue;
            }
            return match label {
                "Random" => self.apply_random_clan(),
                "Cancel" => {
                    self.screen = SetupScreen::Players;
                    self.status.clear();
                    SetupTransition::None
                }
                "Done" => self.apply_clan(),
                _ => {
                    self.status = "Clan help is not ported yet".to_string();
                    SetupTransition::None
                }
            };
        }

        SetupTransition::None
    }

    /// `ChoosePlayerMenu::EventSelectHuman` для одиночной игры: «Human» на
    /// другом слоте переносит флаг, прежний человек получает тип нового слота.
    ///
    /// В hot-seat (`is_single_player = false`) переноса нет: людей можно
    /// выбрать сколько угодно (`chooseplayermenu.cpp:79`).
    pub fn select_team_type(&mut self, row: usize, kind: TeamKind) {
        if kind == TeamKind::Player && self.is_single_player() {
            let previous = self
                .setup
                .team_types
                .iter()
                .position(|slot| *slot == TeamKind::Player);

            if let Some(previous) = previous {
                if previous != row {
                    self.setup.team_types[previous] = self.setup.team_types[row];
                }
            }
        }

        self.setup.team_types[row] = kind;
        self.status.clear();
    }
}

// ---------------------------------------------------------------------------
// Отрисовка
// ---------------------------------------------------------------------------

/// Рисует текущий экран настройки.
///
/// `status` — строка контекста UI; если форма выставила свою строку
/// (`GameSetupForm::status`, например «нужно два игрока»), показывается она.
pub fn draw_setup(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    form: &GameSetupForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let status = if form.status.is_empty() {
        status
    } else {
        form.status.as_str()
    };

    match form.screen {
        SetupScreen::Options => draw_options(frame, menu, form, status, mouse),
        SetupScreen::Planet => draw_planet(frame, menu, form, status, mouse),
        SetupScreen::Players => draw_players(frame, menu, form, status, mouse),
        SetupScreen::Clan => draw_clan(frame, menu, form, status, mouse),
    }
}

/// Фон экрана: `OPTNFRM`/`PLANETSE`/`CHOSPLYR` (fallback — `MAINPIC`).
fn draw_background(frame: &mut RgbaBuffer, menu: &MenuAssets, background: &str) -> u32 {
    match background {
        OPTIONS_BACKGROUND => menu
            .optnfrm
            .as_ref()
            .map(|image| draw_big_image(frame, image))
            .unwrap_or_else(|| draw_big_image(frame, &menu.mainpic)),
        PLAYERS_BACKGROUND => menu
            .chosplyr
            .as_ref()
            .map(|image| draw_big_image(frame, image))
            .unwrap_or_else(|| draw_big_image(frame, menu.planet_background())),
        _ => draw_big_image(frame, menu.planet_background()),
    }
}

/// Заголовок панели: `GNW_TEXT_OUTLINE | COLOR_GREEN` (зелёный + чёрная тень).
fn draw_panel_title(frame: &mut RgbaBuffer, palette: &crate::panels::UiPalette, item: &ConfigItem) {
    let font = GnwFontId::Font5.font();
    let rect = item.rect;
    let width = font.text_width(item.text);
    let x = rect.ulx + (rect.width - (width + 1)).max(0) / 2;
    let y = rect.uly + ((rect.height + 1) - (font.height() + 1)).max(0) / 2;

    draw_text_gnw_shadowed(
        frame,
        x,
        y,
        font,
        item.text,
        palette.rgba(GREEN),
        Rgba::rgb(0, 0, 0),
        Rgba::rgb(0, 0, 0),
    );
}

/// Подпись `FontColor` (жёлтая/коричневая), по центру или слева.
fn draw_caption(
    frame: &mut RgbaBuffer,
    palette: &crate::panels::UiPalette,
    rect: PixelRect,
    text: &str,
    color: (u8, u8, u8),
    centered: bool,
) {
    let font = GnwFontId::Font5.font();
    // `Text_TextBox` с `FontColor`: текст переносится по словам внутри
    // прямоугольника (`Simultaneous Moves` — две строки в (480,115,100,30)).
    let lines = wrap_text(text, rect.width);
    let line_height = font.height() + 1;
    let block = line_height * lines.len() as i32;
    let mut y = rect.uly + ((rect.height + 1) - block).max(0) / 2;

    for line in lines {
        let x = if centered {
            rect.ulx + (rect.width - (font.text_width(&line) + 1)).max(0) / 2
        } else {
            rect.ulx
        };

        draw_text_gnw_shadowed(
            frame,
            x,
            y,
            font,
            &line,
            palette.rgba(color.0),
            palette.rgba(color.1),
            palette.rgba(color.2),
        );
        y += line_height;
    }
}

/// Многострочная подпись `FontColor` в прямоугольнике (`Text_TextBox`).
fn draw_wrapped(
    frame: &mut RgbaBuffer,
    palette: &crate::panels::UiPalette,
    rect: PixelRect,
    text: &str,
    color: (u8, u8, u8),
) {
    let font = GnwFontId::Font5.font();
    let mut y = rect.uly;

    for line in wrap_text(text, rect.width) {
        if y + font.height() > rect.uly + rect.height {
            break;
        }
        draw_text_gnw_shadowed(
            frame,
            rect.ulx,
            y,
            font,
            &line,
            palette.rgba(color.0),
            palette.rgba(color.1),
            palette.rgba(color.2),
        );
        y += font.height() + 1;
    }
}

/// Рамка выбора (`GameConfigMenu::DrawSelector` — `draw_box` цветом 0xFF).
fn draw_selector(frame: &mut RgbaBuffer, palette: &crate::panels::UiPalette, rect: PixelRect) {
    let color = palette.rgba(0xFF);
    let x0 = rect.ulx - 1;
    let y0 = rect.uly - 1;
    let x1 = rect.ulx + rect.width;
    let y1 = rect.uly + rect.height;

    for x in x0..=x1 {
        frame.set(x, y0, color);
        frame.set(x, y1, color);
    }
    for y in y0..=y1 {
        frame.set(x0, y, color);
        frame.set(x1, y, color);
    }
}

/// Экран `GameConfigMenu` (1:1).
fn draw_options(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    form: &GameSetupForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_background(frame, menu, OPTIONS_BACKGROUND);

    // Заголовки панелей (индексы 0..6).
    for item in CONFIG_ITEMS.iter().take(7) {
        draw_panel_title(frame, palette, item);
    }

    // Сложность ИИ: значения + описание.
    for row in 0..6 {
        let item = CONFIG_ITEMS[7 + row];
        draw_caption(frame, palette, item.rect, item.text, BROWN, true);

        if form.setup.opponent as usize == row {
            draw_selector(frame, palette, item.rect);
        }
    }
    draw_wrapped(
        frame,
        palette,
        DIFFICULTY_DESCRIPTION,
        DIFFICULTY_DESCRIPTIONS[(form.setup.opponent as usize).min(5)],
        YELLOW,
    );

    // Таймеры: заголовки (жёлтые) + строки (коричневые) + выбор.
    draw_caption(
        frame,
        palette,
        CONFIG_ITEMS[13].rect,
        CONFIG_ITEMS[13].text,
        YELLOW,
        true,
    );
    draw_caption(
        frame,
        palette,
        CONFIG_ITEMS[14].rect,
        CONFIG_ITEMS[14].text,
        YELLOW,
        true,
    );

    for index in 0..7 {
        let item = CONFIG_ITEMS[15 + index];
        draw_caption(frame, palette, item.rect, item.text, BROWN, true);
        if form.timer_index() == index {
            draw_selector(frame, palette, item.rect);
        }

        let item = CONFIG_ITEMS[22 + index];
        draw_caption(frame, palette, item.rect, item.text, BROWN, true);
        if form.endturn_index() == index {
            draw_selector(frame, palette, item.rect);
        }
    }

    // Режим игры.
    let play_items = [CONFIG_ITEMS[29], CONFIG_ITEMS[30]];
    for (index, item) in play_items.iter().enumerate() {
        draw_caption(frame, palette, item.rect, item.text, BROWN, true);
        let selected = match form.setup.play_mode {
            PlayMode::TurnBased => index == 0,
            PlayMode::SimultaneousMoves => index == 1,
            PlayMode::Unknown => false,
        };
        if selected {
            draw_selector(frame, palette, item.rect);
        }
    }

    // Стартовые кредиты.
    for index in 0..6 {
        let item = CONFIG_ITEMS[31 + index];
        draw_caption(frame, palette, item.rect, item.text, YELLOW, true);
        let item = CONFIG_ITEMS[37 + index];
        draw_caption(frame, palette, item.rect, item.text, BROWN, true);
        if form.credit_index() == index {
            draw_selector(frame, palette, item.rect);
        }
    }

    // Уровни ресурсов.
    for (caption, index) in [(43usize, 0usize), (44, 1), (45, 2), (46, 3)] {
        let item = CONFIG_ITEMS[caption];
        draw_caption(frame, palette, item.rect, item.text, YELLOW, false);

        let start = [47usize, 50, 53, 56][index];
        let selected = match index {
            0 => form.setup.raw,
            1 => form.setup.fuel,
            2 => form.setup.gold,
            _ => form.setup.derelicts,
        } as usize;

        for column in 0..3 {
            let item = CONFIG_ITEMS[start + column];
            draw_caption(frame, palette, item.rect, item.text, BROWN, true);
            if selected == column {
                draw_selector(frame, palette, item.rect);
            }
        }
    }

    // Условие победы.
    draw_caption(
        frame,
        palette,
        CONFIG_ITEMS[59].rect,
        CONFIG_ITEMS[59].text,
        YELLOW,
        true,
    );
    draw_caption(
        frame,
        palette,
        CONFIG_ITEMS[60].rect,
        CONFIG_ITEMS[60].text,
        YELLOW,
        true,
    );

    for index in 0..3 {
        let item = CONFIG_ITEMS[61 + index];
        draw_caption(frame, palette, item.rect, item.text, BROWN, true);
        if form.setup.victory_type == VictoryType::Duration && form.duration_index() == index {
            draw_selector(frame, palette, item.rect);
        }

        let item = CONFIG_ITEMS[64 + index];
        draw_caption(frame, palette, item.rect, item.text, BROWN, true);
        if form.setup.victory_type == VictoryType::Score && form.score_index() == index {
            draw_selector(frame, palette, item.rect);
        }
    }

    // Числовые поля лимита: 67 — Duration, 68 — Score (Text_TypeWriter_TextBox,
    // цвет `GNW_TEXT_OUTLINE | 0xFF`).
    let limit = if form.editing_victory.is_some() {
        form.victory_text.clone()
    } else {
        form.setup.victory_limit.to_string()
    };

    for (index, active) in [
        (67usize, form.setup.victory_type == VictoryType::Duration),
        (68, form.setup.victory_type == VictoryType::Score),
    ] {
        if !active {
            continue;
        }
        let item = CONFIG_ITEMS[index];
        draw_caption(frame, palette, item.rect, &limit, SILVER, true);
    }

    draw_caption(
        frame,
        palette,
        CONFIG_ITEMS[69].rect,
        CONFIG_ITEMS[69].text,
        YELLOW,
        true,
    );
    draw_caption(
        frame,
        palette,
        CONFIG_ITEMS[70].rect,
        CONFIG_ITEMS[70].text,
        YELLOW,
        true,
    );

    // Поле имени игрока (расширение Rust-клиента, свободная область).
    frame.fill_rect(NAME_FIELD, Rgba::BLACK);
    let name = if form.editing_name {
        format!("Name: {}_", form.setup.player_name)
    } else {
        format!("Name: {}", form.setup.player_name)
    };
    draw_text_gnw(
        frame,
        NAME_FIELD.ulx + 4,
        NAME_FIELD.uly + 5,
        GnwFontId::Font5.font(),
        &name,
        palette.rgba(GREEN),
    );

    // Нижняя строка кнопок: Cancel (MNUBTN4U), ? (MNUBTN5U), Done (MNUBTN6U).
    for (rect, label, resource) in [
        (PixelRect::new(354, 438, 109, 40), "Cancel", 4usize),
        (PixelRect::new(465, 438, 48, 40), "?", 5),
        (PixelRect::new(514, 438, 109, 40), "Done", 6),
    ] {
        let Some(pair) = menu.mnubtn_pair(resource) else {
            continue;
        };
        let hovered = mouse.is_some_and(|(x, y)| rect.contains(x, y));
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            rect,
            label,
            hovered,
        );
    }

    draw_status(frame, palette, status, PixelRect::new(16, 425, 620, 12));
    painted
}

/// Экран `PlanetSelectMenu` (1:1): 3×2 мини-карты, стрелки, имя и описание.
fn draw_planet(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    form: &GameSetupForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_background(frame, menu, "PLANETSE");
    let font = GnwFontId::Font5.font();

    // Заголовок экрана и имя выбранной планеты.
    draw_text_gnw_centered(
        frame,
        PLANET_TITLE,
        font,
        "Planet Select Menu",
        palette.rgba(GREEN),
    );
    draw_text_gnw_centered(
        frame,
        PLANET_NAME,
        font,
        PLANET_NAMES[form.world()],
        palette.rgba(GREEN),
    );

    // Большая картинка планеты (SNOW_PIC + world/6, origin 26,98).
    if let Some(Some(picture)) = menu.planet_group_pictures.get(form.world() / 6) {
        painted += draw_big_image_at(frame, picture, palette, PLANET_PICTURE);
    }

    // Шесть слотов: мини-карты планет текущей группы.
    let first = form.world_group_first();
    for (slot, rect) in PLANET_SLOTS.iter().enumerate() {
        let world = first + slot;

        if world < PLANET_NAMES.len() {
            if let Some(image) = form.minimaps.get(world).and_then(|entry| entry.as_ref()) {
                painted += blit_minimap(frame, *rect, image);
            } else {
                // `.WRL` нет — честная заглушка вместо мини-карты.
                frame.fill_rect(*rect, palette.rgba(0x14));
                draw_text_gnw_centered(
                    frame,
                    PixelRect::new(rect.ulx, rect.uly + 48, rect.width, 16),
                    font,
                    "no map data",
                    palette.rgba(TEXT),
                );
            }
        }

        if world == form.world() {
            // `DrawTexts`: красная рамка в 2 пикселя вокруг выбранного слота.
            draw_box(frame, *rect, Rgba::rgb(252, 0, 0), 2);
        }
    }

    // Стрелки групп.
    if let Some((left, right)) = menu.planet_arrows.as_ref() {
        painted += blit_simple(
            frame,
            palette,
            left,
            PLANET_ARROW_LEFT.ulx,
            PLANET_ARROW_LEFT.uly,
            true,
        );
        painted += blit_simple(
            frame,
            palette,
            right,
            PLANET_ARROW_RIGHT.ulx,
            PLANET_ARROW_RIGHT.uly,
            true,
        );
    }

    // Описание планеты: многострочный текст, зелёный.
    let description = PLANET_DESCRIPTIONS[form.world()];
    let mut y = PLANET_DESCRIPTION.uly + 2;

    for line in wrap_text(description, PLANET_DESCRIPTION.width) {
        if y + font.height() > PLANET_DESCRIPTION.uly + PLANET_DESCRIPTION.height {
            break;
        }
        draw_text_gnw(
            frame,
            PLANET_DESCRIPTION.ulx,
            y,
            font,
            &line,
            palette.rgba(GREEN),
        );
        y += font.height() + 1;
    }

    // Кнопки Random/Cancel/?/Done.
    for (rect, label, resource) in PLANET_BUTTONS {
        let Some(pair) = menu.mnubtn_pair(resource) else {
            continue;
        };
        let hovered = mouse.is_some_and(|(x, y)| rect.contains(x, y));
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            rect,
            label,
            hovered,
        );
    }

    draw_status(frame, palette, status, PixelRect::new(41, 414, 400, 12));
    painted
}

/// Экран `ChoosePlayerMenu` (1:1): 4 команды × 3 типа + флаги команд.
fn draw_players(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    form: &GameSetupForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_background(frame, menu, PLAYERS_BACKGROUND);
    let font = GnwFontId::Font5.font();

    draw_text_gnw_centered(
        frame,
        PLAYERS_TITLE,
        font,
        form.players_title(),
        palette.rgba(GREEN),
    );

    for (rect, label) in PLAYER_HEADERS {
        draw_text_gnw_centered(frame, rect, font, label, palette.rgba(GREEN));
    }

    for row in 0..LOCAL_TEAM_COUNT {
        let kind = form.setup.team_types[row];

        for column in 0..3 {
            let rect = player_type_button_rect(row, column);
            let selected = match column {
                0 => kind == TeamKind::Player,
                1 => kind == TeamKind::Computer,
                _ => kind == TeamKind::None,
            };
            let Some(pair) = menu.team_kind_buttons[column].as_ref() else {
                continue;
            };
            let image = if selected {
                pair.image(true)
            } else {
                pair.image(false)
            };
            painted += blit_simple(frame, palette, image, rect.ulx, rect.uly, true);
        }

        // Флаг команды (`CH_TM1_U`…): клик — выбор клана.
        let rect = player_banner_rect(row);
        if let Some(pair) = menu.team_banners[row].as_ref() {
            painted += blit_simple(
                frame,
                palette,
                pair.image(false),
                rect.ulx,
                rect.uly,
                true,
            );
        }

        // Эмблема выбранного клана на флаге команды: C++ `Copy(clan_logo_id,
        // 41, 40)` (`CLN0LOGO + clan`). Точка (41,40) — якорь картинки, поэтому
        // её собственные `ulx/uly` вычитаются (`WindowManager_DecodeSimpleImage`).
        let clan_index = form.setup.team_clans[row] as usize;
        let logo = menu
            .clan_logos
            .get(clan_index)
            .and_then(Option::as_ref)
            .or(menu.clan_logo.as_ref());
        if let Some(logo) = logo {
            painted += blit_simple_anchored(
                frame,
                palette,
                logo,
                rect.ulx + 41,
                rect.uly + 40,
                true,
            );
        }
    }

    for (rect, label, resource) in PLAYER_BUTTONS {
        let Some(pair) = menu.mnubtn_pair(resource) else {
            continue;
        };
        let hovered = mouse.is_some_and(|(x, y)| rect.contains(x, y));
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            rect,
            label,
            hovered,
        );
    }

    draw_status(frame, palette, status, PixelRect::new(16, 424, 330, 12));
    painted
}

/// Экран `ClanSelectMenu` (`clanselectmenu.cpp`): 8 кланов, логотипы на
/// карточках, имена, Random/Cancel/?/Done.
///
/// Полные описания кланов и таблица бонусов из `SC_CLANS`/`clans.json` в
/// Rust-клиент ещё не загружаются, поэтому здесь показываются имя выбранного
/// клана и подсказка; сама механика (выбор + применение к `team_clans`)
/// реализована и влияет на стартовые атрибуты юнитов.
fn draw_clan(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    form: &GameSetupForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = match menu.clansel.as_ref() {
        Some(background) => draw_big_image(frame, background),
        None => draw_background(frame, menu, PLAYERS_BACKGROUND),
    };
    let font = GnwFontId::Font5.font();

    draw_text_gnw_centered(
        frame,
        CLAN_TITLE,
        font,
        "Clan Selection",
        palette.rgba(GREEN),
    );

    for index in 0..CLAN_NAMES.len() {
        let rect = CLAN_SLOTS[index];
        let selected = form.clan_selection as usize == index + 1;

        if let Some(pair) = menu.clan_buttons[index].as_ref() {
            painted += blit_simple(
                frame,
                palette,
                pair.image(selected),
                rect.ulx,
                rect.uly,
                true,
            );
        }
        if let Some(logo) = menu.clan_logos[index + 1].as_ref() {
            // `ClanSelectMenu::ButtonInit`: `Copy(CLN1LOGO + index, 41, 40)` —
            // якорь (41,40) внутри карточки 91×86.
            painted += blit_simple_anchored(
                frame,
                palette,
                logo,
                rect.ulx + 41,
                rect.uly + 40,
                true,
            );
        }

        // Наведение: подсветка рамкой (в C++ hover не рисуется, но клик по
        // карточке — основной способ выбора; подсветка не двигает логотип).
        if mouse.is_some_and(|(x, y)| rect.contains(x, y)) && !selected {
            draw_box(frame, rect, Rgba::rgb(252, 252, 252), 1);
        }

        let color = if selected { GREEN } else { TEXT };
        draw_text_gnw_centered(
            frame,
            CLAN_NAME_RECTS[index],
            font,
            CLAN_NAMES[index],
            palette.rgba(color),
        );
    }

    let team = form.clan_team.min(LOCAL_TEAM_COUNT - 1);
    let line = format!(
        "{}: {}",
        crate::hotseat::team_display_name(max_core::team::TeamId(team as u8)),
        clan_name(form.clan_selection)
    );
    draw_text_gnw_shadowed(
        frame,
        41,
        350,
        font,
        &line,
        palette.rgba(GREEN),
        palette.rgba(0xA7),
        palette.rgba(0xE6),
    );
    draw_text_gnw_shadowed(
        frame,
        41,
        370,
        font,
        "Unit bonuses are applied to this team when the game starts.",
        palette.rgba(TEXT),
        palette.rgba(0xA7),
        palette.rgba(0xE6),
    );

    for (rect, label, resource) in CLAN_BUTTONS {
        let Some(pair) = menu.mnubtn_pair(resource) else {
            continue;
        };
        let hovered = mouse.is_some_and(|(x, y)| rect.contains(x, y));
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            rect,
            label,
            hovered,
        );
    }

    draw_status(frame, palette, status, PixelRect::new(16, 424, 560, 12));
    painted
}

/// Строка статуса зелёным с тенью (`Text_TextLine`, `Fonts_BrightSilverColor`-стиль).
fn draw_status(
    frame: &mut RgbaBuffer,
    palette: &crate::panels::UiPalette,
    status: &str,
    rect: PixelRect,
) {
    if status.is_empty() {
        return;
    }
    let status = clean_status(status);
    let text = truncate_to_width(&status, rect.width, GnwFontId::Font5.font());

    draw_text_gnw_shadowed(
        frame,
        rect.ulx,
        rect.uly,
        GnwFontId::Font5.font(),
        &text,
        palette.rgba(GREEN),
        palette.rgba(0xA7),
        palette.rgba(0xE6),
    );
}

/// Рамка `thickness` внутри прямоугольника.
fn draw_box(frame: &mut RgbaBuffer, rect: PixelRect, color: Rgba, thickness: i32) {
    for inset in 0..thickness.max(1) {
        let x = rect.ulx + inset;
        let y = rect.uly + inset;
        let width = rect.width - inset * 2;
        let height = rect.height - inset * 2;

        if width <= 0 || height <= 0 {
            break;
        }

        for dx in 0..width {
            frame.set(x + dx, y, color);
            frame.set(x + dx, y + height - 1, color);
        }
        for dy in 0..height {
            frame.set(x, y + dy, color);
            frame.set(x + width - 1, y + dy, color);
        }
    }
}

/// Рисует big image в точку `origin` палитрой окна.
fn draw_big_image_at(
    frame: &mut RgbaBuffer,
    image: &max_assets::image::BigImage,
    palette: &crate::panels::UiPalette,
    origin: (i32, i32),
) -> u32 {
    let mut painted = 0;

    for y in 0..image.height as u32 {
        for x in 0..image.width as u32 {
            if let Some(index) = image.pixel(x, y) {
                if frame.set(
                    origin.0 + x as i32,
                    origin.1 + y as i32,
                    palette.rgba(index),
                ) {
                    painted += 1;
                }
            }
        }
    }

    painted
}

/// Копирует 112×112 мини-карту (RGBA) в слот.
fn blit_minimap(frame: &mut RgbaBuffer, rect: PixelRect, rgba: &[u8]) -> u32 {
    let size = 112usize;
    let mut painted = 0;

    for y in 0..size {
        for x in 0..size {
            let base = (y * size + x) * 4;
            let color =
                Rgba::from_rgba([rgba[base], rgba[base + 1], rgba[base + 2], rgba[base + 3]]);

            if frame.set(rect.ulx + x as i32, rect.uly + y as i32, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// Разбивает текст по ширине (по словам) — `Text_SplitText`
/// (общий с окном перехода хода, см. [`crate::menu::wrap_gnw_text`]).
fn wrap_text(text: &str, width: i32) -> Vec<String> {
    wrap_gnw_text(text, width)
}

/// Загружает мини-карту планеты из `.WRL` так же, как
/// `Menu_LoadPlanetMinimap` (`menu.cpp:663`).
///
/// Возвращает 112×112 RGBA: палитра `.WRL` делится на 4 (`palette[i] /= 4`),
/// затем каждый цвет отображается в палитру окна (`Color_MapColor(..., true)`).
pub fn load_minimap(
    world_dir: &Path,
    world: usize,
    palette: &crate::panels::UiPalette,
) -> Option<Vec<u8>> {
    let name = WORLD_ASSETS.get(world)?;
    let path = world_dir.join(format!("{name}.WRL"));

    if !path.is_file() {
        return None;
    }

    let map = max_assets::wrl::WrlMap::open(&path).ok()?;
    let minimap = map.minimap();
    let colors = map.palette();
    let width = usize::try_from(map.width()).ok()?;
    let height = usize::try_from(map.height()).ok()?;

    if minimap.len() < width * height {
        return None;
    }

    let mut out = vec![0u8; 112 * 112 * 4];

    for y in 0..112usize {
        for x in 0..112usize {
            let source_x = x * width / 112;
            let source_y = y * height / 112;
            let index = minimap[source_y * width + source_x] as usize;
            let rgb = [
                colors[index * 3] / 4,
                colors[index * 3 + 1] / 4,
                colors[index * 3 + 2] / 4,
            ];
            let rgba = palette.map_color(rgb).to_rgba();
            let base = (y * 112 + x) * 4;
            out[base..base + 4].copy_from_slice(&rgba);
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    use max_core::team::TeamId;

    #[test]
    fn config_table_matches_the_cpp_oracle() {
        assert_eq!(CONFIG_ITEMS.len(), 71);
        assert_eq!(CONFIG_ITEMS[0].text, "Options Menu");
        assert_eq!(CONFIG_ITEMS[13].rect, PixelRect::new(227, 85, 80, 20));
        assert_eq!(CONFIG_ITEMS[29].text, "Turn Based");
        assert_eq!(CONFIG_ITEMS[30].text, "Simultaneous Moves");
        assert_eq!(CONFIG_ITEMS[70].text, "points");
    }

    #[test]
    fn planet_slots_match_the_cpp_controls() {
        assert_eq!(PLANET_SLOTS[0], PixelRect::new(192, 51, 112, 112));
        assert_eq!(PLANET_SLOTS[5], PixelRect::new(469, 189, 112, 112));
        assert_eq!(PLANET_ARROW_LEFT, PixelRect::new(60, 248, 30, 22));
        assert_eq!(PLANET_BUTTONS[3].0, PixelRect::new(514, 438, 109, 40));
        assert_eq!(PLANET_NAMES[0], "Snowcrab");
        assert_eq!(PLANET_NAMES[23], "Bottleneck");
    }

    #[test]
    fn player_buttons_match_the_cpp_controls() {
        assert_eq!(
            player_type_button_rect(0, 0),
            PixelRect::new(175, 67, 55, 71)
        );
        assert_eq!(
            player_type_button_rect(3, 2),
            PixelRect::new(394, 343, 55, 71)
        );
        assert_eq!(player_banner_rect(0), PixelRect::new(495, 61, 93, 89));
        assert_eq!(PLAYER_BUTTONS[2].0, PixelRect::new(514, 438, 109, 40));
    }

    #[test]
    fn radio_indices_map_settings_to_cpp_rows() {
        let mut form = GameSetupForm::default();
        assert_eq!(form.timer_index(), 3); // 180 → строка "180"
        assert_eq!(form.endturn_index(), 3); // 45 → строка "45"
        assert_eq!(form.credit_index(), 3); // 150 → "150"

        form.setup.timer = 0;
        assert_eq!(form.timer_index(), 0, "No Limit");
        form.setup.timer = 90;
        assert_eq!(form.timer_index(), 1, "60 seconds");

        form.setup.victory_type = VictoryType::Score;
        form.setup.victory_limit = 400;
        assert_eq!(form.score_index(), 1);
    }

    #[test]
    fn single_player_human_selection_moves_the_flag() {
        let mut form = GameSetupForm::default();
        form.setup.team_types = [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ];

        form.select_team_type(1, TeamKind::Player);

        assert_eq!(form.setup.team_types[1], TeamKind::Player);
        assert_eq!(
            form.setup.team_types[0],
            TeamKind::Computer,
            "прежний человек получает тип нового слота"
        );
        assert_eq!(form.setup.human_team(), Some(TeamId(1)));
    }

    #[test]
    fn clan_selection_screen_applies_to_team() {
        use crate::ui::InputEvent;

        let mut form = GameSetupForm::default();
        form.screen = SetupScreen::Players;
        let banner = player_banner_rect(1);
        form.handle_input(
            InputEvent::MouseClick(banner.ulx + banner.width / 2, banner.uly + banner.height / 2),
            640,
            480,
        );
        assert_eq!(form.screen, SetupScreen::Clan);
        assert_eq!(form.clan_team, 1);

        let card = CLAN_SLOTS[4];
        form.handle_input(
            InputEvent::MouseClick(card.ulx + card.width / 2, card.uly + card.height / 2),
            640,
            480,
        );
        assert_eq!(
            form.clan_selection,
            max_core::enums::TeamClan::Musashi,
            "карточка 5 — Musashi"
        );

        let done = CLAN_BUTTONS[3].0;
        form.handle_input(
            InputEvent::MouseClick(done.ulx + done.width / 2, done.uly + done.height / 2),
            640,
            480,
        );

        assert_eq!(form.screen, SetupScreen::Players);
        assert_eq!(
            form.setup.team_clans[1],
            max_core::enums::TeamClan::Musashi,
            "клан применён к команде GREEN"
        );
        assert_eq!(
            form.setup.team_clans[0],
            max_core::enums::TeamClan::Random,
            "другие команды не тронуты"
        );
        assert_eq!(
            form.setup.new_game_config().clans[1],
            max_core::enums::TeamClan::Musashi,
            "кланы уходят в NewGameConfig"
        );
    }

    #[test]
    fn clan_cancel_and_random_are_mechanics() {
        use crate::ui::InputEvent;

        let mut form = GameSetupForm::default();
        form.setup.team_clans[0] = max_core::enums::TeamClan::TheChosen;
        form.screen = SetupScreen::Players;
        let banner = player_banner_rect(0);
        form.handle_input(
            InputEvent::MouseClick(banner.ulx + banner.width / 2, banner.uly + banner.height / 2),
            640,
            480,
        );
        let card = CLAN_SLOTS[7];
        form.handle_input(
            InputEvent::MouseClick(card.ulx + card.width / 2, card.uly + card.height / 2),
            640,
            480,
        );
        let cancel = CLAN_BUTTONS[1].0;
        form.handle_input(
            InputEvent::MouseClick(cancel.ulx + cancel.width / 2, cancel.uly + cancel.height / 2),
            640,
            480,
        );
        assert_eq!(
            form.setup.team_clans[0],
            max_core::enums::TeamClan::TheChosen,
            "Cancel не меняет клан"
        );

        let banner = player_banner_rect(0);
        form.handle_input(
            InputEvent::MouseClick(banner.ulx + banner.width / 2, banner.uly + banner.height / 2),
            640,
            480,
        );
        let random = CLAN_BUTTONS[0].0;
        form.handle_input(
            InputEvent::MouseClick(random.ulx + random.width / 2, random.uly + random.height / 2),
            640,
            480,
        );
        assert_eq!(
            form.setup.team_clans[0],
            max_core::enums::TeamClan::Random,
            "Random сбрасывает клан"
        );
    }
}
