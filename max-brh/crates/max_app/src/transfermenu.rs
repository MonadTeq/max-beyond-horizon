//! Экран X-FER — C++ `TransferMenu` (`max-cpp-port/src/transfermenu.cpp`).
//!
//! # Что это
//!
//! Пункт `x-fer` меню команд включает курсорный режим (`unit->cursor = 3`,
//! `UnitsManager_Popup_OnClick_Transfer`). Клик по своему юниту вызывает
//! `GameManager_TransferCargo` (`game_manager.cpp:4634`): он проверяет типы
//! груза, при необходимости подменяет цель на здание комплекса с подходящим
//! складом (`GameManager_GetUnitWithCargoType`), ставит `SetParent` и
//! открывает это окно. `Done`/`Enter` отдаёт `ORDER_TRANSFER` (пакет **08**,
//! процессор **P3** — `transfer_cargo`), `Esc`/`Cancel` — ничего.
//!
//! # Раскладка (`Window(XFERPIC, WINDOW_MAIN_MAP)`)
//!
//! `XFERPIC` 308×162 центрируется в окне карты (448×448 при (180,18)); C++
//! `Window::Window(ResourceID, win_id)` считает по **включающим** границам
//! (`lrx - ulx = 447`), поэтому окно встаёт в `(249,160)`, а не `(250,161)`.
//!
//! Внутри окна (`transfermenu.cpp:196-266`):
//!
//! | Элемент | Прямоугольник/точка |
//! |---|---|
//! | количество (`xfer_amount`) | (141,15,29,20), `FONT5`, `0xFF`, по центру |
//! | запас источника | (28,28,60,16) |
//! | запас цели | (223,28,60,16) |
//! | иконки юнитов | центры (104,36) и (207,36), `ReportStats_DrawListItemIcon` |
//! | подписи | (10,52,110,30) и (191,52,110,30), `COLOR_BLACK` |
//! | стрелка направления | (141,43) `XFRLFARO`/`XFRRTARO` (31×16) |
//! | левая кнопка | (17,89) `XFRLFT_U/D` (18×17) |
//! | правая кнопка | (278,89) `XFRRGT_U/D` (18×17) |
//! | полоса | (44,89,223,16), `SMBRRAW`/`SMBRFUEL`/`SMBRGOLD` |
//! | Cancel | (82,125) `XFRCAN_U/D` (64×24) |
//! | Help | (147,125) `XFRHLP_U/D` (26×24) |
//! | Done | (174,125) `XFRDNE_U/D` (57×24) |
//!
//! # Значение полосы
//!
//! `LimitedScrollbar` (`scrollbar.cpp:298`) с `free_capacity = target_capacity
//! − target_materials`, `zero_offset = -target_materials`, `xfer_give_max =
//! min(target_free, source_materials)`, `xfer_take_max = -min(source_capacity −
//! source_materials, target_materials)`, `value = give_max`. Положительное
//! значение — из юнита в цель, отрицательное — обратно. Клик по полосе:
//! `new = (w/2 + capacity·x)/w + zero_offset`, затем обрезка
//! `[zero_offset, free_capacity]` и `[take_max, give_max]` (`scrollbar.cpp:327`).
//! Заливка: `fill = (value − zero_offset)·223 / (free_capacity − zero_offset)`,
//! `LoadHorizontalBar` копирует **правые** `fill` пикселей текстуры по строкам.
//!
//! # Клавиши (`TransferMenu::ProcessKey`)
//!
//! `Enter` — Done, `Esc`/`Ctrl+Esc` — Cancel (`value = 0`), `Shift+/`/`?` —
//! `HelpMenu_Menu("TRANSFER_SETUP")`, `←/→` (`PageUp/PageDown`) — шаг полосы.
//! `KP_Enter` в C++ привязан к Done (`SetRValue`).

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::{UnitOrder, UnitOrderState};
use max_core::game::GameState;
use max_core::team::TeamId;
use max_core::transfer::{find_unit_with_cargo_type, transfer_menu_cargo_info};
use max_core::unit::UnitId;
use max_core::victory::{cargo_type_of, CargoType};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout::MAIN_MAP;
use max_render::text::{draw_text_gnw, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::menu::{blit_simple, draw_big_image_at, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::Key;

/// Размер окна `XFERPIC`.
pub const WINDOW_SIZE: (i32, i32) = (308, 162);

/// Прямоугольник количества (141,15,29,20).
pub const AMOUNT_RECT: PixelRect = PixelRect::new(141, 15, 29, 20);
/// Запас источника (28,28,60,16).
pub const SOURCE_RECT: PixelRect = PixelRect::new(28, 28, 60, 16);
/// Запас цели (223,28,60,16).
pub const TARGET_RECT: PixelRect = PixelRect::new(223, 28, 60, 16);
/// Иконка источника — центр (104,36).
pub const SOURCE_ICON: (i32, i32) = (104, 36);
/// Иконка цели — центр (207,36).
pub const TARGET_ICON: (i32, i32) = (207, 36);

/// Подпись источника (10,52,110,30).
pub const SOURCE_NAME_RECT: PixelRect = PixelRect::new(10, 52, 110, 30);
/// Подпись цели (191,52,110,30).
pub const TARGET_NAME_RECT: PixelRect = PixelRect::new(191, 52, 110, 30);

/// Стрелка направления (141,43) 31×16.
pub const ARROW_RECT: PixelRect = PixelRect::new(141, 43, 31, 16);
/// Левая кнопка (17,89) 18×17.
pub const LEFT_RECT: PixelRect = PixelRect::new(17, 89, 18, 17);
/// Правая кнопка (278,89) 18×17.
pub const RIGHT_RECT: PixelRect = PixelRect::new(278, 89, 18, 17);
/// Полоса (44,89,267,105) — 223×16.
pub const SLIDER_RECT: PixelRect = PixelRect::new(44, 89, 223, 16);
/// Кнопка полосы — `Rect(ulx-3, uly, width+6, height)` (`scrollbar.cpp:145`).
pub const SLIDER_BUTTON_RECT: PixelRect = PixelRect::new(41, 89, 229, 16);

/// Cancel (82,125) 64×24.
pub const CANCEL_RECT: PixelRect = PixelRect::new(82, 125, 64, 24);
/// Help (147,125) 26×24.
pub const HELP_RECT: PixelRect = PixelRect::new(147, 125, 26, 24);
/// Done (174,125) 57×24.
pub const DONE_RECT: PixelRect = PixelRect::new(174, 125, 57, 24);

/// `Fonts_GoldColor` — подписи `Cancel`/`Done` (palette indices).
pub const GOLD: (u8, u8, u8) = (165, 175, 199);

/// Ресурсы окна X-FER из `MAX.RES` (`TransferMenu`).
#[derive(Debug, Clone)]
pub struct TransferMenuAssets {
    /// Фон `XFERPIC` 308×162 (big image; рисуется системной палитрой окна,
    /// потому что `Window` создан с `palette_from_image = false`).
    pub background: Option<BigImage>,
    /// `Cancel` (`XFRCAN_U/D`, 64×24).
    pub cancel: Option<MenuButtonPair>,
    /// `?` (`XFRHLP_U/D`, 26×24) — картинка уже с вопросительным знаком.
    pub help: Option<MenuButtonPair>,
    /// `Done` (`XFRDNE_U/D`, 57×24).
    pub done: Option<MenuButtonPair>,
    /// `−` (`XFRLFT_U/D`, 18×17).
    pub left: Option<MenuButtonPair>,
    /// `+` (`XFRRGT_U/D`, 18×17).
    pub right: Option<MenuButtonPair>,
    /// `XFRLFARO` 31×16 — стрелка **влево**, `total < 0` (`button_arrow`
    /// отжата: `rest_state = total >= 0` = `false`, рисуется `up`-кадр).
    pub arrow_left: Option<SimpleImage>,
    /// `XFRRTARO` 31×16 — стрелка **вправо**, `total >= 0` (нажатый
    /// `rest_state`: `win_set_button_rest_state` рисует `down`-кадр кнопки,
    /// а `down` у `Button(XFRLFARO, XFRRTARO, …)` — это `XFRRTARO`).
    pub arrow_right: Option<SimpleImage>,
    /// `SMBRRAW` 285×16 — полоса сырья.
    pub bar_raw: Option<SimpleImage>,
    /// `SMBRFUEL` 342×17 — полоса топлива.
    pub bar_fuel: Option<SimpleImage>,
    /// `SMBRGOLD` 342×17 — полоса золота.
    pub bar_gold: Option<SimpleImage>,
}

impl TransferMenuAssets {
    /// Грузит ресурсы; отсутствие части — не ошибка (окно не рисуется).
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<SimpleImage> {
            archive
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };
        let big = |name: &str| -> Option<BigImage> {
            archive
                .read(name)
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok())
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };

        Self {
            background: big("XFERPIC"),
            cancel: pair("XFRCAN_U", "XFRCAN_D"),
            help: pair("XFRHLP_U", "XFRHLP_D"),
            done: pair("XFRDNE_U", "XFRDNE_D"),
            left: pair("XFRLFT_U", "XFRLFT_D"),
            right: pair("XFRRGT_U", "XFRRGT_D"),
            arrow_left: simple("XFRLFARO"),
            arrow_right: simple("XFRRTARO"),
            bar_raw: simple("SMBRRAW"),
            bar_fuel: simple("SMBRFUEL"),
            bar_gold: simple("SMBRGOLD"),
        }
    }

    /// Фон и основные кнопки на месте.
    pub fn is_ready(&self) -> bool {
        self.background.is_some()
            && self.cancel.is_some()
            && self.done.is_some()
            && self.bar_raw.is_some()
    }

    /// Полоса по типу груза (`material_id` из `TransferMenu`).
    pub fn bar(&self, cargo: CargoType) -> Option<&SimpleImage> {
        match cargo {
            CargoType::Raw => self.bar_raw.as_ref(),
            CargoType::Fuel => self.bar_fuel.as_ref(),
            CargoType::Gold => self.bar_gold.as_ref(),
            _ => None,
        }
    }
}

