//! Меню команд выделенного юнита (попап `UNTBTN_U/D`) — 1:1 к C++-порту.
//!
//! Оракул: `max-cpp-port/src/game_manager.cpp`
//!
//! * `GameManager_InitPopupButtons` (`game_manager.cpp:3568`) —
//!   условия открытия, раскладка, картинки/подписи кнопок;
//! * `GameManager_DeinitPopupButtons` (`:3664`) — закрытие;
//! * `GameManager_ProcessPopupMenuInput` (`:2607`) — клавиши: символ позиции
//!   или первая буква подписи;
//! * `GameManager_ClickUnit` (`:6265`) / `GameManager_UnitSelect` (`:6246`) —
//!   триггер (повторный клик по уже выделенному юниту);
//! * `game_manager.cpp:5455` (`Esc`) — закрытие меню;
//! * `units_manager.cpp:543-1352` — состав пунктов:
//!   `UnitsManager_Popup_InitCommons` и `UnitsManager_Popup_Init*` по типу
//!   юнита (`UnitInfo::Init`, `unitinfo.cpp:454-582` — таблица
//!   `UnitsManager_PopupCallbacks[0..22]`).
//!
//! # Триггер (точный, из C++)
//!
//! Меню открывается **повторным кликом по уже выделенному своему юниту**:
//! `GameManager_ClickUnit` получает `unit1 == unit2` (клик по тому же юниту) и
//! вызывает `GameManager_InitPopupButtons`; если меню уже открыто — закрывает
//! (`DeinitPopupButtons`). Левая кнопка по другому своему юниту только
//! перевыделяет. Правая кнопка меню **не** открывает: по выделенному юниту она
//! открывает статистику (`UnitStats_Menu`), по другому — выделяет.
//!
//! `GameManager_InitPopupButtons` требует:
//!
//! * `unit->GetOrder() != ORDER_DISABLE`;
//! * юнит внутри видимой области карты;
//! * для `MOBILE_LAND_UNIT` — состояние не `ORDER_STATE_UNIT_READY`;
//! * `popup->init` зарегистрировал хотя бы один пункт.
//!
//! # Раскладка (по C++)
//!
//! Вертикальный список кнопок `UNTBTN_U` (42×21, `BUTTON_WIDTH`×`BUTTON_HEIGHT`)
//! с шагом `height + 3`; окно — `image.width + 6` × `image.height * count +
//! (count + 1) * 3`, то есть 3 px поля и 3 px между кнопками. Якорь:
//! `ulx = unit->x + 32` (правый край клетки юнита), `uly = unit->y - height/2`
//! (центр клетки по Y; `unit->x/y` — центр клетки, `UnitInfo::SetPosition`).
//! Если меню вылезает за правый край окна карты — сдвиг влево на
//! `width + Gfx_ZoomLevel` (одна клетка); по вертикали — в пределах окна карты.
//!
//! Подпись — `Button::SetCaption(caption, -1, 2)`: шрифт `GNW_TEXT_FONT_2`,
//! цвет `COLOR_GREEN` (`0x02`), по центру кнопки; нажатый пункт рисуется
//! картинкой `UNTBTN_D` (C++: `state[i]` → `image_up = UNTBTN_D`).
//!
//! # Ввод
//!
//! * мышь: клик по кнопке — пункт (C++ `Button` с `r_func`);
//! * клавиатура: `toupper(позиция)` или `toupper(первая буква подписи)`
//!   (`GameManager_ProcessPopupMenuInput`); стрелки/Enter в C++ **не**
//!   работают — здесь тоже;
//! * `Esc` — закрыть (C++ `game_manager.cpp:5469`);
//! * второй клик по юниту — закрыть (C++ `GameManager_ClickUnit`);
//! * клик вне меню — закрыть (меню модально: карта приказ не получает).
//!
//! # Состав пунктов
//!
//! Порядок пунктов — не порядок регистрации, а `UnitsManager_RegisterButton`:
//! вставка по возрастанию `key_code` (символ позиции, `'0'` → `':'`, чтобы
//! «remove»/«place» оказались последними); равные позиции сохраняют порядок
//! регистрации (устойчивая вставка). Поэтому у танка меню идёт
//! `attack → x-fer → enter → stop → sentry → Done`, а у строителя —
//! `Build → …`.
//!
//! # Реализовано / отключено
//!
//! Пункт, механика которого в нашем стеке не портирована, остаётся в списке
//! (как в C++), но рисуется серым и не срабатывает — причина в
//! [`UnitMenuItem::note`]. С 2026-11-20 (B1) включены `attack` (targeting_mode),
//! `load` (курсорный режим 7), `disable`/`steal` (режимы 9/8), бульдозер
//! (`ORDER_CLEAR`), `remove` (`UnitsManager_SelfDestructMenu`) и `Upg. All`.
//! Отключён только `activate` (`RepairShopMenu` не портирован); мастер-строитель
//! пункт не регистрирует (`FindValidPowerGeneratorPosition`). Список причин и
//! ссылки — `docs/analysis/visual-1to1-status.md` §33.
//!
//! # Осознанные расхождения
//!
//! 1. У стационарного юнита C++ открывает меню уже при выделении
//!    (`GameManager_UnitSelect` → `InitPopupButtons`). У нас выделение кадр не
//!    меняет (golden `mine-selected` и др.), поэтому меню открывается только
//!    повторным кликом — как `GameManager_ClickUnit`, см. триггер выше.
//! 2. `Done` (`UnitsManager_PerformAction`) — приказы в нашем стеке
//!    исполняются сразу при выдаче, отдельного шага «подтвердить» нет: пункт
//!    закрывает меню и снимает выделение (плюс переводит `Await` +
//!    `STATIONARY` в `ORDER_STATE_READY_TO_EXECUTE_ORDER`, как C++).
//! 3. Курсорные режимы C++ (`unit->cursor`: x-fer 3, repair 4, reload 6,
//!    load 7) живут в UI-состоянии ([`crate::hud::HudState`]), а не в
//!    `UnitInfo`: `UnitInfo::cursor` не входит в снапшот, но и не должен
//!    влиять на дайджест. Переключение режима — как C++ (`cursor = N`).

use max_assets::image::SimpleImage;
use max_core::enums::{unit_flags, PlayMode, UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::Point;
use max_core::unit::{UnitId, UnitInfo};
use max_core::unit_values::{
    ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_HITS, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN,
    ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS,
};
use max_core::victory::{cargo_type_of, CargoType};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, GnwFontId};

use crate::panels::UiPalette;
use crate::ui::Key;
use crate::Assets;

/// Ширина кнопки попала (`UNTBTN_U`/`UNTBTN_D`, `MAX.RES`): 42.
pub const BUTTON_WIDTH: i32 = 42;
/// Высота кнопки попапа (`UNTBTN_U`/`UNTBTN_D`, `MAX.RES`): 21.
pub const BUTTON_HEIGHT: i32 = 21;
/// Поле окна и зазор между кнопками (`(count + 1) * 3` в C++): 3.
pub const MARGIN: i32 = 3;

/// Цвет подписи — `COLOR_GREEN` (`color.h:38`, `SetCaption(..., -1, 2)`).
pub const CAPTION_GREEN: u8 = 0x02;
/// Цвет отключённого пункта (в C++ выключенная кнопка не реагирует и рисуется
/// той же картинкой; нам нужен видимый признак — серый `0xA2`).
pub const CAPTION_DISABLED: u8 = 0xA2;

/// Курсорный режим `x-fer` — C++ `unit->cursor = 3` (`CURSOR_TRANSFER`).
pub const CURSOR_TRANSFER: u8 = 3;
/// Курсорный режим `repair` — C++ `unit->cursor = 4` (`CURSOR_REPAIR`).
pub const CURSOR_REPAIR: u8 = 4;
/// Курсорный режим `reload` — C++ `unit->cursor = 6` (`CURSOR_RELOAD`).
pub const CURSOR_RELOAD: u8 = 6;
/// Курсорный режим `load` — C++ `unit->cursor = 7` (`CURSOR_LOAD`).
pub const CURSOR_LOAD: u8 = 7;

