//! Production and construction — factory and builder queues (P2).
//!
//! # Oracle
//!
//! * `buildmenu.cpp` — menu composition and the "which builder builds what" table
//!   (`Builder_GetBuildableUnits` → `GameRulesHandler` → `max_get_buildable_units`
//!   from `default.game_rules.lua`, [`BUILDABLE`]);
//! * `unitinfo.cpp` — `BuildOrder`, `StartBuilding`, `SpawnNewUnit`,
//!   `CancelBuilding`, `GetTurnsToBuild`;
//! * `game_manager.cpp` — `GameManager_ManageEconomy` (`UpdateProduction`,
//!   per-turn charging) and `GameManager_ProgressBuildState`
//!   (`GameManager_SpawnNewUnits` — spawn at the end of the "production turn");
//! * `cargo.cpp` — `Cargo_GetRawConsumptionRate` (the consumption rate),
//!   `Cargo_ApplyUnitConsumption` (raw + fuel + power + life + gold);
//! * `production_manager.cpp` — `ProductionManager_OptimizeProduction`
//!   (power/workers; the port — [`crate::power`]).
//!
//! # Model
//!
//! 1. Every builder/factory has a queue [`crate::unit::UnitInfo::build_list`],
//!    `build_time` (how many "build turns" remain) and `build_rate`
//!    (1/2/4 — acceleration, `buildmenu.cpp:729`).
//! 2. The `Build` order ([`ProductionManager::commit`]) fixes the first queue
//!    element: `build_time = turns`, `build_rate = rate`, `target_grid = site`
//!    (for a mobile builder the site is searched by [`find_build_site`], as
//!    `UnitsManager_IssueBuildOrder`).
//! 3. Every turn [`ProductionManager::progress`] (called from
//!    [`crate::game::GameState::finish_turn`]) **first** charges the turn
//!    cost, **then** advances/spawns — the C++ order
//!    (`ManageEconomy` → `ProgressBuildState`).
//! 4. `build_time <= build_rate` → [`ProductionManager::spawn_new_unit`]:
//!    the factory spawns the unit into an adjacent cell and gets
//!    `ORDER_STATE_UNIT_READY` (like `UnitInfo::SpawnNewUnit`), while a mobile
//!    builder completes the building on the site: the building is placed on the tape cell
//!    in `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY` (not drawn), the builder —
//!    `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` and **stays on the site**
//!    ([`builder_awaiting_exit`]). The exit is initiated by the player clicking a cell
//!    (`ORDER_ACTIVATE`/`IN_TRANSITION`, `game_manager.cpp:6879`) or by the AI with the
//!    `TaskActivate` task ([`crate::unit_order::issue_builder_exit`]); unfolding
//!    (`ScaleUnit(EXPAND)`, tape removal, the slab stays) is done by
//!    the [`crate::game::GameState::process_orders`] `BuildingReady` branch only after
//!    the builder leaves the site (`UnitsManager_BuildingReady`).
//!
//! # Economy
//!
//! C++ spends **materials** on production (`Cargo_GetRawConsumptionRate`,
//! `cargo.cpp:140`), not team gold:
//!
//! * a mobile ground builder — from **its own storage** (`UnitInfo::
//!   UpdateProduction`, `unitinfo.cpp:2904-2915`): `storage -= rate`; the port
//!   does the same in [`crate::game::GameState::update_production`], but **without**
//!   driving storage negative**: if `storage < rate`, there is no charge, and the unit
//!   gets `build_paused` and the construction turn does not advance
//!   ([`ProductionEventKind::PausedNoResources`]). In this case C++ goes into
//!   debt and still completes the build — the port does not (a QA requirement);
//! * a stationary factory — from **its complex's materials**
//!   (`UnitInfo::StartBuilding`, `unitinfo.cpp:2757`:
//!   `complex->material -= Cargo_GetRawConsumptionRate`; `Cargo_GetNetProduction`,
//!   `cargo.cpp:88-95` subtracts the same rate from the balance every turn), and
//!   `ProductionManager::ValidateIndustry` (`production_manager.cpp:673-707`)
//!   interrupts the construction when the complex lacks materials. A complex is
//!   a connected group of buildings/connectors ([`crate::complex`]), and its stock and
//!   capacity are the **sum** of connected storages (`Complex::GetCargoInfo`,
//!   `complex.cpp:136-147`): `ADUMP`/`FDUMP`/`GOLDSM`/mine in one group
//!   form a common pool. The rate is charged from member storages in
//!   `UnitsManager_StationaryUnits` order — like `Complex::Transfer`
//!   (`complex.cpp:178-203`). A shortage pauses the order
//!   ([`ProductionEventKind::PausedNoMaterials`]), the order is not lost;
//! * **power and workers** — also from the complex pool (`Cargo_ApplyUnitConsumption`,
//!   `cargo.cpp:132-138`): every factory under construction spends `power`, `life` —
//!   `TRAINHAL`/`GREENHSE`/`RESEARCH`. Before the start and on every turn
//!   [`crate::power::optimize_complex`] turns producers on
//!   (`POWGEN`/`POWERSTN`), turns consumers off on a deficit and pauses
//!   the construction (`PausedNoPower`/`PausedNoWorkers`); the order **does not
//!   start** if the first turn cannot be paid ([`BuildError::NotEnoughPower`],
//!   [`BuildError::NotEnoughWorkers`]). The message about this —
//!   [`crate::power::IndustryOutcome::message`]
//!   (`production_manager.cpp:308-330/792-804`);
//! * **team gold is not spent on production** — in C++ it is needed only by
//!   `CargoMenu` (purchases during mobilization) and upgrades. The port's old behavior
//!   ("the factory pays with gold") caused "factories produce nothing":
//!   after mobilization credits are usually zero, while mines yield materials.
//!
//! Total construction at `rate = 1` — `cargo_raw_consumption_rate(builder, 1) * turns`,
//! exactly `UnitInfo::GetNormalRateBuildCost` / `CargoMenu::BuyUnit`.
//!
//! `MobileBuildMenu::Build` (`buildmenu.cpp:903-906`) requires the whole
//! construction cost on hand at 1× — [`BuildError::NotEnoughMaterials`].
//!
//! # What is simplified (honestly)
//!
//! * power/workers are ported in [`crate::power`] to the extent of the observable
//!   balance: the exact `OptimizePowerProduction` arithmetic with `count` is replaced
//!   by an equivalent deterministic choice "stations → generators" (see
//!   the `power` header), and `OptimizeCargoProduction` (redistribution of mining
//!   for fuel) is not ported;
//! * **consumers** disabled on a deficit (`RESEARCH`/`GREENHSE`/`COMMTWR`/
//!   `MININGST`) are not turned back on by C++ itself — only by the player/AI; the port follows
//!   this rule (`production_manager.cpp:729-790`);
//! * the factory spawns the unit directly into a free adjacent cell instead of storing it
//!   inside nor "driving out" via `AWAIT_TAPE_POSITIONING`;
//! * `quick_build` (a training-mission setting) is not modeled;
//! * a mobile builder places the building on `target_grid` without the move
//!   animation/`StartBuilding` teleport; the construction site (tape, cones, slab)
//!   appears immediately on order (`PrepareConstructionSite` when
//!   packet 08/P4 is applied, `events::apply_command`), not after 4 frames
//!   `TAPE_POSITIONING`;
//! * the slab/building "growing" animation (`scaler_adjust`/`AwaitScaling`) runs
//!   in the simulation, but the render frame does not scale it yet.

