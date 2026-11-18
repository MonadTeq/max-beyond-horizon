//! `BuildMenu` — экран заказа юнитов/зданий (P2).
//!
//! # Оракул
//!
//! * `buildmenu.cpp` — два режима одного экрана: `FactoryBuildMenu`
//!   (фон `FACBUILD`, очередь завода) и `MobileBuildMenu` (фон `CONBUILD`,
//!   строитель ставит одно здание); координаты списков и кнопок — из
//!   конструкторов C++ (`buildmenu.cpp:821-1045`);
//! * `unittypeselector.cpp` — список: 32 px строка, `max_item_count =
//!   (lry - uly) / 32` строк, прокрутка `page_min_index`/`page_max_index`;
//! * `Builder_GetBuildableUnits` — состав списка (`max_core::production::BUILDABLE`);
//! * `BuildMenu_GetTurnsToBuild`/`BuildMenu_GetMaxPossibleBuildRate` — цифры
//!   «ходов» и «цены» (порт в `max_core::production`).
//!
//! # Раскладка (640×480, пиксели кадра)
//!
//! | Элемент | C++ | Прямоугольник |
//! |---|---|---|
//! | список доступного | `window_info` 483,60–598,429 | 483,60 115×369 (11 строк) |
//! | очередь завода | `cargo_selector` 337,60–453,280 | 337,60 116×220 (6 строк) |
//! | стрелки списка | `BLDUP__U/BLDDWN_U` 471,441/491,441 | 18×17 |
//! | стрелки очереди | 327,293/347,293 | 18×17 |
//! | `Build` (в очередь) | 548,441 | 78×23 |
//! | `Delete` | 412,293 | 53×23 |
//! | `Repeat` | 447,322 | 17×17 |
//! | скорости 1×/2×/4× | `BLDBLD_U`/`BLD2X_U`/`BLD4X_U` | mobile 292,345/369/394; factory 292,371/396/421 |
//! | `Done` | 397,452 | 54×23 |
//! | `Cancel` | 307,452 | 62×23 |
//! | описание | `BLDDES_U` 292,264 | 17×17 |
//!
//! # Ввод (как C++)
//!
//! * `Done`/Enter (`GNW_KB_KEY_KP_ENTER` → `Build()`) — зафиксировать заказ;
//! * `Cancel`/Esc — закрыть без заказа;
//! * `Build` (factory) — `AddSelection`, `Delete` — `RemoveSelection`;
//! * строки списков — выбор (`UnitSelectEvent`);
//! * `1`/`2`/`3` — скорости 1×/2×/4× (`1101..1103`).
//!
//! Заказ уходит **тем же путём**, что в сети: `Command::Order` с `PayloadP4`
//! (`docs/architecture/protocol.md` §8: `repeat_build`, `build_time`,
//! `build_rate`, `build_count × unit_type`) — локально применяется
//! `apply_command`, в сети отправляется пакетом 08.
//!
//! # Иконки и портрет
//!
//! * строки списка/очереди — иконки `I_*` (`ReportStats_DrawListItemIcon`):
//!   здания 1:4 (`divisor = 2`), турельные типы — второй кадр со сдвигом
//!   `angle_offsets[2] >> divisor`, перекраска командных слотов; имя с x=+35,
//!   ходы справа (`0xA2`/`COLOR_RED`), выделение — белые уголки
//!   `GameManager_DrawUnitSelector`. Рисуется тем же `mobilize::draw_icon`, что
//!   и списки `CargoMenu` (`I_*` из `CargoMenuAssets`).
//! * портрет — big image `P_*` 300×240 в (11,13) (`AbstractBuildMenu::Draw`,
//!   `buildmenu.cpp:509-529`), палитра окна, для выбранной строки (у завода —
//!   активного селектора: список или очередь). Кэш `P_*` —
//!   `CargoMenuAssets::portrait`.
//! * характеристики — `UnitStats_DrawStats` в `stats_background`
//!   (11,293,250,174) с иконками `I_*` и строкой `Cost` (`I_GOLD`/`I_GOLDE`):
//!   тот же `mobilize::{stats_rows_for, draw_stats}`, что в `CargoMenu`;
//!   база/текущие значения команды (клановые модификаторы и апгрейды уже внутри).
//! * `Turns`/`Cost` — `UnitInfo::GetTurnsToBuild` (`turns_and_consumption`):
//!   для 2×/4× это не «ставка за ход», а полная сумма с учётом падения
//!   скорости при нехватке склада; 1× — `turns` и `turns * rate1`. У Training
//!   Hall 2×/4× выключены, у мобильного строителя скрыты, если `max_rate`
//!   меньше.
//!
//! # Осознанные упрощения
//!
//! * подсказка `Help` (`BLDHLP_U`) и кнопка `Build path` инженера
//!   (`BLDPTH_U`) не портированы (путь-строительство — задача P5);
//! * звуки кнопок не проигрываются (аудио-слой — `audio.rs`).

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::Point;
use max_core::input::{Command, OrderPayload, Packet08Order, PayloadP1, PayloadP2, PayloadP3, PayloadP4};
use max_core::production::{
    buildable_units, can_build, cost_per_turn, find_build_site, is_factory,
    max_possible_build_rate, turns_and_consumption, turns_to_build as production_turns_to_build,
    BuildError,
};
use max_core::team::TeamId;
use max_core::unit::UnitId;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::menu::MenuButtonPair;
use crate::mobilize::{center_y, draw_icon, draw_text_right, team_lut};
use crate::panels::UiPalette;
use crate::Assets;

/// Логическая ширина экрана (окно оригинала).
pub const SCREEN_WIDTH: i32 = 640;
/// Логическая высота экрана (окно оригинала).
pub const SCREEN_HEIGHT: i32 = 480;

/// Список доступного (`buildmenu.cpp:1008`): 483,60 — 598,429.
pub const LIST_AREA: PixelRect = PixelRect::new(483, 60, 115, 369);
/// Высота строки списка (`unittypeselector.cpp:64`, `* 32`).
pub const LIST_ROW: i32 = 32;
/// Сколько строк в списке: `(429 - 60) / 32`.
pub const LIST_ROWS: usize = 11;

/// Очередь завода (`buildmenu.cpp:1020`): 337,60 — 453,280.
pub const QUEUE_AREA: PixelRect = PixelRect::new(337, 60, 116, 220);
/// Сколько строк в очереди: `(280 - 60) / 32`.
pub const QUEUE_ROWS: usize = 6;

/// Стрелка вверх списка (`BLDUP__U`, 18×17).
pub const LIST_UP: PixelRect = PixelRect::new(471, 441, 18, 17);
/// Стрелка вниз списка (`BLDDWN_U`).
pub const LIST_DOWN: PixelRect = PixelRect::new(491, 441, 18, 17);
/// Стрелка вверх очереди.
pub const QUEUE_UP: PixelRect = PixelRect::new(327, 293, 18, 17);
/// Стрелка вниз очереди.
pub const QUEUE_DOWN: PixelRect = PixelRect::new(347, 293, 18, 17);
/// `Delete` очереди завода (`BLDDEL_U`, 53×23).
pub const QUEUE_DELETE: PixelRect = PixelRect::new(412, 293, 53, 23);
/// `Repeat` завода (`BLDREP_U`, 17×17).
pub const QUEUE_REPEAT: PixelRect = PixelRect::new(447, 322, 17, 17);
/// `Build` (добавить в очередь) завода (`BLDBLD_U`, 78×23).
pub const FACTORY_ADD: PixelRect = PixelRect::new(548, 441, 78, 23);
/// `Done` (`BLDONE_U`, 54×23).
pub const DONE: PixelRect = PixelRect::new(397, 452, 54, 23);
/// `Cancel` (`BLDCAN_U`, 62×23).
pub const CANCEL: PixelRect = PixelRect::new(307, 452, 62, 23);
/// Кнопка описания (`BLDDES_U`, 17×17).
pub const DESCRIPTION: PixelRect = PixelRect::new(292, 264, 17, 17);
/// Скорости 1×/2×/4× мобильного меню.
pub const MOBILE_RATES: [PixelRect; 3] = [
    PixelRect::new(292, 345, 78, 23),
    PixelRect::new(292, 369, 78, 23),
    PixelRect::new(292, 394, 78, 23),
];
/// Скорости 1×/2×/4× меню завода.
pub const FACTORY_RATES: [PixelRect; 3] = [
    PixelRect::new(292, 371, 78, 23),
    PixelRect::new(292, 396, 78, 23),
    PixelRect::new(292, 421, 78, 23),
];
/// Заголовок меню (`Text_TextBox(…, 327, 7, 158, 18, COLOR_GREEN, true)`).
pub const HEADER: PixelRect = PixelRect::new(327, 7, 158, 18);
/// Подпись «Unit» (`209,264,80,17`).
pub const UNIT_LABEL: PixelRect = PixelRect::new(209, 264, 80, 17);