/// Идентификатор пункта меню (все пункты C++ попапа).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemId {
    /// `Manual` (`_(b0b9)`) — тумблер ответного огня в simultaneous moves.
    Manual,
    /// `x-fer` (`_(886b)`) — курсорный режим переноса груза.
    Transfer,
    /// `enter` (`_(e08d)`) — режим посадки в транспорт/на платформу.
    Enter,
    /// `stop` (`_(6c45)`) — отмена маршрута (`ORDER_AWAIT` + `CLEAR_PATH`).
    Stop,
    /// `upgrade` (`_(7fef)`) — приказ `ORDER_UPGRADE`.
    Upgrade,
    /// `Upg. All` (`_(97a2)`) — апгрейд всех зданий комплекса.
    UpgradeAll,
    /// `sentry` (`_(0cf6)`/`_(524e)`) — `ORDER_SENTRY` / `ORDER_AWAIT`.
    Sentry,
    /// `Done` (`_(2b7d)`) — `UnitsManager_PerformAction`.
    Done,
    /// `remove` (`_(7e56)`) — самоуничтожение здания (`SelfDestructMenu`).
    RemoveStationary,
    /// `Auto` (`_(0286)`) — авто-разведка сюрвейера.
    Auto,
    /// `attack` (`_(e183)`/`_(5f6b)`) — режим наведения.
    Attack,
    /// `disable` (`_(6d6a)`) — диверсант: отключение цели.
    Disable,
    /// `steal` (`_(a0fd)`) — диверсант: захват цели.
    Steal,
    /// `reload` (`_(9d39)`) — курсорный режим дозаправки/дозарядки.
    Reload,
    /// `place` (`_(81df)`) — постановка мины.
    PlaceMine,
    /// `remove` (`_(bfe6)`) — снятие мины.
    RemoveMine,
    /// `repair` (`_(a1a1)`) — курсорный режим ремонта.
    Repair,
    /// `Build` (`_(7b6d)`/`_(b3ef)`) — экран заказа.
    Build,
    /// `stop` (`_(bf85)`/`_(bcf7)`) — отмена стройки (`GameManager_SelectBuildSite`).
    StopBuild,
    /// `start` (`_(cced)`) — продолжение стройки завода (`BuildOrder`).
    StartBuild,
    /// `start` (`_(e7b6)`/`_(283e)`/`_(98c9)`/`_(53c3)`/`_(bd97)`/`_(5fe9)`).
    PowerOn,
    /// `stop` (`_(7a07)`/`_(6889)`/`_(d9c8)`/`_(3f13)`/`_(df26)`).
    PowerOff,
    /// `resrch` (`_(2e00)`) — экран исследований.
    Research,
    /// `buy upg` (`_(1da0)`) — экран апгрейдов.
    BuyUpgrade,
    /// `allocate` (`_(33b7)`) — распределение шахты (`AllocMenu`).
    Allocate,
    /// `activate` (`_(200c)`/`_(cec1)`) — `RepairShopMenu`.
    Activate,
    /// `load` (`_(8452)`) — курсорный режим погрузки в транспорт.
    Load,
    /// `stop` (`_(bc20)`) — отмена расчистки бульдозером.
    StopRemove,
    /// `remove` (`_(9737)`) — расчистка завалов бульдозером.
    RemoveRubble,
    /// `start` (`_(2c2a)`) — мастер-строитель (`UnitsManager_FindValidPowerGeneratorPosition`).
    StartMaster,
}

impl ItemId {
    /// Имя пункта для журнала/QA (совпадает с подписью в en-US, кроме
    /// `Upg. All`/`buy upg` — там пробел заменён `_`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Transfer => "x-fer",
            Self::Enter => "enter",
            Self::Stop => "stop",
            Self::Upgrade => "upgrade",
            Self::UpgradeAll => "Upg. All",
            Self::Sentry => "sentry",
            Self::Done => "Done",
            Self::RemoveStationary => "remove",
            Self::Auto => "Auto",
            Self::Attack => "attack",
            Self::Disable => "disable",
            Self::Steal => "steal",
            Self::Reload => "reload",
            Self::PlaceMine => "place",
            Self::RemoveMine => "remove",
            Self::Repair => "repair",
            Self::Build => "Build",
            Self::StopBuild => "stop build",
            Self::StartBuild => "start",
            Self::PowerOn => "power on",
            Self::PowerOff => "power off",
            Self::Research => "resrch",
            Self::BuyUpgrade => "buy upg",
            Self::Allocate => "allocate",
            Self::Activate => "activate",
            Self::Load => "load",
            Self::StopRemove => "stop remove",
            Self::RemoveRubble => "remove rubble",
            Self::StartMaster => "master",
        }
    }
}

/// Пункт меню — кнопка `UNTBTN_U/D` с подписью.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitMenuItem {
    /// Что делает пункт.
    pub id: ItemId,
    /// Подпись (en-US из `language.json`, как в C++ `_(...)`).
    pub caption: String,
    /// Символ позиции для клавиатуры (`UnitsManager_RegisterButton::position`).
    pub hotkey: char,
    /// C++ `state[i]`: нажатый вид (`UNTBTN_D`) — режим включён.
    pub pressed: bool,
    /// Пункт срабатывает; `false` — механика не портирована (серый).
    pub enabled: bool,
    /// Почему пункт отключён (для журнала/документации); `""` — включён.
    pub note: &'static str,
}

impl UnitMenuItem {
    fn new(id: ItemId, caption: &str, hotkey: char, pressed: bool) -> Self {
        Self {
            id,
            caption: caption.to_string(),
            hotkey,
            pressed,
            enabled: true,
            note: "",
        }
    }

    fn disabled(mut self, note: &'static str) -> Self {
        self.enabled = false;
        self.note = note;
        self
    }
}

/// Стабильный ключ сортировки C++ (`'0'` → `':'`, чтобы «remove» был последним).
pub fn key_code(position: char) -> char {
    if position == '0' {
        ':'
    } else {
        position
    }
}

// ---------------------------------------------------------------------------
// Ресурсы
// ---------------------------------------------------------------------------

/// Картинки меню: `UNTBTN_U` (отжатая) и `UNTBTN_D` (нажатая), 42×21.
#[derive(Debug, Clone)]
pub struct UnitMenuAssets {
    /// Отжатая кнопка.
    pub up: Option<SimpleImage>,
    /// Нажатая кнопка (C++: `state[i]` → `image_up = UNTBTN_D`).
    pub down: Option<SimpleImage>,
}

impl UnitMenuAssets {
    /// Грузит `UNTBTN_U/D` из `MAX.RES` (отсутствие — `None`, меню не рисуется).
    pub fn load(max_res: &max_assets::ResArchive) -> Self {
        Self {
            up: max_res
                .read("UNTBTN_U")
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok()),
            down: max_res
                .read("UNTBTN_D")
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok()),
        }
    }

    /// Обе картинки загружены.
    pub fn is_ready(&self) -> bool {
        self.up.is_some() && self.down.is_some()
    }
}

// ---------------------------------------------------------------------------
// Состав пунктов
// ---------------------------------------------------------------------------

/// Набор пунктов по типу юнита — C++ `UnitsManager_PopupCallbacks[i].init`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitKind {
    /// `UnitsManager_Popup_InitCommons` — только общие пункты.
    Commons,
    /// `InitSurveyor` — `Auto`.
    Surveyor,
    /// `InitMilitary` — `attack`.
    Military,
    /// `InitInfiltrator` — `disable`, `steal`, `attack`.
    Infiltrator,
    /// `InitReloaders` — `reload`.
    Reloaders,
    /// `InitMineLayers` — `place`, `remove`.
    MineLayers,
    /// `InitRepair` — `repair`.
    Repair,
    /// `InitBuilders` — `Build`/`stop`.
    Builders,
    /// `InitFactories` — `Build`, `start`/`stop`.
    Factories,
    /// `InitRecreationCenter` — `start` при выключенном питании.
    RecreationCenter,
    /// `InitMiningStation` — `allocate`/`start` + `stop`.
    MiningStation,
    /// `InitEcoSphere` — `start`/`stop`.
    EcoSphere,
    /// `InitResearch` — `resrch` + `stop`, либо `start`.
    Research,
    /// `InitPowerGenerators` — `start`/`stop`.
    PowerGenerators,
    /// `InitRemove` — `remove`/`stop` (бульдозер).
    Remove,
    /// `InitMaster` — `start` мастер-строителя.
    Master,
    /// `InitGoldRefinery` — `start`/`stop` + `buy upg` (у `COMMTWR` — та же
    /// функция, `unitinfo.cpp:456`).
    GoldRefinery,
    /// `InitRepairShops` — `activate` + `load`.
    RepairShops,
}

/// Таблица `UnitInfo::Init` (`unitinfo.cpp:454-582`): тип → набор пунктов.
fn init_kind(unit_type: UnitType) -> InitKind {
    use UnitType::*;

    match unit_type {
        // `COMMTWR` → `UnitsManager_PopupCallbacks[22]` (`unitinfo.cpp:456`).
        CommTower => InitKind::GoldRefinery,
        PowerStation | PowerGenerator => InitKind::PowerGenerators,
        Barracks | Depot | Hangar | Dock => InitKind::RepairShops,
        ADump | FDump => InitKind::Commons,
        Shipyard | LightVehiclePlant | HeavyVehiclePlant | AirUnitsPlant | TrainingHall => {
            InitKind::Factories
        }
        ResearchCenter => InitKind::Research,
        Greenhouse => InitKind::EcoSphere,
        RecreationCenter => InitKind::RecreationCenter,
        GunTurret | AntiAir | ArtilleryTurret | AntiMissile | Scanner | Tank | AssaultGun
        | RocketLauncher | MissileCrawler | MobileAntiAir | Infantry | FastBoat | Corvette
        | Battleship | Submarine | MissileBoat | Fighter | Bomber | Juggernaut | AlienTank
        | AlienAssaultGun | AlienPlane => InitKind::Military,
        MiningStation => InitKind::MiningStation,
        MasterBuilder => InitKind::Master,
        Constructor | Engineer => InitKind::Builders,
        MineLayer | SeaMineLayer => InitKind::MineLayers,
        Surveyor => InitKind::Surveyor,
        GoldSmelter => InitKind::GoldRefinery,
        SupplyTruck | CargoShip => InitKind::Reloaders,
        Bulldozer => InitKind::Remove,
        RepairUnit => InitKind::Repair,
        FuelTruck => InitKind::Commons,
        PersonnelCarrier | SeaTransport | AirTransport => InitKind::RepairShops,
        Commando => InitKind::Infiltrator,
        // `default:` — `UnitsManager_PopupCallbacks[0]` (`InitCommons`).
        _ => InitKind::Commons,
    }
}

