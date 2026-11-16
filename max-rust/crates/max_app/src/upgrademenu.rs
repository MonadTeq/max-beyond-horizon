//! Экран покупных апгрейдов — C++ `AbstractUpgradeMenu` / `UpgradeMenu`
//! (`abstractupgrademenu.cpp`, `upgrademenu.cpp`, `upgradecontrol.cpp`).
//!
//! # Оракул
//!
//! Фон `UPGRADE` 640×480 (own palette), кнопки — те же ресурсы, что в C++:
//! `BLDONE`, `BLDHLP`, `BLDCAN` (y=452), `BLDUP__`/`BLDDWN` (471/491, 387),
//! `BLDDES` (292,264), `SELGRD`/`SELAIR`/`SELSEA`/`SELBLD`/`SELCBT`
//! (467/500/533/565/598, 411), `UPGLFT`/`UPGRGT` (283/301, ряд 293+19·i),
//! иконка `I_GOLD`.
//!
//! Раскладка:
//! * портрет юнита — область (11,13) 280×240 (в C++ — `GetPortrait()`, FLIC;
//!   здесь — спрайт 2×, как в `BuildMenu`);
//! * статистика — (11,293) 250×174; цена — (321,293) 40×174; стрелки
//!   апгрейдов — x=283/301; золото — (361,276) 40×140;
//! * список типов команды — (482,80) 143×290, 9 строк по 32 px
//!   (`UnitTypeSelector`: `max_item_count = height/32`), выбранная строка —
//!   `page_max_index` (в C++ `GetLast()` = `unit_types[page_max_index]`);
//! * заголовок `_(0b85)` («Unit Upgrades») — `Text_TextBox(327,7,158,18)`,
//!   подписи `Cost`/`Gold` — (320,283)/(358,284).
//!
//! # Отличия (осознанные)
//!
//! * `UnitStats_DrawStats` рисует сетку иконок `EI_*`; здесь — текстовая
//!   таблица тех же 13 атрибутов (шрифт 5) в той же области. Значения и
//!   формулы совпадают, картинка — нет; golden-кадр фиксирует нашу версию.
//! * Описание юнита (`BLDDES`/`GetDescription`) не портировано — кнопка есть,
//!   текста в данных Rust-клиента нет.
//! * Звуки `KCARG0`/`NDONE0`/`NCANC0` не проигрываются (аудио-слой отдельный).
//! * Меню открывается клавишей `U` для выделенного юнита своей команды (в C++
//!   путь — popup «Buy Upgrade» у `GOLDREF`, `units_manager.cpp:956`); сам
//!   popup-экран не портирован, поэтому клавиша заменяет пункт меню.
//! * Сеть: C++ на `CommitUpgradeChanges` шлёт пакет 09 (состояние команды) и по
//!   пакету 10 на каждый изменённый тип. Пакета 09 в Rust-протоколе нет,
//!   поэтому в сети отправляются только пакеты **10** (`Command::UpgradeUnitType`),
//!   а золото/`gold_spent` синхронизируются локально — ограничение
//!   задокументировано в `docs/architecture/protocol.md` §8.4.

