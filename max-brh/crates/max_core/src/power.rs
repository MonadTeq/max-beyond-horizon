//! Энергетика комплекса — порт power-части `production_manager.cpp`.
//!
//! # Оракул
//!
//! * `production_manager.cpp:84-160` — `ProductionManager` собирает по членам
//!   комплекса `inventory`/`net_production`/`total`, считает активные
//!   `POWERSTN`/`POWGEN` (`power_station_active`/`power_generator_active`);
//! * `production_manager.cpp:164-197` — `UpdateUnitPowerConsumption`:
//!   лишние включённые генераторы выключаются, недостающие — включаются
//!   (`UnitsManager_SetNewOrder(ORDER_POWER_ON/OFF, ORDER_STATE_INIT)`);
//! * `production_manager.cpp:481-549` — `OptimizePowerProduction`: сколько
//!   станций/генераторов должно работать при текущем дефиците энергии и
//!   запасе топлива;
//! * `production_manager.cpp:551-562` — `OptimizePowerConsumption`: применяет
//!   план к приказам производителей;
//! * `production_manager.cpp:642-664` — `CheckPowerNeed`: хватает ли энергии и
//!   топлива на дополнительное потребление;
//! * `production_manager.cpp:677-727` — `ValidateIndustry`/`OptimizeIndustry`:
//!   при нехватке ресурсов стройка завода останавливается
//!   (`ORDER_STATE_BUILD_ABORT`, в порте — пауза без потери очереди);
//! * `production_manager.cpp:729-790` — `OptimizeMiningIndustry`,
//!   `OptimizeAuxilaryIndustry`: порядок отключения потребителей
//!   (`RESEARCH` → `GREENHSE` → `COMMTWR`, затем стройки, затем `MININGST`);
//! * `production_manager.cpp:366-380` — `UpdateUnitLifeConsumption`: при
//!   нехватке рабочих включается `HABITAT` (`life = -3`, то есть +3 рабочих);
//! * `game_manager.cpp:2058-2076` — `GameManager_ManageEconomy` →
//!   `TeamUnits::OptimizeComplexes` (`teamunits.cpp:170-175`) вызывает
//!   оптимизацию каждого комплекса каждый ход;
//! * `game_manager.cpp:8183-8204` — `GameManager_ProgressBuildState`: после
//!   спавна юнитов ещё раз пересчитывает комплексы и оптимизирует их;
//! * `game_manager.cpp:2555-2584` — `GameManager_OptimizeProduction`: при
//!   отрицательном `material/fuel/gold/power/workers` запускает
//!   `ProductionManager_OptimizeProduction`;
//! * `cargo.cpp:70-138` — `Cargo_GetNetProduction` (расход завода:
//!   raw+power+life) и `Cargo_ApplyUnitConsumption`;
//! * `cargo.cpp:183-271` — ставки топлива/энергии/рабочих/золота.
//!
//! # Модель
//!
//! Энергия — не склад, а **баланс** комплекса: `POWERSTN` даёт +6, `POWGEN`
//! +1, а включённые `RESEARCH`/`GREENHSE`/`COMMTWR`/`MININGST` и строящийся
//! завод потребляют по 1 (`cargo_power_consumption_rate`). Рабочие — тоже
//! баланс: `HABITAT` даёт +3, `TRAINHAL`/`GREENHSE`/`RESEARCH` потребляют по 1.
//!
//! Порт соблюдает контракт `max-brh-docs/architecture/client/game-mechanics.md` §3/§5:
//! * заказ завода **не стартует**, если комплекс не может оплатить первый ход
//!   (raw, power, life) — [`crate::production::BuildError`];
//! * уже идущая стройка при нехватке **паузится** (`build_paused`, очередь
//!   сохраняется), а не отменяется как в C++ (`ValidateIndustry` →
//!   `BUILD_ABORT`); это осознанное требование QA (ресурс не уходит в минус);
//! * при дефиците энергии выключаются здания в C++-порядке:
//!   `RESEARCH` → `GREENHSE` → `COMMTWR` → стройки → `MININGST`;
//! * производители энергии включаются/выключаются автоматически по балансу и
//!   запасу топлива (C++ `OptimizePowerProduction`/`OptimizePowerConsumption`);
//! * выключенные из-за дефицита **потребители** C++ обратно сам не включает —
//!   это делает игрок (`UnitsManager_Popup_OnClick_PowerOn`) или ИИ; порт
//!   повторяет это правило, а генераторы восстанавливаются автоматически;
//! * сообщение о нехватке — `ComposeIndustryMessage`/`DrawResourceMessage`
//!   (`production_manager.cpp:308-330/465-479/792-804`): «Adjustments made:»,
//!   «More power needed, X turned off.», «Cannot turn X on, power needed.».
//!
//! Отличие от C++ (документировано в `visual-1to1-status.md` §42): точная
//! арифметика `OptimizePowerProduction` (формулы с `count`) заменена
//! эквивалентным по наблюдаемому балансу детерминированным выбором
//! «станции → генераторы»; перераспределение добычи ради топлива
//! (`OptimizeCargoProduction`) в этой задаче не портировано — при нехватке
//! топлива производитель просто не включается.

