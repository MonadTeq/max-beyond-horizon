//! Экран помощи `HelpMenu` — C++ `helpmenu.cpp` + `help.json`/`language.json`.
//!
//! # Оракул
//!
//! * окно `HELPFRAM` 301×231: `Window(HELPFRAM, GameManager_GetDialogWindowCenterMode())`,
//!   то есть в игре/`CargoMenu` — по центру области карты `(253,126)`, в меню
//!   настройки (`GameConfigMenu`/`PlanetSelectMenu`/`ChoosePlayerMenu`) — по
//!   центру окна `(169,124)`;
//! * текст — `GNW_TEXT_FONT_5` (высота 10), `GNW_TEXT_OUTLINE | 0xFF` в области
//!   `Image(20, 20, 265, 160)`; `rows_per_page = 160 / 10 = 16`; если текст
//!   короче страницы, блок центрируется по вертикали (`DrawText`);
//! * прокрутка — `PageUp`/`PageDown` и кнопки `BLDUP__U`/`BLDDWN_U` 18×17 в
//!   (20,197)/(40,197) шагом `rows_per_page` строк; кнопки создаются только
//!   если строк больше страницы;
//! * `Done` — `HELPOK_U/D` 65×24 в (120,193), подпись `_(4bfa)`, клавиши
//!   `Enter`/`Esc` закрывают окно; для секции `GAME_SCREEN_SETUP` в C++ есть
//!   вторая кнопка `Keys` (`_(f0d6)`) в (155,193), а `Done` сдвинут в (85,193):
//!   она переключает текст на подсказку по клавишам `_(ceb6)`;
//! * C++ входит в «режим помощи» (`HelpMenu_Menu`): игрок кликает по элементу
//!   экрана, и окно показывает **один** абзац, привязанный к координатам.
//!
//! # Что делает Rust-клиент
//!
//! Кнопки `?`/`Help` открывают окно сразу для нужной секции. Так как выбора
//! координаты нет, окно показывает **весь текст секции** (`help.json`, en-US)
//! в порядке файла, абзацы через пустую строку — этого достаточно, чтобы
//! длинные секции (`CARGO_SETUP`, `OPTIONS_SETUP`) прокручивались, а короткие
//! (`PLANET_SETUP`) помещались на страницу. Это осознанное отличие: в C++
//! текст выбирается курсором, здесь — кнопкой (см.
//! `docs/analysis/visual-1to1-status.md` §11.6).
//!
//! Палитра окна — собственная палитра `HELPFRAM` (как у остальных big image
//! Rust-клиента); C++-порт рисует `HELPFRAM` системной палитрой, но индексы
//! UI-цветов в ней те же.

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

/// JSON с текстами помощи (генератор `max-rust/tools/gen_help_data.py`).
pub const HELP_JSON: &str = include_str!("../data/help.json");

/// `HELPFRAM` 301×231 (`Window(HELPFRAM, …)`).
pub const HELPFRAM_SIZE: (i32, i32) = (301, 231);
/// Окно помощи в меню настройки: `((639-301)/2, (479-231)/2)`.
pub const MENU_RECT: PixelRect = PixelRect::new(169, 124, 301, 231);
/// Окно помощи в игре/мобилизации: центр `WINDOW_MAIN_MAP` (180,18,448,448).
pub const GAME_RECT: PixelRect = PixelRect::new(253, 126, 301, 231);
/// Область текста: `Image(20, 20, 265, 160)` (`HelpMenu` constructor).
pub const TEXT_RECT: PixelRect = PixelRect::new(20, 20, 265, 160);
/// `rows_per_page = 160 / Text_GetHeight()`; высота `FONT5` — 10.
pub const ROWS_PER_PAGE: usize = 16;
/// Цвет текста: `GNW_TEXT_OUTLINE | 0xFF`.
pub const TEXT_COLOR: u8 = 0xFF;
/// Цвет обводки/тени `Text_Blit` с outline — чёрный (индекс 0).
pub const OUTLINE_COLOR: u8 = 0x00;
/// `Done` (не `GAME_SCREEN_SETUP`): `Button(HELPOK_U, HELPOK_D, 120, 193)`.
pub const DONE_RECT: PixelRect = PixelRect::new(120, 193, 65, 24);
/// `Done` у `GAME_SCREEN_SETUP`: (85,193).
pub const DONE_KEYS_RECT: PixelRect = PixelRect::new(85, 193, 65, 24);
/// `Keys` у `GAME_SCREEN_SETUP`: `Button(HELPOK_U, HELPOK_D, 155, 193)`.
pub const KEYS_RECT: PixelRect = PixelRect::new(155, 193, 65, 24);
/// Прокрутка вверх: `Button(BLDUP__U, BLDUP__D, 20, 197)`.
pub const SCROLL_UP_RECT: PixelRect = PixelRect::new(20, 197, 18, 17);
/// Прокрутка вниз: `Button(BLDDWN_U, BLDDWN_D, 40, 197)`.
pub const SCROLL_DOWN_RECT: PixelRect = PixelRect::new(40, 197, 18, 17);

