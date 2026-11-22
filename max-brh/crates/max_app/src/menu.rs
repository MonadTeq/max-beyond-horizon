//! Menu on the original `MAX.RES` resources (item E of `max-brh-docs/visual-1to1-status.md`).
//!
//! The C++ port draws the menu not procedurally but with ready-made pictures and the raster
//! GNW font:
//!
//! * **main menu** (`menu.cpp::main_menu`) — `MAINPIC` background 640×480 (it already has
//!   the M.A.X. logo drawn), the `Main Menu` title in `FONT5` at
//!   (236,145) 172×15, the `P_*` "portrait" 300×240 at (16,182) in the
//!   `BGRSCRNL`/`BGRSCRNR` frames (6,173)/(166,173), six `BLANK_UP` 200×29 buttons at
//!   x=385 with a step of 35 from y=175 and `Exit` `SBLNK_UP` 100×30 at (435,420),
//!   the copyright at the bottom;
//! * **setup/planet selection menu** (`gamesetupmenu.cpp`,
//!   `planetselectmenu.cpp`) — `PLANETSE`/`MAINPIC` background, `MNUBTN4U`/
//!   `MNUBTN5U`/`MNUBTN6U` 109×40/48×40 buttons in the bottom row y=438;
//! * **game list** (`networkmenu.cpp`) — `MULTGAME` background, host rows
//!   (359,28) 209×160, 20 px each, `COLOR_GREEN` separators, `MNUBTN3U`/
//!   `MNUBTN4U`/`MNUBTN5U`/`MNUBTN6U` buttons at (243/354/465/514, 438).
//!
//! Button labels — `GNW_TEXT_FONT_1`, the colors `Fonts_GoldColor (165,175,199)` and
//! `Fonts_DarkOrageColor (5,56,199)`; simple images are indexed by the system
//! window palette, i.e. the `MAINPIC` palette (C++ loads it with
//! `palette_from_image=true`).
//!
//! # Differences from C++
//!
//! * The main menu shows all seven buttons of the original; the actions that
//!   the Rust client does not have yet (`Load Game`, `Setup`, `Introduction`, `Credits`)
//!   write the status line "screen not ported" — like the panel buttons in
//!   `panels.rs`.
//! * Mouse hover does **not** switch the button to `*D`: the pressed picture
//!   remains only for the selected radio/toggle buttons (like `Button::SetRestState`
//!   in C++); for regular buttons `*D` — only while actually pressed.
//! * The 8×3 planet grid is kept from the Rust client (the original has 3×2 on the
//!   `PLANETSE` screen), but is fitted into the 3×2 slot area of `PLANETSE` (192,51) 388×249.
//! * `BTNHIDE` is not used in the C++ port (checked with `grep`); here it is
//!   the "hide list" button in the top-right corner of the game list frame.

use max_assets::image::{BigImage, SimpleImage};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{
    draw_text_gnw, draw_text_gnw_centered, draw_text_gnw_centered_shadowed, draw_text_gnw_shadowed,
    GnwFontId,
};
use max_render::ui::Rect;

use crate::panels::UiPalette;
use crate::ui::{planet_name, CreateField, CreateForm, PLANET_COUNT};
use crate::Assets;

/// Menu logical size (the original window).
pub const MENU_WIDTH: i32 = 640;
/// Menu logical size (the original window).
pub const MENU_HEIGHT: i32 = 480;

/// Regular text index in the `MAINPIC` palette (`0xA2`).
pub const TEXT: u8 = 0xA2;
/// Green text/line index (`COLOR_GREEN`).
pub const GREEN: u8 = 0x02;
/// Copyright — black (`COLOR_BLACK`).
pub const BLACK: u8 = 0x00;

/// The "released/pressed" pair.
#[derive(Debug, Clone)]
pub struct MenuButtonPair {
    up: SimpleImage,
    down: SimpleImage,
}

impl MenuButtonPair {
    /// A pair of ready-made pictures (for screens that load their own resources —
    /// e.g. `BuildMenu` with `BLDBLD_U`/`BLD2X_U`/`BLD4X_U`).
    pub fn new(up: SimpleImage, down: SimpleImage) -> Self {
        Self { up, down }
    }

    pub fn image(&self, down: bool) -> &SimpleImage {
        if down {
            &self.down
        } else {
            &self.up
        }
    }

    /// Picture size (width, height).
    pub fn size(&self) -> (i32, i32) {
        (self.up.width as i32, self.up.height as i32)
    }

    /// Pixel of the released/pressed picture (for tests).
    pub fn pixel(&self, down: bool, x: u32, y: u32) -> Option<u8> {
        self.image(down).pixel(x, y)
    }
}

/// Main menu button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuAction {
    /// Create a game (`New Game`).
    Create,
    /// Game list (`Multiplayer Game`).
    Join,
    /// Exit (`Exit`).
    Quit,
    /// Credits screen (`Credits` → `menu_credits_menu_loop`).
    Credits,
    /// Introduction (`Introduction` → `Movie_PlayIntro`, MVE `maxint.mve`).
    Introduction,
    /// Setup screen (`menu_setup_window` → `OptionsMenu(SETUPPIC)`).
    Setup,
    /// A screen not ported yet.
    Unported(&'static str),
}

/// One main menu button in C++ coordinates.
#[derive(Debug, Clone, Copy)]
pub struct MainMenuButton {
    /// Rectangle in the 640×480 window.
    pub rect: PixelRect,
    /// Label from `language.json` (en-US).
    pub label: &'static str,
    /// Action.
    pub action: MainMenuAction,
    /// Big (`BLANK_UP` 200×29) or small (`SBLNK_UP` 100×30).
    pub big: bool,
}

/// Main menu buttons (`menu.cpp::main_menu`, `MENU_BUTTON_DEF`).
pub const MAIN_MENU_BUTTONS: [MainMenuButton; 7] = [
    MainMenuButton {
        rect: PixelRect::new(385, 175, 200, 29),
        label: "New Game",
        action: MainMenuAction::Create,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 210, 200, 29),
        label: "Load Game",
        action: MainMenuAction::Unported("Load Game"),
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 245, 200, 29),
        label: "Multiplayer Game",
        action: MainMenuAction::Join,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 280, 200, 29),
        label: "Setup",
        action: MainMenuAction::Setup,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 315, 200, 29),
        label: "Introduction",
        action: MainMenuAction::Introduction,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 350, 200, 29),
        label: "Credits",
        action: MainMenuAction::Credits,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(435, 420, 100, 30),
        label: "Exit",
        action: MainMenuAction::Quit,
        big: false,
    },
];

/// Main menu title (`draw_menu_title(window, _(35f9))`).
pub const MAIN_TITLE: PixelRect = PixelRect::new(236, 145, 172, 15);
/// Main menu "portrait" (`P_*` 300×240).
pub const MAIN_PORTRAIT: PixelRect = PixelRect::new(16, 182, 300, 240);
/// Left portrait frame `BGRSCRNL`.
pub const MAIN_PORTRAIT_LEFT: (i32, i32) = (6, 173);
/// Right portrait frame `BGRSCRNR`.
pub const MAIN_PORTRAIT_RIGHT: (i32, i32) = (166, 173);
/// Copyright: (10,469) 620×10 (`draw_copyright_label`).
pub const COPYRIGHT: PixelRect = PixelRect::new(10, 469, 620, 10);
/// Rust client status line (the original does not have it).
pub const MAIN_STATUS: PixelRect = PixelRect::new(10, 452, 370, 12);

// ---------------------------------------------------------------------------
// Multiplayer Menu (`menu_multiplayer_menu_loop`, `menu.cpp:2361`)
// ---------------------------------------------------------------------------

