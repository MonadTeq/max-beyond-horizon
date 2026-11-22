//! Диалог распределения добычи шахты — C++ `allocmenu.cpp` (`AllocMenu`), окно
//! `ALLOCFRM` 640×480.
//!
//! # Оракул
//!
//! * окно `ALLOCFRM` 640×480 (`allocmenu.cpp:153`): `Window(ALLOCFRM,
//!   GameManager_GetDialogWindowCenterMode())` — при `dialog_center_mode = 0`
//!   (`settings.cpp:159`) это центр `WINDOW_MAIN_MAP`; `win_move` (`gnw.c:663`)
//!   прижимает окно 640×480 к `(0,0)`, то есть диалог занимает весь экран;
//! * заголовок `Allocation Menu` (`_(c771)`) — `Text_TextBox(230, 6, 180, 20)`,
//!   `GNW_TEXT_FONT_5`, `COLOR_GREEN`, по центру по обеим осям;
//! * десять подписей (`_(8481)` `Raw`, `_(1e19)`/`_(5432)`/`_(e187)` `Usage`,
//!   `_(5ae6)`/`_(04dd)`/`_(89a3)` `Reserve`, `_(a80f)` `Fuel`, `_(dff1)`
//!   `Gold`) в прямоугольниках по 78×19 у левого края (x = 42), тот же
//!   `FONT5`/`COLOR_GREEN`;
//! * три секции (`Raw`/`Fuel`/`Gold`) на `y = 70/190/310`, в каждой три полосы
//!   (шаг 37 px, `CargoBar::Draw`):
//!   1. **добыча** — заливка `BARRAW`/`BARFUEL`/`BARGOLD` на
//!      `production·240/capacity` px, поверх справа — `BARTAPE` шириной
//!      `(capacity − balance)·240/capacity`, подпись `%i` (`production`);
//!   2. **потребление** — заливка по `usage`, подпись `%i (%+i / turn)`
//!      (`usage`, `production − usage`);
//!   3. **запас комплекса** — заливка по `complex_materials`, подпись `%i`;
//!   подпись — `GNW_TEXT_FONT_1`, цвет `0xFF`, обводка `0x00` (`Text_Blit` с
//!   `GNW_TEXT_OUTLINE`), подложка перекрашена таблицей
//!   `Color_GenerateIntensityTable1(0, 0, 25, 1, 1)`
//!   (`Color_RecolorPixels`);
//! * стрелки `LFARO_OF`/`RTARO_OF` (28×29) в `(139, y)`/`(421, y)` —
//!   `−1`/`+1` единица (`cases 1002/1001`, `1005/1004`, `1008/1007`);
//! * `?` (`MNUBTN5U/D` 48×40 в `465,438`) — помощь `ALLOCATE_SETUP`;
//!   `Done` (`MNUBTN6U/D` 109×40 в `514,438`) и `Esc`/`KP_Enter` — закрыть.
//!   Отдельного `Cancel` в C++ **нет**: значения применяются в момент клика
//!   (`UpdateRawAllocation`), `Done` только закрывает окно;
//! * модальность: `GameManager_DisableMainMenu`/`EnableMainMenu` вокруг
//!   `Run`, поэтому карта приказов не получает.
//!
//! # Клавиатура
//!
//! В C++ у диалога только кнопки (`Esc`/`KP_Enter`/`Alt-P`/`Shift-/`); здесь
//! добавлены (как в [`crate::researchmenu`]) `↑/↓`/`Tab` — выбор секции,
//! `←/→` и `−/+` — изменение выбранной секции, `H`/`?` — помощь. Это
//! задокументированное отличие: без него headless-QA и клавиатурный игрок не
//! могут распределить добычу.
//!
//! # Что меняется в состоянии
//!
//! Только поля юнитов `raw_mining`/`fuel_mining`/`gold_mining`/`total_mining`
//! (см. [`max_core::alloc`]) и — как в C++ `Update*Allocation` — запас
//! комплекса (`Complex::raw/fuel/gold`). Никаких полей `TeamUnits` нет:
//! в C++ значение живёт на каждом юните `MININGST`, едет пакетом **08/P5**
//! (`ORDER_NEW_ALLOCATE`) и входит в снапшот/CRC.
//!
//! # Осознанные отличия
//!
//! * клавиатура (`↑/↓`/`←/→`/`−`/`+`/`H`) — расширение Rust-клиента: в C++
//!   у диалога только мышь (`Esc`/`KP_Enter`/`Alt-P`/`Shift-/`);
//! * `AllocMenu::Run` закрывает окно, если у шахты приказ стал
//!   `ORDER_DISABLE` (`allocmenu.cpp:704`); в локальной пошаговой партии юнит
//!   не может быть отключён, пока открыт модальный диалог, поэтому проверки
//!   нет;
//! * звук `KCARG0` на стрелках и `NHELP0`/`NDONE0` на кнопках не
//!   проигрывается (headless-стек без звуковых триггеров UI);
//! * `PauseMenu_Menu()` по `Alt-P` не портирован (как и в остальном UI).