/// Что сделать после ввода в окне.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferAction {
    /// Ничего не менять.
    None,
    /// `Done`/`Enter` — отдать `ORDER_TRANSFER`.
    Confirm,
    /// `Cancel`/`Esc` — закрыть без приказа (`value = 0`).
    Cancel,
    /// `Help`/`?` — `TRANSFER_SETUP`.
    Help,
}

/// Открытое окно переноса.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferMenu {
    /// Юнит, из чьего меню открыт x-fer (`source_unit` в C++ до возможного
    /// обмена местами — обмен идёт только внутри `ProcessTransfer`).
    pub unit: UnitId,
    /// Цель (`unit->GetParent()`).
    pub target: UnitId,
    /// Команда (обе стороны свои).
    pub team: TeamId,
    /// Тип груза (`ResourceManager_GetUnit(...).GetCargoType()`).
    pub cargo: CargoType,
    /// `source_unit_materials`.
    pub source_materials: i32,
    /// `source_unit_capacity`.
    pub source_capacity: i32,
    /// `target_unit_materials`.
    pub target_materials: i32,
    /// `target_unit_capacity`.
    pub target_capacity: i32,
    /// `source_unit_free_capacity` — минимум свободного у источника и запаса цели.
    pub source_free: i32,
    /// `target_unit_free_capacity` — `xfer_give_max`.
    pub give_max: i32,
    /// `xfer_take_max` (отрицательное).
    pub take_max: i32,
    /// `total_materials_transferred` — текущее значение полосы.
    pub amount: i32,
}

