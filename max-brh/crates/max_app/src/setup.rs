//! Local match setup screen (M-SP) — 1:1 with the C++ port screens.
//!
//! Three screens, exactly in the order of `menu_custom_game_menu` (`menu.cpp:2115`):
//!
//! | # | Screen | Oracle |
//! |---|---|---|
//! | 1 | `Options` — `GameConfigMenu` (`OPTNFRM` background) | `gameconfigmenu.cpp/.hpp` |
//! | 2 | `Planet` — `PlanetSelectMenu` (`PLANETSE` background) | `planetselectmenu.cpp/.hpp` |
//! | 3 | `Players` — `ChoosePlayerMenu` (`CHOSPLYR` background) | `chooseplayermenu.cpp/.hpp` |
//!
//! The layout (coordinates, button pictures, texts) is taken from the C++ tables
//! `game_config_menu_items` / `game_config_menu_controls`,
//! `planet_select_menu_controls` and `choose_player_menu_controls`; the English
//! strings — from `assets/language.json` (`_(hash)` → `en-US`).
//!
//! # Difference from the original
//!
//! The **Name** field (player name) in C++ is set in Preferences (`PREFSPIC`), not in
//! the match setup. In the Rust client it is placed in the free space of the bottom
//! row of the `Options` screen (to the left of the `Cancel`/`?`/`Done` buttons): playing
//! locally without a name is impossible, and the Preferences screen is not ported yet. This
//! is the only addition to the original layout; the panels and buttons themselves are
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

/// Text color (in the window palette) — `COLOR_GREEN`.
pub const GREEN: u8 = 0x02;
/// `TEXT` — the regular menu text color.
pub const TEXT: u8 = 0xA2;
/// `Fonts_BrightBrownColor` (`fonts.cpp`).
pub const BROWN: (u8, u8, u8) = (164, 173, 199);
/// `Fonts_BrightYellowColor` (`fonts.cpp`).
pub const YELLOW: (u8, u8, u8) = (162, 8, 199);
/// `Fonts_BrightSilverColor` (`fonts.cpp`).
pub const SILVER: (u8, u8, u8) = (162, 172, 199);

// ---------------------------------------------------------------------------
// GameConfigMenu: panels and titles
// ---------------------------------------------------------------------------

/// Options screen background — `OPTNFRM`.
pub const OPTIONS_BACKGROUND: &str = "OPTNFRM";
/// Team selection background — `CHOSPLYR`.
pub const PLAYERS_BACKGROUND: &str = "CHOSPLYR";

/// Six `bg_panels` panels (`gameconfigmenu.cpp:93`): 200×190.
pub const CONFIG_PANELS: [PixelRect; 6] = [
    PixelRect::new(6, 38, 200, 190),
    PixelRect::new(216, 38, 200, 190),
    PixelRect::new(426, 38, 200, 190),
    PixelRect::new(6, 233, 200, 190),
    PixelRect::new(216, 233, 200, 190),
    PixelRect::new(426, 233, 200, 190),
];