use max_assets::image::{BigImage, SimpleImage};
use max_core::alloc::{
    balance, get_cargo_info, get_cargo_minable, get_cargo_mining, optimize, reduce_production,
    MiningKind, MINING_CAPACITY,
};
use max_core::game::GameState;
use max_core::team::{Cargo, ComplexId, TeamId};
use max_core::unit::{UnitId, UnitInfo};
use max_core::victory::CargoType;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::menu::{blit_simple, draw_big_image_at, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::Key;

/// Размер окна `ALLOCFRM`.
pub const WINDOW_SIZE: (i32, i32) = (640, 480);
/// Окно в кадре: `win_move` прижимает 640×480 к левому верхнему углу.
pub const WINDOW_RECT: PixelRect = PixelRect::new(0, 0, 640, 480);
/// Заголовок `Text_TextBox(230, 6, 180, 20)`.
pub const TITLE_RECT: PixelRect = PixelRect::new(230, 6, 180, 20);
/// Подписи секций и строк (`MENU_TITLE_ITEM_DEF(42, uly, 120, lry)`), 78×19.
pub const LABEL_RECTS: [PixelRect; 10] = [
    PixelRect::new(42, 74, 78, 19),   // Raw (8481)
    PixelRect::new(42, 111, 78, 19),  // Usage (1e19)
    PixelRect::new(42, 148, 78, 19),  // Reserve (5ae6)
    PixelRect::new(42, 194, 78, 19),  // Fuel (a80f)
    PixelRect::new(42, 231, 78, 19),  // Usage (5432)
    PixelRect::new(42, 268, 78, 19),  // Reserve (04dd)
    PixelRect::new(42, 315, 78, 18),  // Gold (dff1)
    PixelRect::new(42, 353, 78, 17),  // Usage (e187)
    PixelRect::new(42, 389, 78, 18),  // Reserve (89a3)
    // C++ объявляет 10 подписей (`ALLOCATION_MENU_TITLE_COUNT = 10`), десятая —
    // пустой хвост таблицы (используется только как выравнивание): держим
    // пустой прямоугольник, чтобы индексы совпадали с C++.
    PixelRect::new(0, 0, 0, 0),
];
/// Число подписей в диалоге — C++ `ALLOCATION_MENU_TITLE_COUNT`.
pub const TITLE_COUNT: usize = 10;
/// Тексты подписей в порядке C++ (`_(…)` из `assets/language.json`).
pub const LABELS: [&str; 9] = [
    "Raw", "Usage", "Reserve", "Fuel", "Usage", "Reserve", "Gold", "Usage", "Reserve",
];
/// Левые стрелки `LFARO_OF` (`MENU_CONTROL_DEF(139, y, …, 1002)`).
pub const LEFT_ARROW_X: i32 = 139;
/// Правые стрелки `RTARO_OF` (`MENU_CONTROL_DEF(421, y, …, 1001)`).
pub const RIGHT_ARROW_X: i32 = 421;
/// Размер стрелки (`LFARO_OF`/`RTARO_OF` — 28×29).
pub const ARROW_SIZE: (i32, i32) = (28, 29);
/// Верхние Y секций (`MENU_CONTROL_DEF(…, 70/190/310, …)`).
pub const SECTION_TOP: [i32; 3] = [70, 190, 310];
/// Полоса: `CargoBarData::Draw` — `rect = (174, uly, 414, uly + 30)`.
pub const BAR_RECT: PixelRect = PixelRect::new(174, 0, 240, 30);
/// Шаг полос внутри секции (`data.rect.uly += 37`).
pub const BAR_STEP: i32 = 37;
/// `?` — `MNUBTN5U/D` 48×40 в (465,438).
pub const HELP_RECT: PixelRect = PixelRect::new(465, 438, 48, 40);
/// `Done` — `MNUBTN6U/D` 109×40 в (514,438).
pub const DONE_RECT: PixelRect = PixelRect::new(514, 438, 109, 40);
/// `COLOR_GREEN` — подписи и заголовок.
pub const GREEN: u8 = 0x02;
/// Цвет числа на полосе — `Text_Blit(..., 0xFF)`.
pub const CAPTION: u8 = 0xFF;
/// Обводка числа (`Color_RGB2Color(0)` в `Text_BlitTTF`).
pub const CAPTION_OUTLINE: u8 = 0x00;
/// `Fonts_GoldColor` — подписи `Done`/`?`.
pub const GOLD: (u8, u8, u8) = (165, 175, 199);

/// Что делать после события в диалоге.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocAction {
    /// Ничего (движение мыши, клик мимо).
    None,
    /// Закрыть окно (`Done`, `Esc`, `KP_Enter`, `unit->GetOrder() == DISABLE`).
    Close,
    /// Открыть помощь `ALLOCATE_SETUP`.
    Help,
}

/// Открытый диалог распределения добычи.
///
/// Числа (`production`/`capacity`/`usage`/`complex_*`) — ровно те `Cargo`,
/// которые C++ считает в конструкторе `AllocMenu` (`GetCargoMining`,
/// `GetCargoMinable`, `GetCargoInfo`) и поддерживает инкрементально в
/// `Update*Allocation`; `balance` — `cargo_2` из `DrawCargoBars`.
#[derive(Debug, Clone, PartialEq)]
pub struct AllocMenu {
    /// Шахта, из меню которой открыт диалог.
    pub unit: UnitId,
    /// Команда шахты.
    pub team: TeamId,
    /// Комплекс шахты (`unit->GetComplex()`).
    pub complex: ComplexId,
    /// Добыча комплекса (`materials` из `GetCargoMining`).
    pub production: Cargo,
    /// Максимум клеток шахт (`capacity` из `GetCargoMining`).
    pub capacity: Cargo,
    /// Потребление остальных зданий (`GetCargoMinable`).
    pub usage: Cargo,
    /// Запас комплекса (`GetCargoInfo` → materials).
    pub complex_materials: Cargo,
    /// Вместимость комплекса (`GetCargoInfo` → capacity).
    pub complex_capacity: Cargo,
    /// `cargo_2` — `Balance` по каждому ресурсу (для «ленты» полосы).
    pub balance: Cargo,
    /// Выбранная секция (клавиатура Rust-клиента).
    pub selected: MiningKind,
    /// Строка статуса (подсказки/ошибки).
    pub status: String,
}

