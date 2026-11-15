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
//! * `cargo.cpp` — `Cargo_GetRawConsumptionRate` (ставка расхода).
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
//!    строитель достраивает здание на площадке.
//!
//! # Экономика (осознанное решение)
//!
//! C++ списывает **сырьё комплекса** (`Cargo_GetRawConsumptionRate`), а в
//! Rust-порте сырьё комплексов ещё не наполняется (P3: `Access_UpdateResourcesTotal`
//! и перенос груза). Чтобы производство работало уже сейчас и совпадало по
//! формулам, ставка C++ списывается с **золота команды**:
//!
//! * за ход: `cargo_raw_consumption_rate(builder, build_rate)`;
//! * итог за постройку при `rate = 1`: `cargo_raw_consumption_rate(builder, 1) * turns` —
//!   ровно `UnitInfo::GetNormalRateBuildCost` / `CargoMenu::BuyUnit`.
//!
//! Нехватка золота **ставит постройку на паузу** (не отменяет): заказ остаётся
//! в очереди, в этом ходу не списывается и не двигается —
//! [`ProductionEventKind::PausedNoGold`]. C++ в этой ситуации прерывает
//! постройку (`ValidateIndustry`), но там списываются материалы комплекса,
//! которые в порте пока не наполняются (P3); пауза сохраняет заказ игрока и не
//! ломает партии с нулевым стартовым бюджетом.
//!
//! # Что упрощено (честно)
//!
//! * материалы/энергия/рабочие комплекса и `ProductionManager_OptimizeProduction`
//!   (перераспределение шахт) остаются TODO(P3);
//! * завод выпускает юнит сразу в свободную соседнюю клетку, а не хранит его
//!   внутри и не «выезжает» по `AWAIT_TAPE_POSITIONING`;
//! * `quick_build` (настройка обучающих миссий) не моделируется;
//! * мобильный строитель ставит здание на `target_grid` без анимации
//!   переезда/`StartBuilding`-телепорта (только площадка по C++-правилам).

