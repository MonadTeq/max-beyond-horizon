//! `New Game` submenu — `menu_new_game_menu_loop` (`menu.cpp:2171-2320`) — and its
//! `Tips` window (`menu_draw_tips`, `menu.cpp:1114-1204`).
//!
//! Seven buttons of `new_game_menu_buttons` (`menu.cpp:2181-2189`): Training,
//! Scenario, Custom, Multiplayer Scenario, Campaign, Tips and Cancel. The four
//! mission categories open [`crate::ui::Screen::MissionSelect`]
//! (`GameSetupMenu`), `Custom` opens the existing planet/players setup
//! (`menu_custom_game_menu`, `menu.cpp:2115`), `Tips` toggles the tips window and
//! `Cancel` returns to the main menu.
//!
//! Presentation only: the screen never touches a match. A mission category
//! records the category, and the start goes through
//! [`crate::ui::Transition::StartMission`] →
//! [`crate::App::start_mission_from_registry`] (campaign stage 1).
//!
//! Oracle geometry:
//! * title `_(04b5)` "New Game Menu" — `draw_menu_title` at (236,145) 172×15;
//! * six `BLANK_UP` buttons 200×29 at x=385, y=175 with a 35 px step, `Cancel`
//!   `SBLNK_UP` 100×30 at (435,420) (`MENU_BUTTON_DEF`, `menu.cpp:2182-2188`);
//! * portrait frame `BGRSCRNL`/`BGRSCRNR` at (6,173)/(166,173) and a `P_*` 300×240
//!   picture at (16,182) — the same block as the main menu;
//! * tips text `_(e81f)` wrapped to 296 px in the area (19,185) 296×236
//!   (`menu_draw_tips_frame`), `GNW_TEXT_FONT_5`, one row per 10 px; the page
//!   scrolls with `MNUUAROU`/`MNUDAROU` at (16,438)/(48,438)
//!   (`menu_draw_tips`, `menu.cpp:1156-1167`).
//!
//! One deliberate presentation deviation: the C++ blits the tips text straight over
//! the random `P_*` portrait, which leaves dark text on a dark picture unreadable.
//! Here the text area is cleared to black first (the `BGRSCRN*` frames and the
//! layout are unchanged), so the tips stay legible — the owner's "visible, not a
//! silent fallback" rule for screens that carry information.

use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw, GnwFontId};

use crate::menu::{
    clean_status, draw_big_image, draw_image_button, draw_menu_portrait, draw_title, wrap_gnw_text,
    MenuAssets, BLACK, MAIN_STATUS, TEXT,
};

/// Title rectangle (`draw_menu_title(window, _(04b5))`).
pub const NEW_GAME_TITLE: PixelRect = PixelRect::new(236, 145, 172, 15);

/// `New Game` button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewGameAction {
    /// `Training Missions` (`_(182b)`, `GNW_KB_KEY_SHIFT_T`).
    Training,
    /// `Stand Alone Missions` (`_(02c9)`, `GNW_KB_KEY_SHIFT_S`).
    Scenario,
    /// `Custom Game` (`_(3d29)`, `GNW_KB_KEY_SHIFT_U`) — `menu_custom_game_menu`.
    Custom,
    /// `Custom Scenarios` (`_(884f)`, `GNW_KB_KEY_SHIFT_M`) — `MULTI_PLAYER_SCENARIO`.
    MultiplayerScenario,
    /// `Campaign Game` (`_(878a)`, `GNW_KB_KEY_SHIFT_A`).
    Campaign,
    /// `Tips` (`_(3838)`, `GNW_KB_KEY_SHIFT_I`) — the tips window.
    Tips,
    /// `Cancel` (`_(7eec)`, `GNW_KB_KEY_SHIFT_C`/Esc).
    Cancel,
}

/// One `New Game` button in C++ coordinates.
#[derive(Debug, Clone, Copy)]
pub struct NewGameButton {
    /// Rectangle (`BLANK_UP` 200×29, `SBLNK_UP` 100×30).
    pub rect: PixelRect,
    /// Label from `language.json` (en-US).
    pub label: &'static str,
    /// Action.
    pub action: NewGameAction,
    /// Big (`BLANK_*`) or small (`SBLNK_*`) button.
    pub big: bool,
}

/// The seven buttons (`new_game_menu_buttons`, `menu.cpp:2181-2189`).
///
/// The oracle declares an eighth entry at (16,182) `KCARG0` without a label: it is
/// the portrait hit area (`menu_draw_menu_portrait`), not a menu item, so it is not
/// part of this table.
pub const NEW_GAME_BUTTONS: [NewGameButton; 7] = [
    NewGameButton {
        rect: PixelRect::new(385, 175, 200, 29),
        label: "Training Missions",
        action: NewGameAction::Training,
        big: true,
    },
    NewGameButton {
        rect: PixelRect::new(385, 210, 200, 29),
        label: "Stand Alone Missions",
        action: NewGameAction::Scenario,
        big: true,
    },
    NewGameButton {
        rect: PixelRect::new(385, 245, 200, 29),
        label: "Custom Game",
        action: NewGameAction::Custom,
        big: true,
    },
    NewGameButton {
        rect: PixelRect::new(385, 280, 200, 29),
        label: "Custom Scenarios",
        action: NewGameAction::MultiplayerScenario,
        big: true,
    },
    NewGameButton {
        rect: PixelRect::new(385, 315, 200, 29),
        label: "Campaign Game",
        action: NewGameAction::Campaign,
        big: true,
    },
    NewGameButton {
        rect: PixelRect::new(385, 350, 200, 29),
        label: "Tips",
        action: NewGameAction::Tips,
        big: true,
    },
    NewGameButton {
        rect: PixelRect::new(435, 420, 100, 30),
        label: "Cancel",
        action: NewGameAction::Cancel,
        big: false,
    },
];