/// `UnitInfo::HasUpgradeableAttributes` (`unitinfo.cpp:1403`).
fn has_upgradeable_attributes(state: &GameState, unit: &UnitInfo) -> bool {
    let Some(team_units) = state.team_units.get(unit.team.0 as usize) else {
        return false;
    };
    let current = team_units.current_unit_values(unit.unit_type as u16);
    let base = &unit.base_values;

    let better = |attribute: u8| base.get_attribute(attribute) < current.get_attribute(attribute);

    better(ATTRIB_HITS)
        || better(ATTRIB_ATTACK)
        || better(ATTRIB_ARMOR)
        || better(ATTRIB_SPEED)
        || better(ATTRIB_RANGE)
        || better(ATTRIB_ROUNDS)
        || better(ATTRIB_SCAN)
        || better(ATTRIB_AMMO)
        // Меньше ходов на постройку — тоже апгрейд (`ATTRIB_TURNS`).
        || (base.get_attribute(ATTRIB_TURNS) > current.get_attribute(ATTRIB_TURNS)
            && current.get_attribute(ATTRIB_TURNS) >= 1)
}

/// `UnitInfo::IsUpgradeAvailable` (`unitinfo.cpp:1423`): только стационарные.
pub fn is_upgrade_available(state: &GameState, unit: &UnitInfo) -> bool {
    unit.flags & unit_flags::STATIONARY != 0 && has_upgradeable_attributes(state, unit)
}

/// Общие пункты — `UnitsManager_Popup_InitCommons` (`units_manager.cpp:543`).
fn init_commons(state: &GameState, unit: &UnitInfo, items: &mut Vec<UnitMenuItem>) {
    let order = unit.orders;

    // `Manual`: только simultaneous moves, с боезапасом и не диверсант.
    if state.play_mode == PlayMode::SimultaneousMoves
        && unit.base_values.get_attribute(ATTRIB_AMMO) > 0
        && unit.unit_type != UnitType::Commando
    {
        items.push(UnitMenuItem::new(
            ItemId::Manual,
            "Manual",
            '4',
            unit.disabled_reaction_fire,
        ));
    }

    // `x-fer`: у юнитов с грузом `RAW..GOLD`, кроме `CLEAR`/`BUILD`.
    let cargo = cargo_type_of(unit.unit_type);
    if (cargo as u8) > (CargoType::None as u8)
        && (cargo as u8) <= (CargoType::Gold as u8)
        && order != UnitOrder::Clear
        && order != UnitOrder::Build
    {
        items.push(UnitMenuItem::new(ItemId::Transfer, "x-fer", '3', false));
    }

    // `enter`: у подвижных, кроме `CLEAR`/`BUILD`.
    if unit.flags & (unit_flags::MOBILE_AIR_UNIT | unit_flags::MOBILE_SEA_UNIT | unit_flags::MOBILE_LAND_UNIT) != 0
        && order != UnitOrder::Clear
        && order != UnitOrder::Build
    {
        items.push(UnitMenuItem::new(ItemId::Enter, "enter", '5', false));
    }

    // `stop`: только когда есть маршрут.
    if unit.path.is_some() && order != UnitOrder::Clear && order != UnitOrder::Build {
        items.push(UnitMenuItem::new(ItemId::Stop, "stop", '7', false));
    }

    // `upgrade`/`Upg. All`.
    if is_upgrade_available(state, unit) {
        items.push(UnitMenuItem::new(ItemId::Upgrade, "upgrade", '5', false));
        items.push(UnitMenuItem::new(ItemId::UpgradeAll, "Upg. All", '6', false));
    }

    // `sentry` + `Done`.
    if unit.flags & unit_flags::SENTRY_UNIT != 0 {
        if unit.unit_type != UnitType::Commando {
            match order {
                UnitOrder::Sentry => items.push(UnitMenuItem::new(
                    ItemId::Sentry,
                    "sentry",
                    '8',
                    true,
                )),
                UnitOrder::Await => items.push(UnitMenuItem::new(
                    ItemId::Sentry,
                    "sentry",
                    '8',
                    false,
                )),
                _ => {}
            }
        }

        items.push(UnitMenuItem::new(ItemId::Done, "Done", '9', false));
    }

    // `remove` — самоуничтожение здания (`UnitsManager_SelfDestructMenu`).
    if unit.flags & unit_flags::STATIONARY != 0 {
        items.push(UnitMenuItem::new(ItemId::RemoveStationary, "remove", '0', false));
    }
}

fn power_items(unit: &UnitInfo, on_caption: &str, items: &mut Vec<UnitMenuItem>) {
    if unit.orders == UnitOrder::PowerOff {
        items.push(UnitMenuItem::new(ItemId::PowerOn, on_caption, '2', false));
    } else {
        items.push(UnitMenuItem::new(ItemId::PowerOff, "stop", '7', false));
    }
}

/// Пункты по типу юнита (`UnitsManager_Popup_Init*`).
///
/// `false` — общие пункты не добавлять: в C++ `UnitsManager_Popup_InitFactories`
/// у завода с готовым юнитом вызывает `PlaceNewUnit` и выходит, не строя попап.
fn init_for_kind(kind: InitKind, unit: &UnitInfo, items: &mut Vec<UnitMenuItem>) -> bool {
    match kind {
        InitKind::Commons => {}
        InitKind::Surveyor => {
            items.push(UnitMenuItem::new(ItemId::Auto, "Auto", '1', unit.auto_survey));
        }
        InitKind::Military => {
            // `UnitsManager_Popup_OnClick_TargetingMode` (`units_manager.cpp:617`).
            items.push(UnitMenuItem::new(ItemId::Attack, "attack", '3', false));
        }
        InitKind::Infiltrator => {
            // `UnitsManager_Popup_InitInfiltrator` (`units_manager.cpp:932`):
            // `disable`/`steal` — курсорные режимы 9/8, `attack` — наведение.
            items.push(UnitMenuItem::new(
                ItemId::Disable,
                "disable",
                '1',
                false,
            ));
            items.push(UnitMenuItem::new(ItemId::Steal, "steal", '2', false));
            items.push(UnitMenuItem::new(ItemId::Attack, "attack", '3', false));
        }
        InitKind::Reloaders => {
            items.push(UnitMenuItem::new(ItemId::Reload, "reload", '1', false));
        }
        InitKind::MineLayers => {
            items.push(UnitMenuItem::new(
                ItemId::PlaceMine,
                "place",
                '1',
                unit.laying_state == 2,
            ));
            items.push(UnitMenuItem::new(
                ItemId::RemoveMine,
                "remove",
                '0',
                unit.laying_state == 1,
            ));
        }
        InitKind::Repair => {
            items.push(UnitMenuItem::new(ItemId::Repair, "repair", '1', false));
        }
        InitKind::Builders => {
            if unit.orders == UnitOrder::Build
                && unit.state != UnitOrderState::BuildCancel
                && unit.state != UnitOrderState::BuildAbort
            {
                // C++ `UnitsManager_Popup_OnClick_SelectBuildSite` →
                // `GameManager_SelectBuildSite`: отмена текущей стройки —
                // `ORDER_BUILD` + `ORDER_STATE_BUILD_CANCEL`
                // (`UnitInfo::CancelBuilding` у мобильного строителя очищает
                // `build_list`/`build_time` и возвращает `ORDER_AWAIT`).
                items.push(UnitMenuItem::new(ItemId::StopBuild, "stop", '7', false));
            } else {
                items.push(UnitMenuItem::new(ItemId::Build, "Build", '1', false));
            }
        }
        InitKind::Factories => {
            // C++: у завода с готовым юнитом попап не строится вовсе —
            // `UnitsManager_Popup_PlaceNewUnit` ставит юнита сразу.
            if unit.state == UnitOrderState::UnitReady {
                return false;
            }

            items.push(UnitMenuItem::new(ItemId::Build, "Build", '1', false));

            if unit.orders == UnitOrder::HaltBuilding || unit.orders == UnitOrder::HaltBuilding2 {
                // C++ `UnitsManager_Popup_OnClick_BuildOrder` → `UnitInfo::BuildOrder`:
                // `ORDER_BUILD` + `ORDER_STATE_INIT` (очередь и `build_time`
                // сохранены остановкой), затем `StartBuilding`.
                items.push(UnitMenuItem::new(ItemId::StartBuild, "start", '2', false));
            } else if unit.orders == UnitOrder::Build
                && unit.state != UnitOrderState::BuildCancel
                && unit.state != UnitOrderState::BuildAbort
            {
                items.push(UnitMenuItem::new(ItemId::StopBuild, "stop", '7', false));
            }
        }
        InitKind::RecreationCenter => {
            if unit.orders == UnitOrder::PowerOff {
                items.push(UnitMenuItem::new(ItemId::PowerOn, "start", '2', false));
            }
        }
        InitKind::MiningStation => {
            if unit.orders == UnitOrder::PowerOff {
                items.push(UnitMenuItem::new(ItemId::PowerOn, "start", '2', false));
            } else {
                items.push(UnitMenuItem::new(ItemId::Allocate, "allocate", '1', false));
                items.push(UnitMenuItem::new(ItemId::PowerOff, "stop", '7', false));
            }
        }
        InitKind::EcoSphere => power_items(unit, "start", items),
        InitKind::Research => {
            if unit.orders == UnitOrder::PowerOn {
                items.push(UnitMenuItem::new(ItemId::Research, "resrch", '1', false));
                items.push(UnitMenuItem::new(ItemId::PowerOff, "stop", '7', false));
            } else {
                items.push(UnitMenuItem::new(ItemId::PowerOn, "start", '2', false));
            }
        }
        InitKind::PowerGenerators => power_items(unit, "start", items),
        InitKind::Remove => {
            // `UnitsManager_Popup_InitRemove` (`units_manager.cpp:1123`):
            // в `ORDER_CLEAR` — `stop` (`_(bc20)`), иначе `remove` (`_(9737)`).
            if unit.orders == UnitOrder::Clear {
                items.push(UnitMenuItem::new(ItemId::StopRemove, "stop", '7', false));
            } else {
                items.push(UnitMenuItem::new(
                    ItemId::RemoveRubble,
                    "remove",
                    '0',
                    false,
                ));
            }
        }
        InitKind::Master => {
            // `UnitsManager_FindValidPowerGeneratorPosition` (проверка площадки
            // мастер-строителя) не портирована — пункта нет, как при
            // `is_placeable == false`.
        }
        InitKind::GoldRefinery => {
            power_items(unit, "start", items);
            items.push(UnitMenuItem::new(ItemId::BuyUpgrade, "buy upg", '1', false));
        }
        InitKind::RepairShops => {
            let caption = if unit.orders == UnitOrder::PowerOff {
                "start"
            } else {
                "activate"
            };
            items.push(
                UnitMenuItem::new(ItemId::Activate, caption, '1', false)
                    .disabled("RepairShopMenu_Menu не портирован: экран repairshopmenu.cpp:144-1141"),
            );
            // `UnitsManager_Popup_OnClick_Load` (`units_manager.cpp:1011`):
            // тумблер курсорного режима 7; погрузка — `GameManager_ProcessMouse`
            // (CURSOR_LOAD, `game_manager.cpp:6807-6842`).
            items.push(UnitMenuItem::new(ItemId::Load, "load", '2', false));
        }
    }

    true
}