/// One `game_config_menu_items[i]` title (`gameconfigmenu.hpp:61`).
#[derive(Debug, Clone, Copy)]
pub struct ConfigItem {
    /// `bounds` rectangle.
    pub rect: PixelRect,
    /// Text (English `en-US`).
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

/// 71 options screen titles — a verbatim transfer of the
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

/// Difficulty descriptions — `game_config_menu_difficulty_descriptions`
/// (`gameconfigmenu.cpp:155`), drawn at `(19,170)` 180×60.
pub const DIFFICULTY_DESCRIPTIONS: [&str; 6] = [
    "Very easy opponent with no advanced tactics or strategies, and a 25% penalty to building.",
    "Basic opponent with some advanced tactics, and no building penalty.",
    "Fairly clever opponent with some advanced strategies and nastier tactics.",
    "Smartest opponent available, with no advantages.",
    "Smartest opponent with a small (25%) bonus to production.",
    "Merciless opponent with a 50% bonus to production.",
];

/// Difficulty description: rectangle.
pub const DIFFICULTY_DESCRIPTION: PixelRect = PixelRect::new(19, 170, 180, 60);

/// Turn timer values (`game_config_menu_controls[6..13]`, rows 15..21).
pub const TIMER_VALUES: [u32; 7] = [0, 60, 120, 180, 240, 300, 360];
/// End-of-turn timer values (rows 22..28).
pub const ENDTURN_VALUES: [u32; 7] = [0, 15, 30, 45, 60, 75, 90];
/// Starting credits (rows 37..42).
pub const CREDIT_VALUES: [u32; 6] = [0, 50, 100, 150, 200, 250];
/// Victory condition by turns (rows 61..63) → (type, limit).
pub const VICTORY_DURATION_VALUES: [(VictoryType, i32); 3] = [
    (VictoryType::Duration, 100),
    (VictoryType::Duration, 200),
    (VictoryType::Duration, 400),
];
/// Victory condition by score (rows 64..66) → (type, limit).
pub const VICTORY_SCORE_VALUES: [(VictoryType, i32); 3] = [
    (VictoryType::Score, 200),
    (VictoryType::Score, 400),
    (VictoryType::Score, 800),
];

/// Player name field — the free space of the `OPTNFRM` bottom row.
pub const NAME_FIELD: PixelRect = PixelRect::new(16, 444, 320, 22);

// ---------------------------------------------------------------------------
// PlanetSelectMenu
// ---------------------------------------------------------------------------

/// Screen title (`planet_select_menu_screen_title`).
pub const PLANET_TITLE: PixelRect = PixelRect::new(230, 6, 180, 20);
/// Selected planet name (`planet_select_menu_planet_name`).
pub const PLANET_NAME: PixelRect = PixelRect::new(16, 61, 150, 20);
/// Planet description (`planet_select_menu_planet_description`).
pub const PLANET_DESCRIPTION: PixelRect = PixelRect::new(41, 350, 561, 60);
/// Big planet picture — `WindowManager_LoadBigImage(SNOW_PIC + world/6)`,
/// for `SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC` the origin is (26, 98).
pub const PLANET_PICTURE: (i32, i32) = (26, 98);
/// Planet group switch arrows (`MNULAROU`/`MNURAROU`), (60,248)/(92,248).
pub const PLANET_ARROW_LEFT: PixelRect = PixelRect::new(60, 248, 30, 22);
/// Right arrow.
pub const PLANET_ARROW_RIGHT: PixelRect = PixelRect::new(92, 248, 30, 22);
/// Six minimap slots (`planet_select_menu_controls[0..6]`, 112×112).
pub const PLANET_SLOTS: [PixelRect; 6] = [
    PixelRect::new(192, 51, 112, 112),
    PixelRect::new(330, 51, 112, 112),
    PixelRect::new(469, 51, 112, 112),
    PixelRect::new(192, 189, 112, 112),
    PixelRect::new(330, 189, 112, 112),
    PixelRect::new(469, 189, 112, 112),
];

/// Bottom row buttons of the planet screen (`planet_select_menu_controls[8..12]`).
pub const PLANET_BUTTONS: [(PixelRect, &str, usize); 4] = [
    (PixelRect::new(243, 438, 109, 40), "Random", 4),
    (PixelRect::new(354, 438, 109, 40), "Cancel", 4),
    (PixelRect::new(465, 438, 48, 40), "?", 5),
    (PixelRect::new(514, 438, 109, 40), "Done", 6),
];

/// Names of the 24 planets — `menu_planet_names` (`planetselectmenu.cpp:120`).
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

/// Descriptions of the 24 planets — `menu_planet_descriptions` (`planetselectmenu.cpp:116`).
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

/// Team selection screen title (`choose_player_menu_titles[0]`).
pub const PLAYERS_TITLE: PixelRect = PixelRect::new(230, 6, 180, 20);

/// Column titles `choose_player_menu_titles[1..6]`.
pub const PLAYER_HEADERS: [(PixelRect, &str); 5] = [
    (PixelRect::new(61, 31, 80, 20), "Team"),
    (PixelRect::new(162, 31, 79, 20), "Human"),
    (PixelRect::new(272, 31, 79, 20), "Computer"),
    (PixelRect::new(380, 31, 79, 20), "None"),
    (PixelRect::new(497, 31, 79, 20), "Clan"),
];

/// Team type selection buttons: (x, y) — `choose_player_menu_controls[0..12]`,
/// `y` of rows 67/159/251/343.
pub fn player_type_button_rect(row: usize, column: usize) -> PixelRect {
    let x = [175, 285, 394][column.min(2)];
    let y = 67 + 92 * row as i32;

    PixelRect::new(x, y, 55, 71)
}

/// Team flag rectangles (`CH_TM1_U`…`CH_TM4_U`).
pub fn player_banner_rect(row: usize) -> PixelRect {
    PixelRect::new(495, 61 + 92 * row as i32, 93, 89)
}

/// Bottom row buttons of the team screen (`choose_player_menu_controls[16..19]`).
pub const PLAYER_BUTTONS: [(PixelRect, &str, usize); 3] = [
    (PixelRect::new(354, 438, 109, 40), "Cancel", 4),
    (PixelRect::new(465, 438, 48, 40), "?", 5),
    (PixelRect::new(514, 438, 109, 40), "Done", 6),
];

// ---------------------------------------------------------------------------
// ClanSelectMenu
// ---------------------------------------------------------------------------

/// Clan selection screen background (`CLANSEL`).
pub const CLAN_BACKGROUND: &str = "CLANSEL";
/// Title `clan_select_menu_screen_title`.
pub const CLAN_TITLE: PixelRect = PixelRect::new(230, 6, 180, 20);
/// Eight `CH_CN1_U`…`CH_CN8_U` cards 91×86
/// (`clan_select_menu_controls[0..8]`; `lrx/lry` in C++ are zero, the size is taken
/// from the picture — 91×86).
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
/// Clan labels under the cards (`clan_select_menu_clan_icons`).
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
/// Clan names in `TeamClan` order (1 — The Chosen … 8 — Axis Inc.).
pub const CLAN_NAMES: [&str; 8] = [
    "The Chosen",
    "Crimson Path",
    "Von Griffin",
    "Ayers Hand",
    "Musashi",
    "Sacred Eight",
    "Seven Knights",
    "Axis Inc.",
];
/// `ClanSelectMenu` bottom row (`clan_select_menu_controls[8..12]`).
pub const CLAN_BUTTONS: [(PixelRect, &str, usize); 4] = [
    (PixelRect::new(243, 438, 109, 40), "Random", 3),
    (PixelRect::new(354, 438, 109, 40), "Cancel", 4),
    (PixelRect::new(465, 438, 48, 40), "?", 5),
    (PixelRect::new(514, 438, 109, 40), "Done", 6),
];

/// `ChoosePlayerMenu::EventDone` messages.
///
/// `_(bc94)` — two participants are required; `_(06ec)` — hot-seat requires two humans.
pub const MSG_TWO_PLAYERS: &str = crate::hotseat::MSG_TWO_PLAYERS;
/// Hot-seat message: at least two humans are required (`_(06ec)`).
pub const MSG_HOT_SEAT_TWO_HUMANS: &str = MSG_TWO_HUMANS;

/// Setup screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupScreen {
    /// `GameConfigMenu`.
    Options,
    /// `PlanetSelectMenu`.
    Planet,
    /// `ChoosePlayerMenu`.
    Players,
    /// `ClanSelectMenu` — team clan selection.
    Clan,
}