impl TransferMenu {
    /// `TransferMenu::TransferMenu` (`transfermenu.cpp:148`).
    ///
    /// `None`, если нет одной из сторон, груз не `RAW/FUEL/GOLD` или стороны
    /// разных команд (C++ в `Run` закрывает такое окно сразу).
    pub fn new(state: &GameState, source: UnitId, target: UnitId) -> Option<Self> {
        let source_unit = state.unit(source)?;
        let target_unit = state.unit(target)?;

        if source_unit.team != target_unit.team {
            return None;
        }

        let cargo = cargo_type_of(source_unit.unit_type);
        if !matches!(cargo, CargoType::Raw | CargoType::Fuel | CargoType::Gold) {
            return None;
        }

        let team = source_unit.team;
        let (source_materials, source_capacity) =
            transfer_menu_cargo_info(state, source, target, cargo);
        let (target_materials, target_capacity) =
            transfer_menu_cargo_info(state, target, source, cargo);

        let mut give_max = target_capacity - target_materials;
        give_max = give_max.min(source_materials);

        let mut source_free = source_capacity - source_materials;
        source_free = source_free.min(target_materials);

        Some(Self {
            unit: source,
            target,
            team,
            cargo,
            source_materials,
            source_capacity,
            target_materials,
            target_capacity,
            source_free,
            give_max,
            take_max: -source_free,
            amount: give_max,
        })
    }

    /// Положение окна в кадре: центр `WINDOW_MAIN_MAP` по включающим границам
    /// (`Window(ResourceID, WINDOW_MAIN_MAP)`).
    pub fn layout(area: PixelRect) -> PixelRect {
        let ulx = area.ulx + (area.width - 1 - WINDOW_SIZE.0) / 2;
        let uly = area.uly + (area.height - 1 - WINDOW_SIZE.1) / 2;

        PixelRect::new(ulx, uly, WINDOW_SIZE.0, WINDOW_SIZE.1)
    }

    /// Прямоугольник окна в кадре 640×480.
    pub fn window_rect(&self) -> PixelRect {
        Self::layout(MAIN_MAP)
    }

    /// Сколько ресурса показывается у источника: `source_materials − amount`.
    pub fn source_shown(&self) -> i32 {
        self.source_materials - self.amount
    }

    /// Сколько ресурса показывается у цели: `target_materials + amount`.
    pub fn target_shown(&self) -> i32 {
        self.target_materials + self.amount
    }

    /// Пределы значения: `[max(take_max, zero_offset), min(give_max, free_capacity)]`.
    pub fn value_limits(&self) -> (i32, i32) {
        let zero_offset = -self.target_materials;
        let free_capacity = self.target_capacity - self.target_materials;

        (
            self.take_max.max(zero_offset),
            self.give_max.min(free_capacity),
        )
    }

    /// Установить значение с обрезкой — как `LimitedScrollbar::ProcessKey`
    /// после клика по полосе.
    pub fn set_amount(&mut self, value: i32) {
        let zero_offset = -self.target_materials;
        let free_capacity = self.target_capacity - self.target_materials;

        let mut value = value.max(zero_offset).min(free_capacity);

        if value > self.give_max {
            value = self.give_max;
        }
        if value < self.take_max {
            value = self.take_max;
        }

        self.amount = value;
    }

    /// Значение по клику в точке кадра: формула `LimitedScrollbar` для
    /// горизонтальной полосы.
    pub fn click_value(&self, window_x: i32) -> i32 {
        let capacity = self.target_capacity - self.target_materials;
        let width = SLIDER_RECT.width;
        let x = window_x - SLIDER_RECT.ulx;
        let zero_offset = -self.target_materials;

        let value = if capacity != 0 {
            (width / 2 + capacity * x) / width + zero_offset
        } else {
            zero_offset
        };

        value
    }

    /// Один шаг полосы (`Scrollbar::ProcessKey` с `1000`/`1001`).
    pub fn adjust(&mut self, increase: bool) -> bool {
        let before = self.amount;

        if increase {
            let (_, high) = self.value_limits();
            if self.amount < self.give_max && self.amount < high {
                self.amount += 1;
            }
        } else {
            let (low, _) = self.value_limits();
            if self.amount > self.take_max && self.amount > low {
                self.amount -= 1;
            }
        }

        self.amount != before
    }

