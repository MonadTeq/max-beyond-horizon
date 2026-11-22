//! Предвысадочный экран мобилизации — полный порт `CargoMenu`.
//!
//! # Оракул
//!
//! * `cargomenu.cpp` — `CargoMenu` (фон `CARGOPIC`): списки «Available» и
//!   «Purchased», загрузка cargo слайдером, покупка/продажа, полоса золота,
//!   переключатель `CRGTOG` (покупка ↔ модернизация), фильтры
//!   `SELGRD`/`SELAIR`/`SELSEA`/`SELBLD`/`SELCBT`, кнопки `CRGBUY`, `CRGDEL`,
//!   `BLDONE`, `BLDCAN`, `BLDHLP`, `BLDDES`, стрелки списков;
//! * `abstractupgrademenu.cpp` — колонка апгрейдов (`UpgradeControl`),
//!   `IsUnitFiltered`/`IsBetterUnit`, `DrawUnitStats` (полоса золота
//!   `VERTGOLD`), `CommitUpgradeChanges`;
//! * `purchaseTypeSelector.cpp` — цены в списке типов
//!   (`Cargo_GetRawConsumptionRate(CONSTRCT|TRAINHAL|LANDPLT, 1) * turns`);
//! * `cargoselector.cpp` — строки «Purchased» (иконка `I_*`, «Имя N (Full)»);
//! * `unittypeselector.cpp`/`scrollbar.cpp` — строки по 32 px, `max_item_count`,
//!   `LoadVerticalBar` (полосы `VERTRAW`/`VERTFUEL`/`VERTGOLD`);
//! * `reportstats.cpp` — `ReportStats_DrawListItemIcon` (иконка 32×32, масштаб
//!   1/2 у мобильных и 1/4 у стационарных), `DrawNumber`/`DrawText` (число
//!   выравнивается по правому краю колонки).
//!
//! # Раскладка (координаты `cargomenu.cpp`)
//!
//! | Область | Координаты | Источник |
//! |---|---|---|
//! | Портрет | (11,13) 280×240 | `unit_portrait_window` |
//! | Описание | (11+16,13+17) 280×230 | `Text_TextBox` поверх портрета |
//! | Иконка лота / список `Purchased` | (337,45) 118×185, строки 32 px (5) | `wininfo2` |
//! | Список `Available` | (482,80) 123×290, строки 32 px (9) | `wininfo1` |
//! | Колонка цен списка | x 610..625, по правому краю 625 | `PurchaseTypeSelector` |
//! | Статистика | (11,293) 250×174 | `stats_background` |
//! | Колонка апгрейдов | (283,293) 38×174, ряд 19 px | `button_background` |
//! | Колонка цен апгрейдов | (321,293) 40×174, число + `I_GOLD` | `cost_background` |
//! | Полоса золота | (361,276) 40×140, заливка `VERTGOLD` снизу | `gold_background` |
//! | Cargo-слайдер | (421,301) 20×115, значение 5·N | `LimitedScrollbar` |
//! | Счётчик cargo | (421,276) 20×10 | `xfer_amount_background` |
//! | Заголовок | (473,7) 158×18 | `_(eb71)` «Purchase Menu» |
//! | `Buy` / `Delete` / `Cancel` / `Done` / `Help` / `Description` / `CRGTOG` | (590,386) 41×22; (412,240) 53×23; (357,452) 62×23; (447,452) 54×23; (420,452) 26×23; (292,264) 17×17; (542,446) 81×32 |
//!
//! # Поведение
//!
//! * `CargoMenu::unit_types1` — 15 покупаемых типов (список покупки); режим
//!   модернизации показывает **полный** отфильтрованный список
//!   (`IsUnitFiltered`: `UPGRADABLE`, строится командой, домен включён,
//!   опционально «только боевые»), отсортированный `IsBetterUnit`.
//! * Цена покупки — `3 * ATTRIB_TURNS` **текущих** значений
//!   (`unitvalues_actual`, после апгрейдов), возврат при продаже — та же цена
//!   плюс `cargo / 5`.
//! * Cargo: 1 кредит за 5 единиц, ёмкость `ATTRIB_STORAGE / 5` (у `GOLDTRCK`
//!   ёмкость 0 — как `Select` в C++); бесплатный cargo миссии (`SC_L0001`)
//!   нельзя уменьшить ниже стартового (`xfer_take_max`).
//! * Апгрейды (`UpgradeControl`): `ATTRIB_ATTACK`, `ROUNDS`, `RANGE`, `AMMO`
//!   (только если базовая атака), `ARMOR`, `HITS`, `SCAN`, `SPEED`; цена —
//!   [`max_core::research::UpgradeQuote`] (целочисленный
//!   `UpgradeControl_CalculateCost`, сверен с C++-векторами), шаг — 1/2/5/10,
//!   лимит показа — `UPGRADECONTROL_UPGRADE_COST_LIMIT` (800). `ATTRIB_TURNS`
//!   в C++-порте контролом не добавляется (`UPGRADE_CONTROL_9` не
//!   используется) — `CargoMenu::UpdateTeamGold` там мёртвый код.
//! * `Done` фиксирует кредиты/загрузку/изменённые `UnitValues`
//!   (`CommitUpgradeChanges`): они уходят в `LocalSetup` и применяются к
//!   `TeamUnits.current_values` до высадки.
//!
//! # Осознанные отличия
//!
//! * Иконки списков — ресурсы `I_*` (спрайт-формат): корпус (кадр 0) и
//!   отдельный кадр башни/пушки (кадр 1) для `TURRET_SPRITE`/
//!   `SPINNING_TURRET`, как `ReportStats_DrawListItemIcon`. Сдвиг башни —
//!   `angle_offsets[2]` из `D_*` (`Unit::GetFrameInfo()`), делённый на
//!   `divisor` (1 у мобильных, 2 у стационарных).
//! * Портрет — big image `P_*` 300×240 1:1, как `WindowManager_LoadBigImage`
//!   (`palette_from_image=false`, `ulx/uly` из ресурса = 0): индексы читаются
//!   палитрой окна `CARGOPIC`. C++-порт пишет 300 px в буфер диалога **без
//!   клипа**, поэтому его 20 правых пикселей залезают на фон; здесь портрет
//!   обрезается окном `unit_portrait_window` (280×240) — как окно оригинала.
//! * Описание — `Unit::GetDescription()`: тексты `language.json` (en-US) по
//!   hex-ключу из `SC_UNITS`, 93 записи в `data/cargo_unit_data.json`
//!   ([`crate::cargo_unit_data`]). Рисуется поверх портрета
//!   (`Text_TextBox(16,17,280,230, GNW_TEXT_OUTLINE|0xA2)`, шрифт 5, перенос
//!   `Text_SplitText`). У 24 служебных типов (обломки, взрывы, снаряды,
//!   `HARVSTER`, `WALDO`) описания в оригинале нет, но они и не достижимы в
//!   `CargoMenu` (нет `UPGRADABLE`/не строятся).
//! * `UnitStats_DrawStats` — 1:1: сетка иконок `I_*` (`I_HRDATK`, `I_SHOTS`,
//!   `I_RANGE`, `I_AMMO`, `I_RAW`/`I_FUEL`/`I_GOLD`, `I_POWER`, `I_LIFE`,
//!   `I_ARMOR`, `I_HITS`, `I_SCAN`, `I_SPEED`, `I_GOLD`/`I_GOLDE`), шаг pips
//!   `UnitStats_DrawIcons`, красные линии строк, числа/подписи
//!   `ReportStats_DrawNumber`/`Text_Blit` (x=24/28, клип 49 px).
//! * Числа `Cost`/`Credit`/`Cargo` под списками: в C++-порте нарисованы только
//!   подписи (числа потеряны при портировании), здесь значения выводятся над
//!   подписями (как в оригинальной игре).
//! * `Help` (`BLDHLP`) открывает окно `CARGO_SETUP` (`HelpMenu_Menu`,
//!   `HELPFRAM`, P8) поверх `CARGOPIC`; `?` делает то же.
//! * Звуки (`KCARG0`/`NDONE0`/…) не проигрываются (аудио-слой отдельный).
//! * ИИ по-прежнему получает стандартную загрузку `SC_L0001`
//!   (`RollTeamMissionSupplies` не портирован) — это не менялось.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use max_assets::image::{BigImage, SimpleImage};
use max_assets::res::ResArchive;
use max_assets::sprite::{decode_sprite, SpriteFrame};
use max_core::access::unit_type_flags;
use max_core::deploy::{TeamSupply, DEFAULT_MISSION_LOADOUT};
use max_core::enums::{unit_flags, TeamClan, UnitType, UNIT_END};
use max_core::production::builder_type;
use max_core::research::UpgradeQuote;
use max_core::team::TeamId;
use max_core::unit_values::{
    team_base_values, UnitValues, ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK,
    ATTRIB_HITS, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN, ATTRIB_SPEED, ATTRIB_STORAGE,
    ATTRIB_TURNS,
};
#[cfg(test)]
use max_core::unit_values::base_unit_values;
use max_core::victory::{
    cargo_fuel_consumption_rate, cargo_gold_consumption_rate, cargo_life_consumption_rate,
    cargo_power_consumption_rate, cargo_raw_consumption_rate,
};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFont, GnwFontId};
use max_render::{ColorLut, TeamColors};

use crate::assets::Assets;
use crate::cargo_unit_data;
use crate::menu::{
    blit_simple, draw_big_image, draw_big_image_at, draw_image_button, wrap_gnw_text,
    MenuButtonPair,
};
use crate::panels::UiPalette;
use crate::ui::Key;
use crate::upgrademenu::{is_better_unit, is_buildable, UpgradeFilter};

// ---------------------------------------------------------------------------
// Раскладка CargoMenu (cargomenu.cpp)
// ---------------------------------------------------------------------------

/// Ширина кадра оригинала.
pub const SCREEN_WIDTH: i32 = 640;
/// Высота кадра оригинала.
pub const SCREEN_HEIGHT: i32 = 480;

/// Портрет (`unit_portrait_window`, 11,13 + 280×240).
pub const PORTRAIT: PixelRect = PixelRect::new(11, 13, 280, 240);
/// Список «Available» (`wininfo1` 482,80 — 605,370).
pub const AVAILABLE_AREA: PixelRect = PixelRect::new(482, 80, 123, 290);
/// Высота строки списков (`unittypeselector.cpp:64`, `* 32`).
pub const LIST_ROW: i32 = 32;
/// Строк в «Available»: `(370 - 80) / 32`.
pub const AVAILABLE_ROWS: usize = 9;
/// Колонка цен списка «Available» (`PurchaseTypeSelector`, x = 482 + 128, 15 px,
/// число выровнено по правому краю 625).
pub const PRICE_COLUMN: PixelRect = PixelRect::new(610, 80, 15, 288);
/// Список «Purchased» (`wininfo2` 337,45 — 455,230).
pub const PURCHASED_AREA: PixelRect = PixelRect::new(337, 45, 118, 185);
/// Строк в «Purchased»: `(230 - 45) / 32`.
pub const PURCHASED_ROWS: usize = 5;
/// Заголовок «Purchased» (`Text_TextBox(337,22,118,10)`).
pub const PURCHASED_HEADER: PixelRect = PixelRect::new(337, 22, 118, 10);
/// Разделитель под «Purchased» (`draw_line(337,34 — 455,34)`).
pub const PURCHASED_LINE_Y: i32 = 34;
/// Заголовок «Available» (`Text_TextBox(482,57,143,10)`).
pub const AVAILABLE_HEADER: PixelRect = PixelRect::new(482, 57, 143, 10);
/// Разделитель под «Available» (`draw_line(482,69 — 625,69)`).
pub const AVAILABLE_LINE_Y: i32 = 69;
/// Заголовок «Cost» над колонкой цен (`Text_TextBox(625 - w, 57, w, 10)`).
pub const COST_HEADER: PixelRect = PixelRect::new(610, 57, 15, 10);
/// Заголовок окна (`Text_TextBox(473,7,158,18, COLOR_GREEN)`).
pub const TITLE: PixelRect = PixelRect::new(473, 7, 158, 18);

/// Блок статистики (`stats_background` 11,293,250,174).
pub const STATS: PixelRect = PixelRect::new(11, 293, 250, 174);
/// Строка статистики: `UnitStats_DrawText` — 19 px, число/подпись по центру.
pub const STAT_ROW: i32 = 19;
/// Где начинаются pips (`UnitStats_DrawStats`: `&buffer[76]`).
pub const STAT_ICON_X: i32 = 76;
/// Сколько пикселей отводится под подпись (`Text_Blit(..., 49, ...)`).
pub const STAT_LABEL_WIDTH: i32 = 49;
/// Область описания: `Text_TextBox(unit_portrait_window.buffer, ..., 16, 17,
/// 280, 230, GNW_TEXT_OUTLINE | 0xA2, false, false)` — от (11,13).
pub const DESCRIPTION: PixelRect = PixelRect::new(PORTRAIT.ulx + 16, PORTRAIT.uly + 17, 280, 230);
/// `ReportStats_DrawNumber`/`Text_Blit` в строке статистики: `COLOR_YELLOW`.
pub const STAT_COLOR_NUMBER: u8 = 0x04;
/// `UnitStats_DrawText(..., 0xA2)` — цвет подписи и числа описания.
pub const STAT_COLOR_LABEL: u8 = 0xA2;
/// `COLOR_RED` — линии-разделители строк и дельта апгрейда.
pub const STAT_COLOR_LINE: u8 = 0x01;
/// `COLOR_BLACK` — обводка описания (`GNW_TEXT_OUTLINE` → `Color_RGB2Color(0)`).
pub const STAT_COLOR_OUTLINE: u8 = 0x00;

/// Подписи строк статистики — `language.json` en-US, как `_(...)` в
/// `unitstats.cpp` (`Attack` `_(98a8)`, `Shots` `_(5e40)`, `Range` `_(2540)`,
/// `Ammo` `_(d510)`, `Cargo` `_(054b)`, `Prod.` `_(527f)`/`_(3641)`,
/// `Uses` `_(9e61)`, `Armor` `_(49d1)`, `Hits` `_(6e4e)`, `Scan` `_(d10e)`,
/// `Speed` `_(6136)`, `Cost` `_(7dfb)`).
pub const STAT_ATTACK: &str = "Attack";
/// `_(5e40)`.
pub const STAT_SHOTS: &str = "Shots";
/// `_(2540)`.
pub const STAT_RANGE: &str = "Range";
/// `_(d510)`.
pub const STAT_AMMO: &str = "Ammo";
/// `_(054b)`.
pub const STAT_CARGO: &str = "Cargo";
/// `_(527f)` (выработка) и `_(3641)` (жизнеобеспечение).
pub const STAT_PROD: &str = "Prod.";
/// `_(9e61)` — расход ресурсов.
pub const STAT_USES: &str = "Uses";
/// `_(49d1)`.
pub const STAT_ARMOR: &str = "Armor";
/// `_(6e4e)`.
pub const STAT_HITS: &str = "Hits";
/// `_(d10e)`.
pub const STAT_SCAN: &str = "Scan";
/// `_(6136)`.
pub const STAT_SPEED: &str = "Speed";
/// `_(7dfb)`.
pub const STAT_COST: &str = "Cost";
/// Колонка цен апгрейдов (`cost_background` 321,293,40,174).
pub const COST: PixelRect = PixelRect::new(321, 293, 40, 174);
/// Колонка стрелок апгрейдов (`button_background` 283,293,38,174).
pub const CONTROL_BUTTONS: PixelRect = PixelRect::new(283, 293, 38, 174);
/// Шаг строки апгрейда (`upgrade_control_next_uly += 19`).
pub const CONTROL_ROW: i32 = 19;
/// Полоса золота (`gold_background` 361,276,40,140).
pub const GOLD_BAR: PixelRect = PixelRect::new(361, 276, 40, 140);
/// Cargo-слайдер (`Scrollbar` 421,301 — 441,416).
pub const CARGO_BAR: PixelRect = PixelRect::new(421, 301, 20, 115);
/// Счётчик cargo над слайдером (`xfer_amount_bounds` 421,276 — 441,286).
pub const CARGO_AMOUNT: PixelRect = PixelRect::new(421, 276, 20, 10);

/// Подпись «Description» (`Text_TextBox(209,264,80,17)`).
pub const DESCRIPTION_LABEL: PixelRect = PixelRect::new(209, 264, 80, 17);
/// Подпись «Cost» (`Text_TextBox(320,283,48,16)`).
pub const COST_LABEL: PixelRect = PixelRect::new(320, 283, 48, 16);
/// Подпись «Credit» (`Text_TextBox(358,283,48,16)`).
pub const CREDIT_LABEL: PixelRect = PixelRect::new(358, 283, 48, 16);
/// Подпись «Cargo» (`Text_TextBox(409,283,48,16)`).
pub const CARGO_LABEL: PixelRect = PixelRect::new(409, 283, 48, 16);