/// Список пунктов меню юнита — C++ `popup->init(unit, &GameManager_PopupButtons)`.
///
/// Пустой список — меню не открывается (C++ `popup_count == 0`).
pub fn items_for(state: &GameState, unit: &UnitInfo) -> Vec<UnitMenuItem> {
    let mut items = Vec::new();

    if init_for_kind(init_kind(unit.unit_type), unit, &mut items) {
        init_commons(state, unit, &mut items);
    }
    // `UnitsManager_RegisterButton`: устойчивая вставка по возрастанию ключа.
    items.sort_by_key(|item| key_code(item.hotkey));

    items
}

/// Условие открытия — C++ `GameManager_InitPopupButtons` (`game_manager.cpp:3576`).
pub fn can_open(unit: &UnitInfo) -> bool {
    unit.orders != UnitOrder::Disable
        && (unit.flags & unit_flags::MOBILE_LAND_UNIT == 0
            || unit.state != UnitOrderState::UnitReady)
}

// ---------------------------------------------------------------------------
// Меню
// ---------------------------------------------------------------------------

/// Открытое меню команд одного юнита.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitMenu {
    /// Чей юнит.
    pub unit: UnitId,
    /// Пункты в порядке отрисовки (сверху вниз).
    pub items: Vec<UnitMenuItem>,
    /// Окно меню в пикселях кадра (`GameManager_PopupButtons` ulx/uly/width/height).
    pub rect: PixelRect,
    /// Индекс пункта под курсором (подсветки в C++ нет, нужен QA/отладке).
    pub hover: Option<usize>,
}

impl UnitMenu {
    /// Раскладка меню — `GameManager_InitPopupButtons` (`game_manager.cpp:3586-3617`).
    ///
    /// `anchor` — `(unit->x + 32, unit->y)` в пикселях области карты, то есть
    /// правый край клетки юнита по X и её центр по Y; `area` — окно карты,
    /// `zoom` — `Gfx_ZoomLevel` (пикселей на клетку).
    pub fn layout(anchor: (i32, i32), count: usize, area: PixelRect, zoom: u32) -> PixelRect {
        let count = count.max(1) as i32;
        let width = BUTTON_WIDTH + 2 * MARGIN;
        let height = BUTTON_HEIGHT * count + (count + 1) * MARGIN;

        let mut ulx = anchor.0;
        let mut uly = anchor.1 - height / 2;

        if ulx + width > area.lrx() {
            ulx -= width + zoom as i32;
        }

        ulx = ulx.max(area.ulx);

        if uly < area.uly {
            uly = area.uly;
        } else if uly + height > area.lry() {
            uly = area.lry() - height;
        }

        PixelRect::new(ulx, uly, width, height)
    }

    /// Открывает меню для юнита; `None` — пунктов нет/меню не положено.
    ///
    /// `anchor`/`area` — в пикселях кадра (уже со смещением окна карты).
    pub fn new(
        state: &GameState,
        unit: &UnitInfo,
        anchor: (i32, i32),
        area: PixelRect,
        zoom: u32,
    ) -> Option<Self> {
        if !can_open(unit) {
            return None;
        }

        let items = items_for(state, unit);

        if items.is_empty() {
            return None;
        }

        let rect = Self::layout(anchor, items.len(), area, zoom);

        Some(Self {
            unit: unit.id,
            items,
            rect,
            hover: None,
        })
    }

    /// Прямоугольник кнопки `index` (C++: `ulx += 3`, `uly += image->height + 3`).
    pub fn button_rect(&self, index: usize) -> PixelRect {
        PixelRect::new(
            self.rect.ulx + MARGIN,
            self.rect.uly + MARGIN + index as i32 * (BUTTON_HEIGHT + MARGIN),
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
        )
    }

    /// Индекс пункта под точкой кадра.
    pub fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.items.len()).find(|index| self.button_rect(*index).contains(x, y))
    }

    /// Индекс пункта по идентификатору.
    pub fn index_of(&self, id: ItemId) -> Option<usize> {
        self.items.iter().position(|item| item.id == id)
    }

    /// Индекс пункта по подписи (без учёта регистра) или по символу позиции.
    ///
    /// Так же ищет и C++: `toupper(position)` или `toupper(caption[0])`.
    pub fn index_by_text(&self, text: &str) -> Option<usize> {
        let wanted = text.trim().to_ascii_lowercase();
        let mut chars = wanted.chars();

        if let (Some(character), None) = (chars.next(), chars.next()) {
            if let Some(index) = self
                .items
                .iter()
                .position(|item| item.hotkey.eq_ignore_ascii_case(&character))
            {
                return Some(index);
            }
        }

        self.items.iter().position(|item| {
            item.caption.eq_ignore_ascii_case(&wanted)
                || item.id.name().eq_ignore_ascii_case(&wanted)
        })
    }

    /// Клавиша в открытом меню — `GameManager_ProcessPopupMenuInput`.
    ///
    /// Сначала сравнение с символом позиции, затем с первой буквой подписи
    /// (`toupper`); стрелки/Enter не обрабатываются, `Esc` — закрытие.
    pub fn key(&self, key: Key) -> Option<MenuKey> {
        match key {
            Key::Escape => Some(MenuKey::Close),
            Key::Char(character) => {
                let upper = character.to_ascii_uppercase();
                let index = self
                    .items
                    .iter()
                    .position(|item| item.hotkey.to_ascii_uppercase() == upper)
                    .or_else(|| {
                        self.items.iter().position(|item| {
                            item.caption
                                .chars()
                                .next()
                                .is_some_and(|first| first.to_ascii_uppercase() == upper)
                        })
                    });

                index.map(MenuKey::Item)
            }
            _ => None,
        }
    }

    /// Рисует меню поверх кадра (`UNTBTN_U/D` + подпись `FONT2` цветом `GREEN`).
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets, palette: &UiPalette) -> u32 {
        let Some(menu_assets) = assets.unit_menu.as_ref() else {
            return 0;
        };
        let (Some(up), Some(down)) = (menu_assets.up.as_ref(), menu_assets.down.as_ref()) else {
            return 0;
        };
        let font = GnwFontId::Font2.font();
        let mut painted = 0;

        for (index, item) in self.items.iter().enumerate() {
            let rect = self.button_rect(index);
            let image = if item.pressed { down } else { up };
            painted +=
                crate::menu::blit_simple(frame, palette, image, rect.ulx, rect.uly, true);

            let color = if item.enabled {
                palette.rgba(CAPTION_GREEN)
            } else {
                palette.rgba(CAPTION_DISABLED)
            };
            let drawn = draw_text_gnw_centered(frame, rect, font, &item.caption, color);

            if drawn > 0 {
                painted += 1;
            }
        }

        painted
    }
}

/// Результат клавиши в меню.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    /// `Esc` — закрыть.
    Close,
    /// Пункт `index` (позиция или первая буква подписи).
    Item(usize),
}

/// Что делать с событием ввода в открытом меню.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEvent {
    /// Событие меню не касается (движение мыши и т.п.).
    None,
    /// Наведение на пункт (для отладки/подсветки кадра).
    Hover(Option<usize>),
    /// Закрыть меню (Esc, клик вне, повторный клик по юниту).
    Close,
    /// Активировать пункт `index`.
    Item(usize),
}

