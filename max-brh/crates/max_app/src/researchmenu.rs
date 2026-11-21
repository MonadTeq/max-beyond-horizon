//! Экран исследований `ResearchMenu` — C++ `researchmenu.cpp` (окно `RSRCHPIC`).
//!
//! # Оракул
//!
//! * окно `RSRCHPIC` 360×331, отцентрованное в области карты
//!   (`Window(RSRCHPIC, GameManager_GetDialogWindowCenterMode())` → `(223,76)`);
//! * заголовок `Research Topics` (`GNW_TEXT_FONT_5`, `COLOR_GREEN`,
//!   `Text_TextBox(95,19,170,13)`, по центру);
//! * подписи колонок `Labs`/`Topics`/`Turns` шрифтом FONT1 в (23,52)/(184,52)/
//!   (282,52) — по центру своих ширин 125/90/60;
//! * восемь строк тем (шаг 28 px, первая `uly = 70`), порядок C++:
//!   Attack (`I_HRDATK`), Shots, Range, Armor, Hits, Speed, Scan, Cost
//!   (`I_GOLD`); у каждой:
//!   - стрелки `UPGLFT_U/D` (72,uly) и `UPGRGT_U/D` (143,uly) 18×17, у
//!     недоступной — `UPGLFT_X`/`UPGRGT_X`;
//!   - счётчик центров темы (число справа в окне `Image(28,uly+3,32,10)`);
//!   - «ходов до завершения» (`Image(293,uly+3,40,10)`): C++ считает
//!     `turns = (centers + turns_to_complete − 1) / centers` по формуле
//!     `ResearchMenu_UpdateResearchProgress` (без изменения состояния);
//!   - слайдер `PRFSLIDE` 14×17 в слоте `Image(91,uly,48,17)`: позиция
//!     `(48−14) * centers / active_centers`; клик по слоту — пересчёт
//!     `((x−91)·active + 24) / 48` центров темы;
//!   - подпись темы (185,uly,49,15) и иконка по центру (170,uly+7);
//!   - `+N%` (`research_level * 10`) справа у x=278;
//! * `Cancel`/`Help`/`Done` — `DPTBAYUP/DN` (75×25) в (92,294)/(194,294) с
//!   подписями и `DPTHP_UP/DN` (31×26) в (167,294).
//!
//! # Распределение
//!
//! `free_capacity` — «свободные» центры: в C++ он стартует с 0, потому что все
//! включённые центры уже закреплены за темами. Уменьшение темы освобождает
//! центры (`free_capacity += old − new`), увеличение — тратит их. Поэтому
//! перераспределение идёт «сначала вниз у одной темы, потом вверх у другой»;
//! если закрыть экран с ненулевым `free_capacity`, лишние центры выключаются —
//! ровно так работает `ResearchMenu_OnClick_Done` (см.
//! [`GameState::set_research_allocation`]).
//!
//! # Клавиатура (Rust-клиент)
//!
//! В C++ у экрана только кнопки (клавиши — лишь `Esc` для выхода и `Enter`
//! через кнопки). Здесь добавлены `↑/↓` (выбор темы), `←/→` и `−/+`
//! (изменить выбранную тему), `PageUp/PageDown`, `Enter` = `Done`, `Esc` =
//! `Cancel`, `H`/`?` = помощь. Это задокументированное отличие: без него
//! headless-QA и клавиатурный игрок не могут распределить центры.
//!
//! # Подключение
//!
//! В C++ экран открывается из popup'а исследовательского центра
//! (`UnitsManager_Popup_OnClick_Research`) при `ORDER_POWER_ON`. В Rust-клиенте
//! popup-меню юнита нет, поэтому экран открывается клавишей `R` для
//! выделенного включённого `RESEARCH`-центра своей команды (как `U` для
//! апгрейдов). `Done` пишет распределение в `GameState`, а
//! `ResearchMenu_NewTurn` (порт `max_core::research::research_new_turn`) уже
//! вызывается из `GameState::finish_turn` — экран и тик используют одни и те
//! же `research_topics`.

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::{UnitOrder, UnitType, RESEARCH_TOPIC_COUNT};
use max_core::game::GameState;
use max_core::research::{update_research_progress, RESEARCH_TOPIC_FACTORS};
use max_core::team::TeamId;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::assets::Assets;
use crate::menu::{blit_simple, draw_big_image_at, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// `RSRCHPIC` 360×331.
pub const WINDOW_SIZE: (i32, i32) = (360, 331);
/// Окно в кадре: центр `WINDOW_MAIN_MAP` (180,18,448,448).
pub const WINDOW_RECT: PixelRect = PixelRect::new(223, 76, 360, 331);
/// Заголовок `Text_TextBox(95,19,170,13)`.
pub const TITLE_RECT: PixelRect = PixelRect::new(95, 19, 170, 13);
/// Подпись `Labs` (`Text_TextLine(23,52,125)`).
pub const LABS_LABEL: PixelRect = PixelRect::new(23, 52, 125, 10);
/// Подпись `Topics` (`Text_TextLine(184,52,90)`).
pub const TOPICS_LABEL: PixelRect = PixelRect::new(184, 52, 90, 10);
/// Подпись `Turns` (`Text_TextLine(282,52,60)`).
pub const TURNS_LABEL: PixelRect = PixelRect::new(282, 52, 60, 10);
/// Первая строка тем (`uly_slider = topic_index * 28 + 70`).
pub const FIRST_ROW_Y: i32 = 70;
/// Шаг строк.
pub const ROW_HEIGHT: i32 = 28;
/// Стрелка влево (`Button(UPGLFT_U, UPGLFT_D, 72, uly)`).
pub const LEFT_ARROW: (i32, i32) = (72, 0);
/// Стрелка вправо (`Button(UPGRGT_U, UPGRGT_D, 143, uly)`).
pub const RIGHT_ARROW: (i32, i32) = (143, 0);
/// Размер стрелки (18×17).
pub const ARROW_SIZE: (i32, i32) = (18, 17);
/// Поле счётчика центров (`Image(28, uly+3, 32, 10)`).
pub const LABS_COUNT_BG: PixelRect = PixelRect::new(28, 3, 32, 10);
/// Поле «ходов» (`Image(293, uly+3, 40, 10)`).
pub const TURNS_BG: PixelRect = PixelRect::new(293, 3, 40, 10);
/// Слот слайдера (`Image(91, uly, 48, 17)`).
pub const SLIDER_BG: PixelRect = PixelRect::new(91, 0, 48, 17);
/// Размер `PRFSLIDE` (14×17).
pub const SLIDER_SIZE: (i32, i32) = (14, 17);
/// Подпись темы (`Text_TextBox(185, uly, 49, 15)`).
pub const TOPIC_LABEL: PixelRect = PixelRect::new(185, 0, 49, 15);
/// Точка центра иконки (`WindowManager_DecodeSimpleImage(image, 170-w/2, uly+7-h/2)`).
pub const TOPIC_ICON: (i32, i32) = (170, 7);
/// Правый край текста `+N%` (`278 - Text_GetWidth`).
pub const PERCENT_RIGHT: i32 = 278;
/// `Cancel` (`DPTBAYUP/DN` 75×25 в 92,294).
pub const CANCEL_RECT: PixelRect = PixelRect::new(92, 294, 75, 25);
/// `Help` (`DPTHP_UP/DN` 31×26 в 167,294).
pub const HELP_RECT: PixelRect = PixelRect::new(167, 294, 31, 26);
/// `Done` (`DPTBAYUP/DN` 75×25 в 194,294).
pub const DONE_RECT: PixelRect = PixelRect::new(194, 294, 75, 25);
/// Цвет заголовка `COLOR_GREEN`.
pub const GREEN: u8 = 0x02;
/// Цвет обычного текста (`0xA2`).
pub const TEXT: u8 = 0xA2;
/// Цвет подписи кнопок/заголовков — `Fonts_GoldColor (165,175,199)`.
pub const GOLD: (u8, u8, u8) = (165, 175, 199);

/// Подписи тем в порядке C++ `ResearchMenu_TopicLabels`.
pub const TOPIC_LABELS: [&str; RESEARCH_TOPIC_COUNT] = [
    "Attack", "Shots", "Range", "Armor", "Hits", "Speed", "Scan", "Cost",
];

/// Иконки тем в порядке C++ `ResearchMenu_TopicIcons`.
pub const TOPIC_ICONS: [&str; RESEARCH_TOPIC_COUNT] = [
    "I_HRDATK", "I_SHOTS", "I_RANGE", "I_ARMOR", "I_HITS", "I_SPEED", "I_SCAN", "I_GOLD",
];

/// Действие экрана.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchAction {
    /// Ничего.
    None,
    /// `Cancel`/`Esc` — закрыть без изменений.
    Cancel,
    /// `Done`/`Enter` — применить распределение.
    Done,
    /// `Help`/`H` — открыть `RESEARCH_SETUP`.
    Help,
}