use crate::alloc::{cargo_get_inventory, update_resources_total};
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::production::is_factory;
use crate::team::{ComplexId, TeamId};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::victory::{
    cargo_fuel_consumption_rate, cargo_get_net_production, cargo_power_consumption_rate,
};

/// Баланс ресурсов комплекса — C++ `Cargo` в `ProductionManager` (`total`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnergyTotals {
    /// Материалы (запас + добыча − расход строящихся заводов).
    pub raw: i32,
    /// Топливо (запас + добыча − топливо производителей энергии).
    pub fuel: i32,
    /// Золото.
    pub gold: i32,
    /// Энергия (производство − потребление).
    pub power: i32,
    /// Рабочие (`life`).
    pub life: i32,
}

impl EnergyTotals {
    /// Есть ли хоть один отрицательный баланс (`GameManager_OptimizeProduction`).
    #[inline]
    pub const fn has_deficit(&self) -> bool {
        self.raw < 0 || self.fuel < 0 || self.gold < 0 || self.power < 0 || self.life < 0
    }

    /// Первый дефицит в порядке `ValidateIndustry`/`Cargo_ApplyUnitConsumption`:
    /// материалы → энергия → рабочие → топливо → золото.
    #[inline]
    pub const fn first_shortage(&self) -> Option<Shortage> {
        if self.raw < 0 {
            Some(Shortage::Raw)
        } else if self.power < 0 {
            Some(Shortage::Power)
        } else if self.life < 0 {
            Some(Shortage::Workers)
        } else if self.fuel < 0 {
            Some(Shortage::Fuel)
        } else if self.gold < 0 {
            Some(Shortage::Gold)
        } else {
            None
        }
    }
}

/// Какого ресурса не хватило (`ComposeIndustryMessage`, `material`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shortage {
    /// `_(9c91)` / `_(2acf)` — raw material.
    Raw,
    /// `_(2162)` / `_(19b2)` — fuel.
    Fuel,
    /// `_(24d7)` / `_(f260)` — gold.
    Gold,
    /// `_(f814)` / `_(7a37)` — power.
    Power,
    /// `_(73f7)` / `_(57f9)` — workers.
    Workers,
}

impl Shortage {
    /// Строка ресурса в сообщении C++ (`_(...)` из `language.json`).
    pub const fn material(self) -> &'static str {
        match self {
            Self::Raw => "raw material",
            Self::Fuel => "fuel",
            Self::Gold => "gold",
            Self::Power => "power",
            Self::Workers => "workers",
        }
    }
}

/// Действие оптимизатора — для сообщения `ComposeIndustryMessage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Adjustment {
    /// Стройка завода поставлена на паузу (`ValidateIndustry`).
    Paused { unit: UnitId, shortage: Shortage },
    /// Потребитель выключен (`OptimizeAuxilaryIndustry`/`OptimizeMiningIndustry`).
    PowerOff { unit: UnitId, shortage: Shortage },
    /// Производитель включён (`UpdateUnitPowerConsumption` /
    /// `UpdateUnitLifeConsumption`); `shortage` — что именно понадобилось
    /// (`power` у генератора, `workers` у `HABITAT`).
    PowerOn { unit: UnitId, shortage: Shortage },
    /// `PowerOn` не удался из-за топлива (`production_manager.cpp:332-352`):
    /// строка «Cannot turn X on, fuel needed.» без смены приказа.
    CannotTurnOn { unit: UnitId, shortage: Shortage },
}

/// Итог оптимизации комплекса.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndustryOutcome {
    /// Баланс после всех действий.
    pub totals: EnergyTotals,
    /// Стройки, поставленные на паузу, и причина.
    pub paused: Vec<(UnitId, Shortage)>,
    /// Выключенные потребители.
    pub powered_off: Vec<UnitId>,
    /// Включённые производители энергии.
    pub powered_on: Vec<UnitId>,
    /// Сообщение для верхней строки карты (`MESSAGE_BOX_NOTICE`), если
    /// `show_messages` и были действия.
    pub message: Option<String>,
    /// После всех действий остался отрицательный баланс (C++ `return` из
    /// `ProductionManager_OptimizeProduction`).
    pub unresolved: bool,
}