/// Событие ввода в открытом меню — как C++ `Button` в окне `WINDOW_MAIN_MAP`.
///
/// Клик **вне** меню закрывает его (модальность: карта приказ не получает).
pub fn menu_event(menu: &UnitMenu, event: crate::ui::InputEvent) -> MenuEvent {
    match event {
        crate::ui::InputEvent::MouseMove(x, y) => MenuEvent::Hover(menu.item_at(x, y)),
        crate::ui::InputEvent::MouseClick(x, y) => match menu.item_at(x, y) {
            Some(index) if menu.items[index].enabled => MenuEvent::Item(index),
            Some(_) => MenuEvent::None,
            None => MenuEvent::Close,
        },
        crate::ui::InputEvent::RightClick(..) => MenuEvent::Close,
        crate::ui::InputEvent::Key(key) => match menu.key(key) {
            Some(MenuKey::Close) => MenuEvent::Close,
            Some(MenuKey::Item(index)) => {
                if menu.items[index].enabled {
                    MenuEvent::Item(index)
                } else {
                    MenuEvent::None
                }
            }
            None => MenuEvent::None,
        },
    }
}

// ---------------------------------------------------------------------------
// Приказы из меню (пакеты 08, процессоры P1/P2/P3)
// ---------------------------------------------------------------------------

/// Приказ `P1` (пакет 08) — C++ `Remote_SendNetPacket_08` с процессором 1.
pub fn p1_order(
    state: &GameState,
    id: UnitId,
    order: UnitOrder,
    order_state: UnitOrderState,
) -> Option<max_core::input::Command> {
    let unit = state.unit(id)?;

    Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: id,
            payload: max_core::input::OrderPayload::P1(max_core::input::PayloadP1 {
                order,
                state: order_state,
                prior_order: unit.orders,
                prior_state: unit.state,
                disabled_reaction_fire: unit.disabled_reaction_fire,
            }),
        },
    ))
}

/// Приказ `P2` (пакет 08) — с родителем (`ORDER_REPAIR`/`ORDER_RELOAD`).
pub fn p2_order(
    state: &GameState,
    id: UnitId,
    order: UnitOrder,
    order_state: UnitOrderState,
    parent: Option<UnitId>,
) -> Option<max_core::input::Command> {
    let unit = state.unit(id)?;

    Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: id,
            payload: max_core::input::OrderPayload::P2(max_core::input::PayloadP2 {
                p1: max_core::input::PayloadP1 {
                    order,
                    state: order_state,
                    prior_order: unit.orders,
                    prior_state: unit.state,
                    disabled_reaction_fire: unit.disabled_reaction_fire,
                },
                parent_unit: parent,
                experience: unit.experience,
            }),
        },
    ))
}

/// Приказ `P3` (пакет 08) — с клеткой назначения (`ORDER_AWAIT`/`MOVE_TO_UNIT`).
pub fn p3_order(
    state: &GameState,
    id: UnitId,
    order: UnitOrder,
    order_state: UnitOrderState,
    move_to: Point,
    enemy: Option<UnitId>,
) -> Option<max_core::input::Command> {
    let unit = state.unit(id)?;
    let data = state.order_data(id);

    Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: id,
            payload: max_core::input::OrderPayload::P3(max_core::input::PayloadP3 {
                p2: max_core::input::PayloadP2 {
                    p1: max_core::input::PayloadP1 {
                        order,
                        state: order_state,
                        prior_order: unit.orders,
                        prior_state: unit.state,
                        disabled_reaction_fire: unit.disabled_reaction_fire,
                    },
                    parent_unit: unit.parent_unit,
                    experience: unit.experience,
                },
                move_to,
                fire_on: data.fire_on,
                enemy_unit: enemy,
                transfer_cargo: unit.transfer_cargo,
                stealth_dice_roll: unit.stealth_dice_roll,
            }),
        },
    ))
}

/// Курсорный клик по своему юниту при активном режиме `cursor` (C++ `CURSOR_*`
/// ветки `GameManager_ProcessMouse`).
///
/// `repair` (`game_manager.cpp:4617`) и `reload` (`:4600`): нужен груз у
/// выделенного (`unit1->storage`), цель не `ORDER_DISABLE`; родителем
/// становится цель, приказ — `ORDER_REPAIR`/`ORDER_RELOAD`, состояние `INIT`.
///
/// `x-fer` (`cursor = 3`) обрабатывается в UI окном
/// [`crate::transfermenu`] (`GameManager_TransferCargo` → `TransferMenu_Menu`):
/// здесь он не превращается в приказ. `load` — [`load_mode_click`],
/// `steal`/`disable` — [`stealth_mode_click`] (клик по врагу, см. `ui.rs`).
pub fn cursor_mode_click(
    state: &GameState,
    selected: UnitId,
    target: UnitId,
    cursor: u8,
) -> Result<Option<max_core::input::Command>, String> {
    let Some(unit) = state.unit(selected) else {
        return Ok(None);
    };
    let Some(target_unit) = state.unit(target) else {
        return Ok(None);
    };

    if selected == target || target_unit.team != unit.team {
        return Ok(None);
    }

    let (order, label) = match cursor {
        CURSOR_REPAIR => (UnitOrder::Repair, "Repair"),
        CURSOR_RELOAD => (UnitOrder::Reload, "Reload"),
        _ => return Ok(None),
    };

    if target_unit.orders == UnitOrder::Disable {
        return Err(format!("{label}: цель отключена"));
    }

    if unit.storage == 0 {
        return Err(format!("{label}: у юнита нет груза (storage = 0)"));
    }

    Ok(p2_order(
        state,
        selected,
        order,
        UnitOrderState::Init,
        Some(target),
    ))
}

/// Приказ «войти в транспорт» по клику (C++ `CURSOR_UNIT_GO` + `enter_mode`):
/// `ORDER_MOVE_TO_UNIT` в клетку цели, как `GameManager_SetUnitOrder`.
pub fn enter_mode_click(
    state: &GameState,
    selected: UnitId,
    target: UnitId,
) -> Option<max_core::input::Command> {
    let unit = state.unit(selected)?;
    let target_unit = state.unit(target)?;

    if selected == target || target_unit.team != unit.team {
        return None;
    }

    p3_order(
        state,
        selected,
        UnitOrder::MoveToUnit,
        UnitOrderState::Init,
        target_unit.grid,
        None,
    )
}

/// Курсорный режим диверсанта «захват» — C++ `unit->cursor = 8`
/// (`CURSOR_ARROW_W`, `UnitsManager_Popup_OnClick_Steal`).
pub const CURSOR_STEAL: u8 = 8;
/// Курсорный режим диверсанта «отключение» — C++ `unit->cursor = 9`
/// (`CURSOR_ARROW_NW`, `UnitsManager_Popup_OnClick_Disable`).
pub const CURSOR_DISABLE: u8 = 9;

/// `UnitsManager_IsValidStealthTarget` (`units_manager.cpp:1370`) — что вообще
/// может быть целью диверсанта: электронный юнит, не свой, живой, не в полёте,
/// не `IDLE`/`TRANSFORM`/`FIRE`/`EXPLODE`, по соседству и валиден как цель
/// атаки. Для `ORDER_AWAIT_STEAL_UNIT` — ещё и не стационарный/не занятый
/// транспортом с грузом.
pub fn is_valid_stealth_target(
    state: &GameState,
    unit: &UnitInfo,
    target: &UnitInfo,
    order: UnitOrder,
) -> bool {
    let target_order = target.orders;

    if target.team == unit.team
        || target.hits == 0
        || target.flags & unit_flags::ELECTRONIC_UNIT == 0
        || (target.flags & unit_flags::MOBILE_AIR_UNIT != 0
            && target.flags & unit_flags::HOVERING != 0)
        || target_order == UnitOrder::Idle
        || (target_order == UnitOrder::AwaitScaling && target.prior_orders == UnitOrder::Idle)
        || target_order == UnitOrder::Transform
        || target_order == UnitOrder::Fire
        || target_order == UnitOrder::Explode
        || target.state == UnitOrderState::Destroy
        || !max_core::access::is_adjacent_to_grid(target, unit.grid)
        || !max_core::access::is_valid_attack_target(state, unit, target)
    {
        return false;
    }

    if order == UnitOrder::AwaitStealUnit {
        let transport_with_cargo = matches!(
            target.unit_type,
            UnitType::PersonnelCarrier | UnitType::SeaTransport | UnitType::AirTransport
        ) && target.storage != 0;

        target.flags & unit_flags::STATIONARY == 0 && !transport_with_cargo
    } else {        target_order != UnitOrder::Disable
    }
}