/// Кнопка `Buy` (`CRGBUY_U`, 41×22).
pub const BUY_BUTTON: PixelRect = PixelRect::new(590, 386, 41, 22);
/// Кнопка `Delete` (`CRGDEL_U`, 53×23).
pub const DELETE_BUTTON: PixelRect = PixelRect::new(412, 240, 53, 23);
/// Кнопка `Cancel` (`BLDCAN_U`, 62×23).
pub const CANCEL_BUTTON: PixelRect = PixelRect::new(357, 452, 62, 23);
/// Кнопка `Done` (`BLDONE_U`, 54×23).
pub const DONE_BUTTON: PixelRect = PixelRect::new(447, 452, 54, 23);
/// Кнопка `Help` (`BLDHLP_U`, 26×23).
pub const HELP_BUTTON: PixelRect = PixelRect::new(420, 452, 26, 23);
/// Кнопка описания (`BLDDES_U`, 17×17).
pub const DESCRIPTION_BUTTON: PixelRect = PixelRect::new(292, 264, 17, 17);
/// Переключатель «Buy Units»/«Upgrades» (`CRGTOG_U`, 81×32).
pub const TOGGLE_BUTTON: PixelRect = PixelRect::new(542, 446, 81, 32);
/// Скролл списка «Available» вверх (`BLDUP__U`, 18×17).
pub const LIST_UP: PixelRect = PixelRect::new(471, 387, 18, 17);
/// Скролл списка «Available» вниз (`BLDDWN_U`).
pub const LIST_DOWN: PixelRect = PixelRect::new(491, 387, 18, 17);
/// Скролл списка «Purchased» вверх (`BLDUP__U`).
pub const PURCHASE_UP: PixelRect = PixelRect::new(327, 240, 18, 17);
/// Скролл списка «Purchased» вниз (`BLDDWN_U`).
pub const PURCHASE_DOWN: PixelRect = PixelRect::new(348, 240, 18, 17);
/// Размер иконки строки (`ReportStats_DrawListItemIcon`, 32×32).
pub const ICON_BOX: i32 = 32;

/// `CargoMenu::unit_types1` (`cargomenu.cpp:176`) — список покупки, порядок
/// строк списка.
pub const PURCHASE_UNITS: [UnitType; 15] = [
    UnitType::Constructor,
    UnitType::Engineer,
    UnitType::Surveyor,
    UnitType::Scout,
    UnitType::Scanner,
    UnitType::Tank,
    UnitType::AssaultGun,
    UnitType::RocketLauncher,
    UnitType::MissileCrawler,
    UnitType::MineLayer,
    UnitType::RepairUnit,
    UnitType::SupplyTruck,
    UnitType::FuelTruck,
    UnitType::MobileAntiAir,
    UnitType::GoldTruck,
];

/// `Cargo_GetRawConsumptionRate(LANDPLT, 1)` (`cargo.cpp:168`).
pub const LANDPLT_RAW_CONSUMPTION: u32 = 3;
/// Верхняя граница показываемой стоимости (`UPGRADECONTROL_UPGRADE_COST_LIMIT`).
pub const UPGRADE_COST_LIMIT: i32 = 800;

// ---------------------------------------------------------------------------
// Состояние экрана
// ---------------------------------------------------------------------------

/// Режим списка типов (переключатель `CRGTOG`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoMode {
    /// Список покупки — `CargoMenu::unit_types1`.
    Buy,
    /// Полный список модернизации (`AbstractUpgradeMenu::PopulateTeamUnitsList`).
    Upgrade,
}

/// Какой список принимает стрелки/PageUp/PageDown (`active_selector`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveList {
    /// «Available» (типы).
    Types,
    /// «Purchased» (купленные и бесплатная загрузка).
    Loadout,
}

/// Действие экрана после клика/клавиши.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoAction {
    /// Экран остаётся открытым.
    None,
    /// `Cancel`/`Esc` — отмена старта.
    Cancel,
    /// `Done`/`Enter` — зафиксировать и продолжить.
    Done,
    /// `Help`/`?` — открыть окно `CARGO_SETUP` (`HelpMenu_Menu`).
    Help,
}

/// Группа иконок (pips) строки статистики — порт `UnitStats_DrawIcons`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatIcon {
    /// Ресурс `I_*` (simple image), которым рисуются «заполненные» pips.
    pub resource: &'static str,
    /// Ресурс «пустых» pips (`I_*E`); у всех строк кроме `Cost` — тот же.
    pub empty_resource: &'static str,
    /// `current` в `UnitStats_DrawIcons` — сколько pips рисуется `resource`.
    pub current: i32,
    /// `original` — сколько всего; `original - current` рисуется
    /// `empty_resource` за красной чертой (дельта апгрейда).
    pub original: i32,
}

impl StatIcon {
    /// Пара одинаковых иконок (`UnitStats_DrawIcons(id, id, ...)`).
    pub const fn same(resource: &'static str, current: i32, original: i32) -> Self {
        Self {
            resource,
            empty_resource: resource,
            current,
            original,
        }
    }

    /// Сколько всего pips нарисует группа (`max(0, current) + max(0, delta)`).
    pub fn count(&self) -> usize {
        self.current.max(0) as usize + (self.original - self.current).max(0) as usize
    }
}

/// Строка таблицы статистики — порт `UnitStats_DrawText` + `UnitStats_DrawIcons`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatRow {
    /// `y` строки (тот же шаг 19 px, что у колонки апгрейдов).
    pub y: i32,
    /// Подпись из `language.json` (`_(98a8)` → `Attack`, …).
    pub label: &'static str,
    /// Число (`ReportStats_DrawNumber`); `None` — строке число не нужно
    /// (`_(9e61)`: `resource_count == 0`).
    pub value: Option<i32>,
    /// Группы иконок после числа; смещаются на `STAT_ICON_X` и дальше.
    pub icons: Vec<StatIcon>,
    /// Красная линия под строкой (`draw_line(2,18 — 248,18)`); у `Cost` — нет.
    pub separator: bool,
}

impl StatRow {
    /// Сколько pips в строке (для QA/тестов).
    pub fn icon_count(&self) -> usize {
        self.icons.iter().map(StatIcon::count).sum()
    }
}

/// Состояние экрана мобилизации одной команды.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mobilization {
    /// Чья мобилизация.
    pub team: TeamId,
    /// Клан команды (значения и цены).
    pub clan: TeamClan,
    /// Кредиты на входе (`CargoMenu::start_gold`).
    pub credits_start: u32,
    /// Остаток кредитов (`CargoMenu::team_gold`).
    pub credits: u32,
    /// Бесплатная загрузка миссии (`SC_L0001`), удалять/уменьшать нельзя.
    pub free: Vec<TeamSupply>,
    /// Купленные юниты (в порядке покупки).
    pub bought: Vec<TeamSupply>,
    /// Режим списка типов.
    pub mode: CargoMode,
    /// Типы в списке «Available» (в режиме `Buy` — 15).
    pub types: Vec<UnitType>,
    /// Выбранная строка списка типов (`page_max_index`).
    pub type_selected: usize,
    /// Первая видимая строка списка типов (`page_min_index`).
    pub type_page: usize,
    /// Выбранная строка списка «Purchased» (0..free+bought).
    pub loadout_selected: usize,
    /// Первая видимая строка «Purchased».
    pub loadout_page: usize,
    /// Какой список активен для стрелок.
    pub active: ActiveList,
    /// Тип, чей портрет/статистика/апгрейды показаны (`unit_type`).
    pub info: Option<UnitType>,
    /// Базовые значения на момент открытия (`unitvalues_base`).
    pub base: [UnitValues; UNIT_END as usize],
    /// Редактируемые значения (`unitvalues_actual`).
    pub actual: [UnitValues; UNIT_END as usize],
    /// Строки апгрейдов выбранного типа: `(ATTRIB_*, uly)`.
    pub controls: Vec<(u8, i32)>,
    /// Фильтры домена (`button_*_rest_state`): ground, air, sea, building, combat.
    pub filters: [bool; 5],
    /// Показывать описание (`button_description_rest_state`); текста в данных нет.
    pub description: bool,
    /// Строка статуса.
    pub status: String,
}

impl Mobilization {
    /// `CargoMenu::CargoMenu(team)`: `start_gold + Clans::GetCredits(clan)`
    /// (у всех кланов `SC_CLANS` кредиты 0), базовые значения команды и
    /// бесплатная загрузка миссии.
    pub fn new(team: TeamId, clan: TeamClan, start_gold: u32) -> Self {
        let base = team_base_values(clan);
        let free = DEFAULT_MISSION_LOADOUT.to_vec();
        let info = free.first().map(|supply| supply.unit_type);

        let mut mobilization = Self {
            team,
            clan,
            credits_start: start_gold,
            credits: start_gold,
            free,
            bought: Vec::new(),
            mode: CargoMode::Buy,
            types: PURCHASE_UNITS.to_vec(),
            type_selected: 0,
            type_page: 0,
            loadout_selected: 0,
            loadout_page: 0,
            active: ActiveList::Types,
            info,
            base,
            actual: base,
            controls: Vec::new(),
            filters: [true, true, true, true, false],
            description: true,
            status: String::new(),
        };

        mobilization.rebuild_controls();
        mobilization
    }

    /// Нужна ли фаза: в C++ `CargoMenu` не открывается при нулевом бюджете
    /// (`if (team_gold)` в `GameManager_PlayerMissionSetup`).
    pub fn needed(start_gold: u32) -> bool {
        start_gold > 0
    }

    /// Итоговая загрузка `TeamMissionSupplies::units` (бесплатная + купленная).
    pub fn loadout(&self) -> Vec<TeamSupply> {
        let mut loadout = self.free.clone();
        loadout.extend(self.bought.iter().copied());
        loadout
    }

    /// Все строки списка «Purchased» (`unit_types2`).
    pub fn loadout_rows(&self) -> Vec<TeamSupply> {
        self.loadout()
    }

    /// `CargoMenu::DrawUnitInfo`: цена текущих значений, `3 * turns`.
    pub fn price(&self, unit: UnitType) -> u32 {
        unit_price(&self.actual, unit)
    }

    /// Цена строки списка типов (`PurchaseTypeSelector::Draw`):
    /// `STATIONARY` → `CONSTRCT`-ставка (2), `COMMANDO`/`INFANTRY` →
    /// `TRAINHAL`-ставка (1), иначе `LANDPLT` (3); умножить на `turns`.
    pub fn list_price(&self, unit: UnitType) -> u32 {
        list_price(&self.actual, unit)
    }

    /// Цена выбранной строки списка типов (для подписи `Cost`).
    pub fn selected_cost(&self) -> u32 {
        self.types
            .get(self.type_selected)
            .map(|unit| self.price(*unit))
            .unwrap_or(0)
    }

    /// Выбранный тип списка «Available».
    pub fn selected_type(&self) -> Option<UnitType> {
        self.types.get(self.type_selected).copied()
    }

    /// Строка «Purchased» по индексу.
    pub fn loadout_row(&self, index: usize) -> Option<TeamSupply> {
        if index < self.free.len() {
            self.free.get(index).copied()
        } else {
            self.bought.get(index - self.free.len()).copied()
        }
    }

    /// Сколько единиц cargo в строке `index`.
    pub fn cargo(&self, index: usize) -> u16 {
        self.loadout_row(index).map_or(0, |supply| supply.cargo)
    }

    /// Строка, которой можно управлять слайдером (выбранная в «Purchased»).
    fn cargo_row(&self) -> usize {
        self.loadout_selected
    }

    /// Ёмкость cargo в блоках по 5 (`Select`: `ATTRIB_STORAGE / 5`, у
    /// `GOLDTRCK` — 0).
    pub fn cargo_capacity(&self) -> i32 {
        let Some(supply) = self.loadout_row(self.cargo_row()) else {
            return 0;
        };
        cargo_capacity(&self.actual, supply.unit_type)
    }

    /// Значение слайдера (`cargos[index] / 5`).
    pub fn cargo_value(&self) -> i32 {
        i32::from(self.cargo(self.cargo_row())) / 5
    }

    /// `xfer_take_max`: бесплатный cargo миссии нельзя уменьшить, у купленных —
    /// можно до нуля.
    fn cargo_take_max(&self) -> i32 {
        let index = self.cargo_row();
        if index < self.free.len() {
            i32::from(self.cargo(index)) / 5
        } else {
            0
        }
    }

    /// `xfer_give_max`: `cargos[page] / 5 + team_gold` (`DrawUnitStats`).
    fn cargo_give_max(&self) -> i32 {
        self.cargo_value() + self.credits as i32
    }

    /// Материал полосы cargo (`VERTFUEL`/`VERTGOLD`/`VERTRAW`).
    pub fn cargo_bar_kind(&self) -> CargoBarKind {
        match self
            .loadout_row(self.cargo_row())
            .map(|supply| supply.unit_type)
        {
            Some(UnitType::FuelTruck) => CargoBarKind::Fuel,
            Some(UnitType::GoldTruck) => CargoBarKind::Gold,
            _ => CargoBarKind::Raw,
        }
    }

    /// Может ли `Delete` продать выбранную строку (`index >= unit_count`).
    pub fn can_delete(&self) -> bool {
        self.loadout_selected >= self.free.len()
            && self.loadout_selected < self.loadout_rows().len()
    }

    /// Может ли `Buy` купить выбранный тип (`DrawUnitInfo`).
    pub fn can_buy(&self) -> bool {
        self.selected_type()
            .is_some_and(|unit| PURCHASE_UNITS.contains(&unit) && self.price(unit) <= self.credits)
    }

    // -----------------------------------------------------------------------
    // Списки
    // -----------------------------------------------------------------------

    /// `CargoMenu::PopulateTeamUnitsList`: режим `Buy` — `unit_types1`, режим
    /// `Upgrade` — отфильтрованный полный список `IsBetterUnit`-порядка.
    pub fn rebuild_types(&mut self) {
        let previous = self.selected_type();
        self.types = match self.mode {
            CargoMode::Buy => PURCHASE_UNITS.to_vec(),
            CargoMode::Upgrade => upgrade_types(&self.filters),
        };

        self.type_selected = previous
            .and_then(|unit| self.types.iter().position(|candidate| *candidate == unit))
            .unwrap_or(0);
        self.ensure_type_visible();

        if previous.is_some() && self.selected_type() != previous {
            // C++ `AddItems`: `if (GetLast() != unit_type) Select(0)` →
            // `DrawUnitInfo(unit_type)`.
            self.info = self.selected_type();
            self.rebuild_controls();
        }
    }

    /// `UnitTypeSelector::ScrollTo` для списка типов.
    fn ensure_type_visible(&mut self) {
        let count = self.types.len() as i64;
        let visible = AVAILABLE_ROWS as i64;
        let mut page = self.type_page as i64;
        let selected = self.type_selected as i64;

        if selected < page {
            page = selected;
        }
        if page + visible <= selected {
            page = selected - visible + 1;
        }
        if page + visible > count {
            page = count - visible;
        }
        if page < 0 {
            page = 0;
        }

        self.type_page = page.max(0) as usize;
    }

    /// То же для списка «Purchased».
    fn ensure_loadout_visible(&mut self) {
        let count = self.loadout_rows().len() as i64;
        let visible = PURCHASED_ROWS as i64;
        let mut page = self.loadout_page as i64;
        let selected = self.loadout_selected as i64;

        if selected < page {
            page = selected;
        }
        if page + visible <= selected {
            page = selected - visible + 1;
        }
        if page + visible > count {
            page = count - visible;
        }
        if page < 0 {
            page = 0;
        }

        self.loadout_page = page.max(0) as usize;
    }

