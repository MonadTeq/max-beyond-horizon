//! Help screen `HelpMenu` — C++ `helpmenu.cpp` + `help.json`/`language.json`.
//!
//! # Oracle
//!
//! * `HELPFRAM` window 301×231: `Window(HELPFRAM, GameManager_GetDialogWindowCenterMode())`,
//!   i.e. in game/`CargoMenu` — centered in the map area `(253,126)`, in the setup
//!   menu (`GameConfigMenu`/`PlanetSelectMenu`/`ChoosePlayerMenu`) — centered
//!   in the window `(169,124)`;
//! * text — `GNW_TEXT_FONT_5` (height 10), `GNW_TEXT_OUTLINE | 0xFF` in the area
//!   `Image(20, 20, 265, 160)`; `rows_per_page = 160 / 10 = 16`; if the text
//!   is shorter than a page, the block is centered vertically (`DrawText`);
//! * scrolling — `PageUp`/`PageDown` and the `BLDUP__U`/`BLDDWN_U` buttons 18×17 at
//!   (20,197)/(40,197) with a step of `rows_per_page` lines; the buttons are created only
//!   if there are more lines than a page;
//! * `Done` — `HELPOK_U/D` 65×24 at (120,193), label `_(4bfa)`, the keys
//!   `Enter`/`Esc` close the window; for the `GAME_SCREEN_SETUP` section C++ has
//!   a second `Keys` button (`_(f0d6)`) at (155,193), and `Done` is shifted to (85,193):
//!   it switches the text to the key hint `_(ceb6)`;
//! * C++ enters the "help mode" (`HelpMenu_Menu`): the player clicks an element
//!   of the screen, and the window shows **one** paragraph bound to the coordinates.
//!
//! # What the Rust client does
//!
//! The C++ mode is ported (`HelpMenu_Menu`): the `?`/`Help` button enables the
//! element selection mode (`HelpMode`), a click on an element converts the point
//! `HelpMenu_ScaleCursor` (`helpmenu.cpp:412`) and takes the `help.json` entry
//! (`Help::GetEntry`, `help.cpp:140`) — the window shows **one** paragraph, as in
//! the original (not the whole section text). Scrolling, the `BLDUP__/BLDDWN` buttons,
//! `Done`/`Keys` — 1:1; `Esc` closes the window and returns to the mode, a repeated
//! `Esc`/RMB/`?` disables the mode. Details — §33
//! `max-brh-docs/analysis/visual-1to1-status.md`.
//!
//! The window palette is the `HELPFRAM` own palette (like the other big images
//! of the Rust client); the C++ port draws `HELPFRAM` with the system palette, but the indices
//! of the UI colors are the same in it.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use max_assets::image::{BigImage, SimpleImage};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{
    draw_text_gnw, draw_text_gnw_centered_shadowed, draw_text_gnw_shadowed, GnwFontId,
};
use serde::Deserialize;

use crate::menu::{
    blit_simple, clean_status, draw_big_image_at, draw_image_button, wrap_gnw_text, MenuButtonPair,
};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// JSON with the help texts (generator `max-brh/tools/gen_help_data.py`).
pub const HELP_JSON: &str = include_str!("../data/help.json");

/// `HELPFRAM` 301×231 (`Window(HELPFRAM, …)`).
pub const HELPFRAM_SIZE: (i32, i32) = (301, 231);
/// Help window in the setup menu: `((639-301)/2, (479-231)/2)`.
pub const MENU_RECT: PixelRect = PixelRect::new(169, 124, 301, 231);
/// Help window in game/mobilization: center of `WINDOW_MAIN_MAP` (180,18,448,448).
pub const GAME_RECT: PixelRect = PixelRect::new(253, 126, 301, 231);
/// Text area: `Image(20, 20, 265, 160)` (`HelpMenu` constructor).
pub const TEXT_RECT: PixelRect = PixelRect::new(20, 20, 265, 160);
/// `rows_per_page = 160 / Text_GetHeight()`; the `FONT5` height is 10.
pub const ROWS_PER_PAGE: usize = 16;
/// Text color: `GNW_TEXT_OUTLINE | 0xFF`.
pub const TEXT_COLOR: u8 = 0xFF;
/// Outline/shadow color of `Text_Blit` with outline — black (index 0).
pub const OUTLINE_COLOR: u8 = 0x00;
/// `Done` (not `GAME_SCREEN_SETUP`): `Button(HELPOK_U, HELPOK_D, 120, 193)`.
pub const DONE_RECT: PixelRect = PixelRect::new(120, 193, 65, 24);
/// `Done` for `GAME_SCREEN_SETUP`: (85,193).
pub const DONE_KEYS_RECT: PixelRect = PixelRect::new(85, 193, 65, 24);
/// `Keys` for `GAME_SCREEN_SETUP`: `Button(HELPOK_U, HELPOK_D, 155, 193)`.
pub const KEYS_RECT: PixelRect = PixelRect::new(155, 193, 65, 24);
/// Scroll up: `Button(BLDUP__U, BLDUP__D, 20, 197)`.
pub const SCROLL_UP_RECT: PixelRect = PixelRect::new(20, 197, 18, 17);
/// Scroll down: `Button(BLDDWN_U, BLDDWN_D, 40, 197)`.
pub const SCROLL_DOWN_RECT: PixelRect = PixelRect::new(40, 197, 18, 17);

