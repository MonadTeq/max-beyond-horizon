//! Производство и строительство — очереди заводов и строителей (P2).
//!
//! # Оракул
//!
//! * `buildmenu.cpp` — состав меню и таблица «какой строитель что строит»
//!   (`Builder_GetBuildableUnits` → `GameRulesHandler` → `max_get_buildable_units`
//!   из `default.game_rules.lua`, [`BUILDABLE`]);
//! * `unitinfo.cpp` — `BuildOrder`, `StartBuilding`, `SpawnNewUnit`,
//!   `CancelBuilding`, `GetTurnsToBuild`;
//! * `game_manager.cpp` — `GameManager_ManageEconomy` (`UpdateProduction`,
//!   списание за ход) и `GameManager_ProgressBuildState`
//!   (`GameManager_SpawnNewUnits` — спавн в конце «хода производства»);
//! * `cargo.cpp` — `Cargo_GetRawConsumptionRate` (ставка расхода),
//!   `Cargo_ApplyUnitConsumption` (raw + fuel + power + life + gold);
//! * `production_manager.cpp` — `ProductionManager_OptimizeProduction`
//!   (энергия/рабочие; порт — [`crate::power`]).
//!
//! # Модель
//!
//! 1. У каждого строителя/завода есть очередь [`crate::unit::UnitInfo::build_list`],
//!    `build_time` (сколько «ходов постройки» осталось) и `build_rate`
//!    (1/2/4 — ускорение, `buildmenu.cpp:729`).
//! 2. Приказ `Build` ([`ProductionManager::commit`]) фиксирует первый элемент
//!    очереди: `build_time = turns`, `build_rate = rate`, `target_grid = site`
//!    (для мобильного строителя площадка ищется [`find_build_site`], как
//!    `UnitsManager_IssueBuildOrder`).
//! 3. Каждый ход [`ProductionManager::progress`] (вызывается из
//!    [`crate::game::GameState::finish_turn`]) **сначала** списывает стоимость
//!    хода, **потом** двигает/спавнит — порядок C++
//!    (`ManageEconomy` → `ProgressBuildState`).
//! 4. `build_time <= build_rate` → [`ProductionManager::spawn_new_unit`]:
//!    завод выпускает юнит в соседнюю клетку и получает
//!    `ORDER_STATE_UNIT_READY` (как `UnitInfo::SpawnNewUnit`), мобильный
//!    строитель достраивает здание на площадке: здание встаёт на клетку ленты
//!    в `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY` (не рисуется), строитель —
//!    `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` и **остаётся на площадке**
//!    ([`builder_awaiting_exit`]). Уход инициирует игрок кликом по клетке
//!    (`ORDER_ACTIVATE`/`IN_TRANSITION`, `game_manager.cpp:6879`) или ИИ задачей
//!    `TaskActivate` ([`crate::unit_order::issue_builder_exit`]); раскрытие
//!    (`ScaleUnit(EXPAND)`, снятие ленты, плита остаётся) делает
//!    [`crate::unit_order::process_orders`]-ветка `BuildingReady` только после
//!    ухода строителя с площадки (`UnitsManager_BuildingReady`).
//!
//! # Экономика
//!
//! C++ тратит на производство **материалы** (`Cargo_GetRawConsumptionRate`,
//! `cargo.cpp:140`), а не золото команды:
//!
//! * мобильный наземный строитель — со **своего склада** (`UnitInfo::
//!   UpdateProduction`, `unitinfo.cpp:2904-2915`): `storage -= rate`; порт
//!   делает то же в [`crate::game::GameState::update_production`], но **не
//!   уводя склад в минус**: если `storage < rate`, списания нет, а юнит
//!   получает `build_paused` и ход стройки не двигается
//!   ([`ProductionEventKind::PausedNoResources`]). C++ в этом случае уходит в
//!   долг и всё равно достраивает — порт так не делает (требование QA);
//! * стационарный завод — из **материалов своего комплекса**
//!   (`UnitInfo::StartBuilding`, `unitinfo.cpp:2757`:
//!   `complex->material -= Cargo_GetRawConsumptionRate`; `Cargo_GetNetProduction`,
//!   `cargo.cpp:88-95` на каждом ходу вычитает ту же ставку из баланса), а
//!   `ProductionManager::ValidateIndustry` (`production_manager.cpp:673-707`)
//!   прерывает стройку, когда материалов комплекса не хватает. Комплекс —
//!   связная группа зданий/коннекторов ([`crate::complex`]), а его запас и
//!   ёмкость — **сумма** подключённых хранилищ (`Complex::GetCargoInfo`,
//!   `complex.cpp:136-147`): `ADUMP`/`FDUMP`/`GOLDSM`/шахта в одной группе
//!   дают общий котёл. Ставка списывается со складов членов комплекса в
//!   порядке `UnitsManager_StationaryUnits` — как `Complex::Transfer`
//!   (`complex.cpp:178-203`). Нехватка ставит заказ на паузу
//!   ([`ProductionEventKind::PausedNoMaterials`]), заказ не теряется;
//! * **энергия и рабочие** — тоже из котла комплекса (`Cargo_ApplyUnitConsumption`,
//!   `cargo.cpp:132-138`): `power` тратит каждый строящийся завод, `life` —
//!   `TRAINHAL`/`GREENHSE`/`RESEARCH`. Перед стартом и на каждом ходу
//!   [`crate::power::optimize_complex`] включает производителей
//!   (`POWGEN`/`POWERSTN`), выключает потребителей при дефиците и ставит
//!   стройку на паузу (`PausedNoPower`/`PausedNoWorkers`); заказ **не
//!   стартует**, если первый ход не оплачивается ([`BuildError::NotEnoughPower`],
//!   [`BuildError::NotEnoughWorkers`]). Сообщение об этом —
//!   [`crate::power::IndustryOutcome::message`]
//!   (`production_manager.cpp:308-330/792-804`);
//! * **золото команды за производство не тратится** — в C++ оно нужно только
//!   `CargoMenu` (покупка на мобилизации) и апгрейдам. Старое поведение порта
//!   («завод платит золотом») и было причиной «заводы ничего не производят»:
//!   после мобилизации кредиты обычно нулевые, а шахты дают материалы.
//!
//! Итог за постройку при `rate = 1` — `cargo_raw_consumption_rate(builder, 1) * turns`,
//! ровно `UnitInfo::GetNormalRateBuildCost` / `CargoMenu::BuyUnit`.
//!
//! `MobileBuildMenu::Build` (`buildmenu.cpp:903-906`) требует при 1× всю
//! стоимость постройки на руках — [`BuildError::NotEnoughMaterials`].
//!
//! # Что упрощено (честно)
//!
//! * энергия/рабочие портированы в [`crate::power`] в объёме наблюдаемого
//!   баланса: точная арифметика `OptimizePowerProduction` с `count` заменена
//!   эквивалентным детерминированным выбором «станции → генераторы» (см.
//!   шапку `power`), а `OptimizeCargoProduction` (перераспределение добычи
//!   ради топлива) не портирован;
//! * выключенные при дефиците **потребители** (`RESEARCH`/`GREENHSE`/`COMMTWR`/
//!   `MININGST`) C++ сам обратно не включает — только игрок/ИИ; порт следует
//!   этому правилу (`production_manager.cpp:729-790`);
//! * завод выпускает юнит сразу в свободную соседнюю клетку, а не хранит его
//!   внутри и не «выезжает» по `AWAIT_TAPE_POSITIONING`;
//! * `quick_build` (настройка обучающих миссий) не моделируется;
//! * мобильный строитель ставит здание на `target_grid` без анимации
//!   переезда/`StartBuilding`-телепорта; стройплощадка (лента, конусы, плита)
//!   появляется сразу при заказе (`PrepareConstructionSite` в момент
//!   применения пакета 08/P4, `events::apply_command`), а не после 4 кадров
//!   `TAPE_POSITIONING`;
//! * анимация «вырастания» плиты/здания (`scaler_adjust`/`AwaitScaling`) идёт
//!   в симуляции, но кадр рендера её пока не масштабирует.

use crate::access;
use crate::deploy::{LARGE_SLAB_IMAGE_COUNT, SMALL_SLAB_IMAGE_COUNT};
use crate::enums::unit_flags;
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::unit_values::ATTRIB_TURNS;
use crate::victory::cargo_raw_consumption_rate;

/// Таблица `max_builder_capability_list` (`default.game_rules.lua`) — 1:1.
///
/// Порядок строк и элементов совпадает с Lua-файлом: от него зависит порядок
/// строк в `BuildMenu` (C++ `Builder_GetBuildableUnits`).
pub const BUILDABLE: [(UnitType, &[UnitType]); 7] = [
    (
        UnitType::Constructor,
        &[
            UnitType::MiningStation,
            UnitType::PowerStation,
            UnitType::LightVehiclePlant,
            UnitType::HeavyVehiclePlant,
            UnitType::AirUnitsPlant,
            UnitType::Shipyard,
            UnitType::GoldRefinery,
            UnitType::Depot,
            UnitType::Hangar,
            UnitType::Dock,
            UnitType::Habitat,
            UnitType::ResearchCentre,
            UnitType::EcoSphere,
            UnitType::TrainingHall,
            UnitType::Barracks,
        ],
    ),
    (
        UnitType::Engineer,
        &[
            UnitType::MaterialStorage,
            UnitType::FuelStorage,
            UnitType::GoldVault,
            UnitType::PowerGenerator,
            UnitType::Connector,
            UnitType::Radar,
            UnitType::GunTurret,
            UnitType::AntiAir,
            UnitType::Artillery,
            UnitType::MissileLauncher,
            UnitType::LandingPad,
            UnitType::Bridge,
            UnitType::WaterPlatform,
            UnitType::ConcreteBlock,
            UnitType::Road,
        ],
    ),
    (
        UnitType::LightVehiclePlant,
        &[
            UnitType::Scout,
            UnitType::Surveyor,
            UnitType::Engineer,
            UnitType::RepairUnit,
            UnitType::SupplyTruck,
            UnitType::FuelTruck,
            UnitType::GoldTruck,
            UnitType::MobileAntiAir,
            UnitType::MineLayer,
            UnitType::Bulldozer,
            UnitType::ArmouredPersonnelCarrier,
        ],
    ),
    (
        UnitType::HeavyVehiclePlant,
        &[
            UnitType::Constructor,
            UnitType::Scanner,
            UnitType::Tank,
            UnitType::AssaultGun,
            UnitType::RocketLauncher,
            UnitType::MissileCrawler,
        ],
    ),
    (
        UnitType::AirUnitsPlant,
        &[
            UnitType::Fighter,
            UnitType::Bomber,
            UnitType::AirTransport,
            UnitType::Awac,
        ],
    ),
    (
        UnitType::Shipyard,
        &[
            UnitType::Escort,
            UnitType::Corvette,
            UnitType::Gunboat,
            UnitType::Submarine,
            UnitType::SeaTransport,
            UnitType::MissileCruiser,
            UnitType::SeaMineLayer,
            UnitType::CargoShip,
        ],
    ),
    (
        UnitType::TrainingHall,
        &[UnitType::Infiltrator, UnitType::Infantry],
    ),
];