use crate::enums::unit_flags;
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitListKind};
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
            UnitType::CommTower,
            UnitType::Depot,
            UnitType::Hangar,
            UnitType::Dock,
            UnitType::Habitat,
            UnitType::ResearchCenter,
            UnitType::Greenhouse,
            UnitType::TrainingHall,
            UnitType::Barracks,
        ],
    ),
    (
        UnitType::Engineer,
        &[
            UnitType::ADump,
            UnitType::FDump,
            UnitType::GoldSmelter,
            UnitType::PowerGenerator,
            UnitType::Connector,
            UnitType::Radar,
            UnitType::GunTurret,
            UnitType::AntiAir,
            UnitType::ArtilleryTurret,
            UnitType::AntiMissile,
            UnitType::LandingPad,
            UnitType::Bridge,
            UnitType::WaterPlatform,
            UnitType::Block,
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
            UnitType::PersonnelCarrier,
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
            UnitType::FastBoat,
            UnitType::Corvette,
            UnitType::Battleship,
            UnitType::Submarine,
            UnitType::SeaTransport,
            UnitType::MissileBoat,
            UnitType::SeaMineLayer,
            UnitType::CargoShip,
        ],
    ),
    (
        UnitType::TrainingHall,
        &[UnitType::Commando, UnitType::Infantry],
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

/// Радиус поиска свободной клетки для выпуска юнита заводом.
pub const SPAWN_SEARCH_RADIUS: i16 = 4;
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
        .map(|units| units.current_unit_values(unit_type as u16).get_attribute(crate::unit_values::ATTRIB_TURNS))
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

/// Стоимость одного хода постройки в золоте — `Cargo_GetRawConsumptionRate`.
///
/// C++ списывает эту ставку с сырья комплекса; порт — с золота команды (см.
/// шапку модуля). Для `rate = 1` суммарная стоимость равна
/// `Cargo_GetRawConsumptionRate(builder, 1) * turns` — C++-стоимость постройки.
pub fn cost_per_turn(builder: UnitType, build_rate: i32) -> i32 {
    cargo_raw_consumption_rate(builder, build_rate)
}

/// Полная стоимость постройки при скорости 1× (в золоте) — C++-формула
/// `GetNormalRateBuildCost` для юнита, который строит `builder`.
pub fn total_cost(builder: UnitType, turns: i32) -> i32 {
    cost_per_turn(builder, 1) * turns.max(0)
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

    let mut build_speed_multiplier = if build_time > RATE_2X { RATE_4X } else { RATE_2X };

    if crate::access::unit_type_flags(unit_type) & unit_flags::MOBILE_LAND_UNIT != 0 {
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
    /// Постройка приостановлена: золота команды не хватает на ход
    /// (`ValidateIndustry` в C++ прерывает, порт ставит на паузу — см. шапку).
    PausedNoGold,
    /// Спавн отложен: рядом нет свободной клетки (юнит остаётся в очереди).
    NoSpawnCell,
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

// ---------------------------------------------------------------------------
// Площадка и клетка выпуска
// ---------------------------------------------------------------------------

/// Площадка под здание/юнит: все клетки внутри карты, свободны и проходимы.
fn site_is_free(state: &GameState, team: TeamId, unit_type: UnitType, site: Point, size: i16) -> bool {
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

/// Свободная клетка рядом с заводом под выпущенный юнит.
fn find_spawn_cell(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    origin: Point,
) -> Option<Point> {
    let size = footprint(unit_type);
    let mut candidates: Vec<(i32, i16, i16)> = Vec::new();

    for dy in -SPAWN_SEARCH_RADIUS..=SPAWN_SEARCH_RADIUS {
        for dx in -SPAWN_SEARCH_RADIUS..=SPAWN_SEARCH_RADIUS {
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
        let cell = Point::new(x, y);

        if site_is_free(state, team, unit_type, cell, size) {
            return Some(cell);
        }
    }

    None
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

        unit.build_time = turns.min(i32::from(u8::MAX)) as u8;
        unit.build_rate = build_rate;
        unit.target_grid = site;
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::Init;
        self.state.write_unit(unit_id, unit);

        // `ORDER_BUILD` с целью: клетка постройки в `order_data.move_to`.
        let mut data = self.state.order_data(unit_id);
        data.move_to = site;
        self.state.set_order_data(unit_id, data);

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

    /// Ход производства: списание за ход → `SpawnNewUnits` (порядок C++).
    ///
    /// Возвращает события (спавны/остановки) в порядке C++ — для HUD-лога и
    /// тестов.
    pub fn progress(&mut self) -> Vec<ProductionEvent> {
        let (mut events, paused) = self.charge_units();
        events.extend(self.spawn_units(&paused));
        events
    }

    /// `GameManager_ManageEconomy` → `GameManager_UpdateProductions`
    /// (`UnitInfo::UpdateProduction`): списание ставки за ход.
    ///
    /// Возвращает события и множество построек, которые в этом ходу **не
    /// двигаются** из-за нехватки золота (C++ `ValidateIndustry` прерывает
    /// постройку; порт ставит её на паузу — см. шапку модуля).
    fn charge_units(&mut self) -> (Vec<ProductionEvent>, Vec<UnitId>) {
        let mut events = Vec::new();
        let mut paused = Vec::new();
        let team = self.team;

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
                let build_rate = i32::from(unit.build_rate.max(1)).min(max_rate.max(1)).max(1);
                let rate = build_rate as u16;
                let cost = cost_per_turn(unit.unit_type, build_rate);

                if cost > 0 {
                    let slot = team.0 as usize;
                    let gold = self.state.team_units[slot].gold();

                    if gold < cost as u32 {
                        paused.push(id);
                        events.push(ProductionEvent {
                            builder: id,
                            unit_type: unit.build_list[0],
                            spawned: None,
                            kind: ProductionEventKind::PausedNoGold,
                        });
                        continue;
                    }

                    self.state.team_units[slot].set_gold(gold - cost as u32);
                }

                if rate != unit.build_rate {
                    let mut unit = unit;
                    unit.build_rate = rate;
                    self.state.write_unit(id, unit);
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

                if unit.team != team || unit.build_time == 0 || unit.build_list.is_empty() {
                    continue;
                }

                if matches!(unit.orders, UnitOrder::HaltBuilding | UnitOrder::HaltBuilding2) {
                    continue;
                }

                if i32::from(unit.build_time) <= i32::from(unit.build_rate.max(1)) {
                    events.push(self.spawn_new_unit(id));
                } else {
                    let mut unit = unit;
                    unit.build_time = unit
                        .build_time
                        .saturating_sub(unit.build_rate.max(1) as u8);
                    self.state.write_unit(id, unit);
                }
            }
        }

        events
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

        let Some(&unit_type) = builder.build_list.first() else {
            return ProductionEvent {
                builder: id,
                unit_type: builder.unit_type,
                spawned: None,
                kind: ProductionEventKind::NoSpawnCell,
            };
        };

        let factory = is_factory(builder.unit_type);
        let cell = if factory {
            match find_spawn_cell(self.state, team, unit_type, builder.grid) {
                Some(cell) => cell,
                None => {
                    return ProductionEvent {
                        builder: id,
                        unit_type,
                        spawned: None,
                        kind: ProductionEventKind::NoSpawnCell,
                    }
                }
            }
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
            // `SpawnNewUnit` (стационарный): `orders = ORDER_BUILD`,
            // `state = ORDER_STATE_UNIT_READY`.
            builder.orders = UnitOrder::Build;
            builder.state = UnitOrderState::UnitReady;
            builder.build_time = 0;

            // `UnitsManager_BuildNext`: очередь продолжается автоматически.
            if let Some(&next) = builder.build_list.first() {
                let turns = crate::production::turns_to_build(self.state, team, next).max(1);
                builder.build_time = turns.min(i32::from(u8::MAX)) as u8;
                builder.state = UnitOrderState::Init;
            }
        } else {
            // Мобильный строитель достроил здание и ждёт приказов
            // (`SpawnNewUnit`: `orders = ORDER_AWAIT`; `!path` очищает список).
            builder.orders = UnitOrder::Await;
            builder.state = UnitOrderState::ExecutingOrder;
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

/// Хватает ли золота команды на текущий ход постройки (проверка UI/ИИ).
pub fn can_afford_turn(state: &GameState, team: TeamId, unit_id: UnitId) -> bool {
    let Some(unit) = state.unit(unit_id) else {
        return false;
    };

    let cost = cost_per_turn(unit.unit_type, i32::from(unit.build_rate.max(1)));

    state
        .team_units
        .get(team.0 as usize)
        .map(|units| units.gold() >= cost.max(0) as u32)
        .unwrap_or(false)
}