use max_assets::image::{BigImage, SimpleImage};
use max_core::access::unit_type_flags;
use max_core::enums::{unit_flags, ResearchTopic, UnitType, UNIT_END};
use max_core::game::GameState;
use max_core::production::BUILDABLE;
use max_core::research::{
    upgrade_packet_values, UpgradeQuote, RESEARCH_TOPIC_FACTORS, UPGRADE_COST_LIMIT,
};
use max_core::team::TeamId;
use max_core::unit_values::{
    UnitValues, ATTRIB_AGENT_ADJUST, ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK,
    ATTRIB_ATTACK_RADIUS, ATTRIB_FUEL, ATTRIB_HITS, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN,
    ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS, BASE_UNIT_VALUES,
};
use max_core::victory::{
    cargo_fuel_consumption_rate, cargo_life_consumption_rate, cargo_power_consumption_rate,
    cargo_raw_consumption_rate,
};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::assets::Assets;
use crate::buildmenu::unit_name;
use crate::menu::{blit_simple, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::Key;

/// Ширина кадра оригинала.
pub const SCREEN_WIDTH: i32 = 640;
/// Высота кадра оригинала.
pub const SCREEN_HEIGHT: i32 = 480;

/// Портрет (C++ `unit_portrait_window` 11,13 + 280×240).
pub const PORTRAIT: PixelRect = PixelRect::new(11, 13, 280, 240);
/// Область статистики (`stats_background` 11,293,250,174).
pub const STATS: PixelRect = PixelRect::new(11, 293, 250, 174);
/// Колонка цен (`cost_background` 321,293,40,174).
pub const COST: PixelRect = PixelRect::new(321, 293, 40, 174);
/// Колонка стрелок апгрейдов (`button_background` 283,293,38,174).
pub const CONTROL_BUTTONS: PixelRect = PixelRect::new(283, 293, 38, 174);
/// Полоса золота (`gold_background` 361,276,40,140).
pub const GOLD_BAR: PixelRect = PixelRect::new(361, 276, 40, 140);
/// Список типов (`wininfo` 482,80 — 625,370).
pub const LIST_AREA: PixelRect = PixelRect::new(482, 80, 143, 290);
/// Высота строки списка (`UnitTypeSelector`: 32 px).
pub const LIST_ROW: i32 = 32;
/// Число видимых строк списка (`height / 32`).
pub const LIST_ROWS: i32 = 9;
/// Скролл вверх (`BLDUP__U`, 18×17).
pub const SCROLL_UP: PixelRect = PixelRect::new(471, 387, 18, 17);
/// Скролл вниз (`BLDDWN_U`, 18×17).
pub const SCROLL_DOWN: PixelRect = PixelRect::new(491, 387, 18, 17);
/// Описание (`BLDDES_U`, 17×17).
pub const DESCRIPTION: PixelRect = PixelRect::new(292, 264, 17, 17);
/// `Done` (`BLDONE_U`, 54×23).
pub const DONE: PixelRect = PixelRect::new(447, 452, 54, 23);
/// `Help` (`BLDHLP_U`, 26×23).
pub const HELP: PixelRect = PixelRect::new(420, 452, 26, 23);
/// `Cancel` (`BLDCAN_U`, 62×23).
pub const CANCEL: PixelRect = PixelRect::new(357, 452, 62, 23);
/// Заголовок (`Text_TextBox` 327,7,158,18).
pub const TITLE: PixelRect = PixelRect::new(327, 7, 158, 18);

/// Фильтры списка (C++ static rest-state: земля/воздух/море/здания вкл,
/// боевые выкл.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeFilter {
    /// Наземные мобильные.
    Ground,
    /// Воздушные.
    Air,
    /// Морские.
    Sea,
    /// Стационарные здания.
    Building,
    /// Только юниты с атакой (C++ `button_combat_rest_state`).
    Combat,
}

impl UpgradeFilter {
    /// Прямоугольник кнопки (32×31).
    pub const fn rect(self) -> PixelRect {
        match self {
            Self::Ground => PixelRect::new(467, 411, 32, 31),
            Self::Air => PixelRect::new(500, 411, 32, 31),
            Self::Sea => PixelRect::new(533, 411, 32, 31),
            Self::Building => PixelRect::new(565, 411, 32, 31),
            Self::Combat => PixelRect::new(598, 411, 32, 31),
        }
    }
}

/// Действие меню.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeAction {
    /// Ничего.
    None,
    /// `Cancel`/Esc.
    Cancel,
    /// `Done`/Enter — изменения зафиксированы.
    Commit,
}

/// Состояние открытого меню апгрейдов.
#[derive(Debug, Clone)]
pub struct UpgradeMenu {
    /// Команда.
    pub team: TeamId,
    /// Доступные типы (отфильтрованы, порядок C++ `IsBetterUnit`).
    pub unit_types: Vec<UnitType>,
    /// Выбранный индекс (`page_max_index`).
    pub selected: usize,
    /// Первая видимая строка (`page_min_index`).
    pub page_min: usize,
    /// Текущие значения на момент открытия (`unitvalues_base`).
    pub base: [UnitValues; UNIT_END as usize],
    /// Редактируемые значения (`unitvalues_actual`).
    pub actual: [UnitValues; UNIT_END as usize],
    /// Золото команды в меню.
    pub gold: u32,
    /// Золото на момент открытия.
    pub start_gold: u32,
    /// Фильтры (C++ static rest-state).
    pub filters: [bool; 5],
    /// Кнопка описания.
    pub description: bool,
    /// Строки апгрейдов выбранного типа: `(ATTRIB_*, uly)`.
    pub controls: Vec<(u8, i32)>,
    /// Строка статуса (ошибки).
    pub status: String,
}

impl UpgradeMenu {
    /// Открывает меню для команды — C++ `UpgradeMenu::UpgradeMenu(team, complex)`
    /// (complex не используется и в C++).
    pub fn new(state: &GameState, team: TeamId) -> Option<Self> {
        let team_index = team.0 as usize;
        let units = state.team_units.get(team_index)?;
        let base = units.current_values;
        let actual = base;

        let mut menu = Self {
            team,
            unit_types: Vec::new(),
            selected: 0,
            page_min: 0,
            base,
            actual,
            gold: units.gold,
            start_gold: units.gold,
            filters: [true, true, true, true, false],
            description: true,
            controls: Vec::new(),
            status: String::new(),
        };
        menu.rebuild_list(state, None);
        Some(menu)
    }