/// `Multiplayer Menu` button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiplayerAction {
    /// `Host Network Game` (`_(c7a9)`).
    Host,
    /// `Join Network Game` (`_(7064)`).
    Join,
    /// `Hot Seat Game` (`_(f731)`) — M-HS.
    HotSeat,
    /// `Hot Seat Scenarios` (`_(bf00)`).
    HotSeatScenarios,
    /// `Load Hot Seat Game` (`_(847e)`).
    LoadHotSeat,
    /// `Cancel` (`_(e13e)`).
    Cancel,
}

/// `Multiplayer Menu` button in C++ coordinates.
#[derive(Debug, Clone, Copy)]
pub struct MultiplayerButton {
    /// Rectangle (200×29 `BLANK_UP`, or 100×30 `SBLNK_UP` for `Cancel`).
    pub rect: PixelRect,
    /// Label (`language.json`, en-US).
    pub label: &'static str,
    /// Action.
    pub action: MultiplayerAction,
    /// Big button (`BLANK_*`) or small (`SBLNK_*`).
    pub big: bool,
}

/// `Multiplayer Menu` title (`draw_menu_title(window, _(f2b3))`).
pub const MULTIPLAYER_TITLE: PixelRect = PixelRect::new(236, 145, 172, 15);

/// Six `network_game_menu_buttons` buttons (`menu.cpp:2372`).
pub const MULTIPLAYER_BUTTONS: [MultiplayerButton; 6] = [
    MultiplayerButton {
        rect: PixelRect::new(385, 175, 200, 29),
        label: "Host Network Game",
        action: MultiplayerAction::Host,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(385, 210, 200, 29),
        label: "Join Network Game",
        action: MultiplayerAction::Join,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(385, 280, 200, 29),
        label: "Hot Seat Game",
        action: MultiplayerAction::HotSeat,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(385, 315, 200, 29),
        label: "Hot Seat Scenarios",
        action: MultiplayerAction::HotSeatScenarios,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(385, 350, 200, 29),
        label: "Load Hot Seat Game",
        action: MultiplayerAction::LoadHotSeat,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(435, 420, 100, 30),
        label: "Cancel",
        action: MultiplayerAction::Cancel,
        big: false,
    },
];

/// `Multiplayer Menu` button under the point `(x, y)`.
pub fn multiplayer_button_at(x: i32, y: i32) -> Option<MultiplayerAction> {
    MULTIPLAYER_BUTTONS
        .iter()
        .find(|button| button.rect.contains(x, y))
        .map(|button| button.action)
}

/// Bottom row button of the setup/planets menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuButtonAction {
    /// Random planet.
    Random,
    /// Back/cancel.
    Cancel,
    /// Help (screen not ported).
    Help,
    /// Done/create.
    Done,
}

/// Bottom row button in screen coordinates.
#[derive(Debug, Clone, Copy)]
pub struct ActionButton {
    /// Rectangle.
    pub rect: PixelRect,
    /// Label.
    pub label: &'static str,
    /// Action.
    pub action: MenuButtonAction,
    /// `MNUBTNnU` resource number (1..=6).
    pub resource: usize,
}

/// `PlanetSelectMenu` buttons (`planetselectmenu.hpp`, y=438).
pub const PLANET_BUTTONS: [ActionButton; 4] = [
    ActionButton {
        rect: PixelRect::new(243, 438, 109, 40),
        label: "Random",
        action: MenuButtonAction::Random,
        resource: 4,
    },
    ActionButton {
        rect: PixelRect::new(354, 438, 109, 40),
        label: "Cancel",
        action: MenuButtonAction::Cancel,
        resource: 4,
    },
    ActionButton {
        rect: PixelRect::new(465, 438, 48, 40),
        label: "?",
        action: MenuButtonAction::Help,
        resource: 5,
    },
    ActionButton {
        rect: PixelRect::new(514, 438, 109, 40),
        label: "Done",
        action: MenuButtonAction::Done,
        resource: 6,
    },
];

/// Planet screen title (`_(f7c8)` = "Planet Select Menu").
pub const PLANET_TITLE: PixelRect = PixelRect::new(230, 6, 180, 20);
/// Selected planet name (16,61) 149×19.
pub const PLANET_NAME: PixelRect = PixelRect::new(16, 61, 149, 19);
/// `PLANETSE` 3×2 slot area the 8×3 grid is fitted into.
pub const PLANET_GRID: PixelRect = PixelRect::new(192, 51, 388, 249);
/// Description/form fields (41,350) 559×59.
pub const PLANET_DESCRIPTION: PixelRect = PixelRect::new(41, 350, 559, 59);
/// Form status line.
pub const PLANET_STATUS: PixelRect = PixelRect::new(41, 412, 559, 12);

/// Rectangles of the six form fields (3×2 inside the `PLANETSE` description).
pub const CREATE_FIELD_RECTS: [(CreateField, PixelRect); 6] = [
    (CreateField::Name, PixelRect::new(48, 352, 180, 24)),
    (CreateField::Timer, PixelRect::new(237, 352, 180, 24)),
    (CreateField::EndTurn, PixelRect::new(426, 352, 174, 24)),
    (CreateField::Opponent, PixelRect::new(48, 378, 180, 24)),
    (CreateField::Password, PixelRect::new(237, 378, 180, 24)),
    (CreateField::World, PixelRect::new(426, 378, 174, 24)),
];

/// Game list button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinButton {
    /// Refresh the list (`Options` in C++).
    Refresh,
    /// Back (`Cancel`).
    Back,
    /// Help (not ported).
    Help,
    /// Enter (`Ready`).
    Join,
    /// Hide the list (`BTNHIDE`).
    Hide,
}

/// `NetworkMenu` buttons (243/354/465/514, 438).
pub const JOIN_BUTTONS: [ActionButton; 4] = [
    ActionButton {
        rect: PixelRect::new(243, 438, 109, 40),
        label: "Options",
        action: MenuButtonAction::Random,
        resource: 3,
    },
    ActionButton {
        rect: PixelRect::new(354, 438, 109, 40),
        label: "Cancel",
        action: MenuButtonAction::Cancel,
        resource: 4,
    },
    ActionButton {
        rect: PixelRect::new(465, 438, 48, 40),
        label: "?",
        action: MenuButtonAction::Help,
        resource: 5,
    },
    ActionButton {
        rect: PixelRect::new(514, 438, 109, 40),
        label: "Ready",
        action: MenuButtonAction::Done,
        resource: 6,
    },
];

/// Game list frame/area (`MENU_ITEM_JOIN_TEXT_WINDOW`, (359,28) 209×160).
pub const JOIN_LIST: PixelRect = PixelRect::new(359, 28, 209, 160);
/// List row height (20 px).
pub const JOIN_ROW_HEIGHT: i32 = 20;
/// Panel under the list (4 px wider — the "frame").
pub const JOIN_PANEL: PixelRect = PixelRect::new(355, 24, 217, 168);
/// `BTNHIDE` button in the top-right corner of the list.
pub const JOIN_HIDE: PixelRect = PixelRect::new(528, 4, 38, 21);
/// Game list title.
pub const JOIN_TITLE: PixelRect = PixelRect::new(359, 4, 190, 16);
/// List status line.
pub const JOIN_STATUS: PixelRect = PixelRect::new(150, 400, 462, 15);

/// How many list rows are visible at once.
pub const JOIN_VISIBLE_ROWS: usize = 8;

/// First visible row when `selected` is chosen.
pub fn join_scroll(selected: usize) -> usize {
    selected.saturating_sub(JOIN_VISIBLE_ROWS - 1)
}