/// Открытый экран исследований.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchMenu {
    /// Команда.
    pub team: TeamId,
    /// Число включённых исследовательских центров (`active_research_centers`).
    pub active_centers: u16,
    /// Сколько центров ещё не закреплено (`free_capacity`).
    pub free_capacity: u16,
    /// Центры по темам (`topics[i].topic_value`, стартовое значение).
    pub values: [u16; RESEARCH_TOPIC_COUNT],
    /// Выбранная тема (только клавиатура Rust-клиента).
    pub selected: usize,
    /// Строка статуса (ошибки/подсказки).
    pub status: String,
}

impl ResearchMenu {
    /// Открывает экран для команды — C++ `ResearchMenu::ResearchMenu(team)`.
    ///
    /// `None`, если у команды нет ни одного включённого `RESEARCH`-центра:
    /// в C++ popup `Research` есть только у `ORDER_POWER_ON`, так что экран с
    /// нулём центров недостижим.
    pub fn new(state: &GameState, team: TeamId) -> Option<Self> {
        let mut values = [0u16; RESEARCH_TOPIC_COUNT];
        let mut active_centers = 0u16;

        for id in state.unit_ids_in_order() {
            let Some(unit) = state.unit(id) else {
                continue;
            };
            if unit.team != team
                || unit.unit_type != UnitType::ResearchCentre
                || unit.orders != UnitOrder::PowerOn
            {
                continue;
            }
            let index = unit.research_topic as usize;
            if index < RESEARCH_TOPIC_COUNT {
                active_centers = active_centers.saturating_add(1);
                values[index] = values[index].saturating_add(1);
            }
        }

        if active_centers == 0 {
            return None;
        }

        Some(Self {
            team,
            active_centers,
            free_capacity: 0,
            values,
            selected: 0,
            status: String::new(),
        })
    }