impl IndustryOutcome {
    /// Ошибка `BuildMenu::Build` для отклонённой стройки.
    pub fn build_error(&self) -> crate::production::BuildError {
        self.paused
            .first()
            .map(|(_, shortage)| shortage_build_error(*shortage))
            .unwrap_or(crate::production::BuildError::NotEnoughMaterials)
    }
}

/// `Shortage` → `BuildError` (для отказа в старте заказа).
pub fn shortage_build_error(shortage: Shortage) -> crate::production::BuildError {
    use crate::production::BuildError;

    match shortage {
        Shortage::Power => BuildError::NotEnoughPower,
        Shortage::Fuel => BuildError::NotEnoughFuel,
        Shortage::Workers => BuildError::NotEnoughWorkers,
        Shortage::Gold => BuildError::NotEnoughGold,
        Shortage::Raw => BuildError::NotEnoughMaterials,
    }
}

// ---------------------------------------------------------------------------
// Таблицы
// ---------------------------------------------------------------------------

/// `Cargo_GetPowerConsumptionRate(...) < 0` — производитель энергии.
#[inline]
pub fn is_power_producer(unit_type: UnitType) -> bool {
    matches!(unit_type, UnitType::PowerStation | UnitType::PowerGenerator)
}

/// Здание, у которого есть C++-пункт `power on/off` и которое оптимизатор
/// может выключить при дефиците энергии (`OptimizeAuxilaryIndustry` +
/// `OptimizeMiningIndustry`).
#[inline]
pub fn is_switchable_consumer(unit_type: UnitType) -> bool {
    matches!(
        unit_type,
        UnitType::ResearchCentre
            | UnitType::EcoSphere
            | UnitType::GoldRefinery
            | UnitType::MiningStation
    )
}

/// Порядок принудительного отключения потребителей
/// (`ProductionManager_OptimizeProduction`, `production_manager.cpp:877-881`):
/// `RESEARCH` → `GREENHSE` → `COMMTWR`, затем стройки (`OptimizeIndustry`) и
/// только потом `MININGST` (`OptimizeMiningIndustry`).
pub const CONSUMER_OFF_ORDER: [UnitType; 3] = [
    UnitType::ResearchCentre,
    UnitType::EcoSphere,
    UnitType::GoldRefinery,
];

/// Положительная мощность производителя (`-Cargo_GetPowerConsumptionRate`).
#[inline]
fn producer_power(unit_type: UnitType) -> i32 {
    -cargo_power_consumption_rate(unit_type)
}

/// Расход топлива производителя (`Cargo_GetFuelConsumptionRate`).
#[inline]
fn producer_fuel(unit_type: UnitType) -> i32 {
    cargo_fuel_consumption_rate(unit_type)
}

/// Имя юнита для сообщения — `ResourceManager_GetUnit(type).GetSingularName()`.
///
/// В C++ имена лежат в `language.json`; в `max_core` таблицы имён нет
/// (`max_app::buildmenu::unit_name`), поэтому здесь перечислены только типы,
/// которые встречаются в энергетических сообщениях.
pub fn industry_unit_name(unit_type: UnitType) -> &'static str {
    match unit_type {
        UnitType::ResearchCentre => "Research Center",
        UnitType::EcoSphere => "Greenhouse",
        UnitType::GoldRefinery => "Comm Tower",
        UnitType::MiningStation => "Mining Station",
        UnitType::PowerStation => "Power Station",
        UnitType::PowerGenerator => "Power Generator",
        UnitType::Habitat => "Habitat",
        UnitType::TrainingHall => "Training Hall",
        UnitType::LightVehiclePlant => "Light Vehicle Plant",
        UnitType::HeavyVehiclePlant => "Heavy Vehicle Plant",
        UnitType::AirUnitsPlant => "Air Units Plant",
        UnitType::Shipyard => "Shipyard",
        _ => "building",
    }
}

// ---------------------------------------------------------------------------
// Члены комплекса и баланс
// ---------------------------------------------------------------------------