/// List row rectangle (0 — the first visible).
pub fn join_row_rect(row: usize) -> PixelRect {
    if row >= JOIN_VISIBLE_ROWS {
        return PixelRect::new(0, 0, 0, 0);
    }

    PixelRect::new(
        JOIN_LIST.ulx,
        JOIN_LIST.uly + row as i32 * JOIN_ROW_HEIGHT,
        JOIN_LIST.width,
        JOIN_ROW_HEIGHT,
    )
}

/// List row under the point (taking scrolling into account); `None` — outside the list.
pub fn join_row_at(x: i32, y: i32, games: usize, selected: usize) -> Option<usize> {
    if !JOIN_LIST.contains(x, y) {
        return None;
    }

    let row = ((y - JOIN_LIST.uly) / JOIN_ROW_HEIGHT) as usize;
    let index = join_scroll(selected) + row;

    (row < JOIN_VISIBLE_ROWS && index < games).then_some(index)
}

/// Loaded menu resources.
#[derive(Debug, Clone)]
pub struct MenuAssets {
    /// Main menu background 640×480 (`MAINPIC`).
    pub mainpic: BigImage,
    /// Planet screen background (`PLANETSE`).
    pub planetse: Option<BigImage>,
    /// Game list background (`MULTGAME`).
    pub multgame: Option<BigImage>,
    /// Game window background (`FRAMEPIC`) — fallback if something of the menu is missing.
    pub framepic: Option<BigImage>,
    /// Match setup screen background (`OPTNFRM`, `GameConfigMenu`).
    pub optnfrm: Option<BigImage>,
    /// Team selection screen background (`CHOSPLYR`, `ChoosePlayerMenu`).
    pub chosplyr: Option<BigImage>,
    /// Planet pictures by groups (`SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC`).
    pub planet_group_pictures: [Option<BigImage>; 4],
    /// Planet group switch arrows (`MNULAROU`, `MNURAROU`).
    pub planet_arrows: Option<(SimpleImage, SimpleImage)>,
    /// Team type selection buttons: `CH_HUM_U/D`, `CH_CMP_U/D`, `CH_NON_U/D`.
    pub team_kind_buttons: [Option<MenuButtonPair>; 3],
    /// Team flags `CH_TM1_U/D` … `CH_TM4_U/D`.
    pub team_banners: [Option<MenuButtonPair>; 4],
    /// Clan emblem `CLN0LOGO` (42×65): `ChoosePlayerMenu::ButtonInit`
    /// copies it onto the team flag at (41,40).
    pub clan_logo: Option<SimpleImage>,
    /// Clan emblems `CLN0LOGO`…`CLN8LOGO` (0 — Random).
    pub clan_logos: [Option<SimpleImage>; 9],
    /// `CH_CN1_U/D`…`CH_CN8_U/D` cards (91×86) of the `ClanSelectMenu` screen.
    pub clan_buttons: [Option<MenuButtonPair>; 8],
    /// Clan selection screen background (`CLANSEL`).
    pub clansel: Option<BigImage>,
    /// Mobilization screen background (`CARGOPIC`, 640×480) — `CargoMenu`.
    pub cargopic: Option<BigImage>,
    /// `Buy` button of the mobilization screen (`CRGBUY_U/D`, 41×22).
    pub cargo_buy: Option<MenuButtonPair>,
    /// `Delete` button (`CRGDEL_U/D`, 53×23).
    pub cargo_delete: Option<MenuButtonPair>,
    /// `Cancel` button (`BLDCAN_U/D`, 62×23).
    pub cargo_cancel: Option<MenuButtonPair>,
    /// `Done` button (`BLDONE_U/D`, 54×23).
    pub cargo_done: Option<MenuButtonPair>,
    /// Victory/defeat screen backgrounds (`ENDGAME1`…`ENDGAME9`).
    pub endgame_backgrounds: Vec<BigImage>,
    /// `ENDARM` — the picture in the bottom-right corner of the victory screen.
    pub endarm: Option<SimpleImage>,
    /// `OK` button of the victory screen (`ENDOK_U/D`).
    pub endok: Option<MenuButtonPair>,
    /// `HELPFRAM` — `DialogMenu` window background 301×231 (M-HS turn-transition screen).
    pub helpfram: Option<BigImage>,
    /// `OK` button of the `DialogMenu` window (`HELPOK_U/D`, 65×24).
    pub helpok: Option<MenuButtonPair>,
    /// Main menu "portrait" (`P_TANK` 300×240).
    pub portrait: Option<BigImage>,
    /// Big buttons `BLANK_UP/DN` 200×29.
    pub blank: MenuButtonPair,
    /// Small buttons `SBLNK_UP/DN` 100×30.
    pub small_blank: MenuButtonPair,
    /// `MNUBTN1U/D` … `MNUBTN6U/D` (index 0 — resource 1).
    pub mnubtn: [MenuButtonPair; 6],
    /// `BTNHIDE` button 38×21.
    pub btnhide: Option<SimpleImage>,
    /// Left portrait frame `BGRSCRNL` 160×259.
    pub portrait_frame_left: Option<SimpleImage>,
    /// Right portrait frame `BGRSCRNR` 160×259.
    pub portrait_frame_right: Option<SimpleImage>,
    /// Credits screen background `CREDITS` 640×480 (big image with its own palette,
    /// `menu_credits_menu_loop`).
    pub credits: Option<BigImage>,
    /// `Introduction` placeholder background: `MLOGO` 640×480 (big image), if present.
    pub mlogo: Option<BigImage>,
    /// Window system palette (from `MAINPIC`).
    pub palette: UiPalette,
}

