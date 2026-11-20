//! Диалог `DialogMenu` — порт `dialogmenu.cpp` (окно `HELPFRAM` + `OK`).
//!
//! В C++ `DialogMenu` — универсальное модальное окно с текстом:
//! `PauseMenu_Menu` (`menu.cpp:1747`) открывает его с `_(f9bb)` («Game Paused.
//! Click OK to continue.») по `Alt+P` (`GNW_KB_KEY_LALT_P`,
//! `game_manager.cpp:5542`), а `DialogMenu_Menu(_(4556))` — для предупреждения о
//! конце игры.
//!
//! # Раскладка (оракул `dialogmenu.cpp:148-204`)
//!
//! * окно `HELPFRAM` 301×231 по центру области карты — `(253,126)`;
//! * текст — `GNW_TEXT_FONT_5`, область `(20,14)` 265×175; блок центрируется по
//!   вертикали, пока выше 175 px; строки центрируются по горизонтали
//!   (`center_align_text`), когда режим включён (пауза — включён);
//! * `OK` — `HELPOK_U/D` 65×24 в `(120,193)`, подпись `_(ae0b)` = "OK",
//!   sfx `NDONE0`; `Enter`/`Esc`/клик закрывают;
//! * прокрутка (`BLDUP__`/`BLDDWN` 18×17 в `(20,197)`/`(40,197)`) — только если
//!   строк больше `175 / 10 = 17`.
//!
//! # Осознанные отличия
//!
//! * C++ крутит цикл `Run()`/`RunMenu()` и продолжает `GameManager_ProcessState`;
//!   порт держит диалог открытым в обычном UI-цикле, а тики локальной партии на
//!   это время останавливаются (`PauseMenu`: «игра на паузе»).

use max_assets::image::BigImage;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::helpmenu::{HelpPlacement, GAME_RECT, HELPFRAM_SIZE, TEXT_RECT};
use crate::menu::{blit_simple, draw_big_image_at, draw_image_button, wrap_gnw_text};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// Текст паузы — `_(f9bb)` (`language.json`, en-US).
pub const PAUSE_TEXT: &str = "Game Paused.\nClick OK to continue.";

/// Область текста `DialogMenu` (`Image(20, 14, 265, 175)`).
pub const TEXT_AREA: PixelRect = PixelRect::new(20, 14, 265, 175);
/// `max_row_count = 175 / Text_GetHeight()` (`FONT5` — 10).
pub const MAX_ROWS: usize = 17;
/// Кнопка `OK` (`HELPOK_U/D`, 65×24) в (120,193).
pub const OK_RECT: PixelRect = PixelRect::new(120, 193, 65, 24);
/// Прокрутка вверх (`BLDUP__U/D`, 18×17) в (20,197).
pub const SCROLL_UP: PixelRect = PixelRect::new(20, 197, 18, 17);
/// Прокрутка вниз (`BLDDWN_U/D`) в (40,197).
pub const SCROLL_DOWN: PixelRect = PixelRect::new(40, 197, 18, 17);

/// Что делать с вводом диалога.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    /// Окно остаётся открытым.
    None,
    /// `OK`/`Enter`/`Esc`/клик по кнопке — закрыть.
    Close,
}

/// Модальное окно с текстом (`DialogMenu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogMenu {
    /// Строки текста после переноса по 265 px.
    lines: Vec<String>,
    /// Первая видимая строка.
    row_index: usize,
    /// Центрировать строки по горизонтали (`center_align_text`).
    center: bool,
    /// Позиция окна.
    placement: HelpPlacement,
}

impl DialogMenu {
    /// Диалог с текстом; `center` — `center_align_text` (`PauseMenu` — `true`).
    pub fn new(text: &str, placement: HelpPlacement, center: bool) -> Self {
        Self {
            lines: wrap_gnw_text(text, TEXT_AREA.width),
            row_index: 0,
            center,
            placement,
        }
    }

    /// Диалог паузы (`PauseMenu_Menu`).
    pub fn pause() -> Self {
        Self::new(PAUSE_TEXT, HelpPlacement::Game, true)
    }

    /// Строки текста.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Первая видимая строка.
    pub const fn row_index(&self) -> usize {
        self.row_index
    }

    /// Нужна ли прокрутка (`row_count > max_row_count`).
    pub fn is_scrollable(&self) -> bool {
        self.lines.len() > MAX_ROWS
    }

    /// Окно в кадре — `Window(HELPFRAM, GameManager_GetDialogWindowCenterMode())`.
    pub fn window_rect(&self) -> PixelRect {
        self.placement.rect()
    }