/// Клик по чужому юниту в курсорном режиме диверсанта (8/9) —
/// `GameManager_ProcessMouse` → `GameManager_StealUnit`/`GameManager_DisableUnit`
/// (`game_manager.cpp:4680/4696`).
///
/// Ставит `parent = цель`, `stealth_dice_roll = Randomizer_Generate(101)` и
/// `ORDER_AWAIT_STEAL_UNIT`/`ORDER_AWAIT_DISABLE_UNIT` + `INIT` (пакет 08/P3).
/// `Err` — у диверсанта не осталось действия на этот ход (`_(f37a)`/`_(3e56)`).
pub fn stealth_mode_click(
    state: &GameState,
    selected: UnitId,
    target: UnitId,
    cursor: u8,
    stealth_dice_roll: u8,
) -> Result<Option<max_core::input::Command>, String> {
    let (order, message) = match cursor {
        CURSOR_STEAL => (
            UnitOrder::AwaitStealUnit,
            "Infiltrator has already used his action this turn. Try again next turn.",
        ),
        CURSOR_DISABLE => (
            UnitOrder::AwaitDisableUnit,
            "Infiltrator has already used his action this turn. Try again next turn.",
        ),
        _ => return Ok(None),
    };

    let Some(unit) = state.unit(selected) else {
        return Ok(None);
    };
    if selected == target || unit.unit_type != UnitType::Commando || unit.shots == 0 {
        return Err(message.to_string());
    }

    // `Access_GetAttackTarget(unit1, target->grid)` (напр. цель-платформа).
    let Some(target_unit) = state.unit(target) else {
        return Ok(None);
    };
    let target_id = if is_valid_stealth_target(state, unit, target_unit, order) {
        target
    } else if let Some(other) =
        max_core::access::get_attack_target(state, unit, target_unit.grid, false)
    {
        let Some(other_unit) = state.unit(other) else {
            return Ok(None);
        };
        if !is_valid_stealth_target(state, unit, other_unit, order) {
            return Ok(None);
        }
        other
    } else {
        return Ok(None);
    };

    let unit = state.unit(selected).expect("юнит проверен");
    let payload = max_core::input::PayloadP3 {
        p2: max_core::input::PayloadP2 {
            p1: max_core::input::PayloadP1 {
                order,
                state: UnitOrderState::Init,
                prior_order: unit.orders,
                prior_state: unit.state,
                disabled_reaction_fire: unit.disabled_reaction_fire,
            },
            parent_unit: Some(target_id),
            experience: unit.experience,
        },
        move_to: unit.target_grid,
        fire_on: state.order_data(selected).fire_on,
        enemy_unit: None,
        transfer_cargo: unit.transfer_cargo,
        stealth_dice_roll,
    };

    Ok(Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: selected,
            payload: max_core::input::OrderPayload::P3(payload),
        },
    )))
}

/// Условия показа `CURSOR_LOAD` над целью — `GameManager_GetWindowCursor`
/// (`game_manager.cpp:4416-4518`): у выделенного есть место, а цель — тот тип,
/// который этот транспорт/цех принимает.
pub fn load_mode_applies(selected: &UnitInfo, target: &UnitInfo) -> bool {
    if selected.id == target.id || selected.team != target.team || target.hits == 0 {
        return false;
    }

    let storage = selected.base_values.get_attribute(ATTRIB_STORAGE);
    if i32::from(selected.storage) >= storage {
        return false;
    }

    let target_land = target.flags & unit_flags::MOBILE_LAND_UNIT != 0;
    let target_air = target.flags & unit_flags::MOBILE_AIR_UNIT != 0;
    let target_sea = target.flags & unit_flags::MOBILE_SEA_UNIT != 0;
    let infantry = matches!(
        target.unit_type,
        UnitType::Commando | UnitType::Infantry
    );

    match selected.unit_type {
        UnitType::AirTransport => target_land,
        UnitType::PersonnelCarrier => infantry,
        UnitType::SeaTransport => target_land,
        UnitType::Depot => target_land && !infantry,
        UnitType::Hangar => target_air,
        UnitType::Dock => target_sea && !target_land,
        UnitType::Barracks => infantry,
        _ => false,
    }
}

/// Клик в курсорном режиме `load` (`cursor = 7`) —
/// `GameManager_ProcessMouse`, ветка `CURSOR_LOAD` (`game_manager.cpp:6807-6842`).
///
/// * `AIRTRANS`: транспорт летит в клетку цели (`parent = цель`);
/// * цель отключена — сообщение `_(9897)`;
/// * цель уже в клетке транспорта и готова — `ORDER_LAND`/`INIT` с
///   `parent = транспорт` (пассажир «складывается» в груз);
/// * иначе — **цель** получает `ORDER_MOVE_TO_UNIT` в клетку транспорта
///   (`GameManager_Unit` в C++, не выделенный юнит).
pub fn load_mode_click(
    state: &GameState,
    selected: UnitId,
    target: UnitId,
) -> Result<Option<max_core::input::Command>, String> {
    let Some(unit) = state.unit(selected) else {
        return Ok(None);
    };
    let Some(target_unit) = state.unit(target) else {
        return Ok(None);
    };

    if selected == target || unit.team != target_unit.team || target_unit.hits == 0 {
        return Ok(None);
    }

    if i32::from(unit.storage) >= unit.base_values.get_attribute(ATTRIB_STORAGE) {
        return Ok(None);
    }

    if target_unit.orders == UnitOrder::Disable {
        return Err("Unable to load disabled units.".to_string());
    }

    if unit.unit_type == UnitType::AirTransport {
        let payload = max_core::input::PayloadP3 {
            p2: max_core::input::PayloadP2 {
                p1: max_core::input::PayloadP1 {
                    order: UnitOrder::Move,
                    state: UnitOrderState::Init,
                    prior_order: unit.orders,
                    prior_state: unit.state,
                    disabled_reaction_fire: unit.disabled_reaction_fire,
                },
                parent_unit: Some(target),
                experience: unit.experience,
            },
            move_to: target_unit.grid,
            fire_on: state.order_data(selected).fire_on,
            enemy_unit: None,
            transfer_cargo: unit.transfer_cargo,
            stealth_dice_roll: unit.stealth_dice_roll,
        };

        return Ok(Some(max_core::input::Command::Order(
            max_core::input::Packet08Order {
                unit: selected,
                payload: max_core::input::OrderPayload::P3(payload),
            },
        )));
    }

    let same_cell = unit.grid == target_unit.grid
        && target_unit.state == UnitOrderState::ExecutingOrder;

    if same_cell {
        return Ok(p2_order(
            state,
            target,
            UnitOrder::Land,
            UnitOrderState::Init,
            Some(selected),
        ));
    }

    Ok(p3_order(
        state,
        target,
        UnitOrder::MoveToUnit,
        UnitOrderState::Init,
        unit.grid,
        None,
    ))
}

/// `Access_GetRemovableRubble` (`access.cpp:1817`): обломки в клетке, которые
/// бульдозер может убрать. `LRGRUBLE` требует, чтобы остальные три клетки 2×2
/// были проходимы для `BULLDOZR` (`Access_IsAccessible` с
/// `AccessModifier_SameClassBlocks`).
pub fn removable_rubble(state: &GameState, team: max_core::team::TeamId, grid: Point) -> Option<UnitId> {
    for id in state.units_at(grid).iter().copied() {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        match unit.unit_type {
            UnitType::SmallRubble => return Some(id),
            UnitType::LargeRubble => {
                for y in unit.grid.y..=unit.grid.y + 1 {
                    for x in unit.grid.x..=unit.grid.x + 1 {
                        if x == grid.x && y == grid.y {
                            continue;
                        }

                        if max_core::access::is_accessible(
                            state,
                            UnitType::Bulldozer,
                            team,
                            Point::new(x, y),
                            max_core::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                        ) == 0
                        {
                            return None;
                        }
                    }
                }

                return Some(id);
            }
            _ => {}
        }
    }

    None
}

/// Приказ `ORDER_CLEAR` бульдозеру — `UnitsManager_Popup_OnClick_StopRemove`
/// (`units_manager.cpp:1082-1121`): родитель — обломки, `build_time` = 4 для
/// здания и 1 для прочих обломков.
pub fn clear_order(
    state: &GameState,
    selected: UnitId,
    rubble: UnitId,
) -> Option<max_core::input::Command> {
    p3_order(
        state,
        selected,
        UnitOrder::Clear,
        UnitOrderState::Init,
        state.unit(rubble).map(|unit| unit.grid).unwrap_or(Point::ZERO),
        None,
    )
}

/// Строка «номер марки» — `UnitInfo::GetVersion` (`unitinfo.cpp:2930`):
/// римские цифры версии `UnitValues` (`Mk I`, `Mk II`, …).
pub fn roman_version(mut version: u16) -> String {
    fn digit(out: &mut String, count: u16, one: &str, five: &str, ten: &str) {
        match count {
            0 => {}
            1 => out.push_str(one),
            2 => out.push_str(&one.repeat(2)),
            3 => out.push_str(&one.repeat(3)),
            4 => {
                out.push_str(one);
                out.push_str(five);
            }
            5 => out.push_str(five),
            6 => {
                out.push_str(five);
                out.push_str(one);
            }
            7 => {
                out.push_str(five);
                out.push_str(&one.repeat(2));
            }
            8 => {
                out.push_str(five);
                out.push_str(&one.repeat(3));
            }
            _ => {
                out.push_str(one);
                out.push_str(ten);
            }
        }
    }

    let mut out = String::new();
    digit(&mut out, version / 100, "C", "D", "M");
    version %= 100;
    digit(&mut out, version / 10, "X", "L", "C");
    version %= 10;
    digit(&mut out, version, "I", "V", "X");

    out
}

/// Результат `Upg. All` — приказы и сообщение (`UnitsManager_Popup_OnClick_UpgradeAll`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeAllReport {
    /// `ORDER_UPGRADE`/`EXECUTING_ORDER` (P2) каждому улучшенному зданию.
    pub commands: Vec<max_core::input::Command>,
    /// Сколько зданий улучшено.
    pub count: usize,
    /// Потрачено сырья по комплексам.
    pub material_cost: i32,
    /// Сообщение игроку (`_(a0ee)`/`_(8967)`/`_(2693)`).
    pub message: String,
}