    /// Пересобирает список доступных типов (`PopulateTeamUnitsList`), сохраняя
    /// выбранный тип, если он остался.
    pub fn rebuild_list(&mut self, state: &GameState, keep: Option<UnitType>) {
        let previous = keep.or_else(|| self.selected_type());

        let mut types: Vec<UnitType> = (0..UNIT_END)
            .filter_map(|index| UnitType::try_from(index).ok())
            .filter(|unit_type| self.is_unit_filtered(state, *unit_type))
            .collect();

        // C++ `PopulateTeamUnitsList`: пузырьковая сортировка, обмен при
        // `IsBetterUnit(unit_types[i], unit_types[j])`.
        if types.len() > 1 {
            for i in 0..types.len() - 1 {
                for j in i + 1..types.len() {
                    if is_better_unit(types[i], types[j]) {
                        types.swap(i, j);
                    }
                }
            }
        }

        self.unit_types = types;
        self.selected = previous
            .and_then(|unit_type| self.unit_types.iter().position(|item| *item == unit_type))
            .unwrap_or(0);
        self.ensure_visible();
        self.rebuild_controls(state);
    }

    /// Выбранный тип.
    pub fn selected_type(&self) -> Option<UnitType> {
        self.unit_types.get(self.selected).copied()
    }

    /// `IsUnitFiltered` (`abstractupgrademenu.cpp:699`).
    fn is_unit_filtered(&self, state: &GameState, unit_type: UnitType) -> bool {
        let flags = unit_type_flags(unit_type);
        if flags & unit_flags::UPGRADABLE == 0 || !is_buildable(unit_type) {
            return false;
        }

        let class_matches = (flags & unit_flags::MOBILE_LAND_UNIT != 0 && self.filters[0])
            || (flags & unit_flags::MOBILE_AIR_UNIT != 0 && self.filters[1])
            || (flags & unit_flags::MOBILE_SEA_UNIT != 0 && self.filters[2])
            || (flags & unit_flags::STATIONARY != 0 && self.filters[3]);
        if !class_matches {
            return false;
        }

        if self.filters[4] {
            // C++: `UnitsManager_TeamInfo[0]` base values, а не команда меню.
            let base = state
                .team_units
                .first()
                .map(|units| units.base_unit_values(unit_type as u16).attack)
                .unwrap_or(0);
            base > 0
        } else {
            true
        }
    }

    /// `IsBetterUnit` (`abstractupgrademenu.cpp:727`).
    fn ensure_visible(&mut self) {
        if self.selected < self.page_min {
            self.page_min = self.selected;
        }
        let max_page = self.unit_types.len().saturating_sub(LIST_ROWS as usize);
        self.page_min = self.page_min.min(max_page);
        if self.selected >= self.page_min + LIST_ROWS as usize {
            self.page_min = self.selected + 1 - LIST_ROWS as usize;
        }
    }

    /// Прокрутка выбора (`UnitTypeSelector::ProcessKeys` UP/DOWN).
    pub fn step(&mut self, delta: i32, state: &GameState) {
        if self.unit_types.is_empty() {
            return;
        }
        let last = self.unit_types.len() - 1;
        self.selected = (self.selected as i32 + delta).clamp(0, last as i32) as usize;
        self.ensure_visible();
        self.rebuild_controls(state);
    }

    /// Прокрутка страницы (PageUp/PageDown — `max_item_count` строк).
    pub fn page(&mut self, delta: i32, state: &GameState) {
        self.step(delta * LIST_ROWS, state);
    }

    /// Переключает фильтр и пересобирает список.
    pub fn toggle_filter(&mut self, filter: UpgradeFilter, state: &GameState) {
        let index = filter as usize;
        self.filters[index] = !self.filters[index];
        self.rebuild_list(state, None);
    }

    /// Собирает контролы апгрейдов для выбранного типа — C++
    /// `DrawUnitInfo` → `AddUpgradeMilitary/AdjustRow*/AddUpgradeGeneric/Mobile`.
    pub fn rebuild_controls(&mut self, state: &GameState) {
        self.controls.clear();
        let Some(unit_type) = self.selected_type() else {
            return;
        };
        let index = unit_type as usize;
        let true_base = &state.team_units[self.team.0 as usize].base_values[index];
        let actual = self.actual[index];
        let mut uly = COST.uly;
        let mut rows: Vec<(u8, i32)> = Vec::new();
        fn push(rows: &mut Vec<(u8, i32)>, uly: &mut i32, attribute: u8) {
            rows.push((attribute, *uly));
            *uly += 19;
        }

        // `AddUpgradeMilitary`.
        if true_base.get_attribute(ATTRIB_ATTACK) > 0 {
            push(&mut rows, &mut uly, ATTRIB_ATTACK);
            if true_base.get_attribute(ATTRIB_ROUNDS) > 0 {
                push(&mut rows, &mut uly, ATTRIB_ROUNDS);
            }
            if true_base.get_attribute(ATTRIB_RANGE) > 0 {
                push(&mut rows, &mut uly, ATTRIB_RANGE);
            }
            if true_base.get_attribute(ATTRIB_AMMO) > 0 {
                push(&mut rows, &mut uly, ATTRIB_AMMO);
            }
        }

        // `AdjustRowStorage`.
        if actual.get_attribute(ATTRIB_STORAGE) != 0
            && matches!(
                unit_type,
                UnitType::ADump
                    | UnitType::FDump
                    | UnitType::GoldSmelter
                    | UnitType::Engineer
                    | UnitType::Constructor
                    | UnitType::CargoShip
                    | UnitType::SupplyTruck
                    | UnitType::FuelTruck
                    | UnitType::GoldTruck
            )
        {
            uly += 19;
        }

        // `AdjustRowConsumptions`.
        let raw = cargo_raw_consumption_rate(unit_type, 1);
        let fuel = cargo_fuel_consumption_rate(unit_type);
        let power = cargo_power_consumption_rate(unit_type);
        let life = cargo_life_consumption_rate(unit_type);
        if power < 0 {
            uly += 19;
        }
        if life < 0 {
            uly += 19;
        }
        if raw > 0 || fuel > 0 || power > 0 || life > 0 {
            uly += 19;
        }

        // `AddUpgradeGeneric`.
        if true_base.get_attribute(ATTRIB_ARMOR) > 0 {
            push(&mut rows, &mut uly, ATTRIB_ARMOR);
        }
        if true_base.get_attribute(ATTRIB_HITS) > 0 {
            push(&mut rows, &mut uly, ATTRIB_HITS);
        }
        if true_base.get_attribute(ATTRIB_SCAN) > 0 {
            push(&mut rows, &mut uly, ATTRIB_SCAN);
        }

        // `AddUpgradeMobile`.
        if true_base.get_attribute(ATTRIB_SPEED) > 0 {
            push(&mut rows, &mut uly, ATTRIB_SPEED);
        }

        self.controls = rows;
    }