/// Help section (the `section` value in `HelpMenu_Menu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpSection {
    /// `CARGO_SETUP` (`CargoMenu::ProcessKey`, the `?` key).
    Cargo,
    /// `PLANET_SETUP` (`PlanetSelectMenu::EventHelp`).
    Planet,
    /// `NEW_GAME_SETUP` (`ChoosePlayerMenu::EventHelp`, single game).
    NewGame,
    /// `HOT_SEAT_SETUP` (`ChoosePlayerMenu::EventHelp`, hot-seat).
    HotSeat,
    /// `OPTIONS_SETUP` (`GameConfigMenu::EventHelp`).
    Options,
    /// `CLAN_SETUP` (`ClanSelectMenu::EventHelp`).
    Clan,
    /// `RESEARCH_SETUP` (`ResearchMenu::OnClick_Help`).
    Research,
    /// `UPGRADES_SETUP` (`UpgradeMenu::ProcessKey`).
    Upgrades,
    /// `GAME_SCREEN_SETUP` (the `HELP_OF` panel, `GameManager_MenuClickHelpButton`).
    GameScreen,
    /// `ALLOCATE_SETUP` (`AllocMenu::Run`, the `?`/`Shift-/` button).
    Allocate,
    /// `TRANSFER_SETUP` (`TransferMenu::ProcessKey`, `Shift-/`).
    Transfer,
    /// `PREFS_MENU_SETUP` (`OptionsMenu::ProcessKeyPress`, the `PREFSPIC` variant).
    Preferences,
    /// `SETUP_MENU_SETUP` (`OptionsMenu::ProcessKeyPress`, the `SETUPPIC` variant).
    Setup,
}

impl HelpSection {
    /// Section name in `help.json`/C++.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "CARGO_SETUP",
            Self::Planet => "PLANET_SETUP",
            Self::NewGame => "NEW_GAME_SETUP",
            Self::HotSeat => "HOT_SEAT_SETUP",
            Self::Options => "OPTIONS_SETUP",
            Self::Clan => "CLAN_SETUP",
            Self::Research => "RESEARCH_SETUP",
            Self::Upgrades => "UPGRADES_SETUP",
            Self::GameScreen => "GAME_SCREEN_SETUP",
            Self::Allocate => "ALLOCATE_SETUP",
            Self::Transfer => "TRANSFER_SETUP",
            Self::Preferences => "PREFS_MENU_SETUP",
            Self::Setup => "SETUP_MENU_SETUP",
        }
    }

    /// Parsing of the short name (QA/`help cargo`).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "cargo" | "cargo_setup" => Some(Self::Cargo),
            "planet" | "planet_setup" => Some(Self::Planet),
            "newgame" | "new_game" | "new_game_setup" | "players" => Some(Self::NewGame),
            "hotseat" | "hot_seat" | "hot_seat_setup" => Some(Self::HotSeat),
            "options" | "options_setup" => Some(Self::Options),
            "clan" | "clan_setup" => Some(Self::Clan),
            "research" | "research_setup" => Some(Self::Research),
            "upgrades" | "upgrades_setup" => Some(Self::Upgrades),
            "game" | "game_screen" | "game_screen_setup" => Some(Self::GameScreen),
            "allocate" | "allocate_setup" => Some(Self::Allocate),
            "transfer" | "transfer_setup" | "xfer" => Some(Self::Transfer),
            "preferences" | "prefs" | "prefs_menu_setup" => Some(Self::Preferences),
            "setup_menu" | "setup_menu_setup" | "setuphelp" => Some(Self::Setup),
            _ => None,
        }
    }
}

/// Where the window is opened: changes only the position (`GameManager_GetDialogWindowCenterMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpPlacement {
    /// Setup/main menu — `(169,124)`.
    Menu,
    /// Game/`CargoMenu` — `(253,126)`.
    Game,
}

impl HelpPlacement {
    /// Window rectangle in the 640×480 frame.
    pub const fn rect(self) -> PixelRect {
        match self {
            Self::Menu => MENU_RECT,
            Self::Game => GAME_RECT,
        }
    }

    /// Position for `Screen::LocalSetup`/`CargoMenu`/the game.
    pub const fn for_screen(in_game: bool) -> Self {
        if in_game {
            Self::Game
        } else {
            Self::Menu
        }
    }
}

/// Help window action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpAction {
    /// The window stays open.
    None,
    /// `Done`/`Enter`/`Esc` — close.
    Close,
}

// ---------------------------------------------------------------------------
// help.json data
// ---------------------------------------------------------------------------