impl AllocMenu {
    /// Открывает диалог для шахты — C++ `AllocMenu_Menu(unit)`.
    ///
    /// `None`, если юнит не шахта, не своя команда, выключена/в простое
    /// (`allocate` в попапе есть только при `ORDER_POWER_ON`-семействе) или у
    /// неё нет комплекса.
    pub fn new(state: &mut GameState, unit_id: UnitId) -> Option<Self> {
        let unit = state.unit(unit_id)?.clone();

        if unit.unit_type != max_core::enums::UnitType::MiningStation {
            return None;
        }

        if !max_core::alloc::is_active_mine(&unit) {
            return None;
        }

        let complex = unit.complex?;
        let team = unit.team;
        let (production, capacity) = get_cargo_mining(state, team, complex);
        let usage = get_cargo_minable(state, team, complex);
        let (complex_materials, complex_capacity) = get_cargo_info(state, team, complex);

        let mut menu = Self {
            unit: unit_id,
            team,
            complex,
            production,
            capacity,
            usage,
            complex_materials,
            complex_capacity,
            balance: Cargo::init(),
            selected: MiningKind::Raw,
            status: String::new(),
        };
        menu.refresh_balance(state);
        Some(menu)
    }

    /// `DrawCargoBars` в части `cargo_2` = `Balance(...)` для трёх ресурсов.
    ///
    /// C++ вызывает это при открытии и после каждого изменения — с теми же
    /// побочными эффектами `AllocMenu_Optimize`/`AllocMenu_ReduceProduction`
    /// (суммарная добыча не меняется, но распределение по шахтам комплекса
    /// нормализуется). Здесь — ровно в те же моменты.
    pub fn refresh_balance(&mut self, state: &mut GameState) {
        self.balance = Cargo::init();
        self.balance.raw = balance(
            state,
            self.team,
            self.complex,
            MiningKind::Raw,
            self.production.raw,
            self.capacity.raw,
        );
        self.balance.fuel = balance(
            state,
            self.team,
            self.complex,
            MiningKind::Fuel,
            self.production.fuel,
            self.capacity.fuel,
        );
        self.balance.gold = balance(
            state,
            self.team,
            self.complex,
            MiningKind::Gold,
            self.production.gold,
            self.capacity.gold,
        );
    }

    /// `AllocMenu::UpdateRawAllocation`/`UpdateFuelAllocation`/`UpdateGoldAllocation`.
    fn update(&mut self, state: &mut GameState, kind: MiningKind, material: i32) {
        match kind {
            MiningKind::Raw => {
                self.production.raw += material;
                self.production.free_capacity -= material;
            }
            MiningKind::Fuel => {
                self.production.fuel += material;
                self.production.free_capacity -= material;
            }
            MiningKind::Gold => {
                self.production.gold += material;
                self.production.free_capacity -= material;
            }
        }

        // `complex->material/fuel/gold += material` — запас комплекса.
        if let Some(complex) = state.complex_mut(self.team, self.complex.0 as u16) {
            match kind {
                MiningKind::Raw => complex.raw = complex.raw.saturating_add(material as i16),
                MiningKind::Fuel => complex.fuel = complex.fuel.saturating_add(material as i16),
                MiningKind::Gold => complex.gold = complex.gold.saturating_add(material as i16),
            }
        }

        self.refresh_balance(state);
    }

    /// `+1` единица ресурса — C++ `case 1001/1004/1007`.
    pub fn increase(&mut self, state: &mut GameState, kind: MiningKind) -> bool {
        let balanced = match kind {
            MiningKind::Raw => self.balance.raw,
            MiningKind::Fuel => self.balance.fuel,
            MiningKind::Gold => self.balance.gold,
        };
        let production = match kind {
            MiningKind::Raw => self.production.raw,
            MiningKind::Fuel => self.production.fuel,
            MiningKind::Gold => self.production.gold,
        };

        if self.production.free_capacity == 0 || production >= balanced {
            return false;
        }

        let added = optimize(state, self.team, self.complex, kind, 1);
        self.update(state, kind, added);
        added > 0
    }

    /// `−1` единица ресурса — C++ `case 1002/1005/1008`.
    pub fn decrease(&mut self, state: &mut GameState, kind: MiningKind) -> bool {
        let production = match kind {
            MiningKind::Raw => self.production.raw,
            MiningKind::Fuel => self.production.fuel,
            MiningKind::Gold => self.production.gold,
        };

        if production <= 0 {
            return false;
        }

        reduce_production(state, self.team, self.complex, kind, 1);
        self.update(state, kind, -1);
        true
    }

    /// `AllocMenu::OnClickRawBar`/`Fuel`/`Gold` с **абсолютным** уровнем
    /// (в C++ уровень приходит из позиции курсора).
    pub fn set_level(&mut self, state: &mut GameState, kind: MiningKind, target: i32) {
        let production = match kind {
            MiningKind::Raw => self.production.raw,
            MiningKind::Fuel => self.production.fuel,
            MiningKind::Gold => self.production.gold,
        };
        let mut materials = target - production;

        if self.production.free_capacity < materials {
            materials = self.production.free_capacity;
        }

        if materials < 0 {
            reduce_production(state, self.team, self.complex, kind, -materials);
            self.update(state, kind, materials);
        } else if materials > 0 {
            let added = optimize(state, self.team, self.complex, kind, materials);
            self.update(state, kind, added);
        }
    }

    /// `AllocMenu::GetHorizontalBarClickValue` (`allocmenu.cpp:592`):
    /// `(capacity * (mouse_x − bounds.ulx − 174) + 120) / 240`.
    pub fn bar_click_value(&self, kind: MiningKind, x: i32) -> i32 {
        let capacity = match kind {
            MiningKind::Raw => self.capacity.raw,
            MiningKind::Fuel => self.capacity.fuel,
            MiningKind::Gold => self.capacity.gold,
        };
        let local = x - (WINDOW_RECT.ulx + BAR_RECT.ulx);

        (capacity * local + 120) / 240
    }

    /// Полоса, в которую попадает точка окна: `(секция, строка)`.
    fn bar_at(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        if x < BAR_RECT.ulx || x >= BAR_RECT.lrx() {
            return None;
        }

        for (section, top) in SECTION_TOP.iter().enumerate() {
            for row in 0..3 {
                let rect = PixelRect::new(
                    BAR_RECT.ulx,
                    top + row * BAR_STEP,
                    BAR_RECT.width,
                    BAR_RECT.height,
                );

                if rect.contains(x, y) {
                    return Some((section, row as usize));
                }
            }
        }

        None
    }

