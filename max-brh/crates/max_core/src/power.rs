//! Complex power system — a port of the power part of `production_manager.cpp`.
//!
//! # Oracle
//!
//! * `production_manager.cpp:84-160` — `ProductionManager` collects over complex
//!   members `inventory`/`net_production`/`total` and computes active
//!   `POWERSTN`/`POWGEN` (`power_station_active`/`power_generator_active`);
//! * `production_manager.cpp:164-197` — `UpdateUnitPowerConsumption`:
//!   surplus running generators are turned off, missing ones are turned on
//!   (`UnitsManager_SetNewOrder(ORDER_POWER_ON/OFF, ORDER_STATE_INIT)`);
//! * `production_manager.cpp:481-549` — `OptimizePowerProduction`: how many
//!   stations/generators should run at the current power deficit and
//!   fuel reserve;
//! * `production_manager.cpp:551-562` — `OptimizePowerConsumption`: applies
//!   the plan to producer orders;
//! * `production_manager.cpp:642-664` — `CheckPowerNeed`: whether power and
//!   fuel suffice for additional consumption;
//! * `production_manager.cpp:677-727` — `ValidateIndustry`/`OptimizeIndustry`:
//!   when resources are short, factory construction stops
//!   (`ORDER_STATE_BUILD_ABORT`, in the port — a pause without losing the queue);
//! * `production_manager.cpp:729-790` — `OptimizeMiningIndustry`,
//!   `OptimizeAuxilaryIndustry`: the consumer shutdown order
//!   (`RESEARCH` → `GREENHSE` → `COMMTWR`, then constructions, then `MININGST`);
//! * `production_manager.cpp:366-380` — `UpdateUnitLifeConsumption`: when
//!   workers are short, `HABITAT` is turned on (`life = -3`, i.e. +3 workers);
//! * `game_manager.cpp:2058-2076` — `GameManager_ManageEconomy` →
//!   `TeamUnits::OptimizeComplexes` (`teamunits.cpp:170-175`) invokes
//!   optimization of every complex every turn;
//! * `game_manager.cpp:8183-8204` — `GameManager_ProgressBuildState`: after
//!   spawning units, it recomputes the complexes once more and optimizes them;
//! * `game_manager.cpp:2555-2584` — `GameManager_OptimizeProduction`: with a
//!   negative `material/fuel/gold/power/workers` it starts
//!   `ProductionManager_OptimizeProduction`;
//! * `cargo.cpp:70-138` — `Cargo_GetNetProduction` (factory consumption:
//!   raw+power+life) and `Cargo_ApplyUnitConsumption`;
//! * `cargo.cpp:183-271` — fuel/power/workers/gold rates.
//!
//! # Model
//!
//! Power is not a stock but a **balance** of the complex: `POWERSTN` gives +6, `POWGEN`
//! +1, while enabled `RESEARCH`/`GREENHSE`/`COMMTWR`/`MININGST` and a factory
//! under construction consume 1 each (`cargo_power_consumption_rate`). Workers are also
//! a balance: `HABITAT` gives +3, `TRAINHAL`/`GREENHSE`/`RESEARCH` consume 1 each.
//!
//! The port follows the contract of `max-brh-docs/architecture/client/game-mechanics.md` §3/§5:
//! * a factory order **does not start** if the complex cannot pay for the first turn
//!   (raw, power, life) — [`crate::production::BuildError`];
//! * an already running construction **pauses** on shortage (`build_paused`, the queue
//!   is preserved), rather than cancelled as in C++ (`ValidateIndustry` →
//!   `BUILD_ABORT`); this is an intentional QA requirement (a resource never goes negative);
//! * on a power deficit, buildings are turned off in C++ order:
//!   `RESEARCH` → `GREENHSE` → `COMMTWR` → constructions → `MININGST`;
//! * power producers are turned on/off automatically by balance and
//!   fuel reserve (C++ `OptimizePowerProduction`/`OptimizePowerConsumption`);
//! * before a switched-off power source is given up, the complex tries to
//!   allocate the missing fuel by converting raw/gold mining into fuel mining
//!   (`OptimizeCargoProduction(CARGO_FUEL, …)`, `production_manager.cpp:642-664`,
//!   `:849-861`) — see [`settle_producers`];
//! * C++ does not turn **consumers** disabled due to a deficit back on itself —
//!   the player (`UnitsManager_Popup_OnClick_PowerOn`) or the AI does that; the port
//!   repeats this rule, while generators recover automatically;
//! * the shortage message — `ComposeIndustryMessage`/`DrawResourceMessage`
//!   (`production_manager.cpp:308-330/465-479/792-804`): «Adjustments made:»,
//!   «More power needed, X turned off.», «Cannot turn X on, power needed.».
//!
//! Difference from C++ (documented in `visual-1to1-status.md` §42): the exact
//! arithmetic of `OptimizePowerProduction` (formulas with `count`) is replaced
//! by a deterministic choice equivalent in observed balance:
//! "stations → generators". The fuel redistribution of `OptimizeCargoProduction`
//! is performed by [`crate::alloc::optimize_cargo_production`]; the port keeps
//! its "a resource never goes negative" rule, so a producer whose fuel cannot be
//! covered by reallocation is still not turned on.