/// Окно портрета `BuildMenu` (`AbstractBuildMenu::Draw`,
/// `buildmenu.cpp:522-529`): `local_window.buffer = &window.buffer[11 + 640*13]`
/// — big image `P_*` 300×240 в (11,13), палитра окна (`CONBUILD`/`FACBUILD`),
/// **без** обрезки 280×240, как в `CargoMenu` (там своё окно портрета).
pub const PORTRAIT: PixelRect = PixelRect::new(11, 13, 300, 240);

/// Центр иконки строки списка: `ReportStats_DrawListItem(..., ulx, uly)` рисует
/// `ReportStats_DrawListItemIcon(..., ulx + 16, uly + 16)`.
pub const LIST_ICON: i32 = 16;
/// X текста строки: `Text_TextBox(..., ulx + 35, uly, full - 35, 32, ...)`.
pub const LIST_TEXT_X: i32 = 35;
/// Правая граница цифр ходов: `ReportStats_DrawNumber(buffer + 135, turns, 20)`
/// в буфере списка, начатом в `LIST_AREA.ulx` (483 + 135 = 618).
pub const LIST_TURNS_RIGHT: i32 = 618;

/// Доступные скорости (кнопки 1×/2×/4×).
pub const BUILD_RATES: [u16; 3] = [1, 2, 4];

// ---------------------------------------------------------------------------
// Ресурсы BuildMenu
// ---------------------------------------------------------------------------

/// Картинки `BuildMenu` из `MAX.RES`.
///
/// Грузятся один раз в [`Assets::load`]; отсутствие любого ресурса — не
/// ошибка (кнопка/фон рисуются рамкой, как в `mobilize.rs`).
#[derive(Debug, Clone)]
pub struct BuildMenuAssets {
    /// Фон меню завода (`FACBUILD`, 640×480).
    pub facbuild: Option<BigImage>,
    /// Фон меню строителя (`CONBUILD`, 640×480).
    pub conbuild: Option<BigImage>,
    /// `Build` / `BLDBLD_U/D` (78×23).
    pub build: Option<MenuButtonPair>,
    /// `2×` (`BLD2X_U/D`).
    pub build2x: Option<MenuButtonPair>,
    /// `4×` (`BLD4X_U/D`).
    pub build4x: Option<MenuButtonPair>,
    /// `Done` (`BLDONE_U/D`).
    pub done: Option<MenuButtonPair>,
    /// `Cancel` (`BLDCAN_U/D`).
    pub cancel: Option<MenuButtonPair>,
    /// Стрелка вверх (`BLDUP__U/D`).
    pub up: Option<MenuButtonPair>,
    /// Стрелка вниз (`BLDDWN_U/D`).
    pub down: Option<MenuButtonPair>,
    /// `Delete` (`BLDDEL_U/D`).
    pub delete: Option<MenuButtonPair>,
    /// `Repeat` (`BLDREP_U/D`).
    pub repeat: Option<MenuButtonPair>,
    /// Описание (`BLDDES_U/D`).
    pub description: Option<MenuButtonPair>,
}

impl BuildMenuAssets {
    /// Грузит ресурсы из архива (отсутствующие — `None`).
    pub fn load(max_res: &max_assets::ResArchive) -> Self {
        let read_big = |name: &str| -> Option<BigImage> {
            let bytes = max_res.read(name).ok()?;
            BigImage::decode(bytes).ok()
        };
        let read_pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            let up = SimpleImage::decode(max_res.read(up).ok()?).ok()?;
            let down = SimpleImage::decode(max_res.read(down).ok()?).ok()?;
            Some(MenuButtonPair::new(up, down))
        };

        Self {
            facbuild: read_big("FACBUILD"),
            conbuild: read_big("CONBUILD"),
            build: read_pair("BLDBLD_U", "BLDBLD_D"),
            build2x: read_pair("BLD2X_U", "BLD2X_D"),
            build4x: read_pair("BLD4X_U", "BLD4X_D"),
            done: read_pair("BLDONE_U", "BLDONE_D"),
            cancel: read_pair("BLDCAN_U", "BLDCAN_D"),
            up: read_pair("BLDUP__U", "BLDUP__D"),
            down: read_pair("BLDDWN_U", "BLDDWN_D"),
            delete: read_pair("BLDDEL_U", "BLDDEL_D"),
            repeat: read_pair("BLDREP_U", "BLDREP_D"),
            description: read_pair("BLDDES_U", "BLDDES_D"),
        }
    }

    /// Есть ли фон и кнопки (для предупреждения в отчёте ассетов).
    pub fn is_ready(&self) -> bool {
        self.facbuild.is_some() && self.conbuild.is_some() && self.done.is_some()
    }
}

/// Английские имена юнитов (`ResourceManager_GetUnit(...).GetSingularName()`).
pub fn unit_name(unit_type: UnitType) -> &'static str {
    match unit_type {
        // здания (строитель)
        UnitType::MiningStation => "Mining Station",
        UnitType::PowerStation => "Power Station",
        UnitType::LightVehiclePlant => "Light Vehicle Plant",
        UnitType::HeavyVehiclePlant => "Heavy Vehicle Plant",
        UnitType::AirUnitsPlant => "Air Units Plant",
        UnitType::Shipyard => "Shipyard",
        UnitType::CommTower => "Comm Tower",
        UnitType::Depot => "Depot",
        UnitType::Hangar => "Hangar",
        UnitType::Dock => "Dock",
        UnitType::Habitat => "Habitat",
        UnitType::ResearchCenter => "Research Center",
        UnitType::Greenhouse => "Greenhouse",
        UnitType::TrainingHall => "Training Hall",
        UnitType::Barracks => "Barracks",
        UnitType::ADump => "Ammo Dump",
        UnitType::FDump => "Fuel Tank",
        UnitType::GoldSmelter => "Gold Vault",
        UnitType::PowerGenerator => "Power Generator",
        UnitType::Connector => "Connector",
        UnitType::Radar => "Radar",
        UnitType::GunTurret => "Gun Turret",
        UnitType::AntiAir => "Anti Aircraft",
        UnitType::ArtilleryTurret => "Artillery Turret",
        UnitType::AntiMissile => "Missile Launcher",
        UnitType::LandingPad => "Landing Pad",
        UnitType::Bridge => "Bridge",
        UnitType::WaterPlatform => "Water Platform",
        UnitType::Block => "Concrete Block",
        UnitType::Road => "Road",
        // подвижные
        UnitType::Scout => "Scout",
        UnitType::Surveyor => "Surveyor",
        UnitType::Engineer => "Engineer",
        UnitType::RepairUnit => "Repair Unit",
        UnitType::SupplyTruck => "Supply Truck",
        UnitType::FuelTruck => "Fuel Truck",
        UnitType::GoldTruck => "Gold Truck",
        UnitType::MobileAntiAir => "Mobile AA",
        UnitType::MineLayer => "Mine Layer",
        UnitType::Bulldozer => "Bulldozer",
        UnitType::PersonnelCarrier => "Personnel Carrier",
        UnitType::Constructor => "Constructor",
        UnitType::Scanner => "Scanner",
        UnitType::Tank => "Tank",
        UnitType::AssaultGun => "Assault Gun",
        UnitType::RocketLauncher => "Rocket Launcher",
        UnitType::MissileCrawler => "Missile Crawler",
        UnitType::Fighter => "Fighter",
        UnitType::Bomber => "Ground Attack Plane",
        UnitType::AirTransport => "Air Transport",
        UnitType::Awac => "AWAC",
        UnitType::FastBoat => "Escort",
        UnitType::Corvette => "Corvette",
        UnitType::Battleship => "Gunboat",
        UnitType::Submarine => "Submarine",
        UnitType::SeaTransport => "Sea Transport",
        UnitType::MissileBoat => "Missile Cruiser",
        UnitType::SeaMineLayer => "Sea Mine Layer",
        UnitType::CargoShip => "Cargo Ship",
        UnitType::Commando => "Infiltrator",
        UnitType::Infantry => "Infantry",
        _ => "Unit",
    }
}