impl MenuAssets {
    /// Loads the menu resources from `MAX.RES`.
    ///
    /// Returns `None` only if `MAINPIC`/`FRAMEPIC` or the main
    /// buttons are missing: without them the menu cannot be assembled. The rest (`PLANETSE`, `MULTGAME`,
    /// portrait, frames, `BTNHIDE`) — optional.
    pub fn load(max_res: &max_assets::ResArchive) -> Option<Self> {
        let read_big = |name: &str| {
            max_res
                .read(name)
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok())
        };
        let read_simple = |name: &str| {
            max_res
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair {
                up: read_simple(up)?,
                down: read_simple(down)?,
            })
        };

        let framepic = read_big("FRAMEPIC");
        let mainpic = read_big("MAINPIC").or_else(|| framepic.clone())?;
        let palette = UiPalette::from_big_image(&mainpic);

        let mut mnubtn = Vec::with_capacity(6);
        for index in 1..=6 {
            mnubtn.push(pair(
                &format!("MNUBTN{index}U"),
                &format!("MNUBTN{index}D"),
            )?);
        }

        let portrait = ["P_TANK", "P_DEPOT", "P_HANGAR", "P_TRANSP"]
            .iter()
            .find_map(|name| read_big(name))
            .or_else(|| {
                // Any `P_*` 300×240, if the usual names are missing.
                max_res
                    .list()
                    .iter()
                    .map(|entry| entry.name.as_str())
                    .find(|name| name.starts_with("P_"))
                    .and_then(read_big)
            });

        Some(Self {
            mainpic,
            planetse: read_big("PLANETSE"),
            multgame: read_big("MULTGAME"),
            framepic,
            optnfrm: read_big("OPTNFRM"),
            chosplyr: read_big("CHOSPLYR"),
            planet_group_pictures: [
                read_big("SNOW_PIC"),
                read_big("CRTR_PIC"),
                read_big("GREN_PIC"),
                read_big("DSRT_PIC"),
            ],
            planet_arrows: read_simple("MNULAROU").zip(read_simple("MNURAROU")),
            team_kind_buttons: [
                pair("CH_HUM_U", "CH_HUM_D"),
                pair("CH_CMP_U", "CH_CMP_D"),
                pair("CH_NON_U", "CH_NON_D"),
            ],
            team_banners: [
                pair("CH_TM1_U", "CH_TM1_D"),
                pair("CH_TM2_U", "CH_TM2_D"),
                pair("CH_TM3_U", "CH_TM3_D"),
                pair("CH_TM4_U", "CH_TM4_D"),
            ],
            clan_logo: read_simple("CLN0LOGO"),
            clan_logos: std::array::from_fn(|index| read_simple(&format!("CLN{index}LOGO"))),
            clan_buttons: std::array::from_fn(|index| {
                pair(
                    &format!("CH_CN{}_U", index + 1),
                    &format!("CH_CN{}_D", index + 1),
                )
            }),
            clansel: read_big("CLANSEL"),
            cargopic: read_big("CARGOPIC"),
            cargo_buy: pair("CRGBUY_U", "CRGBUY_D"),
            cargo_delete: pair("CRGDEL_U", "CRGDEL_D"),
            cargo_cancel: pair("BLDCAN_U", "BLDCAN_D"),
            cargo_done: pair("BLDONE_U", "BLDONE_D"),
            endgame_backgrounds: [
                "ENDGAME1", "ENDGAME2", "ENDGAME3", "ENDGAME4", "ENDGAME5", "ENDGAME6", "ENDGAME7",
                "ENDGAME8", "ENDGAME9",
            ]
            .iter()
            .filter_map(|name| read_big(name))
            .collect(),
            endarm: read_simple("ENDARM"),
            endok: pair("ENDOK_U", "ENDOK_D"),
            helpfram: read_big("HELPFRAM"),
            helpok: pair("HELPOK_U", "HELPOK_D"),
            portrait,
            blank: pair("BLANK_UP", "BLANK_DN")?,
            small_blank: pair("SBLNK_UP", "SBLNK_DN")?,
            mnubtn: mnubtn.try_into().ok()?,
            btnhide: read_simple("BTNHIDE"),
            portrait_frame_left: read_simple("BGRSCRNL"),
            portrait_frame_right: read_simple("BGRSCRNR"),
            credits: read_big("CREDITS"),
            mlogo: read_big("MLOGO"),
            palette,
        })
    }

    /// Planet screen background (`PLANETSE`, else `FRAMEPIC`, else `MAINPIC`).
    pub fn planet_background(&self) -> &BigImage {
        self.planetse
            .as_ref()
            .or(self.framepic.as_ref())
            .unwrap_or(&self.mainpic)
    }

    /// Game list background (`MULTGAME`, else `PLANETSE`, else `MAINPIC`).
    pub fn join_background(&self) -> &BigImage {
        self.multgame
            .as_ref()
            .or(self.planetse.as_ref())
            .or(self.framepic.as_ref())
            .unwrap_or(&self.mainpic)
    }

    /// `MNUBTNnU/D` pair by resource number (1..=6).
    pub fn mnubtn_pair(&self, resource: usize) -> Option<&MenuButtonPair> {
        self.mnubtn.get(resource.checked_sub(1)?)
    }
}