/// Tips window text area (`menu_draw_tips_frame`: (19,185) … (315,421)).
pub const TIPS_AREA: PixelRect = PixelRect::new(19, 185, 296, 236);
/// Tips text width (`Text_SplitText(_(e81f), …, 296, …)`).
pub const TIPS_TEXT_WIDTH: i32 = 296;
/// Tips page scroll up (`MNUUAROU` at (16,438)).
pub const TIPS_UP: PixelRect = PixelRect::new(16, 438, 28, 29);
/// Tips page scroll down (`MNUDAROU` at (48,438)).
pub const TIPS_DOWN: PixelRect = PixelRect::new(48, 438, 28, 29);
/// The tips text (`_(e81f)` from `language.json`, `menu.cpp:1152`).
pub const TIPS_TEXT: &str = include_str!("../data/tips.txt");

/// `New Game` button under the point.
pub fn new_game_button_at(x: i32, y: i32) -> Option<NewGameAction> {
    NEW_GAME_BUTTONS
        .iter()
        .find(|button| button.rect.contains(x, y))
        .map(|button| button.action)
}

/// `New Game` button rectangle by action.
pub fn new_game_rect(action: NewGameAction) -> Option<PixelRect> {
    NEW_GAME_BUTTONS
        .iter()
        .find(|button| button.action == action)
        .map(|button| button.rect)
}

/// The `Tips` window state: wrapped rows and the first visible row.
///
/// The C++ scroll animates row by row (`tips_on_click_up/down`); here a click jumps
/// a whole page, which is presentation only — the text and the page geometry are the
/// oracle's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipsWindow {
    rows: Vec<String>,
    row_index: usize,
}

impl Default for TipsWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl TipsWindow {
    /// Wraps the tips text to 296 px (`Text_SplitText`).
    pub fn new() -> Self {
        Self {
            rows: wrap_gnw_text(TIPS_TEXT, TIPS_TEXT_WIDTH),
            row_index: 0,
        }
    }

    /// All wrapped rows.
    pub fn rows(&self) -> &[String] {
        &self.rows
    }

    /// First visible row (`menu_tips_current_row_index`).
    pub fn row_index(&self) -> usize {
        self.row_index
    }

    /// How many rows fit the window (`236 / Text_GetHeight()`).
    pub fn rows_per_page() -> usize {
        (TIPS_AREA.height / GnwFontId::Font5.height()).max(1) as usize
    }

    /// Visible rows at the current page.
    pub fn visible(&self) -> &[String] {
        let end = (self.row_index + Self::rows_per_page()).min(self.rows.len());

        &self.rows[self.row_index.min(end)..end]
    }

    /// Scrolls one page up (`tips_on_click_up`) or down (`tips_on_click_down`).
    pub fn scroll(&mut self, down: bool) {
        let page = Self::rows_per_page();

        if down {
            if self.row_index + page < self.rows.len() {
                self.row_index += page;
            }
        } else {
            self.row_index = self.row_index.saturating_sub(page);
        }
    }

    /// Whether the rows can scroll further (for the QA/unit checks).
    pub fn can_scroll(&self, down: bool) -> bool {
        if down {
            self.row_index + Self::rows_per_page() < self.rows.len()
        } else {
            self.row_index > 0
        }
    }
}

/// Tips page arrow under the point: `Some(true)` — down, `Some(false)` — up.
pub fn tips_scroll_at(x: i32, y: i32) -> Option<bool> {
    if TIPS_DOWN.contains(x, y) {
        Some(true)
    } else if TIPS_UP.contains(x, y) {
        Some(false)
    } else {
        None
    }
}

/// Draws the `New Game` menu (`menu_new_game_menu_loop`).
pub fn draw_new_game_menu(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    tips: Option<&TipsWindow>,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, &menu.mainpic);

    draw_title(frame, palette, NEW_GAME_TITLE, "New Game Menu");
    painted += draw_menu_portrait(frame, menu);

    for button in &NEW_GAME_BUTTONS {
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

    if let Some(tips) = tips {
        painted += draw_tips(frame, menu, tips);
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

/// Draws the tips window over the portrait (`menu_draw_tips_frame`).
fn draw_tips(frame: &mut RgbaBuffer, menu: &MenuAssets, tips: &TipsWindow) -> u32 {
    let palette = &menu.palette;

    // Readability: the portrait behind the text is a random `P_*` picture
    // (module docs), so the text panel gets a black backing.
    frame.fill_rect(TIPS_AREA, Rgba::BLACK);

    let font = GnwFontId::Font5.font();
    let mut painted = 0u32;

    for (row, line) in tips.visible().iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        painted += draw_text_gnw(
            frame,
            TIPS_AREA.ulx,
            TIPS_AREA.uly + row as i32 * GnwFontId::Font5.height(),
            font,
            line,
            palette.rgba(TEXT),
        )
        .max(0) as u32;
    }

    // Page arrows (`MNUUAROU`/`MNUDAROU` at (16,438)/(48,438)).
    painted += draw_tips_arrow(frame, menu, menu.arrow_up.as_ref(), TIPS_UP);
    painted += draw_tips_arrow(frame, menu, menu.arrow_down.as_ref(), TIPS_DOWN);

    painted
}

/// One page arrow; a missing resource simply leaves the button area empty.
fn draw_tips_arrow(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    pair: Option<&crate::menu::MenuButtonPair>,
    rect: PixelRect,
) -> u32 {
    match pair {
        Some(pair) => crate::menu::blit_simple(
            frame,
            &menu.palette,
            pair.image(false),
            rect.ulx,
            rect.uly,
            true,
        ),
        None => 0,
    }
}