    /// Контрол-цитата строки для выбранного типа.
    pub fn quote(&self, state: &GameState, row: usize) -> Option<UpgradeQuote> {
        let (attribute, _) = *self.controls.get(row)?;
        let unit_type = self.selected_type()?;
        let index = unit_type as usize;
        let true_base = state.team_units[self.team.0 as usize].base_values[index];
        let team_base = true_base.get_attribute(attribute) as u16;

        let level = if attribute as usize >= RESEARCH_TOPIC_FACTORS.len() {
            0
        } else {
            match ResearchTopic::try_from(attribute).ok() {
                Some(topic) => state.teams[self.team.0 as usize].research_topics[topic as usize]
                    .research_level,
                None => 0,
            }
        };
        let factor = max_core::research::research_factor(i32::from(team_base), level) as u16;

        let mut quote = UpgradeQuote::new(
            attribute,
            team_base,
            self.base[index].get_attribute(attribute) as u16,
            factor,
        );
        quote.actual = self.actual[index].get_attribute(attribute) as u16;
        Some(quote)
    }

    /// Строка (ATTRIB) -> индекс контрола.
    pub fn control_at(&self, x: i32, y: i32) -> Option<(usize, bool)> {
        for (row, (_, uly)) in self.controls.iter().enumerate() {
            let left = PixelRect::new(CONTROL_BUTTONS.ulx, *uly, 18, 17);
            let right = PixelRect::new(CONTROL_BUTTONS.ulx + 18, *uly, 18, 17);
            if left.contains(x, y) {
                return Some((row, false));
            }
            if right.contains(x, y) {
                return Some((row, true));
            }
        }
        None
    }

    /// Нажатие на стрелку апгрейда: `false`, если изменение невозможно.
    pub fn adjust(&mut self, state: &GameState, row: usize, increase: bool) -> bool {
        let Some(unit_type) = self.selected_type() else {
            return false;
        };
        let index = unit_type as usize;
        let Some(mut quote) = self.quote(state, row) else {
            return false;
        };

        let changed = if increase {
            quote.increase(&mut self.gold)
        } else {
            quote.decrease(&mut self.gold)
        };

        if changed {
            self.actual[index].set_attribute(quote.attribute, i32::from(quote.actual));
        }
        changed
    }

    /// Обрабатывает клик; возвращает действие.
    pub fn click(&mut self, state: &GameState, x: i32, y: i32) -> UpgradeAction {
        if DONE.contains(x, y) {
            return UpgradeAction::Commit;
        }
        if CANCEL.contains(x, y) {
            return UpgradeAction::Cancel;
        }
        if HELP.contains(x, y) {
            self.status = "Help: upgrades raise unit attributes; Done pays gold".to_string();
            return UpgradeAction::None;
        }
        if SCROLL_UP.contains(x, y) {
            self.step(-1, state);
            return UpgradeAction::None;
        }
        if SCROLL_DOWN.contains(x, y) {
            self.step(1, state);
            return UpgradeAction::None;
        }
        if DESCRIPTION.contains(x, y) {
            self.description = !self.description;
            return UpgradeAction::None;
        }
        for filter in [
            UpgradeFilter::Ground,
            UpgradeFilter::Air,
            UpgradeFilter::Sea,
            UpgradeFilter::Building,
            UpgradeFilter::Combat,
        ] {
            if filter.rect().contains(x, y) {
                self.toggle_filter(filter, state);
                return UpgradeAction::None;
            }
        }

        for row in 0..LIST_ROWS {
            let row_rect = PixelRect::new(
                LIST_AREA.ulx,
                LIST_AREA.uly + row * LIST_ROW,
                LIST_AREA.width,
                LIST_ROW,
            );
            if row_rect.contains(x, y) {
                let index = self.page_min + row as usize;
                if index < self.unit_types.len() {
                    self.selected = index;
                    self.ensure_visible();
                    self.rebuild_controls(state);
                }
                return UpgradeAction::None;
            }
        }

        if let Some((row, increase)) = self.control_at(x, y) {
            if !self.adjust(state, row, increase) {
                self.status = if increase {
                    "Cannot upgrade: gold or cost limit".to_string()
                } else {
                    "Already at base value".to_string()
                };
            }
        }

        UpgradeAction::None
    }