/// Заводы-производители (стационарные, `Cargo_GetPowerConsumptionRate == 1`).
pub const FACTORIES: [UnitType; 5] = [
    UnitType::Shipyard,
    UnitType::LightVehiclePlant,
    UnitType::HeavyVehiclePlant,
    UnitType::AirUnitsPlant,
    UnitType::TrainingHall,
];

/// Мобильные строители (`CONSTRCT`/`ENGINEER`).
pub const MOBILE_BUILDERS: [UnitType; 2] = [UnitType::Constructor, UnitType::Engineer];

/// Порядок обхода списков в `GameManager_ProgressBuildState` (C++): спавн
/// GroundCover → MobileLandSea → Stationary → MobileAir.
pub const PROGRESS_LIST_ORDER: [UnitListKind; 4] = [
    UnitListKind::GroundCover,
    UnitListKind::MobileLandSea,
    UnitListKind::Stationary,
    UnitListKind::MobileAir,
];

/// Порядок обхода списков в `GameManager_ManageEconomy` (C++ `UpdateProductions`):
/// MobileLandSea → Stationary → MobileAir (GroundCover не расходует).
pub const CHARGE_LIST_ORDER: [UnitListKind; 3] = [
    UnitListKind::MobileLandSea,
    UnitListKind::Stationary,
    UnitListKind::MobileAir,
];

/// Радиус поиска площадки под здание (`TaskCreateBuilding::FindBuildSite`).
pub const SITE_SEARCH_RADIUS: i16 = 8;

// ---------------------------------------------------------------------------
// Таблицы «кто что строит»
// ---------------------------------------------------------------------------

/// `Builder_GetBuildableUnits(team, builder)` — список доступных постройек.
///
/// Пустой срез — строитель не умеет строить (обычный юнит).
pub fn buildable_units(builder: UnitType) -> &'static [UnitType] {
    for (unit_type, units) in BUILDABLE {
        if unit_type == builder {
            return units;
        }
    }

    &[]
}

/// `Builder_GetBuildableUnits(team, unit_type)` — хватает ли строителя.
pub fn can_build(builder: UnitType, unit_type: UnitType) -> bool {
    buildable_units(builder).contains(&unit_type)
}

/// `Builder_GetBuilderType(team, unit_type)` — кто строит этот тип (первый в
/// таблице, как Lua-цикл `pairs` по фиксированному порядку таблицы).
pub fn builder_type(unit_type: UnitType) -> Option<UnitType> {
    for (builder, units) in BUILDABLE {
        if units.contains(&unit_type) {
            return Some(builder);
        }
    }

    None
}

/// Стационарный завод-производитель (`SpawnNewUnit`-ветка `flags & STATIONARY`).
pub fn is_factory(unit_type: UnitType) -> bool {
    FACTORIES.contains(&unit_type)
}

/// Мобильный строитель (`CONSTRCT`/`ENGINEER`).
pub fn is_mobile_builder(unit_type: UnitType) -> bool {
    MOBILE_BUILDERS.contains(&unit_type)
}

/// Есть ли у типа очередь постройки (завод или мобильный строитель).
pub fn is_builder(unit_type: UnitType) -> bool {
    !buildable_units(unit_type).is_empty()
}

// ---------------------------------------------------------------------------
// Стоимость и время
// ---------------------------------------------------------------------------

/// `BuildMenu_GetTurnsToBuild(unit_type, team)` (`buildmenu.cpp:688`).
///
/// Для компьютера действует множитель сложности (`opponent`), как в C++.
/// `quick_build` (обучающие миссии) в порте не моделируется.
pub fn turns_to_build(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
    let turns = state
        .team_units
        .get(team.0 as usize)
        .map(|units| {
            units
                .current_unit_values(unit_type as u16)
                .get_attribute(crate::unit_values::ATTRIB_TURNS)
        })
        .unwrap_or(0);

    if state.teams.get(team.0 as usize).map(|info| info.team_type) != Some(TeamKind::Computer) {
        return turns;
    }

    match state.ai.settings.opponent {
        opponent if opponent == crate::enums::Opponent::Master as u8 => (turns * 4 + 2) / 5,
        opponent if opponent == crate::enums::Opponent::God as u8 => (turns * 2 + 1) / 3,
        opponent if opponent == crate::enums::Opponent::Clueless as u8 => (turns * 5) / 4,
        _ => turns,
    }
}

/// Стоимость одного хода постройки — `Cargo_GetRawConsumptionRate`.
///
/// Мобильный строитель платит ею сырьём со своего склада, завод — материалами
/// своего комплекса; золото команды не тратится (см. шапку модуля). Для
/// `rate = 1` суммарная стоимость равна
/// `Cargo_GetRawConsumptionRate(builder, 1) * turns` — C++-стоимость постройки.
pub fn cost_per_turn(builder: UnitType, build_rate: i32) -> i32 {
    cargo_raw_consumption_rate(builder, build_rate)
}

/// Полная стоимость постройки при скорости 1× — C++-формула
/// `GetNormalRateBuildCost` для юнита, который строит `builder`.
pub fn total_cost(builder: UnitType, turns: i32) -> i32 {
    cost_per_turn(builder, 1) * turns.max(0)
}

/// `UnitInfo::GetTurnsToBuild` (`unitinfo.cpp:3393`) — сколько ходов займёт
/// постройка и сколько всего сырья она спишет при скорости
/// `build_speed_multiplier` (`1/2/4`).
///
/// Возвращает `(ходов, полный расход)` — ровно то, что C++
/// `AbstractBuildMenu::Draw` печатает в колонках `Turns`/`Cost`.
///
/// * база — `build_time`, если строитель уже строит этот тип (голова очереди,
///   `build_time > 0`), иначе текущее `ATTRIB_TURNS` типа (в нём уже учтены
///   клановые модификаторы `SC_CLANS` и исследования — C++
///   `UnitsManager_GetCurrentUnitValues`);
/// * на каждом ходу скорость ограничивается
///   [`max_possible_build_rate`] по **оставшимся** ходам и складу строителя
///   (`INT16_MAX` у стационарных): `min(build_speed_multiplier, limit)`;
/// * расход — `Cargo_GetRawConsumptionRate(builder, effective_rate)`, склад
///   уменьшается на него (у мобильного строителя, как C++ `storage -= ...`).
pub fn turns_and_consumption(
    state: &GameState,
    builder: &UnitInfo,
    unit_type: UnitType,
    build_speed_multiplier: i32,
) -> (i32, i32) {
    let mut turns =
        if builder.build_time > 0 && builder.build_list.first().copied() == Some(unit_type) {
            i32::from(builder.build_time)
        } else {
            state
                .team_units
                .get(builder.team.0 as usize)
                .map(|units| {
                    units
                        .current_unit_values(unit_type as u16)
                        .get_attribute(ATTRIB_TURNS)
                })
                .unwrap_or(0)
        };

    let mut local_storage = if builder.is_ground() || builder.is_amphibious() {
        i32::from(builder.storage)
    } else {
        i32::from(i16::MAX)
    };

    let mut multiplier = build_speed_multiplier.max(1);
    let mut turns_to_build = 0;
    let mut consumption = 0;

    while turns > 0 {
        let limit = max_possible_build_rate(builder.unit_type, turns, local_storage);
        multiplier = multiplier.min(limit);

        let cost = cargo_raw_consumption_rate(builder.unit_type, multiplier);
        consumption += cost;
        turns_to_build += 1;
        local_storage -= cost;
        turns -= multiplier;
    }

    (turns_to_build, consumption)
}