/// Clan by card index `0..8` (`CLN1`…`CLN8`).
pub fn clan_from_index(index: usize) -> max_core::enums::TeamClan {
    max_core::enums::TeamClan::try_from((index + 1).min(8) as u8)
        .unwrap_or(max_core::enums::TeamClan::Random)
}

/// Clan name for the label (`Random` — «No clan chosen»).
pub fn clan_name(clan: max_core::enums::TeamClan) -> &'static str {
    let index = clan as usize;
    if index == 0 {
        "Random"
    } else {
        CLAN_NAMES[(index - 1).min(CLAN_NAMES.len() - 1)]
    }
}

/// Hot-seat initial team types (`menu_choose_player_menu_loop`, `menu.cpp:1883`):
/// `menu_custom_game_menu(false)` sets RED and GREEN as humans, BLUE/GRAY — as empty.
pub fn hot_seat_default_teams() -> [TeamKind; LOCAL_TEAM_COUNT] {
    [
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]
}

/// Transition from the setup form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupTransition {
    /// Just redraw.
    None,
    /// Back to the menu/to the previous screen.
    Back,
    /// Open the help window (`HelpMenu`) over the setup screen.
    Help(crate::helpmenu::HelpSection),
    /// Start the match with these settings.
    Start(Box<LocalSetup>),
}