    /// `ResearchControl::GetValue`.
    pub fn value(&self, index: usize) -> u16 {
        self.values.get(index).copied().unwrap_or(0)
    }

    /// `ResearchControl::Update(value)` для темы `index`.
    ///
    /// `value` обрезается снизу нулём и сверху `topic_value + free_capacity`;
    /// `free_capacity` меняется на `old − new` (C++ `UpdateTopics`).
    pub fn update(&mut self, index: usize, value: i32) -> bool {
        let Some(topic_value) = self.values.get(index).copied() else {
            return false;
        };
        let old = i32::from(topic_value);
        let mut value = value.max(0);

        if value - old > i32::from(self.free_capacity) {
            value = old + i32::from(self.free_capacity);
        }
        if value == old {
            return false;
        }

        let difference = old - value;
        self.values[index] = value as u16;
        self.free_capacity =
            (i32::from(self.free_capacity) + difference).clamp(0, i32::from(u16::MAX)) as u16;
        self.selected = index;

        true
    }

    /// `ResearchMenu_OnClick_Left/Right`: `topic_value ± 1`.
    pub fn adjust(&mut self, index: usize, increase: bool) -> bool {
        let value = i32::from(self.value(index));
        self.update(index, value + if increase { 1 } else { -1 })
    }

    /// Клик по слайдеру темы `index` в координатах **кадра** 640×480.
    ///
    /// C++ `ResearchMenu_OnClick_Slider`:
    /// `((mouse_x − slider_ulx) * active + width/2) / width`.
    pub fn slider_value(&self, index: usize, x: i32) -> i32 {
        if self.active_centers == 0 {
            return i32::from(self.value(index));
        }
        let width = i32::from(SLIDER_BG.width);
        let mouse_x = x - (WINDOW_RECT.ulx + i32::from(SLIDER_BG.ulx));

        (mouse_x * i32::from(self.active_centers) + width / 2) / width
    }

    /// Установка слайдера темы `index` кликом по кадру.
    pub fn slider(&mut self, index: usize, x: i32) -> bool {
        let value = self.slider_value(index, x);
        self.update(index, value)
    }

    /// Позиция `PRFSLIDE` внутри слота (`offset = (48−14)·value/active`).
    pub fn slider_offset(&self, index: usize) -> i32 {
        let range = i32::from(SLIDER_BG.width) - SLIDER_SIZE.0;

        if self.active_centers == 0 {
            0
        } else {
            (range * i32::from(self.value(index))) / i32::from(self.active_centers)
        }
    }

    /// Уровень темы в процентах (`research_level * 10`).
    pub fn topic_percent(&self, state: &GameState, index: usize) -> u32 {
        state
            .teams
            .get(self.team.0 as usize)
            .and_then(|team| team.research_topics.get(index))
            .map(|topic| topic.research_level * 10)
            .unwrap_or(0)
    }

    /// «Ходов до завершения» для текущего числа центров темы.
    ///
    /// Повторяет `ResearchControl::RefreshScreen`: временно применяет
    /// `ResearchMenu_UpdateResearchProgress(team, topic, +1)`, считает
    /// `(centers + turns_to_complete − 1) / centers` и откатывает `−1`. Здесь
    /// изменение считается на копии `ResearchTopic` — состояние игры не
    /// трогается.
    pub fn turns_to_complete(&self, state: &GameState, index: usize) -> Option<u32> {
        let centers = i32::from(self.value(index));
        if centers <= 0 || index >= RESEARCH_TOPIC_COUNT {
            return None;
        }

        let mut topic = *state
            .teams
            .get(self.team.0 as usize)?
            .research_topics
            .get(index)?;
        update_research_progress(&mut topic, RESEARCH_TOPIC_FACTORS[index], 1);

        let turns = (centers + topic.turns_to_complete as i32 - 1) / centers;
        Some(turns.max(0) as u32)
    }