use crate::alloc::{cargo_get_inventory, update_resources_total, MiningKind};
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::production::is_factory;
use crate::team::{ComplexId, TeamId};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::victory::{
    cargo_fuel_consumption_rate, cargo_get_net_production, cargo_power_consumption_rate,
};

/// Resource balance of a complex — C++ `Cargo` in `ProductionManager` (`total`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnergyTotals {
    /// Materials (stock + production − consumption of factories under construction).
    pub raw: i32,
    /// Fuel (stock + production − fuel of power producers).
    pub fuel: i32,
    /// Gold.
    pub gold: i32,
    /// Power (production − consumption).
    pub power: i32,
    /// Workers (`life`).
    pub life: i32,
}

impl EnergyTotals {
    /// Whether at least one balance is negative (`GameManager_OptimizeProduction`).
    #[inline]
    pub const fn has_deficit(&self) -> bool {
        self.raw < 0 || self.fuel < 0 || self.gold < 0 || self.power < 0 || self.life < 0
    }

    /// The first shortage in `ValidateIndustry`/`Cargo_ApplyUnitConsumption` order:
    /// materials → power → workers → fuel → gold.
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

/// Which resource is short (`ComposeIndustryMessage`, `material`).
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
    /// The resource string in the C++ message (`_(...)` from `language.json`).
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

/// An optimizer action — for the `ComposeIndustryMessage` message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Adjustment {
    /// Factory construction is paused (`ValidateIndustry`).
    Paused { unit: UnitId, shortage: Shortage },
    /// A consumer is turned off (`OptimizeAuxilaryIndustry`/`OptimizeMiningIndustry`).
    PowerOff { unit: UnitId, shortage: Shortage },
    /// A producer is turned on (`UpdateUnitPowerConsumption` /
    /// `UpdateUnitLifeConsumption`); `shortage` — what exactly was needed
    /// (`power` at a generator, `workers` at `HABITAT`).
    PowerOn { unit: UnitId, shortage: Shortage },
    /// `PowerOn` failed because of fuel (`production_manager.cpp:332-352`):
    /// the "Cannot turn X on, fuel needed." message without changing the order.
    CannotTurnOn { unit: UnitId, shortage: Shortage },
}

/// Result of complex optimization.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndustryOutcome {
    /// Balance after all actions.
    pub totals: EnergyTotals,
    /// Constructions paused, and the reason.
    pub paused: Vec<(UnitId, Shortage)>,
    /// Consumers turned off.
    pub powered_off: Vec<UnitId>,
    /// Power producers turned on.
    pub powered_on: Vec<UnitId>,
    /// Message for the top map line (`MESSAGE_BOX_NOTICE`) if
    /// `show_messages` and there were actions.
    pub message: Option<String>,
    /// A negative balance remains after all actions (C++ `return` from
    /// `ProductionManager_OptimizeProduction`).
    pub unresolved: bool,
}