    /// Клавиатура.
    pub fn key(&mut self, state: &GameState, key: Key) -> UpgradeAction {
        match key {
            Key::Escape => UpgradeAction::Cancel,
            Key::Enter => UpgradeAction::Commit,
            Key::Up => {
                self.step(-1, state);
                UpgradeAction::None
            }
            Key::Down => {
                self.step(1, state);
                UpgradeAction::None
            }
            Key::PageUp => {
                self.page(-1, state);
                UpgradeAction::None
            }
            Key::PageDown => {
                self.page(1, state);
                UpgradeAction::None
            }
            _ => UpgradeAction::None,
        }
    }

    /// `CommitUpgradeChanges`: записать изменённые типы в `TeamUnits`, списать
    /// золото и `stats_gold_spent_on_upgrades`.
    ///
    /// Возвращает список изменённых типов для пакета 10 (сеть).
    pub fn commit(&mut self, state: &mut GameState) -> Vec<(UnitType, [u16; 13])> {
        let mut changed = Vec::new();
        let team_index = self.team.0 as usize;

        for index in 0..UNIT_END as usize {
            if self.actual[index] == self.base[index] {
                continue;
            }
            let mut values = self.actual[index];
            values.update_version();
            values.mark_as_not_in_use();
            state.team_units[team_index].current_values[index] = values;
            if let Ok(unit_type) = UnitType::try_from(index as u16) {
                changed.push((unit_type, upgrade_packet_values(&values)));
            }
        }

        state.team_units[team_index].gold = self.gold;
        let spent = self.start_gold.saturating_sub(self.gold);
        state.teams[team_index].stats_gold_spent_on_upgrades = state.teams[team_index]
            .stats_gold_spent_on_upgrades
            .saturating_add(spent);

        changed
    }

