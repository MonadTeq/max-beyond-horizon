//! Распределение добычи шахты — C++ `allocmenu.cpp` (`AllocMenu`) и экономика
//! комплекса (`cargo.cpp`/`complex.cpp`/`access.cpp`).
//!
//! # Где хранится значение (оракул — C++)
//!
//! У шахты **нет** процентной доли и нет поля в `TeamUnits`: C++
//! (`unitinfo.hpp:434-440`) держит три счётчика на каждом юните `MININGST`
//! (`uint8_t`, сумма ограничена `16` — «единиц добычи» шахты):
//!
//! | поле | смысл |
//! |---|---|
//! | `raw_mining` | сколько единиц **сырья** в час/ход даёт шахта |
//! | `fuel_mining` | сколько единиц **топлива** |
//! | `gold_mining` | сколько единиц **золота** |
//! | `total_mining` | их сумма (кэш; `raw + fuel + gold`) |
//! | `raw_mining_max`/`fuel_mining_max`/`gold_mining_max` | сколько вообще есть в клетке (`Survey_GetTotalResourcesInArea`) |
//!
//! Эти же поля едут по сети в пакете **08** с процессором `P5`
//! (`max-brh-docs/architecture/protocol.md` §«payload P5»), входят в снапшот/CRC
//! ([`crate::crc`]) и в V70-сейв — отдельного поля в `TeamUnits` нет.
//!
//! # Что делает диалог (`allocmenu.cpp`)
//!
//! Открывается пунктом `allocate` меню юнита у `MININGST`. Три секции (`Raw`,
//! `Fuel`, `Gold`) по три полосы: добыча комплекса, потребление, запас
//! комплекса. Стрелки `LFARO_OF`/`RTARO_OF` (`+1`/`−1`) и клик по полосе
//! меняют счётчики **живо** — `Done`/`Esc` только закрывают окно, применения
//! «на выходе» нет (`AllocMenu::Run` → `GameManager_EnableMainMenu`).
//!
//! `AllocMenu_Optimize` — «включить добычу типа `X` в комплексе на `N`
//! единиц»: сначала добирает свободные единицы у шахт, где `X < максимума
//! клетки`, затем перекладывает в `X` добычу других ресурсов
//! (`Allocator::Optimize`). `AllocMenu_ReduceProduction` — обратное, «выключить
//! `N` единиц типа `X`». `AllocMenu::Balance` — сколько единиц `X` комплекс
//! может добывать при текущем распределении (нужно только для отрисовки
//! полосы).
//!
//! # Влияние на экономику
//!
//! `Cargo_GetNetProduction(MININGST)` (`cargo.cpp:105-126`) при
//! `ORDER_POWER_ON`/`ORDER_NEW_ALLOCATE` даёт
//! `raw += raw_mining`, `fuel += fuel_mining`, `gold += gold_mining`
//! (для компьютера — с множителем сложности `×5/4`, `×6/4`).
//! `Access_UpdateResourcesTotal` (`access.cpp:912`) суммирует это по всем
//! зданиям комплекса в `Complex::{material,fuel,gold,power,workers}` — так доля
//! шахты становится **добычей комплекса за ход**; `GameManager_ManageEconomy`
//! (`game_manager.cpp:2058`) вызывает её каждый ход через
//! `TeamUnits::OptimizeComplexes` → `ProductionManager_OptimizeProduction`.
//! [`crate::game::GameState::manage_economy`] делает то же самое
//! ([`update_resources_total`]).