impl IndustryOutcome {
    /// `BuildMenu::Build` error for a rejected construction.
    pub fn build_error(&self) -> crate::production::BuildError {
        self.paused
            .first()
            .map(|(_, shortage)| shortage_build_error(*shortage))
            .unwrap_or(crate::production::BuildError::NotEnoughMaterials)
    }
}

/// `Shortage` → `BuildError` (for refusing to start an order).
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
// Tables
// ---------------------------------------------------------------------------

/// `Cargo_GetPowerConsumptionRate(...) < 0` — a power producer.
#[inline]
pub fn is_power_producer(unit_type: UnitType) -> bool {
    matches!(unit_type, UnitType::PowerStation | UnitType::PowerGenerator)
}

/// A building that has a C++ `power on/off` entry and that the optimizer
/// may turn off on a power deficit (`OptimizeAuxilaryIndustry` +
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

/// Order of forced consumer shutdown
/// (`ProductionManager_OptimizeProduction`, `production_manager.cpp:877-881`):
/// `RESEARCH` → `GREENHSE` → `COMMTWR`, then constructions (`OptimizeIndustry`) and
/// only then `MININGST` (`OptimizeMiningIndustry`).
pub const CONSUMER_OFF_ORDER: [UnitType; 3] = [
    UnitType::ResearchCentre,
    UnitType::EcoSphere,
    UnitType::GoldRefinery,
];

/// Positive output of a producer (`-Cargo_GetPowerConsumptionRate`).
#[inline]
fn producer_power(unit_type: UnitType) -> i32 {
    -cargo_power_consumption_rate(unit_type)
}

/// Fuel consumption of a producer (`Cargo_GetFuelConsumptionRate`).
#[inline]
fn producer_fuel(unit_type: UnitType) -> i32 {
    cargo_fuel_consumption_rate(unit_type)
}

/// Unit name for a message — `ResourceManager_GetUnit(type).GetSingularName()`.
///
/// In C++ the names live in `language.json`; `max_core` has no name table
/// (`max_app::buildmenu::unit_name`), so only the types that occur in
/// power messages are listed here.
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
// Complex members and balance
// ---------------------------------------------------------------------------