/// One help paragraph/entry from `help.json`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HelpEntry {
    /// Entry name (`UNIT_PICT`, `CANCEL_BUTTON`, …).
    pub name: String,
    /// `coordinates.ux` (C++ help cursor hit test; needed by the Rust client for tests).
    pub ux: i32,
    /// `coordinates.uy`.
    pub uy: i32,
    /// `coordinates.lx`.
    pub lx: i32,
    /// `coordinates.ly`.
    pub ly: i32,
    /// Text `en-US`.
    pub text: String,
}

#[derive(Debug, Deserialize)]
struct HelpFile {
    keys: String,
    sections: BTreeMap<String, Vec<HelpEntry>>,
}

/// Loaded `help.json` (once).
fn help_file() -> &'static HelpFile {
    static FILE: OnceLock<HelpFile> = OnceLock::new();

    FILE.get_or_init(|| {
        serde_json::from_str::<HelpFile>(HELP_JSON)
            .unwrap_or_else(|error| panic!("data/help.json сгенерирован некорректно: {error}"))
    })
}

/// Section entries (empty if the section is not in `help.json`).
pub fn section_entries(section: HelpSection) -> &'static [HelpEntry] {
    help_file()
        .sections
        .get(section.as_str())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Entry index by name (`UNIT_PICT`, …).
pub fn entry_by_name(section: HelpSection, name: &str) -> Option<usize> {
    let wanted = name.trim();

    section_entries(section)
        .iter()
        .position(|entry| entry.name.eq_ignore_ascii_case(wanted))
}

/// Name of entry `index`.
pub fn entry_name(section: HelpSection, index: usize) -> Option<&'static str> {
    section_entries(section)
        .get(index)
        .map(|entry| entry.name.as_str())
}

/// Text of entry `index` (en-US).
pub fn entry_text(section: HelpSection, index: usize) -> &'static str {
    section_entries(section)
        .get(index)
        .map(|entry| entry.text.as_str())
        .unwrap_or("")
}

/// `HelpMenu_ScaleCursor` (`helpmenu.cpp:412-471`): screen click point →
/// coordinates in the `help.json` system.
///
/// For the setup/`CargoMenu` windows (`WINDOW_MAIN_WINDOW`) the coordinates coincide with
/// the frame. In game (`WINDOW_MAIN_MAP`) C++ first converts clicks on
/// the tools into fixed entry points (`COORDINATES` → 250/297,473;
/// `UNIT_DESCRIPTION` → 448,473; `TURN_COUNTER` → 498,11; `TURN_TIMER` →
/// 565,11; `ENDTURN_BUTTON` → 426,11; the top-right tool → 610,11), and
/// leaves clicks on the map as is (in the original 640×480 scale).
pub fn scale_cursor(section: HelpSection, x: i32, y: i32) -> (i32, i32) {
    if section != HelpSection::GameScreen {
        return (x, y);
    }

    let coordinates = max_render::layout::COORDINATES;
    let description = max_render::layout::UNIT_DESCRIPTION;
    let counter = max_render::layout::TURN_COUNTER;
    let timer = max_render::layout::TURN_TIMER;
    let end_turn = max_render::layout::END_TURN_BUTTON;
    let top_instrument = PixelRect::new(timer.lrx() + 5, timer.uly, 20, timer.height);

    if coordinates.contains(x, y) {
        let offset = (coordinates.width * 20) / 100 + coordinates.ulx;

        return if x < offset { (250, 473) } else { (297, 473) };
    }

    if description.contains(x, y) {
        return (448, 473);
    }

    if counter.contains(x, y) {
        return (498, 11);
    }

    if timer.contains(x, y) {
        return (565, 11);
    }

    if end_turn.contains(x, y) {
        return (426, 11);
    }

    if top_instrument.contains(x, y) {
        return (610, 11);
    }

    (x, y)
}

/// Section entry under the frame point — `Help::GetEntry` (`help.cpp:140-167`):
/// the first entry whose coordinates cover the point (the order is the file order).
pub fn entry_at(section: HelpSection, x: i32, y: i32) -> Option<usize> {
    let (x, y) = scale_cursor(section, x, y);

    section_entries(section).iter().position(|entry| {
        x >= entry.ux && x <= entry.lx && y >= entry.uy && y <= entry.ly
    })
}

/// Help mode — C++ `HelpMenu_Menu` (`helpmenu.cpp:315`): the cursor `CURSOR_HELP`,
/// a click on an element opens the window with the entry under the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpMode {
    /// Section.
    pub section: HelpSection,
    /// Where the mode is opened (menu/game) — for the window position.
    pub placement: HelpPlacement,
}

impl HelpMode {
    /// Mode for a section.
    pub const fn new(section: HelpSection, placement: HelpPlacement) -> Self {
        Self { section, placement }
    }

    /// Click on an element: `None` — no entry (the mode stays, as in C++).
    pub fn click(&self, x: i32, y: i32) -> Option<HelpScreen> {
        let index = entry_at(self.section, x, y)?;

        Some(HelpScreen::for_entry(self.section, self.placement, index))
    }
}