    /// `PopulateTeamUnitsList` по переключателю (`1100`/`1101`).
    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            CargoMode::Buy => CargoMode::Upgrade,
            CargoMode::Upgrade => CargoMode::Buy,
        };
        self.rebuild_types();
        self.status = match self.mode {
            CargoMode::Buy => "Buy units".to_string(),
            CargoMode::Upgrade => "Upgrade units".to_string(),
        };
    }

    /// Переключает фильтр домена (`1003`…`1012`); в режиме покупки кнопки
    /// выключены (C++ `Disable`).
    pub fn toggle_filter(&mut self, index: usize) {
        if self.mode != CargoMode::Upgrade || index >= self.filters.len() {
            return;
        }
        self.filters[index] = !self.filters[index];
        let previous = self.selected_type();
        self.types = upgrade_types(&self.filters);
        self.type_selected = previous
            .and_then(|unit| self.types.iter().position(|candidate| *candidate == unit))
            .unwrap_or(0);
        self.ensure_type_visible();
        if self.selected_type() != previous {
            self.info = self.selected_type();
            self.rebuild_controls();
        }
    }

    // -----------------------------------------------------------------------
    // Выбор и загрузка
    // -----------------------------------------------------------------------

    /// `CargoMenu::Select(index)` для списка «Purchased».
    pub fn select_loadout(&mut self, index: usize) {
        let count = self.loadout_rows().len();
        if index >= count {
            return;
        }

        self.loadout_selected = index;
        self.ensure_loadout_visible();

        if let Some(supply) = self.loadout_row(index) {
            self.info = Some(supply.unit_type);
            self.rebuild_controls();
        }
    }

    /// Выбор строки списка «Available» (клик по ней).
    pub fn select_type(&mut self, index: usize) {
        if index >= self.types.len() {
            return;
        }

        self.type_selected = index;
        self.active = ActiveList::Types;
        self.ensure_type_visible();
        self.info = self.selected_type();
        self.rebuild_controls();
    }

    /// Стрелка/PageUp/PageDown по активному списку.
    pub fn step(&mut self, delta: i32) {
        match self.active {
            ActiveList::Types => {
                if self.types.is_empty() {
                    return;
                }
                let last = self.types.len() as i32 - 1;
                self.type_selected = (self.type_selected as i32 + delta).clamp(0, last) as usize;
                self.ensure_type_visible();
                self.info = self.selected_type();
                self.rebuild_controls();
            }
            ActiveList::Loadout => {
                let count = self.loadout_rows().len();
                if count == 0 {
                    return;
                }
                let last = count as i32 - 1;
                self.loadout_selected =
                    (self.loadout_selected as i32 + delta).clamp(0, last) as usize;
                self.ensure_loadout_visible();
                if let Some(supply) = self.loadout_row(self.loadout_selected) {
                    self.info = Some(supply.unit_type);
                    self.rebuild_controls();
                }
            }
        }
    }

    /// Страница списка (по `max_item_count` строк).
    pub fn page(&mut self, delta: i32) {
        let rows = match self.active {
            ActiveList::Types => AVAILABLE_ROWS as i32,
            ActiveList::Loadout => PURCHASED_ROWS as i32,
        };
        self.step(delta * rows);
    }

    /// Прокрутка списка без смены выбора (стрелки `BLDUP__`/`BLDDWN`).
    pub fn scroll(&mut self, list: ActiveList, delta: i32) {
        match list {
            ActiveList::Types => {
                let rows = self.types.len().saturating_sub(AVAILABLE_ROWS);
                self.type_page = (self.type_page as i32 + delta).clamp(0, rows as i32) as usize;
            }
            ActiveList::Loadout => {
                let rows = self.loadout_rows().len().saturating_sub(PURCHASED_ROWS);
                self.loadout_page =
                    (self.loadout_page as i32 + delta).clamp(0, rows as i32) as usize;
            }
        }
    }

    /// `CargoMenu::BuyUnit`: купить выбранный тип.
    pub fn buy_selected(&mut self) -> bool {
        self.buy(self.type_selected)
    }

    /// Покупка по индексу списка «Available».
    pub fn buy(&mut self, index: usize) -> bool {
        let Some(&unit) = self.types.get(index) else {
            return false;
        };

        if !PURCHASE_UNITS.contains(&unit) {
            self.status = format!("{} is not for sale", unit_label(unit));
            return false;
        }

        let cost = self.price(unit);
        if cost > self.credits {
            self.status = format!("Not enough credits: {} costs {cost}", unit_label(unit));
            return false;
        }

        self.credits -= cost;
        self.bought.push(TeamSupply {
            unit_type: unit,
            cargo: 0,
        });
        self.loadout_selected = self.loadout_rows().len() - 1;
        self.ensure_loadout_visible();
        self.info = Some(unit);
        self.rebuild_controls();
        self.status = format!("Bought {} for {cost} credits", unit_label(unit));
        true
    }

    /// `CargoMenu::DeleteUnit`: продать выбранную строку «Purchased» (кроме
    /// бесплатной загрузки миссии).
    pub fn delete_selected(&mut self) -> bool {
        if !self.can_delete() {
            self.status = if self.loadout_selected < self.free.len() {
                "The free mission loadout cannot be sold".to_string()
            } else {
                "Select a purchased unit to delete".to_string()
            };
            return false;
        }

        let index = self.loadout_selected - self.free.len();
        let supply = self.bought.remove(index);
        let refund = self.price(supply.unit_type) + u32::from(supply.cargo) / 5;

        self.credits = self.credits.saturating_add(refund);
        self.loadout_selected = self.loadout_selected.min(self.loadout_rows().len() - 1);
        self.ensure_loadout_visible();
        if let Some(supply) = self.loadout_row(self.loadout_selected) {
            self.info = Some(supply.unit_type);
        }
        self.rebuild_controls();
        self.status = format!("Sold {} for {refund} credits", unit_label(supply.unit_type));
        true
    }

    /// Клик по слайдеру cargo (`Scrollbar::ProcessKey`, `pcode 1106`).
    pub fn click_cargo(&mut self, x: i32, y: i32) -> bool {
        if !CARGO_BAR.contains(x, y) {
            return false;
        }

        let capacity = self.cargo_capacity();
        if capacity <= 0 {
            return false;
        }

        let from_bottom = CARGO_BAR.lry() - y;
        let value = (CARGO_BAR.height / 2 + capacity * from_bottom) / CARGO_BAR.height;
        let value = value
            .max(self.cargo_take_max())
            .min(capacity)
            .min(self.cargo_give_max());
        self.set_cargo_value(value);
        true
    }

    /// Ставит значение слайдера (`cargos[index] = value * 5`), списывая
    /// `new - old` кредитов (1 кредит за 5 единиц).
    pub fn set_cargo_value(&mut self, value: i32) {
        let capacity = self.cargo_capacity();
        if capacity < 0 {
            return;
        }
        let value = value
            .max(self.cargo_take_max())
            .min(capacity)
            .min(self.cargo_give_max())
            .max(0);
        let old = self.cargo_value();
        let delta = value - old;

        if delta > 0 {
            self.credits = self.credits.saturating_sub(delta as u32);
        } else {
            self.credits = self.credits.saturating_add((-delta) as u32);
        }

        let cargo = (value * 5) as u16;
        let index = self.cargo_row();
        if index < self.free.len() {
            self.free[index].cargo = cargo;
        } else if let Some(supply) = self.bought.get_mut(index - self.free.len()) {
            supply.cargo = cargo;
        }
    }

    // -----------------------------------------------------------------------
    // Апгрейды
    // -----------------------------------------------------------------------

    /// `AbstractUpgradeMenu::DrawUnitInfo` → `AddUpgradeMilitary`,
    /// `AdjustRowStorage`, `AdjustRowConsumptions`, `AddUpgradeGeneric`,
    /// `AddUpgradeMobile`. `ATTRIB_TURNS` контролом не добавляется (в C++-порте
    /// `UPGRADE_CONTROL_9` нигде не используется).
    pub fn rebuild_controls(&mut self) {
        self.controls.clear();
        let Some(unit) = self.info else {
            return;
        };
        let true_base = self.base[unit as usize];
        let actual = self.actual[unit as usize];
        let mut y = COST.uly;
        let mut rows: Vec<(u8, i32)> = Vec::new();

        let push = |rows: &mut Vec<(u8, i32)>, y: &mut i32, attribute: u8| {
            rows.push((attribute, *y));
            *y += CONTROL_ROW;
        };

        if true_base.get_attribute(ATTRIB_ATTACK) > 0 {
            push(&mut rows, &mut y, ATTRIB_ATTACK);
            if true_base.get_attribute(ATTRIB_ROUNDS) > 0 {
                push(&mut rows, &mut y, ATTRIB_ROUNDS);
            }
            if true_base.get_attribute(ATTRIB_RANGE) > 0 {
                push(&mut rows, &mut y, ATTRIB_RANGE);
            }
            if true_base.get_attribute(ATTRIB_AMMO) > 0 {
                push(&mut rows, &mut y, ATTRIB_AMMO);
            }
        }

        if actual.get_attribute(ATTRIB_STORAGE) != 0
            && matches!(
                unit,
                UnitType::MaterialStorage
                    | UnitType::FuelStorage
                    | UnitType::GoldVault
                    | UnitType::Engineer
                    | UnitType::Constructor
                    | UnitType::CargoShip
                    | UnitType::SupplyTruck
                    | UnitType::FuelTruck
                    | UnitType::GoldTruck
            )
        {
            y += CONTROL_ROW;
        }

        let raw = cargo_raw_consumption_rate(unit, 1);
        let fuel = cargo_fuel_consumption_rate(unit);
        let power = cargo_power_consumption_rate(unit);
        let life = cargo_life_consumption_rate(unit);
        if power < 0 {
            y += CONTROL_ROW;
        }
        if life < 0 {
            y += CONTROL_ROW;
        }
        if raw > 0 || fuel > 0 || power > 0 || life > 0 {
            y += CONTROL_ROW;
        }

        if true_base.get_attribute(ATTRIB_ARMOR) > 0 {
            push(&mut rows, &mut y, ATTRIB_ARMOR);
        }
        if true_base.get_attribute(ATTRIB_HITS) > 0 {
            push(&mut rows, &mut y, ATTRIB_HITS);
        }
        if true_base.get_attribute(ATTRIB_SCAN) > 0 {
            push(&mut rows, &mut y, ATTRIB_SCAN);
        }
        if true_base.get_attribute(ATTRIB_SPEED) > 0 {
            push(&mut rows, &mut y, ATTRIB_SPEED);
        }

        self.controls = rows;
    }

    /// Контрол строки (порт `UpgradeControl::Init`): `team_base_value` —
    /// базовое значение типа, `control_base_value` — значение на входе в меню,
    /// вклад исследований до старта партии нулевой
    /// (`ResearchMenu_CalculateFactor` при level 0).
    pub fn quote(&self, row: usize) -> Option<UpgradeQuote> {
        let (attribute, _) = *self.controls.get(row)?;
        let unit = self.info?;
        let base_value = self.base[unit as usize].get_attribute(attribute) as u16;

        let mut quote = UpgradeQuote::new(attribute, base_value, base_value, 0);
        quote.actual = self.actual[unit as usize].get_attribute(attribute) as u16;
        Some(quote)
    }

    /// `UpgradeControl::Increase`/`Decrease`; `true`, если значение изменилось.
    pub fn adjust_upgrade(&mut self, row: usize, increase: bool) -> bool {
        let Some(mut quote) = self.quote(row) else {
            return false;
        };
        let Some(unit) = self.info else {
            return false;
        };

        let changed = if increase {
            quote.increase(&mut self.credits)
        } else {
            quote.decrease(&mut self.credits)
        };

        if changed {
            self.actual[unit as usize].set_attribute(quote.attribute, i32::from(quote.actual));
            self.status = format!(
                "{} {} = {}",
                unit_label(unit),
                attribute_name(quote.attribute),
                quote.actual
            );
        } else {
            self.status = if increase {
                "Cannot upgrade: credits, base or limit".to_string()
            } else {
                "Already at the base value".to_string()
            };
        }

        changed
    }

    /// Прямоугольники стрелок строки `row` (левая/правая).
    pub fn control_arrows(&self, row: usize) -> Option<(PixelRect, PixelRect)> {
        let (_, y) = *self.controls.get(row)?;
        Some((
            PixelRect::new(CONTROL_BUTTONS.ulx, y, 18, 17),
            PixelRect::new(CONTROL_BUTTONS.ulx + 18, y, 18, 17),
        ))
    }

    /// Ресурс иконки строки `Cargo` — switch `UnitStats_DrawCargo`.
    ///
    /// Для остальных типов C++ не рисует строку вовсе (`default: return
    /// buffer`), даже если `ATTRIB_STORAGE` > 0 (казармы, док, ремонт, …).
    fn storage_icon(unit: UnitType) -> Option<&'static str> {
        match unit {
            UnitType::MaterialStorage
            | UnitType::CargoShip
            | UnitType::SupplyTruck
            | UnitType::Engineer
            | UnitType::Constructor => Some("I_RAW"),
            UnitType::FuelTruck | UnitType::FuelStorage => Some("I_FUEL"),
            UnitType::GoldVault | UnitType::GoldTruck => Some("I_GOLD"),
            _ => None,
        }
    }

    /// `Cargo_GetRawConsumptionRate(Builder_GetBuilderType(team, unit), 1)` —
    /// ставка строки `Cost` (`UnitStats_DrawCost`). У мин и нестроящихся
    /// `Builder_GetBuilderType` = `INVALID_ID`, ставка 0 (как default в C++).
    pub fn build_rate(&self, unit: UnitType) -> i32 {
        build_rate_for(unit)
    }

    /// `Unit::GetDescription()` выбранного типа (`""` — описания нет).
    pub fn description(&self) -> &'static str {
        self.info.map_or("", cargo_unit_data::unit_description)
    }

    /// Есть ли у выбранного типа портрет `P_*` (`Unit::GetPortrait()`).
    pub fn has_portrait(&self) -> bool {
        self.info.and_then(cargo_unit_data::unit_portrait).is_some()
    }

    /// Сколько pips-иконок в таблице статистики (для QA-проверок).
    pub fn stats_icon_count(&self) -> usize {
        self.stats_rows().iter().map(StatRow::icon_count).sum()
    }

    /// Строки таблицы статистики — порт блоков `UnitStats_DrawStats`.
    ///
    /// Порядок ровно как в C++: military (`Attack`/`Shots`/`Range`/`Ammo`),
    /// cargo (`Cargo`), расход (`Prod.`/`Uses`), общие (`Armor`/`Hits`/`Scan`),
    /// `Speed`, `Cost`. Условия показа — по **базовым** значениям
    /// (`unit_values2 = team_units->GetBaseUnitValues`), числа — по текущим.
    pub fn stats_rows(&self) -> Vec<StatRow> {
        let Some(unit) = self.info else {
            return Vec::new();
        };

        Self::stats_rows_for(&self.base, &self.actual, unit)
    }

    /// [`Self::stats_rows`] по явным наборам значений — нужен `BuildMenu`:
    /// C++ `AbstractBuildMenu::Draw` зовёт `UnitStats_DrawStats` с
    /// `team_units->GetCurrentUnitValues(team, unit_type)` и теми же
    /// `base_values` (клановые модификаторы и апгрейды уже внутри).
    pub fn stats_rows_for(
        base_values: &[UnitValues],
        current_values: &[UnitValues],
        unit: UnitType,
    ) -> Vec<StatRow> {
        let base = base_values[unit as usize];
        let actual = current_values[unit as usize];
        let mut rows = Vec::new();
        let mut y = STATS.uly;

        #[allow(clippy::too_many_arguments)]
        let push = |rows: &mut Vec<StatRow>,
                    y: &mut i32,
                    label: &'static str,
                    value: Option<i32>,
                    icons: Vec<StatIcon>,
                    separator: bool| {
            rows.push(StatRow {
                y: *y,
                label,
                value,
                icons,
                separator,
            });
            *y += STAT_ROW;
        };

        let base_attack = base.get_attribute(ATTRIB_ATTACK);
        let actual_attack = actual.get_attribute(ATTRIB_ATTACK);

        if base_attack > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_ATTACK,
                Some(actual_attack),
                vec![StatIcon::same("I_HRDATK", base_attack, actual_attack)],
                true,
            );

            if base.get_attribute(ATTRIB_ROUNDS) > 0 {
                // Число выстрелов ограничено боезапасом (`std::min(ROUNDS, AMMO)`).
                let shots = actual
                    .get_attribute(ATTRIB_ROUNDS)
                    .min(actual.get_attribute(ATTRIB_AMMO));

                push(
                    &mut rows,
                    &mut y,
                    STAT_SHOTS,
                    Some(shots),
                    vec![StatIcon::same(
                        "I_SHOTS",
                        base.get_attribute(ATTRIB_ROUNDS),
                        shots,
                    )],
                    true,
                );
            }
            if base.get_attribute(ATTRIB_RANGE) > 0 {
                push(
                    &mut rows,
                    &mut y,
                    STAT_RANGE,
                    Some(actual.get_attribute(ATTRIB_RANGE)),
                    vec![StatIcon::same(
                        "I_RANGE",
                        base.get_attribute(ATTRIB_RANGE),
                        actual.get_attribute(ATTRIB_RANGE),
                    )],
                    true,
                );
            }
            if base.get_attribute(ATTRIB_AMMO) > 0 {
                push(
                    &mut rows,
                    &mut y,
                    STAT_AMMO,
                    Some(actual.get_attribute(ATTRIB_AMMO)),
                    vec![StatIcon::same(
                        "I_AMMO",
                        base.get_attribute(ATTRIB_AMMO),
                        actual.get_attribute(ATTRIB_AMMO),
                    )],
                    true,
                );
            }
        }

        let base_storage = base.get_attribute(ATTRIB_STORAGE);
        let actual_storage = actual.get_attribute(ATTRIB_STORAGE);

        if base_storage > 0 {
            if let Some(icon) = Self::storage_icon(unit) {
                push(
                    &mut rows,
                    &mut y,
                    STAT_CARGO,
                    Some(actual_storage),
                    vec![StatIcon::same(icon, base_storage / 10, actual_storage / 10)],
                    true,
                );
            }
        }

        let raw = cargo_raw_consumption_rate(unit, 1);
        let fuel = cargo_fuel_consumption_rate(unit);
        let power = cargo_power_consumption_rate(unit);
        let life = cargo_life_consumption_rate(unit);
        let gold = cargo_gold_consumption_rate(unit);

        if power < 0 {
            push(
                &mut rows,
                &mut y,
                STAT_PROD,
                Some(-power),
                vec![StatIcon::same("I_POWER", -power, -power)],
                true,
            );
        }
        if life < 0 {
            push(
                &mut rows,
                &mut y,
                STAT_PROD,
                Some(-life),
                vec![StatIcon::same("I_LIFE", -life, -life)],
                true,
            );
        }

        if raw > 0 || fuel > 0 || power > 0 || life > 0 {
            // `UnitStats_DrawUsage`: число не рисуется (resource_count = 0),
            // иконки расходов идут подряд в одной строке.
            let mut icons = Vec::new();

            for (resource, value) in [
                ("I_RAW", raw),
                ("I_FUEL", fuel),
                ("I_POWER", power),
                ("I_LIFE", life),
                ("I_GOLD", gold),
            ] {
                if value > 0 {
                    icons.push(StatIcon::same(resource, value, value));
                }
            }

            push(&mut rows, &mut y, STAT_USES, None, icons, true);
        }

        if base.get_attribute(ATTRIB_ARMOR) > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_ARMOR,
                Some(actual.get_attribute(ATTRIB_ARMOR)),
                vec![StatIcon::same(
                    "I_ARMOR",
                    base.get_attribute(ATTRIB_ARMOR),
                    actual.get_attribute(ATTRIB_ARMOR),
                )],
                true,
            );
        }
        if base.get_attribute(ATTRIB_HITS) > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_HITS,
                Some(actual.get_attribute(ATTRIB_HITS)),
                vec![StatIcon::same(
                    "I_HITS",
                    base.get_attribute(ATTRIB_HITS),
                    actual.get_attribute(ATTRIB_HITS),
                )],
                true,
            );
        }
        if base.get_attribute(ATTRIB_SCAN) > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_SCAN,
                Some(actual.get_attribute(ATTRIB_SCAN)),
                vec![StatIcon::same(
                    "I_SCAN",
                    base.get_attribute(ATTRIB_SCAN),
                    actual.get_attribute(ATTRIB_SCAN),
                )],
                true,
            );
        }
        if base.get_attribute(ATTRIB_SPEED) > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_SPEED,
                Some(actual.get_attribute(ATTRIB_SPEED)),
                vec![StatIcon::same(
                    "I_SPEED",
                    base.get_attribute(ATTRIB_SPEED),
                    actual.get_attribute(ATTRIB_SPEED),
                )],
                true,
            );
        }
        if base.get_attribute(ATTRIB_TURNS) > 0 {
            // `UnitStats_DrawCost`: уровень = ставка строителя * ходы, минимум 1;
            // заполнены текущие ходы, «пустые» — базовые (дельта апгрейда).
            let rate = build_rate_for(unit);
            let base_level = (rate * base.get_attribute(ATTRIB_TURNS)).max(1);
            let actual_level = (rate * actual.get_attribute(ATTRIB_TURNS)).max(1);

            push(
                &mut rows,
                &mut y,
                STAT_COST,
                Some(actual_level),
                vec![StatIcon {
                    resource: "I_GOLD",
                    empty_resource: "I_GOLDE",
                    current: actual_level,
                    original: base_level,
                }],
                false,
            );
        }

        rows
    }

    // -----------------------------------------------------------------------
    // Фиксация (CommitUpgradeChanges + CommitUpgradeChanges override)
    // -----------------------------------------------------------------------

    /// Изменённые `UnitValues` (`*unitvalues_actual != *unitvalues_base`).
    pub fn changed_upgrades(&self) -> Vec<(UnitType, UnitValues)> {
        let mut changed = Vec::new();

        for index in 0..UNIT_END as usize {
            if self.actual[index] == self.base[index] {
                continue;
            }
            let Ok(unit) = UnitType::try_from(index as u16) else {
                continue;
            };
            let mut values = self.actual[index];
            values.update_version();
            values.mark_as_not_in_use();
            changed.push((unit, values));
        }

        changed
    }

    /// `units_built`/`stats_gold_spent_on_upgrades`: `(start_gold - team_gold)`
    /// минус текущие цены купленных юнитов и платного cargo
    /// (`CargoMenu::CommitUpgradeChanges`).
    pub fn gold_spent_on_upgrades(&self) -> u32 {
        let mut spent = self.credits_start.wrapping_sub(self.credits);

        for supply in &self.bought {
            spent = spent.wrapping_sub(self.price(supply.unit_type));
        }

        let total: u32 = self
            .loadout()
            .iter()
            .map(|supply| u32::from(supply.cargo))
            .sum();
        let free: u32 = self.free.iter().map(|supply| u32::from(supply.cargo)).sum();
        spent.wrapping_sub((total.saturating_sub(free)) / 5)
    }

    // -----------------------------------------------------------------------
    // Ввод
    // -----------------------------------------------------------------------

    /// Клик по экрану (те же прямоугольники, что у кнопок C++).
    pub fn handle_click(&mut self, x: i32, y: i32) -> CargoAction {
        self.status.clear();

        if DONE_BUTTON.contains(x, y) {
            return CargoAction::Done;
        }
        if CANCEL_BUTTON.contains(x, y) {
            return CargoAction::Cancel;
        }
        if BUY_BUTTON.contains(x, y) {
            self.buy_selected();
            return CargoAction::None;
        }
        if DELETE_BUTTON.contains(x, y) {
            self.delete_selected();
            return CargoAction::None;
        }
        if HELP_BUTTON.contains(x, y) {
            return CargoAction::Help;
        }
        if DESCRIPTION_BUTTON.contains(x, y) {
            self.description = !self.description;
            return CargoAction::None;
        }
        if TOGGLE_BUTTON.contains(x, y) {
            self.toggle_mode();
            return CargoAction::None;
        }
        for (index, filter) in [
            UpgradeFilter::Ground,
            UpgradeFilter::Air,
            UpgradeFilter::Sea,
            UpgradeFilter::Building,
            UpgradeFilter::Combat,
        ]
        .into_iter()
        .enumerate()
        {
            if filter.rect().contains(x, y) {
                self.toggle_filter(index);
                return CargoAction::None;
            }
        }
        if LIST_UP.contains(x, y) {
            self.scroll(ActiveList::Types, -1);
            return CargoAction::None;
        }
        if LIST_DOWN.contains(x, y) {
            self.scroll(ActiveList::Types, 1);
            return CargoAction::None;
        }
        if PURCHASE_UP.contains(x, y) {
            self.scroll(ActiveList::Loadout, -1);
            return CargoAction::None;
        }
        if PURCHASE_DOWN.contains(x, y) {
            self.scroll(ActiveList::Loadout, 1);
            return CargoAction::None;
        }

        if let Some((row, increase)) = self.control_at(x, y) {
            self.adjust_upgrade(row, increase);
            return CargoAction::None;
        }
        if self.click_cargo(x, y) {
            return CargoAction::None;
        }

        if AVAILABLE_AREA.contains(x, y) {
            let row = ((y - AVAILABLE_AREA.uly) / LIST_ROW) as usize;
            let index = self.type_page + row;
            if index < self.types.len() {
                // Повторный клик по выбранной строке — `Select(1)` →
                // `OnUnitTypeConfirmed` → `BuyUnit`.
                if index == self.type_selected && self.info == self.selected_type() {
                    self.buy(index);
                } else {
                    self.select_type(index);
                }
                return CargoAction::None;
            }
        }

        if PURCHASED_AREA.contains(x, y) {
            let row = ((y - PURCHASED_AREA.uly) / LIST_ROW) as usize;
            let index = self.loadout_page + row;
            if index < self.loadout_rows().len() {
                if index == self.loadout_selected
                    && self.info == self.loadout_row(index).map(|supply| supply.unit_type)
                {
                    self.delete_selected();
                } else {
                    self.select_loadout(index);
                }
                return CargoAction::None;
            }
        }

        CargoAction::None
    }

    /// Клавиатура (`CargoMenu::ProcessKey`/`AbstractUpgradeMenu::ProcessKey`).
    pub fn handle_key(&mut self, key: Key) -> CargoAction {
        match key {
            Key::Escape => CargoAction::Cancel,
            Key::Enter => CargoAction::Done,
            Key::Up => {
                self.step(-1);
                CargoAction::None
            }
            Key::Down => {
                self.step(1);
                CargoAction::None
            }
            Key::PageUp => {
                self.page(-1);
                CargoAction::None
            }
            Key::PageDown => {
                self.page(1);
                CargoAction::None
            }
            Key::Char(character) => {
                match character.to_ascii_lowercase() {
                    'b' => {
                        self.buy_selected();
                    }
                    'd' => {
                        self.delete_selected();
                    }
                    'u' => self.toggle_mode(),
                    // `CargoMenu::ProcessKey`: `GNW_KB_KEY_SHIFT_DIVIDE` (`?`)
                    // и код кнопки Help (1001) открывают `CARGO_SETUP`.
                    '?' => return CargoAction::Help,
                    _ => {}
                }
                CargoAction::None
            }
            _ => CargoAction::None,
        }
    }

    /// Строка апгрейда под точкой (`(row, increase)`).
    pub fn control_at(&self, x: i32, y: i32) -> Option<(usize, bool)> {
        for row in 0..self.controls.len() {
            let (left, right) = self.control_arrows(row)?;
            if left.contains(x, y) {
                return Some((row, false));
            }
            if right.contains(x, y) {
                return Some((row, true));
            }
        }
        None
    }
}