/// `UnitsManager_Popup_OnClick_UpgradeAll` (`units_manager.cpp:414-496`):
/// улучшить **все** здания своей команды того же типа, что и выделенное, по
/// комплексам, списывая стоимость из материалов комплекса (`GetCargoInfo`).
///
/// `get_normal_rate_build_cost(unit) / 4` — `UnitInfo::GetUpgradeCost()`.
pub fn upgrade_all_orders(state: &GameState, unit_id: UnitId) -> Option<UpgradeAllReport> {
    let unit = state.unit(unit_id)?;
    let team = unit.team;
    let unit_type = unit.unit_type;
    let upgrade_cost = max_core::victory::get_normal_rate_build_cost(unit) / 4;

    let ids: Vec<UnitId> = state
        .unit_list(max_core::unit::UnitListKind::Stationary)
        .to_vec();
    let mut complexes: Vec<max_core::team::ComplexId> = Vec::new();
    let mut costs: Vec<i32> = Vec::new();
    let mut commands = Vec::new();
    let mut material_cost = 0;
    let mut count = 0;
    let mut last: Option<UnitId> = None;

    for id in ids {
        let Some(other) = state.unit(id) else {
            continue;
        };

        if other.team != team
            || other.unit_type != unit_type
            || !is_upgrade_available(state, other)
            || other.state == UnitOrderState::UnitReady
        {
            continue;
        }

        let Some(complex) = other.complex else {
            continue;
        };

        let index = match complexes.iter().position(|known| *known == complex) {
            Some(index) => index,
            None => {
                // `(*it).GetComplex()->GetCargoInfo(materials, capacity)`: в
                // порте агрегат комплекса уже пересчитан `OptimizeComplexes`
                // (`GameState::update_complex_resources`), поэтому берём его
                // поле `material` без повторного суммирования складов.
                let materials = state
                    .complex(team, complex.0 as u16)
                    .map(|complex| i32::from(complex.material))
                    .unwrap_or(0);

                complexes.push(complex);
                costs.push(materials);

                complexes.len() - 1
            }
        };

        if costs[index] >= upgrade_cost {
            if let Some(command) = p2_order(
                state,
                id,
                UnitOrder::Upgrade,
                UnitOrderState::ExecutingOrder,
                Some(id),
            ) {
                commands.push(command);
            }

            costs[index] -= upgrade_cost;
            material_cost += upgrade_cost;
            count += 1;
            last = Some(id);
        }
    }

    let version = roman_version(
        state
            .team_units
            .get(team.0 as usize)
            .map(|units| units.current_unit_values(unit_type as u16).version)
            .unwrap_or(1),
    );
    let name = crate::buildmenu::unit_name(unit_type);

    let message = if count == 0 {
        format!("{upgrade_cost} raw material needed to upgrade.")
    } else if count == 1 {
        format!("{name} upgraded to mark {version} for {material_cost} raw material.")
    } else {
        format!("{count} {name} upgraded to mark {version} for {material_cost} raw material.")
    };

    let _ = last;

    Some(UpgradeAllReport {
        commands,
        count,
        material_cost,
        message,
    })
}

/// Приказ самоуничтожения здания — `UnitsManager_SetNewOrder(unit,
/// ORDER_EXPLODE, ORDER_STATE_EXPLODE)` (`units_manager.cpp:506-511`).
pub fn explode_order(
    state: &GameState,
    unit_id: UnitId,
) -> Option<max_core::input::Command> {
    p1_order(state, unit_id, UnitOrder::Explode, UnitOrderState::Explode)
}

/// Строка журнала для пункта, который ничего не делает (отключён).
pub fn disabled_line(item: &UnitMenuItem) -> String {
    format!("{}: {}", item.caption, item.note)
}

/// Строка журнала о выборе пункта (`App::activate_unit_menu_item`).
pub fn activation_line(item: &UnitMenuItem) -> String {
    format!("{} — {}", item.caption, item.id.name())
}