/// Секция помощи (значение `section` в `HelpMenu_Menu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpSection {
    /// `CARGO_SETUP` (`CargoMenu::ProcessKey`, клавиша `?`).
    Cargo,
    /// `PLANET_SETUP` (`PlanetSelectMenu::EventHelp`).
    Planet,
    /// `NEW_GAME_SETUP` (`ChoosePlayerMenu::EventHelp`, одиночная игра).
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
    /// `GAME_SCREEN_SETUP` (панель `HELP_OF`, `GameManager_MenuClickHelpButton`).
    GameScreen,
    /// `ALLOCATE_SETUP` (`AllocMenu::Run`, кнопка `?`/`Shift-/`).
    Allocate,
    /// `PREFS_MENU_SETUP` (`OptionsMenu::ProcessKeyPress`, вариант `PREFSPIC`).
    Preferences,
    /// `SETUP_MENU_SETUP` (`OptionsMenu::ProcessKeyPress`, вариант `SETUPPIC`).
    Setup,
}

impl HelpSection {
    /// Имя секции в `help.json`/C++.
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
            Self::Preferences => "PREFS_MENU_SETUP",
            Self::Setup => "SETUP_MENU_SETUP",
        }
    }

    /// Разбор короткого имени (QA/`help cargo`).
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
            "preferences" | "prefs" | "prefs_menu_setup" => Some(Self::Preferences),
            "setup_menu" | "setup_menu_setup" | "setuphelp" => Some(Self::Setup),
            _ => None,
        }
    }
}

/// Где открыто окно: меняет только позицию (`GameManager_GetDialogWindowCenterMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpPlacement {
    /// Меню настройки/главное меню — `(169,124)`.
    Menu,
    /// Игра/`CargoMenu` — `(253,126)`.
    Game,
}

impl HelpPlacement {
    /// Прямоугольник окна в кадре 640×480.
    pub const fn rect(self) -> PixelRect {
        match self {
            Self::Menu => MENU_RECT,
            Self::Game => GAME_RECT,
        }
    }

    /// Позиция для `Screen::LocalSetup`/`CargoMenu`/игры.
    pub const fn for_screen(in_game: bool) -> Self {
        if in_game {
            Self::Game
        } else {
            Self::Menu
        }
    }
}

/// Действие окна помощи.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpAction {
    /// Окно остаётся открытым.
    None,
    /// `Done`/`Enter`/`Esc` — закрыть.
    Close,
}

// ---------------------------------------------------------------------------
// Данные help.json
// ---------------------------------------------------------------------------

/// Один абзац/запись помощи из `help.json`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HelpEntry {
    /// Имя записи (`UNIT_PICT`, `CANCEL_BUTTON`, …).
    pub name: String,
    /// `coordinates.ux` (C++ help cursor hit test; Rust-клиенту нужен тестам).
    pub ux: i32,
    /// `coordinates.uy`.
    pub uy: i32,
    /// `coordinates.lx`.
    pub lx: i32,
    /// `coordinates.ly`.
    pub ly: i32,
    /// Текст `en-US`.
    pub text: String,
}

#[derive(Debug, Deserialize)]
struct HelpFile {
    keys: String,
    sections: BTreeMap<String, Vec<HelpEntry>>,
}

/// Загруженный `help.json` (один раз).
fn help_file() -> &'static HelpFile {
    static FILE: OnceLock<HelpFile> = OnceLock::new();

    FILE.get_or_init(|| {
        serde_json::from_str::<HelpFile>(HELP_JSON)
            .unwrap_or_else(|error| panic!("data/help.json сгенерирован некорректно: {error}"))
    })
}

/// Записи секции (пусто, если секции нет в `help.json`).
pub fn section_entries(section: HelpSection) -> &'static [HelpEntry] {
    help_file()
        .sections
        .get(section.as_str())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Текст секции: все непустые абзацы через пустую строку.
