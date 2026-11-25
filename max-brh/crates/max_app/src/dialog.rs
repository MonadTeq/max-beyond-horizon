//! `DialogMenu` dialog — a port of `dialogmenu.cpp` (the `HELPFRAM` window + `OK`).
//!
//! In C++ `DialogMenu` is a universal modal window with text:
//! `PauseMenu_Menu` (`menu.cpp:1747`) opens it with `_(f9bb)` («Game Paused.
//! Click OK to continue.») on `Alt+P` (`GNW_KB_KEY_LALT_P`,
//! `game_manager.cpp:5542`), and `DialogMenu_Menu(_(4556))` — for the end-of-game
//! warning.
//!
//! # Layout (oracle `dialogmenu.cpp:148-204`)
//!
//! * `HELPFRAM` window 301×231 in the center of the map area — `(253,126)`;
//! * text — `GNW_TEXT_FONT_5`, area `(20,14)` 265×175; the block is centered
//!   vertically while it is above 175 px; the lines are centered horizontally
//!   (`center_align_text`) when the mode is on (pause — on);
//! * `OK` — `HELPOK_U/D` 65×24 at `(120,193)`, label `_(ae0b)` = "OK",
//!   sfx `NDONE0`; `Enter`/`Esc`/click close it;
//! * scrolling (`BLDUP__`/`BLDDWN` 18×17 at `(20,197)`/`(40,197)`) — only if
//!   there are more than `175 / 10 = 17` lines.
//!
//! # Deliberate differences
//!
//! * C++ runs the `Run()`/`RunMenu()` loop and continues `GameManager_ProcessState`;
//!   the port keeps the dialog open in the regular UI loop, and the local game ticks
//!   stop during this time (`PauseMenu`: «the game is paused»).

use max_assets::image::BigImage;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::helpmenu::{HelpPlacement, GAME_RECT, HELPFRAM_SIZE, TEXT_RECT};
use crate::menu::{blit_simple, draw_big_image_at, draw_image_button, wrap_gnw_text};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// Pause text — `_(f9bb)` (`language.json`, en-US).
pub const PAUSE_TEXT: &str = "Game Paused.\nClick OK to continue.";

/// `DialogMenu` text area (`Image(20, 14, 265, 175)`).
pub const TEXT_AREA: PixelRect = PixelRect::new(20, 14, 265, 175);
/// `max_row_count = 175 / Text_GetHeight()` (`FONT5` — 10).
pub const MAX_ROWS: usize = 17;
/// `OK` button (`HELPOK_U/D`, 65×24) at (120,193).
pub const OK_RECT: PixelRect = PixelRect::new(120, 193, 65, 24);
/// Scroll up (`BLDUP__U/D`, 18×17) at (20,197).
pub const SCROLL_UP: PixelRect = PixelRect::new(20, 197, 18, 17);
/// Scroll down (`BLDDWN_U/D`) at (40,197).
pub const SCROLL_DOWN: PixelRect = PixelRect::new(40, 197, 18, 17);

/// What to do with dialog input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    /// The window stays open.
    None,
    /// `OK`/`Enter`/`Esc`/button click — close.
    Close,
}

/// Modal window with text (`DialogMenu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogMenu {
    /// Text lines after wrapping at 265 px.
    lines: Vec<String>,
    /// First visible line.
    row_index: usize,
    /// Center the lines horizontally (`center_align_text`).
    center: bool,
    /// Window position.
    placement: HelpPlacement,
}

impl DialogMenu {
    /// Dialog with text; `center` — `center_align_text` (`PauseMenu` — `true`).
    pub fn new(text: &str, placement: HelpPlacement, center: bool) -> Self {
        Self {
            lines: wrap_gnw_text(text, TEXT_AREA.width),
            row_index: 0,
            center,
            placement,
        }
    }

    /// Pause dialog (`PauseMenu_Menu`).
    pub fn pause() -> Self {
        Self::new(PAUSE_TEXT, HelpPlacement::Game, true)
    }

    /// Text lines.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// First visible line.
    pub const fn row_index(&self) -> usize {
        self.row_index
    }

    /// Whether scrolling is needed (`row_count > max_row_count`).
    pub fn is_scrollable(&self) -> bool {
        self.lines.len() > MAX_ROWS
    }

    /// The window in the frame — `Window(HELPFRAM, GameManager_GetDialogWindowCenterMode())`.
    pub fn window_rect(&self) -> PixelRect {
        self.placement.rect()
    }

    /// `OK` in the frame.
    pub fn ok_rect(&self) -> PixelRect {
        offset(self.ok_local(), self.window_rect())
    }

    /// Scroll buttons in the frame (if the text is longer than a page).
    pub fn scroll_rects(&self) -> Option<(PixelRect, PixelRect)> {
        if !self.is_scrollable() {
            return None;
        }
        let window = self.window_rect();

        Some((offset(SCROLL_UP, window), offset(SCROLL_DOWN, window)))
    }

    fn ok_local(&self) -> PixelRect {
        OK_RECT
    }