/// Стационарные члены комплекса в порядке `UnitsManager_StationaryUnits`.
///
/// C++ `ProductionManager` сравнивает `complex == (*it).GetComplex()`;
/// принадлежность команде следует из самого комплекса, но в `max_core`
/// `ComplexId` уникален только внутри команды, поэтому команда проверяется
/// явно.
pub fn complex_units(state: &GameState, team: TeamId, complex: ComplexId) -> Vec<UnitId> {
    state
        .unit_list(UnitListKind::Stationary)
        .iter()
        .copied()
        .filter(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.team == team && unit.complex == Some(complex))
        })
        .collect()
}

/// `Cargo_GetNetProduction` + `Cargo_GetInventory`, просуммированные по
/// комплексу — `total` из `ProductionManager::ProductionManager`
/// (`production_manager.cpp:97-157`) и `Access_UpdateResourcesTotal`
/// (`access.cpp:912-934`).
///
/// У паузы (`build_paused`) расход строящегося завода уже исключён самим
/// [`cargo_get_net_production`], поэтому отдельная фильтрация не нужна.
pub fn complex_totals(state: &GameState, team: TeamId, complex: ComplexId) -> EnergyTotals {
    let mut totals = EnergyTotals::default();

    for id in complex_units(state, team, complex) {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        let net = cargo_get_net_production(state, unit, true);
        let inventory = cargo_get_inventory(unit);

        totals.raw += net.raw + inventory.raw;
        totals.fuel += net.fuel + inventory.fuel;
        totals.gold += net.gold + inventory.gold;
        totals.power += net.power;
        totals.life += net.life;
    }

    totals
}

/// Строящийся завод, который ещё не на паузе (`ValidateIndustry`).
#[inline]
fn is_building_factory(unit: &UnitInfo) -> bool {
    is_factory(unit.unit_type)
        && unit.orders == UnitOrder::Build
        && unit.state != UnitOrderState::UnitReady
        && !unit.build_paused
}

/// Потребитель, который C++ может выключить: он **включён** и потребляет
/// энергию (для `RESEARCH`/`GREENHSE`/`COMMTWR` это `ORDER_POWER_ON`, для
/// шахты ещё `ORDER_NEW_ALLOCATE`).
#[inline]
fn is_active_switchable_consumer(unit: &UnitInfo) -> bool {
    if !is_switchable_consumer(unit.unit_type) {
        return false;
    }

    match unit.unit_type {
        UnitType::MiningStation => {
            matches!(unit.orders, UnitOrder::PowerOn | UnitOrder::NewAllocate)
        }
        _ => unit.orders == UnitOrder::PowerOn,
    }
}

/// Сбрасывает паузу строящихся заводов комплекса: решение принимается заново
/// на текущих запасах.
fn reset_pauses(state: &mut GameState, units: &[UnitId]) {
    for id in units {
        if state
            .unit(*id)
            .is_some_and(|unit| is_factory(unit.unit_type) && unit.build_paused)
        {
            if let Some(target) = state.unit_mut(*id) {
                target.build_paused = false;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Производители энергии
// ---------------------------------------------------------------------------

/// Сколько энергии и топлива даёт/тратит включённый производитель — снимаем
/// его вклад из `totals`, чтобы получить базу «все производители выключены».
fn without_producers(state: &GameState, units: &[UnitId], totals: EnergyTotals) -> EnergyTotals {
    let mut baseline = totals;

    for id in units {
        let Some(unit) = state.unit(*id) else {
            continue;
        };

        if unit.orders == UnitOrder::PowerOn && is_power_producer(unit.unit_type) {
            baseline.power -= producer_power(unit.unit_type);
            baseline.fuel += producer_fuel(unit.unit_type);
        }
    }

    baseline
}

/// Порт `OptimizePowerProduction`/`OptimizePowerConsumption`
/// (`production_manager.cpp:481-562`): выбрать, сколько `POWERSTN`/`POWGEN`
/// должно работать, и применить приказы.
///
/// C++ сначала считает целевые `power_station_active`/`power_generator_active`
/// формулами с `count`, затем `UpdateUnitPowerConsumption` доводит приказы.
/// Порт делает тот же выбор «станции раньше генераторов» (станция даёт 6
/// энергии за 6 топлива, генератор — 1 за 2), но без промежуточной арифметики:
/// наблюдаемый баланс и приказы совпадают на рабочих сценариях (см. шапку).
fn settle_producers(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    units: &[UnitId],
    totals: EnergyTotals,
    adjustments: &mut Vec<Adjustment>,
) -> EnergyTotals {
    let baseline = without_producers(state, units, totals);

    let stations: Vec<UnitId> = units
        .iter()
        .copied()
        .filter(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::PowerStation)
        })
        .collect();
    let generators: Vec<UnitId> = units
        .iter()
        .copied()
        .filter(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::PowerGenerator)
        })
        .collect();

    let demand = (-baseline.power).max(0);

    // C++-формула выбора станции: `(power + station_rate - count) / station_rate`
    // с `count = (generator_power * station_fuel) / generator_fuel = 3`.
    let mut stations_on = stations.len().min(((demand + 3) / 6) as usize);
    let mut generators_on = generators
        .len()
        .min(demand.saturating_sub(stations_on as i32 * 6) as usize);

    // Топливо: включённый производитель не должен уводить баланс в минус
    // (`CheckPowerNeed`, `production_manager.cpp:642-664`). Станции дешевле по
    // топливу на единицу энергии, поэтому режем сначала генераторы.
    let fuel_budget = baseline.fuel.max(0);
    let mut cost = stations_on as i32 * 6 + generators_on as i32 * 2;

    while cost > fuel_budget && generators_on > 0 {
        generators_on -= 1;
        cost -= 2;
    }

    while cost > fuel_budget && stations_on > 0 {
        stations_on -= 1;
        cost -= 6;
    }

    // `SatisfyPowerDemand` → `PowerOn` (`production_manager.cpp:332-364`):
    // если производитель нужен, но топлива на него нет — сообщение
    // «Cannot turn X on, fuel needed.», приказ не меняется.
    let produced = stations_on as i32 * 6 + generators_on as i32;
    if produced < demand {
        let candidate = generators
            .get(generators_on)
            .copied()
            .or_else(|| stations.get(stations_on).copied());

        if let Some(unit) = candidate {
            adjustments.push(Adjustment::CannotTurnOn {
                unit,
                shortage: Shortage::Fuel,
            });
        }
    }

    for (index, id) in stations.iter().enumerate() {
        let want = index < stations_on;
        set_producer_order(state, *id, want, adjustments);
    }

    for (index, id) in generators.iter().enumerate() {
        let want = index < generators_on;
        set_producer_order(state, *id, want, adjustments);
    }

    // Пересчёт после смены приказов: формулы C++ тоже корректируют `total`.
    complex_totals(state, team, complex)
}