/// Материал полосы cargo (`Select`: `SetMaterialBar(vbar)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoBarKind {
    /// `VERTRAW`.
    Raw,
    /// `VERTFUEL`.
    Fuel,
    /// `VERTGOLD`.
    Gold,
}

// ---------------------------------------------------------------------------
// Формулы (сверены с C++-векторами, tools/cargo_vectors)
// ---------------------------------------------------------------------------

/// Цена юнита — `CargoMenu::BuyUnit`: `Cargo_GetRawConsumptionRate(LANDPLT, 1)`
/// (3) × `ATTRIB_TURNS` **текущих** значений.
pub fn unit_price(values: &[UnitValues; UNIT_END as usize], unit: UnitType) -> u32 {
    let turns = values[unit as usize].get_attribute(ATTRIB_TURNS).max(0) as u32;

    LANDPLT_RAW_CONSUMPTION * turns
}

/// Цена строки списка типов (`PurchaseTypeSelector::Draw`, скобки `if`).
pub fn list_price(values: &[UnitValues; UNIT_END as usize], unit: UnitType) -> u32 {
    let turns = values[unit as usize].get_attribute(ATTRIB_TURNS).max(0) as u32;
    let rate = if !unit.is_mobile() {
        cargo_raw_consumption_rate(UnitType::Constructor, 1) as u32
    } else if unit == UnitType::Infiltrator || unit == UnitType::Infantry {
        cargo_raw_consumption_rate(UnitType::TrainingHall, 1) as u32
    } else {
        LANDPLT_RAW_CONSUMPTION
    };

    rate * turns
}

/// Ёмкость cargo в блоках по 5 (`CargoMenu::Select`): `storage / 5`, у
/// `GOLDTRCK` — 0.
pub fn cargo_capacity(values: &[UnitValues; UNIT_END as usize], unit: UnitType) -> i32 {
    if unit == UnitType::GoldTruck {
        return 0;
    }
    values[unit as usize].get_attribute(ATTRIB_STORAGE) / 5
}

/// Цена покупки для исходных (неапгрейженных) значений — обёртка для тестов и
/// `CargoMenu` без состояния.
pub fn unit_cost(unit: UnitType, clan: TeamClan) -> u32 {
    unit_price(&team_base_values(clan), unit)
}

/// Отображаемое имя типа — User-Friendly Name (`ResourceManager_GetUnit(...).GetSingularName()`).
pub fn unit_label(unit: UnitType) -> &'static str {
    crate::buildmenu::unit_name(unit)
}

/// Имя атрибута `ATTRIB_*` (для статуса).
pub fn attribute_name(attribute: u8) -> &'static str {
    match attribute {
        ATTRIB_ATTACK => "attack",
        ATTRIB_ROUNDS => "rounds",
        ATTRIB_RANGE => "range",
        ATTRIB_AMMO => "ammo",
        ATTRIB_ARMOR => "armor",
        ATTRIB_HITS => "hits",
        ATTRIB_SCAN => "scan",
        ATTRIB_SPEED => "speed",
        ATTRIB_TURNS => "turns",
        ATTRIB_STORAGE => "storage",
        _ => "attribute",
    }
}

/// Полный список модернизации: `AbstractUpgradeMenu::PopulateTeamUnitsList` —
/// `IsUnitFiltered` по всем 93 типам и сортировка `IsBetterUnit`.
pub fn upgrade_types(filters: &[bool; 5]) -> Vec<UnitType> {
    let mut types: Vec<UnitType> = (0..UNIT_END)
        .filter_map(|index| UnitType::try_from(index).ok())
        .filter(|unit| is_unit_filtered(*unit, filters))
        .collect();

    if types.len() > 1 {
        for i in 0..types.len() - 1 {
            for j in i + 1..types.len() {
                if is_better_unit(types[i], types[j]) {
                    types.swap(i, j);
                }
            }
        }
    }

    types
}

/// `AbstractUpgradeMenu::IsUnitFiltered` (`abstractupgrademenu.cpp:699`).
pub fn is_unit_filtered(unit: UnitType, filters: &[bool; 5]) -> bool {
    let flags = unit_type_flags(unit);

    if flags & unit_flags::UPGRADABLE == 0 || !is_buildable(unit) {
        return false;
    }

    // Биты `MOBILE_*` включают амфибий (см. `Environment`).
    let class_matches = ((unit.is_ground() || unit.is_amphibious()) && filters[0])
        || (unit.is_air() && filters[1])
        || ((unit.is_sea() || unit.is_amphibious()) && filters[2])
        || (!unit.is_mobile() && filters[3]);
    if !class_matches {
        return false;
    }

    if filters[4] {
        // C++: `UnitsManager_TeamInfo[0].team_units->GetBaseUnitValues(...)`.
        unit.is_combat()
    } else {
        true
    }
}

/// Строка кланового бонуса (для подсказки на экране мобилизации).
pub fn clan_summary(clan: TeamClan) -> String {
    if clan == TeamClan::Random {
        return "Clan: random".to_string();
    }

    let mut bonuses = Vec::new();

    for unit in [
        UnitType::Scout,
        UnitType::Tank,
        UnitType::Scanner,
        UnitType::Constructor,
        UnitType::Engineer,
    ] {
        let attack = max_core::unit_values::clan_tradeoff(clan, unit, ATTRIB_ATTACK);
        let speed = max_core::unit_values::clan_tradeoff(clan, unit, ATTRIB_SPEED);
        let turns = max_core::unit_values::clan_tradeoff(clan, unit, ATTRIB_TURNS);

        if attack != 0 {
            bonuses.push(format!("{} {:+} atk", unit_label(unit), attack));
        }
        if speed != 0 {
            bonuses.push(format!("{} {:+} spd", unit_label(unit), speed));
        }
        if turns != 0 {
            bonuses.push(format!("{} {:+} turns", unit_label(unit), turns));
        }
    }

    if bonuses.is_empty() {
        format!("Clan: {}", crate::setup::clan_name(clan))
    } else {
        format!(
            "Clan {}: {}",
            crate::setup::clan_name(clan),
            bonuses.join(", ")
        )
    }
}

// ---------------------------------------------------------------------------
// Ресурсы экрана
// ---------------------------------------------------------------------------

/// Иконка типа в списках (`I_*`): корпус и отдельный кадр башни/пушки.
///
/// В C++ `ReportStats_DrawListItemIcon` рисует кадр 0, а для
/// `TURRET_SPRITE`/`SPINNING_TURRET` — ещё кадр 1 со сдвигом
/// `angle_offsets[2] >> divisor` (`divisor` = 1 у мобильных, 2 у стационарных).
#[derive(Debug, Clone)]
pub struct UnitIcon {
    /// Корпус — кадр 0 ресурса `I_*`.
    pub body: SpriteFrame,
    /// Башня/пушка — кадр 1 (у турельных типов).
    pub turret: Option<SpriteFrame>,
    /// `angle_offsets[2]` из `D_*` — сдвиг башни до масштабирования.
    pub turret_offset: (i32, i32),
}

impl UnitIcon {
    /// Есть ли отдельный кадр башни.
    pub fn has_turret(&self) -> bool {
        self.turret.is_some()
    }
}

/// Ресурсы `CargoMenu` из `MAX.RES`.
#[derive(Debug, Clone)]
pub struct CargoMenuAssets {
    /// Фон `CARGOPIC` 640×480.
    pub background: Option<BigImage>,
    /// `Buy` (`CRGBUY_U/D`).
    pub buy: Option<MenuButtonPair>,
    /// `CRGBUY_X` — недоступна.
    pub buy_disabled: Option<SimpleImage>,
    /// `Delete` (`CRGDEL_U/D`).
    pub delete: Option<MenuButtonPair>,
    /// `CRGDEL_X`.
    pub delete_disabled: Option<SimpleImage>,
    /// `Cancel` (`BLDCAN_U/D`).
    pub cancel: Option<MenuButtonPair>,
    /// `Done` (`BLDONE_U/D`).
    pub done: Option<MenuButtonPair>,
    /// `Help` (`BLDHLP_U/D`).
    pub help: Option<MenuButtonPair>,
    /// Переключатель (`CRGTOG_U/D`).
    pub toggle: Option<MenuButtonPair>,
    /// Описание (`BLDDES_U/D`).
    pub description: Option<MenuButtonPair>,
    /// Скролл вверх (`BLDUP__U/D`).
    pub scroll_up: Option<MenuButtonPair>,
    /// `BLDUP__X`.
    pub scroll_up_disabled: Option<SimpleImage>,
    /// Скролл вниз (`BLDDWN_U/D`).
    pub scroll_down: Option<MenuButtonPair>,
    /// `BLDDWN_X`.
    pub scroll_down_disabled: Option<SimpleImage>,
    /// Стрелка апгрейда влево (`UPGLFT_U/D`).
    pub left: Option<SimpleImage>,
    /// `UPGLFT_X`.
    pub left_disabled: Option<SimpleImage>,
    /// Стрелка апгрейда вправо (`UPGRGT_U/D`).
    pub right: Option<SimpleImage>,
    /// `UPGRGT_X`.
    pub right_disabled: Option<SimpleImage>,
    /// Иконка золота `I_GOLD` (колонка цен апгрейдов).
    pub gold_icon: Option<SimpleImage>,
    /// Недоступный фильтр (`CRGSELDX`).
    pub filter_disabled: Option<SimpleImage>,
    /// Фильтры домена (`SELGRD`/`SELAIR`/`SELSEA`/`SELBLD`/`SELCBT`).
    pub filters: [Option<MenuButtonPair>; 5],
    /// Полосы `VERTRAW`/`VERTFUEL`/`VERTGOLD` (индекс — [`CargoBarKind`]).
    pub bars: [Option<SimpleImage>; 3],
    /// Иконки типов (`I_*`): корпус (кадр 0) и башня (кадр 1).
    pub icons: Vec<(UnitType, UnitIcon)>,
    /// Иконки строк статистики (`I_*` simple images, `UnitStats_DrawStats`).
    pub stat_icons: Vec<(&'static str, SimpleImage)>,
    /// Кэш портретов `P_*` (`Unit::GetPortrait`, big image 300×240).
    ///
    /// Грузится лениво при выборе типа: 71 big image в старте не нужен.
    /// `Arc<Mutex<…>>` — `CargoMenuAssets` остаётся `Clone`/`Sync`.
    pub portraits: Arc<Mutex<HashMap<UnitType, Option<Arc<BigImage>>>>>,
}

impl CargoMenuAssets {
    /// Грузит ресурсы; отсутствие части — не ошибка (кнопка рисуется рамкой).
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<SimpleImage> {
            archive
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };
        let icon = |unit: UnitType, name: &str| -> Option<(UnitType, UnitIcon)> {
            let bytes = archive.read(name).ok()?;
            let sheet = decode_sprite(bytes).ok()?;
            let body = sheet.frame(0)?.clone();
            let turret = sheet.frame(1).cloned();
            // `ReportStats_DrawListItemIcon`: сдвиг башни — `angle_offsets[2]`.
            let turret_offset = cargo_unit_data::unit_frame_data(unit)
                .and_then(|data| archive.read(data).ok())
                .and_then(|bytes| max_assets::sprite::FrameInfo::decode(&bytes))
                .map(|info| info.turret_offset(2))
                .unwrap_or((0, 0));

            Some((
                unit,
                UnitIcon {
                    body,
                    turret,
                    turret_offset,
                },
            ))
        };