// ---------------------------------------------------------------------------
// Пакет заказа
// ---------------------------------------------------------------------------

/// `Command::Order` с `PayloadP4` — тот же пакет 08, что уходит в сеть.
///
/// Общий путь `BuildMenu::Build` и подтверждения места стройки
/// (`GameManager_PathBuild`): C++ в обоих случаях заканчивает
/// `UnitInfo::BuildOrder` → `ORDER_BUILD`/`ORDER_STATE_INIT` с клеткой в
/// `move_to`. `build_list` — очередь (у мобильного строителя ровно один тип,
/// у завода — вся очередь), `build_time` считается по голове.
pub fn build_order_command(
    state: &GameState,
    unit_id: UnitId,
    site: Point,
    build_list: &[UnitType],
    repeat_build: bool,
    build_rate: u16,
) -> Option<Command> {
    let unit = state.unit(unit_id)?;
    let head = build_list.first().copied()?;
    let turns = production_turns_to_build(state, unit.team, head).max(1);
    let build_rate = build_rate.max(1);

    Some(Command::Order(Packet08Order {
        unit: unit_id,
        payload: OrderPayload::P4(PayloadP4 {
            p3: PayloadP3 {
                p2: PayloadP2 {
                    p1: PayloadP1 {
                        order: UnitOrder::Build,
                        state: UnitOrderState::Init,
                        prior_order: unit.orders,
                        prior_state: unit.state,
                        disabled_reaction_fire: unit.disabled_reaction_fire,
                    },
                    parent_unit: unit.parent_unit,
                    experience: unit.experience,
                },
                move_to: site,
                fire_on: Point::ZERO,
                enemy_unit: unit.enemy_unit,
                transfer_cargo: unit.transfer_cargo,
                stealth_dice_roll: unit.stealth_dice_roll,
            },
            repeat_build,
            build_time: turns.min(i32::from(u8::MAX)) as u8,
            build_rate,
            build_list: build_list.to_vec(),
        }),
    }))
}

// ---------------------------------------------------------------------------
// Состояние экрана
// ---------------------------------------------------------------------------

/// Что делать после клика/клавиши.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMenuAction {
    /// Экран остаётся открытым.
    None,
    /// `Build()` — заказ подтверждён (Enter/`Done`).
    Commit,
    /// `Esc`/`Cancel` — закрыть без заказа.
    Close,
}

/// Состояние открытого `BuildMenu` для одного юнита.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildMenu {
    /// Команда юнита.
    pub team: TeamId,
    /// Юнит-строитель.
    pub unit: UnitId,
    /// Тип строителя.
    pub builder_type: UnitType,
    /// Стационарный завод (иначе мобильный строитель).
    pub factory: bool,
    /// Доступные к постройке типы (`Builder_GetBuildableUnits`).
    pub unit_types: &'static [UnitType],
    /// Очередь заказа (у мобильного строителя — максимум один элемент).
    pub queue: Vec<UnitType>,
    /// Выбранная строка основного списка (`UnitTypeSelector::page_max_index`).
    pub selected: usize,
    /// Первая видимая строка основного списка (`page_min_index`).
    pub page_min: usize,
    /// Выбранная строка очереди (для `Delete`).
    pub queue_selected: usize,
    /// Первая видимая строка очереди.
    pub queue_page: usize,
    /// Активна очередь (клик по очереди) — для `Delete`.
    pub queue_active: bool,
    /// Скорость постройки (`build_rate`).
    pub build_rate: u16,
    /// `Repeat` (только завод).
    pub repeat: bool,
    /// Показывать описание (`button_description_rest_state`).
    pub show_description: bool,
    /// Строка статуса (ошибки `Build`).
    pub status: String,
}

impl BuildMenu {
    /// Открывает меню для юнита; `None` — юнит не строитель.
    pub fn new(state: &GameState, unit_id: UnitId) -> Option<Self> {
        let unit = state.unit(unit_id)?;
        let unit_types = buildable_units(unit.unit_type);

        if unit_types.is_empty() {
            return None;
        }

        let factory = is_factory(unit.unit_type);
        let queue = if factory {
            unit.build_list.clone()
        } else {
            Vec::new()
        };

        let mut menu = Self {
            team: unit.team,
            unit: unit_id,
            builder_type: unit.unit_type,
            factory,
            unit_types,
            queue,
            selected: 0,
            page_min: 0,
            queue_selected: 0,
            queue_page: 0,
            queue_active: false,
            build_rate: unit.build_rate.max(1),
            repeat: unit.repeat_build,
            show_description: false,
            status: String::new(),
        };

        // `AbstractBuildMenu::Draw` клампит `build_rate` по
        // `BuildMenu_GetMaxPossibleBuildRate`; для Training Hall скорость 1×.
        menu.build_rate = menu.build_rate.min(menu.max_rate(state).max(1));

        Some(menu)
    }

    /// Тип в выбранной строке списка.
    pub fn selected_type(&self) -> Option<UnitType> {
        self.unit_types.get(self.selected).copied()
    }

    /// Тип в выбранной строке очереди.
    pub fn queue_type(&self) -> Option<UnitType> {
        self.queue.get(self.queue_selected).copied()
    }

    /// Тип активного селектора — `selector->GetLast()` в C++
    /// (`AbstractBuildMenu::Select`/`FactoryBuildMenu::Select`): у завода это
    /// либо выбранная строка списка, либо выбранная строка очереди (если
    /// очередь активна). От него зависят портрет, характеристики и колонки
    /// `Turns`/`Cost`.
    pub fn active_type(&self) -> Option<UnitType> {
        if self.queue_active {
            self.queue_type()
        } else {
            self.selected_type()
        }
    }

    /// Сколько строк списка видно.
    pub const fn rows(&self) -> usize {
        LIST_ROWS
    }

    /// Первая видимая строка списка.
    pub const fn first_row(&self) -> usize {
        self.page_min
    }

    /// Ходов на выбранный тип (цифра в списке).
    pub fn turns_for(&self, state: &GameState, unit_type: UnitType) -> i32 {
        production_turns_to_build(state, self.team, unit_type).max(0)
    }

    /// Максимальная скорость для головы очереди (или выбранного типа).
    pub fn max_rate(&self, state: &GameState) -> u16 {
        let Some(unit) = state.unit(self.unit) else {
            return 1;
        };

        // C++ `AbstractBuildMenu::Draw`: у Training Hall кнопки 2×/4×
        // безусловно выключены (`unit->GetUnitType() == TRAINHAL`), поэтому
        // скорость ограничена 1×, хотя `BuildMenu_GetMaxPossibleBuildRate`
        // формально вернул бы 4.
        if unit.unit_type == UnitType::TrainingHall {
            return 1;
        }

        let head = self.active_type().unwrap_or(unit.unit_type);
        let turns = production_turns_to_build(state, self.team, head).max(1);
        let storage = if self.factory {
            i32::from(i16::MAX)
        } else {
            i32::from(unit.storage)
        };

        max_possible_build_rate(unit.unit_type, turns, storage).max(1) as u16
    }

    /// Ходы и **полная** стоимость для скорости `rate` — C++
    /// `UnitInfo::GetTurnsToBuild(unit_type, rate, &turns)`
    /// (`unitinfo.cpp:3393`) и `AbstractBuildMenu::Draw` (`buildmenu.cpp:545-660`).
    ///
    /// `unit_type` — активная строка (`selector->GetLast()`): список у мобильного
    /// строителя, активный список/очередь у завода.
    ///
    /// Для 1× C++ печатает `GetTurnsToBuild(unit_type)` и
    /// `turns * Cargo_GetRawConsumptionRate(builder, 1)`; для 2×/4× — результат
    /// [`turns_and_consumption`]: на каждом ходу скорость ограничивается
    /// `BuildMenu_GetMaxPossibleBuildRate` по остатку и складу, поэтому цена —
    /// не «ставка за ход», а сумма всех ходов (и ходы могут не делиться на
    /// скорость). В `ATTRIB_TURNS` уже входят клановые модификаторы и
    /// исследования (`current_unit_values`).
    pub fn rate_numbers(&self, state: &GameState, rate: u16) -> (i32, i32) {
        let Some(unit) = state.unit(self.unit) else {
            return (0, 0);
        };
        // `Draw(selector->GetLast())`: активная строка, а не обязательно голова
        // очереди завода.
        let Some(unit_type) = self.active_type() else {
            return (0, 0);
        };

        turns_and_consumption(state, unit, unit_type, i32::from(rate.max(1)))
    }