/// Проверка «пункт есть и включён» (QA `assert unit-menu-enabled`).
pub fn item_enabled(menu: &UnitMenu, text: &str) -> Option<bool> {
    menu.index_by_text(text).map(|index| menu.items[index].enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::TeamClan;

    fn test_state() -> GameState {
        let mut state = GameState::new();
        state.team_units[0] = max_core::team::TeamUnits::with_clan(TeamClan::Random);
        state
    }

    fn spawn(state: &mut GameState, unit_type: UnitType) -> UnitId {
        let flags = max_core::access::unit_type_flags(unit_type);
        let id = state
            .spawn_unit(unit_type, max_core::team::TeamId::RED, Point::new(22, 22), flags)
            .expect("юнит");
        id
    }

    #[test]
    fn tank_menu_matches_cpp_order_and_contents() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Tank);
        let unit = state.unit(id).expect("танк").clone();
        let items = items_for(&state, &unit);
        let names: Vec<&str> = items.iter().map(|item| item.id.name()).collect();

        // `InitMilitary`: attack('3') → commons x-fer? (у танка нет груза) →
        // enter('5') → sentry('8') → Done('9'). Сортировка по позиции.
        assert_eq!(names, vec!["attack", "enter", "sentry", "Done"]);

        // `attack` и `sentry` — порядок позиций 3 и 8, `Done` — 9.
        assert_eq!(items[0].id, ItemId::Attack);
        assert!(
            items[0].enabled,
            "attack — тумблер targeting_mode (UnitsManager_Popup_OnClick_TargetingMode)"
        );
        assert_eq!(items[1].id, ItemId::Enter);
        assert!(items[1].enabled);
        assert_eq!(items[2].id, ItemId::Sentry);
        assert!(items[2].enabled, "sentry — приказ P1");
        assert_eq!(items[3].id, ItemId::Done);
    }

    #[test]
    fn constructor_menu_has_build_first() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Constructor);
        let unit = state.unit(id).expect("строитель").clone();
        let items = items_for(&state, &unit);

        assert_eq!(items[0].id, ItemId::Build);
        assert!(items[0].enabled, "Build открывает BuildMenu");
        // Строитель — `RAW`-груз, значит x-fer в списке и включён
        // (`TransferMenu_Menu` + `ProcessOrderTransfer` портированы).
        let transfer = items
            .iter()
            .find(|item| item.id == ItemId::Transfer)
            .expect("x-fer у строителя");
        assert!(transfer.enabled, "x-fer открывает TransferMenu");
        assert!(transfer.note.is_empty());
    }

    #[test]
    fn stop_appears_only_with_path() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Tank);
        let mut unit = state.unit(id).expect("танк").clone();
        assert!(!items_for(&state, &unit)
            .iter()
            .any(|item| item.id == ItemId::Stop));

        unit.path = Some(max_core::unit::UnitPath::Ground(max_core::unit::GroundPath {
            end_x: 24,
            end_y: 22,
            step_index: 0,
            steps: Vec::new(),
        }));

        assert!(items_for(&state, &unit)
            .iter()
            .any(|item| item.id == ItemId::Stop && item.enabled));
    }

    #[test]
    fn builder_mid_build_offers_enabled_stop_instead_of_build() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Constructor);
        let mut unit = state.unit(id).expect("строитель").clone();

        // C++ `UnitsManager_Popup_InitBuilders`: во время `ORDER_BUILD` пункт
        // `Build` заменяется на `stop` (`GameManager_SelectBuildSite`).
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit.build_list = vec![UnitType::TrainingHall];
        unit.build_time = 10;
        state.write_unit(id, unit.clone());

        let items = items_for(&state, &unit);
        assert!(
            !items.iter().any(|item| item.id == ItemId::Build),
            "во время стройки `Build` заменяется на `stop` (как C++)"
        );
        let stop = items
            .iter()
            .find(|item| item.id == ItemId::StopBuild)
            .expect("stop во время стройки");
        assert!(
            stop.enabled,
            "stop отменяет стройку (GameManager_SelectBuildSite) — не серый"
        );
        assert!(stop.note.is_empty());
        assert_eq!(stop.hotkey, '7');
    }

    #[test]
    fn halted_factory_offers_enabled_start() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::TrainingHall);
        let mut unit = state.unit(id).expect("завод").clone();
        unit.orders = UnitOrder::HaltBuilding2;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.build_list = vec![UnitType::Infantry];
        unit.build_time = 2;
        state.write_unit(id, unit.clone());

        let start = items_for(&state, &unit)
            .into_iter()
            .find(|item| item.id == ItemId::StartBuild)
            .expect("start у остановленного завода");
        assert!(
            start.enabled,
            "start продолжает стройку (UnitInfo::BuildOrder) — не серый"
        );
        assert!(start.note.is_empty());
    }

    #[test]
    fn transport_lists_load_and_activate() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::PersonnelCarrier);
        let unit = state.unit(id).expect("транспорт").clone();
        let items = items_for(&state, &unit);

        let load = items
            .iter()
            .find(|item| item.id == ItemId::Load)
            .expect("load у транспорта");

        assert!(
            load.enabled,
            "load — тумблер курсорного режима 7 (UnitsManager_Popup_OnClick_Load)"
        );
        assert!(load.note.is_empty());

        // `activate` (RepairShopMenu) остаётся отключённым — экран не портирован.
        let activate = items
            .iter()
            .find(|item| item.id == ItemId::Activate)
            .expect("activate у транспорта");
        assert!(!activate.enabled);
        assert!(!activate.note.is_empty());
    }

    #[test]
    fn infiltrator_has_stealth_and_targeting_items_enabled() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Commando);
        let unit = state.unit(id).expect("диверсант").clone();
        let items = items_for(&state, &unit);

        for id in [ItemId::Disable, ItemId::Steal, ItemId::Attack] {
            let item = items
                .iter()
                .find(|item| item.id == id)
                .unwrap_or_else(|| panic!("{id:?} у диверсанта"));

            assert!(item.enabled, "{id:?} должен быть включён");
            assert!(item.note.is_empty(), "{id:?}: {}", item.note);
        }
    }

    #[test]
    fn stationary_building_offers_enabled_remove_and_upgrade_all() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::PowerStation);

        // У апгрейда есть что улучшать: текущие значения команды выше базы.
        if let Some(team_units) = state.team_units.get_mut(0) {
            let values = team_units.current_unit_values_mut(UnitType::PowerStation as u16);
            let hits = values.get_attribute(ATTRIB_HITS);
            values.set_attribute(ATTRIB_HITS, hits + 10);
        }

        let unit = state.unit(id).expect("здание").clone();
        let items = items_for(&state, &unit);

        let remove = items
            .iter()
            .find(|item| item.id == ItemId::RemoveStationary)
            .expect("remove у здания");
        assert!(remove.enabled, "remove открывает UnitsManager_SelfDestructMenu");

        let upgrade_all = items
            .iter()
            .find(|item| item.id == ItemId::UpgradeAll)
            .expect("Upg. All у здания");
        assert!(upgrade_all.enabled, "Upg. All портирован");
    }

    #[test]
    fn bulldozer_has_enabled_remove_rubble() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Bulldozer);
        let unit = state.unit(id).expect("бульдозер").clone();
        let items = items_for(&state, &unit);

        let remove = items
            .iter()
            .find(|item| item.id == ItemId::RemoveRubble)
            .expect("remove у бульдозера");
        assert!(remove.enabled, "бульдозер → Access_GetRemovableRubble");
        assert!(remove.note.is_empty());
    }

    #[test]
    fn sentry_pressed_when_order_is_sentry() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Tank);
        let mut unit = state.unit(id).expect("танк").clone();

        assert!(!items_for(&state, &unit)
            .iter()
            .find(|item| item.id == ItemId::Sentry)
            .expect("sentry")
            .pressed);

        unit.orders = UnitOrder::Sentry;

        assert!(items_for(&state, &unit)
            .iter()
            .find(|item| item.id == ItemId::Sentry)
            .expect("sentry")
            .pressed);
    }

    #[test]
    fn layout_follows_cpp_geometry_and_stays_in_map() {
        let area = PixelRect::new(180, 18, 448, 448);
        let rect = UnitMenu::layout((400, 200), 4, area, 64);

        assert_eq!(rect.width, BUTTON_WIDTH + 6);
        assert_eq!(rect.height, BUTTON_HEIGHT * 4 + 5 * 3);
        assert_eq!(rect.ulx, 400);
        assert_eq!(rect.uly, 200 - rect.height / 2);

        // Правый край: сдвиг влево на ширину + одну клетку (`Gfx_ZoomLevel`).
        let rect = UnitMenu::layout((area.lrx() - 2, 100), 4, area, 64);
        assert_eq!(rect.ulx, area.lrx() - 2 - (BUTTON_WIDTH + 6) - 64);

        // Нижний край прижимается к окну карты.
        let rect = UnitMenu::layout((300, area.lry() - 2), 6, area, 64);
        assert_eq!(rect.lry(), area.lry());
    }

    #[test]
    fn buttons_are_stacked_with_three_pixel_gap() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Tank);
        let unit = state.unit(id).expect("танк").clone();
        let menu = UnitMenu::new(
            &state,
            &unit,
            (400, 200),
            PixelRect::new(180, 18, 448, 448),
            64,
        )
        .expect("меню танка");

        assert_eq!(menu.button_rect(0).ulx, menu.rect.ulx + 3);
        assert_eq!(menu.button_rect(0).uly, menu.rect.uly + 3);

        for index in 1..menu.items.len() {
            assert_eq!(
                menu.button_rect(index).uly,
                menu.button_rect(index - 1).uly + BUTTON_HEIGHT + MARGIN
            );
        }
    }

    #[test]
    fn hotkey_uses_position_then_first_letter() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Tank);
        let unit = state.unit(id).expect("танк").clone();
        let menu = UnitMenu::new(
            &state,
            &unit,
            (400, 200),
            PixelRect::new(180, 18, 448, 448),
            64,
        )
        .expect("меню");

        // '8' — позиция `sentry`.
        assert_eq!(menu.key(Key::Char('8')), Some(MenuKey::Item(2)));
        // 'D' — первая буква `Done`.
        assert_eq!(menu.key(Key::Char('d')), Some(MenuKey::Item(3)));
        assert_eq!(menu.key(Key::Escape), Some(MenuKey::Close));
        assert_eq!(menu.key(Key::Enter), None, "Enter в C++ попапе не работает");
        assert_eq!(menu.key(Key::Down), None, "стрелки в C++ попапе не работают");
    }

    #[test]
    fn disabled_item_is_not_activated_by_click_or_key() {
        // Меню с отключённым пунктом: в C++ кнопка без обработчика не реагирует
        // (у нас остался только `activate` ремонтного цеха).
        let menu = UnitMenu {
            unit: UnitId(1),
            items: vec![
                UnitMenuItem::new(ItemId::Activate, "activate", '1', false)
                    .disabled("RepairShopMenu не портирован"),
                UnitMenuItem::new(ItemId::Done, "Done", '9', false),
            ],
            rect: UnitMenu::layout((400, 200), 2, PixelRect::new(180, 18, 448, 448), 64),
            hover: None,
        };

        let rect = menu.button_rect(0);
        let centre = (rect.ulx + 1, rect.uly + 1);

        assert_eq!(
            menu_event(&menu, crate::ui::InputEvent::MouseClick(centre.0, centre.1)),
            MenuEvent::None,
            "отключённый пункт не срабатывает"
        );
        assert_eq!(
            menu_event(&menu, crate::ui::InputEvent::Key(Key::Char('1'))),
            MenuEvent::None
        );
        // Включённый пункт срабатывает.
        let done = menu.button_rect(1);
        assert_eq!(
            menu_event(
                &menu,
                crate::ui::InputEvent::MouseClick(done.ulx + 1, done.uly + 1)
            ),
            MenuEvent::Item(1)
        );
        // Клик вне меню — закрытие.
        assert_eq!(
            menu_event(&menu, crate::ui::InputEvent::MouseClick(1, 1)),
            MenuEvent::Close
        );
    }

    #[test]
    fn sentry_click_produces_p1_order() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Tank);
        let command = p1_order(&state, id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder)
            .expect("приказ");

        match command {
            max_core::input::Command::Order(packet) => {
                assert_eq!(packet.unit, id);
                match packet.payload {
                    max_core::input::OrderPayload::P1(p1) => {
                        assert_eq!(p1.order, UnitOrder::Sentry);
                        assert_eq!(p1.state, UnitOrderState::ExecutingOrder);
                    }
                    other => panic!("ожидался P1, получен {other:?}"),
                }
            }
            other => panic!("ожидался Order, получен {other:?}"),
        }

        // Применение: приказ доходит до юнита (`events::apply_command`).
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::Tank);
        let command = p1_order(&state, id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder)
            .expect("приказ");
        max_core::events::apply_command(&mut state, &command);
        assert_eq!(state.unit(id).expect("танк").orders, UnitOrder::Sentry);
        assert_eq!(
            state.unit(id).expect("танк").state,
            UnitOrderState::ExecutingOrder
        );
    }

    #[test]
    fn repair_mode_click_needs_cargo_and_sets_parent() {
        let mut state = test_state();
        let repair = spawn(&mut state, UnitType::RepairUnit);
        let tank = spawn(&mut state, UnitType::Tank);

        state.unit_mut(repair).expect("repair").storage = 0;

        let error = cursor_mode_click(&state, repair, tank, CURSOR_REPAIR).expect_err("нет груза");
        assert!(error.contains("storage"), "сообщение об отсутствии груза");

        state.unit_mut(repair).expect("repair").storage = 3;

        let command = cursor_mode_click(&state, repair, tank, CURSOR_REPAIR)
            .expect("приказ")
            .expect("не пусто");

        max_core::events::apply_command(&mut state, &command);
        let unit = state.unit(repair).expect("repair");
        assert_eq!(unit.orders, UnitOrder::Repair);
        assert_eq!(unit.parent_unit, Some(tank));
    }

    #[test]
    fn enter_mode_click_orders_move_to_unit() {
        let mut state = test_state();
        let tank = spawn(&mut state, UnitType::Tank);
        let transport = spawn(&mut state, UnitType::PersonnelCarrier);

        state.unit_mut(transport).expect("транспорт").grid = Point::new(25, 22);
        state.unit_mut(tank).expect("танк").grid = Point::new(22, 22);

        let command = enter_mode_click(&state, tank, transport).expect("приказ");

        match &command {
            max_core::input::Command::Order(packet) => {
                assert_eq!(packet.unit, tank);
            }
            other => panic!("ожидался Order, получен {other:?}"),
        }

        max_core::events::apply_command(&mut state, &command);
        let unit = state.unit(tank).expect("танк");
        assert_eq!(unit.orders, UnitOrder::MoveToUnit);
        assert_eq!(
            state.order_data(tank).move_to,
            Point::new(25, 22),
            "цель — клетка транспорта"
        );
    }

    #[test]
    fn factory_with_ready_unit_has_no_menu() {
        let mut state = test_state();
        let id = spawn(&mut state, UnitType::LightVehiclePlant);
        let mut unit = state.unit(id).expect("завод").clone();
        unit.state = UnitOrderState::UnitReady;

        assert!(items_for(&state, &unit).is_empty());
        assert!(UnitMenu::new(
            &state,
            &unit,
            (400, 200),
            PixelRect::new(180, 18, 448, 448),
            64
        )
        .is_none());
    }
}