        let mut icons = Vec::new();
        for (unit, name) in UNIT_ICONS {
            if let Some(entry) = icon(unit, name) {
                icons.push(entry);
            }
        }

        let mut stat_icons = Vec::new();
        for name in STAT_ICONS {
            if let Some(image) = simple(name) {
                stat_icons.push((name, image));
            }
        }

        Self {
            background: archive
                .read("CARGOPIC")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            buy: pair("CRGBUY_U", "CRGBUY_D"),
            buy_disabled: simple("CRGBUY_X"),
            delete: pair("CRGDEL_U", "CRGDEL_D"),
            delete_disabled: simple("CRGDEL_X"),
            cancel: pair("BLDCAN_U", "BLDCAN_D"),
            done: pair("BLDONE_U", "BLDONE_D"),
            help: pair("BLDHLP_U", "BLDHLP_D"),
            toggle: pair("CRGTOG_U", "CRGTOG_D"),
            description: pair("BLDDES_U", "BLDDES_D"),
            scroll_up: pair("BLDUP__U", "BLDUP__D"),
            scroll_up_disabled: simple("BLDUP__X"),
            scroll_down: pair("BLDDWN_U", "BLDDWN_D"),
            scroll_down_disabled: simple("BLDDWN_X"),
            left: simple("UPGLFT_U"),
            left_disabled: simple("UPGLFT_X"),
            right: simple("UPGRGT_U"),
            right_disabled: simple("UPGRGT_X"),
            gold_icon: simple("I_GOLD"),
            filter_disabled: simple("CRGSELDX"),
            filters: [
                pair("SELGRD_U", "SELGRD_D"),
                pair("SELAIR_U", "SELAIR_D"),
                pair("SELSEA_U", "SELSEA_D"),
                pair("SELBLD_U", "SELBLD_D"),
                pair("SELCBT_U", "SELCBT_D"),
            ],
            bars: [simple("VERTRAW"), simple("VERTFUEL"), simple("VERTGOLD")],
            icons,
            stat_icons,
            portraits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Иконка типа (`I_*`, кадр 0).
    pub fn icon(&self, unit: UnitType) -> Option<&UnitIcon> {
        self.icons
            .iter()
            .find(|(candidate, _)| *candidate == unit)
            .map(|(_, icon)| icon)
    }

    /// Иконка строки статистики (`I_*` simple image).
    pub fn stat_icon(&self, resource: &str) -> Option<&SimpleImage> {
        self.stat_icons
            .iter()
            .find(|(name, _)| *name == resource)
            .map(|(_, image)| image)
    }

    /// Портрет типа (`Unit::GetPortrait()` → big image `P_*` 300×240).
    ///
    /// Лениво читает и декодирует ресурс, кэширует результат (включая
    /// отсутствие ресурса). Возвращает `Arc`, чтобы не копировать 72 КиБ кадра
    /// на каждый вызов отрисовки.
    pub fn portrait(&self, archive: &ResArchive, unit: UnitType) -> Option<Arc<BigImage>> {
        let mut cache = self
            .portraits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(entry) = cache.get(&unit) {
            return entry.clone();
        }

        let image = cargo_unit_data::unit_portrait(unit)
            .and_then(|name| archive.read(name).ok())
            .and_then(|bytes| BigImage::decode(bytes).ok())
            .map(Arc::new);

        cache.insert(unit, image.clone());
        image
    }

    /// Есть ли фон и ключевые кнопки.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.done.is_some() && self.cancel.is_some()
    }
}

/// Иконки строк `UnitStats_DrawStats`: атака/выстрелы/дальность/боезапас,
/// груз, расход, броня/прочность/обзор/скорость и золото цены.
pub const STAT_ICONS: [&str; 14] = [
    "I_HRDATK", "I_SHOTS", "I_RANGE", "I_AMMO", "I_RAW", "I_FUEL", "I_GOLD", "I_GOLDE", "I_POWER",
    "I_LIFE", "I_ARMOR", "I_HITS", "I_SCAN", "I_SPEED",
];

/// `Unit::GetIcon()` — ресурсы `I_*` из `SC_UNITS` (units.json), в порядке
/// [`UnitType`]; типы без иконки (`INVALID_ID`) пропущены.
pub const UNIT_ICONS: [(UnitType, &str); 71] = [
    (UnitType::GoldRefinery, "I_TRANSP"),
    (UnitType::PowerStation, "I_POWSTN"),
    (UnitType::PowerGenerator, "I_POWGEN"),
    (UnitType::Barracks, "I_BARRCK"),
    (UnitType::ShieldGenerator, "I_SHIELD"),
    (UnitType::Radar, "I_RADAR"),
    (UnitType::MaterialStorage, "I_SMSTOR"),
    (UnitType::FuelStorage, "I_SMFUEL"),
    (UnitType::GoldVault, "I_SMVLT"),
    (UnitType::Depot, "I_DEPOT"),
    (UnitType::Hangar, "I_HANGAR"),
    (UnitType::Dock, "I_DOCK"),
    (UnitType::Connector, "I_CONNEC"),
    (UnitType::Road, "I_ROAD"),
    (UnitType::LandingPad, "I_LANDPD"),
    (UnitType::Shipyard, "I_SHIPYD"),
    (UnitType::LightVehiclePlant, "I_LGHTPL"),
    (UnitType::HeavyVehiclePlant, "I_HVYPLT"),
    (UnitType::SupportPlant, "I_LIFESP"),
    (UnitType::AirUnitsPlant, "I_AIRPLT"),
    (UnitType::Habitat, "I_HABITA"),
    (UnitType::ResearchCentre, "I_RESEAR"),
    (UnitType::EcoSphere, "I_GREENH"),
    (UnitType::RecreationCenter, "I_RECCTR"),
    (UnitType::TrainingHall, "I_TRNHLL"),
    (UnitType::WaterPlatform, "I_WATER"),
    (UnitType::GunTurret, "I_GUNTUR"),
    (UnitType::AntiAir, "I_FXAA"),
    (UnitType::Artillery, "I_ARTYTR"),
    (UnitType::MissileLauncher, "I_FXROCK"),
    (UnitType::ConcreteBlock, "I_BLOCK"),
    (UnitType::Bridge, "I_BRIDGE"),
    (UnitType::MiningStation, "I_MINING"),
    (UnitType::LandMine, "I_LANDMN"),
    (UnitType::SeaMine, "I_SEAMIN"),
    (UnitType::MasterBuilder, "I_MASTER"),
    (UnitType::Constructor, "I_CONTRC"),
    (UnitType::Scout, "I_SCOUT"),
    (UnitType::Tank, "I_TANK"),
    (UnitType::AssaultGun, "I_ARTY"),
    (UnitType::RocketLauncher, "I_ROCKET"),
    (UnitType::MissileCrawler, "I_MISSIL"),
    (UnitType::MobileAntiAir, "I_AA"),
    (UnitType::MineLayer, "I_MNELAY"),
    (UnitType::Surveyor, "I_SURVEY"),
    (UnitType::Scanner, "I_SCANNR"),
    (UnitType::SupplyTruck, "I_SPLYTR"),
    (UnitType::GoldTruck, "I_GOLDTR"),
    (UnitType::Engineer, "I_ENGINR"),
    (UnitType::Bulldozer, "I_BULLDZ"),
    (UnitType::RepairUnit, "I_REPAIR"),
    (UnitType::FuelTruck, "I_FUELTR"),
    (UnitType::ArmouredPersonnelCarrier, "I_COLNST"),
    (UnitType::Infiltrator, "I_COMMAN"),
    (UnitType::Infantry, "I_INFANT"),
    (UnitType::Escort, "I_ESCORT"),
    (UnitType::Corvette, "I_CORVET"),
    (UnitType::Gunboat, "I_GUNBT"),
    (UnitType::Submarine, "I_SUB"),
    (UnitType::SeaTransport, "I_SEATRN"),
    (UnitType::MissileCruiser, "I_MSLCR"),
    (UnitType::SeaMineLayer, "I_SEAMNL"),
    (UnitType::CargoShip, "I_CARGOS"),
    (UnitType::Fighter, "I_FIGHTR"),
    (UnitType::Bomber, "I_BOMBER"),
    (UnitType::AirTransport, "I_AIRTRN"),
    (UnitType::Awac, "I_AWAC"),
    (UnitType::Juggernaut, "I_JUGGER"),
    (UnitType::AlienTank, "I_ALNTAN"),
    (UnitType::AlienAssaultGun, "I_ALNASG"),
    (UnitType::AlienPlane, "I_ALNPLA"),
];

// ---------------------------------------------------------------------------
// Отрисовка
// ---------------------------------------------------------------------------

impl Mobilization {
    /// Рисует экран (1:1 координаты `cargomenu.cpp`) в кадр 640×480.
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        assets: &Assets,
        status: &str,
        mouse: Option<(i32, i32)>,
    ) -> u32 {
        if frame.width() != SCREEN_WIDTH as u32 || frame.height() != SCREEN_HEIGHT as u32 {
            return 0;
        }
        let Some(cargo) = assets.cargo.as_ref() else {
            return 0;
        };
        let Some(background) = cargo.background.as_ref() else {
            return 0;
        };

        let palette = UiPalette::from_big_image(background);
        let mut painted = draw_big_image(frame, background);

        let font = GnwFontId::Font5.font();
        let green = palette.rgba(crate::setup::GREEN);
        let text = palette.rgba(crate::setup::TEXT);
        let yellow = palette.rgba(0x04);
        let chrome = palette.rgba(0x05);
        let gold_font = palette.rgba(165);

        // Заголовки и подписи.
        painted += draw_text_gnw_centered(frame, TITLE, font, "Purchase Menu", green).max(0) as u32;
        painted += draw_text_gnw(
            frame,
            AVAILABLE_HEADER.ulx,
            center_y(AVAILABLE_HEADER, font),
            font,
            "Available",
            text,
        )
        .max(0) as u32;
        painted += draw_text_right(
            frame,
            PRICE_COLUMN.lrx(),
            center_y(AVAILABLE_HEADER, font),
            font,
            "Cost",
            yellow,
        );
        draw_hline(frame, palette.rgba(0x05), 482, 625, AVAILABLE_LINE_Y);

        painted +=
            draw_text_gnw_centered(frame, PURCHASED_HEADER, font, "Purchased", text).max(0) as u32;
        draw_hline(frame, chrome, 337, 455, PURCHASED_LINE_Y);

        painted += draw_text_gnw(
            frame,
            DESCRIPTION_LABEL.ulx + 1,
            center_y(DESCRIPTION_LABEL, font),
            font,
            "Description",
            gold_font,
        )
        .max(0) as u32;
        for (rect, label) in [
            (COST_LABEL, "Cost"),
            (CREDIT_LABEL, "Credit"),
            (CARGO_LABEL, "Cargo"),
        ] {
            painted += draw_text_gnw_centered(frame, rect, font, label, gold_font).max(0) as u32;
        }

        // Значения Cost/Credit/Cargo в C++-порте не рисуются (остались только
        // подписи); счётчик кредитов — число на полосе золота (`gold_background`),
        // счётчик cargo — над слайдером. Добавлять числа над подписями нельзя:
        // они попадают на полосу золота (361,276) и счётчик cargo (421,276).

        // Портрет/статистика выбранного типа.
        painted += self.draw_info(frame, assets, cargo, &palette);

        // Списки.
        painted += self.draw_type_list(frame, cargo, &palette);
        painted += self.draw_loadout_list(frame, cargo, &palette);

        // Колонка апгрейдов: стрелки + цены.
        painted += self.draw_controls(frame, cargo, &palette);

        // Cargo-слайдер и полоса золота.
        painted += self.draw_gold_bar(frame, cargo, &palette);
        painted += self.draw_cargo_bar(frame, cargo, &palette);

        // Кнопки.
        painted += draw_button(
            frame,
            &palette,
            cargo.buy.as_ref(),
            cargo.buy_disabled.as_ref(),
            BUY_BUTTON,
            "Buy",
            mouse,
            self.can_buy(),
        );
        painted += draw_button(
            frame,
            &palette,
            cargo.delete.as_ref(),
            cargo.delete_disabled.as_ref(),
            DELETE_BUTTON,
            "Delete",
            mouse,
            self.can_delete(),
        );
        painted += draw_button(
            frame,
            &palette,
            cargo.cancel.as_ref(),
            None,
            CANCEL_BUTTON,
            "Cancel",
            mouse,
            true,
        );
        painted += draw_button(
            frame,
            &palette,
            cargo.done.as_ref(),
            None,
            DONE_BUTTON,
            "Done",
            mouse,
            true,
        );
        painted += draw_button(
            frame,
            &palette,
            cargo.help.as_ref(),
            None,
            HELP_BUTTON,
            "",
            mouse,
            true,
        );
        painted += draw_filter_button(
            frame,
            cargo.description.as_ref(),
            &palette,
            DESCRIPTION_BUTTON,
            self.description,
        );

        // Переключатель режима: обе подписи, цвет зависит от состояния
        // (`SetCaption(834a, 20,0,80,15, DarkOrange, Gold)` и
        // `SetCaption(e02a, 20,15,80,31)`).
        painted += self.draw_toggle(frame, cargo, &palette);

        // Фильтры домена.
        for (index, filter) in [
            UpgradeFilter::Ground,
            UpgradeFilter::Air,
            UpgradeFilter::Sea,
            UpgradeFilter::Building,
            UpgradeFilter::Combat,
        ]
        .into_iter()
        .enumerate()
        {
            painted += draw_filter(
                frame,
                cargo,
                &palette,
                filter,
                index,
                self.filters[index],
                self.mode,
            );
        }

        // Стрелки списков.
        painted += draw_scroll(
            frame,
            cargo.scroll_up.as_ref(),
            cargo.scroll_up_disabled.as_ref(),
            &palette,
            LIST_UP,
            self.type_page > 0,
            mouse,
        );
        painted += draw_scroll(
            frame,
            cargo.scroll_down.as_ref(),
            cargo.scroll_down_disabled.as_ref(),
            &palette,
            LIST_DOWN,
            self.type_page + AVAILABLE_ROWS < self.types.len(),
            mouse,
        );
        painted += draw_scroll(
            frame,
            cargo.scroll_up.as_ref(),
            cargo.scroll_up_disabled.as_ref(),
            &palette,
            PURCHASE_UP,
            self.loadout_page > 0,
            mouse,
        );
        painted += draw_scroll(
            frame,
            cargo.scroll_down.as_ref(),
            cargo.scroll_down_disabled.as_ref(),
            &palette,
            PURCHASE_DOWN,
            self.loadout_page + PURCHASED_ROWS < self.loadout_rows().len(),
            mouse,
        );

        // Клановые бонусы (наше дополнение) и строка статуса.
        let clan_line = clan_summary(self.clan);
        painted += draw_text_gnw(frame, 11, 466, font, &clan_line, green).max(0) as u32;

        let line = if status.is_empty() {
            self.status.as_str()
        } else {
            status
        };
        painted += draw_text_gnw(
            frame,
            11,
            451,
            font,
            &line.chars().take(44).collect::<String>(),
            text,
        )
        .max(0) as u32;

        painted
    }

    /// Портрет + описание + таблица статистики (`AbstractUpgradeMenu::DrawUnitInfo`).
    fn draw_info(
        &self,
        frame: &mut RgbaBuffer,
        assets: &Assets,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let Some(unit) = self.info else {
            return 0;
        };
        let mut painted = 0;

        // Портрет: `WindowManager_LoadBigImage(GetPortrait(), ...)` — big image
        // `P_*` 300×240, `ulx/uly` = 0, масштаб 1:1, палитра окна (`CARGOPIC`).
        // Окно `unit_portrait_window` 280×240 обрезает правые 20 px.
        if let Some(portrait) = cargo.portrait(&assets.max_res, unit) {
            painted += draw_big_image_at(
                frame,
                &portrait,
                palette,
                (PORTRAIT.ulx, PORTRAIT.uly),
                PORTRAIT,
            );
        } else {
            // C++ `buf_fill(unit_portrait_window.buffer, 300, 240, width, 0)`.
            frame.fill_rect(PORTRAIT, palette.rgba(STAT_COLOR_OUTLINE));
        }

        // Описание — поверх портрета, `button_description_rest_state`.
        if self.description {
            painted += draw_description(frame, palette, unit);
        }

        painted += draw_stats(frame, cargo, palette, &self.stats_rows());

        painted
    }

    /// Список «Available» с иконками и ценами.
    fn draw_type_list(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let text = palette.rgba(crate::setup::TEXT);
        let yellow = palette.rgba(0x04);
        let white = palette.rgba(0xFF);
        let lut = team_lut(self.team);
        let mut painted = 0;

        for row in 0..AVAILABLE_ROWS {
            let index = self.type_page + row;
            let Some(&unit) = self.types.get(index) else {
                break;
            };
            let y = AVAILABLE_AREA.uly + row as i32 * LIST_ROW;

            painted += draw_text_gnw(
                frame,
                AVAILABLE_AREA.ulx + 35,
                center_y(PixelRect::new(0, y, 0, LIST_ROW), font),
                font,
                &fit_label(unit_label(unit), 88, font),
                text,
            )
            .max(0) as u32;
            painted += draw_text_right(
                frame,
                PRICE_COLUMN.lrx(),
                center_y(PixelRect::new(0, y, 0, LIST_ROW), font),
                font,
                &self.list_price(unit).to_string(),
                yellow,
            );

            if let Some(icon) = cargo.icon(unit) {
                let stationary = !unit.is_mobile();
                let zoom = if stationary { 16 } else { 32 };
                painted += draw_icon(
                    frame,
                    palette,
                    unit,
                    icon,
                    (AVAILABLE_AREA.ulx + 16, y + 16),
                    zoom,
                    &lut,
                );
            }

            if index == self.type_selected {
                painted += draw_selection(
                    frame,
                    white,
                    AVAILABLE_AREA.ulx,
                    y,
                    AVAILABLE_AREA.width,
                    LIST_ROW,
                    1,
                );
            }
        }

        painted
    }