/// Stationary members of the complex in `UnitsManager_StationaryUnits` order.
///
/// C++ `ProductionManager` compares `complex == (*it).GetComplex()`;
/// team ownership follows from the complex itself, but in `max_core`
/// `ComplexId` is unique only within a team, so the team is checked
/// explicitly.
pub(crate) fn complex_units(state: &GameState, team: TeamId, complex: ComplexId) -> Vec<UnitId> {
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

/// `Cargo_GetNetProduction` + `Cargo_GetInventory`, summed over
/// the complex — `total` from `ProductionManager::ProductionManager`
/// (`production_manager.cpp:97-157`) and `Access_UpdateResourcesTotal`
/// (`access.cpp:912-934`).
///
/// For a pause (`build_paused`) the consumption of the factory under construction is already excluded by
/// [`cargo_get_net_production`] itself, so no separate filtering is needed.
pub(crate) fn complex_totals(state: &GameState, team: TeamId, complex: ComplexId) -> EnergyTotals {
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

/// A factory under construction that is not paused yet (`ValidateIndustry`).
#[inline]
fn is_building_factory(unit: &UnitInfo) -> bool {
    is_factory(unit.unit_type)
        && unit.orders == UnitOrder::Build
        && unit.state != UnitOrderState::UnitReady
        && !unit.build_paused
}

/// A consumer C++ may turn off: it is **on** and consumes
/// power (for `RESEARCH`/`GREENHSE`/`COMMTWR` that is `ORDER_POWER_ON`, for
/// a mine also `ORDER_NEW_ALLOCATE`).
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

/// Resets the pause of the complex's factories under construction: the decision is made anew
/// on the current stocks.
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
// Power producers
// ---------------------------------------------------------------------------

/// How much power and fuel an enabled producer gives/consumes — its contribution is
/// subtracted from `totals` to obtain the base "all producers off".
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

/// Bound of the `OptimizePowerProduction` → `OptimizeCargoProduction` re-planning
/// rounds (`production_manager.cpp:830-861`). Every successful round allocates a
/// finite number of free mining units, so the loop converges much earlier; the
/// bound keeps the optimizer deterministic on a pathological fixture.
const POWER_REDISTRIBUTION_ROUNDS: usize = 4;

/// Port of `OptimizePowerProduction`/`OptimizePowerConsumption`
/// (`production_manager.cpp:481-562`, `production_manager.cpp:830-901`): choose
/// how many `POWERSTN`/`POWGEN` should run, redistribute mining for their fuel,
/// and apply the orders.
///
/// C++ first computes the target `power_station_active`/`power_generator_active`
/// with formulas involving `count`, then `UpdateUnitPowerConsumption` adjusts the orders.
/// The port makes the same "stations before generators" choice (a station gives 6
/// power for 6 fuel, a generator — 1 for 2), but without intermediate arithmetic:
/// the observed balance and orders match in working scenarios (see the header).
///
/// # Automatic fuel redistribution
///
/// `ProductionManager_OptimizeProduction` calls `CheckPowerNeed(0)` in every
/// iteration of its debt loop (`production_manager.cpp:847-849`), and
/// `CheckPowerNeed` runs `OptimizeCargoProduction(CARGO_FUEL, -total.fuel, false)`
/// when the planned producers overdraw the complex fuel
/// (`production_manager.cpp:642-664`). The same call is repeated in the debt loop
/// itself (`production_manager.cpp:855-857`). It converts raw and gold mining of
/// the complex into fuel mining *before* the optimizer gives up on the
/// switched-off power source and starts shutting the industry down. The port
/// does the same: the fuel for the planned producers is allocated here, and only
/// when the complex cannot reallocate it (no fuel in the mine cells, no free
/// mining units) are the producers cut back and a mine switched off.
fn settle_producers(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    units: &[UnitId],
    totals: EnergyTotals,
    adjustments: &mut Vec<Adjustment>,
) -> EnergyTotals {
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

    let mut totals = totals;
    let mut stations_on = 0usize;
    let mut generators_on = 0usize;
    let mut demand = 0i32;
    let mut fuel_budget = 0i32;
    let mut cost = 0i32;
    let mut redistributed = false;

    // The demand is computed with all producers off (`without_producers`), so a
    // producer that is currently switched off is considered for re-enabling.
    // C++ re-plans after every successful fuel reallocation; the free mining
    // units of the complex are finite, so one round is normally enough — the
    // bound only guards against a pathological fixture.
    for _round in 0..=POWER_REDISTRIBUTION_ROUNDS {
        let baseline = without_producers(state, units, totals);

        demand = (-baseline.power).max(0);

        // C++ station selection formula: `(power + station_rate - count) / station_rate`
        // with `count = (generator_power * station_fuel) / generator_fuel = 3`.
        stations_on = stations.len().min(((demand + 3) / 6) as usize);
        generators_on = generators
            .len()
            .min(demand.saturating_sub(stations_on as i32 * 6) as usize);

        fuel_budget = baseline.fuel.max(0);
        cost = stations_on as i32 * 6 + generators_on as i32 * 2;

        if cost <= fuel_budget || demand == 0 {
            break;
        }

        // `OptimizeCargoProduction(CARGO_FUEL, -total.fuel, false)` — raw/gold
        // mining of the complex is converted into the fuel the plan needs.
        let added = crate::alloc::optimize_cargo_production(
            state,
            team,
            complex,
            MiningKind::Fuel,
            cost - fuel_budget,
            false,
        );

        if added == 0 {
            break;
        }

        totals = complex_totals(state, team, complex);
        redistributed = true;
    }

    if redistributed {
        // The plan above may belong to the pre-conversion balance when the loop
        // exhausted its rounds right after a successful reallocation; C++
        // re-runs `OptimizePowerProduction` on the new totals
        // (`CheckPowerNeed`, `production_manager.cpp:642-664`).
        let baseline = without_producers(state, units, totals);

        demand = (-baseline.power).max(0);
        stations_on = stations.len().min(((demand + 3) / 6) as usize);
        generators_on = generators
            .len()
            .min(demand.saturating_sub(stations_on as i32 * 6) as usize);
        fuel_budget = baseline.fuel.max(0);
        cost = stations_on as i32 * 6 + generators_on as i32 * 2;
    }

    // Fuel: an enabled producer must not drive the balance negative
    // (`CheckPowerNeed`, `production_manager.cpp:642-664`). Stations are cheaper in
    // fuel per unit of power, so generators are cut first.
    while cost > fuel_budget && generators_on > 0 {
        generators_on -= 1;
        cost -= 2;
    }

    while cost > fuel_budget && stations_on > 0 {
        stations_on -= 1;
        cost -= 6;
    }

    // `SatisfyPowerDemand` → `PowerOn` (`production_manager.cpp:332-364`):
    // if a producer is needed but there is no fuel for it — the message
    // "Cannot turn X on, fuel needed.", the order does not change.
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

    // Recompute after order changes: the C++ formulas also adjust `total`.
    complex_totals(state, team, complex)
}

/// Turns a producer on/off (`UpdateUnitPowerConsumption`,
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
        // `GameState::set_order` returns `delete_path`, not success —
        // the order is always applied (C++ `UnitsManager_SetNewOrder`).
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
// Workers
// ---------------------------------------------------------------------------

/// `Delivery`: `UpdateUnitLifeConsumption` (`production_manager.cpp:366-380`) —
/// on a worker shortage, turn on `HABITAT` (gives +3).
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
        // `Cargo_GetLifeConsumptionRate(HABITAT) = -3`, i.e. +3 workers.
        life += 3;

        if life >= 0 {
            break;
        }
    }

    complex_totals(state, team, complex)
}