    /// `AddSelection` — добавить выбранный тип в очередь (завод).
    pub fn add_to_queue(&mut self) -> bool {
        let Some(unit_type) = self.selected_type() else {
            return false;
        };

        if !can_build(self.builder_type, unit_type) {
            self.status = BuildError::NotBuildable.message().to_string();
            return false;
        }

        if self.factory {
            if self.queue.len() >= 16 {
                self.status = "build queue is full".to_string();
                return false;
            }

            self.queue.push(unit_type);
        } else {
            self.queue.clear();
            self.queue.push(unit_type);
        }

        self.queue_selected = self.queue.len().saturating_sub(1);
        self.scroll_queue_to_selection();
        self.status.clear();
        true
    }

    /// `RemoveSelection` — убрать выбранный элемент очереди.
    pub fn remove_from_queue(&mut self) -> bool {
        if self.queue.is_empty() {
            return false;
        }

        let index = self.queue_selected.min(self.queue.len() - 1);
        self.queue.remove(index);

        if self.queue_selected >= self.queue.len() {
            self.queue_selected = self.queue.len().saturating_sub(1);
        }

        self.queue_active = !self.queue.is_empty();
        self.scroll_queue_to_selection();
        true
    }

    /// `Repeat` (только завод).
    pub fn toggle_repeat(&mut self) -> bool {
        if !self.factory {
            return false;
        }

        self.repeat = !self.repeat;
        true
    }

    /// `1101..1103` — кнопки 1×/2×/4×.
    pub fn set_rate(&mut self, state: &GameState, rate: u16) -> bool {
        let max_rate = self.max_rate(state);

        if rate > max_rate {
            // C++ `AbstractBuildMenu::Draw`: у Training Hall кнопки 2×/4×
            // выключены, а у мобильного строителя скорость ограничена складом.
            self.status = if self.builder_type == UnitType::TrainingHall {
                format!("{rate}x is not available for Training Hall")
            } else {
                format!("{rate}x needs more storage; max {max_rate}x")
            };
            return false;
        }

        self.build_rate = rate.max(1);
        self.status.clear();
        true
    }

    /// Выбор строки основного списка.
    pub fn select(&mut self, index: usize) {
        if index < self.unit_types.len() {
            self.selected = index;
            self.queue_active = false;
            self.status.clear();
            self.scroll_to_selection();
        }
    }

    /// Выбор строки очереди.
    pub fn select_queue(&mut self, index: usize) {
        if index < self.queue.len() {
            self.queue_selected = index;
            self.queue_active = true;
            self.status.clear();
            self.scroll_queue_to_selection();
        }
    }

    /// Прокрутка списка (`UnitTypeSelector::ScrollTo`).
    fn scroll_to_selection(&mut self) {
        if self.selected < self.page_min {
            self.page_min = self.selected;
        }

        if self.selected >= self.page_min + LIST_ROWS {
            self.page_min = self.selected + 1 - LIST_ROWS;
        }

        if self.page_min + LIST_ROWS > self.unit_types.len() {
            self.page_min = self.unit_types.len().saturating_sub(LIST_ROWS);
        }
    }

    /// Прокрутка очереди.
    fn scroll_queue_to_selection(&mut self) {
        if self.queue_selected < self.queue_page {
            self.queue_page = self.queue_selected;
        }

        if self.queue_selected >= self.queue_page + QUEUE_ROWS {
            self.queue_page = self.queue_selected + 1 - QUEUE_ROWS;
        }

        if self.queue_page + QUEUE_ROWS > self.queue.len() {
            self.queue_page = self.queue.len().saturating_sub(QUEUE_ROWS);
        }
    }

    /// Стрелка вверх/вниз списка.
    pub fn page_list(&mut self, down: bool) {
        if down {
            if self.selected + 1 < self.unit_types.len() {
                self.selected += 1;
            }
        } else {
            self.selected = self.selected.saturating_sub(1);
        }

        self.scroll_to_selection();
    }

    /// Стрелка вверх/вниз очереди.
    pub fn page_queue(&mut self, down: bool) {
        if self.queue.is_empty() {
            return;
        }

        if down {
            if self.queue_selected + 1 < self.queue.len() {
                self.queue_selected += 1;
            }
        } else {
            self.queue_selected = self.queue_selected.saturating_sub(1);
        }

        self.scroll_queue_to_selection();
    }

    /// Клик по экрану — те же кнопки, что в C++.
    pub fn click(&mut self, state: &GameState, x: i32, y: i32) -> BuildMenuAction {
        // Строки списков — первыми: они перекрывают фон, но не кнопки.
        if LIST_AREA.contains(x, y) {
            let row = ((y - LIST_AREA.uly) / LIST_ROW) as usize;
            let index = self.page_min + row;

            if index < self.unit_types.len() {
                self.select(index);
            }

            return BuildMenuAction::None;
        }

        if self.factory && QUEUE_AREA.contains(x, y) {
            let row = ((y - QUEUE_AREA.uly) / LIST_ROW) as usize;
            let index = self.queue_page + row;

            if index < self.queue.len() {
                self.select_queue(index);
            }

            return BuildMenuAction::None;
        }

        let rates = if self.factory {
            FACTORY_RATES
        } else {
            MOBILE_RATES
        };

        for (rate, rect) in BUILD_RATES.iter().zip(rates) {
            if rect.contains(x, y) {
                self.set_rate(state, *rate);
                return BuildMenuAction::None;
            }
        }

        if LIST_UP.contains(x, y) {
            self.page_list(false);
            return BuildMenuAction::None;
        }

        if LIST_DOWN.contains(x, y) {
            self.page_list(true);
            return BuildMenuAction::None;
        }

        if self.factory && QUEUE_UP.contains(x, y) {
            self.page_queue(false);
            return BuildMenuAction::None;
        }

        if self.factory && QUEUE_DOWN.contains(x, y) {
            self.page_queue(true);
            return BuildMenuAction::None;
        }

        if self.factory && FACTORY_ADD.contains(x, y) {
            self.add_to_queue();
            return BuildMenuAction::None;
        }

        if self.factory && QUEUE_DELETE.contains(x, y) {
            self.remove_from_queue();
            return BuildMenuAction::None;
        }

        if self.factory && QUEUE_REPEAT.contains(x, y) {
            self.toggle_repeat();
            return BuildMenuAction::None;
        }

        if DESCRIPTION.contains(x, y) {
            self.show_description = !self.show_description;
            return BuildMenuAction::None;
        }

        if DONE.contains(x, y) {
            return BuildMenuAction::Commit;
        }

        if CANCEL.contains(x, y) {
            return BuildMenuAction::Close;
        }

        BuildMenuAction::None
    }

    /// Клавиши C++ (`1/2/3`, Enter `Build()`, Esc `Cancel`).
    pub fn key(&mut self, state: &GameState, key: crate::ui::Key) -> BuildMenuAction {
        use crate::ui::Key;

        match key {
            Key::Enter => BuildMenuAction::Commit,
            Key::Escape => BuildMenuAction::Close,
            Key::Up => {
                if self.queue_active {
                    self.page_queue(false);
                } else {
                    self.page_list(false);
                }
                BuildMenuAction::None
            }
            Key::Down => {
                if self.queue_active {
                    self.page_queue(true);
                } else {
                    self.page_list(true);
                }
                BuildMenuAction::None
            }
            Key::Tab => {
                self.queue_active = self.factory && !self.queue_active;
                BuildMenuAction::None
            }
            Key::Char('1') => {
                self.set_rate(state, 1);
                BuildMenuAction::None
            }
            Key::Char('2') => {
                self.set_rate(state, 2);
                BuildMenuAction::None
            }
            Key::Char('4') => {
                self.set_rate(state, 4);
                BuildMenuAction::None
            }
            Key::Char('d' | 'D') if self.factory => {
                self.remove_from_queue();
                BuildMenuAction::None
            }
            Key::Char('b' | 'B') if self.factory => {
                self.add_to_queue();
                BuildMenuAction::None
            }
            Key::Char('r' | 'R') => {
                self.toggle_repeat();
                BuildMenuAction::None
            }
            Key::Backspace
            | Key::Left
            | Key::Right
            | Key::PageUp
            | Key::PageDown
            | Key::Other
            | Key::Char(_) => BuildMenuAction::None,
        }
    }