    /// Рисует экран поверх кадра (C++ `DrawUnitStats` + `win_draw`).
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets, state: &GameState) -> u32 {
        if frame.width() != SCREEN_WIDTH as u32 || frame.height() != SCREEN_HEIGHT as u32 {
            return 0;
        }

        let Some(menu_assets) = assets.upgrade.as_ref() else {
            return 0;
        };
        let background = menu_assets.background.as_ref();
        frame.fill_rect(
            PixelRect::new(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT),
            max_render::buffer::Rgba::rgb(0, 0, 0),
        );
        let mut painted = match background {
            Some(background) => crate::menu::draw_big_image(frame, background),
            None => 0,
        };
        let palette = background
            .map(UiPalette::from_big_image)
            .or_else(|| UiPalette::from_assets(assets))
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return painted;
        };
        let font = GnwFontId::Font5.font();
        let green = palette.rgba(crate::setup::GREEN);
        let text = palette.rgba(crate::setup::TEXT);
        let yellow = palette.rgba(crate::setup::YELLOW.0);
        let dim = palette.rgba(0x5A);

        // Заголовок.
        painted += draw_text_gnw_centered(frame, TITLE, font, "Unit Upgrades", green).max(0) as u32;
        painted += draw_text_gnw(frame, COST.ulx - 1, 283, font, "Cost", text).max(0) as u32;
        painted += draw_text_gnw(frame, GOLD_BAR.ulx - 3, 284, font, "Gold", text).max(0) as u32;

        // Портрет 2× (C++ `GetPortrait`, FLIC не портирован).
        if let Some(unit_type) = self.selected_type() {
            if let Some(sprite) = assets.sprite(unit_type) {
                if let Some(sprite_frame) = sprite.frame_for(0, 0) {
                    let colors = max_render::TeamColors::ALL
                        .get(self.team.0 as usize)
                        .copied()
                        .unwrap_or(max_render::TeamColors::Derelict);
                    let renderer = max_render::SpriteRenderer::new(
                        &assets.palette,
                        &assets.brightness,
                        max_render::ColorLut::for_team(colors),
                        max_render::FULL_BRIGHTNESS_LEVEL,
                    );
                    renderer.draw_unit_scaled(
                        frame,
                        sprite_frame,
                        None,
                        (PORTRAIT.ulx + 140, PORTRAIT.uly + 120),
                        (0, 0),
                        128,
                    );

                    // Башня/пушка отдельным кадром (как на карте): C++
                    // `GetPortrait()` — big image, где техника уже с башней.
                    if let Some(turret) = sprite.turret_frame(0) {
                        renderer.draw_turret_scaled(
                            frame,
                            turret,
                            None,
                            (PORTRAIT.ulx + 140, PORTRAIT.uly + 120),
                            (0, 0),
                            sprite.turret_offset(0),
                            128,
                        );
                    }

                    painted += 1;
                }
            }

            painted += draw_text_gnw(
                frame,
                STATS.ulx + 2,
                STATS.uly + 2,
                font,
                unit_name(unit_type),
                green,
            )
            .max(0) as u32;

            // Таблица атрибутов (C++ `UnitStats_DrawStats`, здесь текстом).
            let values = &self.actual[unit_type as usize];
            let labels: [(u8, &str); 13] = [
                (ATTRIB_ATTACK, "Attack"),
                (ATTRIB_ROUNDS, "Rounds"),
                (ATTRIB_RANGE, "Range"),
                (ATTRIB_AMMO, "Ammo"),
                (ATTRIB_ARMOR, "Armor"),
                (ATTRIB_HITS, "Hits"),
                (ATTRIB_SPEED, "Speed"),
                (ATTRIB_SCAN, "Scan"),
                (ATTRIB_TURNS, "Turns"),
                (ATTRIB_STORAGE, "Storage"),
                (ATTRIB_FUEL, "Fuel"),
                (ATTRIB_ATTACK_RADIUS, "Blast"),
                (ATTRIB_AGENT_ADJUST, "Agent"),
            ];
            for (row, (attribute, label)) in labels.iter().enumerate() {
                let y = STATS.uly + 16 + row as i32 * 12;
                if y + 12 > STATS.lry() {
                    break;
                }
                painted += draw_text_gnw(frame, STATS.ulx + 2, y, font, label, text).max(0) as u32;
                let value = values.get_attribute(*attribute).to_string();
                let width = font.text_width(&value);
                painted += draw_text_gnw(
                    frame,
                    STATS.ulx + 120 - width,
                    y,
                    font,
                    &value,
                    yellow,
                )
                .max(0) as u32;
            }
        }

        // Строки апгрейдов: стрелки, цена.
        for (row, (attribute, uly)) in self.controls.iter().enumerate() {
            let quote = self.quote(state, row);
            let left_rect = PixelRect::new(CONTROL_BUTTONS.ulx, *uly, 18, 17);
            let right_rect = PixelRect::new(CONTROL_BUTTONS.ulx + 18, *uly, 18, 17);

            let can_up = quote.as_ref().is_some_and(|quote| quote.can_increase(self.gold));
            let can_down = quote.as_ref().is_some_and(UpgradeQuote::can_decrease);

            painted += draw_upgrade_arrow(
                frame,
                &palette,
                menu_assets.left.as_ref(),
                menu_assets.left_disabled.as_ref(),
                left_rect,
                can_down,
            );
            painted += draw_upgrade_arrow(
                frame,
                &palette,
                menu_assets.right.as_ref(),
                menu_assets.right_disabled.as_ref(),
                right_rect,
                can_up,
            );

            if let Some(quote) = quote {
                let cost = quote.cost();
                if cost > 0 && cost < UPGRADE_COST_LIMIT {
                    if let Some(icon) = menu_assets.gold_icon.as_ref() {
                        painted += blit_simple(frame, &palette, icon, COST.ulx + 2, *uly + 4, true);
                    }
                    let label = cost.to_string();
                    let width = font.text_width(&label);
                    painted += draw_text_gnw(
                        frame,
                        COST.ulx + COST.width - width - 2,
                        *uly + 4,
                        font,
                        &label,
                        if !can_up { dim } else { yellow },
                    )
                    .max(0) as u32;
                } else if cost >= UPGRADE_COST_LIMIT {
                    painted += draw_text_gnw(
                        frame,
                        COST.ulx + 2,
                        *uly + 4,
                        font,
                        "max",
                        dim,
                    )
                    .max(0) as u32;
                }
            }

            // Подпись атрибута (в C++ колонка уже; здесь помогает
            // ориентироваться между таблицей и строками цен).
            let label = match *attribute {
                ATTRIB_ATTACK => "ATK",
                ATTRIB_ROUNDS => "RND",
                ATTRIB_RANGE => "RNG",
                ATTRIB_AMMO => "AMMO",
                ATTRIB_ARMOR => "ARM",
                ATTRIB_HITS => "HITS",
                ATTRIB_SCAN => "SCAN",
                ATTRIB_SPEED => "SPD",
                _ => "",
            };
            painted += draw_text_gnw(
                frame,
                STATS.ulx + 140,
                *uly + 4,
                font,
                label,
                dim,
            )
            .max(0) as u32;
        }

        // Список типов команды.
        for row in 0..LIST_ROWS as usize {
            let index = self.page_min + row;
            let Some(unit_type) = self.unit_types.get(index).copied() else {
                break;
            };
            let y = LIST_AREA.uly + row as i32 * LIST_ROW + 8;
            let selected = index == self.selected;
            let color = if selected { green } else { text };
            let name = fit_label(unit_name(unit_type), LIST_AREA.width - 6, font);
            painted += draw_text_gnw(frame, LIST_AREA.ulx + 3, y, font, &name, color).max(0) as u32;
        }

        // Фильтры.
        for (filter, pair) in [
            (UpgradeFilter::Ground, menu_assets.filter_ground.as_ref()),
            (UpgradeFilter::Air, menu_assets.filter_air.as_ref()),
            (UpgradeFilter::Sea, menu_assets.filter_sea.as_ref()),
            (UpgradeFilter::Building, menu_assets.filter_building.as_ref()),
            (UpgradeFilter::Combat, menu_assets.filter_combat.as_ref()),
        ] {
            let active = self.filters[filter as usize];
            if let Some(pair) = pair {
                painted += blit_simple(
                    frame,
                    &palette,
                    pair.image(!active),
                    filter.rect().ulx,
                    filter.rect().uly,
                    true,
                );
            }
        }

        // Кнопка описания.
        if let Some(pair) = menu_assets.description.as_ref() {
            painted += blit_simple(
                frame,
                &palette,
                pair.image(!self.description),
                DESCRIPTION.ulx,
                DESCRIPTION.uly,
                true,
            );
        }

        // Кнопки Done/Help/Cancel.
        if let Some(pair) = menu_assets.done.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                DONE,
                "Done",
                false,
            );
        }
        if let Some(pair) = menu_assets.cancel.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                CANCEL,
                "Cancel",
                false,
            );
        }
        if let Some(pair) = menu_assets.help.as_ref() {
            painted += blit_simple(frame, &palette, pair.image(false), HELP.ulx, HELP.uly, true);
        }

        // Стрелки списка.
        if let Some(pair) = menu_assets.scroll_up.as_ref() {
            painted += blit_simple(
                frame,
                &palette,
                pair.image(false),
                SCROLL_UP.ulx,
                SCROLL_UP.uly,
                true,
            );
        }
        if let Some(pair) = menu_assets.scroll_down.as_ref() {
            painted += blit_simple(
                frame,
                &palette,
                pair.image(false),
                SCROLL_DOWN.ulx,
                SCROLL_DOWN.uly,
                true,
            );
        }

        // Полоса золота: C++ `LoadVerticalBar(..., VERTGOLD)`.
        let bar_top = GOLD_BAR.uly + 25;
        let bar_height = GOLD_BAR.height - 25;
        let fill = if self.start_gold > 0 {
            ((bar_height as i64 * i64::from(self.gold)) / i64::from(self.start_gold))
                .clamp(0, i64::from(bar_height)) as i32
        } else {
            0
        };
        frame.fill_rect(
            PixelRect::new(GOLD_BAR.ulx, bar_top, GOLD_BAR.width, bar_height),
            palette.rgba(0x18),
        );
        frame.fill_rect(
            PixelRect::new(
                GOLD_BAR.ulx,
                bar_top + (bar_height - fill),
                GOLD_BAR.width,
                fill,
            ),
            yellow,
        );
        painted += 1;

        let gold_label = self.gold.to_string();
        let width = font.text_width(&gold_label);
        painted += draw_text_gnw(
            frame,
            GOLD_BAR.ulx + (GOLD_BAR.width - width) / 2,
            GOLD_BAR.uly,
            font,
            &gold_label,
            yellow,
        )
        .max(0) as u32;

        if !self.status.is_empty() {
            painted += draw_text_gnw(
                frame,
                STATS.ulx + 2,
                STATS.lry() - 12,
                font,
                &fit_label(&self.status, STATS.width - 4, font),
                green,
            )
            .max(0) as u32;
        }

        painted
    }
}