use crate::enums::{UnitOrder, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{Cargo, ComplexId, TeamId};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::victory::{cargo_get_net_production, cargo_type_of, CargoType};

/// Предел суммы добычи одной шахты (`16 - total_mining` в `allocmenu.cpp`).
pub const MINING_CAPACITY: i32 = 16;
/// Лимит `Survey_GetResourcesInArea(..., resource_limit = 16, ...)`
/// (`allocmenu.cpp:457`).
pub const MINING_SURVEY_LIMIT: i32 = 16;
/// Радиус замера клетки шахты в `allocmenu.cpp:457`.
pub const MINING_SURVEY_RADIUS: i32 = 1;

// ---------------------------------------------------------------------------
// Ресурс добычи
// ---------------------------------------------------------------------------

/// Ресурс добычи — C++ `CARGO_MATERIALS`/`CARGO_FUEL`/`CARGO_GOLD`
/// (`enums.hpp:230-232`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MiningKind {
    /// Сырьё (`raw_mining`, `CARGO_MATERIALS = 0x80`).
    Raw,
    /// Топливо (`fuel_mining`, `CARGO_FUEL = 0x20`).
    Fuel,
    /// Золото (`gold_mining`, `CARGO_GOLD = 0x40`).
    Gold,
}

impl MiningKind {
    /// Порядок секций диалога и обхода `switch` в `allocmenu.cpp`
    /// (`Raw` → `Fuel` → `Gold`).
    pub const ALL: [Self; 3] = [Self::Raw, Self::Fuel, Self::Gold];

    /// Бит `CARGO_*` (используется как маска в `Allocator::cargo_material_type`).
    #[inline]
    pub const fn flag(self) -> u32 {
        match self {
            Self::Fuel => 0x20,
            Self::Gold => 0x40,
            Self::Raw => 0x80,
        }
    }

    /// Тип груза [`CargoType`] для `Cargo_GetNetProduction`/`Cargo_GetInventory`.
    #[inline]
    pub const fn cargo_type(self) -> CargoType {
        match self {
            Self::Raw => CargoType::Raw,
            Self::Fuel => CargoType::Fuel,
            Self::Gold => CargoType::Gold,
        }
    }

    /// Имя для журнала/QA (`allocate set raw N`).
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Fuel => "fuel",
            Self::Gold => "gold",
        }
    }

    /// Разбор имени (регистр не важен).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "raw" | "materials" | "material" => Some(Self::Raw),
            "fuel" => Some(Self::Fuel),
            "gold" => Some(Self::Gold),
            _ => None,
        }
    }

    /// Текущее значение поля у юнита.
    #[inline]
    pub fn get(self, unit: &UnitInfo) -> u8 {
        match self {
            Self::Raw => unit.raw_mining,
            Self::Fuel => unit.fuel_mining,
            Self::Gold => unit.gold_mining,
        }
    }

    /// Запись поля (`uint8_t` в C++).
    #[inline]
    pub fn set(self, unit: &mut UnitInfo, value: i32) {
        let value = value.clamp(0, i32::from(u8::MAX)) as u8;

        match self {
            Self::Raw => unit.raw_mining = value,
            Self::Fuel => unit.fuel_mining = value,
            Self::Gold => unit.gold_mining = value,
        }
    }

    /// Максимум добычи в клетке шахты (`raw_mining_max` и т.п.).
    #[inline]
    pub fn max(self, unit: &UnitInfo) -> u8 {
        match self {
            Self::Raw => unit.raw_mining_max,
            Self::Fuel => unit.fuel_mining_max,
            Self::Gold => unit.gold_mining_max,
        }
    }

    /// Индекс 0..2 (`Raw`, `Fuel`, `Gold`) — ключ массивов UI.
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
// Вспомогательное
// ---------------------------------------------------------------------------

/// Шахта, которую C++ учитывает в диалоге/комплексе:
/// `GetOrder() != POWER_OFF && != DISABLE && != IDLE` (`allocmenu.cpp:454-457`,
/// `complex.cpp:119-120`).
#[inline]
pub fn is_active_mine(unit: &UnitInfo) -> bool {
    unit.unit_type == UnitType::MiningStation
        && unit.orders != UnitOrder::PowerOff
        && unit.orders != UnitOrder::Disable
        && unit.orders != UnitOrder::Idle
}

/// Стационарные юниты в порядке обхода C++ (`UnitsManager_StationaryUnits`).
#[inline]
fn stationary_ids(state: &GameState) -> Vec<UnitId> {
    state.unit_list(UnitListKind::Stationary).to_vec()
}