    /// Клик мышью в координатах кадра.
    pub fn click(&mut self, x: i32, y: i32) -> TransferAction {
        let window = self.window_rect();
        let local_x = x - window.ulx;
        let local_y = y - window.uly;

        if DONE_RECT.contains(local_x, local_y) {
            return TransferAction::Confirm;
        }
        if CANCEL_RECT.contains(local_x, local_y) {
            return TransferAction::Cancel;
        }
        if HELP_RECT.contains(local_x, local_y) {
            return TransferAction::Help;
        }
        if LEFT_RECT.contains(local_x, local_y) {
            self.adjust(false);
            return TransferAction::None;
        }
        if RIGHT_RECT.contains(local_x, local_y) {
            self.adjust(true);
            return TransferAction::None;
        }
        if SLIDER_BUTTON_RECT.contains(local_x, local_y) {
            let value = self.click_value(local_x);
            self.set_amount(value);
            return TransferAction::None;
        }

        TransferAction::None
    }

    /// Клавиша: `TransferMenu::ProcessKey`.
    pub fn key(&mut self, key: Key) -> TransferAction {
        match key {
            Key::Enter => TransferAction::Confirm,
            Key::Escape => {
                self.amount = 0;
                TransferAction::Cancel
            }
            Key::Char('?') | Key::Char('/') | Key::Char('h') | Key::Char('H') => {
                TransferAction::Help
            }
            Key::Left | Key::PageUp | Key::Char('-') | Key::Char('_') => {
                self.adjust(false);
                TransferAction::None
            }
            Key::Right | Key::PageDown | Key::Char('+') | Key::Char('=') => {
                self.adjust(true);
                TransferAction::None
            }
            _ => TransferAction::None,
        }
    }

    /// Событие ввода ([`crate::ui::InputEvent`]).
    pub fn input(&mut self, event: crate::ui::InputEvent) -> TransferAction {
        match event {
            crate::ui::InputEvent::MouseClick(x, y) => self.click(x, y),
            crate::ui::InputEvent::Key(key) => self.key(key),
            _ => TransferAction::None,
        }
    }

    /// Длина заливки полосы в пикселях (`Scrollbar::RefreshScreen`).
    pub fn bar_fill(&self) -> i32 {
        let zero_offset = -self.target_materials;
        let denominator = self.target_capacity - self.target_materials;

        if denominator == 0 {
            return 0;
        }

        ((self.amount - zero_offset) * SLIDER_RECT.width / denominator)
            .clamp(0, SLIDER_RECT.width)
    }

    /// Куда смотрит стрелка направления: `true` — вправо (от источника к
    /// приёмнику), `false` — влево (из приёмника в источник).
    ///
    /// Оракул — `TransferMenu::UpdateIndicators` (`transfermenu.cpp:94-96`):
    /// `button_arrow->SetRestState(total_materials_transferred >= 0)`. Кнопка
    /// создана как `Button(XFRLFARO, XFRRTARO, 141, 43)`
    /// (`transfermenu.cpp:239`), то есть `up = XFRLFARO` (стрелка влево) и
    /// `down = XFRRTARO` (стрелка вправо). `win_set_button_rest_state`
    /// (`button.c:1022`) при `rest_down != 0` рисует `b->down` — значит при
    /// `total >= 0` (перенос из источника в приёмник) видна стрелка **вправо**.
    /// Раньше порт рисовал наоборот (`amount >= 0` → `XFRLFARO`), то есть
    /// направление было перевёрнуто.
    pub fn arrow_points_right(&self) -> bool {
        self.amount >= 0
    }