    /// `ResearchMenu_OnClick_Done`: применяет распределение к `GameState`.
    ///
    /// Возвращает фактическое распределение (C++ тоже может выключить центры,
    /// если в цели меньше мест, чем включённых центров).
    pub fn commit(&mut self, state: &mut GameState) -> [u32; RESEARCH_TOPIC_COUNT] {
        let target = self.values.map(u32::from);
        let actual = state.set_research_allocation(self.team, target);

        for (index, value) in self.values.iter_mut().enumerate() {
            *value = actual[index].min(u32::from(u16::MAX)) as u16;
        }
        self.free_capacity = 0;
        self.status = format!(
            "Research: {} center(s) assigned",
            actual.iter().sum::<u32>()
        );

        actual
    }

    /// Обрабатывает клик в кадре; возвращает действие.
    pub fn click(&mut self, x: i32, y: i32) -> ResearchAction {
        let window_x = x - WINDOW_RECT.ulx;
        let window_y = y - WINDOW_RECT.uly;

        if CANCEL_RECT.contains(window_x, window_y) {
            return ResearchAction::Cancel;
        }
        if HELP_RECT.contains(window_x, window_y) {
            return ResearchAction::Help;
        }
        if DONE_RECT.contains(window_x, window_y) {
            return ResearchAction::Done;
        }

        for index in 0..RESEARCH_TOPIC_COUNT {
            let uly = FIRST_ROW_Y + index as i32 * ROW_HEIGHT;
            let left = PixelRect::new(LEFT_ARROW.0, uly, ARROW_SIZE.0, ARROW_SIZE.1);
            let right = PixelRect::new(RIGHT_ARROW.0, uly, ARROW_SIZE.0, ARROW_SIZE.1);
            let slider = PixelRect::new(
                SLIDER_BG.ulx,
                uly + SLIDER_BG.uly,
                SLIDER_BG.width,
                SLIDER_BG.height,
            );

            if left.contains(window_x, window_y) {
                if !self.adjust(index, false) {
                    self.status = format!("{}: no centers to release", TOPIC_LABELS[index]);
                }
                return ResearchAction::None;
            }
            if right.contains(window_x, window_y) {
                if !self.adjust(index, true) {
                    self.status = format!("{}: no free research centers", TOPIC_LABELS[index]);
                }
                return ResearchAction::None;
            }
            if slider.contains(window_x, window_y) {
                self.slider(index, x);
                return ResearchAction::None;
            }
        }

        ResearchAction::None
    }

    /// Клавиатура (Rust-клиент; см. шапку модуля).
    pub fn key(&mut self, key: Key) -> ResearchAction {
        match key {
            Key::Escape => ResearchAction::Cancel,
            Key::Enter => ResearchAction::Done,
            Key::Up => {
                self.selected = self
                    .selected
                    .checked_sub(1)
                    .unwrap_or(RESEARCH_TOPIC_COUNT - 1);
                ResearchAction::None
            }
            Key::Down | Key::Tab => {
                self.selected = (self.selected + 1) % RESEARCH_TOPIC_COUNT;
                ResearchAction::None
            }
            Key::Left | Key::Char('-') | Key::Char('_') => {
                self.adjust(self.selected, false);
                ResearchAction::None
            }
            Key::Right | Key::Char('+') | Key::Char('=') => {
                self.adjust(self.selected, true);
                ResearchAction::None
            }
            Key::PageUp => {
                self.adjust(self.selected, false);
                ResearchAction::None
            }
            Key::PageDown => {
                self.adjust(self.selected, true);
                ResearchAction::None
            }
            Key::Char('h' | 'H' | '?') => ResearchAction::Help,
            Key::Backspace | Key::Char(_) | Key::Alt(_) | Key::Other => ResearchAction::None,
        }
    }

    /// Ввод события.
    pub fn input(&mut self, event: InputEvent) -> ResearchAction {
        match event {
            InputEvent::MouseClick(x, y) => self.click(x, y),
            InputEvent::Key(key) => self.key(key),
            _ => ResearchAction::None,
        }
    }