/// Включает/выключает производителя (`UpdateUnitPowerConsumption`,
/// `production_manager.cpp:164-186`).
fn set_producer_order(
    state: &mut GameState,
    id: UnitId,
    want_on: bool,
    adjustments: &mut Vec<Adjustment>,
) {
    let Some(unit) = state.unit(id) else {
        return;
    };

    if unit.orders == UnitOrder::Disable {
        return;
    }

    let is_on = unit.orders == UnitOrder::PowerOn;

    if want_on == is_on {
        return;
    }

    if want_on {
        // `GameState::set_order` возвращает `delete_path`, а не успех —
        // приказ применяется всегда (C++ `UnitsManager_SetNewOrder`).
        let _ = state.set_order(id, UnitOrder::PowerOn, UnitOrderState::Init);
        adjustments.push(Adjustment::PowerOn {
            unit: id,
            shortage: Shortage::Power,
        });
    } else {
        let _ = state.set_order(id, UnitOrder::PowerOff, UnitOrderState::Init);
        adjustments.push(Adjustment::PowerOff {
            unit: id,
            shortage: Shortage::Power,
        });
    }
}

// ---------------------------------------------------------------------------
// Рабочие
// ---------------------------------------------------------------------------

/// `Delivery`: `UpdateUnitLifeConsumption` (`production_manager.cpp:366-380`) —
/// при нехватке рабочих включить `HABITAT` (даёт +3).
fn settle_workers(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    units: &[UnitId],
    totals: EnergyTotals,
    adjustments: &mut Vec<Adjustment>,
) -> EnergyTotals {
    if totals.life >= 0 {
        return totals;
    }

    let mut life = totals.life;

    for id in units {
        let Some(unit) = state.unit(*id) else {
            continue;
        };

        if unit.unit_type != UnitType::Habitat
            || unit.orders == UnitOrder::PowerOn
            || unit.orders == UnitOrder::Disable
        {
            continue;
        }

        let _ = state.set_order(*id, UnitOrder::PowerOn, UnitOrderState::Init);
        adjustments.push(Adjustment::PowerOn {
            unit: *id,
            shortage: Shortage::Workers,
        });
        // `Cargo_GetLifeConsumptionRate(HABITAT) = -3`, то есть +3 рабочих.
        life += 3;

        if life >= 0 {
            break;
        }
    }

    complex_totals(state, team, complex)
}