    /// `PageUp`: `row_index -= max_row_count`, not below 0.
    pub fn page_up(&mut self) {
        self.row_index = self.row_index.saturating_sub(MAX_ROWS);
    }

    /// `PageDown`: only while `row_index + max_row_count < row_count`.
    pub fn page_down(&mut self) {
        if self.row_index + MAX_ROWS < self.lines.len() {
            self.row_index += MAX_ROWS;
        }
    }

    /// Input: keys/click.
    pub fn input(&mut self, event: InputEvent) -> DialogAction {
        match event {
            InputEvent::Key(Key::Enter) | InputEvent::Key(Key::Escape) => DialogAction::Close,
            InputEvent::Key(Key::PageUp) => {
                self.page_up();
                DialogAction::None
            }
            InputEvent::Key(Key::PageDown) => {
                self.page_down();
                DialogAction::None
            }
            InputEvent::MouseClick(x, y) => {
                if self.ok_rect().contains(x, y) {
                    return DialogAction::Close;
                }
                if let Some((up, down)) = self.scroll_rects() {
                    if up.contains(x, y) {
                        self.page_up();
                    } else if down.contains(x, y) {
                        self.page_down();
                    }
                }
                DialogAction::None
            }
            _ => DialogAction::None,
        }
    }

    /// Draws the window (C++ `DialogMenu::DrawText`).
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        assets: Option<&crate::helpmenu::HelpMenuAssets>,
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

        // `text_full_height = row_count * height`; the block is centered while < 175.
        let block = font.height() * self.visible_lines().count() as i32;
        let mut y = window.uly + TEXT_AREA.uly;

        if block < TEXT_AREA.height {
            y += (TEXT_AREA.height - block) / 2;
        }

        for line in self.visible_lines() {
            let x = if self.center {
                let width = font.text_width(line);

                window.ulx + TEXT_AREA.ulx + (TEXT_AREA.width - width.min(TEXT_AREA.width)) / 2
            } else {
                window.ulx + TEXT_AREA.ulx
            };
            painted += draw_text_gnw(frame, x, y, font, line, palette.rgba(0xFF)).max(0) as u32;
            y += font.height();
        }

        // `OK` (`_(ae0b)`).
        let ok = self.ok_rect();
        if let Some(pair) = assets.and_then(|assets| assets.done.as_ref()) {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                ok,
                "OK",
                false,
            );
        } else {
            frame.fill_rect(ok, palette.rgba(0x17));
            painted += 1;
            painted += draw_text_gnw_centered_shadowed(
                frame,
                ok,
                font,
                "OK",
                palette.rgba(165),
                palette.rgba(175),
                palette.rgba(199),
            )
            .max(0) as u32;
        }

        if let Some((up, down)) = self.scroll_rects() {
            if let Some(pair) = assets.and_then(|assets| assets.up.as_ref()) {
                painted += blit_simple(frame, &palette, pair.image(false), up.ulx, up.uly, true);
            }
            if let Some(pair) = assets.and_then(|assets| assets.down.as_ref()) {
                painted +=
                    blit_simple(frame, &palette, pair.image(false), down.ulx, down.uly, true);
            }
        }

        painted
    }

    fn visible_lines(&self) -> impl Iterator<Item = &String> {
        self.lines.iter().skip(self.row_index).take(MAX_ROWS)
    }
}

/// Local window rectangle → frame coordinates.
fn offset(rect: PixelRect, window: PixelRect) -> PixelRect {
    PixelRect::new(
        window.ulx + rect.ulx,
        window.uly + rect.uly,
        rect.width,
        rect.height,
    )
}

/// Dialog assets — the same as `HelpMenu` (`HELPFRAM`/`HELPOK`/`BLDUP/BLDDWN`).
pub type DialogAssets = crate::helpmenu::HelpMenuAssets;

/// `HELPFRAM` 301×231 and the map window center (for tests).
pub const fn window_size() -> (i32, i32) {
    HELPFRAM_SIZE
}

/// Standard pause window in the frame (center of `WINDOW_MAIN_MAP`).
pub const fn pause_window() -> PixelRect {
    GAME_RECT
}

/// Dialog background (for tests/resource warnings).
pub type Background = BigImage;

/// Text wrapping by the window width — for tests.
pub fn wrap(text: &str) -> Vec<String> {
    wrap_gnw_text(text, TEXT_AREA.width)
}

/// Text area in the frame (for tests).
pub fn text_rect(placement: HelpPlacement) -> PixelRect {
    let window = placement.rect();

    PixelRect::new(
        window.ulx + TEXT_AREA.ulx,
        window.uly + TEXT_AREA.uly,
        TEXT_AREA.width,
        TEXT_AREA.height,
    )
}

/// `TEXT_RECT` from `helpmenu` (shared with the help window) — for comparing layouts.
pub const HELP_TEXT_RECT: PixelRect = TEXT_RECT;

#[cfg(test)]
#[path = "dialog_tests.rs"]
mod tests;