    /// Рисует экран поверх кадра 640×480 (`ResearchControl::Init/RefreshScreen`
    /// + `win_draw`).
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets, state: &GameState) -> u32 {
        if frame.width() != 640 || frame.height() != 480 {
            return 0;
        }

        let Some(menu_assets) = assets.research.as_ref() else {
            return 0;
        };
        let background = menu_assets.background.as_ref();
        let palette = background
            .map(UiPalette::from_big_image)
            .or_else(|| UiPalette::from_assets(assets))
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return 0;
        };
        let font = GnwFontId::Font5.font();
        let green = palette.rgba(GREEN);
        let text = palette.rgba(TEXT);
        let (gold_base, gold_outline, gold_shadow) = GOLD;
        let mut painted = 0;

        let window = WINDOW_RECT;
        if let Some(background) = background {
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

        // `Text_TextBox(buffer, …, "Research Topics", 95, 19, 170, 13,
        // COLOR_GREEN, true)`; vertical_align=true → `19 + ((13+1)−10)/2 = 21`.
        painted += draw_text_gnw_centered_shadowed(
            frame,
            PixelRect::new(
                window.ulx + TITLE_RECT.ulx,
                window.uly + TITLE_RECT.uly + 2,
                TITLE_RECT.width,
                font.height(),
            ),
            font,
            "Research Topics",
            green,
            palette.rgba(0x00),
            palette.rgba(0x00),
        )
        .max(0) as u32;

        // Подписи колонок: `Text_TextLine` с Fonts_GoldColor.
        for (rect, label) in [
            (LABS_LABEL, "Labs"),
            (TOPICS_LABEL, "Topics"),
            (TURNS_LABEL, "Turns"),
        ] {
            painted += draw_text_gnw_centered_shadowed(
                frame,
                PixelRect::new(
                    window.ulx + rect.ulx,
                    window.uly + rect.uly,
                    rect.width,
                    font.height(),
                ),
                font,
                label,
                palette.rgba(gold_base),
                palette.rgba(gold_outline),
                palette.rgba(gold_shadow),
            )
            .max(0) as u32;
        }

        for index in 0..RESEARCH_TOPIC_COUNT {
            let y = window.uly + FIRST_ROW_Y + index as i32 * ROW_HEIGHT;

            // `UPGLFT`/`UPGRGT`: левая доступна при centers > 0, правая при
            // `free_capacity > 0` (`UpdateButtons`).
            painted += draw_arrow(
                frame,
                &palette,
                menu_assets.left.as_ref(),
                menu_assets.left_disabled.as_ref(),
                PixelRect::new(window.ulx + LEFT_ARROW.0, y, ARROW_SIZE.0, ARROW_SIZE.1),
                self.value(index) > 0,
            );
            painted += draw_arrow(
                frame,
                &palette,
                menu_assets.right.as_ref(),
                menu_assets.right_disabled.as_ref(),
                PixelRect::new(window.ulx + RIGHT_ARROW.0, y, ARROW_SIZE.0, ARROW_SIZE.1),
                self.free_capacity > 0,
            );

            // `ReportStats_DrawNumber(topic_value, 32, …)`.
            let centers = self.value(index).to_string();
            let width = font.text_width(&centers);
            painted += draw_text_gnw(
                frame,
                window.ulx + i32::from(LABS_COUNT_BG.ulx + LABS_COUNT_BG.width) - 1 - width,
                y + LABS_COUNT_BG.uly,
                font,
                &centers,
                text,
            )
            .max(0) as u32;

            // `turns = (topic_value + turns_to_complete − 1) / topic_value`.
            if let Some(turns) = self.turns_to_complete(state, index) {
                let label = turns.to_string();
                let width = font.text_width(&label);
                painted += draw_text_gnw(
                    frame,
                    window.ulx + i32::from(TURNS_BG.ulx + TURNS_BG.width) - 1 - width,
                    y + TURNS_BG.uly,
                    font,
                    &label,
                    text,
                )
                .max(0) as u32;
            }

            // Слайдер: слот уже нарисован фоном, поверх — `PRFSLIDE`.
            if let Some(slide) = menu_assets.slider.as_ref() {
                painted += blit_simple(
                    frame,
                    &palette,
                    slide,
                    window.ulx + SLIDER_BG.ulx + self.slider_offset(index),
                    y + SLIDER_BG.uly,
                    true,
                );
            }

            // Подпись темы: по центру 49×15, `vertical_align=true` → y+2.
            painted += draw_text_gnw_centered_shadowed(
                frame,
                PixelRect::new(
                    window.ulx + TOPIC_LABEL.ulx,
                    y + 2,
                    TOPIC_LABEL.width,
                    font.height(),
                ),
                font,
                TOPIC_LABELS[index],
                palette.rgba(gold_base),
                palette.rgba(gold_outline),
                palette.rgba(gold_shadow),
            )
            .max(0) as u32;

            // Иконка: `WindowManager_DecodeSimpleImage(image, 170−w/2, uly+7−h/2)`.
            if let Some(icon) = menu_assets.icons[index].as_ref() {
                painted += blit_simple(
                    frame,
                    &palette,
                    icon,
                    window.ulx + TOPIC_ICON.0 - i32::from(icon.width) / 2,
                    y + TOPIC_ICON.1 - i32::from(icon.height) / 2,
                    true,
                );
            }

            // `+N%` у правого края 278, цвет 0xA2, без обводки.
            let percent = format!("+{}%", self.topic_percent(state, index));
            let width = font.text_width(&percent);
            painted += draw_text_gnw(
                frame,
                window.ulx + PERCENT_RIGHT - width,
                y + 3,
                font,
                &percent,
                text,
            )
            .max(0) as u32;
        }

        // Кнопки `Cancel`/`Help`/`Done`.
        if let Some(pair) = menu_assets.cancel.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                frame_rect(CANCEL_RECT),
                "Cancel",
                false,
            );
        }
        if let Some(pair) = menu_assets.help.as_ref() {
            painted += blit_simple(
                frame,
                &palette,
                pair.image(false),
                window.ulx + HELP_RECT.ulx,
                window.uly + HELP_RECT.uly,
                true,
            );
        }
        if let Some(pair) = menu_assets.done.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                frame_rect(DONE_RECT),
                "Done",
                false,
            );
        }

        painted
    }
}