use crate::access;
use crate::classification::UnitTypeClassify;
use crate::deploy::{LARGE_SLAB_IMAGE_COUNT, SMALL_SLAB_IMAGE_COUNT};
use crate::enums::unit_flags;
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::unit_values::ATTRIB_TURNS;
use crate::victory::cargo_raw_consumption_rate;

/// `max_builder_capability_list` table (`default.game_rules.lua`) — 1:1.
///
/// Row and element order match the Lua file: the order of
/// rows in `BuildMenu` depends on it (C++ `Builder_GetBuildableUnits`).
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

/// Producing factories (stationary, `Cargo_GetPowerConsumptionRate == 1`).
pub const FACTORIES: [UnitType; 5] = [
    UnitType::Shipyard,
    UnitType::LightVehiclePlant,
    UnitType::HeavyVehiclePlant,
    UnitType::AirUnitsPlant,
    UnitType::TrainingHall,
];

/// Mobile builders (`CONSTRCT`/`ENGINEER`).
pub const MOBILE_BUILDERS: [UnitType; 2] = [UnitType::Constructor, UnitType::Engineer];

/// List traversal order in `GameManager_ProgressBuildState` (C++): spawning
/// GroundCover → MobileLandSea → Stationary → MobileAir.
pub const PROGRESS_LIST_ORDER: [UnitListKind; 4] = [
    UnitListKind::GroundCover,
    UnitListKind::MobileLandSea,
    UnitListKind::Stationary,
    UnitListKind::MobileAir,
];

/// List traversal order in `GameManager_ManageEconomy` (C++ `UpdateProductions`):
/// MobileLandSea → Stationary → MobileAir (GroundCover does not consume).
pub const CHARGE_LIST_ORDER: [UnitListKind; 3] = [
    UnitListKind::MobileLandSea,
    UnitListKind::Stationary,
    UnitListKind::MobileAir,
];

/// Build-site search radius (`TaskCreateBuilding::FindBuildSite`).
pub const SITE_SEARCH_RADIUS: i16 = 8;

// ---------------------------------------------------------------------------
// "Who builds what" tables
// ---------------------------------------------------------------------------

/// `Builder_GetBuildableUnits(team, builder)` — the list of available builds.
///
/// An empty slice — the builder cannot build (an ordinary unit).
pub fn buildable_units(builder: UnitType) -> &'static [UnitType] {
    for (unit_type, units) in BUILDABLE {
        if unit_type == builder {
            return units;
        }
    }

    &[]
}

/// `Builder_GetBuildableUnits(team, unit_type)` — whether the builder suffices.
pub fn can_build(builder: UnitType, unit_type: UnitType) -> bool {
    buildable_units(builder).contains(&unit_type)
}

/// `Builder_GetBuilderType(team, unit_type)` — who builds this type (the first in
/// the table, like the Lua `pairs` loop over the fixed table order).
pub fn builder_type(unit_type: UnitType) -> Option<UnitType> {
    for (builder, units) in BUILDABLE {
        if units.contains(&unit_type) {
            return Some(builder);
        }
    }

    None
}

/// A stationary producing factory (`SpawnNewUnit` branch `flags & STATIONARY`).
pub fn is_factory(unit_type: UnitType) -> bool {
    FACTORIES.contains(&unit_type)
}

/// A mobile builder (`CONSTRCT`/`ENGINEER`).
pub fn is_mobile_builder(unit_type: UnitType) -> bool {
    MOBILE_BUILDERS.contains(&unit_type)
}

/// Whether the type has a build queue (factory or mobile builder).
pub fn is_builder(unit_type: UnitType) -> bool {
    !buildable_units(unit_type).is_empty()
}

// ---------------------------------------------------------------------------
// Cost and time
// ---------------------------------------------------------------------------

/// `BuildMenu_GetTurnsToBuild(unit_type, team)` (`buildmenu.cpp:688`).
///
/// For a computer the difficulty multiplier applies (`opponent`), as in C++.
/// `quick_build` (training missions) is not modeled in the port.
pub(crate) fn turns_to_build(state: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
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

/// Cost of one construction turn — `Cargo_GetRawConsumptionRate`.
///
/// A mobile builder pays it in raw materials from its storage, a factory — in materials
/// of its complex; team gold is not spent (see the module header). For
/// `rate = 1` the total cost equals
/// `Cargo_GetRawConsumptionRate(builder, 1) * turns` — the C++ construction cost.
pub fn cost_per_turn(builder: UnitType, build_rate: i32) -> i32 {
    cargo_raw_consumption_rate(builder, build_rate)
}

/// The full construction cost at 1× rate — the C++ formula
/// `GetNormalRateBuildCost` for the unit that `builder` builds.
pub fn total_cost(builder: UnitType, turns: i32) -> i32 {
    cost_per_turn(builder, 1) * turns.max(0)
}

/// `UnitInfo::GetTurnsToBuild` (`unitinfo.cpp:3393`) — how many turns the
/// construction takes and how much raw material it charges in total at
/// `build_speed_multiplier` (`1/2/4`).
///
/// Returns `(turns, total consumption)` — exactly what C++
/// `AbstractBuildMenu::Draw` prints in the `Turns`/`Cost` columns.
///
/// * the base is `build_time` if the builder already builds this type (queue head,
///   `build_time > 0`), otherwise the type's current `ATTRIB_TURNS` (which already includes
///   the `SC_CLANS` clan modifiers and research — C++
///   `UnitsManager_GetCurrentUnitValues`);
/// * every turn the rate is limited by
///   [`max_possible_build_rate`] by the **remaining** turns and the builder's storage
///   (`INT16_MAX` for stationary ones): `min(build_speed_multiplier, limit)`;
/// * consumption — `Cargo_GetRawConsumptionRate(builder, effective_rate)`, the storage
///   decreases by it (at a mobile builder, like C++ `storage -= ...`).
pub(crate) fn turns_and_consumption(
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
/// `unit_type` — the **builder's** type (C++ passes `unit->GetUnitType()`), and
/// `storage` — its storage (for buildings C++ passes `INT16_MAX`).
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
// Errors and reports
// ---------------------------------------------------------------------------

/// Why a build order was rejected (`BuildMenu::Build` in C++).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    /// The unit is not a builder (no build queue).
    NotABuilder,
    /// `Builder_IsBuildable` is false: the builder cannot build this type.
    NotBuildable,
    /// The queue is empty (`FactoryBuildMenu::Build` without selected units).
    EmptyQueue,
    /// No site (`UnitsManager_IssueBuildOrder` == false).
    NoSite,
    /// The team was not found in the state.
    InvalidTeam,
    /// `BuildMenu::Build` refused: at `build_rate == 1` a mobile builder
    /// lacks raw material for the whole construction (`buildmenu.cpp:903-906`, message
    /// `_(c529)`). C++ does not issue the order — otherwise the build would stall forever.
    NotEnoughMaterials,
    /// The factory cannot pay for the first turn: the complex lacks power
    /// (`Cargo_GetPowerConsumptionRate` → `Cargo_ApplyUnitConsumption`,
    /// `production_manager.cpp:677-711`; the port refuses instead of `BUILD_ABORT`
    /// so as not to lose the queue).
    NotEnoughPower,
    /// The factory lacks fuel for its complex's power producers
    /// (`PowerOn`/`OptimizePowerProduction`, `production_manager.cpp:332-364`).
    NotEnoughFuel,
    /// The factory lacks workers (`TRAINHAL`/`GREENHSE`/`RESEARCH`:
    /// `Cargo_GetLifeConsumptionRate = 1`, `production_manager.cpp:366-380`).
    NotEnoughWorkers,
    /// The factory lacks gold (`COMMTWR`: `Cargo_GetGoldConsumptionRate = 5`).
    NotEnoughGold,
}

impl BuildError {
    /// Text for the UI/log.
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

/// `Build` result (C++ `BuildMenu_EventStrings_Available*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildReport {
    /// What will be built (queue head).
    pub unit_type: UnitType,
    /// Build turns (`GET_TURNS`).
    pub turns: i32,
    /// Rate (1/2/4).
    pub build_rate: u16,
    /// Cost of one turn in gold.
    pub cost_per_turn: i32,
    /// Site (for a mobile builder).
    pub site: Point,
}

/// Production event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionEventKind {
    /// The factory spawned a unit (`SpawnNewUnit`).
    Spawned,
    /// The mobile builder completed the building.
    BuildingDone,
    /// Construction paused: the mobile builder lacks **raw material** in
    /// storage (`storage < Cargo_GetRawConsumptionRate`) — C++
    /// `UnitInfo::UpdateProduction` goes negative in this case, while the port does not
    /// advance the construction (see [`UnitInfo::build_paused`]).
    PausedNoResources,
    /// Construction paused: the team lacks **materials** for
    /// the factory's current turn (`Cargo_GetRawConsumptionRate` → base materials).
    /// C++ `ProductionManager::ValidateIndustry` interrupts the construction
    /// (`ORDER_STATE_BUILD_ABORT`) and resumes it later; the port keeps the order
    /// in the queue on pause.
    PausedNoMaterials,
    /// Spawn deferred: there is no free adjacent cell (the unit stays in the queue).
    NoSpawnCell,
    /// The bulldozer cleared rubble (`UnitInfo::SpawnNewUnit`, BULLDOZR branch).
    RubbleCleared,
    /// Factory construction paused: the complex lacks **power**
    /// (`Cargo_GetPowerConsumptionRate` → `Cargo_ApplyUnitConsumption`;
    /// C++ `ValidateIndustry` turns the construction off, the port pauses it).
    PausedNoPower,
    /// Factory construction paused: there is not enough **fuel** for the
    /// power producers (`CalculatePowerConsumption`/`CheckPowerNeed`).
    PausedNoFuel,
    /// Factory construction paused: the complex lacks **workers**
    /// (`Cargo_GetLifeConsumptionRate`, `production_manager.cpp:366-380`).
    PausedNoWorkers,
    /// Factory construction paused: there is not enough **gold** (`COMMTWR`).
    PausedNoGold,
}