/// Section text: all non-empty paragraphs separated by an empty line.
pub fn section_text(section: HelpSection) -> String {
    section_entries(section)
        .iter()
        .map(|entry| entry.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Key hint text (`_(ceb6)`, the `Keys` button).
pub fn keys_text() -> &'static str {
    &help_file().keys
}

/// How many lines the section occupies at the window width of 265 px.
pub fn section_line_count(section: HelpSection) -> usize {
    wrap_gnw_text(&section_text(section), TEXT_RECT.width).len()
}

// ---------------------------------------------------------------------------
// Screen
// ---------------------------------------------------------------------------

/// Open help window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpScreen {
    section: HelpSection,
    placement: HelpPlacement,
    keys_mode: bool,
    lines: Vec<String>,
    row_index: usize,
    /// Index of the shown `help.json` entry.
    entry: usize,
}

impl HelpScreen {
    /// Window for the first section entry (a safeguard for calls without coordinates).
    pub fn new(section: HelpSection, placement: HelpPlacement) -> Self {
        Self::for_entry(section, placement, 0)
    }

    /// Window for entry `index` — C++ `HelpMenu(section, cursor_x, cursor_y)`
    /// shows **one** entry (`ResourceManager_GetHelpEntry`).
    pub fn for_entry(section: HelpSection, placement: HelpPlacement, index: usize) -> Self {
        let text = entry_text(section, index);

        Self {
            section,
            placement,
            keys_mode: false,
            lines: wrap_gnw_text(text, TEXT_RECT.width),
            row_index: 0,
            entry: index,
        }
    }

    /// Window for the entry under the point; `None` — no entry (C++ closes the window).
    pub fn for_point(
        section: HelpSection,
        placement: HelpPlacement,
        x: i32,
        y: i32,
    ) -> Option<Self> {
        let index = entry_at(section, x, y)?;

        Some(Self::for_entry(section, placement, index))
    }

    /// Section.
    pub const fn section(&self) -> HelpSection {
        self.section
    }

    /// Entry index.
    pub const fn entry(&self) -> usize {
        self.entry
    }

