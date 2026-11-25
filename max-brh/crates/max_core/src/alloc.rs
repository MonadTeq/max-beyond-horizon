//! Mine mining allocation — C++ `allocmenu.cpp` (`AllocMenu`) and complex
//! economy (`cargo.cpp`/`complex.cpp`/`access.cpp`).
//!
//! # Where the value is stored (oracle — C++)
//!
//! A mine has **no** percentage share and no `TeamUnits` field: C++
//! (`unitinfo.hpp:434-440`) keeps three counters on each `MININGST` unit
//! (`uint8_t`, the sum is limited to `16` "mining units" of the mine):
//!
//! | field | meaning |
//! |---|---|
//! | `raw_mining` | how many units of **raw materials** per hour/turn the mine yields |
//! | `fuel_mining` | how many units of **fuel** |
//! | `gold_mining` | how many units of **gold** |
//! | `total_mining` | their sum (a cache; `raw + fuel + gold`) |
//! | `raw_mining_max`/`fuel_mining_max`/`gold_mining_max` | how much exists in the cell at all (`Survey_GetTotalResourcesInArea`) |
//!
//! The same fields travel over the network in packet **08** with the `P5` processor
//! (`max-brh-docs/architecture/client/protocol.md` §"payload P5"), enter the snapshot/CRC
//! ([`crate::crc`]) and the V70 save — there is no separate `TeamUnits` field.
//!
//! # What the dialog does (`allocmenu.cpp`)
//!
//! Opened by the `allocate` item of the unit menu at `MININGST`. Three sections (`Raw`,
//! `Fuel`, `Gold`) with three bars each: complex mining, consumption, complex
//! stock. The `LFARO_OF`/`RTARO_OF` arrows (`+1`/`−1`) and a click on a bar
//! change the counters **live** — `Done`/`Esc` only close the window, there is no
//! "on exit" application (`AllocMenu::Run` → `GameManager_EnableMainMenu`).
//!
//! `AllocMenu_Optimize` — "enable mining of type `X` in the complex by `N`
//! units": first it takes free units at mines where `X < cell
//! maximum`, then it converts the mining of other resources into `X`
//! (`Allocator::Optimize`). `AllocMenu_ReduceProduction` is the reverse, "disable
//! `N` units of type `X`". `AllocMenu::Balance` — how many units of `X` the complex
//! can mine under the current allocation (needed only to draw
//! the bar).
//!
//! # Effect on the economy
//!
//! `Cargo_GetNetProduction(MININGST)` (`cargo.cpp:105-126`) with
//! `ORDER_POWER_ON`/`ORDER_NEW_ALLOCATE` yields
//! `raw += raw_mining`, `fuel += fuel_mining`, `gold += gold_mining`
//! (for a computer — with the difficulty multiplier `×5/4`, `×6/4`).
//! `Access_UpdateResourcesTotal` (`access.cpp:912`) sums this over all
//! complex buildings into `Complex::{raw,fuel,gold,power,workers}` — so the mine's
//! share becomes the **complex production per turn**; `GameManager_ManageEconomy`
//! (`game_manager.cpp:2058`) calls it every turn via
//! `TeamUnits::OptimizeComplexes` → `ProductionManager_OptimizeProduction`.
//! [`crate::game::GameState::manage_economy`] does the same
//! ([`update_resources_total`]).

use crate::enums::{UnitOrder, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{Cargo, ComplexId, TeamId};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::victory::{cargo_get_net_production, cargo_type_of, CargoType};

/// Limit of the mining sum of one mine (`16 - total_mining` in `allocmenu.cpp`).
pub const MINING_CAPACITY: i32 = 16;
/// Limit of `Survey_GetResourcesInArea(..., resource_limit = 16, ...)`
/// (`allocmenu.cpp:457`).
pub const MINING_SURVEY_LIMIT: i32 = 16;
/// Survey radius of the mine cell in `allocmenu.cpp:457`.
pub const MINING_SURVEY_RADIUS: i32 = 1;

// ---------------------------------------------------------------------------
// Mining resource
// ---------------------------------------------------------------------------

/// Mining resource — C++ `CARGO_MATERIALS`/`CARGO_FUEL`/`CARGO_GOLD`
/// (`enums.hpp:230-232`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MiningKind {
    /// Raw materials (`raw_mining`, `CARGO_MATERIALS = 0x80`).
    Raw,
    /// Fuel (`fuel_mining`, `CARGO_FUEL = 0x20`).
    Fuel,
    /// Gold (`gold_mining`, `CARGO_GOLD = 0x40`).
    Gold,
}