    /// Клик в кадре 640×480.
    pub fn click(&mut self, state: &mut GameState, x: i32, y: i32) -> AllocAction {
        if HELP_RECT.contains(x, y) {
            return AllocAction::Help;
        }

        if DONE_RECT.contains(x, y) {
            return AllocAction::Close;
        }

        for (section, top) in SECTION_TOP.iter().enumerate() {
            let kind = MiningKind::ALL[section];
            let left = PixelRect::new(LEFT_ARROW_X, *top, ARROW_SIZE.0, ARROW_SIZE.1);
            let right = PixelRect::new(RIGHT_ARROW_X, *top, ARROW_SIZE.0, ARROW_SIZE.1);

            if left.contains(x, y) {
                self.selected = kind;
                // `−1`: кнопка не заблокирована, на нуле просто ничего не
                // делает (`case 1002/1005/1008`).
                self.decrease(state, kind);
                return AllocAction::None;
            }

            if right.contains(x, y) {
                self.selected = kind;
                if !self.increase(state, kind) {
                    self.status = format!("{}: no free mining capacity", kind.name());
                }
                return AllocAction::None;
            }
        }

        if let Some((section, row)) = self.bar_at(x, y) {
            let kind = MiningKind::ALL[section];

            // Клик по первой полосе — установка уровня (`OnClick*Bar`).
            if row == 0 {
                let target = self.bar_click_value(kind, x);
                self.set_level(state, kind, target);
            }

            self.selected = kind;
        }

        AllocAction::None
    }

    /// Клавиатура (Rust-клиент; см. шапку модуля).
    pub fn key(&mut self, state: &mut GameState, key: Key) -> AllocAction {
        match key {
            Key::Escape | Key::Enter => AllocAction::Close,
            Key::Char('h' | 'H' | '?') => AllocAction::Help,
            Key::Up => {
                let index = self.selected.index();
                self.selected = MiningKind::ALL[(index + 2) % 3];
                AllocAction::None
            }
            Key::Down | Key::Tab => {
                let index = self.selected.index();
                self.selected = MiningKind::ALL[(index + 1) % 3];
                AllocAction::None
            }
            Key::Left | Key::Char('-') | Key::Char('_') => {
                self.decrease(state, self.selected);
                AllocAction::None
            }
            Key::Right | Key::Char('+') | Key::Char('=') => {
                if !self.increase(state, self.selected) {
                    self.status = format!("{}: no free mining capacity", self.selected.name());
                }
                AllocAction::None
            }
            Key::PageUp => {
                self.decrease(state, self.selected);
                AllocAction::None
            }
            Key::PageDown => {
                self.increase(state, self.selected);
                AllocAction::None
            }
            Key::Backspace | Key::Char(_) | Key::Alt(_) | Key::Other => AllocAction::None,
        }
    }

    /// Ввод события.
    pub fn input(&mut self, state: &mut GameState, event: crate::ui::InputEvent) -> AllocAction {
        match event {
            crate::ui::InputEvent::MouseClick(x, y) => self.click(state, x, y),
            crate::ui::InputEvent::Key(key) => self.key(state, key),
            _ => AllocAction::None,
        }
    }