/// A production event — for the HUD log and QA assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionEvent {
    /// Builder/factory.
    pub builder: UnitId,
    /// What was being built.
    pub unit_type: UnitType,
    /// What appeared (if anything).
    pub spawned: Option<UnitId>,
    /// Step outcome.
    pub kind: ProductionEventKind,
}

/// Pause event for a resource from [`crate::power::Shortage`].
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
// Site and spawn cell
// ---------------------------------------------------------------------------

/// Site for a building/unit: all cells are inside the map, free and passable.
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

/// Construction size in cells (2×2 — `Footprint::Quad`, otherwise 1×1).
pub fn footprint(unit_type: UnitType) -> i16 {
    if unit_type.is_quad() {
        2
    } else {
        1
    }
}

/// `UnitsManager_IsAccessible` (`units_manager.cpp:4982`): whether the whole
/// footprint of the type is passable at `site` (2×2 for buildings, 1×1 for the rest).
///
/// Unlike [`find_build_site`] (AI search/fallback) there is **no**
/// requirement "no units on the cell": C++ checks only
/// `Access_IsAccessible(..., AccessModifier_SameClassBlocks)`, so a construction
/// can be laid out over own mobile units — C++ does not consider them
/// an obstacle to the building (`flags & STATIONARY` branch in `Access_IsAccessible`).
#[allow(dead_code)] // Oracle trace: C++ parity; callers use is_site_accessible_ignoring.
pub(crate) fn is_site_accessible(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    site: Point,
) -> bool {
    is_site_accessible_ignoring(state, team, unit_type, site, &[])
}

/// [`is_site_accessible`] with units C++ removed from the hash during
/// the check (`UnitsManager_MoveUnitAndParent`/`IssueBuildOrder` remove
/// the builder and the temporary tape, otherwise the 2×2 is "occupied" by the builder itself).
pub(crate) fn is_site_accessible_ignoring(
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

/// `UnitsManager_IssueBuildOrder` (`units_manager.cpp:4998`): the starting cell
/// of the site "tape" when a building is ordered in `BuildMenu`.
///
/// C++ iterates exactly four 2×2 anchors, all of them contain the builder's cell
/// (shift of at most one cell per axis):
/// `(x, y)`, `(x, y-1)`, `(x-1, y-1)`, `(x-1, y)` — the order matters because
/// the first passable anchor becomes the ghost's initial position.
pub(crate) fn initial_build_site(
    state: &GameState,
    team: TeamId,
    unit_type: UnitType,
    builder_grid: Point,
    ignored: &[UnitId],
) -> Option<Point> {
    if footprint(unit_type) == 1 {
        // For non-buildings C++ checks only the builder's own cell.
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

/// Site anchor for the cursor in site-selection mode — a port of the cursor part of
/// `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5845`) for the
/// constructor.
///
/// C++ binds the 2×2 to the builder: cursor to the right of the builder — anchor
/// `builder.x`, to the left — `builder.x - 1`; exactly on the builder's column the anchor
/// stays as before (hence `previous` is passed). On each axis
/// independently; the site always covers the builder's cell.
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

/// The cursor part of `GameManager_GetBuilderUnitCursor` for `ENGINEER`
/// (`game_manager.cpp:5854-5909`).
///
/// The engineer places a **1×1** tape (`SMLTAPE`) exactly on the cell under the cursor:
/// C++ does not bind it to the builder (`target_grid_x = grid_x`,
/// `target_grid_y = grid_y`), but allows only cells **in its own row or
/// column** (`grid_x == unit_grid_x || grid_y == unit_grid_y`), otherwise
/// `CURSOR_UNIT_NO_GO`. Passability is checked by `UnitsManager_MoveUnitAndParent`
/// (in the port — [`is_site_accessible_ignoring`] without the builder and tape).
pub fn engineer_site_covers(builder_grid: Point, cursor: Point) -> bool {
    cursor.x == builder_grid.x || cursor.y == builder_grid.y
}

/// Whether the site of `unit_type` with `anchor` covers the `cursor` cell
/// (`UnitsManager_MoveUnitAndParent` + the hit check in `GameManager_GetBuilderUnitCursor`).
pub fn site_covers(unit_type: UnitType, anchor: Point, cursor: Point) -> bool {
    let size = footprint(unit_type);

    cursor.x >= anchor.x
        && cursor.x < anchor.x + size
        && cursor.y >= anchor.y
        && cursor.y < anchor.y + size
}

/// `TaskCreateBuilding::FindBuildSite` — the nearest free site.
///
/// The traversal order is deterministic: by ascending distance, then `y`,
/// then `x` (no hash tables), so two runs match.
pub(crate) fn find_build_site(
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
// Construction site (C++ `DeployConstructionSiteUtilities` + `PrepareConstructionSite`)
// ---------------------------------------------------------------------------

/// `DeployConstructionSiteUtilities` (`unitinfo.cpp:3802`): the "tape+cones" pair.
///
/// A `Footprint::Quad` type gets the large decals `LRGTAPE`/`LRGCONES` (2×2),
/// everything else — the small `SMLTAPE`/`SMLCONES` (1×1).
pub fn construction_utility_types(unit_type: UnitType) -> (UnitType, UnitType) {
    if unit_type.is_quad() {
        (UnitType::LargeTape, UnitType::LargeCones)
    } else {
        (UnitType::SmallTape, UnitType::SmallCones)
    }
}

/// `PrepareConstructionSite` (`unitinfo.cpp:4122`): the slab under a construction.
///
/// `LRGSLAB` for `Footprint::Quad`, `SMLSLAB` for the other types; created
/// only with the `REQUIRES_SLAB` flag.
pub fn construction_slab_type(unit_type: UnitType) -> UnitType {
    if unit_type.is_quad() {
        UnitType::LargeSlab
    } else {
        UnitType::SmallSlab
    }
}

/// Slab `image_count` — the second byte of `D_LRGSLA`/`D_DEFALT` (`MAX.RES`).
fn slab_image_count(unit_type: UnitType) -> i32 {
    if unit_type.is_quad() {
        LARGE_SLAB_IMAGE_COUNT
    } else {
        SMALL_SLAB_IMAGE_COUNT
    }
}

/// What the construction site placed (for tests/QA; the ids allow checking cleanup).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstructionSite {
    /// Source cell of the site (`DeployConstructionSiteUtilities`).
    pub site: Point,
    /// `LRGTAPE`/`SMLTAPE` — parent builder, stands until the construction ends.
    pub tape: UnitId,
    /// `LRGCONES`/`SMLCONES`; `None` on water/coast (C++ does not place cones).
    pub cones: Option<UnitId>,
    /// `LRGSLAB`/`SMLSLAB` — the slab; `None` if the type is not `REQUIRES_SLAB`.
    pub slab: Option<UnitId>,
}

/// The team's site tape on a cell (`Access_GetConstructionUtility`,
/// `access.cpp:1773`).
pub(crate) fn construction_utility(state: &GameState, team: TeamId, site: Point) -> Option<UnitId> {
    state.units_at(site).iter().copied().find(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.team == team && matches!(unit.unit_type, UnitType::LargeTape | UnitType::SmallTape)
        })
    })
}