    /// Приказ заказа — `Command::Order` с `PayloadP4` (как в сети).
    ///
    /// Возвращает `None` при пустой очереди/отсутствии площадки (текст в
    /// [`Self::status`]).
    pub fn command(&mut self, state: &GameState) -> Option<Command> {
        self.command_at(state, None)
    }

    /// То же, но с явной клеткой площадки (режим выбора места, `PathBuild`).
    ///
    /// `site == None` — запасной путь ИИ/тестов: площадка ищется
    /// [`find_build_site`] (C++ `TaskCreateBuilding::FindBuildSite`); для
    /// мобильного строителя **не-здания** — его собственная клетка
    /// (`UnitsManager_IssueBuildOrder`).
    pub fn command_at(&mut self, state: &GameState, site: Option<Point>) -> Option<Command> {
        let unit = state.unit(self.unit)?;
        // Завод строит из очереди; мобильный строитель — ровно выбранный тип
        // (`MobileBuildMenu::Build`: `selector->GetLast()`).
        let head = if self.factory {
            self.queue.first().copied()
        } else {
            self.queue.first().copied().or_else(|| self.selected_type())
        };
        let Some(head) = head else {
            self.status = BuildError::EmptyQueue.message().to_string();
            return None;
        };

        if !can_build(unit.unit_type, head) {
            self.status = BuildError::NotBuildable.message().to_string();
            return None;
        }

        let site = if self.factory {
            unit.grid
        } else {
            match site {
                Some(site) => site,
                None if max_core::production::footprint(head) == 1 => unit.grid,
                None => match find_build_site(state, self.team, head, unit.grid) {
                    Some(site) => site,
                    None => {
                        self.status = BuildError::NoSite.message().to_string();
                        return None;
                    }
                },
            }
        };

        let build_rate = self.build_rate.max(1).min(self.max_rate(state).max(1));
        let build_list = if self.factory {
            self.queue.clone()
        } else {
            vec![head]
        };

        build_order_command(
            state,
            self.unit,
            site,
            &build_list,
            self.factory && self.repeat,
            build_rate,
        )
    }

    /// Нужен ли режим выбора места (`MobileBuildMenu::Build`, C++
    /// `(base_unit.GetFlags() & BUILDING)`) — мобильный строитель ставит здание.
    ///
    /// Возвращает голову очереди (что строим) и скорость постройки.
    pub fn site_selection_target(&self, state: &GameState) -> Option<(UnitType, u16)> {
        if self.factory {
            return None;
        }

        let head = self
            .queue
            .first()
            .copied()
            .or_else(|| self.selected_type())?;

        if !can_build(self.builder_type, head) {
            return None;
        }

        if max_core::access::unit_type_flags(head) & max_core::enums::unit_flags::BUILDING == 0 {
            return None;
        }

        let build_rate = self.build_rate.max(1).min(self.max_rate(state).max(1));

        Some((head, build_rate))
    }