pub fn section_text(section: HelpSection) -> String {
    section_entries(section)
        .iter()
        .map(|entry| entry.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Текст подсказки по клавишам (`_(ceb6)`, кнопка `Keys`).
pub fn keys_text() -> &'static str {
    &help_file().keys
}

/// Сколько строк занимает секция при ширине окна 265 px.
pub fn section_line_count(section: HelpSection) -> usize {
    wrap_gnw_text(&section_text(section), TEXT_RECT.width).len()
}

// ---------------------------------------------------------------------------
// Экран
// ---------------------------------------------------------------------------

/// Открытое окно помощи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpScreen {
    section: HelpSection,
    placement: HelpPlacement,
    keys_mode: bool,
    lines: Vec<String>,
    row_index: usize,
}

impl HelpScreen {
    /// Окно для секции: сразу показывает её текст (в C++ — после клика по
    /// элементу экрана).
    pub fn new(section: HelpSection, placement: HelpPlacement) -> Self {
        let text = section_text(section);

        Self {
            section,
            placement,
            keys_mode: false,
            lines: wrap_gnw_text(&text, TEXT_RECT.width),
            row_index: 0,
        }
    }

    /// Секция.
    pub const fn section(&self) -> HelpSection {
        self.section
    }

    /// Позиция окна.
    pub const fn placement(&self) -> HelpPlacement {
        self.placement
    }

    /// Показывается ли подсказка по клавишам (`GAME_SCREEN_SETUP`).
    pub const fn keys_mode(&self) -> bool {
        self.keys_mode
    }

    /// Первая видимая строка (0-based).
    pub const fn row_index(&self) -> usize {
        self.row_index
    }

    /// Сколько всего строк.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Строки текста (для тестов).
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Есть ли что прокручивать (`string_row_count > rows_per_page`).
    pub fn is_scrollable(&self) -> bool {
        self.lines.len() > ROWS_PER_PAGE
    }

    /// Прямоугольник окна в кадре.
    pub fn window_rect(&self) -> PixelRect {
        self.placement.rect()
    }

    /// Область текста в кадре.
    pub fn text_rect(&self) -> PixelRect {
        let window = self.window_rect();

        PixelRect::new(
            window.ulx + TEXT_RECT.ulx,
            window.uly + TEXT_RECT.uly,
            TEXT_RECT.width,
            TEXT_RECT.height,
        )
    }

    /// Кнопка `Done` в кадре (для `GAME_SCREEN_SETUP` — сдвинута влево).
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

    /// Кнопка `Keys` в кадре (`GAME_SCREEN_SETUP`).
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

    /// Кнопки прокрутки в кадре (если текст длиннее страницы).
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

    /// Строка `row` после `row_index` (в пределах `rows_per_page`).
    pub fn visible_lines(&self) -> impl Iterator<Item = &String> {
        self.lines.iter().skip(self.row_index).take(ROWS_PER_PAGE)
    }

    /// Переключает текст `Keys` ↔ текст секции — кнопка `Keys`
    /// (`HelpMenu::ProcessKey`, `case 1000`).
    pub fn toggle_keys(&mut self) {
        if self.section != HelpSection::GameScreen {
            return;
        }
        self.keys_mode = !self.keys_mode;
        let text = if self.keys_mode {
            keys_text().to_string()
        } else {
            section_text(self.section)
        };
        self.lines = wrap_gnw_text(&text, TEXT_RECT.width);
        self.row_index = 0;
    }

    /// `PageUp`: `row_index -= rows_per_page`, не ниже 0 (C++ анимирует по
    /// строке, итог тот же).
    pub fn page_up(&mut self) {
        if self.row_index == 0 {
            return;
        }
        self.row_index = self.row_index.saturating_sub(ROWS_PER_PAGE);
    }

    /// `PageDown`: C++ увеличивает `row_index` на страницу только пока
    /// `row_index + rows_per_page < row_count`.
    pub fn page_down(&mut self) {
        if self.row_index + ROWS_PER_PAGE < self.lines.len() {
            self.row_index += ROWS_PER_PAGE;
        }
    }

    /// Одна строка вверх/вниз (клавиши ↑/↓; в C++ только страницы).
    pub fn scroll(&mut self, delta: i32) {
        let max = self.lines.len().saturating_sub(1);
        let next = (self.row_index as i64 + i64::from(delta)).clamp(0, max as i64);

        self.row_index = next as usize;
    }

    /// Клавиша окна (`HelpMenu::ProcessKey`).
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

    /// Ввод события (клик/клавиша). Мышь двигается без эффекта.
    pub fn input(&mut self, event: InputEvent) -> HelpAction {
        match event {
            InputEvent::Key(key) => self.key(key),
            InputEvent::MouseClick(x, y) => self.click(x, y),
            _ => HelpAction::None,
        }
    }