/// Setup form state (all three screens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSetupForm {
    /// Current screen.
    pub screen: SetupScreen,
    /// Match settings (timer, planet, team types…).
    pub setup: LocalSetup,
    /// M-HS: the form is opened by the `Hot Seat Game` item (`is_single_player = false`).
    pub hot_seat: bool,
    /// Name editing is in progress.
    pub editing_name: bool,
    /// The victory condition field is being edited (which type: `Duration`/`Score`).
    pub editing_victory: Option<VictoryType>,
    /// Victory condition edit buffer.
    pub victory_text: String,
    /// Status/error line.
    pub status: String,
    /// Planet minimap cache (112×112 RGBA).
    pub minimaps: Vec<Option<Vec<u8>>>,
    /// Where to load `.WRL` from (game data directory).
    pub world_dir: PathBuf,
    /// The team for which `ClanSelectMenu` is open.
    pub clan_team: usize,
    /// The clan chosen in `ClanSelectMenu` (applied on Done/Random).
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
    /// Form with the data directory (for planet minimaps).
    pub fn with_world_dir(world_dir: impl Into<PathBuf>) -> Self {
        Self {
            world_dir: world_dir.into(),
            ..Self::default()
        }
    }

    /// **Hot-seat** form (`menu_custom_game_menu(false)`).
    ///
    /// Initial values of `menu_choose_player_menu_loop` (`menu.cpp:1883`):
    /// RED — human, GREEN — human, BLUE/GRAY — empty.
    pub fn hot_seat() -> Self {
        let mut form = Self {
            hot_seat: true,
            ..Self::default()
        };

        form.setup.hot_seat = true;
        form.setup.team_types = hot_seat_default_teams();
        form
    }

    /// Which mode's screens are configured (`ChoosePlayerMenu::is_single_player`).
    pub fn is_single_player(&self) -> bool {
        !self.hot_seat
    }

    /// Team screen title: `_(5512)` / `_(55c8)`.
    pub fn players_title(&self) -> &'static str {
        if self.hot_seat {
            TITLE_HOT_SEAT
        } else {
            TITLE_SINGLE
        }
    }

    /// Selected planet index (`0..23`).
    pub fn world(&self) -> usize {
        self.setup.world.clamp(0, 23) as usize
    }

    /// First planet of the current group (`(world / 6) * 6`).
    pub fn world_group_first(&self) -> usize {
        (self.world() / 6) * 6
    }

    /// Index of the selected turn timer row (`timer` → row).
    pub fn timer_index(&self) -> usize {
        TIMER_VALUES
            .iter()
            .rposition(|value| self.setup.timer >= *value)
            .unwrap_or(0)
    }

    /// End-of-turn timer row index.
    pub fn endturn_index(&self) -> usize {
        ENDTURN_VALUES
            .iter()
            .rposition(|value| self.setup.endturn >= *value)
            .unwrap_or(0)
    }

    /// Starting credits row index.
    pub fn credit_index(&self) -> usize {
        CREDIT_VALUES
            .iter()
            .rposition(|value| self.setup.start_gold >= *value)
            .unwrap_or(0)
    }

    /// Victory condition index in the `Duration` group (0..2).
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

    /// Victory condition index in the `Score` group (0..2).
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

    /// Apply the default settings from `--player` and the data directory.
    pub fn with_player_name(mut self, name: impl Into<String>) -> Self {
        self.setup.player_name = name.into();
        self
    }

    /// Sets `team_types` (for tests/re-entry).
    pub fn with_team_types(mut self, team_types: [TeamKind; LOCAL_TEAM_COUNT]) -> Self {
        self.setup.team_types = team_types;
        self
    }

    /// Loads the minimaps of the current planet group (if `.WRL` is on disk).
    ///
    /// The render order repeats `Menu_LoadPlanetMinimap` (`menu.cpp:663`):
    /// the `.WRL` palette is divided by 4, each index is translated into the window palette
    /// (`Color_MapColor` with a full scan).
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

    /// Match status: updates the field and returns the transition.
    fn fail(&mut self, message: &str) -> SetupTransition {
        self.status = message.to_string();
        SetupTransition::None
    }

    /// Input (key/click). Mouse coordinates — the original 640×480 screen.
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
            Key::Tab
            | Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::PageUp
            | Key::PageDown
            | Key::Alt(_) | Key::Other => SetupTransition::None,
        }
    }

    /// Transition to the next screen (Enter without editing).
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

    /// `ClanSelectMenu::EventDone`: save the chosen clan for the team.
    ///
    /// The clan is only recorded here: the menu has no `GameState` yet, so there
    /// are no team complexes and no unit values to rebuild. The C++ initialization
    /// (`ResourceManager_InitClanUnitValues`: `ClearComplexes()` + `TeamUnits::Init()`
    /// + clan trade-offs) runs at match start (`GameState::with_setup` /
    /// `App::new_game_on_surface`).
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

    /// `ChoosePlayerMenu::EventDone` + starting the match.
    fn done(&mut self) -> SetupTransition {
        // Mode — part of the match settings (`MISSION_CATEGORY_HOT_SEAT`).
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
        // Name: a click on the free field — editing.
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

        // Difficulty (`game_config_menu_controls[0..6]`).
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

        // Resource levels: four rows of three values.
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

        // Victory condition: Duration/Score + six variants.
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
        // Limit input fields (rows 67/68): `EventVictoryConditionPrefs`.
        // A click on a field activates the condition type too: 67 — turns, 68 — points
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

        // `Cancel`/`?`/`Done` buttons.
        if Self::inside(PixelRect::new(354, 438, 109, 40), x, y) {
            return SetupTransition::Back;
        }
        if Self::inside(PixelRect::new(465, 438, 48, 40), x, y) {
            // `GameConfigMenu::EventHelp` → `HelpMenu_Menu("OPTIONS_SETUP")`.
            return SetupTransition::Help(crate::helpmenu::HelpSection::Options);
        }
        if Self::inside(PixelRect::new(514, 438, 109, 40), x, y) {
            self.screen = SetupScreen::Planet;
            self.status.clear();
        }

        SetupTransition::None
    }

    /// Text → victory limit (`GameConfigMenu::UpdateTextEdit`):
    /// `victory_type = active_text_edit_index - 46`, `victory_limit` — the number.
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
            // `EventWorld(direction=false)`: group −6, <0 → 18.
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
                    // `EventRandom`: a random planet and immediately Done.
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
                    // `PlanetSelectMenu::EventHelp` → `PLANET_SETUP`.
                    SetupTransition::Help(crate::helpmenu::HelpSection::Planet)
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
                // `ChoosePlayerMenu::EventSelectClan`: open `ClanSelectMenu`
                // for this team, keeping the current choice.
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
                    // `ChoosePlayerMenu::EventHelp`: `NEW_GAME_SETUP` in a single
                    // game and `HOT_SEAT_SETUP` in hot-seat.
                    SetupTransition::Help(if self.hot_seat {
                        crate::helpmenu::HelpSection::HotSeat
                    } else {
                        crate::helpmenu::HelpSection::NewGame
                    })
                }
            };
        }

        SetupTransition::None
    }

    /// `ClanSelectMenu`: eight cards, Random/Cancel/?/Done.
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
                    // `ClanSelectMenu::EventHelp` → `CLAN_SETUP`.
                    SetupTransition::Help(crate::helpmenu::HelpSection::Clan)
                }
            };
        }

        SetupTransition::None
    }

    /// `ChoosePlayerMenu::EventSelectHuman` for a single game: «Human» on
    /// another slot copies the flag, the previous human gets the new slot's type.
    ///
    /// In hot-seat (`is_single_player = false`) there is no copy: humans can be
    /// chosen as many as desired (`chooseplayermenu.cpp:79`).
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
// Drawing
// ---------------------------------------------------------------------------