// ---------------------------------------------------------------------------
// Оптимизация комплекса
// ---------------------------------------------------------------------------

/// `ProductionManager_OptimizeProduction` (`production_manager.cpp:830-901`) в
/// объёме энергии/рабочих/остановки строек.
///
/// `selected` — выделенный юнит игрока (C++ `manager.selected_unit`): его
/// стройка останавливается первой, его сообщения идут без префикса
/// «Adjustments made:». `proposal` — завод, которому только предстоит начать
/// стройку (`BuildMenu::Build` → `StartBuilding`): проверка «влезет ли заказ».
/// `show_messages` — компоновка [`IndustryOutcome::message`] для верхней
/// строки карты (C++ `is_player`/`show_messages`).
pub fn optimize_complex(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    selected: Option<UnitId>,
    proposal: Option<UnitId>,
    show_messages: bool,
) -> IndustryOutcome {
    let units = complex_units(state, team, complex);

    if units.is_empty() {
        return IndustryOutcome::default();
    }

    reset_pauses(state, &units);

    let mut adjustments: Vec<Adjustment> = Vec::new();
    let mut paused: Vec<(UnitId, Shortage)> = Vec::new();

    let mut totals = complex_totals(state, team, complex);

    // `OptimizePowerProduction` → `OptimizePowerConsumption`.
    totals = settle_producers(state, team, complex, &units, totals, &mut adjustments);

    // `UpdateLifeConsumption`.
    totals = settle_workers(state, team, complex, &units, totals, &mut adjustments);

    // Основной цикл `ProductionManager_OptimizeProduction`: пока есть
    // отрицательный баланс, гасим стройки, затем потребителей.
    let mut guard = units.len().saturating_mul(4).saturating_add(16);

    while totals.has_deficit() && guard > 0 {
        guard -= 1;

        let Some(shortage) = totals.first_shortage() else {
            break;
        };

        // `ValidateIndustry(selected, true)` — выделенный/заявленный завод
        // останавливается первым (C++ `production_manager.cpp:877`).
        let selected_factory = selected.or(proposal).filter(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| is_building_factory(unit) && !paused.iter().any(|(p, _)| p == id))
        });

        if let Some(id) = selected_factory {
            if matches!(shortage, Shortage::Raw | Shortage::Power | Shortage::Workers) {
                pause_factory(state, &mut paused, &mut adjustments, &mut totals, team, complex, id, shortage);
                continue;
            }
        }

        if shortage == Shortage::Power {
            // C++ `OptimizeProduction` (`:877-881`): RESEARCH → GREENHSE →
            // COMMTWR, затем стройки (`OptimizeIndustry`), затем MININGST.
            if let Some(id) = pick_auxiliary_to_off(state, &units) {
                let _ = state.set_order(id, UnitOrder::PowerOff, UnitOrderState::Init);
                adjustments.push(Adjustment::PowerOff {
                    unit: id,
                    shortage: Shortage::Power,
                });
                totals = complex_totals(state, team, complex);
                continue;
            }

            if let Some(id) = pick_factory_to_pause(state, &units, &paused, selected, proposal) {
                pause_factory(
                    state,
                    &mut paused,
                    &mut adjustments,
                    &mut totals,
                    team,
                    complex,
                    id,
                    shortage,
                );
                continue;
            }

            if let Some(id) = pick_mine_to_off(state, &units) {
                let _ = state.set_order(id, UnitOrder::PowerOff, UnitOrderState::Init);
                adjustments.push(Adjustment::PowerOff {
                    unit: id,
                    shortage: Shortage::Power,
                });
                totals = complex_totals(state, team, complex);
                continue;
            }
        } else if matches!(shortage, Shortage::Raw | Shortage::Workers) {
            // `OptimizeIndustry(false)` для материалов; для рабочих C++
            // сначала пробует включить HABITAT (`settle_workers` выше), затем
            // останавливает стройку.
            if let Some(id) = pick_factory_to_pause(state, &units, &paused, selected, proposal) {
                pause_factory(
                    state,
                    &mut paused,
                    &mut adjustments,
                    &mut totals,
                    team,
                    complex,
                    id,
                    shortage,
                );
                continue;
            }
        }

        // Золото тратит только `COMMTWR` (`Cargo_GetGoldConsumptionRate`); при
        // его дефиците C++ гасит именно башню, а не RESEARCH.
        if shortage == Shortage::Gold {
            let candidate = units.iter().copied().find(|id| {
                state.unit(*id).is_some_and(|unit| {
                    unit.unit_type == UnitType::GoldRefinery && is_active_switchable_consumer(unit)
                })
            });

            if let Some(id) = candidate {
                let _ = state.set_order(id, UnitOrder::PowerOff, UnitOrderState::Init);
                adjustments.push(Adjustment::PowerOff {
                    unit: id,
                    shortage: Shortage::Gold,
                });
                totals = complex_totals(state, team, complex);
                continue;
            }
        }

        break;
    }

    // `Access_UpdateResourcesTotal` в конце оптимизации.
    update_resources_total(state, team, complex);

    let unresolved = totals.has_deficit();
    let message = if show_messages {
        compose_message(state, &adjustments, selected.or(proposal))
    } else {
        None
    };

    let powered_on = adjustments
        .iter()
        .filter_map(|adjustment| match adjustment {
            Adjustment::PowerOn { unit, .. } => Some(*unit),
            _ => None,
        })
        .collect();

    let powered_off = adjustments
        .iter()
        .filter_map(|adjustment| match adjustment {
            Adjustment::PowerOff { unit, .. } => Some(*unit),
            _ => None,
        })
        .collect();

    IndustryOutcome {
        totals,
        paused,
        powered_off,
        powered_on,
        message,
        unresolved,
    }
}