    /// Entry name (`UNIT_PICT`, …).
    pub fn entry_name(&self) -> Option<&'static str> {
        entry_name(self.section, self.entry)
    }

    /// Window position.
    pub const fn placement(&self) -> HelpPlacement {
        self.placement
    }

    /// Whether the key hint is shown (`GAME_SCREEN_SETUP`).
    pub const fn keys_mode(&self) -> bool {
        self.keys_mode
    }

    /// First visible line (0-based).
    pub const fn row_index(&self) -> usize {
        self.row_index
    }

    /// Total number of lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Text lines (for tests).
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Whether there is anything to scroll (`string_row_count > rows_per_page`).
    pub fn is_scrollable(&self) -> bool {
        self.lines.len() > ROWS_PER_PAGE
    }

    /// Window rectangle in the frame.
    pub fn window_rect(&self) -> PixelRect {
        self.placement.rect()
    }

    /// Text area in the frame.
    pub fn text_rect(&self) -> PixelRect {
        let window = self.window_rect();

        PixelRect::new(
            window.ulx + TEXT_RECT.ulx,
            window.uly + TEXT_RECT.uly,
            TEXT_RECT.width,
            TEXT_RECT.height,
        )
    }

    /// `Done` button in the frame (for `GAME_SCREEN_SETUP` — shifted left).
    pub fn done_rect(&self) -> PixelRect {
        let rect = if self.section == HelpSection::GameScreen {
            DONE_KEYS_RECT
        } else {
            DONE_RECT
        };
        let window = self.window_rect();

        PixelRect::new(
            window.ulx + rect.ulx,
            window.uly + rect.uly,
            rect.width,
            rect.height,
        )
    }

    /// `Keys` button in the frame (`GAME_SCREEN_SETUP`).
    pub fn keys_rect(&self) -> Option<PixelRect> {
        if self.section != HelpSection::GameScreen {
            return None;
        }
        let window = self.window_rect();

        Some(PixelRect::new(
            window.ulx + KEYS_RECT.ulx,
            window.uly + KEYS_RECT.uly,
            KEYS_RECT.width,
            KEYS_RECT.height,
        ))
    }

    /// Scroll buttons in the frame (if the text is longer than a page).
    pub fn scroll_rects(&self) -> Option<(PixelRect, PixelRect)> {
        if !self.is_scrollable() {
            return None;
        }
        let window = self.window_rect();

        Some((
            PixelRect::new(
                window.ulx + SCROLL_UP_RECT.ulx,
                window.uly + SCROLL_UP_RECT.uly,
                SCROLL_UP_RECT.width,
                SCROLL_UP_RECT.height,
            ),
            PixelRect::new(
                window.ulx + SCROLL_DOWN_RECT.ulx,
                window.uly + SCROLL_DOWN_RECT.uly,
                SCROLL_DOWN_RECT.width,
                SCROLL_DOWN_RECT.height,
            ),
        ))
    }

    /// Line `row` after `row_index` (within `rows_per_page`).
    pub fn visible_lines(&self) -> impl Iterator<Item = &String> {
        self.lines.iter().skip(self.row_index).take(ROWS_PER_PAGE)
    }

    /// Toggles the `Keys` text ↔ the section text — the `Keys` button
    /// (`HelpMenu::ProcessKey`, `case 1000`).
    pub fn toggle_keys(&mut self) {
        if self.section != HelpSection::GameScreen {
            return;
        }
        self.keys_mode = !self.keys_mode;
        let text = if self.keys_mode {
            keys_text().to_string()
        } else {
            entry_text(self.section, self.entry).to_string()
        };
        self.lines = wrap_gnw_text(&text, TEXT_RECT.width);
        self.row_index = 0;
    }

    /// `PageUp`: `row_index -= rows_per_page`, not below 0 (C++ animates line
    /// by line, the result is the same).
    pub fn page_up(&mut self) {
        if self.row_index == 0 {
            return;
        }
        self.row_index = self.row_index.saturating_sub(ROWS_PER_PAGE);
    }

    /// `PageDown`: C++ increases `row_index` by a page only while
    /// `row_index + rows_per_page < row_count`.
    pub fn page_down(&mut self) {
        if self.row_index + ROWS_PER_PAGE < self.lines.len() {
            self.row_index += ROWS_PER_PAGE;
        }
    }

    /// One line up/down (↑/↓ keys; in C++ only pages).
    pub fn scroll(&mut self, delta: i32) {
        let max = self.lines.len().saturating_sub(1);
        let next = (self.row_index as i64 + i64::from(delta)).clamp(0, max as i64);

        self.row_index = next as usize;
    }

    /// Window key (`HelpMenu::ProcessKey`).
    pub fn key(&mut self, key: Key) -> HelpAction {
        match key {
            Key::Escape | Key::Enter => HelpAction::Close,
            Key::PageUp => {
                self.page_up();
                HelpAction::None
            }
            Key::PageDown => {
                self.page_down();
                HelpAction::None
            }
            Key::Up => {
                self.scroll(-1);
                HelpAction::None
            }
            Key::Down => {
                self.scroll(1);
                HelpAction::None
            }
            Key::Char('k' | 'K') if self.section == HelpSection::GameScreen => {
                self.toggle_keys();
                HelpAction::None
            }
            Key::Char('?') => HelpAction::Close,
            _ => HelpAction::None,
        }
    }

    /// Input event (click/key). The mouse moves without effect.
    pub fn input(&mut self, event: InputEvent) -> HelpAction {
        match event {
            InputEvent::Key(key) => self.key(key),
            InputEvent::MouseClick(x, y) => self.click(x, y),
            _ => HelpAction::None,
        }
    }

    /// Click on the window in 640×480 frame coordinates.
    pub fn click(&mut self, x: i32, y: i32) -> HelpAction {
        if self.done_rect().contains(x, y) {
            return HelpAction::Close;
        }
        if self.keys_rect().is_some_and(|rect| rect.contains(x, y)) {
            self.toggle_keys();
            return HelpAction::None;
        }
        if let Some((up, down)) = self.scroll_rects() {
            if up.contains(x, y) {
                self.page_up();
            } else if down.contains(x, y) {
                self.page_down();
            }
        }

        HelpAction::None
    }

    /// Draws the window over the frame (C++ `HelpMenu::DrawText`).
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        assets: Option<&HelpMenuAssets>,
        fallback: Option<UiPalette>,
    ) -> u32 {
        let window = self.window_rect();
        let palette = assets
            .and_then(|assets| assets.background.as_ref())
            .map(UiPalette::from_big_image)
            .or(fallback);

        let Some(palette) = palette else {
            return 0;
        };
        let font = GnwFontId::Font5.font();
        let mut painted = 0;

        if let Some(background) = assets.and_then(|assets| assets.background.as_ref()) {
            painted += draw_big_image_at(
                frame,
                background,
                &palette,
                (window.ulx, window.uly),
                window,
            );
        } else {
            frame.fill_rect(window, palette.rgba(0x22));
            painted += 1;
        }

        // An empty paragraph separator line (`\n\n`) takes space, as in C++.
        let text = self.text_rect();
        let block = font.height() * self.visible_lines().count() as i32;
        let mut y = text.uly;

        if self.line_count() <= ROWS_PER_PAGE {
            // C++: `if (text_full_height < 160) buffer += width * ((160 - height)/2)`.
            y += (TEXT_RECT.height - block).max(0) / 2;
        }

        for line in self.visible_lines() {
            painted += draw_text_gnw_shadowed(
                frame,
                text.ulx,
                y,
                font,
                line,
                palette.rgba(TEXT_COLOR),
                palette.rgba(OUTLINE_COLOR),
                palette.rgba(OUTLINE_COLOR),
            )
            .max(0) as u32;
            y += font.height();
        }

        // Scroll buttons — only when there are more lines than a page.
        if let Some((up, down)) = self.scroll_rects() {
            if let Some(pair) = assets.and_then(|assets| assets.up.as_ref()) {
                painted += blit_simple(frame, &palette, pair.image(false), up.ulx, up.uly, true);
            }
            if let Some(pair) = assets.and_then(|assets| assets.down.as_ref()) {
                painted +=
                    blit_simple(frame, &palette, pair.image(false), down.ulx, down.uly, true);
            }
        }

        // `Done` (`_(4bfa)` = "Done").
        let done = self.done_rect();
        if let Some(pair) = assets.and_then(|assets| assets.done.as_ref()) {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                done,
                "Done",
                false,
            );
        } else {
            frame.fill_rect(done, palette.rgba(0x17));
            painted += 1;
            painted += draw_text_gnw_centered_shadowed(
                frame,
                done,
                font,
                "Done",
                palette.rgba(165),
                palette.rgba(175),
                palette.rgba(199),
            )
            .max(0) as u32;
        }

        // `Keys` (`_(f0d6)`) — only `GAME_SCREEN_SETUP`.
        if let Some(keys) = self.keys_rect() {
            if let Some(pair) = assets.and_then(|assets| assets.done.as_ref()) {
                painted += draw_image_button(
                    frame,
                    &palette,
                    pair.image(false),
                    pair.image(true),
                    keys,
                    "Keys",
                    false,
                );
            } else {
                frame.fill_rect(keys, palette.rgba(0x17));
                painted += 1;
                painted += draw_text_gnw_centered_shadowed(
                    frame,
                    keys,
                    font,
                    "Keys",
                    palette.rgba(165),
                    palette.rgba(175),
                    palette.rgba(199),
                )
                .max(0) as u32;
            }
        }

        // Status line of the Rust client: C++ does not have it, but it helps
        // distinguish "no text in help.json" from an empty window.
        if self.lines.is_empty() {
            painted += draw_text_gnw(
                frame,
                text.ulx,
                text.uly,
                font,
                &clean_status("No help text for this section"),
                palette.rgba(0x05),
            )
            .max(0) as u32;
        }

        painted
    }
}