    /// Рисует окно поверх кадра.
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &crate::Assets, state: &GameState) -> u32 {
        let Some(menu_assets) = assets.transfer.as_ref() else {
            return 0;
        };
        let palette = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return 0;
        };

        let window = self.window_rect();
        let mut painted = 0;

        // `Window(XFERPIC, WINDOW_MAIN_MAP)`: фон окна.
        if let Some(background) = menu_assets.background.as_ref() {
            painted += draw_big_image_at(
                frame,
                background,
                &palette,
                (window.ulx, window.uly),
                window,
            );
        }

        let font = GnwFontId::Font5.font();
        let white = palette.rgba(0xFF);
        let black = palette.rgba(0x00);
        let gold_base = palette.rgba(GOLD.0);
        let gold_outline = palette.rgba(GOLD.1);
        let gold_shadow = palette.rgba(GOLD.2);
        let team_lut = crate::mobilize::team_lut(self.team);

        // Иконки юнитов (`ReportStats_DrawListItemIcon`): здания — 1:4,
        // мобильные — 1:2 (те же `I_*`, что в `BuildMenu`).
        if let Some(cargo) = assets.cargo.as_ref() {
            for (unit_id, anchor) in [(self.unit, SOURCE_ICON), (self.target, TARGET_ICON)] {
                let Some(unit) = state.unit(unit_id) else {
                    continue;
                };
                let Some(icon) = cargo.icon(unit.unit_type) else {
                    continue;
                };
                let stationary = !unit.unit_type.is_mobile();
                let zoom = if stationary { 16 } else { 32 };

                painted += crate::mobilize::draw_icon(
                    frame,
                    &palette,
                    unit.unit_type,
                    icon,
                    (window.ulx + anchor.0, window.uly + anchor.1),
                    zoom,
                    &team_lut,
                );
            }
        }

        // Подписи `SingularName`, `COLOR_BLACK`, `FONT5`.
        for (unit_id, rect) in [
            (self.unit, SOURCE_NAME_RECT),
            (self.target, TARGET_NAME_RECT),
        ] {
            if let Some(unit) = state.unit(unit_id) {
                let name = crate::buildmenu::unit_name(unit.unit_type);

                painted += draw_text_hcentered(
                    frame,
                    window.ulx + rect.ulx,
                    window.uly + rect.uly,
                    rect.width,
                    font,
                    name,
                    black,
                );
            }
        }

        // `UpdateIndicators`: количество и запасы.
        painted += draw_text_hcentered(
            frame,
            window.ulx + AMOUNT_RECT.ulx,
            window.uly + AMOUNT_RECT.uly,
            AMOUNT_RECT.width,
            font,
            &self.amount.abs().to_string(),
            white,
        );
        painted += draw_text_hcentered(
            frame,
            window.ulx + SOURCE_RECT.ulx,
            window.uly + SOURCE_RECT.uly,
            SOURCE_RECT.width,
            font,
            &self.source_shown().abs().to_string(),
            white,
        );
        painted += draw_text_hcentered(
            frame,
            window.ulx + TARGET_RECT.ulx,
            window.uly + TARGET_RECT.uly,
            TARGET_RECT.width,
            font,
            &self.target_shown().abs().to_string(),
            white,
        );

        // Полоса `SMBR*`: `LoadHorizontalBar` — правые `fill` пикселей строки.
        if let Some(bar) = menu_assets.bar(self.cargo) {
            painted += draw_horizontal_bar(
                frame,
                &palette,
                bar,
                window.ulx + SLIDER_RECT.ulx,
                window.uly + SLIDER_RECT.uly,
                self.bar_fill().max(0),
                SLIDER_RECT.height,
            );
        }

        // Кнопки: картинка + подпись `Fonts_GoldColor` (`SetCaption(c,1,2)`).
        for (pair, rect, caption) in [
            (&menu_assets.cancel, CANCEL_RECT, "Cancel"),
            (&menu_assets.help, HELP_RECT, ""),
            (&menu_assets.done, DONE_RECT, "Done"),
        ] {
            let Some(pair) = pair.as_ref() else {
                continue;
            };
            let frame_rect =
                PixelRect::new(window.ulx + rect.ulx, window.uly + rect.uly, rect.width, rect.height);

            painted += blit_simple(frame, &palette, pair.image(false), frame_rect.ulx, frame_rect.uly, true);

            if !caption.is_empty() {
                painted += draw_caption(
                    frame,
                    frame_rect,
                    font,
                    caption,
                    gold_base,
                    gold_outline,
                    gold_shadow,
                );
            }
        }

        for (pair, rect) in [(&menu_assets.left, LEFT_RECT), (&menu_assets.right, RIGHT_RECT)] {
            let Some(pair) = pair.as_ref() else {
                continue;
            };
            painted += blit_simple(
                frame,
                &palette,
                pair.image(false),
                window.ulx + rect.ulx,
                window.uly + rect.uly,
                true,
            );
        }

        // Стрелка направления: `Button::SetRestState(total_materials_transferred
        // >= 0)` (`transfermenu.cpp:95`). Кнопка создана как
        // `Button(XFRLFARO, XFRRTARO, 141, 43)` (`transfermenu.cpp:239`), то есть
        // `up = XFRLFARO` (стрелка влево), `down = XFRRTARO` (стрелка вправо), а
        // `win_set_button_rest_state` (`button.c:1022`) при `rest_down != 0`
        // рисует именно `b->down`. Значит положительная сумма (перенос из
        // источника в приёмник, источник нарисован слева, приёмник справа)
        // показывает стрелку **вправо**, отрицательная — влево.
        let arrow = if self.arrow_points_right() {
            menu_assets.arrow_right.as_ref()
        } else {
            menu_assets.arrow_left.as_ref()
        };
        if let Some(arrow) = arrow {
            painted += blit_simple(
                frame,
                &palette,
                arrow,
                window.ulx + ARROW_RECT.ulx,
                window.uly + ARROW_RECT.uly,
                true,
            );
        }

        painted
    }
}

/// `Text_TextBox(..., horizontal_align = true)` без вертикального центрирования:
/// строка на `y`, по центру ширины `width`.
fn draw_text_hcentered(
    frame: &mut RgbaBuffer,
    x: i32,
    y: i32,
    width: i32,
    font: &max_render::text::GnwFont,
    text: &str,
    color: Rgba,
) -> u32 {
    let text_width = font.text_width(text) + 1;
    let spacing = (width - text_width).max(0) / 2;

    draw_text_gnw(frame, x + spacing, y, font, text, color).max(0) as u32
}