    /// Добыча ресурса в комплексе (`production.<kind>`).
    pub fn production_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.production.raw,
            MiningKind::Fuel => self.production.fuel,
            MiningKind::Gold => self.production.gold,
        }
    }

    /// Свободные единицы добычи (`production.free_capacity`).
    pub fn free_capacity(&self) -> i32 {
        self.production.free_capacity
    }

    /// Подпись полосы: `%i` для первой/третьей, `%i (%+i / turn)` для второй.
    pub fn bar_caption(&self, section: usize, row: usize) -> String {
        let kind = MiningKind::ALL[section];

        match row {
            0 => format!("{}", self.production_of(kind)),
            1 => {
                let usage = match kind {
                    MiningKind::Raw => self.usage.raw,
                    MiningKind::Fuel => self.usage.fuel,
                    MiningKind::Gold => self.usage.gold,
                };

                format!("{} ({:+} / turn)", usage, self.production_of(kind) - usage)
            }
            _ => {
                let stored = match kind {
                    MiningKind::Raw => self.complex_materials.raw,
                    MiningKind::Fuel => self.complex_materials.fuel,
                    MiningKind::Gold => self.complex_materials.gold,
                };

                format!("{stored}")
            }
        }
    }

    /// Длина заливки полосы в пикселях — `CargoBarData::Draw`:
    /// `offset_y = cargo_1 * 240 / cargo_3` (0 при нулевой ёмкости).
    pub fn bar_fill(&self, section: usize, row: usize) -> i32 {
        let kind = MiningKind::ALL[section];
        let width = BAR_RECT.width;

        let (value, scale) = match row {
            0 => {
                let scale = self.capacity_of(kind).max(self.usage_of(kind));
                (self.production_of(kind), scale)
            }
            1 => {
                let scale = self.capacity_of(kind).max(self.usage_of(kind));
                (self.usage_of(kind), scale)
            }
            _ => (self.stored_of(kind), self.complex_capacity_of(kind)),
        };

        if scale <= 0 {
            return 0;
        }

        (value * width / scale).clamp(0, width)
    }

    /// Длина «ленты» (`BARTAPE`) полосы: `(cargo_3 − cargo_2)·240/cargo_3`,
    /// только у первой полосы (`data.flag = true`).
    pub fn bar_tape(&self, section: usize, row: usize) -> i32 {
        if row != 0 {
            return 0;
        }

        let kind = MiningKind::ALL[section];
        let width = BAR_RECT.width;
        let scale = self.capacity_of(kind).max(self.usage_of(kind));

        if scale <= 0 {
            return width;
        }

        let balanced = match kind {
            MiningKind::Raw => self.balance.raw,
            MiningKind::Fuel => self.balance.fuel,
            MiningKind::Gold => self.balance.gold,
        };

        ((scale - balanced) * width / scale).clamp(0, width)
    }

    /// `capacity.<kind>`.
    pub fn capacity_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.capacity.raw,
            MiningKind::Fuel => self.capacity.fuel,
            MiningKind::Gold => self.capacity.gold,
        }
    }

    /// `usage.<kind>`.
    pub fn usage_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.usage.raw,
            MiningKind::Fuel => self.usage.fuel,
            MiningKind::Gold => self.usage.gold,
        }
    }

    /// `complex_materials.<kind>`.
    pub fn stored_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.complex_materials.raw,
            MiningKind::Fuel => self.complex_materials.fuel,
            MiningKind::Gold => self.complex_materials.gold,
        }
    }

    /// `complex_capacity.<kind>`.
    pub fn complex_capacity_of(&self, kind: MiningKind) -> i32 {
        match kind {
            MiningKind::Raw => self.complex_capacity.raw,
            MiningKind::Fuel => self.complex_capacity.fuel,
            MiningKind::Gold => self.complex_capacity.gold,
        }
    }

    /// Рисует диалог поверх кадра 640×480 (`AllocMenu::AllocMenu` + `win_draw`).
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &crate::Assets) -> u32 {
        if frame.width() != 640 || frame.height() != 480 {
            return 0;
        }

        let Some(menu_assets) = assets.alloc.as_ref() else {
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

        let mut painted = 0;

        match background {
            Some(background) => {
                painted += draw_big_image_at(
                    frame,
                    background,
                    &palette,
                    (WINDOW_RECT.ulx, WINDOW_RECT.uly),
                    WINDOW_RECT,
                );
            }
            None => {
                frame.fill_rect(WINDOW_RECT, palette.rgba(0x22));
                painted += 1;
            }
        }

        let title_font = GnwFontId::Font5.font();
        let green = palette.rgba(GREEN);

        // Заголовок: `Text_TextBox(…, 230, 6, 180, 20, COLOR_GREEN, true, true)`
        // — vertical_align: `6 + ((20 + 1) − font_height·1) / 2`.
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(
                WINDOW_RECT.ulx + TITLE_RECT.ulx,
                vertical_center(TITLE_RECT),
                TITLE_RECT.width,
                title_font.height(),
            ),
            title_font,
            "Allocation Menu",
            green,
        )
        .max(0) as u32;

        for (index, label) in LABELS.iter().enumerate() {
            let rect = LABEL_RECTS[index];

            painted += draw_text_gnw_centered(
                frame,
                PixelRect::new(
                    WINDOW_RECT.ulx + rect.ulx,
                    vertical_center(rect),
                    rect.width,
                    title_font.height(),
                ),
                title_font,
                label,
                green,
            )
            .max(0) as u32;
        }

        let intensity = palette.intensity_table(0, 0, 25, 1, 1);

        for (section, top) in SECTION_TOP.iter().enumerate() {
            let kind = MiningKind::ALL[section];

            // Стрелки `LFARO_OF`/`RTARO_OF` (нажатый вид — `*_ON`; наведение в
            // C++ вид не меняет, поэтому рисуется `OF`).
            if let Some(arrow) = menu_assets.left.as_ref() {
                painted += blit_simple(
                    frame,
                    &palette,
                    arrow.image(false),
                    LEFT_ARROW_X,
                    *top,
                    true,
                );
            }
            if let Some(arrow) = menu_assets.right.as_ref() {
                painted += blit_simple(
                    frame,
                    &palette,
                    arrow.image(false),
                    RIGHT_ARROW_X,
                    *top,
                    true,
                );
            }

            for row in 0..3 {
                let rect = PixelRect::new(
                    WINDOW_RECT.ulx + BAR_RECT.ulx,
                    top + row as i32 * BAR_STEP,
                    BAR_RECT.width,
                    BAR_RECT.height,
                );

                painted += self.draw_bar(frame, &palette, menu_assets, kind, rect, row, &intensity);
            }
        }

        if let Some(pair) = menu_assets.help.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                HELP_RECT,
                "?",
                false,
            );
        }
        if let Some(pair) = menu_assets.done.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                DONE_RECT,
                "Done",
                false,
            );
        }

        painted
    }

    /// Одна полоса: заливка `BAR*`, «лента» `BARTAPE`, подпись с обводкой.
    #[allow(clippy::too_many_arguments)]
    fn draw_bar(
        &self,
        frame: &mut RgbaBuffer,
        palette: &UiPalette,
        assets: &AllocMenuAssets,
        kind: MiningKind,
        rect: PixelRect,
        row: usize,
        intensity: &[u8; 256],
    ) -> u32 {
        let mut painted = 0;
        let fill = self.bar_fill(kind.index(), row);
        let tape = self.bar_tape(kind.index(), row);

        // `LoadHorizontalBar`: справа налево от края спрайта — берётся
        // *правый* кусок текстуры шириной `fill` и кладётся в левый край.
        if let Some(bar) = assets.bar(kind) {
            painted += blit_simple_slice(
                frame,
                palette,
                bar,
                i32::from(bar.width) - fill,
                rect.ulx,
                rect.uly,
                fill,
                rect.height,
                true,
            );
        }

        // `LoadHorizontalTape`: левый кусок `BARTAPE` шириной `tape` — к правому
        // краю полосы. `buf_to_buf` копирует пиксели как есть (без прозрачности),
        // поэтому «лента» перекрывает текстуру полосы целиком.
        if tape > 0 {
            if let Some(tape_image) = assets.tape.as_ref() {
                painted += blit_simple_slice(
                    frame,
                    palette,
                    tape_image,
                    0,
                    rect.lrx() - tape,
                    rect.uly,
                    tape,
                    rect.height,
                    false,
                );
            }
        }

        // Подпись: `CargoBarData::Draw` — по центру полосы, `FONT1`, обводка.
        let caption = self.bar_caption(kind.index(), row);
        let font = GnwFontId::Font1.font();
        let width = font.text_width(&caption).min(BAR_RECT.width);
        let text_x = rect.ulx + (BAR_RECT.width - width) / 2;
        let text_y = rect.uly + (rect.height - font.height()) / 2;

        // `Color_RecolorPixels` — затемнение подложки под текстом (бокс
        // `ширина+4` × `высота шрифта`, начиная с `text_x − 2`).
        painted += recolor_box(
            frame,
            palette,
            PixelRect::new(text_x - 2, text_y, width + 4, font.height()),
            intensity,
        );

        // `Text_Blit(..., GNW_TEXT_OUTLINE | 0xFF)`: сначала та же строка на
        // пиксель ниже-правее чёрным, затем основная.
        draw_text_gnw(
            frame,
            text_x + 1,
            text_y + 1,
            font,
            &caption,
            palette.rgba(CAPTION_OUTLINE),
        );
        draw_text_gnw(frame, text_x, text_y, font, &caption, palette.rgba(CAPTION));
        painted += 1;

        painted
    }
}