/// Прямоугольник кнопки в координатах кадра.
fn frame_rect(rect: PixelRect) -> PixelRect {
    PixelRect::new(
        WINDOW_RECT.ulx + rect.ulx,
        WINDOW_RECT.uly + rect.uly,
        rect.width,
        rect.height,
    )
}

/// Стрелка с disabled-вариантом (`UPGLFT_X`/`UPGRGT_X` 16×16).
fn draw_arrow(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: Option<&MenuButtonPair>,
    disabled: Option<&SimpleImage>,
    rect: PixelRect,
    enabled: bool,
) -> u32 {
    let chosen = if enabled {
        image.map(|pair| pair.image(false))
    } else {
        disabled.or_else(|| image.map(|pair| pair.image(false)))
    };

    match chosen {
        Some(chosen) => blit_simple(frame, palette, chosen, rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// Ассеты экрана исследований.
#[derive(Debug, Clone)]
pub struct ResearchMenuAssets {
    /// Фон `RSRCHPIC` 360×331.
    pub background: Option<BigImage>,
    /// Иконки `I_HRDATK`/`I_SHOTS`/`I_RANGE`/`I_ARMOR`/`I_HITS`/`I_SPEED`/
    /// `I_SCAN`/`I_GOLD`.
    pub icons: [Option<SimpleImage>; RESEARCH_TOPIC_COUNT],
    /// `UPGLFT_U/D` 18×17.
    pub left: Option<MenuButtonPair>,
    /// `UPGLFT_X` 16×16.
    pub left_disabled: Option<SimpleImage>,
    /// `UPGRGT_U/D` 18×17.
    pub right: Option<MenuButtonPair>,
    /// `UPGRGT_X` 16×16.
    pub right_disabled: Option<SimpleImage>,
    /// `PRFSLIDE` 14×17.
    pub slider: Option<SimpleImage>,
    /// `DPTBAYUP/DN` 75×25 (Cancel/Done).
    pub cancel: Option<MenuButtonPair>,
    /// `DPTBAYUP/DN` 75×25 (Done; в C++ тот же ресурс).
    pub done: Option<MenuButtonPair>,
    /// `DPTHP_UP/DN` 31×26.
    pub help: Option<MenuButtonPair>,
}

impl ResearchMenuAssets {
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
                .read("RSRCHPIC")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            icons: TOPIC_ICONS.map(simple),
            left: pair("UPGLFT_U", "UPGLFT_D"),
            left_disabled: simple("UPGLFT_X"),
            right: pair("UPGRGT_U", "UPGRGT_D"),
            right_disabled: simple("UPGRGT_X"),
            slider: simple("PRFSLIDE"),
            cancel: pair("DPTBAYUP", "DPTBAYDN"),
            done: pair("DPTBAYUP", "DPTBAYDN"),
            help: pair("DPTHP_UP", "DPTHP_DN"),
        }
    }

    /// Есть ли фон и ключевые кнопки.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.done.is_some() && self.slider.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::ResearchTopic;
    use max_core::geo::Point;
    use max_core::research::research_turns_to_complete;
    use max_core::team::TeamId;

    fn state_with_center(topic: ResearchTopic, level: u32) -> (GameState, TeamId) {
        let mut state = GameState::new();
        state.teams[0].research_topics[topic as usize].research_level = level;
        let id = state
            .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(20, 20), 0)
            .expect("research center");
        let mut unit = state.unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.research_topic = topic;
        state.write_unit(id, unit);
        state.update_research_progress(TeamId::RED, topic, 1);
        (state, TeamId::RED)
    }

    #[test]
    fn layout_matches_the_oracle() {
        assert_eq!(WINDOW_RECT, PixelRect::new(223, 76, 360, 331));
        // `180 + (448-360)/2 = 224`? C++ целочисленно: (447-360)/2 = 43 → 223.
        assert_eq!(WINDOW_RECT.ulx, 180 + (447 - 360) / 2);
        assert_eq!(WINDOW_RECT.uly, 18 + (447 - 331) / 2);
        assert_eq!(FIRST_ROW_Y, 70);
        assert_eq!(ROW_HEIGHT, 28);
        assert_eq!(
            PixelRect::new(
                LEFT_ARROW.0,
                FIRST_ROW_Y + 7 * ROW_HEIGHT,
                ARROW_SIZE.0,
                ARROW_SIZE.1
            ),
            PixelRect::new(72, 266, 18, 17)
        );
        assert_eq!(
            frame_rect(DONE_RECT),
            PixelRect::new(223 + 194, 76 + 294, 75, 25)
        );
    }

    #[test]
    fn topic_order_matches_cpp() {
        let topics = [
            ResearchTopic::Attack,
            ResearchTopic::Shots,
            ResearchTopic::Range,
            ResearchTopic::Armor,
            ResearchTopic::Hits,
            ResearchTopic::Speed,
            ResearchTopic::Scan,
            ResearchTopic::Cost,
        ];

        for (index, topic) in topics.iter().enumerate() {
            assert_eq!(*topic as usize, index);
        }
        assert_eq!(TOPIC_LABELS[0], "Attack");
        assert_eq!(TOPIC_LABELS[7], "Cost");
        assert_eq!(TOPIC_ICONS[0], "I_HRDATK");
        assert_eq!(TOPIC_ICONS[7], "I_GOLD");
    }

    #[test]
    fn opening_counts_powered_centers_per_topic() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let menu = ResearchMenu::new(&state, team).expect("menu");

        assert_eq!(menu.active_centers, 1);
        assert_eq!(menu.values[ResearchTopic::Attack as usize], 1);
        assert_eq!(menu.free_capacity, 0, "в C++ free_capacity стартует с 0");
        assert_eq!(menu.slider_offset(ResearchTopic::Attack as usize), 48 - 14);
    }

    #[test]
    fn can_only_redistribute_released_centers() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(&state, team).expect("menu");

        // Наверх нельзя: свободных центров нет.
        assert!(!menu.adjust(ResearchTopic::Shots as usize, true));
        assert_eq!(menu.values[ResearchTopic::Shots as usize], 0);

        // Вниз по Attack освобождает центр...
        assert!(menu.adjust(ResearchTopic::Attack as usize, false));
        assert_eq!(menu.free_capacity, 1);
        assert_eq!(menu.values[ResearchTopic::Attack as usize], 0);

        // ...и его можно отдать Shots.
        assert!(menu.adjust(ResearchTopic::Shots as usize, true));
        assert_eq!(menu.free_capacity, 0);
        assert_eq!(menu.values[ResearchTopic::Shots as usize], 1);
    }

    #[test]
    fn slider_maps_pixels_to_centers_like_cpp() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let menu = ResearchMenu::new(&state, team).expect("menu");
        let left = WINDOW_RECT.ulx + SLIDER_BG.ulx;
        let right = left + SLIDER_BG.width;

        assert_eq!(menu.slider_value(0, left), 0);
        assert_eq!(menu.slider_value(0, right), 1);
    }

    #[test]
    fn done_reassigns_the_unit_and_keeps_allocation_consistent() {
        let (mut state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(&state, team).expect("menu");
        menu.adjust(ResearchTopic::Attack as usize, false);
        menu.adjust(ResearchTopic::Shots as usize, true);
        let actual = menu.commit(&mut state);

        assert_eq!(actual[ResearchTopic::Attack as usize], 0);
        assert_eq!(actual[ResearchTopic::Shots as usize], 1);

        let allocation = state.research_allocation(team);
        assert_eq!(allocation[ResearchTopic::Attack as usize], 0);
        assert_eq!(allocation[ResearchTopic::Shots as usize], 1);

        let unit = state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.unit(id))
            .find(|unit| unit.unit_type == UnitType::ResearchCentre)
            .expect("center");
        assert_eq!(unit.research_topic, ResearchTopic::Shots);
    }

    #[test]
    fn done_with_released_centers_powers_them_off() {
        let (mut state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(&state, team).expect("menu");
        menu.adjust(ResearchTopic::Attack as usize, false);
        let actual = menu.commit(&mut state);

        assert_eq!(
            actual.iter().sum::<u32>(),
            1,
            "центр ещё числится до Power Off"
        );
        let unit = state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.unit(id))
            .find(|unit| unit.unit_type == UnitType::ResearchCentre)
            .expect("center");
        assert_eq!(unit.orders, UnitOrder::PowerOff);

        // На следующем тике `process_order_power_off` спишет allocation
        // (`ResearchMenu_UpdateResearchProgress(-1)`), как в C++.
        state.tick();
        assert_eq!(state.research_allocation(team).iter().sum::<u32>(), 0);
    }

    #[test]
    fn turns_to_complete_shrink_with_more_centers() {
        let (mut state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(&state, team).expect("menu");
        let one = menu
            .turns_to_complete(&state, ResearchTopic::Attack as usize)
            .unwrap();
        assert!(one > 0);
        assert_eq!(
            one,
            research_turns_to_complete(0, RESEARCH_TOPIC_FACTORS[0]) as u32
        );

        // Второй центр: скорость темы меняется (детерминированная формула).
        let second = state
            .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(21, 20), 0)
            .expect("second center");
        let mut unit = state.unit(second).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.research_topic = ResearchTopic::Attack;
        state.write_unit(second, unit);
        state.update_research_progress(TeamId::RED, ResearchTopic::Attack, 1);

        menu = ResearchMenu::new(&state, team).expect("menu");
        assert_eq!(menu.value(ResearchTopic::Attack as usize), 2);
        let two = menu
            .turns_to_complete(&state, ResearchTopic::Attack as usize)
            .unwrap();
        assert!(two < one, "два центра быстрее одного: {two} < {one}");
    }

    #[test]
    fn research_new_turn_uses_the_same_distribution() {
        let (mut state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(&state, team).expect("menu");
        menu.adjust(ResearchTopic::Attack as usize, false);
        menu.adjust(ResearchTopic::Shots as usize, true);
        menu.commit(&mut state);

        // `finish_turn`/`advance_research` тикают ровно ту тему, на которую
        // перераспределили центры: Attack остаётся 0, Shots растёт.
        for _ in 0..16 {
            state.advance_research(team);
        }
        assert_eq!(
            state.teams[team.0 as usize].research_topics[ResearchTopic::Attack as usize]
                .research_level,
            0
        );
        assert!(
            state.teams[team.0 as usize].research_topics[ResearchTopic::Shots as usize]
                .research_level
                >= 1
        );
    }

    #[test]
    fn action_keys_and_clicks() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(&state, team).expect("menu");

        assert_eq!(menu.key(Key::Enter), ResearchAction::Done);
        assert_eq!(menu.key(Key::Escape), ResearchAction::Cancel);
        assert_eq!(menu.key(Key::Char('h')), ResearchAction::Help);

        // Кнопки в координатах кадра: Help (167..198) перекрывает левый край
        // Done (194..269), поэтому кликаем по центрам кнопок.
        assert_eq!(
            menu.click(WINDOW_RECT.ulx + 92 + 37, WINDOW_RECT.uly + 294 + 12),
            ResearchAction::Cancel
        );
        assert_eq!(
            menu.click(WINDOW_RECT.ulx + 167 + 15, WINDOW_RECT.uly + 294 + 13),
            ResearchAction::Help
        );
        assert_eq!(
            menu.click(WINDOW_RECT.ulx + 194 + 37, WINDOW_RECT.uly + 294 + 12),
            ResearchAction::Done
        );

        // Правая стрелка Attack при free_capacity = 0 ничего не делает.
        let right = PixelRect::new(
            WINDOW_RECT.ulx + RIGHT_ARROW.0,
            WINDOW_RECT.uly + FIRST_ROW_Y,
            ARROW_SIZE.0,
            ARROW_SIZE.1,
        );
        assert_eq!(
            menu.click(right.ulx + 1, right.uly + 1),
            ResearchAction::None
        );
        assert_eq!(menu.values[0], 1);

        // Левая стрелка освобождает центр.
        let left = PixelRect::new(
            WINDOW_RECT.ulx + LEFT_ARROW.0,
            WINDOW_RECT.uly + FIRST_ROW_Y,
            ARROW_SIZE.0,
            ARROW_SIZE.1,
        );
        assert_eq!(menu.click(left.ulx + 1, left.uly + 1), ResearchAction::None);
        assert_eq!(menu.values[0], 0);
        assert_eq!(menu.free_capacity, 1);
    }

    #[test]
    fn determinism_of_two_identical_menus() {
        let (mut a, team) = state_with_center(ResearchTopic::Attack, 0);
        let (mut b, _) = state_with_center(ResearchTopic::Attack, 0);

        let mut menu_a = ResearchMenu::new(&a, team).unwrap();
        let mut menu_b = ResearchMenu::new(&b, team).unwrap();
        for menu in [&mut menu_a, &mut menu_b] {
            menu.adjust(ResearchTopic::Attack as usize, false);
            menu.adjust(ResearchTopic::Hits as usize, true);
            menu.adjust(ResearchTopic::Hits as usize, false);
            menu.adjust(ResearchTopic::Scan as usize, true);
        }
        assert_eq!(menu_a.values, menu_b.values);
        assert_eq!(menu_a.free_capacity, menu_b.free_capacity);
        assert_eq!(menu_a.commit(&mut a), menu_b.commit(&mut b));
        assert_eq!(a.research_allocation(team), b.research_allocation(team));
    }
}