/// Draws the current setup screen.
///
/// `status` — the UI context line; if the form has set its own line
/// (`GameSetupForm::status`, e.g. "two players required"), it is shown.
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

/// Screen background: `OPTNFRM`/`PLANETSE`/`CHOSPLYR` (fallback — `MAINPIC`).
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

/// Panel title: `GNW_TEXT_OUTLINE | COLOR_GREEN` (green + black shadow).
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

/// `FontColor` label (yellow/brown), centered or left.
fn draw_caption(
    frame: &mut RgbaBuffer,
    palette: &crate::panels::UiPalette,
    rect: PixelRect,
    text: &str,
    color: (u8, u8, u8),
    centered: bool,
) {
    let font = GnwFontId::Font5.font();
    // `Text_TextBox` with `FontColor`: the text is wrapped by words inside
    // the rectangle (`Simultaneous Moves` — two lines in (480,115,100,30)).
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

/// Multiline `FontColor` label in a rectangle (`Text_TextBox`).
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

/// Selection frame (`GameConfigMenu::DrawSelector` — `draw_box` in color 0xFF).
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

/// `GameConfigMenu` screen (1:1).
fn draw_options(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    form: &GameSetupForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_background(frame, menu, OPTIONS_BACKGROUND);

    // Panel titles (indices 0..6).
    for item in CONFIG_ITEMS.iter().take(7) {
        draw_panel_title(frame, palette, item);
    }

    // AI difficulty: values + description.
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

    // Timers: titles (yellow) + rows (brown) + selection.
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

    // Game mode.
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

    // Starting credits.
    for index in 0..6 {
        let item = CONFIG_ITEMS[31 + index];
        draw_caption(frame, palette, item.rect, item.text, YELLOW, true);
        let item = CONFIG_ITEMS[37 + index];
        draw_caption(frame, palette, item.rect, item.text, BROWN, true);
        if form.credit_index() == index {
            draw_selector(frame, palette, item.rect);
        }
    }

    // Resource levels.
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

    // Victory condition.
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

    // Numeric limit fields: 67 — Duration, 68 — Score (Text_TypeWriter_TextBox,
    // color `GNW_TEXT_OUTLINE | 0xFF`).
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

    // Player name field (a Rust client extension, free area).
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

    // Bottom row of buttons: Cancel (MNUBTN4U), ? (MNUBTN5U), Done (MNUBTN6U).
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

/// `PlanetSelectMenu` screen (1:1): 3×2 minimaps, arrows, name and description.
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

    // Screen title and the selected planet name.
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

    // Big planet picture (SNOW_PIC + world/6, origin 26,98).
    if let Some(Some(picture)) = menu.planet_group_pictures.get(form.world() / 6) {
        painted += draw_big_image_at(frame, picture, palette, PLANET_PICTURE);
    }

    // Six slots: minimaps of the current group's planets.
    let first = form.world_group_first();
    for (slot, rect) in PLANET_SLOTS.iter().enumerate() {
        let world = first + slot;

        if world < PLANET_NAMES.len() {
            if let Some(image) = form.minimaps.get(world).and_then(|entry| entry.as_ref()) {
                painted += blit_minimap(frame, *rect, image);
            } else {
                // No `.WRL` — an honest placeholder instead of a minimap.
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
            // `DrawTexts`: a red 2-pixel frame around the selected slot.
            draw_box(frame, *rect, Rgba::rgb(252, 0, 0), 2);
        }
    }

    // Group arrows.
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

    // Planet description: multiline text, green.
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

    // Random/Cancel/?/Done buttons.
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

/// `ChoosePlayerMenu` screen (1:1): 4 teams × 3 types + team flags.
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

        // Team flag (`CH_TM1_U`…): a click — clan selection.
        let rect = player_banner_rect(row);
        if let Some(pair) = menu.team_banners[row].as_ref() {
            painted += blit_simple(frame, palette, pair.image(false), rect.ulx, rect.uly, true);
        }

        // Emblem of the selected clan on the team flag: C++ `Copy(clan_logo_id,
        // 41, 40)` (`CLN0LOGO + clan`). The point (41,40) is the picture anchor, so
        // its own `ulx/uly` are subtracted (`WindowManager_DecodeSimpleImage`).
        let clan_index = form.setup.team_clans[row] as usize;
        let logo = menu
            .clan_logos
            .get(clan_index)
            .and_then(Option::as_ref)
            .or(menu.clan_logo.as_ref());
        if let Some(logo) = logo {
            painted +=
                blit_simple_anchored(frame, palette, logo, rect.ulx + 41, rect.uly + 40, true);
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

/// `ClanSelectMenu` screen (`clanselectmenu.cpp`): 8 clans, logos on
/// the cards, names, Random/Cancel/?/Done.
///
/// Full clan descriptions and the bonus table from `SC_CLANS`/`clans.json` are not
/// loaded into the Rust client yet, so here only the name of the selected
/// clan and a hint are shown; the mechanics themselves (selection + application to `team_clans`)
/// are implemented and affect the starting unit attributes.
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
            // anchor (41,40) inside the 91×86 card.
            painted +=
                blit_simple_anchored(frame, palette, logo, rect.ulx + 41, rect.uly + 40, true);
        }

        // Hover: highlight with a frame (C++ does not draw hover, but a click on
        // the card is the main way to select; the highlight does not move the logo).
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

/// Status line in green with a shadow (`Text_TextLine`, `Fonts_BrightSilverColor` style).
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

/// `thickness` frame inside the rectangle.
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

/// Draws a big image at `origin` with the window palette.
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

/// Copies a 112×112 minimap (RGBA) into the slot.
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

/// Splits text by width (by words) — `Text_SplitText`
/// (shared with the turn-transition window, see [`crate::menu::wrap_gnw_text`]).
fn wrap_text(text: &str, width: i32) -> Vec<String> {
    wrap_gnw_text(text, width)
}

/// Loads a planet minimap from `.WRL` the same way as
/// `Menu_LoadPlanetMinimap` (`menu.cpp:663`).
///
/// Returns 112×112 RGBA: the `.WRL` palette is divided by 4 (`palette[i] /= 4`),
/// then each color is mapped into the window palette (`Color_MapColor(..., true)`).
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
        assert_eq!(form.timer_index(), 3); // 180 → the string "180"
        assert_eq!(form.endturn_index(), 3); // 45 → the string "45"
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
            InputEvent::MouseClick(
                banner.ulx + banner.width / 2,
                banner.uly + banner.height / 2,
            ),
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
            InputEvent::MouseClick(
                banner.ulx + banner.width / 2,
                banner.uly + banner.height / 2,
            ),
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
            InputEvent::MouseClick(
                cancel.ulx + cancel.width / 2,
                cancel.uly + cancel.height / 2,
            ),
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
            InputEvent::MouseClick(
                banner.ulx + banner.width / 2,
                banner.uly + banner.height / 2,
            ),
            640,
            480,
        );
        let random = CLAN_BUTTONS[0].0;
        form.handle_input(
            InputEvent::MouseClick(
                random.ulx + random.width / 2,
                random.uly + random.height / 2,
            ),
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