    /// Список «Purchased» (`CargoSelector::Draw`).
    fn draw_loadout_list(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let text = palette.rgba(crate::setup::TEXT);
        let white = palette.rgba(0xFF);
        let lut = team_lut(self.team);
        let rows = self.loadout_rows();
        let mut painted = 0;

        for row in 0..PURCHASED_ROWS {
            let index = self.loadout_page + row;
            let Some(supply) = rows.get(index).copied() else {
                break;
            };
            let y = PURCHASED_AREA.uly + row as i32 * LIST_ROW;
            let mut count = 1;
            for earlier in rows.iter().take(index) {
                if earlier.unit_type == supply.unit_type {
                    count += 1;
                }
            }
            let mut label = format!("{} {count}", unit_label(supply.unit_type));
            let storage = self.actual[supply.unit_type as usize].get_attribute(ATTRIB_STORAGE);
            if storage > 0 {
                if supply.cargo == 0 {
                    label.push_str(" (Empty)");
                } else if i32::from(supply.cargo) < storage {
                    label.push_str(&match supply.unit_type {
                        UnitType::FuelTruck => format!(" ({} Fuel)", supply.cargo),
                        UnitType::GoldTruck => format!(" ({} Gold)", supply.cargo),
                        _ => format!(" ({} Mat.)", supply.cargo),
                    });
                } else {
                    label.push_str(" (Full)");
                }
            }

            painted += draw_wrapped(
                frame,
                PixelRect::new(
                    PURCHASED_AREA.ulx + 32,
                    y,
                    PURCHASED_AREA.width - 32,
                    LIST_ROW,
                ),
                font,
                &label,
                text,
                3,
            );

            if let Some(icon) = cargo.icon(supply.unit_type) {
                let stationary = !supply.unit_type.is_mobile();
                let zoom = if stationary { 16 } else { 32 };
                painted += draw_icon(
                    frame,
                    palette,
                    supply.unit_type,
                    icon,
                    (PURCHASED_AREA.ulx + 16, y + 16),
                    zoom,
                    &lut,
                );
            }

            if index == self.loadout_selected {
                painted += draw_selection(
                    frame,
                    white,
                    PURCHASED_AREA.ulx,
                    y,
                    PURCHASED_AREA.width,
                    LIST_ROW,
                    2,
                );
            }
        }

        painted
    }

    /// Колонка апгрейдов: стрелки и цены (`AbstractUpgradeMenu::DrawUnitStats`).
    fn draw_controls(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let chrome = palette.rgba(0x05);
        let mut painted = 0;

        for row in 0..self.controls.len() {
            let (left, right) = match self.control_arrows(row) {
                Some(rects) => rects,
                None => break,
            };
            let quote = self.quote(row);
            let can_up = quote
                .as_ref()
                .is_some_and(|quote| quote.can_increase(self.credits));
            let can_down = quote.as_ref().is_some_and(UpgradeQuote::can_decrease);

            painted += draw_upgrade_arrow(
                frame,
                palette,
                cargo.left.as_ref(),
                cargo.left_disabled.as_ref(),
                left,
                can_down,
            );
            painted += draw_upgrade_arrow(
                frame,
                palette,
                cargo.right.as_ref(),
                cargo.right_disabled.as_ref(),
                right,
                can_up,
            );

            if let Some(quote) = quote {
                let cost = quote.cost();
                if cost > 0 && cost < UPGRADE_COST_LIMIT {
                    // `ReportStats_DrawNumber` в `cost_background`: число
                    // выровнено по правому краю x=345, иконка — x=346.
                    painted += draw_text_right(
                        frame,
                        COST.ulx + 24,
                        left.uly + 5,
                        font,
                        &cost.to_string(),
                        chrome,
                    );
                    if let Some(icon) = cargo.gold_icon.as_ref() {
                        painted +=
                            blit_simple(frame, palette, icon, COST.ulx + 25, left.uly + 3, true);
                    }
                }
            }
        }

        painted
    }

    /// Полоса золота (`gold_background` + `VERTGOLD`).
    fn draw_gold_bar(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let mut painted = 0;
        let inner = GOLD_BAR.height - 25;
        let fill = if self.credits_start > 0 {
            ((i64::from(inner) * i64::from(self.credits)) / i64::from(self.credits_start))
                .clamp(0, i64::from(inner)) as i32
        } else {
            0
        };

        if let Some(bar) = cargo.bars[2].as_ref() {
            painted += draw_vertical_bar(
                frame,
                palette,
                bar,
                GOLD_BAR.ulx,
                GOLD_BAR.uly + 25,
                GOLD_BAR.width,
                inner,
                fill,
            );
        }

        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(GOLD_BAR.ulx, GOLD_BAR.uly, GOLD_BAR.width, font.height()),
            font,
            &self.credits.to_string(),
            palette.rgba(0x05),
        )
        .max(0) as u32;

        painted
    }

    /// Cargo-слайдер (`LimitedScrollbar::RefreshScreen`).
    fn draw_cargo_bar(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let capacity = self.cargo_capacity();
        let value = self.cargo_value();
        let fill = if capacity != 0 {
            (value * CARGO_BAR.height) / capacity
        } else {
            0
        }
        .clamp(0, CARGO_BAR.height);
        let kind = self.cargo_bar_kind();
        let mut painted = 0;

        if let Some(bar) = cargo.bars[kind as usize].as_ref() {
            painted += draw_vertical_bar(
                frame,
                palette,
                bar,
                CARGO_BAR.ulx,
                CARGO_BAR.uly,
                CARGO_BAR.width,
                CARGO_BAR.height,
                fill,
            );
        }

        painted += draw_text_gnw_centered(
            frame,
            CARGO_AMOUNT,
            font,
            &(value * 5).to_string(),
            palette.rgba(0xFF),
        )
        .max(0) as u32;

        painted
    }

    /// Переключатель `CRGTOG`: обе подписи, цвета — как `SetCaption` C++.
    fn draw_toggle(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let down = self.mode == CargoMode::Upgrade;
        let mut painted = 0;

        if let Some(pair) = cargo.toggle.as_ref() {
            painted += blit_simple(
                frame,
                palette,
                pair.image(down),
                TOGGLE_BUTTON.ulx,
                TOGGLE_BUTTON.uly,
                true,
            );
        }

        // up: Buy Units — DarkOrange (5), Upgrades — Gold (165);
        // down: наоборот.
        let (buy_color, upgrade_color) = if down {
            (palette.rgba(165), palette.rgba(0x05))
        } else {
            (palette.rgba(0x05), palette.rgba(165))
        };
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(TOGGLE_BUTTON.ulx + 20, TOGGLE_BUTTON.uly, 80, 15),
            font,
            "Buy Units",
            buy_color,
        )
        .max(0) as u32;
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(TOGGLE_BUTTON.ulx + 20, TOGGLE_BUTTON.uly + 15, 80, 16),
            font,
            "Upgrades",
            upgrade_color,
        )
        .max(0) as u32;

        painted
    }
}

// ---------------------------------------------------------------------------
// Помощники отрисовки
// ---------------------------------------------------------------------------

/// Цвет команды (LUT перекраски командных слотов).
pub(crate) fn team_lut(team: TeamId) -> ColorLut {
    ColorLut::for_team(
        TeamColors::ALL
            .get(team.0 as usize)
            .copied()
            .unwrap_or(TeamColors::Derelict),
    )
}

/// `Cargo_GetRawConsumptionRate(Builder_GetBuilderType(team, unit), 1)` —
/// ставка строки `Cost` (`UnitStats_DrawCost`); см. [`Mobilization::build_rate`].
pub(crate) fn build_rate_for(unit: UnitType) -> i32 {
    builder_type(unit).map_or(0, |builder| cargo_raw_consumption_rate(builder, 1))
}

/// Вертикальная координата однострочного текста по центру прямоугольника
/// (`Text_TextBox`: `((height + 1) - font_height) / 2`).
pub(crate) fn center_y(rect: PixelRect, font: &max_render::text::GnwFont) -> i32 {
    rect.uly + ((rect.height + 1) - font.height()).max(0) / 2
}

/// Горизонтальная линия (C++ `draw_line`).
fn draw_hline(frame: &mut RgbaBuffer, color: Rgba, x1: i32, x2: i32, y: i32) {
    for x in x1..=x2 {
        frame.set(x, y, color);
    }
}

/// Текст, выровненный по правому краю `right` (`ReportStats_DrawText`).
pub(crate) fn draw_text_right(
    frame: &mut RgbaBuffer,
    right: i32,
    y: i32,
    font: &max_render::text::GnwFont,
    text: &str,
    color: Rgba,
) -> u32 {
    let width = font.text_width(text);
    draw_text_gnw(frame, right - width, y, font, text, color).max(0) as u32
}

/// Обрезает строку по ширине как `Text_Blit` (`swidth`): глиф рисуется, пока
/// его конец не вышел за границу.
fn clip_gnw_text<'a>(text: &'a str, max_width: i32, font: &GnwFont) -> &'a str {
    let mut width = 0;

    for (index, character) in text.char_indices() {
        width += font.char_width(character);

        if width > max_width {
            return &text[..index];
        }
    }

    text
}

/// `ReportStats_DrawNumber`: `> 1e6` → `%.1fM`, `> 1000` → `%.1fk`, иначе
/// целое (`%lld`).
fn report_stats_number(value: i32) -> String {
    let number = i64::from(value);

    if number > 1_000_000 {
        format!("{:.1}M", number as f64 / 1_000_000.0)
    } else if number > 1_000 {
        format!("{:.1}k", number as f64 / 1_000.0)
    } else {
        number.to_string()
    }
}

/// Описание юнита поверх портрета.
///
/// `Text_TextBox(unit_portrait_window.buffer, dialog_window.width, description,
/// 16, 17, 280, 230, GNW_TEXT_OUTLINE | 0xA2, false, false)`:
/// шрифт 5, межстрочный интервал `Text_GetHeight + 1` (обводка), 20 строк,
/// чёрная копия в `(x+1, y+1)` (`Text_BlitTTF` + `GNW_TEXT_OUTLINE`).
pub(crate) fn draw_description(frame: &mut RgbaBuffer, palette: &UiPalette, unit: UnitType) -> u32 {
    let text = cargo_unit_data::unit_description(unit);

    if text.is_empty() {
        return 0;
    }

    let font = GnwFontId::Font5.font();
    let color = palette.rgba(STAT_COLOR_LABEL);
    let outline = palette.rgba(STAT_COLOR_OUTLINE);
    let line_height = font.height() + 1;
    let max_rows = (DESCRIPTION.height / line_height).max(0) as usize;
    let lines = wrap_gnw_text(text, DESCRIPTION.width);
    let mut painted = 0;

    for (row, line) in lines.iter().take(max_rows).enumerate() {
        let y = DESCRIPTION.uly + row as i32 * line_height;

        painted +=
            draw_text_gnw(frame, DESCRIPTION.ulx + 1, y + 1, font, line, outline).max(0) as u32;
        painted += draw_text_gnw(frame, DESCRIPTION.ulx, y, font, line, color).max(0) as u32;
    }

    painted
}

/// Таблица статистики (`UnitStats_DrawStats`): строки по 19 px, число справа
/// от x=24, подпись с x=28 (клип 49 px), pips с x=76, красная линия под строкой.
pub(crate) fn draw_stats(
    frame: &mut RgbaBuffer,
    cargo: &CargoMenuAssets,
    palette: &UiPalette,
    rows: &[StatRow],
) -> u32 {
    let font = GnwFontId::Font5.font();
    let number_color = palette.rgba(STAT_COLOR_NUMBER);
    let label_color = palette.rgba(STAT_COLOR_LABEL);
    let line_color = palette.rgba(STAT_COLOR_LINE);
    let mut painted = 0;

    for row in rows {
        // `UnitStats_DrawText`: `((19 - Text_GetHeight()) / 2)`.
        let text_y = row.y + (STAT_ROW - font.height()).max(0) / 2;

        if let Some(value) = row.value {
            let text = report_stats_number(value);
            painted += draw_text_right(frame, STATS.ulx + 24, text_y, font, &text, number_color);
        }

        let label = clip_gnw_text(row.label, STAT_LABEL_WIDTH, font);
        painted +=
            draw_text_gnw(frame, STATS.ulx + 28, text_y, font, label, label_color).max(0) as u32;

        // `draw_line(buffer, sw, 2, 18, line_length - 2, 18, COLOR_RED)`.
        if row.separator {
            draw_hline(
                frame,
                line_color,
                STATS.ulx + 2,
                STATS.ulx + STATS.width - 2,
                row.y + 18,
            );
        }

        let mut offset = STAT_ICON_X;
        for icons in &row.icons {
            offset += draw_stat_icons(
                frame,
                palette,
                cargo,
                STATS.ulx + offset,
                row.y,
                STATS.width - offset,
                icons,
            );
        }
    }

    painted
}

/// Одна группа pips — `UnitStats_DrawIcons`: масштаб шага под ширину, дельта
/// апгрейда (`original - current`) рисуется пустыми иконками за красной чертой.
#[allow(clippy::too_many_arguments)]
fn draw_stat_icons(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    cargo: &CargoMenuAssets,
    base_x: i32,
    row_y: i32,
    max_width: i32,
    icons: &StatIcon,
) -> i32 {
    let Some(normal) = cargo.stat_icon(icons.resource) else {
        return 0;
    };
    let empty = cargo.stat_icon(icons.empty_resource).unwrap_or(normal);

    // `original -= current` — дельта апгрейда (может быть отрицательной).
    let original = icons.original - icons.current;
    let mut max_width = max_width;

    if original != 0 {
        max_width -= 5;
    }

    let image_width = i32::from(normal.width);
    let image_height = i32::from(normal.height);
    let row_step = if icons.current >= 6 {
        (18 - image_height) / 4
    } else {
        0
    };
    // `address = &buffer[(((18 - (height * 4)) - image_height) / 2) * sw]`.
    let top = row_y + ((18 - row_step * 4) - image_height) / 2;

    let current_scaled = (icons.current + 4) / 5;
    let current_offset = icons.current - (current_scaled - 1) * 5;
    let original_scaled = (original + 4) / 5;
    let original_offset = original - (original_scaled - 1) * 5;
    let mut full_scaled = current_scaled + original_scaled;

    max_width -= image_width * full_scaled;

    // `var_2C` — суммарная ширина шагов, которая осталась бы при полном
    // масштабе иконки.
    let mut full_step_width = full_scaled * 4 - (5 - current_offset);

    if original_scaled != 0 {
        full_step_width -= 5 - original_offset;
    }

    let (spacing, shrink) = if full_step_width != 0 {
        if max_width < full_step_width {
            if original_scaled != 0 {
                full_scaled -= 1;
            }

            let denominator = full_scaled - 1;
            let shrink = if denominator > 0 {
                (full_scaled + (full_step_width - max_width) - 2) / denominator
            } else {
                0
            };

            (1, shrink.max(0))
        } else {
            ((max_width / full_step_width).min(image_width), 0)
        }
    } else {
        (image_width, 0)
    };

    let stride = spacing * 4 + image_width - shrink;
    let mut advance = draw_stat_pips(
        frame,
        palette,
        base_x,
        top,
        normal,
        icons.current,
        spacing,
        stride,
        row_step,
    );

    if original != 0 {
        // `draw_line(&buffer[var_24 + 2], sw, 0, 2, 0, 16, COLOR_RED)` —
        // вертикальная черта на всю высоту строки.
        let x = base_x + advance + 2;

        for y in (row_y + 2)..=(row_y + 16) {
            frame.set(x, y, palette.rgba(STAT_COLOR_LINE));
        }

        advance += 5;
        advance += draw_stat_pips(
            frame,
            palette,
            base_x + advance,
            top,
            empty,
            original,
            spacing,
            stride,
            row_step,
        );
    }

    advance
}

/// `UnitStats_DrawIcon`: до `count` иконок блоками по 5, со сдвигом
/// `reminder * row_step` по вертикали.
#[allow(clippy::too_many_arguments)]
fn draw_stat_pips(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    base_x: i32,
    base_y: i32,
    image: &SimpleImage,
    count: i32,
    spacing: i32,
    stride: i32,
    row_step: i32,
) -> i32 {
    let mut offset = 0;

    for index in 0..count.max(0) {
        let reminder = index % 5;
        offset = stride * (index / 5) + spacing * reminder;

        draw_stat_image(
            frame,
            palette,
            image,
            base_x + offset,
            base_y + reminder * row_step,
        );
    }

    i32::from(image.width) + offset
}