// ---------------------------------------------------------------------------
// Complex optimization
// ---------------------------------------------------------------------------

/// `ProductionManager_OptimizeProduction` (`production_manager.cpp:830-901`) in
/// the scope of power/workers/construction stopping.
///
/// `selected` — the player's selected unit (C++ `manager.selected_unit`): its
/// construction is stopped first, its messages go without the
/// "Adjustments made:" prefix. `proposal` — a factory that is about to start
/// construction (`BuildMenu::Build` → `StartBuilding`): a check whether the order fits.
/// `show_messages` — composition of [`IndustryOutcome::message`] for the top
/// map line (C++ `is_player`/`show_messages`).
pub(crate) fn optimize_complex(
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

    // Main loop of `ProductionManager_OptimizeProduction`: while there is a
    // negative balance, we stop constructions, then consumers.
    let mut guard = units.len().saturating_mul(4).saturating_add(16);

    while totals.has_deficit() && guard > 0 {
        guard -= 1;

        let Some(shortage) = totals.first_shortage() else {
            break;
        };

        // `CheckPowerNeed(0)` → `OptimizeCargoProduction(CARGO_FUEL, -total.fuel,
        // false)` (`production_manager.cpp:849-857`): a fuel deficit of the
        // enabled producers is first balanced by reallocating the complex's
        // mining (raw/gold → fuel); only if that yields nothing are consumers
        // switched off. `settle_producers` already allocates the fuel of the
        // planned producers, so this path covers the remaining cases.
        if shortage == Shortage::Fuel && totals.fuel < 0 {
            let added = crate::alloc::optimize_cargo_production(
                state,
                team,
                complex,
                MiningKind::Fuel,
                -totals.fuel,
                false,
            );

            if added > 0 {
                totals = complex_totals(state, team, complex);
                continue;
            }
        }

        // `ValidateIndustry(selected, true)` — the selected/proposed factory
        // is stopped first (C++ `production_manager.cpp:877`).
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
            // COMMTWR, then constructions (`OptimizeIndustry`), then MININGST.
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
            // `OptimizeIndustry(false)` for materials; for workers C++
            // first tries to turn HABITAT on (`settle_workers` above), then
            // stops the construction.
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

        // Only `COMMTWR` spends gold (`Cargo_GetGoldConsumptionRate`); on
        // its deficit C++ switches off the tower, not RESEARCH.
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

    // `Access_UpdateResourcesTotal` at the end of optimization.
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
/// which factory under construction to stop. The selected/proposed factory is first.
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

/// `ValidateIndustry` → construction pause: the `build_paused` flag, an event and
/// balance recomputation (pause consumption is excluded by `cargo_get_net_production`).
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

/// `OptimizeAuxilaryIndustry` (`production_manager.cpp:776-790`): an enabled
/// `RESEARCH` → `GREENHSE` → `COMMTWR` (does not touch the mine).
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

/// `OptimizeMiningIndustry` (`production_manager.cpp:729-774`): the last
/// consumer the optimizer turns off.
fn pick_mine_to_off(state: &GameState, units: &[UnitId]) -> Option<UnitId> {
    units.iter().copied().find(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.unit_type == UnitType::MiningStation && is_active_switchable_consumer(unit)
        })
    })
}

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