/// `Button::SetCaption(caption, 1, 2)`: прямоугольник `{0,0,w-1,h-2}` с
/// `Fonts_GoldColor` (палитровые индексы 165/175/199).
fn draw_caption(
    frame: &mut RgbaBuffer,
    rect: PixelRect,
    font: &max_render::text::GnwFont,
    caption: &str,
    base: Rgba,
    outline: Rgba,
    shadow: Rgba,
) -> u32 {
    let bounds = PixelRect::new(
        rect.ulx + 1,
        rect.uly + 2,
        (rect.width - 1).max(1),
        (rect.height - 2).max(1),
    );
    let y = bounds.uly + ((bounds.height - font.height()) / 2).max(0);
    let text_width = font.text_width(caption) + 1;
    let x = bounds.ulx + (bounds.width - text_width).max(0) / 2;

    let mut painted = 0;
    if x < frame.width() as i32 && y < frame.height() as i32 {
        // `draw_text_gnw_centered_shadowed` центрирует и по вертикали, поэтому
        // рисуем тень вручную: та же ячейка, вычисленная по C++.
        let cell = PixelRect::new(x, y, text_width, font.height());
        painted += draw_text_gnw_centered_shadowed(
            frame,
            cell,
            font,
            caption,
            base,
            outline,
            shadow,
        )
        .max(0) as u32;
    }

    painted
}