    /// Рисует экран поверх кадра 640×480.
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        assets: &Assets,
        state: &GameState,
        mouse: Option<(i32, i32)>,
    ) -> u32 {
        if frame.width() != SCREEN_WIDTH as u32 || frame.height() != SCREEN_HEIGHT as u32 {
            return 0;
        }

        let Some(menu) = assets.build_menu.as_ref() else {
            return 0;
        };
        let background = if self.factory {
            menu.facbuild.as_ref()
        } else {
            menu.conbuild.as_ref()
        };
        // Прозрачные пиксели фона C++ показывает поверх игры; здесь под ними
        // чёрный кадр — иначе сквозь меню просвечивает HUD-строка партии.
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

        // Заголовок окна: `_(3a76)`/`_(faaf)` — «Build»/«Factory Build».
        painted += draw_text_gnw_centered(
            frame,
            HEADER,
            font,
            if self.factory {
                "Factory Build"
            } else {
                "Build"
            },
            green,
        )
        .max(0) as u32;

        // Портрет выбранного типа (`AbstractBuildMenu::Draw(unit_type)`,
        // `buildmenu.cpp:509-529`): `base_unit.GetPortrait()` — big image `P_*`
        // 300×240 в (11,13), палитра окна (`CONBUILD`/`FACBUILD`). C++ берёт
        // `selector->GetLast()` — выбранную строку списка (у завода — активного
        // селектора: список или очередь).
        let portrait_type = self.active_type();

        if let Some(unit_type) = portrait_type {
            if let Some(cargo) = assets.cargo.as_ref() {
                match cargo.portrait(&assets.max_res, unit_type) {
                    Some(portrait) => {
                        painted += crate::menu::draw_big_image_at(
                            frame,
                            &portrait,
                            &palette,
                            (PORTRAIT.ulx, PORTRAIT.uly),
                            PORTRAIT,
                        );
                    }
                    None => {
                        // C++ `buf_fill(local_window.buffer, 300, 240, width, COLOR_BLACK)`.
                        frame.fill_rect(PORTRAIT, palette.rgba(0));
                    }
                }
            }
        }

        // Список доступного (`BuildUnitTypeSelector::Draw` /
        // `UnitTypeSelector::Draw`): иконка `I_*` в центре (16,16) строки,
        // имя с x=35 (ширина 80), ходы справа (`ReportStats_DrawNumber`).
        let builder_unit = state.unit(self.unit);
        let builder_stationary = builder_unit
            .map(|unit| unit.flags & unit_flags::STATIONARY != 0)
            .unwrap_or(self.factory);
        let builder_storage = builder_unit.map(|unit| unit.storage).unwrap_or(0);
        let consumption_rate = cost_per_turn(self.builder_type, 1);
        let lut = team_lut(self.team);
        let cargo = assets.cargo.as_ref();
        let white = palette.rgba(0xFF);

        for row in 0..LIST_ROWS {
            let index = self.page_min + row;
            let Some(unit_type) = self.unit_types.get(index) else {
                break;
            };
            let y = LIST_AREA.uly + row as i32 * LIST_ROW;
            let text_y = center_y(
                PixelRect::new(LIST_AREA.ulx, y, LIST_AREA.width, LIST_ROW),
                font,
            );

            // `ReportStats_DrawListItemIcon`: здания — 1:4 (`divisor = 2`),
            // мобильные — 1:2; перекраска командных слотов как в CargoMenu.
            if let Some(icon) = cargo.and_then(|cargo| cargo.icon(*unit_type)) {
                let stationary =
                    max_core::access::unit_type_flags(*unit_type) & unit_flags::STATIONARY != 0;
                let zoom = if stationary { 16 } else { 32 };

                painted += draw_icon(
                    frame,
                    &palette,
                    *unit_type,
                    icon,
                    (LIST_AREA.ulx + LIST_ICON, y + LIST_ICON),
                    zoom,
                    &lut,
                );
            }

            let name = fit_label(unit_name(*unit_type), LIST_AREA.width - LIST_TEXT_X, font);
            painted += draw_text_gnw(
                frame,
                LIST_AREA.ulx + LIST_TEXT_X,
                text_y,
                font,
                &name,
                text,
            )
            .max(0) as u32;

            // Цвет ходов — `0xA2`, если строитель стационарный или ему хватает
            // склада (`unit->storage >= turns * consumption_rate`), иначе
            // `COLOR_RED` (C++ `BuildUnitTypeSelector::Draw`).
            let turns = self.turns_for(state, *unit_type);
            let affordable =
                builder_stationary || i32::from(builder_storage) >= turns * consumption_rate;
            painted += draw_text_right(
                frame,
                LIST_TURNS_RIGHT,
                text_y,
                font,
                &turns.to_string(),
                if affordable { text } else { palette.rgba(0x01) },
            );

            if index == self.selected {
                // `GameManager_DrawUnitSelector`: белые уголки 8 px вокруг
                // иконки выбранной строки (не заливка и не цвет текста).
                draw_item_selector(frame, white, LIST_AREA.ulx, y);
            }
        }

        // Очередь завода — тот же `UnitTypeSelector::Draw`: иконка + имя.
        if self.factory {
            for row in 0..QUEUE_ROWS {
                let index = self.queue_page + row;
                let Some(unit_type) = self.queue.get(index) else {
                    break;
                };
                let y = QUEUE_AREA.uly + row as i32 * LIST_ROW;
                let text_y = center_y(
                    PixelRect::new(QUEUE_AREA.ulx, y, QUEUE_AREA.width, LIST_ROW),
                    font,
                );

                if let Some(icon) = cargo.and_then(|cargo| cargo.icon(*unit_type)) {
                    let stationary =
                        max_core::access::unit_type_flags(*unit_type) & unit_flags::STATIONARY != 0;
                    let zoom = if stationary { 16 } else { 32 };

                    painted += draw_icon(
                        frame,
                        &palette,
                        *unit_type,
                        icon,
                        (QUEUE_AREA.ulx + LIST_ICON, y + LIST_ICON),
                        zoom,
                        &lut,
                    );
                }

                let name = fit_label(unit_name(*unit_type), QUEUE_AREA.width - LIST_TEXT_X, font);
                painted += draw_text_gnw(
                    frame,
                    QUEUE_AREA.ulx + LIST_TEXT_X,
                    text_y,
                    font,
                    &name,
                    text,
                )
                .max(0) as u32;

                if index == self.queue_selected {
                    draw_item_selector(frame, white, QUEUE_AREA.ulx, y);
                }
            }
        }

        // Характеристики выбранного типа с иконками — `UnitStats_DrawStats`
        // в `stats_background` (11,293,250,174), как в
        // `AbstractBuildMenu::Draw` (`buildmenu.cpp:531-536`): база/текущие
        // значения команды (клановые модификаторы и апгрейды уже внутри),
        // `I_RAW`/`I_RAWE` для строки `Cost`. Это тот же код, что в CargoMenu.
        if let Some(unit_type) = portrait_type {
            if let (Some(cargo), Some(team_units)) = (
                assets.cargo.as_ref(),
                state.team_units.get(self.team.0 as usize),
            ) {
                let rows = crate::mobilize::Mobilization::stats_rows_for(
                    &team_units.base_values,
                    &team_units.current_values,
                    unit_type,
                );
                painted += crate::mobilize::draw_stats(frame, cargo, &palette, &rows);
            }
        }

        // Описание выбранного типа — поверх портрета (`Text_TextBox` 16,17,
        // 290×230, `button_description_rest_state`).
        if self.show_description {
            if let Some(unit_type) = portrait_type {
                painted += crate::mobilize::draw_description(frame, &palette, unit_type);
            }
        }

        // Цифры ходов/цены под списком (C++ `turns_background_*`).
        let labels_y = if self.factory { 357 } else { 330 };
        let first_y = if self.factory { 376 } else { 349 };
        let max_rate = self.max_rate(state);
        let build_rate = self.build_rate.min(max_rate.max(1));

        painted += draw_text_gnw(frame, 374, labels_y - 2, font, "Turns", dim).max(0) as u32;
        painted += draw_text_gnw(frame, 410, labels_y - 2, font, "Cost", dim).max(0) as u32;

        for (index, rate) in BUILD_RATES.iter().enumerate() {
            // C++ `AbstractBuildMenu::Draw`: строки 2×/4× не рисуются, если
            // скорость недоступна (`max_build_rate < rate`) или это Training
            // Hall (кнопки выключены безусловно).
            if *rate > 1 && (self.builder_type == UnitType::TrainingHall || *rate > max_rate) {
                continue;
            }

            let y = first_y + index as i32 * 25;
            let (turns, cost) = self.rate_numbers(state, *rate);
            let active = build_rate == *rate;
            let color = if active { green } else { yellow };

            painted += draw_text_gnw(frame, 378, y, font, &turns.to_string(), color).max(0) as u32;
            painted += draw_text_gnw(frame, 414, y, font, &cost.to_string(), color).max(0) as u32;
        }

        if self.factory {
            painted += draw_text_gnw(frame, 390, 325, font, "Repeat", dim).max(0) as u32;
            painted += draw_text_gnw(
                frame,
                470,
                325,
                font,
                if self.repeat { "on" } else { "off" },
                if self.repeat { green } else { text },
            )
            .max(0) as u32;
        }

        // Статус/ошибка — нижняя строка (до кнопок `Cancel`, x=307).
        let status = if self.status.is_empty() {
            "Enter: build  Esc: cancel  1/2/3: speed"
        } else {
            self.status.as_str()
        };
        painted += draw_text_gnw(
            frame,
            11,
            466,
            font,
            &status.chars().take(38).collect::<String>(),
            if self.status.is_empty() { dim } else { yellow },
        )
        .max(0) as u32;

        // Кнопки.
        let rates = if self.factory {
            FACTORY_RATES
        } else {
            MOBILE_RATES
        };
        let rate_pairs = [&menu.build, &menu.build2x, &menu.build4x];
        let rate_labels = ["1x", "2x", "4x"];

        for (index, ((pair, rect), label)) in rate_pairs
            .iter()
            .zip(rates)
            .zip(rate_labels)
            .enumerate()
        {
            painted += draw_button(
                frame,
                &palette,
                pair.as_ref(),
                rect,
                label,
                mouse,
                build_rate == BUILD_RATES[index],
            );
        }

        painted += draw_button(frame, &palette, menu.up.as_ref(), LIST_UP, "", mouse, false);
        painted += draw_button(frame, &palette, menu.down.as_ref(), LIST_DOWN, "", mouse, false);

        if self.factory {
            painted += draw_button(frame, &palette, menu.up.as_ref(), QUEUE_UP, "", mouse, false);
            painted += draw_button(frame, &palette, menu.down.as_ref(), QUEUE_DOWN, "", mouse, false);
            painted += draw_button(
                frame,
                &palette,
                menu.delete.as_ref(),
                QUEUE_DELETE,
                "Del",
                mouse,
                false,
            );
            painted += draw_button(
                frame,
                &palette,
                menu.repeat.as_ref(),
                QUEUE_REPEAT,
                "",
                mouse,
                self.repeat,
            );
            painted += draw_button(
                frame,
                &palette,
                menu.build.as_ref(),
                FACTORY_ADD,
                "Build",
                mouse,
                false,
            );
        }

        painted += draw_button(
            frame,
            &palette,
            menu.description.as_ref(),
            DESCRIPTION,
            "",
            mouse,
            self.show_description,
        );
        painted += draw_button(frame, &palette, menu.done.as_ref(), DONE, "Done", mouse, false);
        painted += draw_button(
            frame,
            &palette,
            menu.cancel.as_ref(),
            CANCEL,
            "Cancel",
            mouse,
            false,
        );

        painted
    }
}

/// Маркер выбранной строки — `GameManager_DrawUnitSelector`
/// (`game_manager.cpp`): белые уголки длиной 8 px вокруг 32×32-иконки. В C++
/// `offsetx = 0`, `item_height = 32`, `scaled_size2 = 30`, поэтому уголки
/// стоят по x/y 0..8 и 22..30, а не по всей строке.
fn draw_item_selector(frame: &mut RgbaBuffer, color: Rgba, x: i32, y: i32) {
    const SIZE: i32 = 8;
    const EDGE: i32 = 30;

    for offset in 0..SIZE {
        // Верхний левый и верхний правый.
        frame.set(x + offset, y, color);
        frame.set(x, y + offset, color);
        frame.set(x + EDGE - offset, y, color);
        frame.set(x + EDGE, y + offset, color);
        // Нижний левый и нижний правый.
        frame.set(x + offset, y + EDGE, color);
        frame.set(x, y + EDGE - offset, color);
        frame.set(x + EDGE - offset, y + EDGE, color);
        frame.set(x + EDGE, y + EDGE - offset, color);
    }
}

/// Кнопка: картинка C++ или честная рамка с подписью (как `mobilize.rs`).
fn draw_button(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    pair: Option<&MenuButtonPair>,
    rect: PixelRect,
    label: &str,
    mouse: Option<(i32, i32)>,
    active: bool,
) -> u32 {
    let hovered = mouse.is_some_and(|(x, y)| rect.contains(x, y));

    if let Some(pair) = pair {
        return crate::menu::draw_image_button(
            frame,
            palette,
            pair.image(active),
            pair.image(true),
            rect,
            label,
            hovered,
        );
    }

    frame.fill_rect(rect, palette.rgba(crate::setup::TEXT));
    let mut painted = 1;

    if !label.is_empty() {
        draw_text_gnw_centered(
            frame,
            rect,
            GnwFontId::Font1.font(),
            label,
            palette.rgba(crate::setup::GREEN),
        );
        painted += 1;
    }

    painted
}