/// Help window assets (`HELPFRAM` + `HELPOK` + `BLDUP__/BLDDWN`).
#[derive(Debug, Clone)]
pub struct HelpMenuAssets {
    /// `HELPFRAM` background 301×231.
    pub background: Option<BigImage>,
    /// `HELPOK_U/D` button 65×24 (Done/Keys).
    pub done: Option<MenuButtonPair>,
    /// `BLDUP__U/D` 18×17.
    pub up: Option<MenuButtonPair>,
    /// `BLDDWN_U/D` 18×17.
    pub down: Option<MenuButtonPair>,
}

impl HelpMenuAssets {
    /// Loads resources; a missing part is a warning, not an error.
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<SimpleImage> {
            let bytes = archive.read(name).ok()?;
            SimpleImage::decode(bytes).ok()
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };

        Self {
            background: archive
                .read("HELPFRAM")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            done: pair("HELPOK_U", "HELPOK_D"),
            up: pair("BLDUP__U", "BLDUP__D"),
            down: pair("BLDDWN_U", "BLDDWN_D"),
        }
    }

    /// Whether the background and the `Done` button are present.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.done.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry with the longest text — for the scrolling check.
    fn longest_entry(section: HelpSection) -> HelpScreen {
        let index = section_entries(section)
            .iter()
            .enumerate()
            .max_by_key(|(_, entry)| entry.text.len())
            .map(|(index, _)| index)
            .unwrap_or(0);

        HelpScreen::for_entry(section, HelpPlacement::Menu, index)
    }

    #[test]
    fn four_help_screens_have_text() {
        for section in [
            HelpSection::Cargo,
            HelpSection::Planet,
            HelpSection::NewGame,
            HelpSection::Options,
        ] {
            let text = section_text(section);
            assert!(!text.is_empty(), "{section:?}: пустой текст");

            let screen = HelpScreen::new(section, HelpPlacement::Menu);
            assert!(screen.line_count() > 1, "{section:?}: нет строк");
            assert!(screen.entry_name().is_some());
        }
    }

    #[test]
    fn help_shows_one_entry_per_click_not_the_whole_section() {
        // All sections have entries with coordinates: the window shows exactly one.
        for section in [
            HelpSection::Cargo,
            HelpSection::Planet,
            HelpSection::NewGame,
            HelpSection::Options,
            HelpSection::GameScreen,
            HelpSection::Research,
            HelpSection::Upgrades,
            HelpSection::Allocate,
            HelpSection::Transfer,
            HelpSection::Preferences,
            HelpSection::Setup,
            HelpSection::Clan,
            HelpSection::HotSeat,
        ] {
            let entries = section_entries(section);
            assert!(!entries.is_empty(), "{section:?}: нет записей в help.json");

            let screen = HelpScreen::new(section, HelpPlacement::Menu);
            let entry = &entries[0];

            assert_eq!(screen.entry(), 0);
            assert_eq!(
                screen.lines(),
                wrap_gnw_text(&entry.text, TEXT_RECT.width),
                "{section:?}: окно должно показывать только первую запись"
            );
        }
    }

    #[test]
    fn entry_is_selected_by_point_like_cpp_get_entry() {
        // `Help::GetEntry`: the first entry whose coordinates cover the point.
        let entries = section_entries(HelpSection::Options);

        for (index, entry) in entries.iter().enumerate() {
            let x = (entry.ux + entry.lx) / 2;
            let y = (entry.uy + entry.ly) / 2;

            assert_eq!(
                entry_at(HelpSection::Options, x, y),
                Some(index),
                "{:?}: центр записи должен выбирать её",
                entry.name
            );
        }

        // A point outside all entries — `None` (C++ closes the window).
        assert_eq!(entry_at(HelpSection::Options, 0, 0), None);
        assert_eq!(entry_at(HelpSection::Cargo, 639, 479), None);
    }

    #[test]
    fn game_screen_panels_map_to_their_entries() {
        // `HelpMenu_ScaleCursor`: clicks on the tools are converted into
        // `GAME_SCREEN_SETUP` entry points.
        assert_eq!(
            entry_at(HelpSection::GameScreen, 242 + 10, 456 + 10).map(entry_name_of),
            entry_at(HelpSection::GameScreen, 250, 473).map(entry_name_of)
        );
        assert_eq!(
            entry_at(HelpSection::GameScreen, 338 + 10, 456 + 10).map(entry_name_of),
            entry_at(HelpSection::GameScreen, 448, 473).map(entry_name_of)
        );
        assert_eq!(
            entry_at(HelpSection::GameScreen, 534 + 10, 2 + 10).map(entry_name_of),
            entry_at(HelpSection::GameScreen, 565, 11).map(entry_name_of)
        );
        assert_eq!(
            entry_at(HelpSection::GameScreen, 390 + 10, 2 + 10).map(entry_name_of),
            entry_at(HelpSection::GameScreen, 426, 11).map(entry_name_of)
        );

        // A click on the map is not converted: the map is the common `MAIN_MAP` entry.
        assert_eq!(
            entry_at(HelpSection::GameScreen, 300, 200).map(entry_name_of),
            entry_at(HelpSection::GameScreen, 300, 200).map(entry_name_of)
        );
    }

    fn entry_name_of(index: usize) -> Option<&'static str> {
        entry_name(HelpSection::GameScreen, index)
    }

    #[test]
    fn entries_show_their_own_text_and_scroll_only_when_longer_than_a_page() {
        // The window shows exactly the entry text; scroll buttons — only if
        // there are more lines than a page (C++ `ProcessText`: `string_row_count >
        // rows_per_page`).
        for section in [
            HelpSection::Cargo,
            HelpSection::Planet,
            HelpSection::NewGame,
            HelpSection::Options,
            HelpSection::GameScreen,
            HelpSection::Research,
            HelpSection::Upgrades,
            HelpSection::Preferences,
            HelpSection::Allocate,
            HelpSection::Transfer,
            HelpSection::Clan,
            HelpSection::HotSeat,
        ] {
            for index in 0..section_entries(section).len() {
                let screen = HelpScreen::for_entry(section, HelpPlacement::Menu, index);
                let expected =
                    wrap_gnw_text(entry_text(section, index), TEXT_RECT.width).len();

                assert_eq!(screen.line_count(), expected);
                assert_eq!(screen.is_scrollable(), expected > ROWS_PER_PAGE);
                assert_eq!(screen.scroll_rects().is_some(), expected > ROWS_PER_PAGE);
            }
        }

        // And the `Keys` hint (`_(ceb6)`) is longer than a page and scrolls.
        let mut keys = HelpScreen::new(HelpSection::GameScreen, HelpPlacement::Game);
        keys.toggle_keys();
        assert!(keys.is_scrollable());

        assert_eq!(keys.key(Key::PageDown), HelpAction::None);
        assert_eq!(keys.row_index(), ROWS_PER_PAGE);
        assert_eq!(keys.key(Key::PageUp), HelpAction::None);
        assert_eq!(keys.row_index(), 0);

        for _ in 0..100 {
            keys.page_down();
        }
        assert!(keys.row_index() < keys.line_count());
    }

    #[test]
    fn short_text_is_centered_by_the_draw_rule() {
        // C++ `DrawText`: the block is centered vertically while the height < 160.
        let mut planet = HelpScreen::for_entry(HelpSection::Planet, HelpPlacement::Menu, 0);
        planet.lines = vec!["line".to_string(); 5];
        planet.row_index = 0;

        let block = GnwFontId::Font5.font().height() * planet.line_count() as i32;
        let offset = (TEXT_RECT.height - block).max(0) / 2;

        assert!(!planet.is_scrollable());
        assert!(offset > 0);
    }

    #[test]
    fn window_is_centered_like_dialog_menu() {
        // `Window(HELPFRAM, WINDOW_MAIN_WINDOW)`; the window is 639×479 (lrx/lry inclusive).
        assert_eq!(MENU_RECT.ulx, (639 - HELPFRAM_SIZE.0) / 2);
        assert_eq!(MENU_RECT.uly, (479 - HELPFRAM_SIZE.1) / 2);
        // `WINDOW_MAIN_MAP` 180,18 — 448×448: `180 + (448-301)/2`.
        assert_eq!(GAME_RECT.ulx, 180 + (448 - HELPFRAM_SIZE.0) / 2);
        assert_eq!(GAME_RECT.uly, 18 + (448 - HELPFRAM_SIZE.1) / 2);

        let screen = HelpScreen::new(HelpSection::Cargo, HelpPlacement::Game);
        assert_eq!(screen.window_rect(), GAME_RECT);
        assert_eq!(
            screen.text_rect(),
            PixelRect::new(253 + 20, 126 + 20, 265, 160)
        );
        assert_eq!(screen.done_rect(), PixelRect::new(373, 319, 65, 24));
    }

    #[test]
    fn game_screen_has_keys_button_and_shifted_done() {
        let screen = HelpScreen::new(HelpSection::GameScreen, HelpPlacement::Game);

        assert_eq!(
            screen.done_rect(),
            PixelRect::new(253 + 85, 126 + 193, 65, 24)
        );
        assert_eq!(
            screen.keys_rect(),
            Some(PixelRect::new(253 + 155, 126 + 193, 65, 24))
        );

        // `Keys` (`_(ceb6)`) is longer than any entry and scrolls; toggling
        // returns to the top and back to the entry.
        let mut screen = screen;
        screen.toggle_keys();
        assert!(screen.keys_mode());
        assert!(screen.is_scrollable());
        screen.page_down();
        assert!(screen.row_index() > 0);
        screen.toggle_keys();
        assert!(!screen.keys_mode());
        assert_eq!(screen.row_index(), 0);
        assert_eq!(
            screen.lines(),
            wrap_gnw_text(entry_text(HelpSection::GameScreen, 0), TEXT_RECT.width)
        );
    }

    #[test]
    fn keys_close_and_scroll() {
        // The entry may not scroll — page navigation is checked on
        // `Keys` (FONT5, 265 px, more lines than a page).
        let mut screen = HelpScreen::new(HelpSection::GameScreen, HelpPlacement::Menu);
        screen.toggle_keys();
        assert_eq!(screen.key(Key::PageDown), HelpAction::None);
        assert_eq!(screen.row_index(), ROWS_PER_PAGE);
        assert_eq!(screen.key(Key::PageUp), HelpAction::None);
        assert_eq!(screen.row_index(), 0);
        assert_eq!(screen.key(Key::Down), HelpAction::None);
        assert_eq!(screen.row_index(), 1);
        assert_eq!(screen.key(Key::Up), HelpAction::None);
        assert_eq!(screen.row_index(), 0);
        assert_eq!(screen.key(Key::Enter), HelpAction::Close);
        assert_eq!(screen.key(Key::Escape), HelpAction::Close);
        assert_eq!(screen.key(Key::Char('x')), HelpAction::None);
    }

    #[test]
    fn clicks_hit_buttons_and_ignore_the_rest() {
        let mut screen = HelpScreen::new(HelpSection::GameScreen, HelpPlacement::Game);
        screen.toggle_keys();
        let (up, down) = screen.scroll_rects().expect("Keys прокручивается");

        assert_eq!(screen.click(up.ulx + 1, up.uly + 1), HelpAction::None);
        assert_eq!(screen.row_index(), 0);
        assert_eq!(screen.click(down.ulx + 1, down.uly + 1), HelpAction::None);
        assert_eq!(screen.row_index(), ROWS_PER_PAGE);

        let done = screen.done_rect();
        assert_eq!(screen.click(done.ulx + 1, done.uly + 1), HelpAction::Close);

        // A click outside the buttons does nothing.
        let mut screen = HelpScreen::new(HelpSection::Planet, HelpPlacement::Menu);
        assert_eq!(screen.click(0, 0), HelpAction::None);
        assert_eq!(screen.row_index(), 0);
    }

    #[test]
    fn section_names_match_the_oracle() {
        assert_eq!(HelpSection::Cargo.as_str(), "CARGO_SETUP");
        assert_eq!(HelpSection::Planet.as_str(), "PLANET_SETUP");
        assert_eq!(HelpSection::NewGame.as_str(), "NEW_GAME_SETUP");
        assert_eq!(HelpSection::Options.as_str(), "OPTIONS_SETUP");
        assert_eq!(HelpSection::Research.as_str(), "RESEARCH_SETUP");
        assert_eq!(HelpSection::Upgrades.as_str(), "UPGRADES_SETUP");
        assert_eq!(HelpSection::GameScreen.as_str(), "GAME_SCREEN_SETUP");
        assert_eq!(HelpSection::parse("cargo"), Some(HelpSection::Cargo));
        assert_eq!(HelpSection::parse("newgame"), Some(HelpSection::NewGame));
        assert_eq!(HelpSection::parse("bogus"), None);
    }
}