/// Стрелка `UPGLFT`/`UPGRGT` (disabled-вариант `UPGLFT_X`/`UPGRGT_X`).
fn draw_upgrade_arrow(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: Option<&SimpleImage>,
    disabled: Option<&SimpleImage>,
    rect: PixelRect,
    enabled: bool,
) -> u32 {
    let chosen = if enabled {
        image
    } else {
        disabled.or(image)
    };
    match chosen {
        Some(chosen) => blit_simple(frame, palette, chosen, rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// Обрезка длинной подписи (как `buildmenu::fit_label`).
fn fit_label(text: &str, max_width: i32, font: &max_render::text::GnwFont) -> String {
    if font.text_width(text) <= max_width {
        return text.to_string();
    }
    let mut result = String::new();
    for character in text.chars() {
        let mut candidate = result.clone();
        candidate.push(character);
        if font.text_width(&candidate) > max_width {
            break;
        }
        result = candidate;
    }
    result
}

/// `AbstractUpgradeMenu::IsBuildable` (`abstractupgrademenu.cpp:681`).
///
/// Игровые правила `max_is_buildable` не зависят от команды; мины — особый
/// случай: они не строятся, но апгрейдятся, если строится их постановщик.
pub fn is_buildable(unit_type: UnitType) -> bool {
    match unit_type {
        UnitType::LandMine => is_buildable(UnitType::MineLayer),
        UnitType::SeaMine => is_buildable(UnitType::SeaMineLayer),
        _ => in_builder_lists(unit_type),
    }
}

fn in_builder_lists(unit_type: UnitType) -> bool {
    BUILDABLE
        .iter()
        .any(|(_, units)| units.contains(&unit_type))
}

/// `AbstractUpgradeMenu::IsBetterUnit` (`abstractupgrademenu.cpp:727`).
pub fn is_better_unit(unit_type1: UnitType, unit_type2: UnitType) -> bool {
    let flags1 = unit_type_flags(unit_type1);
    let flags2 = unit_type_flags(unit_type2);

    if (flags1 & unit_flags::MOBILE_LAND_UNIT) != (flags2 & unit_flags::MOBILE_LAND_UNIT) {
        return flags1 & unit_flags::MOBILE_LAND_UNIT == 0;
    }
    if (flags1 & unit_flags::MOBILE_SEA_UNIT) != (flags2 & unit_flags::MOBILE_SEA_UNIT) {
        return flags1 & unit_flags::MOBILE_SEA_UNIT == 0;
    }
    if (flags1 & unit_flags::MOBILE_AIR_UNIT) != (flags2 & unit_flags::MOBILE_AIR_UNIT) {
        return flags1 & unit_flags::MOBILE_AIR_UNIT == 0;
    }

    let values1 = BASE_UNIT_VALUES[unit_type1 as usize];
    let values2 = BASE_UNIT_VALUES[unit_type2 as usize];

    if values1.attack == 0 || values2.attack != 0 {
        if values1.attack != 0 || values2.attack == 0 {
            if values1.turns == values2.turns {
                (unit_type1 as u16) > (unit_type2 as u16)
            } else {
                values1.turns > values2.turns
            }
        } else {
            true
        }
    } else {
        false
    }
}

/// Ассеты экрана апгрейдов из `MAX.RES`.
#[derive(Debug, Clone)]
pub struct UpgradeMenuAssets {
    /// Фон `UPGRADE` 640×480.
    pub background: Option<BigImage>,
    /// Стрелка влево `UPGLFT_U/D`.
    pub left: Option<SimpleImage>,
    /// `UPGLFT_X` — недоступна.
    pub left_disabled: Option<SimpleImage>,
    /// Стрелка вправо `UPGRGT_U/D`.
    pub right: Option<SimpleImage>,
    /// `UPGRGT_X`.
    pub right_disabled: Option<SimpleImage>,
    /// Иконка золота `I_GOLD`.
    pub gold_icon: Option<SimpleImage>,
    /// `BLDUP__U/D`.
    pub scroll_up: Option<MenuButtonPair>,
    /// `BLDDWN_U/D`.
    pub scroll_down: Option<MenuButtonPair>,
    /// `BLDDES_U/D`.
    pub description: Option<MenuButtonPair>,
    /// `BLDONE_U/D`.
    pub done: Option<MenuButtonPair>,
    /// `BLDHLP_U/D`.
    pub help: Option<MenuButtonPair>,
    /// `BLDCAN_U/D`.
    pub cancel: Option<MenuButtonPair>,
    /// `SELGRD_U/D`.
    pub filter_ground: Option<MenuButtonPair>,
    /// `SELAIR_U/D`.
    pub filter_air: Option<MenuButtonPair>,
    /// `SELSEA_U/D`.
    pub filter_sea: Option<MenuButtonPair>,
    /// `SELBLD_U/D`.
    pub filter_building: Option<MenuButtonPair>,
    /// `SELCBT_U/D`.
    pub filter_combat: Option<MenuButtonPair>,
}

impl UpgradeMenuAssets {
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
                .read("UPGRADE")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            left: simple("UPGLFT_U"),
            left_disabled: simple("UPGLFT_X"),
            right: simple("UPGRGT_U"),
            right_disabled: simple("UPGRGT_X"),
            gold_icon: simple("I_GOLD"),
            scroll_up: pair("BLDUP__U", "BLDUP__D"),
            scroll_down: pair("BLDDWN_U", "BLDDWN_D"),
            description: pair("BLDDES_U", "BLDDES_D"),
            done: pair("BLDONE_U", "BLDONE_D"),
            help: pair("BLDHLP_U", "BLDHLP_D"),
            cancel: pair("BLDCAN_U", "BLDCAN_D"),
            filter_ground: pair("SELGRD_U", "SELGRD_D"),
            filter_air: pair("SELAIR_U", "SELAIR_D"),
            filter_sea: pair("SELSEA_U", "SELSEA_D"),
            filter_building: pair("SELBLD_U", "SELBLD_D"),
            filter_combat: pair("SELCBT_U", "SELCBT_D"),
        }
    }

    /// Есть ли фон и ключевые кнопки.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.done.is_some() && self.cancel.is_some()
    }
}