/// `ComposeIndustryMessage`/`ComposeResourceMessage`
/// (`production_manager.cpp:308-330/465-479`): «Cannot turn X on, Y needed.»,
/// "More Y needed, X turned off./on.", the "Adjustments made:" prefix — only
/// if the first action concerns a unit **other than** the selected one.
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

        // C++ `PowerOn` writes the string to the buffer directly, without the prefix
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

        // Demand 1 (mine), one generator suffices: the second must turn off.
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

        // Added a factory under construction: demand 2 — both turn on.
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

    #[test]
    fn totals_first_shortage_follows_the_cpp_validation_order() {
        // `ValidateIndustry`/`Cargo_ApplyUnitConsumption` order:
        // materials → power → workers → fuel → gold.
        let mut totals = EnergyTotals::default();
        assert!(!totals.has_deficit());
        assert_eq!(totals.first_shortage(), None);

        totals = EnergyTotals {
            raw: -1,
            fuel: -1,
            gold: -1,
            power: -1,
            life: -1,
        };
        assert!(totals.has_deficit());
        assert_eq!(totals.first_shortage(), Some(Shortage::Raw));

        totals.raw = 0;
        assert_eq!(totals.first_shortage(), Some(Shortage::Power));

        totals.power = 0;
        assert_eq!(totals.first_shortage(), Some(Shortage::Workers));

        totals.life = 0;
        assert_eq!(totals.first_shortage(), Some(Shortage::Fuel));

        totals.fuel = 0;
        assert_eq!(totals.first_shortage(), Some(Shortage::Gold));

        totals.gold = 0;
        assert!(!totals.has_deficit());
    }

    #[test]
    fn shortage_build_error_covers_every_shortage() {
        use crate::production::BuildError;

        assert_eq!(shortage_build_error(Shortage::Raw), BuildError::NotEnoughMaterials);
        assert_eq!(shortage_build_error(Shortage::Power), BuildError::NotEnoughPower);
        assert_eq!(shortage_build_error(Shortage::Workers), BuildError::NotEnoughWorkers);
        assert_eq!(shortage_build_error(Shortage::Fuel), BuildError::NotEnoughFuel);
        assert_eq!(shortage_build_error(Shortage::Gold), BuildError::NotEnoughGold);
    }

    #[test]
    fn power_tables_match_cargo_cpp() {
        // `Cargo_GetPowerConsumptionRate` (cargo.cpp:203).
        assert!(is_power_producer(UnitType::PowerStation));
        assert!(is_power_producer(UnitType::PowerGenerator));
        assert!(!is_power_producer(UnitType::MiningStation));
        assert_eq!(producer_power(UnitType::PowerStation), 6);
        assert_eq!(producer_power(UnitType::PowerGenerator), 1);
        assert_eq!(producer_fuel(UnitType::PowerStation), 6);
        assert_eq!(producer_fuel(UnitType::PowerGenerator), 2);
        assert_eq!(cargo_power_consumption_rate(UnitType::TrainingHall), 1);
        assert_eq!(cargo_power_consumption_rate(UnitType::Tank), 0);

        // `OptimizeAuxilaryIndustry`/`OptimizeMiningIndustry` scope.
        assert!(is_switchable_consumer(UnitType::ResearchCentre));
        assert!(is_switchable_consumer(UnitType::EcoSphere));
        assert!(is_switchable_consumer(UnitType::GoldRefinery));
        assert!(is_switchable_consumer(UnitType::MiningStation));
        assert!(!is_switchable_consumer(UnitType::TrainingHall));
        assert_eq!(
            CONSUMER_OFF_ORDER,
            [
                UnitType::ResearchCentre,
                UnitType::EcoSphere,
                UnitType::GoldRefinery
            ]
        );

        // Message names (`ResourceManager_GetUnit(type).GetSingularName()`).
        assert_eq!(industry_unit_name(UnitType::ResearchCentre), "Research Center");
        assert_eq!(industry_unit_name(UnitType::EcoSphere), "Greenhouse");
        assert_eq!(industry_unit_name(UnitType::GoldRefinery), "Comm Tower");
        assert_eq!(industry_unit_name(UnitType::Tank), "building");
    }

    #[test]
    fn a_producer_is_not_started_without_fuel() {
        let mut state = state();
        let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
        let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(9, 10));
        let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(9, 11));

        // `POWGEN` needs 2 fuel (`Cargo_GetFuelConsumptionRate`); only 1 is stocked.
        state.unit_mut(fuel).unwrap().storage = 1;
        let complex = state.unit(mine).unwrap().complex.unwrap();
        assert_eq!(state.unit(generator).unwrap().complex, Some(complex));

        let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, true);

        assert_eq!(
            state.unit(generator).unwrap().orders,
            UnitOrder::PowerOff,
            "POWGEN with 1 fuel cannot cover its rate"
        );
        assert!(
            outcome
                .message
                .as_deref()
                .is_some_and(|message| message.contains("Cannot turn Power Generator on, fuel needed.")),
            "message: {:?}",
            outcome.message
        );
        // With no producer left the mine becomes the last consumer to switch off.
        assert_eq!(state.unit(mine).unwrap().orders, UnitOrder::PowerOff);
    }

    #[test]
    fn worker_shortage_turns_a_habitat_on() {
        let mut state = state();
        let factory = spawn(&mut state, UnitType::TrainingHall, Point::new(12, 11));
        let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(12, 13));
        let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(14, 11));
        let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(14, 12));
        let habitat = spawn(&mut state, UnitType::Habitat, Point::new(11, 12));

        state.unit_mut(dump).unwrap().storage = 20;
        state.unit_mut(fuel).unwrap().storage = 50;
        // `DeployUnit` puts `HABITAT` into `POWER_ON`; the C++ optimizer turns it on
        // only when it was off (`UpdateUnitLifeConsumption`).
        state.unit_mut(habitat).unwrap().orders = UnitOrder::PowerOff;
        state.unit_mut(habitat).unwrap().state = UnitOrderState::ExecutingOrder;

        let complex = state.unit(factory).unwrap().complex.unwrap();
        for id in [dump, generator, fuel, habitat] {
            assert_eq!(state.unit(id).unwrap().complex, Some(complex));
        }

        // A factory under construction consumes 1 worker (`Cargo_GetLifeConsumptionRate`).
        {
            let unit = state.unit_mut(factory).unwrap();
            unit.orders = UnitOrder::Build;
            unit.state = UnitOrderState::Init;
            unit.build_time = 2;
            unit.build_rate = 1;
            unit.build_list = vec![UnitType::Infantry];
        }

        let totals_before = complex_totals(&state, TeamId::RED, complex);
        assert_eq!(totals_before.life, -1, "TRAINHAL needs one worker");

        let outcome = optimize_complex(&mut state, TeamId::RED, complex, None, None, false);

        assert_eq!(state.unit(habitat).unwrap().orders, UnitOrder::PowerOn);
        assert!(outcome.paused.is_empty(), "workers supplied: {outcome:?}");
        assert!(
            outcome.powered_on.contains(&habitat),
            "HABITAT turned on: {outcome:?}"
        );
        assert!(outcome.totals.life >= 0);
    }
}