/// `BuildMenu_GetMaxPossibleBuildRate(unit_type, build_time, storage)`
/// (`buildmenu.cpp:729`) — 1:1.
///
/// `unit_type` — тип **строителя** (C++ передаёт `unit->GetUnitType()`), а
/// `storage` — его склад (для зданий C++ передаёт `INT16_MAX`).
pub fn max_possible_build_rate(unit_type: UnitType, build_time: i32, storage: i32) -> i32 {
    const RATE_1X: i32 = 1;
    const RATE_2X: i32 = 2;
    const RATE_4X: i32 = 4;

    if build_time <= RATE_1X {
        return RATE_1X;
    }

    let mut build_speed_multiplier = if build_time > RATE_2X {
        RATE_4X
    } else {
        RATE_2X
    };

    if unit_type.is_ground() || unit_type.is_amphibious() {
        let cost_1x = cargo_raw_consumption_rate(unit_type, 1);
        let cost_2x = cargo_raw_consumption_rate(unit_type, 2);
        let cost_4x = cargo_raw_consumption_rate(unit_type, 4);
        let mut turns_1x = build_time / RATE_1X;
        let mut turns_2x = 0;
        let mut turns_4x = 0;

        let const_change_2x = cost_2x - cost_1x * (RATE_2X / RATE_1X);
        let mut cost = turns_1x * cost_1x + const_change_2x;

        while cost <= storage {
            if turns_1x >= RATE_2X / RATE_1X {
                turns_2x += 1;
                turns_1x -= RATE_2X / RATE_1X;
                cost += const_change_2x;
            } else {
                break;
            }
        }

        let const_change_4x = cost_4x - cost_2x * (RATE_4X / RATE_2X);
        cost = turns_1x * cost_1x + turns_2x * cost_2x + const_change_4x;

        while cost <= storage {
            if turns_2x >= RATE_4X / RATE_2X {
                turns_4x += 1;
                turns_2x -= RATE_4X / RATE_2X;
                cost += const_change_4x;
            } else {
                if turns_2x > 0 && turns_1x == 1 && cost - RATE_2X - RATE_1X + RATE_4X <= storage {
                    turns_4x += 1;
                }

                break;
            }
        }

        build_speed_multiplier = if turns_4x != 0 {
            RATE_4X
        } else if turns_2x != 0 {
            RATE_2X
        } else {
            RATE_1X
        };
    }

    build_speed_multiplier
}

// ---------------------------------------------------------------------------
// Ошибки и отчёты
// ---------------------------------------------------------------------------

/// Почему заказ/приказ постройки отклонён (`BuildMenu::Build` в C++).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    /// Юнит не строитель (нет очереди постройки).
    NotABuilder,
    /// `Builder_IsBuildable` ложно: строитель не умеет строить этот тип.
    NotBuildable,
    /// Очередь пуста (`FactoryBuildMenu::Build` без выбранных юнитов).
    EmptyQueue,
    /// Нет площадки (`UnitsManager_IssueBuildOrder` == false).
    NoSite,
    /// Команда не найдена в состоянии.
    InvalidTeam,
    /// `BuildMenu::Build` отказал: при `build_rate == 1` у мобильного строителя
    /// не хватает сырья на всю постройку (`buildmenu.cpp:903-906`, сообщение
    /// `_(c529)`). C++ не отдаёт приказ — иначе постройка встала бы навсегда.
    NotEnoughMaterials,
    /// Завод не может оплатить первый ход: комплексу не хватает энергии
    /// (`Cargo_GetPowerConsumptionRate` → `Cargo_ApplyUnitConsumption`,
    /// `production_manager.cpp:677-711`; в порте — отказ вместо `BUILD_ABORT`,
    /// чтобы не терять очередь).
    NotEnoughPower,
    /// Заводу не хватает топлива на производителей энергии его комплекса
    /// (`PowerOn`/`OptimizePowerProduction`, `production_manager.cpp:332-364`).
    NotEnoughFuel,
    /// Заводу не хватает рабочих (`TRAINHAL`/`GREENHSE`/`RESEARCH`:
    /// `Cargo_GetLifeConsumptionRate = 1`, `production_manager.cpp:366-380`).
    NotEnoughWorkers,
    /// Заводу не хватает золота (`COMMTWR`: `Cargo_GetGoldConsumptionRate = 5`).
    NotEnoughGold,
}

impl BuildError {
    /// Текст для UI/лога.
    pub const fn message(self) -> &'static str {
        match self {
            Self::NotABuilder => "unit cannot build",
            Self::NotBuildable => "unit is not buildable here",
            Self::EmptyQueue => "build queue is empty",
            Self::NoSite => "no valid build site nearby",
            Self::InvalidTeam => "unknown team",
            Self::NotEnoughMaterials => "not enough raw materials on hand",
            Self::NotEnoughPower => "not enough power in the complex",
            Self::NotEnoughFuel => "not enough fuel for the power plants",
            Self::NotEnoughWorkers => "not enough workers in the complex",
            Self::NotEnoughGold => "not enough gold in the complex",
        }
    }
}

/// Результат `Build` (C++ `BuildMenu_EventStrings_Available*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildReport {
    /// Что будет построено (голова очереди).
    pub unit_type: UnitType,
    /// Ходов постройки (`GET_TURNS`).
    pub turns: i32,
    /// Скорость (1/2/4).
    pub build_rate: u16,
    /// Стоимость одного хода в золоте.
    pub cost_per_turn: i32,
    /// Площадка (для мобильного строителя).
    pub site: Point,
}

/// Тип события производства.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionEventKind {
    /// Завод выпустил юнит (`SpawnNewUnit`).
    Spawned,
    /// Мобильный строитель достроил здание.
    BuildingDone,
    /// Постройка приостановлена: у мобильного строителя не хватает **сырья** на
    /// складе (`storage < Cargo_GetRawConsumptionRate`) — C++
    /// `UnitInfo::UpdateProduction` в этом случае уходит в минус, порт не
    /// двигает стройку (см. [`UnitInfo::build_paused`]).
    PausedNoResources,
    /// Постройка приостановлена: у команды не хватает **материалов** на
    /// текущий ход завода (`Cargo_GetRawConsumptionRate` → материалы базы).
    /// C++ `ProductionManager::ValidateIndustry` прерывает стройку
    /// (`ORDER_STATE_BUILD_ABORT`) и возобновляет её позже; порт держит заказ в
    /// очереди на паузе.
    PausedNoMaterials,
    /// Спавн отложен: рядом нет свободной клетки (юнит остаётся в очереди).
    NoSpawnCell,
    /// Бульдозер расчистил обломок (`UnitInfo::SpawnNewUnit`, ветка BULLDOZR).
    RubbleCleared,
    /// Стройка завода приостановлена: комплексу не хватает **энергии**
    /// (`Cargo_GetPowerConsumptionRate` → `Cargo_ApplyUnitConsumption`;
    /// C++ `ValidateIndustry` выключает стройку, порт — пауза).
    PausedNoPower,
    /// Стройка завода приостановлена: не хватает **топлива** производителям
    /// энергии (`CalculatePowerConsumption`/`CheckPowerNeed`).
    PausedNoFuel,
    /// Стройка завода приостановлена: комплексу не хватает **рабочих**
    /// (`Cargo_GetLifeConsumptionRate`, `production_manager.cpp:366-380`).
    PausedNoWorkers,
    /// Стройка завода приостановлена: не хватает **золота** (`COMMTWR`).
    PausedNoGold,
}

/// Событие производства — для HUD-лога и QA-ассертов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionEvent {
    /// Строитель/завод.
    pub builder: UnitId,
    /// Что строилось.
    pub unit_type: UnitType,
    /// Что появилось (если появилось).
    pub spawned: Option<UnitId>,
    /// Итог шага.
    pub kind: ProductionEventKind,
}

/// Событие паузы по ресурсу из [`crate::power::Shortage`].
fn pause_event_kind(shortage: crate::power::Shortage) -> ProductionEventKind {
    use crate::power::Shortage;

    match shortage {
        Shortage::Power => ProductionEventKind::PausedNoPower,
        Shortage::Fuel => ProductionEventKind::PausedNoFuel,
        Shortage::Workers => ProductionEventKind::PausedNoWorkers,
        Shortage::Gold => ProductionEventKind::PausedNoGold,
        Shortage::Raw => ProductionEventKind::PausedNoMaterials,
    }
}

// ---------------------------------------------------------------------------
// Площадка и клетка выпуска
// ---------------------------------------------------------------------------

/// Площадка под здание/юнит: все клетки внутри карты, свободны и проходимы.
fn site_is_free(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    site: Point,
    size: i16,
) -> bool {
    for dy in 0..size {
        for dx in 0..size {
            let cell = Point::new(site.x + dx, site.y + dy);

            if state.map.index(cell.x, cell.y).is_none() {
                return false;
            }

            if !state.units_at(cell).is_empty() {
                return false;
            }

            if crate::access::is_accessible(
                state,
                unit_type,
                team,
                cell,
                crate::access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS,
            ) == 0
            {
                return false;
            }
        }
    }

    true
}

/// Габарит постройки в клетках (здания — 2×2, `BUILDING`).
pub fn footprint(unit_type: UnitType) -> i16 {
    if crate::access::unit_type_flags(unit_type) & unit_flags::BUILDING != 0 {
        2
    } else {
        1
    }
}

/// `UnitsManager_IsAccessible` (`units_manager.cpp:4982`): проходим ли весь
/// footprint типа в клетке `site` (2×2 у зданий, 1×1 у остальных).
///
/// В отличие от [`find_build_site`] (поиск ИИ/запасной путь) здесь **нет**
/// требования «в клетке нет юнитов»: C++ проверяет только
/// `Access_IsAccessible(..., AccessModifier_SameClassBlocks)`, поэтому стройку
/// можно разметить поверх своих мобильных юнитов — их C++ не считает
/// препятствием зданию (ветка `flags & STATIONARY` в `Access_IsAccessible`).
pub fn is_site_accessible(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    site: Point,
) -> bool {
    is_site_accessible_ignoring(state, team, unit_type, site, &[])
}

/// [`is_site_accessible`] с юнитами, которых C++ вынул из хэша на время
/// проверки (`UnitsManager_MoveUnitAndParent`/`IssueBuildOrder` убирают
/// строителя и временную ленту, иначе 2×2 «занята» самим строителем).
pub fn is_site_accessible_ignoring(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    site: Point,
    ignored: &[UnitId],
) -> bool {
    let size = footprint(unit_type);

    for dy in 0..size {
        for dx in 0..size {
            let cell = Point::new(site.x + dx, site.y + dy);

            if state.map.index(cell.x, cell.y).is_none() {
                return false;
            }

            if crate::access::is_accessible_ignoring(
                state,
                unit_type,
                team,
                cell,
                crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                ignored,
            ) == 0
            {
                return false;
            }
        }
    }

    true
}