/// `Survey_GetResourcesInArea(x, y, 1, 16, &raw, &gold, &fuel, true, team)` —
/// ресурсы в квадрате клетки шахты с лимитом 16 (а не 255, как у
/// `Survey_GetTotalResourcesInArea`).
///
/// Возвращает `(raw, gold, fuel)` — порядок выходных параметров C++.
pub fn mining_survey(state: &GameState, grid: Point) -> (i32, i32, i32) {
    let (raw, gold, fuel) = crate::deploy::survey_resources_in_area(state, grid, MINING_SURVEY_RADIUS);

    (
        raw.min(MINING_SURVEY_LIMIT),
        gold.min(MINING_SURVEY_LIMIT),
        fuel.min(MINING_SURVEY_LIMIT),
    )
}

/// Максимум добычи ресурса в клетке шахты из `mining_survey`.
#[inline]
fn survey_capacity(state: &GameState, grid: Point, kind: MiningKind) -> i32 {
    let (raw, gold, fuel) = mining_survey(state, grid);

    match kind {
        MiningKind::Raw => raw,
        MiningKind::Fuel => fuel,
        MiningKind::Gold => gold,
    }
}

/// `Cargo_GetInventory(unit)` (`cargo.cpp:30`) — груз в `storage` по типу груза.
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

/// `Cargo_GetCargoCapacity(unit)` (`cargo.cpp:50`) — вместимость `ATTRIB_STORAGE`
/// по типу груза.
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
    /// Команда, чей комплекс перераспределяется.
    ///
    /// В C++ комплекс — объект команды (`Complex*`), поэтому сравнение
    /// указателей само отделяет чужие команды. В `max_core` комплекс — это
    /// `(team, ComplexId)` (id уникален только внутри команды), поэтому
    /// команда проверяется явно.
    team: TeamId,
    /// Комплекс, в котором идёт перераспределение.
    complex: ComplexId,
    /// Сколько единиц ещё нужно раздать.
    cargo_demand: i32,
    /// Маска «какие ресурсы нельзя забирать» (`cargo_type1 | cargo_type2`).
    cargo_material_type: u32,
    /// Сколько единиц раздали.
    material_mining: i32,
}

impl Allocator {
    /// `Allocator::Optimize(cargo_type, cargo1, cargo_value, cargo2)`
    /// (`allocmenu.cpp:413`): переложить до `cargo_demand` единиц из поля
    /// `source` этого же юнита в его поле `target`.
    ///
    /// `cargo1`/`cargo2` в C++ — указатели в поля **одного** юнита; вложенный
    /// `AllocMenu_Optimize` может изменить этот же юнит, поэтому значения
    /// перечитываются после вызова (как и в C++, где указатели «живые»).
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