/// Draws a big image into the frame taking the picture's `ulx/uly` into account.
pub fn draw_big_image(frame: &mut RgbaBuffer, image: &BigImage) -> u32 {
    let offset_x = i32::from(image.ulx);
    let offset_y = i32::from(image.uly);
    let mut painted = 0;

    for y in 0..image.height as u32 {
        for x in 0..image.width as u32 {
            let Some([r, g, b]) = image.rgb(x, y) else {
                continue;
            };

            if frame.set(offset_x + x as i32, offset_y + y as i32, Rgba::rgb(r, g, b)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Copies a simple image into the frame with the window palette.
///
/// `transparent` — skip pixels of the `image.transparent` index (C++
/// `WindowManager_LoadSimpleImage(..., has_transparency)`).
pub fn blit_simple(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    x: i32,
    y: i32,
    transparent: bool,
) -> u32 {
    let mut painted = 0;

    for py in 0..image.height as u32 {
        for px in 0..image.width as u32 {
            let Some(index) = image.pixel(px, py) else {
                continue;
            };

            if transparent && index == image.transparent {
                continue;
            }

            if frame.set(x + px as i32, y + py as i32, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Copies a simple image taking its own `ulx/uly` into account — the "anchor" point.
///
/// This is `WindowManager_DecodeSimpleImage` (`window_manager.cpp:784`) verbatim:
///
/// ```c
/// ulx -= image->ulx;
/// uly -= image->uly;
/// ```
///
/// This is how clan emblems are drawn: `Button::Copy(CLNnLOGO, 41, 40)` puts
/// the picture at the **anchor** point `(41,40)`, and the resource's own `ulx/uly`
/// (`CLN0LOGO` — 21,32; `CLN1LOGO` — 37,28; …) shift it so that the logo
/// ends up in the center of the button. Without this correction the picture drifts right-down.
pub fn blit_simple_anchored(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    anchor_x: i32,
    anchor_y: i32,
    transparent: bool,
) -> u32 {
    blit_simple(
        frame,
        palette,
        image,
        anchor_x - i32::from(image.ulx),
        anchor_y - i32::from(image.uly),
        transparent,
    )
}

/// Word wrapping by the width of the GNW `FONT5` font (`Text_SplitText`).
///
/// Used by the setup screens (`GameConfigMenu`/`PlanetSelectMenu`) and
/// the M-HS turn-transition window (`DialogMenu::DrawText`): the promised width is in
/// pixels, wrapping is by spaces, `\n` — a hard line break.
pub fn wrap_gnw_text(text: &str, width: i32) -> Vec<String> {
    let font = GnwFontId::Font5.font();
    let mut lines = Vec::new();

    for paragraph in text.split('\n') {
        let mut line = String::new();

        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };

            if font.text_width(&candidate) > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }

        lines.push(line);
    }

    lines
}

/// Replaces typographic characters that the GNW fonts do not have (`—`, `…`) with
/// ASCII: otherwise `draw_text_gnw` draws `?` instead of them.
pub fn clean_status(text: &str) -> String {
    let mut output = String::with_capacity(text.len());

    for character in text.chars() {
        match character {
            '—' | '–' | '·' => output.push('-'),
            '…' => output.push_str("..."),
            '’' | '‘' => output.push('\''),
            '“' | '”' => output.push('"'),
            other => output.push(other),
        }
    }

    output
}

/// Menu title: `GNW_TEXT_FONT_5`, `COLOR_GREEN`, centered in the rectangle.
pub fn draw_title(frame: &mut RgbaBuffer, palette: &UiPalette, rect: PixelRect, text: &str) -> i32 {
    draw_text_gnw_centered(
        frame,
        rect,
        GnwFontId::Font5.font(),
        text,
        palette.rgba(GREEN),
    )
}

/// Menu button with an `up/down` picture and a `GNW_TEXT_FONT_1` label.
///
/// In the original `*D` is shown only while the button is **pressed** (`Button` in
/// `GNW`/`Button::Run`). Mouse hover is not a press, so here
/// `up` is always drawn; the pressed state is left for the radio/toggle buttons
/// that pass `down` explicitly. `_down_image` and `_pressed` are kept in the
/// signature so as not to change all calls.
pub fn draw_image_button(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    _down_image: &SimpleImage,
    rect: PixelRect,
    label: &str,
    _pressed: bool,
) -> u32 {
    let mut painted = blit_simple(frame, palette, image, rect.ulx, rect.uly, true);

    if !label.is_empty() {
        let (base, outline, shadow) = (165u8, 175u8, 199u8);

        painted += draw_text_gnw_centered_shadowed(
            frame,
            rect,
            GnwFontId::Font1.font(),
            label,
            palette.rgba(base),
            palette.rgba(outline),
            palette.rgba(shadow),
        )
        .max(0) as u32;
    }

    painted
}

/// "Portrait" with the `BGRSCRNL`/`BGRSCRNR` frames — shared by the main menu and
/// `Multiplayer Menu` (`menu_draw_menu_portrait`).
fn draw_menu_portrait(frame: &mut RgbaBuffer, menu: &MenuAssets) -> u32 {
    let palette = &menu.palette;
    let mut painted = 0;

    if let Some(ref frame_left) = menu.portrait_frame_left {
        painted += blit_simple(
            frame,
            palette,
            frame_left,
            MAIN_PORTRAIT_LEFT.0,
            MAIN_PORTRAIT_LEFT.1,
            false,
        );
    }
    if let Some(ref frame_right) = menu.portrait_frame_right {
        painted += blit_simple(
            frame,
            palette,
            frame_right,
            MAIN_PORTRAIT_RIGHT.0,
            MAIN_PORTRAIT_RIGHT.1,
            false,
        );
    }
    if let Some(ref portrait) = menu.portrait {
        painted += draw_big_image_at(
            frame,
            portrait,
            palette,
            (MAIN_PORTRAIT.ulx, MAIN_PORTRAIT.uly),
            MAIN_PORTRAIT,
        );
    }

    painted
}

/// Draws `Multiplayer Menu` (`menu_multiplayer_menu_loop`): `MAINPIC`, title
/// `_(f2b3)`, portrait and six buttons (`Host/Join/Hot Seat/…/Cancel`).
pub fn draw_multiplayer_menu(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, &menu.mainpic);

    draw_title(frame, palette, MULTIPLAYER_TITLE, "Multiplayer Menu");
    painted += draw_menu_portrait(frame, menu);

    for button in &MULTIPLAYER_BUTTONS {
        let pair = if button.big {
            &menu.blank
        } else {
            &menu.small_blank
        };
        let hovered = mouse.is_some_and(|(x, y)| button.rect.contains(x, y));

        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            button.rect,
            button.label,
            hovered,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw(
            frame,
            MAIN_STATUS.ulx,
            MAIN_STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(BLACK),
        );
    }

    painted
}

/// Draws the main menu (`menu.cpp::main_menu`).
pub fn draw_main_menu(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, &menu.mainpic);

    draw_title(frame, palette, MAIN_TITLE, "Main Menu");
    painted += draw_menu_portrait(frame, menu);

    draw_text_gnw_centered(
        frame,
        COPYRIGHT,
        GnwFontId::Font5.font(),
        "Copyright 1996 Interplay Productions. v1.04  (M.A.X. Rust client)",
        palette.rgba(BLACK),
    );

    for button in &MAIN_MENU_BUTTONS {
        let pair = if button.big {
            &menu.blank
        } else {
            &menu.small_blank
        };
        let hovered = mouse.is_some_and(|(x, y)| button.rect.contains(x, y));

        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            button.rect,
            button.label,
            hovered,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw(
            frame,
            MAIN_STATUS.ulx,
            MAIN_STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(BLACK),
        );
    }

    painted
}

/// Draws the game creation/planet selection screen (`planetselectmenu.cpp` +
/// the 8×3 grid of the Rust client).
pub fn draw_create_form(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    assets: Option<&Assets>,
    form: &CreateForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, menu.planet_background());

    draw_title(frame, palette, PLANET_TITLE, "Planet Select Menu");
    let selected_planet = planet_for_world(form.world);
    draw_title(frame, palette, PLANET_NAME, planet_name(selected_planet));

    // A big picture of the selected planet — exactly like
    // `WindowManager_LoadBigImage(SNOW_PIC + world/6, ..., ulx, uly)`:
    // `SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC` have `ulx=26, uly=98`.
    if let Some(picture) = assets.and_then(|a| a.planet_picture(selected_planet)) {
        let picture_rect = PixelRect::new(26, 98, 128, 128);
        painted += draw_big_image_at(frame, picture, palette, (26, 98), picture_rect);
    }

    // The 8×3 grid inside the 3×2 slot area of PLANETSE.
    let name_font = GnwFontId::Font5.font();

    for index in 0..PLANET_COUNT {
        let cell = planet_cell(index as i32);
        let size = (cell.width - 6).min(cell.height - 24).max(1);
        let content_height = size + 2 + name_font.height();
        let top = cell.uly + ((cell.height - content_height) / 2).max(0);
        let image_rect = PixelRect::new(cell.ulx + (cell.width - size) / 2, top, size, size);
        let planet = crate::ui::planet_from_index(index);
        let picture = planet.and_then(|planet| assets.and_then(|a| a.planet_picture(planet)));
        let selected = form.world == index as i64;

        // Dark backing of the cell: the 8×3 grid lies on the 3×2 black slots
        // of `PLANETSE` as an even "canvas" rather than ragged pieces of the background.
        frame.fill_rect(cell, palette.rgba(BLACK));

        if let (Some(picture), Some(planet)) = (picture, planet) {
            painted += blit_big_scaled(frame, picture, image_rect, planet_picture_crop(planet));
        }

        if let Some(planet) = planet {
            let max_width = (cell.width - 2).max(1);
            let mut name = planet_name(planet).to_string();

            while name_font.text_width(&name) > max_width && name.pop().is_some() {}

            let x = cell.ulx + (cell.width - name_font.text_width(&name)).max(0) / 2;
            let y = top + size + 2;
            let color = if selected { GREEN } else { TEXT };

            draw_text_gnw(frame, x, y, name_font, &name, palette.rgba(color));
        }

        let border = if selected {
            Rgba::rgb(252, 0, 0)
        } else {
            palette.rgba(TEXT).darken(3)
        };
        draw_border(frame, cell, border, if selected { 2 } else { 1 });
    }

    // Form fields inside the PLANETSE description.
    for (field, rect) in CREATE_FIELD_RECTS {
        let focused = form.focus == field;
        let mut line = format!("{}: {}", field.label(), form.value(field));

        if focused {
            line.push('_');
        }

        let text = truncate_to_width(line.as_str(), rect.width - 4, GnwFontId::Font5.font());
        let color = if focused { GREEN } else { TEXT };

        draw_text_gnw(
            frame,
            rect.ulx + 2,
            rect.uly + (rect.height - GnwFontId::Font5.font().height()).max(0) / 2,
            GnwFontId::Font5.font(),
            &text,
            palette.rgba(color),
        );
    }

    for button in &PLANET_BUTTONS {
        let Some(pair) = menu.mnubtn_pair(button.resource) else {
            continue;
        };
        let hovered = mouse.is_some_and(|(x, y)| button.rect.contains(x, y));
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            button.rect,
            button.label,
            hovered,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw_shadowed(
            frame,
            PLANET_STATUS.ulx,
            PLANET_STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(GREEN),
            palette.rgba(0xA7),
            palette.rgba(0xE6),
        );
    }

    painted
}

/// Draws the game list (`networkmenu.cpp::DrawJoinScreen`).
pub fn draw_join_list(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    games: &[max_net::GameInfo],
    selected: usize,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, menu.join_background());

    // List frame: a black panel with a beige outline (host rows on `MULTGAME`
    // lie on a dark window; the panel makes them readable on the light
    // part of the background too).
    frame.fill_rect(JOIN_PANEL, Rgba::BLACK);
    draw_border(frame, JOIN_PANEL, palette.rgba(TEXT), 1);
    // The title — on a black plate: on the beige part of `MULTGAME` the green
    // GNW text is almost unreadable.
    frame.fill_rect(
        PixelRect::new(
            JOIN_TITLE.ulx,
            JOIN_TITLE.uly - 2,
            JOIN_TITLE.width,
            JOIN_TITLE.height + 4,
        ),
        Rgba::BLACK,
    );
    draw_title(frame, palette, JOIN_TITLE, "JOIN GAME");

    if let Some(ref btnhide) = menu.btnhide {
        let hovered = mouse.is_some_and(|(x, y)| JOIN_HIDE.contains(x, y));
        painted += blit_simple(frame, palette, btnhide, JOIN_HIDE.ulx, JOIN_HIDE.uly, true);
        if hovered {
            // `BTNHIDE` frame highlight: the C++ resource does not use it, the behavior
            // (hide/back) is ours.
            draw_border(frame, JOIN_HIDE, palette.rgba(GREEN), 1);
        }
    }

    let scroll = join_scroll(selected);

    if games.is_empty() {
        draw_text_gnw(
            frame,
            JOIN_LIST.ulx + 5,
            JOIN_LIST.uly + 6,
            GnwFontId::Font5.font(),
            "(no games — press Options to refresh)",
            palette.rgba(TEXT),
        );
    }

    for row in 0..JOIN_VISIBLE_ROWS {
        let index = scroll + row;
        let Some(game) = games.get(index) else {
            break;
        };
        let rect = join_row_rect(row);
        let lock = if game.password_protected { " *" } else { "" };
        let line = format!(
            "{}  [{}/{}]{}",
            game.name, game.players, game.min_players, lock
        );
        let text = truncate_to_width(&line, JOIN_LIST.width - 14, GnwFontId::Font5.font());
        let y = rect.uly + (JOIN_ROW_HEIGHT - GnwFontId::Font5.font().height()).max(0) / 2;

        if index == selected {
            draw_text_gnw(
                frame,
                JOIN_LIST.ulx + 1,
                y,
                GnwFontId::Font5.font(),
                ">",
                palette.rgba(GREEN),
            );
        }

        draw_text_gnw(
            frame,
            JOIN_LIST.ulx + 6,
            y,
            GnwFontId::Font5.font(),
            &text,
            palette.rgba(GREEN),
        );

        // Row separator — `draw_line(..., COLOR_GREEN)` as in C++.
        if row + 1 < JOIN_VISIBLE_ROWS {
            let line_y = rect.uly + JOIN_ROW_HEIGHT - 1;
            for x in JOIN_LIST.ulx..JOIN_LIST.ulx + JOIN_LIST.width {
                frame.set(x, line_y, palette.rgba(GREEN));
            }
        }

        if index == selected {
            let line_y = rect.uly + JOIN_ROW_HEIGHT - 2;
            for x in JOIN_LIST.ulx..JOIN_LIST.ulx + JOIN_LIST.width {
                frame.set(x, line_y, palette.rgba(GREEN));
            }
        }
    }

    for button in &JOIN_BUTTONS {
        let Some(pair) = menu.mnubtn_pair(button.resource) else {
            continue;
        };
        let hovered = mouse.is_some_and(|(x, y)| button.rect.contains(x, y));
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            button.rect,
            button.label,
            hovered,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw_shadowed(
            frame,
            JOIN_STATUS.ulx,
            JOIN_STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(GREEN),
            palette.rgba(0xA7),
            palette.rgba(0xE6),
        );
    }

    painted
}

/// Main menu button under the point.
pub fn main_menu_at(x: i32, y: i32) -> Option<MainMenuAction> {
    MAIN_MENU_BUTTONS
        .iter()
        .find(|button| button.rect.contains(x, y))
        .map(|button| button.action)
}

/// Main menu button rectangle by action.
pub fn main_menu_rect(action: MainMenuAction) -> Option<PixelRect> {
    MAIN_MENU_BUTTONS
        .iter()
        .find(|button| button.action == action)
        .map(|button| button.rect)
}

/// Bottom row form button under the point.
pub fn create_button_at(x: i32, y: i32) -> Option<ActionButton> {
    PLANET_BUTTONS
        .iter()
        .copied()
        .find(|b| b.rect.contains(x, y))
}

/// Form field under the point.
pub fn create_field_at(x: i32, y: i32) -> Option<CreateField> {
    CREATE_FIELD_RECTS
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(field, _)| *field)
}

/// Game list button under the point.
pub fn join_button_at(x: i32, y: i32) -> Option<JoinButton> {
    if JOIN_HIDE.contains(x, y) {
        return Some(JoinButton::Hide);
    }

    JOIN_BUTTONS
        .iter()
        .find(|button| button.rect.contains(x, y))
        .map(|button| match button.action {
            MenuButtonAction::Random => JoinButton::Refresh,
            MenuButtonAction::Cancel => JoinButton::Back,
            MenuButtonAction::Help => JoinButton::Help,
            MenuButtonAction::Done => JoinButton::Join,
        })
}

/// 8×3 planet grid in `PLANETSE` coordinates.
pub fn planet_cell(index: i32) -> PixelRect {
    let columns = 8;
    let rows = 3;
    let cell_width = PLANET_GRID.width / columns;
    let cell_height = PLANET_GRID.height / rows;

    PixelRect::new(
        PLANET_GRID.ulx + (index % columns) * cell_width,
        PLANET_GRID.uly + (index / columns) * cell_height,
        cell_width,
        cell_height,
    )
}

/// Planet by index `0..24` (`Planet::try_from`).
fn planet_for_world(world: i64) -> max_core::enums::Planet {
    crate::ui::planet_from_index(world.clamp(0, (PLANET_COUNT - 1) as i64) as usize)
        .unwrap_or(max_core::enums::Planet::Snowcrab)
}

/// Trims a string by width in pixels (GNW font).
pub fn truncate_to_width(text: &str, max_width: i32, font: &max_render::GnwFont) -> String {
    let mut out = text.to_string();

    while font.text_width(&out) > max_width && out.pop().is_some() {}

    out
}

/// `thickness` frame inside the rectangle.
fn draw_border(frame: &mut RgbaBuffer, rect: PixelRect, color: Rgba, thickness: i32) {
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

/// Draws a big image at `origin`, clipped by `clip`, with the **window palette**.
///
/// C++ `WindowManager_LoadBigImage(..., palette_from_image=false)` does not change
/// the system palette: `P_*` indices are interpreted by the `MAINPIC` palette. For
/// `P_TANK` its own palette differs (index 19 is magenta), so
/// drawing the portrait with its palette is impossible.
/// Draws a big image with the window palette at `origin` (clipped by `clip`).
///
/// This is how C++ draws pictures without their own palette: `palette_from_image =
/// false` → the indices are taken from the window palette (`WindowManager_LoadBigImage`).
/// Needed both by the menu (`PLANETSE`, portrait) and by the M-HS turn-transition window (`HELPFRAM`).
pub fn draw_big_image_at(
    frame: &mut RgbaBuffer,
    image: &BigImage,
    palette: &UiPalette,
    origin: (i32, i32),
    clip: PixelRect,
) -> u32 {
    let offset_x = origin.0;
    let offset_y = origin.1;
    let mut painted = 0;

    for y in 0..image.height as u32 {
        for x in 0..image.width as u32 {
            let px = offset_x + x as i32;
            let py = offset_y + y as i32;

            if !clip.contains(px, py) {
                continue;
            }

            if let Some(index) = image.pixel(x, y) {
                if frame.set(px, py, palette.rgba(index)) {
                    painted += 1;
                }
            }
        }
    }

    painted
}

/// Scales a big image into `rect` (nearest): planet pictures 128×128 into a cell.
///
/// `crop_top` — how many top rows of the picture to skip: for `SNOW_PIC` and
/// the neighboring groups the first 2–10 rows are the service color strip of the resource, not
/// the planet. In the big picture (26,98) it remains as in the original, but in the
/// 8×3 thumbnails it only gets in the way.
fn blit_big_scaled(
    frame: &mut RgbaBuffer,
    image: &BigImage,
    rect: PixelRect,
    crop_top: u32,
) -> u32 {
    if rect.is_empty() || image.width <= 0 || image.height as u32 <= crop_top {
        return 0;
    }

    let source_height = image.height as u32 - crop_top;
    let mut painted = 0;

    for y in 0..rect.height {
        let source_y = crop_top + (y as u32 * source_height / rect.height as u32);

        for x in 0..rect.width {
            let source_x = (x as u32 * image.width as u32 / rect.width as u32) as u32;
            let Some(index) = image.pixel(source_x, source_y) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            let [r, g, b] = image.color(index);

            if frame.set(rect.ulx + x, rect.uly + y, Rgba::rgb(r, g, b)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Top service strip of the planet group picture.
///
/// Measurement on `MAX.RES`: `SNOW_PIC` — 10 rows, `CRTR_PIC` — 3, `GREN_PIC` — 2,
/// `DSRT_PIC` — 9 (the planet itself starts after that).
pub fn planet_picture_crop(planet: max_core::enums::Planet) -> u32 {
    match planet as usize / 6 {
        0 => 10,
        1 => 3,
        2 => 2,
        3 => 9,
        _ => 0,
    }
}

/// Planet grid rectangle in `max_render::ui::Rect` terms.
pub const fn planet_grid_rect() -> Rect {
    Rect::new(
        PLANET_GRID.ulx,
        PLANET_GRID.uly,
        PLANET_GRID.width,
        PLANET_GRID.height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    fn repo_path(relative: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(relative)
    }

    fn test_assets() -> Option<(max_assets::ResArchive, MenuAssets)> {
        let max_res = repo_path("max-original/MAX.RES");

        if !max_res.is_file() {
            return None;
        }

        let archive = max_assets::ResArchive::open(&max_res).ok()?;
        let menu = MenuAssets::load(&archive)?;

        Some((archive, menu))
    }

    #[test]
    fn main_menu_layout_matches_the_cpp_oracle() {
        // `menu.cpp::main_menu`: 6 big buttons 200×29 at x=385 with a step of 35,
        // Exit 100×30 at (435,420).
        for (index, button) in MAIN_MENU_BUTTONS.iter().take(6).enumerate() {
            assert_eq!(
                button.rect,
                PixelRect::new(385, 175 + index as i32 * 35, 200, 29)
            );
            assert!(button.big);
        }

        let exit = MAIN_MENU_BUTTONS[6];
        assert_eq!(exit.rect, PixelRect::new(435, 420, 100, 30));
        assert!(!exit.big);
        assert_eq!(exit.action, MainMenuAction::Quit);

        assert_eq!(MAIN_MENU_BUTTONS[0].label, "New Game");
        assert_eq!(MAIN_MENU_BUTTONS[2].label, "Multiplayer Game");
        assert_eq!(MAIN_MENU_BUTTONS[6].label, "Exit");

        // Bottom row buttons — `planetselectmenu.hpp` / `networkmenu.hpp`.
        assert_eq!(PLANET_BUTTONS[0].rect, PixelRect::new(243, 438, 109, 40));
        assert_eq!(PLANET_BUTTONS[3].rect, PixelRect::new(514, 438, 109, 40));
        assert_eq!(JOIN_LIST, PixelRect::new(359, 28, 209, 160));
        assert_eq!(JOIN_ROW_HEIGHT, 20);
    }

    #[test]
    fn clan_logos_are_drawn_at_their_anchor() {
        // `WindowManager_DecodeSimpleImage`: `ulx -= image->ulx`, `uly -= image->uly`.
        let Some((_archive, menu)) = test_assets() else {
            return;
        };

        let logo = menu.clan_logos[1].as_ref().expect("CLN1LOGO");
        // CLN1LOGO: 74×57, ulx=37, uly=28 — the anchor inside the picture.
        assert_eq!((logo.width, logo.height), (74, 57));
        assert_eq!((logo.ulx, logo.uly), (37, 28));

        let mut frame = RgbaBuffer::filled(640, 480, Rgba::rgb(0, 0, 0));
        blit_simple_anchored(&mut frame, &menu.palette, logo, 41, 40, true);

        // Rectangle of the drawn area: from (41-37, 40-28) = (4, 12) and no wider
        // than the picture — otherwise (as before the fix) the logo went beyond the button.
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for y in 0..480 {
            for x in 0..640 {
                if frame.get(x, y) != Some(Rgba::rgb(0, 0, 0)) {
                    xs.push(x);
                    ys.push(y);
                }
            }
        }

        assert!(!xs.is_empty(), "логотип нарисован");
        assert!(xs.iter().min().copied().unwrap() >= 4);
        assert!(ys.iter().min().copied().unwrap() >= 12);
        assert!(
            xs.iter().max().copied().unwrap() <= 4 + 74 - 1,
            "логотип не должен выходить за 74 px от левого края картинки"
        );
        assert!(ys.iter().max().copied().unwrap() <= 12 + 57 - 1);
    }

    #[test]
    fn quest_caption_is_centered_like_cpp() {
        // `Button::SetCaption` + `Text_TextLine` (`button.cpp:433`, `text.cpp:778`):
        //   uly = ((40 - 16) / 2) = 12; ulx = (48 - (Text_GetWidth+1)) / 2.
        let font = GnwFontId::Font1.font();
        let text_width = font.text_width("?") + 1;
        let expected_x = 465 + (48 - text_width).max(0) / 2;
        let expected_y = 438 + (40 - font.height()).max(0) / 2;

        assert_eq!(expected_x, 484, "знак ? в кнопке 48×40");
        assert_eq!(expected_y, 450);

        let mut frame = RgbaBuffer::filled(640, 480, Rgba::rgb(0, 0, 0));
        draw_text_gnw_centered(
            &mut frame,
            PixelRect::new(465, 438, 48, 40),
            font,
            "?",
            Rgba::WHITE,
        );

        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for y in 438..478 {
            for x in 465..513 {
                if frame.get(x, y) == Some(Rgba::WHITE) {
                    xs.push(x);
                    ys.push(y);
                }
            }
        }

        assert_eq!(xs.iter().min().copied(), Some(expected_x));
        // The '?' "ink" starts from the second row of the cell (ink bbox y0 = 1).
        assert_eq!(ys.iter().min().copied(), Some(expected_y + 1));
        // Horizontally the sign is indeed inside the button, not at the edge.
        assert!(xs.iter().max().copied().unwrap() < 513);
        assert!(ys.iter().max().copied().unwrap() < 478);
    }

    #[test]
    fn menu_assets_load_all_button_images_from_max_res() {
        let Some((_archive, menu)) = test_assets() else {
            return;
        };

        assert_eq!(menu.blank.size(), (200, 29));
        assert_eq!(menu.small_blank.size(), (100, 30));

        for index in 0..6 {
            let pair = &menu.mnubtn[index];
            let expected = if index == 4 { (48, 40) } else { (109, 40) };
            assert_eq!(pair.size(), expected, "MNUBTN{}", index + 1);
        }

        assert_eq!(
            (menu.mainpic.width, menu.mainpic.height),
            (640, 480),
            "MAINPIC — фон главного меню"
        );
        assert_eq!(
            menu.planet_background().width,
            640,
            "PLANETSE или fallback 640×480"
        );
        assert_eq!(menu.join_background().width, 640);
        assert!(menu.portrait.is_some(), "P_* портрет 300×240");
        assert!(menu.btnhide.is_some(), "BTNHIDE 38×21");
        assert!(menu.portrait_frame_left.is_some());
        assert!(menu.portrait_frame_right.is_some());
    }

    #[test]
    fn main_menu_draws_mainpic_title_and_gold_captions() {
        let Some((_archive, menu)) = test_assets() else {
            return;
        };
        let mut frame = RgbaBuffer::new(640, 480);

        draw_main_menu(&mut frame, &menu, "", Some((390, 180)));

        // Background — MAINPIC (0,0 in its palette).
        assert_eq!(
            frame.get(0, 0),
            menu.mainpic.rgb(0, 0).map(Rgba::from_rgb),
            "фон главного меню — MAINPIC"
        );

        let gold = menu.palette.rgba(165);
        let mut gold_pixels = 0;

        for y in 160..460 {
            for x in 380..600 {
                if frame.get(x, y) == Some(gold) {
                    gold_pixels += 1;
                }
            }
        }

        assert!(
            gold_pixels > 50,
            "подписи кнопок — Fonts_GoldColor: {gold_pixels}"
        );

        // Mouse hover does not press the button: the frame with the cursor is bitwise equal to
        // the frame without the cursor (in the original `*D` — only while the button is pressed).
        let mut idle = RgbaBuffer::new(640, 480);
        draw_main_menu(&mut idle, &menu, "", None);
        assert_eq!(
            frame.pixels(),
            idle.pixels(),
            "наведение не должно рисовать нажатую кнопку *D"
        );
    }

    #[test]
    fn create_form_draws_planetse_grid_and_mnubtn_buttons() {
        let Some((_archive, menu)) = test_assets() else {
            return;
        };
        let mut frame = RgbaBuffer::new(640, 480);
        let form = CreateForm {
            world: 12,
            ..Default::default()
        };

        draw_create_form(&mut frame, &menu, None, &form, "status", None);

        // Background — PLANETSE (if it exists in MAX.RES).
        if let Some(ref planetse) = menu.planetse {
            assert_eq!(
                frame.get(600, 300),
                planetse.rgb(600, 300).map(Rgba::from_rgb),
                "фон формы — PLANETSE"
            );
        }

        // Done button — MNUBTN6U at (514,438). The first pixel of the picture —
        // transparent (C++ `Image::Blend`), so we take the frame pixel (2,2).
        let pair = menu.mnubtn_pair(6).expect("MNUBTN6");
        let source = pair
            .pixel(false, 2, 2)
            .map(|index| menu.palette.rgba(index));

        if let Some(source) = source {
            assert_eq!(frame.get(514 + 2, 438 + 2), Some(source));
        }

        // The selected planet is outlined in red.
        let cell = planet_cell(12);
        let red = Rgba::rgb(252, 0, 0);
        assert_eq!(frame.get(cell.ulx, cell.uly), Some(red));
    }

    #[test]
    fn join_list_draws_multgame_frame_rows_and_buttons() {
        let Some((_archive, menu)) = test_assets() else {
            return;
        };
        let games = vec![
            max_net::GameInfo {
                id: "g1".to_string(),
                name: "Alpha".to_string(),
                world: 0,
                status: max_net::GameStatus::Running,
                port: 31554,
                players: 1,
                min_players: 2,
                password_protected: false,
            },
            max_net::GameInfo {
                id: "g2".to_string(),
                name: "Beta".to_string(),
                world: 0,
                status: max_net::GameStatus::Running,
                port: 31554,
                players: 2,
                min_players: 2,
                password_protected: true,
            },
        ];
        let mut frame = RgbaBuffer::new(640, 480);

        draw_join_list(&mut frame, &menu, &games, 1, "2 game(s)", None);

        if let Some(ref multgame) = menu.multgame {
            assert_eq!(
                frame.get(10, 10),
                multgame.rgb(10, 10).map(Rgba::from_rgb),
                "фон списка — MULTGAME"
            );
        }

        // Rows — in the green GNW font.
        let green = menu.palette.rgba(GREEN);
        let mut green_pixels = 0;
        for y in JOIN_LIST.uly..JOIN_LIST.uly + JOIN_LIST.height {
            for x in JOIN_LIST.ulx..JOIN_LIST.ulx + JOIN_LIST.width {
                if frame.get(x, y) == Some(green) {
                    green_pixels += 1;
                }
            }
        }
        assert!(
            green_pixels > 100,
            "строки/разделители списка: {green_pixels}"
        );

        // Ready button — MNUBTN6U (frame at (2,2), the first pixel is transparent).
        let pair = menu.mnubtn_pair(6).expect("MNUBTN6");
        if let Some(source) = pair
            .pixel(false, 2, 2)
            .map(|index| menu.palette.rgba(index))
        {
            assert_eq!(frame.get(514 + 2, 438 + 2), Some(source));
        }

        // BTNHIDE is drawn in the top-right corner.
        let hide = menu.btnhide.as_ref().expect("BTNHIDE");
        if let Some(source) = hide.pixel(5, 5).map(|index| menu.palette.rgba(index)) {
            assert_eq!(
                frame.get(JOIN_HIDE.ulx + 5, JOIN_HIDE.uly + 5),
                Some(source)
            );
        }
    }

    #[test]
    fn hit_testing_follows_the_cpp_rectangles() {
        assert_eq!(
            main_menu_at(385 + 100, 175 + 14),
            Some(MainMenuAction::Create)
        );
        assert_eq!(
            main_menu_at(385 + 100, 245 + 14),
            Some(MainMenuAction::Join)
        );
        assert_eq!(main_menu_at(435 + 50, 420 + 15), Some(MainMenuAction::Quit));
        assert_eq!(main_menu_at(100, 100), None);

        assert_eq!(
            create_button_at(243 + 10, 438 + 10).map(|b| b.action),
            Some(MenuButtonAction::Random)
        );
        assert_eq!(
            create_button_at(514 + 10, 438 + 10).map(|b| b.action),
            Some(MenuButtonAction::Done)
        );
        assert_eq!(create_field_at(48 + 2, 352 + 2), Some(CreateField::Name));
        assert_eq!(create_field_at(426 + 2, 378 + 2), Some(CreateField::World));

        assert_eq!(
            join_button_at(243 + 10, 438 + 10),
            Some(JoinButton::Refresh)
        );
        assert_eq!(join_button_at(354 + 10, 438 + 10), Some(JoinButton::Back));
        assert_eq!(join_button_at(514 + 10, 438 + 10), Some(JoinButton::Join));
        assert_eq!(
            join_button_at(JOIN_HIDE.ulx + 2, JOIN_HIDE.uly + 2),
            Some(JoinButton::Hide)
        );

        // Rows: 8 visible, 20 px.
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 5, 3, 0),
            Some(0)
        );
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 25, 3, 0),
            Some(1)
        );
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 200, 3, 0),
            None
        );
        // Scrolling: with the 9th row selected, 2..=9 are visible, a click on row 0
        // selects the 2nd, on the 7th — the 9th.
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 5, 10, 9),
            Some(2)
        );
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 145, 10, 9),
            Some(9)
        );
    }

    #[test]
    fn seven_by_three_planet_grid_fits_the_planetse_slots() {
        assert_eq!(PLANET_GRID, PixelRect::new(192, 51, 388, 249));
        assert_eq!(planet_cell(0), PixelRect::new(192, 51, 48, 83));
        assert_eq!(planet_cell(7), PixelRect::new(528, 51, 48, 83));
        assert_eq!(planet_cell(23), PixelRect::new(528, 217, 48, 83));

        for index in 0..PLANET_COUNT as i32 {
            let cell = planet_cell(index);
            assert!(cell.intersect(PLANET_GRID) == Some(cell));
        }
    }
}