/// `UnitsManager_IssueBuildOrder` (`units_manager.cpp:4998`): стартовая клетка
/// «ленты» площадки при заказе здания в `BuildMenu`.
///
/// C++ перебирает ровно четыре якоря 2×2, все они содержат клетку строителя
/// (сдвиг не больше одной клетки по каждой оси):
/// `(x, y)`, `(x, y-1)`, `(x-1, y-1)`, `(x-1, y)` — порядок важен, потому что
/// первый проходимый якорь становится стартовым положением призрака.
pub fn initial_build_site(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    builder_grid: Point,
    ignored: &[UnitId],
) -> Option<Point> {
    if footprint(unit_type) == 1 {
        // C++ для не-зданий проверяет только собственную клетку строителя.
        return is_site_accessible_ignoring(state, team, unit_type, builder_grid, ignored)
            .then_some(builder_grid);
    }

    let x = builder_grid.x;
    let y = builder_grid.y;

    [(x, y), (x, y - 1), (x - 1, y - 1), (x - 1, y)]
        .into_iter()
        .map(|(x, y)| Point::new(x, y))
        .find(|site| is_site_accessible_ignoring(state, team, unit_type, *site, ignored))
}

/// Якорь площадки для курсора в режиме выбора места — порт курсорной части
/// `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5845`) для
/// конструктора.
///
/// C++ привязывает 2×2 к строителю: курсор правее строителя — якорь
/// `builder.x`, левее — `builder.x - 1`; ровно на колонке строителя якорь
/// сохраняется прежним (поэтому передаётся `previous`). По каждой оси
/// независимо; стройплощадка всегда накрывает клетку строителя.
pub fn site_anchor_for_cursor(builder_grid: Point, cursor: Point, previous: Point) -> Point {
    let anchor_x = if cursor.x < builder_grid.x {
        builder_grid.x - 1
    } else if cursor.x > builder_grid.x {
        builder_grid.x
    } else {
        previous.x
    };
    let anchor_y = if cursor.y < builder_grid.y {
        builder_grid.y - 1
    } else if cursor.y > builder_grid.y {
        builder_grid.y
    } else {
        previous.y
    };

    Point::new(anchor_x, anchor_y)
}

/// Курсорная часть `GameManager_GetBuilderUnitCursor` для `ENGINEER`
/// (`game_manager.cpp:5854-5909`).
///
/// Инженер ставит **1×1**-ленту (`SMLTAPE`) ровно в клетку под курсором:
/// C++ не привязывает её к строителю (`target_grid_x = grid_x`,
/// `target_grid_y = grid_y`), но разрешает только клетки **в своей строке или
/// столбце** (`grid_x == unit_grid_x || grid_y == unit_grid_y`), иначе
/// `CURSOR_UNIT_NO_GO`. Проходимость проверяет `UnitsManager_MoveUnitAndParent`
/// (в порте — [`is_site_accessible_ignoring`] без строителя и ленты).
pub fn engineer_site_covers(builder_grid: Point, cursor: Point) -> bool {
    cursor.x == builder_grid.x || cursor.y == builder_grid.y
}

/// Накрывает ли площадка типа `unit_type` с якорем `anchor` клетку `cursor`
/// (`UnitsManager_MoveUnitAndParent` + проверка попадания в `GameManager_GetBuilderUnitCursor`).
pub fn site_covers(unit_type: UnitType, anchor: Point, cursor: Point) -> bool {
    let size = footprint(unit_type);

    cursor.x >= anchor.x
        && cursor.x < anchor.x + size
        && cursor.y >= anchor.y
        && cursor.y < anchor.y + size
}

/// `TaskCreateBuilding::FindBuildSite` — ближайшая свободная площадка.
///
/// Порядок обхода детерминированный: по возрастанию расстояния, затем `y`,
/// затем `x` (никаких хэш-таблиц), поэтому два прогона совпадают.
pub fn find_build_site(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    origin: Point,
) -> Option<Point> {
    let size = footprint(unit_type);
    let mut candidates: Vec<(i32, i16, i16)> = Vec::new();

    for dy in -SITE_SEARCH_RADIUS..=SITE_SEARCH_RADIUS {
        for dx in -SITE_SEARCH_RADIUS..=SITE_SEARCH_RADIUS {
            let x = origin.x + dx;
            let y = origin.y + dy;

            if x < 0 || y < 0 {
                continue;
            }

            candidates.push((
                i32::from(dx) * i32::from(dx) + i32::from(dy) * i32::from(dy),
                y,
                x,
            ));
        }
    }

    candidates.sort_unstable();

    for (_, y, x) in candidates {
        let site = Point::new(x, y);

        if site_is_free(state, team, unit_type, site, size) {
            return Some(site);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Стройплощадка (C++ `DeployConstructionSiteUtilities` + `PrepareConstructionSite`)
// ---------------------------------------------------------------------------

/// `DeployConstructionSiteUtilities` (`unitinfo.cpp:3802`): пара «лента+конусы».
///
/// Здание (флаг `BUILDING`) получает большие декали `LRGTAPE`/`LRGCONES`
/// (2×2), всё остальное — малые `SMLTAPE`/`SMLCONES` (1×1).
pub fn construction_utility_types(unit_type: UnitType) -> (UnitType, UnitType) {
    if access::unit_type_flags(unit_type) & unit_flags::BUILDING != 0 {
        (UnitType::LargeTape, UnitType::LargeCones)
    } else {
        (UnitType::SmallTape, UnitType::SmallCones)
    }
}

/// `PrepareConstructionSite` (`unitinfo.cpp:4122`): плита под постройкой.
///
/// `LRGSLAB` для зданий, `SMLSLAB` для остальных типов; создаётся только при
/// флаге `REQUIRES_SLAB`.
pub fn construction_slab_type(unit_type: UnitType) -> UnitType {
    if access::unit_type_flags(unit_type) & unit_flags::BUILDING != 0 {
        UnitType::LargeSlab
    } else {
        UnitType::SmallSlab
    }
}

/// `image_count` плиты — второй байт `D_LRGSLA`/`D_DEFALT` (`MAX.RES`).
fn slab_image_count(unit_type: UnitType) -> i32 {
    if access::unit_type_flags(unit_type) & unit_flags::BUILDING != 0 {
        LARGE_SLAB_IMAGE_COUNT
    } else {
        SMALL_SLAB_IMAGE_COUNT
    }
}

/// Что поставила стройплощадка (для тестов/QA; id позволяют проверить уборку).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstructionSite {
    /// Клетка-источник площадки (`DeployConstructionSiteUtilities`).
    pub site: Point,
    /// `LRGTAPE`/`SMLTAPE` — родитель-строитель, стоит до конца стройки.
    pub tape: UnitId,
    /// `LRGCONES`/`SMLCONES`; `None` на воде/побережье (C++ не ставит конусы).
    pub cones: Option<UnitId>,
    /// `LRGSLAB`/`SMLSLAB` — плита; `None`, если тип не `REQUIRES_SLAB`.
    pub slab: Option<UnitId>,
}

/// Лента площадки команды в клетке (`Access_GetConstructionUtility`,
/// `access.cpp:1773`).
pub fn construction_utility(state: &GameState, team: TeamId, site: Point) -> Option<UnitId> {
    state.units_at(site).iter().copied().find(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.team == team && matches!(unit.unit_type, UnitType::LargeTape | UnitType::SmallTape)
        })
    })
}

/// Поставить стройплощадку — порт `DeployConstructionSiteUtilities` +
/// `PrepareConstructionSite` в точке `site`.
///
/// Раскладка C++ (`unitinfo.cpp:3802/4122`):
/// * конусы `LRGCONES`/`SMLCONES` — в клетке площадки, угол `0`; на
///   воде/побережье (`SURFACE_TYPE_WATER|COAST`) **не ставятся**;
/// * лента `LRGTAPE`/`SMLTAPE` — там же, угол `0` на земле и `1` на воде,
///   родитель — строитель;
/// * плита `LRGSLAB`/`SMLSLAB` (если `REQUIRES_SLAB`) — там же, кадр
///   `Randomizer_Generate(image_count)` (5 у большой, 1 у малой), затем
///   `scaler_adjust = 4` + `ScaleUnit(EXPAND)` — анимация роста.
///
/// Функция идемпотентна по клетке: `StartBuilding` в C++ вызывается один раз,
/// но порт зовёт её ещё из `ProductionManager::commit` (заказ применяется сразу,
/// а не на следующем кадре) и из машины приказов — вторая площадка не нужна.
pub fn deploy_construction_site(
    state: &mut GameState,
    builder: UnitId,
    unit_type: UnitType,
    site: Point,
) -> Option<ConstructionSite> {
    let Some(builder_unit) = state.unit(builder) else {
        return None;
    };
    let team = builder_unit.team;

    if construction_utility(state, team, site).is_some() {
        return None;
    }

    let (tape_type, cones_type) = construction_utility_types(unit_type);
    let surface = access::modified_surface_type(state, site, false);
    let on_water = surface == access::SURFACE_TYPE_WATER || surface == access::SURFACE_TYPE_COAST;
    let mut cones = None;

    if !on_water {
        cones = state.spawn_unit(cones_type, team, site, 0);
        if let Some(id) = cones {
            set_site_unit_orders(state, id, None);
        }
    }

    let tape = state.spawn_unit(tape_type, team, site, 0)?;
    if let Some(unit) = state.unit_mut(tape) {
        unit.angle = if on_water { 1 } else { 0 };
        unit.storage = 0;
    }
    set_site_unit_orders(state, tape, Some(builder));

    let slab = if access::unit_type_flags(unit_type) & unit_flags::REQUIRES_SLAB != 0 {
        let slab_type = construction_slab_type(unit_type);
        let angle = state.rng.generate_i32(slab_image_count(unit_type)) as u8;
        let id = state.spawn_unit(slab_type, team, site, 0);

        if let Some(id) = id {
            if let Some(unit) = state.unit_mut(id) {
                unit.angle = angle;
                unit.storage = 0;
            }
            set_site_unit_orders(state, id, None);

            // C++ `PrepareConstructionSite`: `scaler_adjust = 4` и
            // `UnitsManager_ScaleUnit(unit, ORDER_STATE_EXPAND)`.
            if let Some(unit) = state.unit_mut(id) {
                unit.scaler_adjust = 4;
            }
            state.scale_unit(id, UnitOrderState::Expand);
        }

        id
    } else {
        None
    };

    Some(ConstructionSite {
        site,
        tape,
        cones,
        slab,
    })
}