/// Обрезка длинного имени по ширине (как `mobilize::fit_label`).
fn fit_label(text: &str, max_width: i32, font: &max_render::text::GnwFont) -> String {
    if font.text_width(text) <= max_width {
        return text.to_string();
    }

    let mut out = text.to_string();

    while !out.is_empty() && font.text_width(&format!("{out}..")) > max_width {
        out.pop();
    }

    format!("{out}..")
}

// ---------------------------------------------------------------------------
// Интеграция с приложением
// ---------------------------------------------------------------------------

impl crate::App {
    /// Модальный ввод `BuildMenu` (C++ `AbstractBuildMenu::Run`).
    ///
    /// `None` — экран не открыт (ввод идёт в HUD). Пока экран открыт, карта и
    /// панели приказов не получают: `GameManager_DisableMainMenu`.
    pub fn build_menu_input(
        &mut self,
        event: crate::ui::InputEvent,
    ) -> Option<BuildMenuAction> {
        let mut menu = self.build_menu.take()?;

        let action = match event {
            crate::ui::InputEvent::MouseMove(x, y) => {
                self.hud.mouse_position = Some((x, y));
                BuildMenuAction::None
            }
            crate::ui::InputEvent::MouseClick(x, y) => menu.click(&self.state, x, y),
            // В C++ правый клик в меню не назначен (`ProcessKey` не ловит
            // `RMB`); здесь он закрывает экран, как `Esc`.
            crate::ui::InputEvent::RightClick(..) => BuildMenuAction::Close,
            crate::ui::InputEvent::Key(key) => menu.key(&self.state, key),
        };

        self.build_menu = Some(menu);
        Some(action)
    }

    /// `Build()` — зафиксировать заказ (`Done`/Enter).
    ///
    /// Возвращает строку для журнала HUD: что построить или почему нельзя.
    ///
    /// Мобильный строитель и здание (`MobileBuildMenu::Build`, C++
    /// `(base_unit.GetFlags() & BUILDING)`) — заказ **ещё не уходит**:
    /// открывается режим выбора места (`ORDER_STATE_SELECT_SITE`), площадку
    /// ставит [`crate::App::confirm_build_site`] по LMB. Завод (и не-здания
    /// инженера) идут прежним путём — сразу в очередь.
    pub fn commit_build_menu(&mut self, online: bool) -> Option<String> {
        let mut menu = self.build_menu.take()?;

        // `UnitsManager_IssueBuildOrder`: рядом со строителем должен быть
        // проходимый якорь 2×2, иначе `BuildMenu` остаётся открытым с
        // сообщением «Invalid square» (`_(1031)`).
        if let Some((unit_type, build_rate)) = menu.site_selection_target(&self.state) {
            self.build_menu = Some(menu);

            if self.open_build_site(unit_type, build_rate) {
                let site = self.build_site.as_ref().map(|site| site.anchor);
                let (x, y) = site.map_or((0, 0), |site| (site.x, site.y));

                return Some(format!(
                    "Build: {} — choose a site (ghost at {x},{y}; LMB places, Esc/RMB cancels)",
                    unit_name(unit_type)
                ));
            }

            let Some(menu) = self.build_menu.as_mut() else {
                return Some(BuildError::NoSite.message().to_string());
            };
            menu.status = format!("{} ({} — no free 2x2 site)", BuildError::NoSite.message(), unit_name(unit_type));

            return Some(menu.status.clone());
        }

        let command = menu.command(&self.state);
        let status = menu.status.clone();
        let head = menu.queue.first().copied().or_else(|| menu.selected_type());
        let build_rate = menu.build_rate;
        let queue_len = menu.queue.len();
        self.build_menu = Some(menu);

        match command {
            Some(command) => {
                if online {
                    self.hud.pending.push(command);
                } else {
                    self.apply(command);
                }

                self.build_menu = None;

                Some(match head {
                    Some(unit_type) => format!(
                        "Build order: {} ({queue_len} in queue, {build_rate}x)",
                        unit_name(unit_type)
                    ),
                    None => "Build order".to_string(),
                })
            }
            None => Some(status),
        }
    }

    /// Выбор строки списка в открытом меню (QA/мышь).
    pub fn build_menu_select(&mut self, index: usize) -> bool {
        let Some(menu) = self.build_menu.as_mut() else {
            return false;
        };
        let before = menu.selected;
        menu.select(index);
        menu.selected != before || index < menu.unit_types.len()
    }

    /// `AddSelection` (`Build` у завода).
    pub fn build_menu_add(&mut self) -> bool {
        let Some(mut menu) = self.build_menu.take() else {
            return false;
        };
        let added = menu.add_to_queue();
        self.build_menu = Some(menu);
        added
    }

    /// `RemoveSelection` (`Delete`).
    pub fn build_menu_delete(&mut self) -> bool {
        let Some(mut menu) = self.build_menu.take() else {
            return false;
        };
        let removed = menu.remove_from_queue();
        self.build_menu = Some(menu);
        removed
    }

    /// Выбор скорости 1×/2×/4×.
    pub fn build_menu_set_rate(&mut self, rate: u16) -> bool {
        let Some(mut menu) = self.build_menu.take() else {
            return false;
        };
        let applied = menu.set_rate(&self.state, rate);
        self.build_menu = Some(menu);
        applied
    }

    /// `Repeat` (только завод).
    pub fn build_menu_toggle_repeat(&mut self) -> bool {
        let Some(menu) = self.build_menu.as_mut() else {
            return false;
        };
        menu.toggle_repeat()
    }

    /// Стрелки списка (`true` — вниз).
    pub fn build_menu_step(&mut self, down: bool) -> bool {
        let Some(menu) = self.build_menu.as_mut() else {
            return false;
        };
        menu.page_list(down);
        true
    }

    /// Стрелки очереди (`true` — вниз).
    pub fn build_menu_queue_step(&mut self, down: bool) -> bool {
        let Some(menu) = self.build_menu.as_mut() else {
            return false;
        };
        menu.page_queue(down);
        true
    }