    /// `OK` в кадре.
    pub fn ok_rect(&self) -> PixelRect {
        offset(self.ok_local(), self.window_rect())
    }

    /// Кнопки прокрутки в кадре (если текст длиннее страницы).
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

    /// `PageUp`: `row_index -= max_row_count`, не ниже 0.
    pub fn page_up(&mut self) {
        self.row_index = self.row_index.saturating_sub(MAX_ROWS);
    }

    /// `PageDown`: только пока `row_index + max_row_count < row_count`.
    pub fn page_down(&mut self) {
        if self.row_index + MAX_ROWS < self.lines.len() {
            self.row_index += MAX_ROWS;
        }
    }

    /// Ввод: клавиши/клик.
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

    /// Рисует окно (C++ `DialogMenu::DrawText`).
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

        // `text_full_height = row_count * height`; блок центрируется, пока < 175.
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

/// Локальный прямоугольник окна → координаты кадра.
fn offset(rect: PixelRect, window: PixelRect) -> PixelRect {
    PixelRect::new(
        window.ulx + rect.ulx,
        window.uly + rect.uly,
        rect.width,
        rect.height,
    )
}

/// Ассеты диалога — те же, что у `HelpMenu` (`HELPFRAM`/`HELPOK`/`BLDUP/BLDDWN`).
pub type DialogAssets = crate::helpmenu::HelpMenuAssets;

/// `HELPFRAM` 301×231 и центр окна карты (для тестов).
pub const fn window_size() -> (i32, i32) {
    HELPFRAM_SIZE
}

/// Стандартное окно паузы в кадре (центр `WINDOW_MAIN_MAP`).
pub const fn pause_window() -> PixelRect {
    GAME_RECT
}

/// Фон диалога (для тестов/предупреждений о ресурсах).
pub type Background = BigImage;

/// Перенос текста по ширине окна — для тестов.
pub fn wrap(text: &str) -> Vec<String> {
    wrap_gnw_text(text, TEXT_AREA.width)
}

/// Текстовая область в кадре (для тестов).
pub fn text_rect(placement: HelpPlacement) -> PixelRect {
    let window = placement.rect();

    PixelRect::new(
        window.ulx + TEXT_AREA.ulx,
        window.uly + TEXT_AREA.uly,
        TEXT_AREA.width,
        TEXT_AREA.height,
    )
}

/// `TEXT_RECT` из `helpmenu` (общий с окном помощи) — для сверки раскладок.
pub const HELP_TEXT_RECT: PixelRect = TEXT_RECT;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_dialog_has_two_centered_lines_and_ok() {
        let dialog = DialogMenu::pause();

        assert_eq!(dialog.lines(), &["Game Paused.", "Click OK to continue."]);
        assert!(!dialog.is_scrollable());
        assert_eq!(dialog.window_rect(), pause_window());
        assert_eq!(
            dialog.ok_rect(),
            PixelRect::new(253 + 120, 126 + 193, 65, 24)
        );
        assert_eq!(
            dialog,
            DialogMenu::new(PAUSE_TEXT, HelpPlacement::Game, true)
        );
    }

    #[test]
    fn ok_enter_escape_and_click_close_the_dialog() {
        let mut dialog = DialogMenu::pause();
        let ok = dialog.ok_rect();

        assert_eq!(
            dialog.input(InputEvent::Key(Key::Enter)),
            DialogAction::Close
        );
        assert_eq!(
            dialog.input(InputEvent::Key(Key::Escape)),
            DialogAction::Close
        );
        assert_eq!(
            dialog.input(InputEvent::MouseClick(ok.ulx + 2, ok.uly + 2)),
            DialogAction::Close
        );
        // Клик мимо кнопки ничего не делает.
        assert_eq!(
            dialog.input(InputEvent::MouseClick(0, 0)),
            DialogAction::None
        );
    }

    #[test]
    fn long_text_gets_scroll_buttons() {
        let text = (0..40)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut dialog = DialogMenu::new(&text, HelpPlacement::Game, false);

        assert!(dialog.is_scrollable());
        let (up, down) = dialog.scroll_rects().expect("прокрутка");

        assert_eq!(
            dialog.input(InputEvent::MouseClick(down.ulx + 1, down.uly + 1)),
            DialogAction::None
        );
        assert_eq!(dialog.row_index(), MAX_ROWS);
        assert_eq!(
            dialog.input(InputEvent::MouseClick(up.ulx + 1, up.uly + 1)),
            DialogAction::None
        );
        assert_eq!(dialog.row_index(), 0);

        dialog.page_down();
        assert_eq!(dialog.row_index(), MAX_ROWS);
    }
}