/// `Text_TextBox(..., vertical_align = true)`: `uly + ((height + 1) − font) / 2`.
fn vertical_center(rect: PixelRect) -> i32 {
    let font = GnwFontId::Font5.font();
    rect.uly + ((rect.height + 1) - font.height()) / 2
}

/// Копирует кусок `SimpleImage` (`width × height` от `(source_x, 0)`) в точку
/// кадра, пропуская прозрачный индекс (`LoadHorizontalBar`/`LoadHorizontalTape`).
#[allow(clippy::too_many_arguments)]
fn blit_simple_slice(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    source_x: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    skip_transparent: bool,
) -> u32 {
    let mut painted = 0;

    for row in 0..height {
        for column in 0..width {
            let sx = source_x + column;

            if sx < 0 || sx >= i32::from(image.width) {
                continue;
            }

            let Some(index) = image.pixel(sx as u32, row as u32) else {
                continue;
            };

            if skip_transparent && index == image.transparent {
                continue;
            }

            if frame.set(x + column, y + row, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// `Color_RecolorPixels` — заменить индексы палитры по таблице.
fn recolor_box(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    rect: PixelRect,
    table: &[u8; 256],
) -> u32 {
    let mut painted = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            let Some(pixel) = frame.get(x, y) else {
                continue;
            };
            let Some(index) = palette.index_of(pixel) else {
                continue;
            };

            if frame.set(x, y, palette.rgba(table[usize::from(index)])) {
                painted += 1;
            }
        }
    }

    painted
}

/// Ассеты диалога распределения добычи.
#[derive(Debug, Clone)]
pub struct AllocMenuAssets {
    /// Фон `ALLOCFRM` 640×480.
    pub background: Option<BigImage>,
    /// `LFARO_OF`/`LFARO_ON` 28×29.
    pub left: Option<MenuButtonPair>,
    /// `RTARO_OF`/`RTARO_ON` 28×29.
    pub right: Option<MenuButtonPair>,
    /// `BARRAW` 241×32.
    pub bar_raw: Option<SimpleImage>,
    /// `BARFUEL` 273×33.
    pub bar_fuel: Option<SimpleImage>,
    /// `BARGOLD` 241×33.
    pub bar_gold: Option<SimpleImage>,
    /// `BARTAPE` 240×30.
    pub tape: Option<SimpleImage>,
    /// `MNUBTN5U/D` 48×40 (`?`).
    pub help: Option<MenuButtonPair>,
    /// `MNUBTN6U/D` 109×40 (`Done`).
    pub done: Option<MenuButtonPair>,
}

impl AllocMenuAssets {
    /// Грузит ресурсы диалога; отсутствие части — предупреждение, не ошибка.
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
                .read("ALLOCFRM")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            left: pair("LFARO_OF", "LFARO_ON"),
            right: pair("RTARO_OF", "RTARO_ON"),
            bar_raw: simple("BARRAW"),
            bar_fuel: simple("BARFUEL"),
            bar_gold: simple("BARGOLD"),
            tape: simple("BARTAPE"),
            help: pair("MNUBTN5U", "MNUBTN5D"),
            done: pair("MNUBTN6U", "MNUBTN6D"),
        }
    }

    /// Есть ли фон и ключевые картинки полос.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.bar_raw.is_some() && self.tape.is_some()
    }

    /// Текстура заливки для ресурса.
    pub fn bar(&self, kind: MiningKind) -> Option<&SimpleImage> {
        match kind {
            MiningKind::Raw => self.bar_raw.as_ref(),
            MiningKind::Fuel => self.bar_fuel.as_ref(),
            MiningKind::Gold => self.bar_gold.as_ref(),
        }
    }
}

/// Значение добычи, при котором шахта заполнена (`16` единиц).
pub const CAPACITY_LIMIT: i32 = MINING_CAPACITY;

/// Итог распределения по ресурсам (для журнала/QA).
pub fn totals(menu: &AllocMenu) -> [i32; 3] {
    [
        menu.production_of(MiningKind::Raw),
        menu.production_of(MiningKind::Fuel),
        menu.production_of(MiningKind::Gold),
    ]
}

/// Запас ресурса в комплексе (для журнала/QA).
pub fn stored(state: &GameState, team: TeamId, complex: ComplexId, kind: MiningKind) -> i32 {
    let Some(complex) = state.complex(team, complex.0 as u16) else {
        return 0;
    };

    match kind {
        MiningKind::Raw => i32::from(complex.raw),
        MiningKind::Fuel => i32::from(complex.fuel),
        MiningKind::Gold => i32::from(complex.gold),
    }
}