/// `ValidateIndustry`/`OptimizeIndustry` (`production_manager.cpp:677-727`):
/// кого из строящихся заводов остановить. Выделенный/заявленный завод — первым.
fn pick_factory_to_pause(
    state: &GameState,
    units: &[UnitId],
    paused: &[(UnitId, Shortage)],
    selected: Option<UnitId>,
    proposal: Option<UnitId>,
) -> Option<UnitId> {
    let is_paused = |id: UnitId| paused.iter().any(|(unit, _)| *unit == id);
    let candidate = |id: UnitId| {
        state
            .unit(id)
            .is_some_and(|unit| is_building_factory(unit) && !is_paused(id))
    };

    if let Some(id) = proposal.filter(|id| candidate(*id)) {
        return Some(id);
    }

    if let Some(id) = selected.filter(|id| candidate(*id)) {
        return Some(id);
    }

    units.iter().copied().find(|id| candidate(*id))
}

/// `ValidateIndustry` → пауза стройки: флаг `build_paused`, событие и
/// пересчёт баланса (расход паузы исключается `cargo_get_net_production`).
#[allow(clippy::too_many_arguments)]
fn pause_factory(
    state: &mut GameState,
    paused: &mut Vec<(UnitId, Shortage)>,
    adjustments: &mut Vec<Adjustment>,
    totals: &mut EnergyTotals,
    team: TeamId,
    complex: ComplexId,
    id: UnitId,
    shortage: Shortage,
) {
    if let Some(target) = state.unit_mut(id) {
        target.build_paused = true;
    }

    paused.push((id, shortage));
    adjustments.push(Adjustment::Paused {
        unit: id,
        shortage,
    });
    *totals = complex_totals(state, team, complex);
}

/// `OptimizeAuxilaryIndustry` (`production_manager.cpp:776-790`): включённый
/// `RESEARCH` → `GREENHSE` → `COMMTWR` (шахту не трогает).
fn pick_auxiliary_to_off(state: &GameState, units: &[UnitId]) -> Option<UnitId> {
    for unit_type in CONSUMER_OFF_ORDER {
        if let Some(id) = units.iter().copied().find(|id| {
            state.unit(*id).is_some_and(|unit| {
                unit.unit_type == unit_type && is_active_switchable_consumer(unit)
            })
        }) {
            return Some(id);
        }
    }

    None
}

/// `OptimizeMiningIndustry` (`production_manager.cpp:729-774`): последний
/// потребитель, которого гасит оптимизатор.
fn pick_mine_to_off(state: &GameState, units: &[UnitId]) -> Option<UnitId> {
    units.iter().copied().find(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.unit_type == UnitType::MiningStation && is_active_switchable_consumer(unit)
        })
    })
}

// ---------------------------------------------------------------------------
// Сообщение
// ---------------------------------------------------------------------------