/// Новые декали площадки — `ORDER_AWAIT`/`ORDER_STATE_EXECUTING_ORDER`, как
/// `UnitInfo::UnitInfo` (`unitinfo.cpp:174`); иначе `App::draw_units` пропустит
/// их как `ORDER_IDLE` и площадка будет невидима.
fn set_site_unit_orders(state: &mut GameState, id: UnitId, parent: Option<UnitId>) {
    if let Some(unit) = state.unit_mut(id) {
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.prior_orders = UnitOrder::Await;
        unit.prior_state = UnitOrderState::ExecutingOrder;
        unit.parent_unit = parent;
    }
}

/// Строитель достроил здание и **ждёт приказа уйти с площадки**.
///
/// C++ `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3536-3592`, не стационарный
/// строитель): здание ставится на клетку ленты в `ORDER_IDLE`/
/// `ORDER_STATE_BUILDING_READY` и не рисуется (`UnitInfoGroup::IsRelevant`),
/// строитель получает `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` и остаётся на
/// площадке; взаимные родители (`this->SetParent(&*utility_unit)` +
/// `utility_unit->SetParent(this)`). Уход инициирует **игрок** — клик по клетке
/// (`game_manager.cpp:6879-6890`: у выделенного строителя состояние
/// `ORDER_STATE_UNIT_READY` → `ORDER_ACTIVATE`/`ORDER_STATE_IN_TRANSITION`), у
/// ИИ — `TaskCreateBuilding::Activate`/`TaskActivate` (`taskactivate.cpp:104`).
///
/// Возвращает id здания-родителя, пока строитель стоит на площадке.
pub fn builder_awaiting_exit(state: &GameState, builder_id: UnitId) -> Option<UnitId> {
    let builder = state.unit(builder_id)?;

    if builder.orders != UnitOrder::Build || builder.state != UnitOrderState::UnitReady {
        return None;
    }

    let building = state.unit(builder.parent_unit?)?;

    if building.orders == UnitOrder::Idle && building.state == UnitOrderState::BuildingReady {
        Some(building.id)
    } else {
        None
    }
}

/// Клетка, куда строитель уходит с достроенной площадки — порт поиска
/// `TaskActivate::Activate` (`taskactivate.cpp:104-167`).
///
/// C++ перебирает клетки вокруг родителя-здания (2×2): старт в
/// `(grid_x - 1, grid_y + unit_size)`, затем четыре направления
/// (`N, E, S, W` — `direction += 2`) по `unit_size + 1` шагов, накопительно —
/// первая проходимая клетка становится целью. Это цель **ИИ** (`TaskActivate`);
/// игрок кликом задаёт любую клетку (`move_to` = клик), поэтому у него
/// `builder_exit_cell` не используется.
pub fn builder_exit_cell(
    state: &GameState,
    builder: &UnitInfo,
    building: &UnitInfo,
) -> Option<Point> {
    crate::unit_order::activation_exit_cell(state, builder, building)
}

/// Прямоугольник 2×2 (здание) или 1×1 в клетках — `UnitInfo::GetBounds` +
/// `Access_IsInsideBounds` (`access.cpp`).
pub fn unit_bounds_contains(unit: &UnitInfo, grid: Point) -> bool {
    let size: i16 = if unit.flags & unit_flags::BUILDING != 0 {
        2
    } else {
        1
    };

    grid.x >= unit.grid.x
        && grid.y >= unit.grid.y
        && grid.x < unit.grid.x.wrapping_add(size)
        && grid.y < unit.grid.y.wrapping_add(size)
}

/// Клетка, с которой готовый юнит завода **выходит** из него — порт
/// `TaskActivate::Activate` (`taskactivate.cpp:104-167`) для приказа
/// `ORDER_ACTIVATE`.
///
/// C++ перебирает клетки вокруг контейнера, начиная с
/// `(grid_x - 1, grid_y + unit_size)`, затем четыре направления (`N, E, S, W` —
/// `direction += 2`) по `unit_size + 1` шагов, накопительно; первая проходимая
/// клетка становится целью. `unit_size` — габарит **контейнера** (2 у здания),
/// проходимость проверяется для **выводимого** типа.
///
/// Возвращает `None`, если рядом нет проходимой клетки — тогда C++
/// (`UnitsManager_ActivateUnit`, `units_manager.cpp:3832-3836`) приказ не
/// выполняет и возвращает прежний.
pub fn factory_exit_cell(
    state: &GameState,
    container: &UnitInfo,
    client: &UnitInfo,
) -> Option<Point> {
    crate::unit_order::activation_exit_cell(state, client, container)
}

// ---------------------------------------------------------------------------
// Материалы комплекса (C++ `Complex::GetCargoInfo` + `Complex::Transfer`)
// ---------------------------------------------------------------------------

/// Материалы комплекса — `Complex::GetCargoInfo(materials, capacity)` →
/// `materials.raw` (`complex.cpp:136-147`).
///
/// Сумма `storage` **всех подключённых** зданий комплекса с типом груза
/// `CARGO_TYPE_RAW` (`victory::cargo_type_of`): шахты, `ADUMP`, теплицы и
/// казармы. Именно этот «общий котёл» тратит завод
/// (`UnitInfo::StartBuilding`, `unitinfo.cpp:2757`).
pub fn complex_raw_materials(
    state: &GameState,
    team: TeamId,
    complex: crate::team::ComplexId,
) -> i32 {
    crate::alloc::get_cargo_info(state, team, complex).0.raw
}

/// Списывает `amount` материалов **комплекса**: порт `Complex::Transfer`
/// (`complex.cpp:178-203`) для отрицательной суммы.
///
/// C++ раскладывает отрицательную сумму по складам членов комплекса в порядке
/// `UnitsManager_StationaryUnits` (`Complex::TransferCargo`), поэтому списание
/// детерминировано. Возвращает `false` (ничего не списав), если материалов
/// меньше ставки — C++-долг (`storage`/`complex->material` в минус) порт не
/// допускает (требование QA), завод вместо этого встаёт на паузу.
fn spend_complex_raw_materials(
    state: &mut GameState,
    team: TeamId,
    complex: crate::team::ComplexId,
    amount: i32,
) -> bool {
    if amount <= 0 {
        return true;
    }

    if complex_raw_materials(state, team, complex) < amount {
        return false;
    }

    let mut remaining = amount;
    let raw = crate::victory::CargoType::Raw;

    for id in state.unit_list(UnitListKind::Stationary).to_vec() {
        if remaining <= 0 {
            break;
        }

        let Some(unit) = state.unit(id).cloned() else {
            continue;
        };

        if unit.team != team
            || unit.complex != Some(complex)
            || crate::victory::cargo_type_of(unit.unit_type) != raw
        {
            continue;
        }

        let take = i32::from(unit.storage).max(0).min(remaining);

        if take > 0 {
            if let Some(target) = state.unit_mut(id) {
                target.storage -= take as i16;
            }
            remaining -= take;
        }
    }

    remaining == 0
}

// ---------------------------------------------------------------------------
// ProductionManager
// ---------------------------------------------------------------------------

/// Менеджер производства команды — порт `ProductionManager`/`BuildMenu`-части
/// `production_manager.cpp` + `unitinfo.cpp` (`BuildOrder`/`SpawnNewUnit`).
///
/// Держит `&mut GameState`: все изменения (очередь, `build_time`, всплытие
/// юнита) применяются к состоянию сразу, поэтому сетевой/локальный цикл видит
/// один и тот же результат.
pub struct ProductionManager<'a> {
    state: &'a mut GameState,
    team: TeamId,
}

impl<'a> ProductionManager<'a> {
    /// Менеджер для команды.
    pub fn new(state: &'a mut GameState, team: TeamId) -> Self {
        Self { state, team }
    }

    /// Команда менеджера.
    pub const fn team(&self) -> TeamId {
        self.team
    }