impl MiningKind {
    /// Order of dialog sections and of the `switch` in `allocmenu.cpp`
    /// (`Raw` → `Fuel` → `Gold`).
    pub const ALL: [Self; 3] = [Self::Raw, Self::Fuel, Self::Gold];

    /// The `CARGO_*` bit (used as a mask in `Allocator::cargo_material_type`).
    #[inline]
    pub const fn flag(self) -> u32 {
        match self {
            Self::Fuel => 0x20,
            Self::Gold => 0x40,
            Self::Raw => 0x80,
        }
    }

    /// Cargo type [`CargoType`] for `Cargo_GetNetProduction`/`Cargo_GetInventory`.
    #[inline]
    pub const fn cargo_type(self) -> CargoType {
        match self {
            Self::Raw => CargoType::Raw,
            Self::Fuel => CargoType::Fuel,
            Self::Gold => CargoType::Gold,
        }
    }

    /// Name for the log/QA (`allocate set raw N`).
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Fuel => "fuel",
            Self::Gold => "gold",
        }
    }

    /// Name parsing (case-insensitive).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "raw" | "materials" | "material" => Some(Self::Raw),
            "fuel" => Some(Self::Fuel),
            "gold" => Some(Self::Gold),
            _ => None,
        }
    }

    /// Current value of the field on the unit.
    #[inline]
    pub fn get(self, unit: &UnitInfo) -> u8 {
        match self {
            Self::Raw => unit.raw_mining,
            Self::Fuel => unit.fuel_mining,
            Self::Gold => unit.gold_mining,
        }
    }

    /// Field write (`uint8_t` in C++).
    #[inline]
    pub fn set(self, unit: &mut UnitInfo, value: i32) {
        let value = value.clamp(0, i32::from(u8::MAX)) as u8;

        match self {
            Self::Raw => unit.raw_mining = value,
            Self::Fuel => unit.fuel_mining = value,
            Self::Gold => unit.gold_mining = value,
        }
    }

    /// Mining maximum in the mine cell (`raw_mining_max` etc.).
    #[inline]
    pub fn max(self, unit: &UnitInfo) -> u8 {
        match self {
            Self::Raw => unit.raw_mining_max,
            Self::Fuel => unit.fuel_mining_max,
            Self::Gold => unit.gold_mining_max,
        }
    }

    /// Index 0..2 (`Raw`, `Fuel`, `Gold`) — the UI array key.
    #[inline]
    pub const fn index(self) -> usize {
        match self {
            Self::Raw => 0,
            Self::Fuel => 1,
            Self::Gold => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A mine that C++ counts in the dialog/complex:
/// `GetOrder() != POWER_OFF && != DISABLE && != IDLE` (`allocmenu.cpp:454-457`,
/// `complex.cpp:119-120`).
#[inline]
pub fn is_active_mine(unit: &UnitInfo) -> bool {
    unit.unit_type == UnitType::MiningStation
        && unit.orders != UnitOrder::PowerOff
        && unit.orders != UnitOrder::Disable
        && unit.orders != UnitOrder::Idle
}

/// Stationary units in C++ traversal order (`UnitsManager_StationaryUnits`).
#[inline]
fn stationary_ids(state: &GameState) -> Vec<UnitId> {
    state.unit_list(UnitListKind::Stationary).to_vec()
}

/// `Survey_GetResourcesInArea(x, y, 1, 16, &raw, &gold, &fuel, true, team)` —
/// resources in the mine-cell square with limit 16 (not 255, as in
/// `Survey_GetTotalResourcesInArea`).
///
/// Returns `(raw, gold, fuel)` — the C++ output-parameter order.
pub(crate) fn mining_survey(state: &GameState, grid: Point) -> (i32, i32, i32) {
    let (raw, gold, fuel) = crate::deploy::survey_resources_in_area(state, grid, MINING_SURVEY_RADIUS);

    (
        raw.min(MINING_SURVEY_LIMIT),
        gold.min(MINING_SURVEY_LIMIT),
        fuel.min(MINING_SURVEY_LIMIT),
    )
}

/// Mining maximum of a resource in the mine cell, from `mining_survey`.
#[inline]
fn survey_capacity(state: &GameState, grid: Point, kind: MiningKind) -> i32 {
    let (raw, gold, fuel) = mining_survey(state, grid);

    match kind {
        MiningKind::Raw => raw,
        MiningKind::Fuel => fuel,
        MiningKind::Gold => gold,
    }
}

/// `Cargo_GetInventory(unit)` (`cargo.cpp:30`) — cargo in `storage` by cargo type.
pub fn cargo_get_inventory(unit: &UnitInfo) -> Cargo {
    let mut cargo = Cargo::init();

    match cargo_type_of(unit.unit_type) {
        CargoType::Raw => cargo.raw = i32::from(unit.storage),
        CargoType::Fuel => cargo.fuel = i32::from(unit.storage),
        CargoType::Gold => cargo.gold = i32::from(unit.storage),
        _ => {}
    }

    cargo
}

/// `Cargo_GetCargoCapacity(unit)` (`cargo.cpp:50`) — `ATTRIB_STORAGE` capacity
/// by cargo type.
///
/// The function has no special cases: a `MININGST` reports its 25-unit raw
/// buffer too, and [`get_cargo_info`] sums it into the complex capacity
/// (`Cargo_GetCargoCapacity` is called for every `Complex` member).
pub fn cargo_get_cargo_capacity(unit: &UnitInfo) -> Cargo {
    let mut cargo = Cargo::init();

    if cargo_type_of(unit.unit_type) == CargoType::None {
        return cargo;
    }

    let storage = unit.base_values.get_attribute(crate::unit_values::ATTRIB_STORAGE);

    match cargo_type_of(unit.unit_type) {
        CargoType::Raw => cargo.raw = storage,
        CargoType::Fuel => cargo.fuel = storage,
        CargoType::Gold => cargo.gold = storage,
        _ => {}
    }

    cargo
}

// ---------------------------------------------------------------------------
// AllocMenu_Optimize / AllocMenu_ReduceProduction / AllocMenu::Balance
// ---------------------------------------------------------------------------

/// `struct Allocator` (`allocmenu.cpp:45-52`).
struct Allocator {
    /// The team whose complex is being reallocated.
    ///
    /// In C++ a complex is a team object (`Complex*`), so comparing
    /// pointers separates foreign teams by itself. In `max_core` a complex is
    /// `(team, ComplexId)` (the id is unique only within a team), so
    /// the team is checked explicitly.
    team: TeamId,
    /// The complex in which reallocation happens.
    complex: ComplexId,
    /// How many units still need to be distributed.
    cargo_demand: i32,
    /// Mask of "which resources must not be taken" (`cargo_type1 | cargo_type2`).
    cargo_material_type: u32,
    /// How many units were distributed.
    material_mining: i32,
}

impl Allocator {
    /// `Allocator::Optimize(cargo_type, cargo1, cargo_value, cargo2)`
    /// (`allocmenu.cpp:413`): move up to `cargo_demand` units from the field
    /// `source` of this same unit into its field `target`.
    ///
    /// `cargo_type` in C++ is the **source** resource (`cargo2`); the gate
    /// `!(cargo_material_type & cargo_type)` therefore tests the source flag, not
    /// the target one. The target's flag is always part of the mask, so testing
    /// it would make the conversion branch unreachable.
    ///
    /// `cargo1`/`cargo2` in C++ are pointers to fields of **one** unit; the nested
    /// `AllocMenu_Optimize` may change this same unit, so the values
    /// are re-read after the call (as in C++, where the pointers are "live").
    ///
    /// # Recursion
    ///
    /// The nested call passes `source` as its `cargo_type1`, so the nested mask is
    /// `source | cargo_material_type`. The gate above rejects any resource already
    /// in the mask, hence every recursion level adds a distinct `CARGO_*` bit and
    /// the depth is bounded by the three resource kinds (at most 3 levels). This
    /// is exactly the C++ termination argument — no separate counter is needed.
    fn optimize_step(
        &mut self,
        state: &mut GameState,
        unit_id: UnitId,
        target: MiningKind,
        capacity: i32,
        source: MiningKind,
    ) -> bool {
        let Some(unit) = state.unit(unit_id).cloned() else {
            return self.cargo_demand == 0;
        };

        if self.cargo_material_type & source.flag() == 0 && source.get(&unit) != 0 {
            let mut demand = self.cargo_demand;
            let target_value = i32::from(target.get(&unit));

            if target_value + demand > capacity {
                demand = capacity - target_value;
            }

            if i32::from(source.get(&unit)) < demand {
                demand = i32::from(source.get(&unit));
            }

            // `AllocMenu_Optimize(&*complex, cargo_type, demand, cargo_material_type)`
            // — `cargo_type` is the source resource, not the target.
            let demand = optimize_masked(
                state,
                self.team,
                self.complex,
                source,
                demand,
                self.cargo_material_type,
            );

            if demand != 0 {
                if let Some(mut unit) = state.unit(unit_id).cloned() {
                    let source_value = i32::from(source.get(&unit));
                    let target_value = i32::from(target.get(&unit));

                    source.set(&mut unit, source_value - demand);
                    target.set(&mut unit, target_value + demand);
                    // C++ (`allocmenu.cpp:429-432`) does not touch `total_mining`
                    // here: the two fields belong to the same unit and the swap
                    // keeps `raw + fuel + gold` unchanged.
                    state.write_unit(unit_id, unit);
                }

                self.material_mining += demand;
                self.cargo_demand -= demand;
            }
        }

        self.cargo_demand == 0
    }
}

/// `AllocMenu_Optimize(complex, cargo_type1, material, cargo_type2)`
/// (`allocmenu.cpp:438`) — "enable mining of `kind` by `material` units".
///
/// `mask` — the second type `cargo_type2` (in C++ either the same resource or the mask
/// `cargo_material_type` of the nested allocator).
///
/// Returns how many units were actually added.
pub(crate) fn optimize_masked(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    kind: MiningKind,
    material: i32,
    mask: u32,
) -> i32 {
    if material == 0 {
        return 0;
    }

    let mut alloc = Allocator {
        team,
        complex,
        cargo_demand: material,
        cargo_material_type: kind.flag() | mask,
        material_mining: 0,
    };

    for id in stationary_ids(state) {
        let Some(unit) = state.unit(id).cloned() else {
            continue;
        };

        if unit.team != team || unit.complex != Some(complex) || !is_active_mine(&unit) {
            continue;
        }

        let capacity = survey_capacity(state, unit.grid, kind);
        let current = i32::from(kind.get(&unit));

        if current < capacity {
            let total = i32::from(unit.total_mining);

            if total < MINING_CAPACITY {
                let mut remaining = MINING_CAPACITY - total;

                if remaining > alloc.cargo_demand {
                    remaining = alloc.cargo_demand;
                }

                if capacity - current < remaining {
                    remaining = capacity - current;
                }

                if remaining > 0 {
                    if let Some(mut unit) = state.unit(id).cloned() {
                        let target = i32::from(kind.get(&unit)) + remaining;
                        kind.set(&mut unit, target);
                        unit.total_mining = unit.total_mining.wrapping_add(remaining as u8);
                        state.write_unit(id, unit);
                    }

                    alloc.material_mining += remaining;
                    alloc.cargo_demand -= remaining;

                    if alloc.cargo_demand == 0 {
                        break;
                    }
                }
            }

            if alloc.optimize_step(state, id, kind, capacity, MiningKind::Raw)
                || alloc.optimize_step(state, id, kind, capacity, MiningKind::Fuel)
                || alloc.optimize_step(state, id, kind, capacity, MiningKind::Gold)
            {
                break;
            }
        }
    }

    alloc.material_mining
}

/// `AllocMenu_Optimize(complex, kind, material, kind)` — the public form
/// (`AllocMenu::OnClick*Bar`, `Run` case 1001/1004/1007).
pub(crate) fn optimize(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    kind: MiningKind,
    material: i32,
) -> i32 {
    optimize_masked(state, team, complex, kind, material, kind.flag())
}

/// `AllocMenu_ReduceProduction(complex, cargo_type, amount)`
/// (`allocmenu.cpp:525`) — disable `amount` units of `kind` mining in the complex.
pub(crate) fn reduce_production(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    kind: MiningKind,
    amount: i32,
) {
    let mut amount = amount;

    for id in stationary_ids(state) {
        let Some(unit) = state.unit(id).cloned() else {
            continue;
        };

        if unit.team != team || unit.complex != Some(complex) || !is_active_mine(&unit) {
            continue;
        }

        let production = i32::from(kind.get(&unit));

        if production < amount {
            if production != 0 {
                amount -= production;

                if let Some(mut unit) = state.unit(id).cloned() {
                    unit.total_mining = unit.total_mining.wrapping_sub(production as u8);
                    kind.set(&mut unit, 0);
                    state.write_unit(id, unit);
                }
            }
        } else {
            if let Some(mut unit) = state.unit(id).cloned() {
                kind.set(&mut unit, production - amount);
                unit.total_mining = unit.total_mining.wrapping_sub(amount as u8);
                state.write_unit(id, unit);
            }

            return;
        }
    }
}

/// `ProductionManager::OptimizeCargoProduction(type, amount, mode)`
/// (`production_manager.cpp:564-640`) — the automatic redistribution of mining
/// that the end-of-turn optimizer runs when a resource balance is negative.
///
/// C++ first tries to grow `type` on the free mining units of the complex
/// (`AllocMenu_Optimize`, `production_manager.cpp:591-599`), then swaps the
/// production of the other resources into `type` (`SwapProduction` —
/// `FreeUpProductionCapacity` + `AllocMenu_Optimize`, `:603-615`) and, if the
/// request still cannot be met in `mode == false`, rolls the whole partial
/// reallocation back (`AllocMenu_ReduceProduction` and the inverse
/// `AllocMenu_Optimize` calls, `:617-633`) and returns 0.
///
/// The port keeps a single allocator call: `AllocMenu_Optimize` already converts
/// raw/fuel/gold shares of the complex's mines into the target resource and
/// re-allocates the displaced shares to the other mines
/// (`Allocator::Optimize`, `allocmenu.cpp:413-523`), so the separate
/// `SwapProduction` steps do not add a new distribution. What matters is the
/// "all or nothing" property of `mode == false`: a partial conversion would only
/// lower the raw/gold income of the complex without covering the shortage, so it
/// is rolled back.
///
/// `amount <= 0` returns 1 like C++ (`result = 1`), so the caller can treat
/// "already satisfied" and "converted" uniformly.
pub(crate) fn optimize_cargo_production(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    kind: MiningKind,
    amount: i32,
    mode: bool,
) -> i32 {
    if amount <= 0 {
        return 1;
    }

    let added = optimize(state, team, complex, kind, amount);

    if added == 0 {
        return 0;
    }

    if !mode && added < amount {
        // C++ `free_capacity == 0 || (amount != 0 && !mode)` rollback: the
        // partial growth of `kind` is removed again, the other resources keep
        // their (re-allocated) shares — `AllocMenu_ReduceProduction` is the
        // inverse of the added units only.
        reduce_production(state, team, complex, kind, added);
        return 0;
    }

    added
}

/// `AllocMenu::Balance(complex, cargo_type, material1, material2)`
/// (`allocmenu.cpp:567`) — "how many units of a resource are available to the complex".
///
/// Needed only for the dialog bar: reallocates mining to show
/// the achievable level, and returns it.
pub(crate) fn balance(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    kind: MiningKind,
    material1: i32,
    material2: i32,
) -> i32 {
    if material1 >= material2 {
        return material2;
    }

    let extra = optimize(state, team, complex, kind, material2 - material1);
    reduce_production(state, team, complex, kind, extra);

    extra + material1
}

// ---------------------------------------------------------------------------
// Complex::GetCargoMining / GetCargoMinable / GetCargoInfo
// ---------------------------------------------------------------------------

/// `Complex::GetCargoMining(materials, capacity)` (`complex.cpp:109`) —
/// actual complex mining and the maximum over mine cells.
///
/// `materials` — the sum of `raw/fuel/gold_mining` (+ `free_capacity` free
/// mining units), `capacity` — the sum of cell maximums.
pub(crate) fn get_cargo_mining(state: &GameState, team: TeamId, complex: ComplexId) -> (Cargo, Cargo) {
    let mut materials = Cargo::init();
    let mut capacity = Cargo::init();

    for id in stationary_ids(state) {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        if unit.team != team || unit.complex != Some(complex) || !is_active_mine(unit) {
            continue;
        }

        materials.gold += i32::from(unit.gold_mining);
        materials.raw += i32::from(unit.raw_mining);
        materials.fuel += i32::from(unit.fuel_mining);
        materials.free_capacity +=
            MINING_CAPACITY - (i32::from(unit.gold_mining) + i32::from(unit.raw_mining) + i32::from(unit.fuel_mining));

        let (raw, gold, fuel) = mining_survey(state, unit.grid);

        capacity.raw += raw;
        capacity.fuel += fuel;
        capacity.gold += gold;
    }

    (materials, capacity)
}

/// `Complex::GetCargoMinable(capacity)` (`complex.cpp:87`) — "occupied"
/// by the production of the other complex buildings (consumption).
pub(crate) fn get_cargo_minable(state: &GameState, team: TeamId, complex: ComplexId) -> Cargo {
    let mut capacity = Cargo::init();

    for id in stationary_ids(state) {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        if unit.team != team || unit.complex != Some(complex) {
            continue;
        }

        if unit.orders == UnitOrder::PowerOff
            || unit.orders == UnitOrder::Disable
            || unit.orders == UnitOrder::Idle
        {
            continue;
        }

        let mut cargo = cargo_get_net_production(state, unit, false);

        if unit.unit_type == UnitType::MiningStation {
            cargo.gold -= i32::from(unit.gold_mining);
            cargo.raw -= i32::from(unit.raw_mining);
            cargo.fuel -= i32::from(unit.fuel_mining);
        }

        capacity.gold -= cargo.gold;
        capacity.raw -= cargo.raw;
        capacity.fuel -= cargo.fuel;
    }

    capacity
}

/// `Complex::GetCargoInfo(materials, capacity)` (`complex.cpp:136`) — what is
/// in the complex and how much it can store.
///
/// The capacity is the sum of `Cargo_GetCargoCapacity` over **every** member
/// (`complex.cpp:140-146`): the function is generic over the cargo type, so a
/// connected `MININGST` contributes its `ATTRIB_STORAGE` buffer (25 raw) to the
/// complex reserve exactly like an `ADUMP` contributes 50. The port previously
/// narrowed the aggregation to the storage buildings — the mine's buffer was
/// missing from the reserve (defect 1 of the manual run 1.2).
pub(crate) fn get_cargo_info(state: &GameState, team: TeamId, complex: ComplexId) -> (Cargo, Cargo) {
    let mut materials = Cargo::init();
    let mut capacity = Cargo::init();

    for id in stationary_ids(state) {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        if unit.team != team || unit.complex != Some(complex) {
            continue;
        }

        let inventory = cargo_get_inventory(unit);
        let storage = cargo_get_cargo_capacity(unit);

        materials.raw += inventory.raw;
        materials.fuel += inventory.fuel;
        materials.gold += inventory.gold;

        capacity.raw += storage.raw;
        capacity.fuel += storage.fuel;
        capacity.gold += storage.gold;
    }

    (materials, capacity)
}

// ---------------------------------------------------------------------------
// Access_UpdateResourcesTotal
// ---------------------------------------------------------------------------

/// `Access_UpdateResourcesTotal(complex)` (`access.cpp:912`) — recompute
/// the complex stocks from its buildings: for each member
/// `Cargo_GetNetProduction(unit, false) + Cargo_GetInventory(unit)`.
///
/// This is where the mine's mining (`raw_mining`/`fuel_mining`/`gold_mining`)
/// becomes the complex production **per turn**: `GameManager_ManageEconomy`
/// calls this every turn (`OptimizeComplexes`).
///
/// Port caveat: C++ charges the raw consumption of a factory under construction here too
/// (via `Cargo_GetNetProduction`), but the port does not allow negative
/// stocks and charges the rate in [`crate::production`] with a pause. Therefore for
/// the complex cache [`cargo_get_net_production_for_total`] is used — the same
/// formula without the factory's `raw` consumption (see its documentation).
pub(crate) fn update_resources_total(state: &mut GameState, team: crate::team::TeamId, complex: ComplexId) {
    let mut material = 0i32;
    let mut fuel = 0i32;
    let mut gold = 0i32;
    let mut power = 0i32;
    let mut workers = 0i32;

    for id in stationary_ids(state) {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        if unit.team != team || unit.complex != Some(complex) {
            continue;
        }

        let net = crate::victory::cargo_get_net_production_for_total(state, unit, false);
        let inventory = cargo_get_inventory(unit);

        material += net.raw + inventory.raw;
        fuel += net.fuel + inventory.fuel;
        gold += net.gold + inventory.gold;
        power += net.power;
        workers += net.life;
    }

    if let Some(target) = state.complex_mut(team, complex.0 as u16) {
        target.raw = material.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        target.fuel = fuel.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        target.gold = gold.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        target.power = power.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        target.workers = workers.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    }
}

/// Total complex mining by resource — what the dialog shows
/// (`production.raw` in `AllocMenu`).
pub(crate) fn total_mining(state: &GameState, team: TeamId, complex: ComplexId, kind: MiningKind) -> i32 {
    let mut total = 0i32;

    for id in stationary_ids(state) {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        if unit.team != team || unit.complex != Some(complex) || !is_active_mine(unit) {
            continue;
        }

        total += i32::from(kind.get(unit));
    }

    total
}

/// The complex the mine stands in (to open the dialog).
pub fn unit_complex(unit: &UnitInfo) -> Option<ComplexId> {
    unit.complex
}

#[cfg(test)]
#[path = "alloc_tests.rs"]
mod tests;