/// `UnitStats_DrawImage`: копирует пиксели `SimpleImage`, пропуская
/// прозрачный индекс (первый пиксель ресурса).
fn draw_stat_image(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    x: i32,
    y: i32,
) -> u32 {
    let mut painted = 0;

    for py in 0..image.height as u32 {
        for px in 0..image.width as u32 {
            let Some(index) = image.pixel(px, py) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            if frame.set(x + px as i32, y + py as i32, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Текст по центру прямоугольника с переносом и вертикальным центрированием
/// (`Text_TextBox`), не больше `max_rows` строк.
fn draw_wrapped(
    frame: &mut RgbaBuffer,
    rect: PixelRect,
    font: &max_render::text::GnwFont,
    text: &str,
    color: Rgba,
    max_rows: usize,
) -> u32 {
    let mut lines = wrap_gnw_text(text, rect.width);
    if lines.len() > max_rows {
        lines.truncate(max_rows);
        if let Some(last) = lines.last_mut() {
            last.push_str("..");
        }
    }
    let rows = lines.len() as i32;
    let mut y = rect.uly + ((rect.height + 1) - font.height() * rows).max(0) / 2;
    let mut painted = 0;

    for line in &lines {
        painted += draw_text_gnw(frame, rect.ulx, y, font, line, color).max(0) as u32;
        y += font.height();
    }

    painted
}

/// Рамка выбранной строки (порт `GameManager_DrawUnitSelector`: 1 px, цвет
/// 0xFF; у списка «Purchased» — двойная).
fn draw_selection(
    frame: &mut RgbaBuffer,
    color: Rgba,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    thickness: i32,
) -> u32 {
    let mut painted = 0;

    for step in 0..thickness {
        let left = x + step;
        let top = y + step;
        let right = x + width - 1 - step;
        let bottom = y + height - 1 - step;

        for px in left..=right {
            if frame.set(px, top, color) {
                painted += 1;
            }
            if frame.set(px, bottom, color) {
                painted += 1;
            }
        }
        for py in top..=bottom {
            if frame.set(left, py, color) {
                painted += 1;
            }
            if frame.set(right, py, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// Кнопка: картинка (или честная рамка), при недоступности — `_X`.
#[allow(clippy::too_many_arguments)]
fn draw_button(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    pair: Option<&MenuButtonPair>,
    disabled: Option<&SimpleImage>,
    rect: PixelRect,
    label: &str,
    _mouse: Option<(i32, i32)>,
    enabled: bool,
) -> u32 {
    let image = if enabled {
        pair.map(|pair| pair.image(false))
    } else {
        disabled.or_else(|| pair.map(|pair| pair.image(false)))
    };

    match image {
        Some(image) => {
            let down = pair.map(|pair| pair.image(true));
            let down = down.unwrap_or(image);
            draw_image_button(frame, palette, image, down, rect, label, false)
        }
        None => {
            let mut painted = frame.fill_rect(rect, palette.rgba(crate::setup::TEXT));
            if !label.is_empty() {
                painted += draw_text_gnw_centered(
                    frame,
                    rect,
                    GnwFontId::Font1.font(),
                    label,
                    palette.rgba(crate::setup::GREEN),
                )
                .max(0) as u32;
            }
            painted
        }
    }
}

/// Фильтр домена: `_D` (нажат) для активного, `CRGSELDX` в режиме покупки.
fn draw_filter(
    frame: &mut RgbaBuffer,
    cargo: &CargoMenuAssets,
    palette: &UiPalette,
    filter: UpgradeFilter,
    index: usize,
    active: bool,
    mode: CargoMode,
) -> u32 {
    let rect = filter.rect();

    if mode == CargoMode::Buy {
        if let Some(disabled) = cargo.filter_disabled.as_ref() {
            return blit_simple(frame, palette, disabled, rect.ulx, rect.uly, true);
        }
        return 0;
    }

    match cargo.filters.get(index).and_then(|pair| pair.as_ref()) {
        Some(pair) => blit_simple(frame, palette, pair.image(active), rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// Кнопка-переключатель (`BLDDES`): `_D`, когда описание включено.
fn draw_filter_button(
    frame: &mut RgbaBuffer,
    pair: Option<&MenuButtonPair>,
    palette: &UiPalette,
    rect: PixelRect,
    active: bool,
) -> u32 {
    match pair {
        Some(pair) => blit_simple(frame, palette, pair.image(active), rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// Стрелка скролла списка с `_X`-вариантом.
fn draw_scroll(
    frame: &mut RgbaBuffer,
    pair: Option<&MenuButtonPair>,
    disabled: Option<&SimpleImage>,
    palette: &UiPalette,
    rect: PixelRect,
    enabled: bool,
    _mouse: Option<(i32, i32)>,
) -> u32 {
    if enabled {
        match pair {
            Some(pair) => blit_simple(frame, palette, pair.image(false), rect.ulx, rect.uly, true),
            None => 0,
        }
    } else if let Some(disabled) = disabled {
        blit_simple(frame, palette, disabled, rect.ulx, rect.uly, true)
    } else {
        0
    }
}

/// Стрелка апгрейда (`UPGLFT`/`UPGRGT`, недоступная — `*_X`).
fn draw_upgrade_arrow(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: Option<&SimpleImage>,
    disabled: Option<&SimpleImage>,
    rect: PixelRect,
    enabled: bool,
) -> u32 {
    let chosen = if enabled { image } else { disabled.or(image) };
    match chosen {
        Some(chosen) => blit_simple(frame, palette, chosen, rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// Полоса (`LoadVerticalBar`): 20-пиксельная текстура снизу вверх.
#[allow(clippy::too_many_arguments)]
fn draw_vertical_bar(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    bar: &SimpleImage,
    x: i32,
    y: i32,
    width: i32,
    capacity: i32,
    fill: i32,
) -> u32 {
    if fill <= 0 || capacity <= 0 {
        return 0;
    }

    let sprite_width = bar.width as i32;
    let sprite_height = bar.height as i32;
    let fill = fill.min(sprite_height).min(capacity);
    let offset = (width - sprite_width).max(0) / 2;
    let mut painted = 0;

    for row in 0..fill {
        let target_y = y + capacity - fill + row;
        for column in 0..sprite_width {
            let Some(index) = bar.pixel(column as u32, row as u32) else {
                continue;
            };
            if index == bar.transparent {
                continue;
            }
            if frame.set(x + offset + column, target_y, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Иконка юнита (`ReportStats_DrawListItemIcon`): кадр с горячей точкой в
/// центре, масштаб `zoom` (64 — 1:1), перекраска командных слотов.
pub(crate) fn draw_icon(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    unit: UnitType,
    icon: &UnitIcon,
    anchor: (i32, i32),
    zoom: u32,
    lut: &ColorLut,
) -> u32 {
    let mut painted = draw_icon_frame(frame, palette, &icon.body, anchor, zoom, lut);

    // `ReportStats_DrawListItemIcon`: для турельных типов — второй кадр
    // `I_*` со сдвигом `angle_offsets[2] >> divisor`.
    if unit_type_flags(unit) & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
        if let Some(turret) = icon.turret.as_ref() {
            let divisor = if !unit.is_mobile() {
                2
            } else {
                1
            };
            let offset = (
                icon.turret_offset.0 >> divisor,
                icon.turret_offset.1 >> divisor,
            );

            painted += draw_icon_frame(
                frame,
                palette,
                turret,
                (anchor.0 + offset.0, anchor.1 + offset.1),
                zoom,
                lut,
            );
        }
    }

    painted
}

/// Один кадр иконки `I_*` с масштабом `zoom` (64 — 1:1) и перекраской команд.
fn draw_icon_frame(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    icon: &SpriteFrame,
    anchor: (i32, i32),
    zoom: u32,
    lut: &ColorLut,
) -> u32 {
    let zoom = zoom.max(1);
    let width = ((u32::from(icon.width) * zoom) / 64).max(1) as i32;
    let height = ((u32::from(icon.height) * zoom) / 64).max(1) as i32;
    let ulx = anchor.0 - ((i32::from(icon.hotx) * zoom as i32) / 64);
    let uly = anchor.1 - ((i32::from(icon.hoty) * zoom as i32) / 64);
    let mut painted = 0;

    for y in 0..height {
        let source_y = ((y as i64 * 64) / i64::from(zoom)) as u32;
        let source_y = source_y.min(u32::from(icon.height).saturating_sub(1));
        for x in 0..width {
            let source_x = ((x as i64 * 64) / i64::from(zoom)) as u32;
            let source_x = source_x.min(u32::from(icon.width).saturating_sub(1));
            let Some(index) = icon.pixel(source_x, source_y) else {
                continue;
            };
            if frame.set(ulx + x, uly + y, palette.rgba(lut.map(index))) {
                painted += 1;
            }
        }
    }

    painted
}

/// Обрезка имени по ширине (как `Text_Blit` с `swidth`).
fn fit_label(text: &str, max_width: i32, font: &max_render::text::GnwFont) -> String {
    if font.text_width(text) <= max_width {
        return text.to_string();
    }

    let mut out = String::new();
    for character in text.chars() {
        let mut candidate = out.clone();
        candidate.push(character);
        if font.text_width(&format!("{candidate}..")) > max_width {
            break;
        }
        out = candidate;
    }

    format!("{out}..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mobilization() -> Mobilization {
        Mobilization::new(TeamId::RED, TeamClan::Random, 150)
    }

    #[test]
    fn purchase_list_matches_cargo_menu_unit_types1() {
        assert_eq!(PURCHASE_UNITS.len(), 15, "CargoMenu::unit_types1");
        assert_eq!(
            PURCHASE_UNITS,
            [
                UnitType::Constructor,
                UnitType::Engineer,
                UnitType::Surveyor,
                UnitType::Scout,
                UnitType::Scanner,
                UnitType::Tank,
                UnitType::AssaultGun,
                UnitType::RocketLauncher,
                UnitType::MissileCrawler,
                UnitType::MineLayer,
                UnitType::RepairUnit,
                UnitType::SupplyTruck,
                UnitType::FuelTruck,
                UnitType::MobileAntiAir,
                UnitType::GoldTruck,
            ]
        );
        assert_eq!(mobilization().types, PURCHASE_UNITS.to_vec());
    }

    #[test]
    fn purchase_prices_are_three_times_turns() {
        // `CargoMenu::BuyUnit`: `Cargo_GetRawConsumptionRate(LANDPLT, 1) * turns`
        // (`SC_ATTRI`: CONSTRCT 8, ENGINEER 6, TANK 4, SURVEYOR 3).
        assert_eq!(unit_cost(UnitType::Constructor, TeamClan::Random), 24);
        assert_eq!(unit_cost(UnitType::Engineer, TeamClan::Random), 18);
        assert_eq!(unit_cost(UnitType::Tank, TeamClan::Random), 12);
        assert_eq!(unit_cost(UnitType::Surveyor, TeamClan::Random), 9);
        assert_eq!(LANDPLT_RAW_CONSUMPTION, 3);

        let mobilization = mobilization();
        for unit in PURCHASE_UNITS {
            assert_eq!(
                mobilization.price(unit),
                LANDPLT_RAW_CONSUMPTION
                    * base_unit_values(unit).get_attribute(ATTRIB_TURNS).max(0) as u32,
                "{unit:?}"
            );
        }
    }

    #[test]
    fn list_prices_follow_unit_flags() {
        let mobilization = mobilization();

        // Мобильные — ставка LANDPLT (3), здания — CONSTRCT (2), пехота —
        // TRAINHAL (1).
        assert_eq!(mobilization.list_price(UnitType::Tank), 12);
        assert_eq!(mobilization.list_price(UnitType::Infiltrator), 9);
        assert_eq!(mobilization.list_price(UnitType::Infantry), 2);
        assert_eq!(mobilization.list_price(UnitType::MineLayer), 9);
        assert_eq!(
            mobilization.list_price(UnitType::PowerGenerator),
            8,
            "4 хода * 2"
        );
        assert_eq!(mobilization.list_price(UnitType::GoldRefinery), 24);
    }

    #[test]
    fn clan_tradeoffs_change_the_price() {
        let base = unit_cost(UnitType::Tank, TeamClan::Random);
        let musashi = unit_cost(UnitType::Tank, TeamClan::Musashi);

        assert_eq!(
            musashi,
            LANDPLT_RAW_CONSUMPTION
                * team_base_values(TeamClan::Musashi)[UnitType::Tank as usize]
                    .get_attribute(ATTRIB_TURNS)
                    .max(0) as u32
        );
        assert!(base > 0);
    }

    #[test]
    fn upgrade_list_is_the_full_filtered_set() {
        let mobilization = mobilization();
        let mut upgrade = mobilization.clone();
        upgrade.toggle_mode();

        assert_eq!(upgrade.mode, CargoMode::Upgrade);
        assert!(
            upgrade.types.len() > PURCHASE_UNITS.len(),
            "список модернизации шире списка покупки: {}",
            upgrade.types.len()
        );
        // Строится командой и апгрейдится.
        for unit in [
            UnitType::Constructor,
            UnitType::Tank,
            UnitType::PowerGenerator,
            UnitType::Shipyard,
            UnitType::Fighter,
            UnitType::LandMine,
            UnitType::SeaMine,
            UnitType::Infiltrator,
            UnitType::Infantry,
        ] {
            assert!(upgrade.types.contains(&unit), "{unit:?}");
        }
        // Road не UPGRADABLE, мины — через постановщика (уже включены), а
        // ракеты/взрывы/обломки не строятся.
        for unit in [
            UnitType::Road,
            UnitType::Rocket,
            UnitType::Torpedo,
            UnitType::LargeRubble,
            UnitType::LandExplosion,
            UnitType::DeadWaldo,
        ] {
            assert!(!upgrade.types.contains(&unit), "{unit:?}");
        }
        assert_eq!(
            upgrade.types.len(),
            upgrade_types(&mobilization.filters).len()
        );
    }

    #[test]
    fn upgrade_filters_switch_the_list() {
        let mut mobilization = mobilization();
        mobilization.toggle_mode();
        let all = mobilization.types.len();

        // Только наземные.
        mobilization.filters = [true, false, false, false, false];
        mobilization.rebuild_types();
        let ground = mobilization.types.len();
        assert!(ground > 0 && ground < all, "ground {ground} из {all}");

        // Только боевые (с атакой).
        mobilization.filters = [true, true, true, true, true];
        mobilization.rebuild_types();
        assert!(mobilization
            .types
            .iter()
            .all(|unit| { base_unit_values(*unit).get_attribute(ATTRIB_ATTACK) > 0 }));
        assert!(mobilization.types.len() < all);

        // Все домены выключены — пусто (C++: class_matches ложно).
        mobilization.filters = [false, false, false, false, true];
        mobilization.rebuild_types();
        assert!(mobilization.types.is_empty());
    }

    #[test]
    fn buying_and_selling_conserves_credits() {
        let mut mobilization = mobilization();

        assert!(mobilization.buy(0), "Constructor за 24 кредита");
        assert_eq!(mobilization.credits, 126);
        assert_eq!(mobilization.bought.len(), 1);
        assert_eq!(mobilization.loadout_selected, 3);
        assert_eq!(mobilization.info, Some(UnitType::Constructor));

        // Cargo покупается по 1 кредиту за 5 единиц: Constructor (storage 60)
        // может взять 12 блоков.
        let cargo_row = mobilization.loadout_selected;
        assert_eq!(mobilization.cargo(cargo_row), 0);
        mobilization.set_cargo_value(4);
        assert_eq!(mobilization.cargo(cargo_row), 20);
        assert_eq!(mobilization.credits, 122);

        assert!(mobilization.delete_selected());
        // Возврат: цена (24) + cargo/5 (4).
        assert_eq!(mobilization.credits, 150);
        assert!(mobilization.bought.is_empty());
        assert_eq!(
            mobilization.loadout().len(),
            DEFAULT_MISSION_LOADOUT.len(),
            "бесплатная загрузка остаётся"
        );
    }

    #[test]
    fn purchase_is_rejected_without_credits() {
        let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 10);

        assert!(!mobilization.buy(0));
        assert!(mobilization.bought.is_empty());
        assert_eq!(mobilization.credits, 10);
        assert!(mobilization.status.contains("Not enough credits"));
    }

    #[test]
    fn free_loadout_cannot_be_sold_or_unloaded() {
        let mut mobilization = mobilization();

        mobilization.select_loadout(0);
        assert!(!mobilization.delete_selected(), "бесплатная загрузка");
        assert_eq!(mobilization.loadout(), DEFAULT_MISSION_LOADOUT.to_vec());
        assert_eq!(mobilization.cargo_capacity(), 12, "60 / 5");
        mobilization.set_cargo_value(0);
        assert_eq!(
            mobilization.cargo(0),
            40,
            "xfer_take_max не даёт снять бесплатный cargo"
        );
    }

    #[test]
    fn gold_truck_has_no_cargo_slider() {
        let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 150);
        let index = PURCHASE_UNITS
            .iter()
            .position(|unit| *unit == UnitType::GoldTruck)
            .unwrap();
        assert!(mobilization.buy(index));
        assert_eq!(
            mobilization.cargo_capacity(),
            0,
            "GOLDTRCK: free_capacity 0"
        );
        mobilization.set_cargo_value(8);
        assert_eq!(mobilization.cargo(mobilization.loadout_selected), 0);
    }

    #[test]
    fn cargo_slider_material_depends_on_type() {
        let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 500);

        assert_eq!(
            mobilization.cargo_bar_kind(),
            CargoBarKind::Raw,
            "Constructor"
        );
        mobilization.select_loadout(1);
        assert_eq!(mobilization.cargo_bar_kind(), CargoBarKind::Raw, "Engineer");

        let fuel = PURCHASE_UNITS
            .iter()
            .position(|unit| *unit == UnitType::FuelTruck)
            .unwrap();
        assert!(mobilization.buy(fuel));
        assert_eq!(mobilization.cargo_bar_kind(), CargoBarKind::Fuel);

        let gold = PURCHASE_UNITS
            .iter()
            .position(|unit| *unit == UnitType::GoldTruck)
            .unwrap();
        assert!(mobilization.buy(gold));
        assert_eq!(mobilization.cargo_bar_kind(), CargoBarKind::Gold);
    }

    #[test]
    fn cargo_slider_click_matches_scrollbar_formula() {
        let mut mobilization = mobilization();
        mobilization.select_loadout(0); // Constructor, cargo 40/60 (8/12 блоков)

        // Клик у самого низа — 0 блоков, но бесплатный cargo не снимается.
        assert!(mobilization.click_cargo(CARGO_BAR.ulx + 2, CARGO_BAR.lry() - 1));
        assert_eq!(mobilization.cargo(0), 40);

        // Клик у самого верха — ёмкость (12 блоков).
        assert!(mobilization.click_cargo(CARGO_BAR.ulx + 2, CARGO_BAR.uly));
        assert_eq!(mobilization.cargo(0), 60);
        assert_eq!(mobilization.credits, 150 - 4, "4 блока * 1 кредит");
    }

    #[test]
    fn upgrades_change_all_units_of_the_type() {
        let mut mobilization = mobilization();
        mobilization.select_loadout(0); // Constructor

        // Конструктор: armor, hits, scan, speed (после пропусков storage и
        // расхода).
        let rows: Vec<u8> = mobilization
            .controls
            .iter()
            .map(|(attribute, _)| *attribute)
            .collect();
        assert_eq!(
            rows,
            vec![ATTRIB_ARMOR, ATTRIB_HITS, ATTRIB_SCAN, ATTRIB_SPEED]
        );
        // Две пропущенные строки до блока Common: storage (Constructor в
        // списке `AdjustRowStorage`) и расход (`AdjustRowConsumptions`).
        assert_eq!(mobilization.controls[0].1, COST.uly + 2 * CONTROL_ROW);
        assert_eq!(mobilization.controls[1].1, COST.uly + 3 * CONTROL_ROW);

        let before = mobilization.credits;
        let armor_base = mobilization.base[UnitType::Constructor as usize].armor;

        assert!(mobilization.adjust_upgrade(0, true));
        assert!(mobilization.credits < before, "апгрейд списывает кредиты");
        assert_eq!(
            mobilization.actual[UnitType::Constructor as usize].armor,
            armor_base + 1,
            "шаг 1 при базе < 10"
        );

        // Возврат — обратное изменение.
        assert!(mobilization.adjust_upgrade(0, false));
        assert_eq!(mobilization.credits, before);
        assert_eq!(
            mobilization.actual[UnitType::Constructor as usize].armor,
            armor_base
        );

        // Апгрейд, затем покупка: цена берётся из текущих turns.
        mobilization.adjust_upgrade(1, true); // hits (+2 при базе 24)
        let changed = mobilization.changed_upgrades();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, UnitType::Constructor);
        assert_eq!(changed[0].1.hits, armor_base_hits() + 2);
    }

    fn armor_base_hits() -> u16 {
        base_unit_values(UnitType::Constructor).hits
    }

    #[test]
    fn upgrade_cost_is_deterministic() {
        let run = || {
            let mut mobilization = mobilization();
            mobilization.select_loadout(0);
            let mut log = Vec::new();
            for row in 0..mobilization.controls.len() {
                for _ in 0..3 {
                    let credits = mobilization.credits;
                    mobilization.adjust_upgrade(row, true);
                    log.push((row, credits, mobilization.credits));
                }
            }
            (log, mobilization.changed_upgrades())
        };

        assert_eq!(run(), run());
    }

    #[test]
    fn upgrades_raise_purchase_prices_of_the_type() {
        let mut mobilization = mobilization();
        // Апгрейд «ходов» в CargoMenu недоступен, но цена покупки зависит от
        // текущих turns: проверим через прямое изменение actual (как это делает
        // апгрейд атрибута turns в других меню).
        assert_eq!(mobilization.price(UnitType::Tank), 12);
        mobilization.actual[UnitType::Tank as usize].turns = 8;
        assert_eq!(mobilization.price(UnitType::Tank), 24, "3 * turns");
    }

    #[test]
    fn zero_budget_skips_the_phase() {
        assert!(!Mobilization::needed(0));
        assert!(Mobilization::needed(150));
    }

    #[test]
    fn long_names_are_shortened_with_dots() {
        let font = GnwFontId::Font5.font();

        assert_eq!(fit_label("Tank", 100, font), "Tank");
        let short = fit_label("Rocket Launcher", 80, font);

        assert!(short.ends_with(".."), "{short}");
        assert!(font.text_width(&short) <= 80, "{short}");

        let mobilization = mobilization();
        assert_eq!(
            mobilization.selected_cost(),
            unit_cost(UnitType::Constructor, TeamClan::Random)
        );
    }

    #[test]
    fn stats_rows_follow_the_cpp_blocks() {
        let mut mobilization = mobilization();
        mobilization.select_loadout(0); // Constructor

        let rows = mobilization.stats_rows();
        let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();
        assert_eq!(
            labels,
            vec!["Cargo", "Uses", "Armor", "Hits", "Scan", "Speed", "Cost"],
            "UnitStats_DrawStats: cargo, usage, common, speed, cost"
        );
        // Шаг тот же, что у колонки апгрейдов (`UnitStats_DrawText` — 19 px).
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row.y, STATS.uly + STAT_ROW * index as i32);
        }

        // `Cargo`: число — текущая вместимость, pips — `storage / 10`.
        assert_eq!(rows[0].value, Some(60));
        assert_eq!(rows[0].icons, vec![StatIcon::same("I_RAW", 6, 6)]);
        // `Uses`: числа нет (`resource_count == 0`), raw-расход 2 — два pips.
        assert_eq!(rows[1].value, None);
        assert_eq!(rows[1].icons, vec![StatIcon::same("I_RAW", 2, 2)]);
        // `Armor`/`Hits`/`Scan`/`Speed` — 6/24/3/6.
        assert_eq!(rows[2].value, Some(6));
        assert_eq!(rows[3].value, Some(24));
        assert_eq!(rows[3].icons[0], StatIcon::same("I_HITS", 24, 24));
        assert_eq!(rows[4].value, Some(3));
        assert_eq!(rows[5].value, Some(6));
        // `Cost`: `Cargo_GetRawConsumptionRate(LANDPLT,1) * turns = 3 * 8 = 24`,
        // иконки — `I_GOLD`/`I_GOLDE` (`interface_icon_full/empty`), без линии.
        assert_eq!(rows[6].value, Some(24));
        assert_eq!(
            rows[6].icons,
            vec![StatIcon {
                resource: "I_GOLD",
                empty_resource: "I_GOLDE",
                current: 24,
                original: 24,
            }]
        );
        assert!(!rows[6].separator, "Cost — единственная строка без линии");
        assert!(rows[..6].iter().all(|row| row.separator));

        assert!(mobilization.stats_icon_count() >= 6 + 2 + 6 + 24 + 3 + 6 + 24);
    }

    #[test]
    fn military_rows_follow_min_rounds_ammo_and_base_values() {
        let mut mobilization = mobilization();
        mobilization.info = Some(UnitType::Tank);

        let rows = mobilization.stats_rows();
        let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();
        assert_eq!(
            labels,
            vec!["Attack", "Shots", "Range", "Ammo", "Armor", "Hits", "Scan", "Speed", "Cost"]
        );
        assert_eq!(rows[0].value, Some(16));
        assert_eq!(rows[0].icons[0], StatIcon::same("I_HRDATK", 16, 16));
        // `std::min(ROUNDS, AMMO)` = min(2, 14).
        assert_eq!(rows[1].value, Some(2));
        assert_eq!(rows[2].value, Some(4));
        assert_eq!(rows[3].value, Some(14));
        // Военных строк у Constructora нет (ATTACK = 0).
        mobilization.info = Some(UnitType::Constructor);
        assert!(mobilization
            .stats_rows()
            .iter()
            .all(|row| !matches!(row.label, "Attack" | "Shots" | "Range" | "Ammo")));
    }

    #[test]
    fn usage_and_storage_rows_follow_unit_stats_switches() {
        let mut mobilization = mobilization();

        // `POWGEN`: выработка -1 (`Prod.` + `I_POWER`), `Cost` 2 * 4.
        mobilization.info = Some(UnitType::PowerGenerator);
        let rows = mobilization.stats_rows();
        assert_eq!(rows[0].label, "Prod.");
        assert_eq!(rows[0].value, Some(1));
        assert_eq!(rows[0].icons, vec![StatIcon::same("I_POWER", 1, 1)]);
        assert_eq!(rows.last().unwrap().label, "Cost");
        assert_eq!(rows.last().unwrap().value, Some(8), "CONSTRCT 2 * 4 turns");

        // `HABITAT`: жизнеобеспечение -3 (`Prod.` + `I_LIFE`); выработки нет —
        // `Cargo_GetPowerConsumptionRate` для HABITAT = 0.
        mobilization.info = Some(UnitType::Habitat);
        let rows = mobilization.stats_rows();
        assert_eq!(rows[0].label, "Prod.");
        assert_eq!(rows[0].icons, vec![StatIcon::same("I_LIFE", 3, 3)]);

        // `MINELAYR`: storage 16, но `UnitStats_DrawCargo` его не рисует
        // (default switch) — строки `Cargo` нет; ставка LIGHTPLT 3 * 3.
        mobilization.info = Some(UnitType::MineLayer);
        let rows = mobilization.stats_rows();
        assert!(rows.iter().all(|row| row.label != "Cargo"));
        assert_eq!(rows.last().unwrap().value, Some(9));
        assert_eq!(rows.last().unwrap().icons[0].resource, "I_GOLD");

        // `LANDMINE`: `Builder_GetBuilderType` = INVALID_ID → ставка 0,
        // уровень зажимается в 1 (`UnitStats_DrawCost`).
        mobilization.info = Some(UnitType::LandMine);
        let rows = mobilization.stats_rows();
        assert_eq!(rows.last().unwrap().value, Some(1));
        assert_eq!(rows.last().unwrap().icons[0].current, 1);
        assert_eq!(mobilization.build_rate(UnitType::LandMine), 0);
    }

    /// `UnitStats_DrawStats`: `Speed` — только при `ATTRIB_SPEED > 0`
    /// (`if (unit_values2->GetAttribute(ATTRIB_SPEED))`), поэтому здания
    /// (шахта/завод/турель) в `CargoMenu` строку `Speed` не показывают.
    #[test]
    fn cargo_menu_buildings_never_show_speed() {
        let mut mobilization = mobilization();

        for unit_type in [
            UnitType::MiningStation,
            UnitType::TrainingHall,
            UnitType::GunTurret,
        ] {
            mobilization.info = Some(unit_type);
            let rows = mobilization.stats_rows();

            assert!(
                rows.iter().all(|row| row.label != "Speed"),
                "{unit_type:?}: {:?}",
                rows.iter().map(|row| row.label).collect::<Vec<_>>()
            );
        }

        mobilization.info = Some(UnitType::Tank);
        assert!(
            mobilization
                .stats_rows()
                .iter()
                .any(|row| row.label == "Speed"),
            "у танка Speed должен остаться"
        );
    }

    #[test]
    fn upgraded_values_show_delta_pips() {
        let mut mobilization = mobilization();
        mobilization.info = Some(UnitType::Tank);
        let base_rows = mobilization.stats_rows();

        mobilization.actual[UnitType::Tank as usize].attack = 20;
        let rows = mobilization.stats_rows();
        let attack = &rows[0];

        // `UnitStats_DrawIcons(current = base, original = actual)`: 4 пустых
        // pips дельты (`original - current`) за красной чертой.
        assert_eq!(attack.value, Some(20), "число — текущее значение");
        assert_eq!(attack.icons[0].current, 16);
        assert_eq!(attack.icons[0].original, 20);
        assert_eq!(attack.icons[0].count(), 20);
        assert_eq!(attack.icon_count(), 20);
        assert_eq!(base_rows[0].icon_count(), 16);

        // Уменьшение turns: текущая цена ниже базовой — `original > current`,
        // поэтому 3 «пустых» pips показывают базовую стоимость.
        mobilization.actual[UnitType::Tank as usize].turns = 3;
        let rows = mobilization.stats_rows();
        let cost = rows.last().unwrap();
        assert_eq!(cost.value, Some(9), "LANDPLT 3 * 3");
        assert_eq!(cost.icons[0].current, 9);
        assert_eq!(cost.icons[0].original, 12);
        assert_eq!(cost.icons[0].count(), 12, "9 заполненных + 3 пустых");

        // Увеличение turns: `original < current` — пустых pips нет.
        mobilization.actual[UnitType::Tank as usize].turns = 5;
        let rows = mobilization.stats_rows();
        let cost = rows.last().unwrap();
        assert_eq!(cost.value, Some(15));
        assert_eq!(cost.icons[0].current, 15);
        assert_eq!(cost.icons[0].original, 12);
        assert_eq!(cost.icons[0].count(), 15);
    }

    #[test]
    fn description_and_portrait_come_from_the_generated_resource() {
        assert!(cargo_unit_data::is_loaded());

        let mobilization = mobilization();
        assert_eq!(mobilization.info, Some(UnitType::Constructor));
        assert!(mobilization
            .description()
            .starts_with("Large construction vehicle"));
        assert!(mobilization.has_portrait());

        // Каждый тип списка покупки и режима модернизации описан и имеет
        // портрет — у каждого открываемого в `CargoMenu` типа.
        let mut all = PURCHASE_UNITS.to_vec();
        all.extend(upgrade_types(&[true, true, true, true, false]));
        all.sort_by_key(|unit| *unit as u16);
        all.dedup();

        assert!(all.len() >= 60, "типов в `CargoMenu`: {}", all.len());

        for unit in all {
            assert!(
                !cargo_unit_data::unit_description(unit).is_empty(),
                "{unit:?}: описание в `language.json`"
            );
            assert!(
                cargo_unit_data::unit_portrait(unit).is_some(),
                "{unit:?}: `P_*` портрет"
            );
        }
    }

    #[test]
    fn only_unreachable_types_have_no_description() {
        let mut reachable = PURCHASE_UNITS.to_vec();
        reachable.extend(upgrade_types(&[true, true, true, true, false]));

        for index in 0..UNIT_END {
            let unit = UnitType::try_from(index).expect("UnitType 0..92");

            if cargo_unit_data::unit_description(unit).is_empty() {
                // 24 служебных типа (обломки, взрывы, снаряды, HARVSTER,
                // WALDO) — без описания и в оригинале; в `CargoMenu` их не
                // открыть, поэтому `assert cargo-description` всегда видит текст.
                assert!(
                    !reachable.contains(&unit),
                    "{unit:?}: без описания, но достижим в CargoMenu"
                );
            } else {
                assert!(
                    cargo_unit_data::unit_portrait(unit).is_some(),
                    "{unit:?}: описание есть, портрета нет"
                );
            }
        }
    }

    #[test]
    fn stat_icon_resource_names_match_unitstats_resources() {
        // `UnitStats_DrawStats`: ровно те `I_*`, что читает C++.
        for name in [
            "I_HRDATK", "I_SHOTS", "I_RANGE", "I_AMMO", "I_RAW", "I_FUEL", "I_GOLD", "I_POWER",
            "I_LIFE", "I_ARMOR", "I_HITS", "I_SCAN", "I_SPEED",
        ] {
            assert!(STAT_ICONS.contains(&name), "{name}");
        }
        // `CargoMenu` передаёт `interface_icon_full/empty = I_GOLD/I_GOLDE`.
        assert!(STAT_ICONS.contains(&"I_GOLDE"));

        let mut mobilization = mobilization();
        for unit in upgrade_types(&[true, true, true, true, false]) {
            mobilization.info = Some(unit);
            let rows = mobilization.stats_rows();
            assert!(!rows.is_empty(), "{unit:?}: строки статистики");
            for row in &rows {
                for icon in &row.icons {
                    assert!(STAT_ICONS.contains(&icon.resource), "{}", icon.resource);
                    assert!(
                        STAT_ICONS.contains(&icon.empty_resource),
                        "{}",
                        icon.empty_resource
                    );
                }
            }
        }
    }

    #[test]
    fn clicks_follow_cargo_menu_buttons_and_double_click() {
        let mut mobilization = mobilization();

        // Клик по уже выбранной строке списка — `Select(1)` → `BuyUnit`.
        assert_eq!(
            mobilization.handle_click(AVAILABLE_AREA.ulx + 4, AVAILABLE_AREA.uly + 4),
            CargoAction::None
        );
        assert_eq!(mobilization.bought.len(), 1, "двойной клик покупает");
        assert_eq!(mobilization.credits, 126);

        // Клик по строке купленного юнита: после `BuyUnit` она уже выбрана
        // (`PushBack` → `Select`), поэтому повторный клик — продажа.
        let bought_row = mobilization.free.len();
        let y = PURCHASED_AREA.uly + bought_row as i32 * LIST_ROW + 4;
        mobilization.handle_click(PURCHASED_AREA.ulx + 4, y);
        assert!(mobilization.bought.is_empty(), "повторный клик продаёт");
        assert_eq!(mobilization.credits, 150);

        // Выбор строк бесплатной загрузки — информация следует за списком.
        mobilization.handle_click(PURCHASED_AREA.ulx + 4, PURCHASED_AREA.uly + 4);
        assert_eq!(mobilization.info, Some(UnitType::Constructor));
        mobilization.handle_click(PURCHASED_AREA.ulx + 4, PURCHASED_AREA.uly + LIST_ROW + 4);
        assert_eq!(mobilization.info, Some(UnitType::Engineer));

        // Кнопки Done/Cancel.
        assert_eq!(
            mobilization.handle_click(DONE_BUTTON.ulx + 2, DONE_BUTTON.uly + 2),
            CargoAction::Done
        );
        assert_eq!(
            mobilization.handle_click(CANCEL_BUTTON.ulx + 2, CANCEL_BUTTON.uly + 2),
            CargoAction::Cancel
        );

        // Переключатель режима и стрелка апгрейда.
        assert_eq!(
            mobilization.handle_click(TOGGLE_BUTTON.ulx + 2, TOGGLE_BUTTON.uly + 2),
            CargoAction::None
        );
        assert_eq!(mobilization.mode, CargoMode::Upgrade);
        let (_, right) = mobilization.control_arrows(0).expect("строка");
        let before = mobilization.credits;
        mobilization.handle_click(right.ulx + 2, right.uly + 2);
        assert!(mobilization.credits < before, "правая стрелка — Increase");

        // Клавиши.
        assert_eq!(mobilization.handle_key(Key::Enter), CargoAction::Done);
        assert_eq!(mobilization.handle_key(Key::Escape), CargoAction::Cancel);
        mobilization.handle_key(Key::Char('u'));
        assert_eq!(mobilization.mode, CargoMode::Buy);
        mobilization.handle_key(Key::Char('b'));
        assert_eq!(mobilization.bought.len(), 1, "b — BuyUnit");
    }
}