    /// Очередь открытого меню (копия).
    pub fn build_menu_queue(&self) -> Vec<UnitType> {
        self.build_menu
            .as_ref()
            .map(|menu| menu.queue.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::TeamUnits;

    fn state() -> GameState {
        let mut state = GameState::new();
        state.teams[0].team_type = max_core::TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(max_core::TeamClan::Random);
        state.team_units[0].set_gold(150);
        state
    }

    #[test]
    fn mobile_menu_lists_buildings_and_queues_one() {
        let mut state = state();
        let constructor = state
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
            .expect("constructor");
        let mut menu = BuildMenu::new(&state, constructor).expect("constructor builds");

        assert!(!menu.factory);
        assert_eq!(menu.unit_types.len(), 15);
        assert_eq!(menu.selected_type(), Some(UnitType::MiningStation));

        // Выбор TRAINHAL (индекс 13 в списке CONSTRCT).
        let index = menu
            .unit_types
            .iter()
            .position(|unit| *unit == UnitType::TrainingHall)
            .expect("training hall buildable");
        menu.select(index);
        assert!(menu.add_to_queue());
        assert_eq!(menu.queue, vec![UnitType::TrainingHall]);

        // Второй выбор заменяет (MobileBuildMenu::Build: Clear + PushBack).
        menu.select(0);
        assert!(menu.add_to_queue());
        assert_eq!(menu.queue, vec![UnitType::MiningStation]);

        let command = menu.command(&state).expect("build command");
        let Command::Order(packet) = command else {
            panic!("expected order");
        };
        let OrderPayload::P4(p4) = packet.payload else {
            panic!("expected P4 payload");
        };
        assert_eq!(p4.p3.p2.p1.order, UnitOrder::Build);
        assert_eq!(p4.build_list, vec![UnitType::MiningStation]);
        assert_eq!(p4.build_time, 12, "MININGST: ATTRIB_TURNS = 12");
        assert_ne!(p4.p3.move_to, Point::new(10, 10), "площадка не под юнитом");
    }

    #[test]
    fn factory_menu_builds_a_queue_and_repeats() {
        let mut state = state();
        let factory = state
            .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
            .expect("factory");
        let mut menu = BuildMenu::new(&state, factory).expect("factory builds");

        assert!(menu.factory);
        assert_eq!(menu.unit_types.len(), 2);
        menu.select(1); // INFANTRY
        assert!(menu.add_to_queue());
        menu.select(0); // COMMANDO
        assert!(menu.add_to_queue());
        assert_eq!(
            menu.queue,
            vec![UnitType::Infantry, UnitType::Commando],
            "порядок очереди — порядок добавления"
        );

        assert!(menu.toggle_repeat());
        let command = menu.command(&state).expect("command");
        let Command::Order(packet) = command else {
            panic!("expected order");
        };
        let OrderPayload::P4(p4) = packet.payload else {
            panic!("expected P4 payload");
        };
        assert!(p4.repeat_build);
        assert_eq!(p4.build_list, menu.queue);
        assert_eq!(p4.p3.move_to, Point::new(10, 10), "завод строит на месте");
    }

    #[test]
    fn cancel_closes_without_a_command() {
        let mut state = state();
        let constructor = state
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
            .expect("constructor");
        let mut menu = BuildMenu::new(&state, constructor).expect("menu");

        assert_eq!(
            menu.click(&state, CANCEL.ulx + 1, CANCEL.uly + 1),
            BuildMenuAction::Close
        );
        assert_eq!(
            menu.key(&state, crate::ui::Key::Escape),
            BuildMenuAction::Close
        );
        // Cancel сам по себе не меняет выбор: заказ существует только после
        // `Done` (C++ `event_click_cancel` не вызывает `Build()`).
        assert!(menu.queue.is_empty());
        let command = menu.command(&state).expect("выбранный тип — это заказ");
        let Command::Order(packet) = command else {
            panic!("expected order");
        };
        let OrderPayload::P4(p4) = packet.payload else {
            panic!("expected P4 payload");
        };
        assert_eq!(p4.build_list, vec![menu.selected_type().expect("selected")]);
    }

    #[test]
    fn rows_scroll_with_the_arrows() {
        let mut state = state();
        let constructor = state
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
            .expect("constructor");
        let mut menu = BuildMenu::new(&state, constructor).expect("menu");

        for _ in 0..LIST_ROWS {
            menu.page_list(true);
        }

        assert_eq!(menu.selected, LIST_ROWS);
        assert!(menu.page_min <= menu.selected);
        assert_eq!(
            menu.click(&state, LIST_UP.ulx + 1, LIST_UP.uly + 1),
            BuildMenuAction::None
        );
        assert_eq!(menu.selected, LIST_ROWS - 1);

        let down_row = 3;
        menu.click(
            &state,
            LIST_AREA.ulx + 2,
            LIST_AREA.uly + down_row * LIST_ROW + 2,
        );
        assert_eq!(menu.selected, menu.page_min + down_row as usize);
    }

    #[test]
    fn non_builders_have_no_menu() {
        let mut state = state();
        let tank = state
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
            .expect("tank");

        assert!(BuildMenu::new(&state, tank).is_none());
    }

    #[test]
    fn rate_numbers_are_turns_and_total_cost_like_get_turns_to_build() {
        let mut state = state();
        let factory = state
            .spawn_unit(
                UnitType::HeavyVehiclePlant,
                TeamId::RED,
                Point::new(10, 10),
                0,
            )
            .expect("factory");
        let mut menu = BuildMenu::new(&state, factory).expect("factory menu");
        let tank = menu
            .unit_types
            .iter()
            .position(|unit| *unit == UnitType::Tank)
            .expect("TANK in HVP list");
        menu.select(tank);

        // TANK = 4 хода; завод 3/ход на 1×, 12 на 2×, 36 на 4×. Второе число —
        // полная стоимость (сумма ходов), а не ставка за ход.
        assert_eq!(menu.max_rate(&state), 4);
        assert_eq!(menu.rate_numbers(&state, 1), (4, 12));
        assert_eq!(menu.rate_numbers(&state, 2), (2, 24));
        assert_eq!(menu.rate_numbers(&state, 4), (1, 36));
    }

    #[test]
    fn training_hall_is_locked_to_1x_like_the_cpp_buttons() {
        let mut state = state();
        let hall = state
            .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
            .expect("training hall");
        let mut menu = BuildMenu::new(&state, hall).expect("factory menu");
        let infantry = menu
            .unit_types
            .iter()
            .position(|unit| *unit == UnitType::Infantry)
            .expect("INFANTRY in TRAINHAL list");
        menu.select(infantry);

        // C++ `AbstractBuildMenu::Draw`: `build_rate_x2/x4->Enable(false)`.
        assert_eq!(menu.max_rate(&state), 1);
        // INFANTRY = 2 хода, ставка TRAINHAL 1/ход.
        assert_eq!(menu.rate_numbers(&state, 1), (2, 2));
    }

    #[test]
    fn clan_turns_change_the_build_menu_cost() {
        let mut random = state();
        let builder = random
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
            .expect("constructor");
        // В реальной партии `deploy_starting_units` ставит `storage = cargo`.
        random.unit_mut(builder).unwrap().storage = 0;
        let mut menu = BuildMenu::new(&random, builder).expect("menu");
        let shipyard = menu
            .unit_types
            .iter()
            .position(|unit| *unit == UnitType::Shipyard)
            .expect("SHIPYARD in CONSTRCT list");
        menu.select(shipyard);
        assert_eq!(menu.max_rate(&random), 1);
        assert_eq!(menu.rate_numbers(&random, 1), (20, 40));

        // CrimsonPath: SHIPYARD -5 ходов (`CLAN_TRADEOFFS`), цена 15 × 2.
        random.team_units[0] = TeamUnits::with_clan(max_core::TeamClan::CrimsonPath);
        random.unit_mut(builder).unwrap().storage = 0;
        let mut menu = BuildMenu::new(&random, builder).expect("menu");
        menu.select(shipyard);
        assert_eq!(menu.rate_numbers(&random, 1), (15, 30));
    }

    #[test]
    fn factory_rate_numbers_follow_the_active_selection_not_the_queue_head() {
        let mut state = state();
        let factory = state
            .spawn_unit(
                UnitType::HeavyVehiclePlant,
                TeamId::RED,
                Point::new(10, 10),
                0,
            )
            .expect("factory");
        let mut menu = BuildMenu::new(&state, factory).expect("factory menu");
        let tank = menu
            .unit_types
            .iter()
            .position(|unit| *unit == UnitType::Tank)
            .expect("TANK in HVP list");
        let launcher = menu
            .unit_types
            .iter()
            .position(|unit| *unit == UnitType::RocketLauncher)
            .expect("ROCKET in HVP list");

        // В очереди TANK, но выбран ROCKET: C++ `Draw(selector->GetLast())`
        // показывает цифры выбранной строки (4 хода TANK — только когда
        // активна очередь).
        menu.queue.push(UnitType::Tank);
        menu.select(launcher);
        let turns = menu.turns_for(&state, UnitType::RocketLauncher);
        assert_eq!(menu.rate_numbers(&state, 1), (turns, turns * 3));

        menu.queue_selected = 0;
        menu.queue_active = true;
        assert_eq!(menu.rate_numbers(&state, 1), (4, 12));
        assert_eq!(menu.rate_numbers(&state, 2), (2, 24));
        assert_eq!(menu.rate_numbers(&state, 4), (1, 36));

        // Возврат к списку — снова выбранная строка.
        menu.queue_active = false;
        menu.select(tank);
        assert_eq!(menu.rate_numbers(&state, 1), (4, 12));
    }

    #[test]
    fn stats_rows_for_a_building_carry_icons_and_the_cpp_cost() {
        let state = state();
        let team_units = &state.team_units[0];
        let rows = crate::mobilize::Mobilization::stats_rows_for(
            &team_units.base_values,
            &team_units.current_values,
            UnitType::TrainingHall,
        );
        let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();

        assert!(labels.contains(&"Hits"), "характеристики: {labels:?}");
        assert!(labels.contains(&"Armor"), "характеристики: {labels:?}");
        assert_eq!(labels.last(), Some(&"Cost"), "строка Cost последняя");

        // `UnitStats_DrawCost`: ставка строителя CONSTRCT (2) × ATTRIB_TURNS (10).
        let cost = rows.last().expect("cost row");
        assert_eq!(cost.value, Some(20));
        assert_eq!(cost.icons.len(), 1);
        assert_eq!(cost.icons[0].resource, "I_GOLD");
        assert_eq!(cost.icons[0].empty_resource, "I_GOLDE");
        assert_eq!(cost.icons[0].current, 20);
    }
}