    /// `Builder_GetBuildableUnits` для строителя.
    pub fn buildable(&self, builder: UnitType) -> &'static [UnitType] {
        buildable_units(builder)
    }

    /// `BuildMenu_GetTurnsToBuild` для типа.
    pub fn turns_to_build(&self, unit_type: UnitType) -> i32 {
        turns_to_build(self.state, self.team, unit_type)
    }

    /// Текущая очередь постройки юнита (копия).
    pub fn queue(&self, unit_id: UnitId) -> Vec<UnitType> {
        self.state
            .unit(unit_id)
            .map(|unit| unit.build_list.clone())
            .unwrap_or_default()
    }

    /// `FactoryBuildMenu::AddSelection` — добавить тип в конец очереди.
    ///
    /// Для мобильного строителя очередь — ровно один элемент (`Build` заменяет
    /// выбор, как `MobileBuildMenu::Build`: `build_list.Clear(); PushBack`).
    pub fn enqueue(&mut self, unit_id: UnitId, unit_type: UnitType) -> Result<usize, BuildError> {
        let Some(mut unit) = self.state.unit(unit_id).cloned() else {
            return Err(BuildError::NotABuilder);
        };

        if unit.team != self.team || !is_builder(unit.unit_type) {
            return Err(BuildError::NotABuilder);
        }

        if !can_build(unit.unit_type, unit_type) {
            return Err(BuildError::NotBuildable);
        }

        if is_mobile_builder(unit.unit_type) {
            unit.build_list.clear();
            unit.repeat_build = false;
        }

        unit.build_list.push(unit_type);
        let len = unit.build_list.len();
        self.state.write_unit(unit_id, unit);

        Ok(len)
    }

    /// `FactoryBuildMenu::RemoveSelection` — убрать последний выбранный тип.
    pub fn dequeue_last(&mut self, unit_id: UnitId) -> bool {
        let Some(mut unit) = self.state.unit(unit_id).cloned() else {
            return false;
        };

        if unit.build_list.pop().is_none() {
            return false;
        }

        self.state.write_unit(unit_id, unit);
        true
    }

    /// `SetRepeatBuildState` (кнопка `Repeat`).
    pub fn set_repeat(&mut self, unit_id: UnitId, repeat: bool) -> bool {
        let Some(mut unit) = self.state.unit(unit_id).cloned() else {
            return false;
        };

        if !is_factory(unit.unit_type) {
            return false;
        }

        unit.repeat_build = repeat;
        self.state.write_unit(unit_id, unit);
        true
    }

    /// Состояние повтора.
    pub fn repeat(&self, unit_id: UnitId) -> bool {
        self.state
            .unit(unit_id)
            .map(|unit| unit.repeat_build)
            .unwrap_or(false)
    }

    /// Выбранная скорость постройки (`GetBuildRate`).
    pub fn build_rate(&self, unit_id: UnitId) -> u16 {
        self.state
            .unit(unit_id)
            .map(|unit| unit.build_rate)
            .unwrap_or(1)
    }

    /// `SetBuildRate` с клампом по `BuildMenu_GetMaxPossibleBuildRate`.
    ///
    /// До `Build` (`build_time == 0`) скорость ограничивается по времени головы
    /// очереди, иначе кнопки 2×/4× не нажимались бы до заказа.
    pub fn set_build_rate(&mut self, unit_id: UnitId, rate: u16) -> bool {
        let Some(mut unit) = self.state.unit(unit_id).cloned() else {
            return false;
        };

        let storage = if is_factory(unit.unit_type) {
            i32::from(i16::MAX)
        } else {
            i32::from(unit.storage)
        };
        let build_time = if unit.build_time > 0 {
            i32::from(unit.build_time)
        } else {
            unit.build_list
                .first()
                .map(|unit_type| self.turns_to_build(*unit_type))
                .unwrap_or(1)
                .max(1)
        };
        let max_rate = max_possible_build_rate(unit.unit_type, build_time, storage);
        let rate = i32::from(rate.max(1)).min(max_rate.max(1)) as u16;

        unit.build_rate = rate;
        self.state.write_unit(unit_id, unit);
        true
    }

    /// `BuildMenu::Build` — зафиксировать голову очереди и выдать `ORDER_BUILD`.
    ///
    /// Для мобильного строителя площадка ищется [`find_build_site`]
    /// (`UnitsManager_IssueBuildOrder`), для завода — клетка не нужна.
    pub fn commit(&mut self, unit_id: UnitId) -> Result<BuildReport, BuildError> {
        let Some(mut unit) = self.state.unit(unit_id).cloned() else {
            return Err(BuildError::InvalidTeam);
        };

        if unit.team != self.team {
            return Err(BuildError::InvalidTeam);
        }

        if !is_builder(unit.unit_type) {
            return Err(BuildError::NotABuilder);
        }

        let Some(&unit_type) = unit.build_list.first() else {
            return Err(BuildError::EmptyQueue);
        };

        if !can_build(unit.unit_type, unit_type) {
            return Err(BuildError::NotBuildable);
        }

        let site = if is_mobile_builder(unit.unit_type) {
            match find_build_site(self.state, self.team, unit_type, unit.grid) {
                Some(site) => site,
                None => return Err(BuildError::NoSite),
            }
        } else {
            unit.grid
        };

        let turns = self.turns_to_build(unit_type).max(1);
        let storage = if is_factory(unit.unit_type) {
            i32::from(i16::MAX)
        } else {
            i32::from(unit.storage)
        };
        let max_rate = max_possible_build_rate(unit.unit_type, turns, storage);
        let build_rate = i32::from(unit.build_rate.max(1)).min(max_rate.max(1)) as u16;
        let builder_type = unit.unit_type;

        // `MobileBuildMenu::Build` (`buildmenu.cpp:903-906`): при скорости 1×
        // требуется всё сырьё на руках — `storage >= rate(1) * turns`, иначе
        // сообщение `_(c529)` и приказ не отдаётся. C++ считает по
        // `Cargo_GetRawConsumptionRate(unit->GetUnitType(), 1)` и `turns_to_build`
        // из `BuildMenu_GetTurnsToBuild`.
        if is_mobile_builder(builder_type)
            && build_rate == 1
            && i32::from(unit.storage)
                < crate::victory::cargo_raw_consumption_rate(builder_type, 1) * turns
        {
            return Err(BuildError::NotEnoughMaterials);
        }

        let prior_orders = unit.orders;
        let prior_state = unit.state;

        unit.build_time = turns.min(i32::from(u8::MAX)) as u8;
        unit.build_rate = build_rate;
        unit.target_grid = site;
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::Init;
        unit.build_paused = false;
        self.state.write_unit(unit_id, unit.clone());

        // `ORDER_BUILD` с целью: клетка постройки в `order_data.move_to`.
        let mut data = self.state.order_data(unit_id);
        data.move_to = site;
        self.state.set_order_data(unit_id, data);

        // `UnitInfo::StartBuilding` (`unitinfo.cpp:2751-2794`) сразу списала бы
        // `complex->material/power/workers` и вызвала
        // `GameManager_OptimizeProduction` (`game_manager.cpp:2555-2584`):
        // при нехватке `ValidateIndustry` (`production_manager.cpp:677-711`)
        // прерывает заказ. Порт не теряет очередь: проверяет баланс до старта и
        // возвращает `BuildError`, оставляя `build_list` нетронутым.
        if is_factory(builder_type) {
            let Some(complex) = unit.complex else {
                let mut unit = self.state.unit(unit_id).cloned().unwrap_or(unit);
                unit.build_time = 0;
                unit.orders = prior_orders;
                unit.state = prior_state;
                unit.build_paused = false;
                self.state.write_unit(unit_id, unit);
                return Err(BuildError::NotEnoughMaterials);
            };

            let is_player = self
                .state
                .teams
                .get(self.team.0 as usize)
                .is_some_and(|info| info.team_type == TeamKind::Player);
            let outcome = crate::power::optimize_complex(
                self.state,
                self.team,
                complex,
                Some(unit_id),
                Some(unit_id),
                is_player,
            );

            if let Some(message) = outcome.message.as_ref() {
                self.state.push_industry_message(message.clone());
            }

            if outcome.paused.iter().any(|(id, _)| *id == unit_id) {
                let mut unit = self.state.unit(unit_id).cloned().unwrap_or(unit);
                unit.build_time = 0;
                unit.orders = prior_orders;
                unit.state = prior_state;
                unit.build_paused = false;
                self.state.write_unit(unit_id, unit);
                let shortage = outcome
                    .paused
                    .iter()
                    .find(|(id, _)| *id == unit_id)
                    .map(|(_, shortage)| *shortage)
                    .unwrap_or(crate::power::Shortage::Raw);
                return Err(crate::power::shortage_build_error(shortage));
            }
        }

        // `UnitInfo::StartBuilding` в C++ зовётся синхронно из `Build`:
        // `DeployConstructionSiteUtilities` + `PrepareConstructionSite`.
        // Ставим площадку сразу, чтобы она была видна уже на кадре заказа, а
        // машина приказов (`start_building`) повторно её не дублирует.
        if is_mobile_builder(builder_type) {
            let _ = deploy_construction_site(self.state, unit_id, unit_type, site);
        }

        Ok(BuildReport {
            unit_type,
            turns,
            build_rate,
            cost_per_turn: cost_per_turn(builder_type, i32::from(build_rate)),
            site,
        })
    }

    /// `UnitInfo::CancelBuilding` — прервать постройку и очистить очередь.
    ///
    /// Списывания за уже прошедшие ходы не возвращаются (в C++ материал также
    /// не возвращается), очередь обнуляется.
    pub fn cancel(&mut self, unit_id: UnitId) -> bool {
        let Some(mut unit) = self.state.unit(unit_id).cloned() else {
            return false;
        };

        if unit.team != self.team {
            return false;
        }

        if unit.build_list.is_empty() && unit.build_time == 0 {
            return false;
        }

        if unit.orders == UnitOrder::Build {
            unit.orders = if unit.state == UnitOrderState::BuildCancel {
                UnitOrder::HaltBuilding
            } else {
                UnitOrder::HaltBuilding2
            };
        }

        unit.state = UnitOrderState::BuildCancel;
        unit.build_list.clear();
        unit.build_time = 0;
        self.state.write_unit(unit_id, unit);
        true
    }

    /// Ход производства: оптимизация энергии → списание за ход →
    /// `SpawnNewUnits` (порядок C++).
    ///
    /// Возвращает события (спавны/остановки) в порядке C++ — для HUD-лога и
    /// тестов.
    pub fn progress(&mut self) -> Vec<ProductionEvent> {
        let (mut events, paused) = self.charge_units();
        events.extend(self.spawn_units(&paused));

        // `GameManager_ProgressBuildState` (`game_manager.cpp:8183-8204`):
        // после `SpawnNewUnits` C++ ещё раз пересчитывает комплексы и зовёт
        // `ProductionManager_OptimizeProduction`. Заводы в `UNIT_READY` больше
        // не потребляют энергию, поэтому лишние производители выключаются.
        let team = self.team;
        let complexes = self.team_complexes();
        for complex in complexes {
            let _ = crate::power::optimize_complex(self.state, team, complex, None, None, false);
        }

        events
    }

    /// Комплексы команды в порядке `TeamUnits::complexes` (по возрастанию id).
    fn team_complexes(&self) -> Vec<crate::team::ComplexId> {
        self.state
            .team_units
            .get(self.team.0 as usize)
            .map(|units| {
                units
                    .complexes
                    .iter()
                    .map(|complex| complex.complex_id())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `GameManager_ManageEconomy` → `GameManager_UpdateProductions`
    /// (`UnitInfo::UpdateProduction`): списание ставки за ход.
    ///
    /// Мобильный строитель уже заплатил сырьём со склада в
    /// [`crate::game::GameState::update_production`]; завод платит здесь
    /// **материалами своего комплекса** (`Cargo_GetRawConsumptionRate` →
    /// [`complex_raw_materials`]) — как C++ `UnitInfo::StartBuilding`/
    /// `Cargo_GetNetProduction`.
    ///
    /// Перед списанием каждый комплекс проходит
    /// [`crate::power::optimize_complex`] (C++
    /// `TeamUnits::OptimizeComplexes`): производители энергии включаются,
    /// потребители при дефиците выключаются, стройки без энергии/рабочих
    /// ставятся на паузу.
    ///
    /// Возвращает события и множество построек, которые в этом ходу **не
    /// двигаются** из-за нехватки ресурсов (C++ `ValidateIndustry` прерывает
    /// постройку; порт ставит её на паузу — см. шапку модуля).
    fn charge_units(&mut self) -> (Vec<ProductionEvent>, Vec<UnitId>) {
        let mut events = Vec::new();
        let mut paused = Vec::new();
        let team = self.team;
        let is_player = self
            .state
            .teams
            .get(team.0 as usize)
            .is_some_and(|info| info.team_type == TeamKind::Player);

        // `TeamUnits::OptimizeComplexes(team)` + `ProductionManager_OptimizeProduction`:
        // решение о паузах и режиме производителей принимается до списания.
        let mut pause_reason: Vec<(UnitId, crate::power::Shortage)> = Vec::new();
        let complexes = self.team_complexes();

        for complex in complexes {
            let outcome =
                crate::power::optimize_complex(self.state, team, complex, None, None, is_player);

            if let Some(message) = outcome.message.as_ref() {
                self.state.push_industry_message(message.clone());
            }

            for (id, shortage) in outcome.paused {
                paused.push(id);
                pause_reason.push((id, shortage));
            }
        }

        for kind in CHARGE_LIST_ORDER {
            let ids = self.state.unit_list(kind).to_vec();

            for id in ids {
                let Some(unit) = self.state.unit(id).cloned() else {
                    continue;
                };

                if unit.team != team || unit.build_list.is_empty() || unit.build_time == 0 {
                    continue;
                }

                // C++ списывает только во время `ORDER_BUILD`
                // (`UnitInfo::UpdateProduction`/`StartBuilding`).
                if unit.orders != UnitOrder::Build {
                    continue;
                }

                if matches!(
                    unit.orders,
                    UnitOrder::HaltBuilding | UnitOrder::HaltBuilding2 | UnitOrder::Disable
                ) || unit.state == UnitOrderState::UnitReady
                {
                    continue;
                }

                // `max_allowed_build_rate`: здания не ограничены складом.
                let storage = if is_factory(unit.unit_type) {
                    i32::from(i16::MAX)
                } else {
                    i32::from(unit.storage)
                };
                let max_rate =
                    max_possible_build_rate(unit.unit_type, i32::from(unit.build_time), storage);
                // У мобильного строителя `UnitInfo::UpdateProduction` уже в этом
                // ходу урезал `build_rate` по складу **и** списал сырьё именно
                // по нему; повторный пересчёт по уменьшившемуся `storage` срезал
                // бы скорость второй раз, и «золото» порта перестало бы
                // соответствовать израсходованному сырью (колонка `Cost`).
                let mobile_land = unit.is_ground() || unit.is_amphibious();
                let build_rate = if mobile_land {
                    i32::from(unit.build_rate.max(1))
                } else {
                    i32::from(unit.build_rate.max(1))
                        .min(max_rate.max(1))
                        .max(1)
                };

                // Мобильный строитель платит **сырьём со склада** (C++
                // `Cargo_GetRawConsumptionRate` → `UnitInfo::UpdateProduction`),
                // а не золотом команды; факт оплаты уже посчитан там же
                // (`build_paused`). Ход без оплаты стройку не двигает.
                if mobile_land {
                    if unit.build_paused {
                        paused.push(id);
                        events.push(ProductionEvent {
                            builder: id,
                            unit_type: unit.build_list[0],
                            spawned: None,
                            kind: ProductionEventKind::PausedNoResources,
                        });
                    }

                    continue;
                }

                let cost = cost_per_turn(unit.unit_type, build_rate);

                // Завод, который оптимизатор уже поставил на паузу (нет
                // энергии/топлива/рабочих/материалов), в этом ходу не платит и
                // не двигается (C++ `ValidateIndustry` → `BUILD_ABORT`; порт —
                // пауза с сохранением очереди).
                if let Some((_, shortage)) = pause_reason.iter().find(|(unit, _)| *unit == id) {
                    events.push(ProductionEvent {
                        builder: id,
                        unit_type: unit.build_list[0],
                        spawned: None,
                        kind: pause_event_kind(*shortage),
                    });
                    continue;
                }

                // C++ `UnitInfo::StartBuilding` (`unitinfo.cpp:2757`):
                // `complex->material -= Cargo_GetRawConsumptionRate(...)`;
                // `Cargo_GetNetProduction` (`cargo.cpp:88-95`) вычитает ставку
                // каждый ход, пока `ORDER_BUILD` и состояние не `UNIT_READY`.
                // Материалы — **комплекса завода** ([`complex_raw_materials`]),
                // а не всей команды: завод без подключённого хранилища
                // (`ADUMP`/шахта) стоит, как в оракуле.
                let Some(complex) = unit.complex else {
                    paused.push(id);
                    events.push(ProductionEvent {
                        builder: id,
                        unit_type: unit.build_list[0],
                        spawned: None,
                        kind: ProductionEventKind::PausedNoMaterials,
                    });
                    continue;
                };

                if !spend_complex_raw_materials(self.state, team, complex, cost) {
                    paused.push(id);
                    events.push(ProductionEvent {
                        builder: id,
                        unit_type: unit.build_list[0],
                        spawned: None,
                        kind: ProductionEventKind::PausedNoMaterials,
                    });
                    continue;
                }
            }
        }

        (events, paused)
    }

    /// `GameManager_SpawnNewUnits` для всех четырёх списков в порядке C++.
    ///
    /// `paused` — постройки, за которые в этом ходу не заплачено: они не
    /// двигаются и не выпускают юнитов.
    fn spawn_units(&mut self, paused: &[UnitId]) -> Vec<ProductionEvent> {
        let mut events = Vec::new();
        let team = self.team;

        for kind in PROGRESS_LIST_ORDER {
            let ids = self.state.unit_list(kind).to_vec();

            for id in ids {
                if paused.contains(&id) {
                    continue;
                }

                let Some(unit) = self.state.unit(id).cloned() else {
                    continue;
                };

                if unit.team != team || unit.build_time == 0 {
                    continue;
                }

                // `GameManager_SpawnNewUnits` (`game_manager.cpp:8078`) крутит
                // `build_time` и у бульдозера (`ORDER_CLEAR`, очередь пуста):
                // по нулю вызывается `UnitInfo::SpawnNewUnit`, который сносит
                // обломок и забирает его груз.
                if unit.build_list.is_empty() && unit.unit_type != UnitType::Bulldozer {
                    continue;
                }

                if matches!(
                    unit.orders,
                    UnitOrder::HaltBuilding | UnitOrder::HaltBuilding2
                ) {
                    continue;
                }

                if i32::from(unit.build_time) <= i32::from(unit.build_rate.max(1)) {
                    events.push(self.spawn_new_unit(id));
                } else {
                    let mut unit = unit;
                    unit.build_time = unit.build_time.saturating_sub(unit.build_rate.max(1) as u8);
                    self.state.write_unit(id, unit);
                }
            }
        }

        events
    }

    /// `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3441-3487`), ветка `BULLDOZR`:
    /// обломок расчищен.
    ///
    /// C++ сносит `SMLRUBLE`/`LRGRUBLE` в клетке бульдозера (у большого — ещё
    /// `+1` по обеим осям), забирает их `storage` (груз обломка) себе, режет по
    /// `ATTRIB_STORAGE`, снимает утилиты клетки и возвращается в
    /// `ORDER_AWAIT`/`EXECUTING_ORDER`. Для обломка здания дополнительно
    /// запускается `AWAIT_TAPE_POSITIONING`/`DEINIT` — в порте ленты расчистки
    /// нет (`ProcessOrderClear` без `StartClearing`), поэтому шаг пропущен.
    fn finish_bulldozer_clearing(&mut self, id: UnitId) -> ProductionEvent {
        let Some(mut builder) = self.state.unit(id).cloned() else {
            return ProductionEvent {
                builder: id,
                unit_type: UnitType::Bulldozer,
                spawned: None,
                kind: ProductionEventKind::NoSpawnCell,
            };
        };

        let parent = builder
            .parent_unit
            .and_then(|parent_id| self.state.unit(parent_id).cloned());
        let limit = if parent
            .as_ref()
            .is_some_and(|parent| parent.unit_type == UnitType::LargeRubble)
        {
            1
        } else {
            0
        };

        let mut storage = i32::from(builder.storage);

        for dx in 0..=limit {
            for dy in 0..=limit {
                let cell = Point::new(builder.grid.x + dx, builder.grid.y + dy);

                for rubble_id in self.state.units_at(cell).to_vec() {
                    let Some(rubble) = self.state.unit(rubble_id) else {
                        continue;
                    };

                    if matches!(
                        rubble.unit_type,
                        UnitType::SmallRubble | UnitType::LargeRubble
                    ) {
                        storage += i32::from(rubble.storage);
                        self.state.destroy_unit(rubble_id);
                    }
                }
            }
        }

        crate::access::destroy_utilities(self.state, builder.grid, false, false, false, false);

        let capacity = builder
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_STORAGE);
        builder.storage = storage.min(capacity).clamp(0, i32::from(i16::MAX)) as i16;
        // `DrawSpriteFrame(image_index - 8)`.
        builder.image_index = builder.image_index.saturating_sub(8);
        builder.orders = UnitOrder::Await;
        builder.state = UnitOrderState::ExecutingOrder;
        builder.build_time = 0;
        builder.build_paused = false;
        builder.parent_unit = None;
        builder.target_grid = builder.grid;
        self.state.write_unit(id, builder);

        ProductionEvent {
            builder: id,
            unit_type: UnitType::Bulldozer,
            spawned: None,
            kind: ProductionEventKind::RubbleCleared,
        }
    }

    /// `UnitInfo::SpawnNewUnit` — выпуск юнита/достройка здания.
    fn spawn_new_unit(&mut self, id: UnitId) -> ProductionEvent {
        let team = self.team;
        let Some(builder) = self.state.unit(id).cloned() else {
            return ProductionEvent {
                builder: id,
                unit_type: UnitType::Tank,
                spawned: None,
                kind: ProductionEventKind::NoSpawnCell,
            };
        };

        // `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3441-3487`), ветка BULLDOZR:
        // расчистка обломка завершена — обломки сносятся, их груз переходит
        // бульдозеру.
        if builder.unit_type == UnitType::Bulldozer {
            return self.finish_bulldozer_clearing(id);
        }

        let Some(&unit_type) = builder.build_list.first() else {
            return ProductionEvent {
                builder: id,
                unit_type: builder.unit_type,
                spawned: None,
                kind: ProductionEventKind::NoSpawnCell,
            };
        };

        let factory = is_factory(builder.unit_type);
        // `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3489-3494`): стационарный
        // завод ставит готовый юнит **на свою клетку** —
        // `UnitsManager_DeployUnit(GetConstructedUnitType(), team, GetComplex(),
        // position_x = this->grid_x, position_y = this->grid_y, …)`. Свободная
        // клетка рядом не ищется: юнит «внутри» завода до приказа игрока.
        let cell = if factory {
            builder.grid
        } else {
            builder.target_grid
        };

        let Some(spawned) = self.state.spawn_unit(unit_type, team, cell, 0) else {
            return ProductionEvent {
                builder: id,
                unit_type,
                spawned: None,
                kind: ProductionEventKind::NoSpawnCell,
            };
        };

        // Новый юнит принадлежит комплексу строителя (`SpawnNewUnit`:
        // `UnitsManager_DeployUnit(..., GetComplex(), ...)`).
        if builder.complex.is_some() {
            if let Some(mut unit) = self.state.unit(spawned).cloned() {
                unit.complex = builder.complex;
                self.state.write_unit(spawned, unit);
            }
        }

        if let Some(info) = self.state.teams.get_mut(team.0 as usize) {
            info.stats_units_built = info.stats_units_built.saturating_add(1);
        }

        // Учёт очереди: `build_list.Remove(0)`, `repeat_build` возвращает тип.
        let mut builder = builder;
        builder.build_list.remove(0);

        if factory && builder.repeat_build {
            builder.build_list.push(unit_type);
        }

        let kind = if factory {
            ProductionEventKind::Spawned
        } else {
            ProductionEventKind::BuildingDone
        };

        if factory {
            // `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3489-3520`), ветка
            // `flags & STATIONARY` — построенный юнит остаётся **в заводе**:
            // * `factory_unit->SetParent(this)` — родитель юнита — завод;
            // * `factory_unit->SetOrder(ORDER_IDLE)`;
            //   `factory_unit->SetOrderState(ORDER_STATE_STORE)` — юнит «в
            //   трюме» (`GetTeamUnit` такие не выбирает, `Access_IsAccessible`
            //   не считает, рендер `IsRelevant` не рисует);
            // * `factory_unit->scaler_adjust = 4` — сжат (1/32 размера, за
            //   пределами отрисовки), пока стоит в заводе;
            // * `SetParent(factory_unit.Get())` — у **завода** родитель теперь
            //   готовый юнит: именно это читает `UnitsManager_ActivateUnit`
            //   (`units_manager.cpp:3827`: `client = unit->GetParent()`), когда
            //   игрок выводит юнита приказом;
            // * `storage = 1` — счётчик готовых юнитов в заводе;
            // * `orders = ORDER_BUILD`, `state = ORDER_STATE_UNIT_READY` —
            //   завод ждёт вывода (следующий заказ стартует только в
            //   `UnitsManager_ActivateUnit`, `units_manager.cpp:3843-3848`, а не
            //   здесь: автостарт очереди был расхождением порта).
            if let Some(mut unit) = self.state.unit(spawned).cloned() {
                unit.parent_unit = Some(id);
                // C++: `SetOrder(ORDER_IDLE)` + `SetOrderState(STORE)` — приказ
                // ставится напрямую, `prior` остаётся от `DeployUnit`
                // (`ORDER_AWAIT`/`EXECUTING_ORDER`).
                unit.orders = UnitOrder::Idle;
                unit.state = UnitOrderState::Store;
                unit.target_grid = cell;
                unit.scaler_adjust = 4;
                self.state.write_unit(spawned, unit);
            }

            builder.parent_unit = Some(spawned);
            builder.storage = 1;
            builder.orders = UnitOrder::Build;
            builder.state = UnitOrderState::UnitReady;
            builder.build_time = 0;
            builder.build_paused = false;
        } else {
            // `UnitInfo::SpawnNewUnit` (мобильный строитель) — порядок C++:
            //  1. здание встаёт на клетку ленты, но остаётся
            //     `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY` и **не рисуется**
            //     (`UnitInfoGroup::IsRelevant`), пока строитель на площадке;
            //  2. строитель получает `ORDER_BUILD`/`ORDER_STATE_UNIT_READY`
            //     (`this->SetParent(&*utility_unit)` — взаимный родитель) и
            //     продолжает стоять (`ProcessOrderBuild`/`UNIT_READY` только
            //     анимирует) — **никакого автовыхода**;
            //  3. уход заказывает игрок кликом (`ORDER_ACTIVATE` →
            //     `PositionInTape` DEINIT/LEAVE → `ActivateEngineer` →
            //     `ORDER_MOVE`, см. `hud::build_builder_exit_order`) или ИИ
            //     (`TaskActivate` → `unit_order::issue_builder_exit`);
            //  4. `UnitsManager_BuildingReady` снимает ленту и конусы, плита
            //     (`*SLAB`) остаётся фундаментом, здание раскрывается
            //     (`ScaleUnit(EXPAND)`) — только после ухода строителя.
            if let Some(mut unit) = self.state.unit(spawned).cloned() {
                unit.parent_unit = Some(id);
                // `UnitsManager_SetNewOrderInt(&*utility_unit, ORDER_IDLE,
                // ORDER_STATE_BUILDING_READY)` (`unitinfo.cpp:3581`): в отличие
                // от `SetOrder`, `SetNewOrderInt` сохраняет **prior**
                // (`units_manager.cpp:1970-1971`). Это важно для юнитов,
                // которые `UnitsManager_DeployUnit` ставит в особый приказ:
                // `RADAR`, турели и мины рождаются в `ORDER_SENTRY`
                // (`units_manager.cpp:2412-2421`), и `RestoreOrders`
                // (`unitinfo.cpp:3783`) после достройки возвращает именно
                // `ORDER_SENTRY` — иначе ANIMATED-радар (`ProcessOrderSentry`)
                // замирал бы в `ORDER_AWAIT` и не листал 16 кадров.
                crate::unit_order::set_new_order_int(
                    &mut unit,
                    UnitOrder::Idle,
                    UnitOrderState::BuildingReady,
                );
                self.state.write_unit(spawned, unit);
            }

            // `this->SetParent(&*utility_unit)`: строитель «внутри» здания до
            // ухода с площадки; `if (!path) build_list.Clear()` — порте строитель
            // к площадке не идёт (`PositionInTape` двигает только спрайт),
            // пути нет, поэтому очередь очищается.
            builder.parent_unit = Some(spawned);
            builder.orders = UnitOrder::Build;
            builder.state = UnitOrderState::UnitReady;
            builder.build_time = 0;
            builder.build_list.clear();
        }

        self.state.write_unit(id, builder);

        ProductionEvent {
            builder: id,
            unit_type,
            spawned: Some(spawned),
            kind,
        }
    }
}

// ---------------------------------------------------------------------------
// Свободные функции для GameState/ИИ
// ---------------------------------------------------------------------------

/// `GameManager_ProgressBuildState(team)` — ход производства команды.
///
/// Порядок 1:1 C++: `ManageEconomy` (списание) → `SpawnNewUnits` по спискам
/// `GroundCover → MobileLandSea → Stationary → MobileAir`.
pub fn progress_build_state(state: &mut GameState, team: TeamId) -> Vec<ProductionEvent> {
    ProductionManager::new(state, team).progress()
}

/// Хватает ли материалов на текущий ход постройки (проверка UI/ИИ).
///
/// У мобильного строителя платёж — сырьё на своём складе
/// (`Cargo_GetRawConsumptionRate` → `UnitInfo::UpdateProduction`), у
/// заводов — материалы **своего комплекса** ([`complex_raw_materials`],
/// C++ `complex->material`).
pub fn can_afford_turn(state: &GameState, team: TeamId, unit_id: UnitId) -> bool {
    let Some(unit) = state.unit(unit_id) else {
        return false;
    };

    let cost = cost_per_turn(unit.unit_type, i32::from(unit.build_rate.max(1)));

    if unit.is_ground() || unit.is_amphibious() {
        return i32::from(unit.storage) >= cost.max(0);
    }

    unit.complex
        .is_some_and(|complex| complex_raw_materials(state, team, complex) >= cost.max(0))
}