    /// Клик по окну в координатах кадра 640×480.
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

    /// Рисует окно поверх кадра (C++ `HelpMenu::DrawText`).
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

        // Пустая строка-разделитель абзацев (`\n\n`) занимает место, как в C++.
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

        // Кнопки прокрутки — только когда строк больше страницы.
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

        // `Keys` (`_(f0d6)`) — только `GAME_SCREEN_SETUP`.
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

        // Строка состояния Rust-клиента: в C++ её нет, но она помогает
        // отличить «нет текста в help.json» от пустого окна.
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

/// Ассеты окна помощи (`HELPFRAM` + `HELPOK` + `BLDUP__/BLDDWN`).
#[derive(Debug, Clone)]
pub struct HelpMenuAssets {
    /// Фон `HELPFRAM` 301×231.
    pub background: Option<BigImage>,
    /// Кнопка `HELPOK_U/D` 65×24 (Done/Keys).
    pub done: Option<MenuButtonPair>,
    /// `BLDUP__U/D` 18×17.
    pub up: Option<MenuButtonPair>,
    /// `BLDDWN_U/D` 18×17.
    pub down: Option<MenuButtonPair>,
}

impl HelpMenuAssets {
    /// Грузит ресурсы; отсутствие части — предупреждение, не ошибка.
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

    /// Есть ли фон и кнопка `Done`.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.done.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }

    #[test]
    fn all_four_help_screens_scroll() {
        for section in [
            HelpSection::Cargo,
            HelpSection::Planet,
            HelpSection::NewGame,
            HelpSection::Options,
        ] {
            let screen = HelpScreen::new(section, HelpPlacement::Menu);
            assert!(
                screen.is_scrollable(),
                "{section:?}: {} строк — должен прокручиваться",
                screen.line_count()
            );
        }

        // Один `PageDown` — ровно на страницу; `PageUp` возвращает наверх.
        let mut cargo = HelpScreen::new(HelpSection::Cargo, HelpPlacement::Menu);
        cargo.page_down();
        assert_eq!(cargo.row_index(), ROWS_PER_PAGE);
        cargo.page_up();
        assert_eq!(cargo.row_index(), 0);

        // Ниже конца текста не уходим.
        for _ in 0..100 {
            cargo.page_down();
        }
        assert!(cargo.row_index() < cargo.line_count());
    }

    #[test]
    fn short_text_is_centered_by_the_draw_rule() {
        // C++ `DrawText`: блок центрируется по вертикали, пока высота < 160.
        let mut planet = HelpScreen::new(HelpSection::Planet, HelpPlacement::Menu);
        planet.lines = vec!["line".to_string(); 5];
        planet.row_index = 0;

        let block = GnwFontId::Font5.font().height() * planet.line_count() as i32;
        let offset = (TEXT_RECT.height - block).max(0) / 2;

        assert!(!planet.is_scrollable());
        assert!(offset > 0);
    }

    #[test]
    fn window_is_centered_like_dialog_menu() {
        // `Window(HELPFRAM, WINDOW_MAIN_WINDOW)`; окно 639×479 (lrx/lry включительно).
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
        assert!(screen.is_scrollable());

        // `Keys` переключает текст и возвращает наверх.
        let mut screen = screen;
        screen.page_down();
        let scrolled = screen.row_index();
        assert!(scrolled > 0);
        screen.toggle_keys();
        assert!(screen.keys_mode());
        assert_eq!(screen.row_index(), 0);
        assert_eq!(
            screen.lines().len(),
            wrap_gnw_text(keys_text(), TEXT_RECT.width).len()
        );
        screen.toggle_keys();
        assert!(!screen.keys_mode());
    }

    #[test]
    fn keys_close_and_scroll() {
        let mut screen = HelpScreen::new(HelpSection::Cargo, HelpPlacement::Menu);

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
        let mut screen = HelpScreen::new(HelpSection::Cargo, HelpPlacement::Game);
        let (up, down) = screen.scroll_rects().expect("CARGO scrolls");

        assert_eq!(screen.click(up.ulx + 1, up.uly + 1), HelpAction::None);
        assert_eq!(screen.row_index(), 0);
        assert_eq!(screen.click(down.ulx + 1, down.uly + 1), HelpAction::None);
        assert_eq!(screen.row_index(), ROWS_PER_PAGE);

        let done = screen.done_rect();
        assert_eq!(screen.click(done.ulx + 1, done.uly + 1), HelpAction::Close);

        // Клик мимо кнопок ничего не делает.
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