/// `LoadHorizontalBar` (`scrollbar.cpp:36`): `height` строк, каждая — правые
/// `fill` пикселей текстуры (`src_x = width - fill + column`), прозрачные
/// пиксели пропускаются.
fn draw_horizontal_bar(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    bar: &SimpleImage,
    x: i32,
    y: i32,
    fill: i32,
    height: i32,
) -> u32 {
    let width = i32::from(bar.width);
    let fill = fill.clamp(0, width);
    let mut painted = 0;

    for row in 0..height {
        for column in 0..fill {
            let src_x = (width - fill + column) as u32;
            let src_y = row as u32;

            let Some(index) = bar.pixel(src_x, src_y) else {
                continue;
            };
            if index == bar.transparent {
                continue;
            }

            if frame.set(x + column, y + row, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// `GameManager_TransferCargo` (`game_manager.cpp:4634`) в объёме выбора цели.
///
/// * цель в `ORDER_DISABLE` — `Err` (C++ показывает `NCANC0` и сообщение);
/// * тип груза совпадает — цель как есть;
/// * цель — стационарное здание комплекса, у которого есть здание с нужным
///   грузом, — цель подменяется на это здание (`GetUnitWithCargoType`);
/// * иначе `Ok(None)` — C++ молча ничего не делает.
pub fn resolve_target(
    state: &GameState,
    source: UnitId,
    clicked: UnitId,
) -> Result<Option<UnitId>, String> {
    let Some(source_unit) = state.unit(source) else {
        return Ok(None);
    };
    let Some(target_unit) = state.unit(clicked) else {
        return Ok(None);
    };

    if source == clicked || target_unit.team != source_unit.team {
        return Ok(None);
    }

    if target_unit.orders == UnitOrder::Disable {
        return Err("x-fer: цель отключена".to_string());
    }

    let cargo = cargo_type_of(source_unit.unit_type);
    if !matches!(cargo, CargoType::Raw | CargoType::Fuel | CargoType::Gold) {
        return Ok(None);
    }

    if cargo_type_of(target_unit.unit_type) == cargo {
        return Ok(Some(clicked));
    }

    if let Some(complex) = target_unit.complex {
        if !target_unit.is_mobile() {
            if let Some(id) =
                find_unit_with_cargo_type(state, target_unit.team, complex, cargo)
            {
                return Ok(Some(id));
            }
        }
    }

    Ok(None)
}

/// `GameManager_TransferCargo` → `unit1->transfer_cargo = cargo_transferred;
/// UnitsManager_SetNewOrder(unit1, ORDER_TRANSFER, ORDER_STATE_INIT)`.
///
/// Пакет **08/P3** (`max-brh-docs/architecture/client/protocol.md` §8.4): `parent_unit` —
/// цель, `transfer_cargo` — знаковое количество. `prior_order`/`prior_state`
/// берутся из юнита, как `Remote_SendNetPacket_08` после `SetNewOrder`.
pub fn transfer_order(
    state: &GameState,
    unit_id: UnitId,
    target: UnitId,
    amount: i16,
) -> Option<max_core::input::Command> {
    let unit = state.unit(unit_id)?;
    let data = state.order_data(unit_id);

    Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: unit_id,
            payload: max_core::input::OrderPayload::P3(max_core::input::PayloadP3 {
                p2: max_core::input::PayloadP2 {
                    p1: max_core::input::PayloadP1 {
                        order: UnitOrder::Transfer,
                        state: UnitOrderState::Init,
                        prior_order: unit.orders,
                        prior_state: unit.state,
                        disabled_reaction_fire: unit.disabled_reaction_fire,
                    },
                    parent_unit: Some(target),
                    experience: unit.experience,
                },
                move_to: data.move_to,
                fire_on: data.fire_on,
                enemy_unit: None,
                transfer_cargo: amount,
                stealth_dice_roll: unit.stealth_dice_roll,
            }),
        },
    ))
}

/// `UTILITY`: знаковое количество переноса из состояния окна.
pub fn amount_to_transfer(menu: &TransferMenu) -> i16 {
    menu.amount.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// Проверка QA: цель допустима для x-fer (для тестов/журнала).
pub fn can_transfer_between(
    state: &GameState,
    source: UnitId,
    target: UnitId,
) -> bool {
    resolve_target(state, source, target)
        .ok()
        .flatten()
        .is_some()
}

/// Груз юнита по типу (для QA/журнала).
pub fn unit_storage_of(state: &GameState, unit_id: UnitId) -> i32 {
    state.unit(unit_id).map_or(0, |unit| i32::from(unit.storage))
}

/// Единицы груза в комплексе по типу — обёртка для QA-строк.
pub fn stored_in_complex(
    state: &GameState,
    team: TeamId,
    complex: max_core::team::ComplexId,
    cargo: CargoType,
) -> i32 {
    max_core::transfer::complex_stored(state, team, complex, cargo)
}

// Для тестов и будущего «перетаскивания» полосы: тип ресурса-иконки.

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::{unit_flags, TeamClan, UnitType};
    use max_core::geo::Point;
    use max_core::team::TeamUnits;
    use max_render::buffer::PixelRect;
    use max_render::layout::MAIN_MAP;

    fn state_with_transfer() -> (GameState, UnitId, UnitId) {
        let mut state = GameState::new();
        state.teams[0].team_type = max_core::team::TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);

        let mine = state
            .spawn_unit(
                UnitType::MiningStation,
                TeamId::RED,
                Point::new(20, 20),
                unit_flags::STATIONARY | unit_flags::BUILDING,
            )
            .expect("mine");
        let complex = state.unit(mine).expect("mine").complex.expect("complex");
        let dump = state
            .spawn_unit(
                UnitType::MaterialStorage,
                TeamId::RED,
                Point::new(22, 24),
                unit_flags::STATIONARY | unit_flags::BUILDING,
            )
            .expect("dump");
        if let Some(own) = state.unit(dump).and_then(|unit| unit.complex) {
            state.team_units[0].remove_complex(own);
        }
        state.unit_mut(dump).expect("dump").complex = Some(complex);
        state
            .team_units[0]
            .get_complex_mut(complex)
            .expect("complex")
            .grow();

        state.unit_mut(mine).expect("mine").storage = 10;
        state.unit_mut(dump).expect("dump").storage = 0;

        let truck = state
            .spawn_unit(
                UnitType::SupplyTruck,
                TeamId::RED,
                Point::new(21, 21),
                unit_flags::MOBILE_LAND_UNIT,
            )
            .expect("truck");
        state.unit_mut(truck).expect("truck").storage = 30;

        (state, truck, dump)
    }

    #[test]
    fn menu_numbers_follow_cpp_formulas() {
        let (state, truck, dump) = state_with_transfer();
        let menu = TransferMenu::new(&state, truck, dump).expect("menu");

        // источник — грузовик: 30/50; цель — весь комплекс: 10/75.
        assert_eq!((menu.source_materials, menu.source_capacity), (30, 50));
        assert_eq!((menu.target_materials, menu.target_capacity), (10, 75));

        // total = min(75-10, 30) = 30; source_free = min(50-30, 10) = 10.
        assert_eq!(menu.give_max, 30);
        assert_eq!(menu.source_free, 10);
        assert_eq!(menu.take_max, -10);
        assert_eq!(menu.amount, 30);

        assert_eq!(menu.source_shown(), 0);
        assert_eq!(menu.target_shown(), 40);
    }

    #[test]
    fn slider_click_uses_limited_scrollbar_formula() {
        let (state, truck, dump) = state_with_transfer();
        let mut menu = TransferMenu::new(&state, truck, dump).expect("menu");

        // zero_offset = -10, capacity = 75-10 = 65, width = 223.
        // x = 0 → (111 + 0)/223 - 10 = -10.
        assert_eq!(menu.click_value(44), -10);
        // x = 223 → (111 + 65*223)/223 - 10 = 65 - 10 = 55 → clamp до give_max 30.
        assert_eq!(menu.click_value(44 + 223), 55);
        menu.set_amount(menu.click_value(44 + 223));
        assert_eq!(menu.amount, 30);

        // Середина: (111 + 65*111)/223 - 10 = 32 - 10 = 22.
        assert_eq!(menu.click_value(44 + 111), 22);
    }

    #[test]
    fn adjust_respects_give_and_take_limits() {
        let (state, truck, dump) = state_with_transfer();
        let mut menu = TransferMenu::new(&state, truck, dump).expect("menu");

        assert!(!menu.adjust(true), "give_max достигнут");
        assert!(menu.adjust(false));
        assert_eq!(menu.amount, 29);

        menu.set_amount(-10);
        assert!(!menu.adjust(false), "take_max достигнут");
        assert!(menu.adjust(true));
        assert_eq!(menu.amount, -9);
    }

    #[test]
    fn click_buttons_map_to_actions() {
        let (state, truck, dump) = state_with_transfer();
        let mut menu = TransferMenu::new(&state, truck, dump).expect("menu");
        let window = menu.window_rect();
        let centre = |rect: PixelRect| {
            (
                window.ulx + rect.ulx + rect.width / 2,
                window.uly + rect.uly + rect.height / 2,
            )
        };

        let (x, y) = centre(DONE_RECT);
        assert_eq!(menu.click(x, y), TransferAction::Confirm);
        let (x, y) = centre(CANCEL_RECT);
        assert_eq!(menu.click(x, y), TransferAction::Cancel);
        let (x, y) = centre(HELP_RECT);
        assert_eq!(menu.click(x, y), TransferAction::Help);
        let (x, y) = centre(RIGHT_RECT);
        let before = menu.amount;
        assert_eq!(menu.click(x, y), TransferAction::None);
        assert_eq!(menu.amount, before, "give_max уже достигнут");
        let (x, y) = centre(LEFT_RECT);
        menu.click(x, y);
        assert_eq!(menu.amount, before - 1);
        // Клик мимо кнопок — ничего.
        assert_eq!(menu.click(window.ulx + 2, window.uly + 2), TransferAction::None);
    }

    #[test]
    fn keys_match_cpp() {
        let (state, truck, dump) = state_with_transfer();
        let mut menu = TransferMenu::new(&state, truck, dump).expect("menu");

        assert_eq!(menu.key(Key::Enter), TransferAction::Confirm);
        assert_eq!(menu.key(Key::Escape), TransferAction::Cancel);
        assert_eq!(menu.amount, 0, "Esc сбрасывает количество");
        assert_eq!(menu.key(Key::Char('?')), TransferAction::Help);
        assert_eq!(menu.key(Key::Left), TransferAction::None);
        assert_eq!(menu.amount, -1);
        assert_eq!(menu.key(Key::Right), TransferAction::None);
        assert_eq!(menu.amount, 0);
    }

    #[test]
    fn layout_centres_with_inclusive_bounds() {
        let rect = TransferMenu::layout(MAIN_MAP);
        assert_eq!(rect, PixelRect::new(249, 160, 308, 162));

        let area = PixelRect::new(0, 0, 640, 480);
        let rect = TransferMenu::layout(area);
        assert_eq!(rect.ulx, (640 - 1 - 308) / 2);
        assert_eq!(rect.uly, (480 - 1 - 162) / 2);
    }

    #[test]
    fn arrow_points_from_source_to_target() {
        let (state, truck, dump) = state_with_transfer();
        let mut menu = TransferMenu::new(&state, truck, dump).expect("menu");

        // `SetRestState(total >= 0)` → нажата → `down` = `XFRRTARO` (вправо):
        // перенос идёт из юнита (источник, слева) в приёмник (справа).
        menu.set_amount(5);
        assert!(menu.arrow_points_right(), "give: стрелка к приёмнику");
        assert_eq!(menu.amount, 5);

        // Ноль — тоже `total >= 0`, C++ оставляет кнопку нажатой.
        menu.set_amount(0);
        assert!(menu.arrow_points_right());

        // Отрицательное значение — обратный перенос: кнопка отжата, `up` =
        // `XFRLFARO` (влево).
        menu.set_amount(-5);
        assert!(!menu.arrow_points_right(), "take: стрелка обратно к юниту");
    }

    #[test]
    fn bar_fill_scales_amount_over_target_capacity() {
        let (state, truck, dump) = state_with_transfer();
        let mut menu = TransferMenu::new(&state, truck, dump).expect("menu");

        // amount 30: (30 + 10) * 223 / 65 = 137.
        assert_eq!(menu.bar_fill(), (40 * 223) / 65);
        menu.set_amount(0);
        assert_eq!(menu.bar_fill(), (10 * 223) / 65);
        menu.set_amount(-10);
        assert_eq!(menu.bar_fill(), 0);
    }

    #[test]
    fn resolve_target_swaps_to_a_storage_building() {
        let (state, truck, dump) = state_with_transfer();
        // Клик по шахте (RAW, но это склад 25) — тип совпадает, цель она.
        let mine = state
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                state
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::MiningStation)
            })
            .expect("mine");
        assert_eq!(resolve_target(&state, truck, mine), Ok(Some(mine)));

        // Грузовик с топливом на комплекс с ADUMP: C++ подменяет цель на
        // здание с топливом — его нет, значит `None` (никакой передачи).
        let cargo_state = state;
        let fuel_truck = cargo_state
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                cargo_state
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::SupplyTruck)
            })
            .expect("truck");
        assert_eq!(
            resolve_target(&cargo_state, fuel_truck, dump),
            Ok(Some(dump)),
            "тип груза совпадает (RAW)"
        );
    }

    #[test]
    fn resolve_target_rejects_disabled_target() {
        let (mut state, truck, dump) = state_with_transfer();
        state.unit_mut(dump).expect("dump").orders = UnitOrder::Disable;
        assert!(resolve_target(&state, truck, dump).is_err());
    }

    #[test]
    fn transfer_order_carries_parent_and_cargo_in_p3() {
        let (state, truck, dump) = state_with_transfer();
        let command = transfer_order(&state, truck, dump, 12).expect("command");

        match command {
            max_core::input::Command::Order(packet) => match packet.payload {
                max_core::input::OrderPayload::P3(p3) => {
                    assert_eq!(p3.p2.p1.order, UnitOrder::Transfer);
                    assert_eq!(p3.p2.parent_unit, Some(dump));
                    assert_eq!(p3.transfer_cargo, 12);
                }
                other => panic!("ожидался P3, получен {other:?}"),
            },
            other => panic!("ожидался Order, получен {other:?}"),
        }
    }
}