/// `ComposeIndustryMessage`/`ComposeResourceMessage`
/// (`production_manager.cpp:308-330/465-479`): «Cannot turn X on, Y needed.»,
/// «More Y needed, X turned off./on.», префикс «Adjustments made:» — только
/// если первое действие относится **не** к выделенному юниту.
fn compose_message(
    state: &GameState,
    adjustments: &[Adjustment],
    selected: Option<UnitId>,
) -> Option<String> {
    if adjustments.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = Vec::new();

    for (index, adjustment) in adjustments.iter().enumerate() {
        let (unit, shortage, turned_on, cannot_turn_on) = match adjustment {
            Adjustment::Paused { unit, shortage } => (*unit, *shortage, false, false),
            Adjustment::PowerOff { unit, shortage } => (*unit, *shortage, false, false),
            Adjustment::PowerOn { unit, shortage } => (*unit, *shortage, true, false),
            Adjustment::CannotTurnOn { unit, shortage } => (*unit, *shortage, false, true),
        };

        let Some(unit_info) = state.unit(unit) else {
            continue;
        };
        let name = industry_unit_name(unit_info.unit_type);
        let is_selected = Some(unit) == selected;

        // C++ `PowerOn` пишет строку в буфер напрямую, без префикса
        // «Adjustments made:» (`production_manager.cpp:341-342`).
        if index == 0 && !is_selected && !cannot_turn_on {
            lines.push("Adjustments made:".to_string());
        }

        if is_selected || cannot_turn_on {
            lines.push(format!(
                "Cannot turn {name} on, {} needed.",
                shortage.material()
            ));
        } else if turned_on {
            lines.push(format!(
                "More {} needed, {name} turned on.",
                shortage.material()
            ));
        } else {
            lines.push(format!(
                "More {} needed, {name} turned off.",
                shortage.material()
            ));
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::UnitType;
    use crate::geo::Point;
    use crate::TeamUnits;

    fn state() -> GameState {
        let mut state = GameState::new();
        state.teams[0].team_type = crate::team::TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        state
    }

    fn spawn(state: &mut GameState, unit_type: UnitType, grid: Point) -> UnitId {
        state
            .spawn_unit(unit_type, TeamId::RED, grid, 0)
            .expect("unit")
    }

    #[test]
    fn totals_include_inventory_and_net_production() {
        let mut state = state();
        let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
        let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(12, 10));
        state.unit_mut(dump).unwrap().storage = 7;

        let complex = state.unit(mine).unwrap().complex.unwrap();
        assert_eq!(
            state.unit(dump).unwrap().complex,
            Some(complex),
            "ADUMP вплотную к шахте — один комплекс"
        );

        let totals = complex_totals(&state, TeamId::RED, complex);
        assert_eq!(
            totals.raw,
            7 + i32::from(state.unit(mine).unwrap().raw_mining)
        );
        assert_eq!(totals.power, -1, "включённая шахта потребляет 1 энергию");
    }

    #[test]
    fn surplus_turns_extra_generators_off_and_deficit_turns_them_on() {
        let mut state = state();
        let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
        let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(12, 10));
        let gen1 = spawn(&mut state, UnitType::PowerGenerator, Point::new(9, 11));
        let gen2 = spawn(&mut state, UnitType::PowerGenerator, Point::new(9, 12));
        let raw = spawn(&mut state, UnitType::MaterialStorage, Point::new(14, 12));

        state.unit_mut(fuel).unwrap().storage = 40;
        state.unit_mut(raw).unwrap().storage = 10;
        let complex = state.unit(mine).unwrap().complex.unwrap();
        for id in [gen1, gen2] {
            assert_eq!(state.unit(id).unwrap().complex, Some(complex));
        }

        // Спрос 1 (шахта), хватит одного генератора: второй должен выключиться.
        let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, false);
        assert!(outcome.unresolved == false);
        assert_eq!(
            [gen1, gen2]
                .iter()
                .filter(|id| state.unit(**id).unwrap().orders == UnitOrder::PowerOn)
                .count(),
            1,
            "на спрос 1 энергии достаточно одного генератора"
        );

        // Добавили строящийся завод: спрос 2 — включаются оба.
        let plant = spawn(&mut state, UnitType::LightVehiclePlant, Point::new(12, 11));
        assert_eq!(state.unit(plant).unwrap().complex, Some(complex));
        assert_eq!(state.unit(raw).unwrap().complex, Some(complex));
        if let Some(unit) = state.unit_mut(plant) {
            unit.orders = UnitOrder::Build;
            unit.state = UnitOrderState::Init;
            unit.build_time = 2;
            unit.build_rate = 1;
            unit.build_list = vec![UnitType::Scout];
        }
        let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, false);
        assert!(
            outcome.paused.is_empty(),
            "генераторов хватает: {outcome:?}"
        );
        assert_eq!(
            [gen1, gen2]
                .iter()
                .filter(|id| state.unit(**id).unwrap().orders == UnitOrder::PowerOn)
                .count(),
            2,
            "на спрос 2 нужны оба генератора"
        );
    }
}