        if self.cargo_material_type & target.flag() == 0 && source.get(&unit) != 0 {
            let mut demand = self.cargo_demand;
            let target_value = i32::from(target.get(&unit));

            if target_value + demand > capacity {
                demand = capacity - target_value;
            }

            if i32::from(source.get(&unit)) < demand {
                demand = i32::from(source.get(&unit));
            }

            // `AllocMenu_Optimize(&*complex, cargo_type, demand, cargo_material_type)`
            let demand = optimize_masked(
                state,
                self.team,
                self.complex,
                target,
                demand,
                self.cargo_material_type,
            );

            if demand != 0 {
                if let Some(mut unit) = state.unit(unit_id).cloned() {
                    let source_value = i32::from(source.get(&unit));
                    let target_value = i32::from(target.get(&unit));

                    source.set(&mut unit, source_value - demand);
                    target.set(&mut unit, target_value + demand);
                    unit.total_mining = unit.total_mining.wrapping_add(demand as u8);
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
/// (`allocmenu.cpp:438`) — «включить добычу `kind` на `material` единиц».
///
/// `mask` — второй тип `cargo_type2` (в C++ это либо тот же ресурс, либо маска
/// `cargo_material_type` вложенного аллокатора).
///
/// Возвращает, сколько единиц реально удалось добавить.
pub fn optimize_masked(
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

/// `AllocMenu_Optimize(complex, kind, material, kind)` — публичная форма
/// (`AllocMenu::OnClick*Bar`, `Run` case 1001/1004/1007).
pub fn optimize(
    state: &mut GameState,
    team: TeamId,
    complex: ComplexId,
    kind: MiningKind,
    material: i32,
) -> i32 {
    optimize_masked(state, team, complex, kind, material, kind.flag())
}

/// `AllocMenu_ReduceProduction(complex, cargo_type, amount)`
/// (`allocmenu.cpp:525`) — выключить `amount` единиц добычи `kind` в комплексе.
pub fn reduce_production(
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

/// `AllocMenu::Balance(complex, cargo_type, material1, material2)`
/// (`allocmenu.cpp:567`) — «сколько единиц ресурса доступно комплексу».
///
/// Нужен только для полосы диалога: перераспределяет добычу, чтобы показать
/// достижимый уровень, и возвращает его.
pub fn balance(
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
/// фактическая добыча комплекса и максимум по клеткам шахт.
///
/// `materials` — сумма `raw/fuel/gold_mining` (+ `free_capacity` свободных
/// единиц добычи), `capacity` — сумма максимумов клеток.
pub fn get_cargo_mining(state: &GameState, team: TeamId, complex: ComplexId) -> (Cargo, Cargo) {
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

/// `Complex::GetCargoMinable(capacity)` (`complex.cpp:87`) — «занято»
/// производством остальных зданий комплекса (потребление).
pub fn get_cargo_minable(state: &GameState, team: TeamId, complex: ComplexId) -> Cargo {
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

/// `Complex::GetCargoInfo(materials, capacity)` (`complex.cpp:136`) — что лежит
/// в комплексе и сколько он может хранить.
pub fn get_cargo_info(state: &GameState, team: TeamId, complex: ComplexId) -> (Cargo, Cargo) {
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

/// `Access_UpdateResourcesTotal(complex)` (`access.cpp:912`) — пересчитать
/// запасы комплекса из его зданий: для каждого члена
/// `Cargo_GetNetProduction(unit, false) + Cargo_GetInventory(unit)`.
///
/// Именно здесь добыча шахты (`raw_mining`/`fuel_mining`/`gold_mining`)
/// становится добычей комплекса **за ход**: `GameManager_ManageEconomy`
/// вызывает это каждый ход (`OptimizeComplexes`).
///
/// Оговорка порта: расход сырья строящегося завода C++ списывает здесь же
/// (через `Cargo_GetNetProduction`), но порт не допускает отрицательных
/// складов и списывает ставку в [`crate::production`] с паузой. Поэтому для
/// кэша комплекса берётся [`cargo_get_net_production_for_total`] — та же
/// формула без `raw`-расхода завода (см. её документацию).
pub fn update_resources_total(state: &mut GameState, team: crate::team::TeamId, complex: ComplexId) {
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
        target.material = material.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        target.fuel = fuel.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        target.gold = gold.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        target.power = power.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        target.workers = workers.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    }
}

/// Суммарная добыча комплекса по ресурсу — то, что показывает диалог
/// (`production.raw` в `AllocMenu`).
pub fn total_mining(state: &GameState, team: TeamId, complex: ComplexId, kind: MiningKind) -> i32 {
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

/// Комплекс, в котором стоит шахта (для открытия диалога).
pub fn unit_complex(unit: &UnitInfo) -> Option<ComplexId> {
    unit.complex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{unit_flags, TeamClan, UnitOrderState};
    use crate::team::TeamId;
    use crate::TeamUnits;

    /// Партия с одной шахтой RED в клетке с известными ресурсами:
    /// квадрат 2×2 вокруг `(20, 20)` — сырьё 8+8, топливо 8, золото 8
    /// (`Survey_GetResourcesInArea` суммирует 4 клетки и режет по 16).
    fn state_with_mine() -> (GameState, UnitId, ComplexId) {
        let mut state = GameState::new();
        state.teams[0].team_type = crate::team::TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);

        state.map.resources.set(
            20,
            20,
            crate::map::ResourceMapEntry::new(8, Some(crate::map::ResourceKind::Raw), 0),
        );
        state.map.resources.set(
            21,
            20,
            crate::map::ResourceMapEntry::new(8, Some(crate::map::ResourceKind::Raw), 0),
        );
        state.map.resources.set(
            20,
            21,
            crate::map::ResourceMapEntry::new(8, Some(crate::map::ResourceKind::Gold), 0),
        );
        state.map.resources.set(
            21,
            21,
            crate::map::ResourceMapEntry::new(8, Some(crate::map::ResourceKind::Fuel), 0),
        );

        let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
        let grid = Point::new(20, 20);
        let id = state
            .spawn_unit(UnitType::MiningStation, TeamId::RED, grid, flags)
            .expect("шахта");

        let mut unit = state.unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.total_mining = 0;
        unit.raw_mining = 0;
        unit.fuel_mining = 0;
        unit.gold_mining = 0;
        state.write_unit(id, unit);

        let complex = ComplexId(1);
        state.team_units[0].complexes.push(crate::team::Complex::new(1));
        state.unit_mut(id).expect("unit").complex = Some(complex);

        (state, id, complex)
    }

    #[test]
    fn mining_kind_fields_match_cpp_layout() {
        assert_eq!(MiningKind::Raw.flag(), 0x80);
        assert_eq!(MiningKind::Fuel.flag(), 0x20);
        assert_eq!(MiningKind::Gold.flag(), 0x40);
        assert_eq!(MiningKind::Raw.cargo_type(), CargoType::Raw);
        assert_eq!(MiningKind::ALL.map(MiningKind::index), [0, 1, 2]);
        assert_eq!(MiningKind::parse("RAW"), Some(MiningKind::Raw));
        assert_eq!(MiningKind::parse("nope"), None);
    }

    #[test]
    fn optimize_fills_free_capacity_up_to_the_cell_maximum() {
        let (mut state, id, complex) = state_with_mine();
        let unit = state.unit(id).cloned().expect("unit");
        let capacity = survey_capacity(&state, unit.grid, MiningKind::Raw);
        assert_eq!(capacity, 16, "в квадрате 2×2 лежит сырьё 8+8");

        let added = optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 4);
        assert_eq!(added, 4);
        assert_eq!(i32::from(state.unit(id).unwrap().raw_mining), 4);
        assert_eq!(i32::from(state.unit(id).unwrap().total_mining), 4);

        // Избыток упирается в свободные единицы шахты (всего 16).
        let added = optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 99);
        assert_eq!(added, 12, "16 − 4 свободных единиц");
        assert_eq!(i32::from(state.unit(id).unwrap().raw_mining), 16);
    }

    #[test]
    fn survey_limit_sixteen_matches_cpp() {
        let (state, id, _complex) = state_with_mine();
        let unit = state.unit(id).cloned().expect("unit");
        let (raw, gold, fuel) = mining_survey(&state, unit.grid);

        assert_eq!(raw, 16, "8+8, но лимит 16");
        assert_eq!(gold, 8);
        assert_eq!(fuel, 8);
    }

    #[test]
    fn reduce_production_keeps_total_mining_in_sync() {
        let (mut state, id, complex) = state_with_mine();
        optimize(&mut state, TeamId::RED, complex, MiningKind::Raw, 6);

        let before = i32::from(state.unit(id).unwrap().raw_mining);
        assert_eq!(before, 6);

        reduce_production(&mut state, TeamId::RED, complex, MiningKind::Raw, 2);
        let unit = state.unit(id).unwrap();
        assert_eq!(i32::from(unit.raw_mining), before - 2);
        assert_eq!(
            i32::from(unit.total_mining),
            i32::from(unit.raw_mining) + i32::from(unit.fuel_mining) + i32::from(unit.gold_mining)
        );
    }

    #[test]
    fn mining_never_exceeds_sixteen_units() {
        let (mut state, id, complex) = state_with_mine();

        for kind in MiningKind::ALL {
            optimize(&mut state, TeamId::RED, complex, kind, 16);
        }

        let unit = state.unit(id).unwrap();
        let total = i32::from(unit.raw_mining) + i32::from(unit.fuel_mining) + i32::from(unit.gold_mining);
        assert_eq!(total, MINING_CAPACITY, "16 — потолок шахты");
        assert_eq!(i32::from(unit.total_mining), total);
        assert_eq!(i32::from(unit.raw_mining), 16, "первым забрал сырьё");
        assert_eq!(i32::from(unit.fuel_mining), 0, "свободных единиц уже нет");
    }

    #[test]
    fn net_production_counts_mining_only_when_powered() {
        let (mut state, id, _complex) = state_with_mine();
        {
            let unit = state.unit_mut(id).unwrap();
            unit.raw_mining = 3;
            // `Cargo_GetNetProduction(..., current_order = false)` берёт
            // `prior_order` при `ORDER_STATE_INIT` — приказ должен быть уже в
            // исполнении, как у работающей шахты.
            unit.state = UnitOrderState::ExecutingOrder;
        }

        let unit = state.unit(id).cloned().unwrap();
        let net = cargo_get_net_production(&state, &unit, false);
        assert_eq!(net.raw, 3);

        let mut unit = unit;
        unit.orders = UnitOrder::PowerOff;
        let net = cargo_get_net_production(&state, &unit, false);
        assert_eq!(net.raw, 0, "выключенная шахта не добывает");
    }

    #[test]
    fn update_resources_total_puts_mining_into_the_complex() {
        let (mut state, id, complex) = state_with_mine();
        state.unit_mut(id).unwrap().raw_mining = 4;
        state.unit_mut(id).unwrap().total_mining = 4;

        update_resources_total(&mut state, TeamId::RED, complex);

        let stored = state.complex(TeamId::RED, complex.0 as u16).expect("комплекс");
        assert!(stored.material >= 4, "добыча сырья в комплексе: {}", stored.material);
    }

    #[test]
    fn allocation_changes_the_complex_income() {
        let (mut state, id, complex) = state_with_mine();

        // Переключить всю добычу на золото.
        state.unit_mut(id).unwrap().raw_mining = 0;
        state.unit_mut(id).unwrap().gold_mining = 5;
        state.unit_mut(id).unwrap().total_mining = 5;
        update_resources_total(&mut state, TeamId::RED, complex);
        let gold_income = state
            .complex(TeamId::RED, complex.0 as u16)
            .expect("комплекс")
            .gold;
        assert!(gold_income >= 5);

        // Переключить на сырьё — золото уходит из добычи.
        state.unit_mut(id).unwrap().gold_mining = 0;
        state.unit_mut(id).unwrap().raw_mining = 5;
        update_resources_total(&mut state, TeamId::RED, complex);
        let after = state.complex(TeamId::RED, complex.0 as u16).expect("комплекс");
        assert!(after.material >= 5);
        assert_eq!(after.gold, 0, "доля золота 0 — добычи золота нет");
    }

    #[test]
    fn mining_weight_uses_the_cpp_difficulty_factor() {
        let (mut state, id, _complex) = state_with_mine();
        state.unit_mut(id).unwrap().raw_mining = 4;
        let unit = state.unit(id).cloned().unwrap();
        assert_eq!(cargo_get_net_production(&state, &unit, false).raw, 4);

        state.teams[0].team_type = crate::team::TeamKind::Computer;
        state.ai.settings.opponent = crate::enums::Opponent::Master as u8;
        let unit = state.unit(id).cloned().unwrap();
        assert_eq!(cargo_get_net_production(&state, &unit, false).raw, 5);

        state.ai.settings.opponent = crate::enums::Opponent::God as u8;
        let unit = state.unit(id).cloned().unwrap();
        assert_eq!(cargo_get_net_production(&state, &unit, false).raw, 6);
    }
}