/// Place a construction site — a port of `DeployConstructionSiteUtilities` +
/// `PrepareConstructionSite` at `site`.
///
/// C++ layout (`unitinfo.cpp:3802/4122`):
/// * cones `LRGCONES`/`SMLCONES` — on the site cell, angle `0`; on
///   water/coast (`SURFACE_TYPE_WATER|COAST`) they are **not placed**;
/// * tape `LRGTAPE`/`SMLTAPE` — there too, angle `0` on land and `1` on water,
///   parent — the builder;
/// * slab `LRGSLAB`/`SMLSLAB` (if `REQUIRES_SLAB`) — there too, frame
///   `Randomizer_Generate(image_count)` (5 for the large, 1 for the small), then
///   `scaler_adjust = 4` + `ScaleUnit(EXPAND)` — the growth animation.
///
/// The function is idempotent per cell: `StartBuilding` is called once in C++,
/// but the port also calls it from `ProductionManager::commit` (the order applies immediately,
/// not on the next frame) and from the order machine — a second site is not needed.
pub(crate) fn deploy_construction_site(
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

            // C++ `PrepareConstructionSite`: `scaler_adjust = 4` and
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

/// New site decals — `ORDER_AWAIT`/`ORDER_STATE_EXECUTING_ORDER`, as in
/// `UnitInfo::UnitInfo` (`unitinfo.cpp:174`); otherwise `App::draw_units` would skip
/// them as `ORDER_IDLE` and the site would be invisible.
fn set_site_unit_orders(state: &mut GameState, id: UnitId, parent: Option<UnitId>) {
    if let Some(unit) = state.unit_mut(id) {
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.prior_orders = UnitOrder::Await;
        unit.prior_state = UnitOrderState::ExecutingOrder;
        unit.parent_unit = parent;
    }
}

/// The builder completed the building and **waits for an order to leave the site**.
///
/// C++ `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3536-3592`, not a stationary
/// builder): the building is placed on the tape cell in `ORDER_IDLE`/
/// `ORDER_STATE_BUILDING_READY` and is not drawn (`UnitInfoGroup::IsRelevant`),
/// the builder gets `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` and stays on
/// the site; mutual parents (`this->SetParent(&*utility_unit)` +
/// `utility_unit->SetParent(this)`). The exit is initiated by the **player** — a cell click
/// (`game_manager.cpp:6879-6890`: for the selected builder the state
/// `ORDER_STATE_UNIT_READY` → `ORDER_ACTIVATE`/`ORDER_STATE_IN_TRANSITION`), by
/// the AI — `TaskCreateBuilding::Activate`/`TaskActivate` (`taskactivate.cpp:104`).
///
/// Returns the parent building id while the builder stands on the site.
pub(crate) fn builder_awaiting_exit(state: &GameState, builder_id: UnitId) -> Option<UnitId> {
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

/// The cell the builder leaves to from a completed site — a port of the search
/// `TaskActivate::Activate` (`taskactivate.cpp:104-167`).
///
/// C++ iterates cells around the parent building (2×2): start at
/// `(grid_x - 1, grid_y + unit_size)`, then four directions
/// (`N, E, S, W` — `direction += 2`) for `unit_size + 1` steps, cumulatively —
/// the first passable cell becomes the target. This is the **AI** target (`TaskActivate`);
/// the player sets any cell by clicking (`move_to` = click), so for them
/// `builder_exit_cell` is not used.
pub(crate) fn builder_exit_cell(
    state: &GameState,
    builder: &UnitInfo,
    building: &UnitInfo,
) -> Option<Point> {
    crate::unit_order::activation_exit_cell(state, builder, building)
}

/// A 2×2 (`Footprint::Quad`) or 1×1 rectangle in cells —
/// `UnitInfo::GetBounds` + `Access_IsInsideBounds` (`access.cpp`).
pub fn unit_bounds_contains(unit: &UnitInfo, grid: Point) -> bool {
    let size: i16 = if unit.is_quad() { 2 } else { 1 };

    grid.x >= unit.grid.x
        && grid.y >= unit.grid.y
        && grid.x < unit.grid.x.wrapping_add(size)
        && grid.y < unit.grid.y.wrapping_add(size)
}

/// The cell from which the factory's ready unit **exits** it — a port of
/// `TaskActivate::Activate` (`taskactivate.cpp:104-167`) for the
/// `ORDER_ACTIVATE`.
///
/// C++ iterates cells around the container, starting from
/// `(grid_x - 1, grid_y + unit_size)`, then four directions (`N, E, S, W` —
/// `direction += 2`) for `unit_size + 1` steps, cumulatively; the first passable
/// cell becomes the target. `unit_size` — the **container** size (2 for a building),
/// passability is checked for the **exiting** type.
///
/// Returns `None` if there is no passable cell nearby — then C++
/// (`UnitsManager_ActivateUnit`, `units_manager.cpp:3832-3836`) does not
/// execute the order and returns the previous one.
pub(crate) fn factory_exit_cell(
    state: &GameState,
    container: &UnitInfo,
    client: &UnitInfo,
) -> Option<Point> {
    crate::unit_order::activation_exit_cell(state, client, container)
}

// ---------------------------------------------------------------------------
// Complex materials (C++ `Complex::GetCargoInfo` + `Complex::Transfer`)
// ---------------------------------------------------------------------------

/// Complex materials — `Complex::GetCargoInfo(materials, capacity)` →
/// `materials.raw` (`complex.cpp:136-147`).
///
/// The sum of `storage` of **all connected** complex buildings with cargo type
/// `CARGO_TYPE_RAW` (`victory::cargo_type_of`): mines, `ADUMP`, greenhouses and
/// barracks. This "common pool" is what the factory spends
/// (`UnitInfo::StartBuilding`, `unitinfo.cpp:2757`).
pub(crate) fn complex_raw_materials(
    state: &GameState,
    team: TeamId,
    complex: crate::team::ComplexId,
) -> i32 {
    crate::alloc::get_cargo_info(state, team, complex).0.raw
}

/// Charges `amount` of **complex** materials: a port of `Complex::Transfer`
/// (`complex.cpp:178-203`) for a negative amount.
///
/// C++ distributes the negative amount over member storages in
/// `UnitsManager_StationaryUnits` order (`Complex::TransferCargo`), so the charge
/// is deterministic. Returns `false` (having charged nothing) if there are fewer
/// materials than the rate — the port does not allow C++ debt (`storage`/`complex->material`
/// going negative) (a QA requirement), and the factory pauses instead.
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

/// Team production manager — a port of the `ProductionManager`/`BuildMenu` part
/// `production_manager.cpp` + `unitinfo.cpp` (`BuildOrder`/`SpawnNewUnit`).
///
/// Holds `&mut GameState`: all changes (queue, `build_time`, unit
/// spawning) apply to the state immediately, so the network/local loop sees
/// the same result.
pub struct ProductionManager<'a> {
    state: &'a mut GameState,
    team: TeamId,
}

impl<'a> ProductionManager<'a> {
    /// A manager for the team.
    pub(crate) fn new(state: &'a mut GameState, team: TeamId) -> Self {
        Self { state, team }
    }

    /// The manager's team.
    pub const fn team(&self) -> TeamId {
        self.team
    }

    /// `Builder_GetBuildableUnits` for a builder.
    pub fn buildable(&self, builder: UnitType) -> &'static [UnitType] {
        buildable_units(builder)
    }

    /// `BuildMenu_GetTurnsToBuild` for a type.
    pub fn turns_to_build(&self, unit_type: UnitType) -> i32 {
        turns_to_build(self.state, self.team, unit_type)
    }

    /// The unit's current build queue (a copy).
    pub fn queue(&self, unit_id: UnitId) -> Vec<UnitType> {
        self.state
            .unit(unit_id)
            .map(|unit| unit.build_list.clone())
            .unwrap_or_default()
    }

    /// `FactoryBuildMenu::AddSelection` — add a type to the end of the queue.
    ///
    /// For a mobile builder the queue is exactly one element (`Build` replaces
    /// the selection, like `MobileBuildMenu::Build`: `build_list.Clear(); PushBack`).
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

    /// `FactoryBuildMenu::RemoveSelection` — remove the last selected type.
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

    /// `SetRepeatBuildState` (the `Repeat` button).
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

    /// Repeat state.
    pub fn repeat(&self, unit_id: UnitId) -> bool {
        self.state
            .unit(unit_id)
            .map(|unit| unit.repeat_build)
            .unwrap_or(false)
    }

    /// Selected build rate (`GetBuildRate`).
    pub fn build_rate(&self, unit_id: UnitId) -> u16 {
        self.state
            .unit(unit_id)
            .map(|unit| unit.build_rate)
            .unwrap_or(1)
    }

    /// `SetBuildRate` with a clamp by `BuildMenu_GetMaxPossibleBuildRate`.
    ///
    /// Before `Build` (`build_time == 0`) the rate is limited by the queue head's
    /// time, otherwise the 2×/4× buttons could not be pressed before the order.
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

    /// `BuildMenu::Build` — fix the queue head and issue `ORDER_BUILD`.
    ///
    /// For a mobile builder the site is searched by [`find_build_site`]
    /// (`UnitsManager_IssueBuildOrder`), for a factory no cell is needed.
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

        // `MobileBuildMenu::Build` (`buildmenu.cpp:903-906`): at 1× rate
        // all raw material must be on hand — `storage >= rate(1) * turns`, otherwise
        // the `_(c529)` message and the order is not issued. C++ computes by
        // `Cargo_GetRawConsumptionRate(unit->GetUnitType(), 1)` and `turns_to_build`
        // from `BuildMenu_GetTurnsToBuild`.
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

        // `ORDER_BUILD` with a target: the build cell in `order_data.move_to`.
        let mut data = self.state.order_data(unit_id);
        data.move_to = site;
        self.state.set_order_data(unit_id, data);

        // `UnitInfo::StartBuilding` (`unitinfo.cpp:2751-2794`) would immediately charge
        // `complex->material/power/workers` and call
        // `GameManager_OptimizeProduction` (`game_manager.cpp:2555-2584`):
        // on a shortage `ValidateIndustry` (`production_manager.cpp:677-711`)
        // interrupts the order. The port does not lose the queue: it checks the balance before starting and
        // returns `BuildError`, leaving `build_list` untouched.
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

        // In C++ `UnitInfo::StartBuilding` is called synchronously from `Build`:
        // `DeployConstructionSiteUtilities` + `PrepareConstructionSite`.
        // The site is placed immediately so it is visible on the order frame, and
        // the order machine (`start_building`) does not duplicate it.
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

    /// `UnitInfo::CancelBuilding` — abort the construction and clear the queue.
    ///
    /// Charges for past turns are not refunded (in C++ the material is also
    /// not refunded), the queue is cleared.
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

    /// Production turn: power optimization → per-turn charging →
    /// `SpawnNewUnits` (C++ order).
    ///
    /// Returns events (spawns/stops) in C++ order — for the HUD log and
    /// tests.
    pub fn progress(&mut self) -> Vec<ProductionEvent> {
        let (mut events, paused) = self.charge_units();
        events.extend(self.spawn_units(&paused));

        // `GameManager_ProgressBuildState` (`game_manager.cpp:8183-8204`):
        // after `SpawnNewUnits` C++ recomputes the complexes once more and calls
        // `ProductionManager_OptimizeProduction`. Factories in `UNIT_READY` no longer
        // consume power, so surplus producers are turned off.
        let team = self.team;
        let complexes = self.team_complexes();
        for complex in complexes {
            let _ = crate::power::optimize_complex(self.state, team, complex, None, None, false);
        }

        events
    }

    /// Team complexes in `TeamUnits::complexes` order (ascending id).
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
    /// (`UnitInfo::UpdateProduction`): charging the per-turn rate.
    ///
    /// The mobile builder has already paid in raw material from its storage in
    /// [`crate::game::GameState::update_production`]; the factory pays here
    /// **with its complex's materials** (`Cargo_GetRawConsumptionRate` →
    /// [`complex_raw_materials`]) — like C++ `UnitInfo::StartBuilding`/
    /// `Cargo_GetNetProduction`.
    ///
    /// Before charging, every complex goes through
    /// [`crate::power::optimize_complex`] (C++
    /// `TeamUnits::OptimizeComplexes`): power producers are turned on,
    /// consumers are turned off on a deficit, constructions without power/workers
    /// are paused.
    ///
    /// Returns events and the set of constructions that **do not
    /// advance** this turn due to a resource shortage (C++ `ValidateIndustry` interrupts
    /// the construction; the port pauses it — see the module header).
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
        // the pause and producer-mode decision is made before charging.
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

                // C++ charges only during `ORDER_BUILD`
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

                // `max_allowed_build_rate`: buildings are not limited by storage.
                let storage = if is_factory(unit.unit_type) {
                    i32::from(i16::MAX)
                } else {
                    i32::from(unit.storage)
                };
                let max_rate =
                    max_possible_build_rate(unit.unit_type, i32::from(unit.build_time), storage);
                // For a mobile builder `UnitInfo::UpdateProduction` has already
                // this turn cut `build_rate` by storage **and** charged raw material
                // exactly by it; a repeated computation over the reduced `storage` would cut
                // the rate a second time, and the port's "gold" would stop
                // matching the consumed raw material (the `Cost` column).
                let mobile_land = unit.is_ground() || unit.is_amphibious();
                let build_rate = if mobile_land {
                    i32::from(unit.build_rate.max(1))
                } else {
                    i32::from(unit.build_rate.max(1))
                        .min(max_rate.max(1))
                        .max(1)
                };

                // A mobile builder pays **in raw material from its storage** (C++
                // `Cargo_GetRawConsumptionRate` → `UnitInfo::UpdateProduction`),
                // rather than team gold; the payment fact is already computed there
                // (`build_paused`). A turn without payment does not advance the construction.
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

                // A factory the optimizer has already paused (no
                // power/fuel/workers/materials) does not pay this turn and
                // does not advance (C++ `ValidateIndustry` → `BUILD_ABORT`; the port —
                // a pause with the queue preserved).
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
                // `Cargo_GetNetProduction` (`cargo.cpp:88-95`) subtracts the rate
                // every turn while `ORDER_BUILD` and the state is not `UNIT_READY`.
                // Materials — of the **factory's complex** ([`complex_raw_materials`]),
                // not of the whole team: a factory without a connected storage
                // (`ADUMP`/mine) stands still, as in the oracle.
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

    /// `GameManager_SpawnNewUnits` for all four lists in C++ order.
    ///
    /// `paused` — constructions not paid for this turn: they do not
    /// advance and do not spawn units.
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

                // `GameManager_SpawnNewUnits` (`game_manager.cpp:8078`) advances
                // `build_time` for the bulldozer too (`ORDER_CLEAR`, empty queue):
                // at zero `UnitInfo::SpawnNewUnit` is called, which demolishes
                // the rubble and takes its cargo.
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

    /// `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3441-3487`), the `BULLDOZR` branch:
    /// the rubble is cleared.
    ///
    /// C++ demolishes `SMLRUBLE`/`LRGRUBLE` on the bulldozer's cell (for the large one also
    /// `+1` on both axes), takes their `storage` (the rubble cargo) for itself, clamps to
    /// `ATTRIB_STORAGE`, removes the cell utilities and returns to
    /// `ORDER_AWAIT`/`EXECUTING_ORDER`. For building rubble additionally
    /// `AWAIT_TAPE_POSITIONING`/`DEINIT` is started — the port has no clearing tape
    /// (`ProcessOrderClear` without `StartClearing`), so the step is skipped.
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

    /// `UnitInfo::SpawnNewUnit` — spawning a unit/completing a building.
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

        // `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3441-3487`), BULLDOZR branch:
        // rubble clearing is complete — the rubble is demolished, its cargo goes
        // to the bulldozer.
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
        // `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3489-3494`): a stationary
        // factory places the ready unit **on its own cell** —
        // `UnitsManager_DeployUnit(GetConstructedUnitType(), team, GetComplex(),
        // position_x = this->grid_x, position_y = this->grid_y, …)`. A free
        // adjacent cell is not searched: the unit is "inside" the factory until the player's order.
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

        // The new unit belongs to the builder's complex (`SpawnNewUnit`:
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

        // Queue accounting is **per branch**: C++ `UnitInfo::SpawnNewUnit`
        // (`unitinfo.cpp:3489-3520`) removes the head only in the stationary
        // (factory) branch; a mobile builder keeps its queue when it has a path
        // (`:3536-3538` `if (!path) build_list.Clear()`), otherwise it is cleared.
        let mut builder = builder;

        let kind = if factory {
            ProductionEventKind::Spawned
        } else {
            ProductionEventKind::BuildingDone
        };

        if factory {
            builder.build_list.remove(0);

            if builder.repeat_build {
                builder.build_list.push(unit_type);
            }
            // `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3489-3520`), the
            // `flags & STATIONARY` branch — the built unit stays **in the factory**:
            // * `factory_unit->SetParent(this)` — the unit's parent is the factory;
            // * `factory_unit->SetOrder(ORDER_IDLE)`;
            //   `factory_unit->SetOrderState(ORDER_STATE_STORE)` — the unit is "in
            //   the hold" (`GetTeamUnit` does not select such units, `Access_IsAccessible`
            //   does not count it, the `IsRelevant` render does not draw it);
            // * `factory_unit->scaler_adjust = 4` — compressed (1/32 size, beyond
            //   the drawing bounds), while it stands in the factory;
            // * `SetParent(factory_unit.Get())` — the **factory's** parent is now
            //   the ready unit: this is exactly what `UnitsManager_ActivateUnit` reads
            //   (`units_manager.cpp:3827`: `client = unit->GetParent()`) when
            //   the player orders the unit out;
            // * `storage = 1` — the counter of ready units in the factory;
            // * `orders = ORDER_BUILD`, `state = ORDER_STATE_UNIT_READY` —
            //   the factory waits for the exit (the next order starts only in
            //   `UnitsManager_ActivateUnit`, `units_manager.cpp:3843-3848`, not
            //   here: auto-starting the queue was a port divergence).
            if let Some(mut unit) = self.state.unit(spawned).cloned() {
                unit.parent_unit = Some(id);
                // C++: `SetOrder(ORDER_IDLE)` + `SetOrderState(STORE)` — the order
                // is set directly, `prior` stays from `DeployUnit`
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
            // `UnitInfo::SpawnNewUnit` (mobile builder) — C++ order:
            //  1. the building is placed on the tape cell but remains
            //     `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY` and is **not drawn**
            //     (`UnitInfoGroup::IsRelevant`) while the builder is on the site;
            //  2. the builder gets `ORDER_BUILD`/`ORDER_STATE_UNIT_READY`
            //     (`this->SetParent(&*utility_unit)` — mutual parent) and
            //     keeps standing (`ProcessOrderBuild`/`UNIT_READY` only
            //     animates) — **no automatic exit**;
            //  3. the exit is ordered by the player clicking (`ORDER_ACTIVATE` →
            //     `PositionInTape` DEINIT/LEAVE → `ActivateEngineer` →
            //     `ORDER_MOVE`, see `hud::build_builder_exit_order`) or by the AI
            //     (`TaskActivate` → `unit_order::issue_builder_exit`);
            //  4. `UnitsManager_BuildingReady` removes the tape and cones, the slab
            //     (`*SLAB`) remains as the foundation, the building unfolds
            //     (`ScaleUnit(EXPAND)`) — only after the builder leaves.
            if let Some(mut unit) = self.state.unit(spawned).cloned() {
                unit.parent_unit = Some(id);
                // `UnitsManager_SetNewOrderInt(&*utility_unit, ORDER_IDLE,
                // ORDER_STATE_BUILDING_READY)` (`unitinfo.cpp:3581`): unlike
                // `SetOrder`, `SetNewOrderInt` preserves **prior**
                // (`units_manager.cpp:1970-1971`). This matters for units
                // that `UnitsManager_DeployUnit` puts into a special order:
                // `RADAR`, turrets and mines are born in `ORDER_SENTRY`
                // (`units_manager.cpp:2412-2421`), and `RestoreOrders`
                // (`unitinfo.cpp:3783`) after completion returns exactly
                // `ORDER_SENTRY` — otherwise an ANIMATED radar (`ProcessOrderSentry`)
                // would freeze in `ORDER_AWAIT` and not cycle through 16 frames.
                crate::unit_order::set_new_order_int(
                    &mut unit,
                    UnitOrder::Idle,
                    UnitOrderState::BuildingReady,
                );
                self.state.write_unit(spawned, unit);
            }

            // `this->SetParent(&*utility_unit)`: the builder is "inside" the building until
            // leaving the site. `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3536-3538`):
            // `if (!path) build_list.Clear();` — a builder that was ordered a **Path**
            // (`GameManager_PathBuild`, `game_manager.cpp:4583-4587`) keeps the queue
            // head so that `UnitsManager_BuildNext` (`units_manager.cpp:3807`) can
            // repeat the same type on the next cell of the route. The queue is
            // cleared only when there is no path (a single building order).
            builder.parent_unit = Some(spawned);
            builder.orders = UnitOrder::Build;
            builder.state = UnitOrderState::UnitReady;
            builder.build_time = 0;

            if builder.path.is_none() {
                builder.build_list.clear();
            }
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
// Free functions for GameState/AI
// ---------------------------------------------------------------------------

/// `GameManager_ProgressBuildState(team)` — the team's production turn.
///
/// C++ order 1:1: `ManageEconomy` (charging) → `SpawnNewUnits` over the lists
/// `GroundCover → MobileLandSea → Stationary → MobileAir`.
pub(crate) fn progress_build_state(state: &mut GameState, team: TeamId) -> Vec<ProductionEvent> {
    ProductionManager::new(state, team).progress()
}

/// Whether there are enough materials for the current construction turn (UI/AI check).
///
/// For a mobile builder the payment is raw material in its storage
/// (`Cargo_GetRawConsumptionRate` → `UnitInfo::UpdateProduction`), for
/// factories — the materials of **their own complex** ([`complex_raw_materials`],
/// C++ `complex->material`).
#[allow(dead_code)] // Oracle trace: C++ semantics documented; not wired into UI/AI yet.
pub(crate) fn can_afford_turn(state: &GameState, team: TeamId, unit_id: UnitId) -> bool {
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

/// `UnitsManager_BuildNext(unit)` (`units_manager.cpp:3807-3824`) — the next
/// building of a **Path** construction (`GameManager_PathBuild`).
///
/// Called for a mobile builder in `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` that
/// has a path (see [`crate::unit_order`]). C++:
///
/// ```text
/// unit_type = *build_list[0];
/// turns = BuildMenu_GetTurnsToBuild(unit_type, team);
/// if (storage >= turns * Cargo_GetRawConsumptionRate(unit_type, 1) &&
///     (path->GetEndX() != grid_x || path->GetEndY() != grid_y)) {
///     remove_road = type != CNCT_4W && type != ROAD && type != BRIDGE && type != WTRPLTFM;
///     Access_DestroyUtilities(grid_x, grid_y, false, false, remove_road, remove_road);
///     state = ORDER_STATE_IN_PROGRESS;          // move to the next cell
/// } else {
///     ClearBuildListAndPath();                  // no material / route finished
/// }
/// ```
///
/// Returns `true` when the builder continues along the path (the order machine
/// then walks it and [`crate::unit_order`] starts the next site on the next
/// cell), `false` when the queue/path were cleared.
pub(crate) fn build_next(state: &mut GameState, builder_id: UnitId) -> bool {
    let Some(builder) = state.unit(builder_id).cloned() else {
        return false;
    };
    let Some(&unit_type) = builder.build_list.first() else {
        return false;
    };

    let turns = turns_to_build(state, builder.team, unit_type);
    let rate = cargo_raw_consumption_rate(builder.unit_type, 1);
    let path_end = builder.path.as_ref().map(crate::unit::UnitPath::end);
    let has_materials = i32::from(builder.storage) >= turns.saturating_mul(rate).max(0);
    let route_continues = path_end.is_some_and(|(x, y)| x != builder.grid.x || y != builder.grid.y);

    if has_materials && route_continues {
        // `remove_road`/`remove_connectors` are the same expression in the oracle:
        // a connector/road/bridge/platform is not removed under the next site.
        let remove = !matches!(
            unit_type,
            UnitType::Connector | UnitType::Road | UnitType::Bridge | UnitType::WaterPlatform
        );

        crate::access::destroy_utilities(state, builder.grid, false, false, remove, remove);

        if let Some(mut unit) = state.unit(builder_id).cloned() {
            unit.state = UnitOrderState::InProgress;
            state.write_unit(builder_id, unit);
        }

        true
    } else {
        // `UnitInfo::ClearBuildListAndPath` (`unitinfo.cpp:1681`).
        let mut unit = builder;
        unit.path = None;

        if unit.parent_unit.is_some() {
            unit.orders = UnitOrder::Build;
            unit.state = UnitOrderState::UnitReady;
        } else {
            unit.orders = UnitOrder::Await;
            unit.state = UnitOrderState::ExecutingOrder;
        }

        unit.build_list.clear();
        state.write_unit(builder_id, unit);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classification::Footprint;

    #[test]
    fn turret_layout_is_one_by_one() {
        // A turret is a building semantically, but `Footprint::Single`: the site,
        // tape/cones and slab are small (1×1), the size is one cell.
        for turret in [
            UnitType::GunTurret,
            UnitType::Artillery,
            UnitType::AntiAir,
            UnitType::MissileLauncher,
        ] {
            assert_eq!(turret.footprint(), Footprint::Single, "{turret:?}");
            assert!(turret.is_building(), "{turret:?}");
            assert_eq!(footprint(turret), 1, "{turret:?}");
            assert_eq!(
                construction_utility_types(turret),
                (UnitType::SmallTape, UnitType::SmallCones),
                "{turret:?}"
            );
            assert_eq!(
                construction_slab_type(turret),
                UnitType::SmallSlab,
                "{turret:?}"
            );
        }

        // For contrast: a 2×2 building — large utilities and slab.
        assert_eq!(footprint(UnitType::MiningStation), 2);
        assert_eq!(
            construction_utility_types(UnitType::MiningStation),
            (UnitType::LargeTape, UnitType::LargeCones)
        );
        assert_eq!(
            construction_slab_type(UnitType::MiningStation),
            UnitType::LargeSlab
        );
    }

    #[test]
    fn turret_bounds_cover_exactly_one_cell() {
        let mut turret = UnitInfo::new(UnitType::GunTurret, TeamId::RED, UnitId::NO_ID);
        turret.flags = crate::access::unit_type_flags(UnitType::GunTurret);
        turret.grid = Point::new(10, 10);
        let mut mine = UnitInfo::new(UnitType::MiningStation, TeamId::RED, UnitId::NO_ID);
        mine.flags = crate::access::unit_type_flags(UnitType::MiningStation);
        mine.grid = Point::new(10, 10);

        assert!(unit_bounds_contains(&turret, Point::new(10, 10)));
        assert!(!unit_bounds_contains(&turret, Point::new(11, 10)));
        assert!(!unit_bounds_contains(&turret, Point::new(10, 11)));

        assert!(unit_bounds_contains(&mine, Point::new(10, 10)));
        assert!(unit_bounds_contains(&mine, Point::new(11, 11)));
    }

    #[test]
    fn builder_tables_are_consistent() {
        for (builder, units) in BUILDABLE {
            assert!(!units.is_empty(), "{builder:?} row is empty");
            assert!(is_builder(builder), "{builder:?} row must be a builder");
            assert_eq!(buildable_units(builder), units);

            for unit_type in units {
                assert!(can_build(builder, *unit_type));
                // No type is listed in two rows: the first match is the only match.
                assert_eq!(
                    builder_type(*unit_type),
                    Some(builder),
                    "{unit_type:?} builder"
                );
            }
        }

        // An ordinary unit cannot build anything.
        assert!(buildable_units(UnitType::Tank).is_empty());
        assert!(!is_builder(UnitType::Tank));
        assert!(!is_mobile_builder(UnitType::Tank));
        assert!(!can_build(UnitType::Tank, UnitType::Scout));
        assert_eq!(builder_type(UnitType::Tank), Some(UnitType::HeavyVehiclePlant));

        // The factory list is exactly the stationary producers.
        for factory in FACTORIES {
            assert!(is_factory(factory), "{factory:?}");
            assert!(is_builder(factory), "{factory:?}");
            assert!(!is_mobile_builder(factory), "{factory:?}");
        }
        for builder in MOBILE_BUILDERS {
            assert!(is_mobile_builder(builder), "{builder:?}");
            assert!(is_builder(builder), "{builder:?}");
            assert!(!is_factory(builder), "{builder:?}");
        }
    }

    #[test]
    fn build_error_messages_are_complete_and_distinct() {
        let errors = [
            BuildError::NotABuilder,
            BuildError::NotBuildable,
            BuildError::EmptyQueue,
            BuildError::NoSite,
            BuildError::InvalidTeam,
            BuildError::NotEnoughMaterials,
            BuildError::NotEnoughPower,
            BuildError::NotEnoughFuel,
            BuildError::NotEnoughWorkers,
            BuildError::NotEnoughGold,
        ];

        let messages: Vec<&str> = errors.iter().map(|error| error.message()).collect();
        for (index, message) in messages.iter().enumerate() {
            assert!(!message.is_empty(), "{:?} message", errors[index]);
            for other in &messages[index + 1..] {
                assert_ne!(message, other, "duplicate message {message:?}");
            }
        }
    }

    #[test]
    fn cost_helpers_follow_the_cpp_rates() {
        // `Cargo_GetRawConsumptionRate` (cargo.cpp:140): CONSTRCT/ENGINEER 2 per turn,
        // TRAINHAL 1, factories 3; the rate multiplier maps 2× → 4 and 4× → 12.
        assert_eq!(cost_per_turn(UnitType::Constructor, 1), 2);
        assert_eq!(cost_per_turn(UnitType::Engineer, 1), 2);
        assert_eq!(cost_per_turn(UnitType::TrainingHall, 1), 1);
        assert_eq!(cost_per_turn(UnitType::LightVehiclePlant, 1), 3);
        assert_eq!(cost_per_turn(UnitType::HeavyVehiclePlant, 1), 3);
        assert_eq!(cost_per_turn(UnitType::AirUnitsPlant, 1), 3);
        assert_eq!(cost_per_turn(UnitType::Shipyard, 1), 3);
        assert_eq!(cost_per_turn(UnitType::TrainingHall, 2), 4);
        assert_eq!(cost_per_turn(UnitType::TrainingHall, 4), 12);
        assert_eq!(cost_per_turn(UnitType::Tank, 1), 0, "a unit is not a builder");

        assert_eq!(total_cost(UnitType::Constructor, 10), 20);
        assert_eq!(total_cost(UnitType::TrainingHall, 3), 3);
        assert_eq!(total_cost(UnitType::Constructor, 0), 0);
        assert_eq!(total_cost(UnitType::Constructor, -5), 0);
    }

    #[test]
    fn max_possible_build_rate_boundaries() {
        // `build_time <= 1` always returns 1 (`buildmenu.cpp:733`).
        assert_eq!(max_possible_build_rate(UnitType::Constructor, 0, 100), 1);
        assert_eq!(max_possible_build_rate(UnitType::Constructor, 1, 100), 1);
        // An empty storage cannot pay for a 2× pair.
        assert_eq!(max_possible_build_rate(UnitType::Constructor, 2, 0), 1);
        assert_eq!(max_possible_build_rate(UnitType::Constructor, 2, 100), 2);
        // A stationary builder is passed `INT16_MAX` for storage, so the cap does not apply.
        assert_eq!(max_possible_build_rate(UnitType::TrainingHall, 10, 0), 4);
        assert_eq!(max_possible_build_rate(UnitType::Shipyard, 10, 0), 4);
        // Air units are not ground/amphibious: storage is ignored as well.
        assert_eq!(max_possible_build_rate(UnitType::AirUnitsPlant, 10, 0), 4);
    }

    #[test]
    fn pause_event_kind_covers_every_shortage() {
        use crate::power::Shortage;

        assert_eq!(pause_event_kind(Shortage::Raw), ProductionEventKind::PausedNoMaterials);
        assert_eq!(pause_event_kind(Shortage::Power), ProductionEventKind::PausedNoPower);
        assert_eq!(pause_event_kind(Shortage::Workers), ProductionEventKind::PausedNoWorkers);
        assert_eq!(pause_event_kind(Shortage::Fuel), ProductionEventKind::PausedNoFuel);
        assert_eq!(pause_event_kind(Shortage::Gold), ProductionEventKind::PausedNoGold);
    }

    #[test]
    fn turns_and_consumption_terminates_on_degenerate_inputs() {
        let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId::NO_ID);
        builder.flags = crate::access::unit_type_flags(UnitType::Constructor);
        builder.grid = Point::new(10, 10);

        // Zero turns: nothing to pay, no turns needed.
        let mut empty = builder.clone();
        empty.build_time = 0;
        empty.build_list.clear();
        let state = crate::game::GameState::new();
        let (turns, consumption) =
            turns_and_consumption(&state, &empty, UnitType::TrainingHall, 1);
        assert_eq!(turns, 0, "the team table has no ATTRIB_TURNS for a new state");
        assert_eq!(consumption, 0);
    }
}