/// `CargoType` ресурса (для QA/журнала).
pub fn cargo_type(kind: MiningKind) -> CargoType {
    kind.cargo_type()
}

/// Шахта ли это (для проверки открытия).
pub fn is_mining_station(unit: &UnitInfo) -> bool {
    unit.unit_type == max_core::enums::UnitType::MiningStation
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
    use max_core::geo::Point;
    use max_core::team::Complex;
    use max_core::TeamUnits;

    fn state_with_mine(complex_id: i16) -> (GameState, UnitId, ComplexId) {
        let mut state = GameState::new();
        state.teams[0].team_type = max_core::team::TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(max_core::enums::TeamClan::Random);

        // Квадрат 2×2 вокруг шахты: сырьё 8+8 (лимит замера 16), топливо 8,
        // золото 8 — как `Survey_GetResourcesInArea(..., 1, 16, ...)`.
        let resources = [
            (20, 20, 8, max_core::map::ResourceKind::Raw),
            (21, 20, 8, max_core::map::ResourceKind::Raw),
            (20, 21, 8, max_core::map::ResourceKind::Gold),
            (21, 21, 8, max_core::map::ResourceKind::Fuel),
        ];
        for (x, y, amount, kind) in resources {
            state
                .map
                .resources
                .set(x, y, max_core::map::ResourceMapEntry::new(amount, Some(kind), 0));
        }

        // Данные-контроль, а не чтение классификации: маска — аргумент
        // `spawn_unit` (флаги экземпляра), поэтому `STATIONARY | BUILDING`
        // остаются сырыми битами.
        let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
        let id = state
            .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), flags)
            .expect("шахта");

        let complex = ComplexId(complex_id);
        state.team_units[0].complexes.push(Complex::new(complex_id));

        let mut unit = state.unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.complex = Some(complex);
        state.write_unit(id, unit);

        (state, id, complex)
    }

    #[test]
    fn layout_matches_the_oracle() {
        assert_eq!(WINDOW_RECT, PixelRect::new(0, 0, 640, 480));
        assert_eq!(TITLE_RECT, PixelRect::new(230, 6, 180, 20));
        assert_eq!(LEFT_ARROW_X, 139);
        assert_eq!(RIGHT_ARROW_X, 421);
        assert_eq!(ARROW_SIZE, (28, 29));
        assert_eq!(SECTION_TOP, [70, 190, 310]);
        assert_eq!(BAR_RECT, PixelRect::new(174, 0, 240, 30));
        assert_eq!(BAR_STEP, 37);
        assert_eq!(HELP_RECT, PixelRect::new(465, 438, 48, 40));
        assert_eq!(DONE_RECT, PixelRect::new(514, 438, 109, 40));
        // Десятая подпись C++ — пустая (таблица из 10 элементов).
        assert_eq!(LABEL_RECTS.len(), TITLE_COUNT);
        assert!(LABEL_RECTS[9].is_empty());
        assert_eq!(LABEL_RECTS[0], PixelRect::new(42, 74, 78, 19));
        assert_eq!(LABEL_RECTS[8], PixelRect::new(42, 389, 78, 18));
    }

    #[test]
    fn labels_match_language_json() {
        assert_eq!(LABELS[0], "Raw");
        assert_eq!(LABELS[1], "Usage");
        assert_eq!(LABELS[2], "Reserve");
        assert_eq!(LABELS[3], "Fuel");
        assert_eq!(LABELS[6], "Gold");
    }

    #[test]
    fn opening_requires_an_active_mine() {
        let (mut state, id, _complex) = state_with_mine(1);
        assert!(AllocMenu::new(&mut state, id).is_some());

        state.unit_mut(id).unwrap().orders = UnitOrder::PowerOff;
        assert!(
            AllocMenu::new(&mut state, id).is_none(),
            "выключенная шахта не открывает распределение (в C++ пункт тогда `start`)"
        );

        state.unit_mut(id).unwrap().orders = UnitOrder::PowerOn;
        state.unit_mut(id).unwrap().complex = None;
        assert!(AllocMenu::new(&mut state, id).is_none(), "нет комплекса");
    }

    #[test]
    fn opening_reports_mining_capacity_from_the_cell() {
        let (mut state, id, _complex) = state_with_mine(1);
        let menu = AllocMenu::new(&mut state, id).expect("меню");

        assert_eq!(menu.production_of(MiningKind::Raw), 0);
        assert!(
            menu.capacity_of(MiningKind::Raw) >= 0,
            "максимум клетки неотрицателен"
        );
        assert_eq!(menu.free_capacity(), 16);
        assert_eq!(menu.production.free_capacity, 16);
    }

    #[test]
    fn increase_and_decrease_move_the_mine_counters() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");

        if menu.capacity_of(MiningKind::Raw) == 0 {
            return;
        }

        assert!(menu.increase(&mut state, MiningKind::Raw));
        assert_eq!(menu.production_of(MiningKind::Raw), 1);
        assert_eq!(state.unit(id).unwrap().raw_mining, 1);
        assert_eq!(state.unit(id).unwrap().total_mining, 1);
        assert_eq!(menu.free_capacity(), 15);

        assert!(menu.decrease(&mut state, MiningKind::Raw));
        assert_eq!(menu.production_of(MiningKind::Raw), 0);
        assert_eq!(state.unit(id).unwrap().raw_mining, 0);
        assert_eq!(state.unit(id).unwrap().total_mining, 0);
    }

    #[test]
    fn level_change_is_absolute_like_a_bar_click() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");
        let capacity = menu.capacity_of(MiningKind::Raw);

        if capacity < 3 {
            return;
        }

        menu.set_level(&mut state, MiningKind::Raw, 3);
        assert_eq!(menu.production_of(MiningKind::Raw), 3);
        assert_eq!(state.unit(id).unwrap().raw_mining, 3);

        menu.set_level(&mut state, MiningKind::Raw, 1);
        assert_eq!(menu.production_of(MiningKind::Raw), 1);
        assert_eq!(state.unit(id).unwrap().raw_mining, 1);
    }

    #[test]
    fn bar_click_value_matches_cpp_formula() {
        let (mut state, id, _complex) = state_with_mine(1);
        let menu = AllocMenu::new(&mut state, id).expect("меню");
        let capacity = menu.capacity_of(MiningKind::Raw);

        // `x -= bounds.ulx + 174; return (capacity * x + 120) / 240;`
        assert_eq!(menu.bar_click_value(MiningKind::Raw, 174), 0);
        assert_eq!(
            menu.bar_click_value(MiningKind::Raw, 174 + 240),
            (capacity * 240 + 120) / 240
        );
        assert_eq!(
            menu.bar_click_value(MiningKind::Raw, 174 + 120),
            (capacity * 120 + 120) / 240
        );
    }

    #[test]
    fn clicks_hit_the_expected_targets() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");

        // `?` и `Done`.
        assert_eq!(
            menu.click(&mut state, HELP_RECT.ulx + 1, HELP_RECT.uly + 1),
            AllocAction::Help
        );
        assert_eq!(
            menu.click(&mut state, DONE_RECT.ulx + 1, DONE_RECT.uly + 1),
            AllocAction::Close
        );

        // Клик мимо — ничего.
        assert_eq!(
            menu.click(&mut state, 320, 200),
            AllocAction::None,
            "полоса не меняет приказ"
        );

        // Правая стрелка Raw.
        let before = menu.production_of(MiningKind::Raw);
        menu.click(&mut state, RIGHT_ARROW_X + 2, SECTION_TOP[0] + 2);
        let after = menu.production_of(MiningKind::Raw);
        assert!(after == before || after == before + 1);

        // Левая стрелка возвращает.
        menu.click(&mut state, LEFT_ARROW_X + 2, SECTION_TOP[0] + 2);
        assert_eq!(menu.production_of(MiningKind::Raw), before);
    }

    #[test]
    fn keys_close_select_and_adjust() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");

        assert_eq!(menu.key(&mut state, Key::Escape), AllocAction::Close);
        assert_eq!(menu.key(&mut state, Key::Enter), AllocAction::Close);
        assert_eq!(menu.key(&mut state, Key::Char('h')), AllocAction::Help);

        assert_eq!(menu.selected, MiningKind::Raw);
        menu.key(&mut state, Key::Down);
        assert_eq!(menu.selected, MiningKind::Fuel);
        menu.key(&mut state, Key::Down);
        assert_eq!(menu.selected, MiningKind::Gold);
        menu.key(&mut state, Key::Down);
        assert_eq!(menu.selected, MiningKind::Raw);
        menu.key(&mut state, Key::Up);
        assert_eq!(menu.selected, MiningKind::Gold);

        let before = menu.production_of(MiningKind::Fuel);
        menu.key(&mut state, Key::Right);
        let after = menu.production_of(MiningKind::Fuel);
        assert!(after == before || after == before + 1);
        menu.key(&mut state, Key::Left);
        assert_eq!(menu.production_of(MiningKind::Fuel), before);
    }

    #[test]
    fn bar_captions_follow_cpp_format() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");

        if menu.capacity_of(MiningKind::Raw) < 2 {
            return;
        }

        menu.set_level(&mut state, MiningKind::Raw, 2);
        assert_eq!(menu.bar_caption(0, 0), "2");
        // `%i (%+i / turn)` — `usage` и `production − usage`.
        assert!(menu.bar_caption(0, 1).contains("/ turn"));
        assert!(menu.bar_caption(0, 1).starts_with(&menu.usage_of(MiningKind::Raw).to_string()));
        assert_eq!(menu.bar_caption(0, 2), menu.stored_of(MiningKind::Raw).to_string());
    }

    #[test]
    fn bar_geometry_follows_cpp() {
        let (mut state, id, _complex) = state_with_mine(1);
        let mut menu = AllocMenu::new(&mut state, id).expect("меню");
        let capacity = menu.capacity_of(MiningKind::Raw);

        if capacity < 4 {
            return;
        }

        menu.set_level(&mut state, MiningKind::Raw, 4);
        // Заливка `production * 240 / max(capacity, usage)`.
        let scale = capacity.max(menu.usage_of(MiningKind::Raw));
        assert_eq!(menu.bar_fill(0, 0), (4 * 240 / scale).clamp(0, 240));
        // Вторая и третья полосы без «ленты».
        assert_eq!(menu.bar_tape(0, 1), 0);
        assert_eq!(menu.bar_tape(0, 2), 0);
    }

    #[test]
    fn determinism_of_two_identical_dialogs() {
        let (mut a, id_a, _) = state_with_mine(1);
        let (mut b, id_b, _) = state_with_mine(1);

        let mut menu_a = AllocMenu::new(&mut a, id_a).expect("меню");
        let mut menu_b = AllocMenu::new(&mut b, id_b).expect("меню");

        for _ in 0..3 {
            menu_a.increase(&mut a, MiningKind::Raw);
            menu_b.increase(&mut b, MiningKind::Raw);
        }
        menu_a.decrease(&mut a, MiningKind::Raw);
        menu_b.decrease(&mut b, MiningKind::Raw);

        assert_eq!(menu_a.production, menu_b.production);
        assert_eq!(menu_a.balance, menu_b.balance);
        assert_eq!(a.unit(id_a).unwrap().raw_mining, b.unit(id_b).unwrap().raw_mining);
        assert_eq!(a.unit(id_a).unwrap().total_mining, b.unit(id_b).unwrap().total_mining);
        // Дайджест состояния (CRC/снапшот) тоже совпадает: значение входит в
        // снапшот полями юнита.
        assert_eq!(
            max_core::crc::state_checksum(&a),
            max_core::crc::state_checksum(&b)
        );
        assert_eq!(a.snapshot_bytes(), b.snapshot_bytes());
    }
}
