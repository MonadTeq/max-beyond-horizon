//! Build orders and weight-table consumers.
//!
//! Oracle: `aiplayer.cpp:945-1054` (`AiPlayer::GetWeightTable`),
//! `aiplayer.cpp:1647-2054` (`AddBuildOrder`, `CheckReconnaissanceNeeds`,
//! `GetMaxSpottedScan`, `ChooseStrategicBuildOrders`, `ProcessBuildOrders`,
//! `ChooseGenericBuildOrders`), `aiplayer.cpp:3387-3517`
//! (`GetFilteredWeightTable`, `GetExtendedWeightTable`).
//!
//! # What the AI builds
//!
//! `ChooseStrategicBuildOrders`/`ChooseGenericBuildOrders` produce the turn's
//! prioritised [`BuildOrder`] list: reconnaissance (`SCANNER`/`AWAC`/`FASTBOAT`/
//! `SCOUT`), strategy-driven combat types, plus the generic fallback list. The
//! list feeds `AiPlayer::ChooseUpgrade` in C++ (phase A.5) and is kept on
//! [`AiPlayer`] (`strategic_build_orders`/`generic_build_orders`) so that the
//! upgrade stage sees exactly what C++ sees at `aiplayer.cpp:2503-2504`.
//!
//! `GetWeightTable` and its filtered/extended variants select unit types by the
//! 18 [`WeightTable`]s of phase A.1. `GetWeightTable(INVALID_ID)` composes the
//! table from the air-defense, bomber and submarine tables, the commando table
//! (expert opponents) and the tables of every spotted enemy type — this is the
//! "army composition against what we have seen" rule. `GetExtendedWeightTable`
//! further adapts the table to a concrete target (alien/expert commando,
//! water-only targets, land/water reachability).
//!
//! `ProcessBuildOrders` drops orders for types that no team unit is
//! constructing right now (`ORDER_BUILD` and not `ORDER_STATE_UNIT_READY`); that
//! is why a depleted build list leads to building factories first.
//!
//! Determinism: no RNG is used anywhere in this module; every decision is a pure
//! function of the AI state, the spotted units and the current unit values.

use crate::access::{
    is_valid_attack_target_type, modified_surface_type, surface_type, unit_type_flags,
    unit_type_land_type, SURFACE_TYPE_LAND, SURFACE_TYPE_WATER,
};
use crate::enums::{unit_flags, AiStrategy, Opponent, UnitOrder, UnitOrderState, UnitType};
use crate::geo::Point;
use crate::unit::UnitInfo;
use crate::unit_values::{
    ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_HITS, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN,
    ATTRIB_SPEED,
};

use super::strategy::AiPlayer;
use super::threat::ZoneWalker;
use super::weight::WeightTable;

/// `struct BuildOrder` (`aiplayer.hpp:39`) — a unit type and the attribute the
/// upgrade should improve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildOrder {
    /// `primary_attribute` — an `ATTRIB_*` value.
    pub primary_attribute: u8,
    /// `unit_type`.
    pub unit_type: UnitType,
}

impl BuildOrder {
    /// `BuildOrder(primary_attribute, unit_type)`.
    pub const fn new(primary_attribute: u8, unit_type: UnitType) -> Self {
        Self {
            primary_attribute,
            unit_type,
        }
    }
}

/// `Builder_IsBuildable` (`builder.cpp:31`) under the default game rules: some
/// builder capability list contains the type (see [`crate::production::BUILDABLE`]).
pub(crate) fn is_buildable(unit_type: UnitType) -> bool {
    crate::production::builder_type(unit_type).is_some()
}

/// `GetWeightTable` (`aiplayer.cpp:945`).
///
/// `unit_type == None` — C++ `INVALID_ID`: the composed table (air defense +
/// bomber + submarine, the commando table for expert opponents, and the table of
/// every spotted enemy type). Unknown types fall through to the generic table.
pub(crate) fn get_weight_table(
    player: &AiPlayer,
    world: &crate::game::GameState,
    unit_type: Option<UnitType>,
) -> WeightTable {
    let Some(unit_type) = unit_type else {
        let mut table = WeightTable::new(0);

        table.merge(&player.weight_table_air_defense);
        table.merge(&player.weight_table_bomber);
        table.merge(&player.weight_table_submarine);

        if world.ai.settings.opponent >= Opponent::Expert as u8 {
            let buildable = is_buildable(UnitType::Infiltrator);
            table.add_buildable(UnitType::Infiltrator, 1, buildable);
        }

        for spotted in &player.spotted_units {
            let Some(spotted_type) = spotted
                .unit
                .and_then(|unit_id| world.unit(unit_id))
                .map(|unit| unit.unit_type)
            else {
                // C++ dereferences `SpottedUnit::GetUnit()` without a null check;
                // a spotted unit that no longer exists is simply skipped here.
                continue;
            };

            let table_for_type = get_weight_table(player, world, Some(spotted_type));
            table.merge(&table_for_type);
        }

        return table;
    };

    match unit_type {
        UnitType::Escort => player.weight_table_fastboat.clone(),
        UnitType::Fighter | UnitType::AlienPlane | UnitType::AirTransport | UnitType::Awac => {
            player.weight_table_aircrafts.clone()
        }
        UnitType::Corvette => player.weight_table_corvette.clone(),
        UnitType::Juggernaut | UnitType::Gunboat => player.weight_table_battleships.clone(),
        UnitType::Submarine => player.weight_table_submarine.clone(),
        UnitType::MissileCruiser => player.weight_table_missileboat.clone(),
        UnitType::SeaTransport | UnitType::CargoShip => player.weight_table_support_ships.clone(),
        UnitType::AlienTank | UnitType::Tank => player.weight_table_tanks.clone(),
        UnitType::AlienAssaultGun | UnitType::AssaultGun | UnitType::RocketLauncher => {
            player.weight_table_assault_guns.clone()
        }
        UnitType::Bomber => player.weight_table_bomber.clone(),
        UnitType::MissileCrawler => player.weight_table_missile_launcher.clone(),
        UnitType::Infiltrator => player.weight_table_commando.clone(),
        UnitType::Infantry => player.weight_table_infantry.clone(),
        UnitType::Scout => player.weight_table_scout.clone(),
        UnitType::MobileAntiAir | UnitType::AntiAir => player.weight_table_air_defense.clone(),
        UnitType::LandMine | UnitType::SeaMine => player.weight_table_generic.clone(),
        UnitType::GunTurret | UnitType::Artillery | UnitType::MissileLauncher => {
            player.weight_table_ground_defense.clone()
        }
        _ => player.weight_table_generic.clone(),
    }
}

/// `GetFilteredWeightTable` (`aiplayer.cpp:3387`).
///
/// `flags`:
/// * `0x01` — zero the weights of stationary types (`STATIONARY`);
/// * `0x02` — zero the weights of regenerating alien types (`REGENERATING_UNIT`).
pub(crate) fn get_filtered_weight_table(
    player: &AiPlayer,
    world: &crate::game::GameState,
    unit_type: Option<UnitType>,
    flags: u8,
) -> WeightTable {
    let mut table = get_weight_table(player, world, unit_type);

    for entry in table.entries_mut() {
        let Some(entry_type) = entry.unit_type else {
            continue;
        };

        let type_flags = unit_type_flags(entry_type);

        if flags & 0x01 != 0 && type_flags & unit_flags::STATIONARY != 0 {
            entry.weight = 0;
        }

        if flags & 0x02 != 0 && type_flags & unit_flags::REGENERATING_UNIT != 0 {
            entry.weight = 0;
        }
    }

    table
}

/// `AiPlayer::IsSurfaceTypePresent` (`aiplayer.cpp:3481`): whether a cell of
/// `surface_type` is within `range` of `site`.
pub(crate) fn is_surface_type_present(
    world: &crate::game::GameState,
    site: Point,
    range: i32,
    surface_type: u8,
) -> bool {
    let mut walker = ZoneWalker::new(site, range);

    loop {
        let grid = Point::new(walker.grid_x() as i16, walker.grid_y() as i16);

        if modified_surface_type(world, grid, true) == surface_type {
            return true;
        }

        if !walker.find_next() {
            return false;
        }
    }
}

/// `GetExtendedWeightTable` (`aiplayer.cpp:3459`).
///
/// Adapts the filtered table to the target: alien targets against expert
/// opponents get commandos; other alien targets get the `ADUMP` table; a
/// stationary target that touches water adds a corvette (and a submarine for
/// average opponents and above); finally, land/water attack types whose weapon
/// range does not reach the target's surface are disabled.
pub(crate) fn get_extended_weight_table(
    player: &AiPlayer,
    world: &crate::game::GameState,
    target: &UnitInfo,
    flags: u8,
) -> WeightTable {
    let opponent = world.ai.settings.opponent;
    let team_units = &world.team_units[player.player_team.0 as usize];
    let position = target.grid;

    let mut table = get_filtered_weight_table(player, world, Some(target.unit_type), flags);

    if target.team == crate::team::TeamId::ALIEN && opponent >= Opponent::Expert as u8 {
        table.clear();
        let buildable = is_buildable(UnitType::Infiltrator);
        table.add_buildable(UnitType::Infiltrator, 1, buildable);
    } else {
        if target.team == crate::team::TeamId::ALIEN {
            table =
                get_filtered_weight_table(player, world, Some(UnitType::MaterialStorage), flags);
        }

        if target.flags & unit_flags::STATIONARY != 0 {
            let mut is_water_present = false;
            let (ulx, uly, lrx, lry) = unit_bounds(target);

            for x in ulx..lrx {
                for y in uly..lry {
                    if surface_type(world, Point::new(x as i16, y as i16)) == SURFACE_TYPE_WATER {
                        is_water_present = true;
                    }
                }
            }

            if is_water_present {
                let buildable = is_buildable(UnitType::Corvette);
                table.add_buildable(UnitType::Corvette, 1, buildable);
            }

            if opponent >= Opponent::Average as u8 {
                let buildable = is_buildable(UnitType::Submarine);
                table.add_buildable(UnitType::Submarine, 1, buildable);
            }
        }

        for entry in table.entries_mut() {
            let Some(entry_type) = entry.unit_type else {
                continue;
            };

            let surface_type = unit_type_land_type(entry_type);
            let unit_range = team_units
                .current_unit_values(entry_type as u16)
                .get_attribute(ATTRIB_RANGE);

            if surface_type == SURFACE_TYPE_LAND
                && !is_surface_type_present(world, position, unit_range, SURFACE_TYPE_LAND)
            {
                entry.weight = 0;
            }

            if surface_type == SURFACE_TYPE_WATER
                && !is_surface_type_present(world, position, unit_range, SURFACE_TYPE_WATER)
            {
                entry.weight = 0;
            }
        }
    }

    table
}

/// `UnitInfo::GetBounds` (`unitinfo.cpp`) — the grid rectangle of a unit
/// (`lrx`/`lry` are exclusive, as in the C++ loops).
fn unit_bounds(unit: &UnitInfo) -> (i32, i32, i32, i32) {
    let ulx = i32::from(unit.grid.x);
    let uly = i32::from(unit.grid.y);
    let mut lrx = ulx + 1;
    let mut lry = uly + 1;

    if unit.flags & unit_flags::BUILDING != 0 {
        lrx += 1;
        lry += 1;
    }

    (ulx, uly, lrx, lry)
}

/// `AiPlayer::GetMaxSpottedScan` (`aiplayer.cpp:1686`) — the best scan of the
/// spotted enemy units of a type (0 if none).
pub(crate) fn get_max_spotted_scan(
    player: &AiPlayer,
    world: &crate::game::GameState,
    unit_type: UnitType,
) -> i32 {
    let mut result = 0;

    for spotted in &player.spotted_units {
        let Some(unit) = spotted.unit.and_then(|unit_id| world.unit(unit_id)) else {
            continue;
        };

        if unit.unit_type == unit_type && player.is_enemy_team(world, unit.team) {
            result = result.max(unit.base_values.get_attribute(ATTRIB_SCAN));
        }
    }

    result
}

/// `AiPlayer::AddBuildOrder` (`aiplayer.cpp:1647`) — pushes only buildable types.
pub fn add_build_order(build_orders: &mut Vec<BuildOrder>, unit_type: UnitType, attribute: u8) {
    if is_buildable(unit_type) {
        build_orders.push(BuildOrder::new(attribute, unit_type));
    }
}

/// `AiPlayer::CheckReconnaissanceNeeds` (`aiplayer.cpp:1656`).
///
/// A reconnaissance type is ordered when the spotted armed enemy units outrange
/// the type's scan. `mode` — only faster-than-zero armed enemies count (mobile
/// C++ call sites), `false` — every armed enemy.
pub(crate) fn check_reconnaissance_needs(
    player: &AiPlayer,
    world: &crate::game::GameState,
    build_orders: &mut Vec<BuildOrder>,
    unit_type: UnitType,
    mode: bool,
) {
    let team_units = &world.team_units[player.player_team.0 as usize];
    let mut unit_range_max = 0;

    let unit_scan = team_units
        .base_unit_values(unit_type as u16)
        .get_attribute(ATTRIB_SCAN);

    for spotted in &player.spotted_units {
        let Some(unit) = spotted.unit.and_then(|unit_id| world.unit(unit_id)) else {
            continue;
        };

        if !player.is_enemy_team(world, unit.team) {
            continue;
        }

        let attack = unit.base_values.get_attribute(ATTRIB_ATTACK);
        let speed = unit.base_values.get_attribute(ATTRIB_SPEED);

        if attack > 0
            && (mode || speed != 0)
            && is_valid_attack_target_type(unit.unit_type, unit_type)
        {
            // Do not compete with units that are in another league.
            if team_units
                .base_unit_values(unit.unit_type as u16)
                .get_attribute(ATTRIB_RANGE)
                > unit_scan
            {
                continue;
            }

            unit_range_max = unit_range_max.max(unit.base_values.get_attribute(ATTRIB_RANGE) + 1);
        }
    }

    if unit_range_max
        > team_units
            .current_unit_values(unit_type as u16)
            .get_attribute(ATTRIB_SCAN)
    {
        add_build_order(build_orders, unit_type, ATTRIB_SCAN);
    }
}

/// `AiPlayer::ProcessBuildOrders` (`aiplayer.cpp:1935`).
///
/// Keeps only orders whose type some team unit is constructing right now
/// (`ORDER_BUILD`, not `ORDER_STATE_UNIT_READY`). Stationary factories and
/// mobile builders are both scanned.
pub(crate) fn process_build_orders(
    player: &AiPlayer,
    world: &crate::game::GameState,
    build_orders: &mut Vec<BuildOrder>,
) {
    let mut unit_types: Vec<UnitType> = Vec::new();

    for kind in [
        crate::unit::UnitListKind::Stationary,
        crate::unit::UnitListKind::MobileLandSea,
    ] {
        for id in world.unit_list(kind) {
            let Some(unit) = world.unit(*id) else {
                continue;
            };

            if unit.team != player.player_team
                || unit.orders != UnitOrder::Build
                || unit.state == UnitOrderState::UnitReady
            {
                continue;
            }

            let Some(constructed) = unit.build_list.first().copied() else {
                continue;
            };

            if !unit_types.contains(&constructed) {
                unit_types.push(constructed);
            }
        }
    }

    build_orders.retain(|order| unit_types.contains(&order.unit_type));
}

/// `AiPlayer::ChooseStrategicBuildOrders` (`aiplayer.cpp:1700`).
///
/// `mode == true` — only the reconnaissance needs and the strategy's first
/// combat orders (used by `ChooseInitialUpgrades`); `mode == false` — the full
/// list, including the generic recon needs and the low-priority extras (used by
/// `BeginTurn`).
pub(crate) fn choose_strategic_build_orders(
    player: &AiPlayer,
    world: &crate::game::GameState,
    mode: bool,
) -> Vec<BuildOrder> {
    let mut build_orders = Vec::new();

    if mode {
        match player.strategy {
            AiStrategy::Missiles | AiStrategy::CombinedArms => {
                check_reconnaissance_needs(
                    player,
                    world,
                    &mut build_orders,
                    UnitType::Scanner,
                    false,
                );
            }
            AiStrategy::Air => {
                check_reconnaissance_needs(player, world, &mut build_orders, UnitType::Awac, true);
            }
            AiStrategy::Sea => {
                check_reconnaissance_needs(
                    player,
                    world,
                    &mut build_orders,
                    UnitType::Escort,
                    true,
                );
            }
            _ => {}
        }
    } else {
        check_reconnaissance_needs(player, world, &mut build_orders, UnitType::Scanner, true);
        check_reconnaissance_needs(player, world, &mut build_orders, UnitType::Awac, true);
        check_reconnaissance_needs(player, world, &mut build_orders, UnitType::Escort, true);
        check_reconnaissance_needs(player, world, &mut build_orders, UnitType::Scout, true);

        let commando_scan = world.team_units[player.player_team.0 as usize]
            .current_unit_values(UnitType::Infiltrator as u16)
            .get_attribute(ATTRIB_SCAN);

        if get_max_spotted_scan(player, world, UnitType::Infantry) >= commando_scan {
            add_build_order(&mut build_orders, UnitType::Infiltrator, ATTRIB_SCAN);
        }

        let submarine_scan = world.team_units[player.player_team.0 as usize]
            .current_unit_values(UnitType::Submarine as u16)
            .get_attribute(ATTRIB_SCAN);

        if get_max_spotted_scan(player, world, UnitType::Corvette) >= submarine_scan {
            add_build_order(&mut build_orders, UnitType::Submarine, ATTRIB_SCAN);
        }

        process_build_orders(player, world, &mut build_orders);
    }

    if build_orders.is_empty() {
        strategic_build_orders_for_strategy(player, &mut build_orders, mode);
    }

    if !mode {
        process_build_orders(player, world, &mut build_orders);
    }

    build_orders
}

/// The `switch (strategy)` body of `ChooseStrategicBuildOrders`.
fn strategic_build_orders_for_strategy(
    player: &AiPlayer,
    build_orders: &mut Vec<BuildOrder>,
    mode: bool,
) {
    let mut add = |unit_type: UnitType, attribute: u8| {
        add_build_order(build_orders, unit_type, attribute);
    };

    match player.strategy {
        AiStrategy::Defensive => {
            add(UnitType::MissileLauncher, ATTRIB_RANGE);
            add(UnitType::MissileLauncher, ATTRIB_ATTACK);

            if !mode {
                add(UnitType::Radar, ATTRIB_SCAN);

                add(UnitType::GunTurret, ATTRIB_ATTACK);
                add(UnitType::GunTurret, ATTRIB_ROUNDS);
                add(UnitType::GunTurret, ATTRIB_ARMOR);
                add(UnitType::GunTurret, ATTRIB_HITS);
                add(UnitType::GunTurret, ATTRIB_RANGE);

                add(UnitType::AntiAir, ATTRIB_ROUNDS);
                add(UnitType::AntiAir, ATTRIB_RANGE);
                add(UnitType::AntiAir, ATTRIB_ATTACK);
            }
        }

        AiStrategy::Missiles => {
            add(UnitType::MissileCrawler, ATTRIB_RANGE);

            if !mode {
                add(UnitType::Scanner, ATTRIB_SCAN);

                add(UnitType::MissileCrawler, ATTRIB_ATTACK);

                add(UnitType::Scanner, ATTRIB_SPEED);

                add(UnitType::MissileCrawler, ATTRIB_SPEED);

                add(UnitType::MissileCruiser, ATTRIB_RANGE);
                add(UnitType::MissileCruiser, ATTRIB_ATTACK);
                add(UnitType::MissileCruiser, ATTRIB_SPEED);

                add(UnitType::Escort, ATTRIB_SCAN);
                add(UnitType::Escort, ATTRIB_SPEED);
            }
        }

        AiStrategy::Air => {
            add(UnitType::Fighter, ATTRIB_ATTACK);
            add(UnitType::Fighter, ATTRIB_RANGE);

            add(UnitType::Bomber, ATTRIB_ATTACK);

            if !mode {
                add(UnitType::Fighter, ATTRIB_ROUNDS);
                add(UnitType::Fighter, ATTRIB_HITS);
                add(UnitType::Fighter, ATTRIB_ARMOR);

                add(UnitType::Awac, ATTRIB_SCAN);

                add(UnitType::Bomber, ATTRIB_ROUNDS);
                add(UnitType::Bomber, ATTRIB_HITS);
                add(UnitType::Bomber, ATTRIB_ARMOR);
            }
        }

        AiStrategy::Sea => {
            add(UnitType::Gunboat, ATTRIB_ATTACK);
            add(UnitType::Gunboat, ATTRIB_ARMOR);
            add(UnitType::Gunboat, ATTRIB_HITS);

            add(UnitType::MissileCruiser, ATTRIB_RANGE);

            if !mode {
                add(UnitType::Submarine, ATTRIB_ATTACK);
                add(UnitType::Submarine, ATTRIB_SCAN);

                add(UnitType::Gunboat, ATTRIB_ROUNDS);

                add(UnitType::MissileCruiser, ATTRIB_ATTACK);

                add(UnitType::Escort, ATTRIB_ATTACK);
                add(UnitType::Escort, ATTRIB_ROUNDS);
                add(UnitType::Escort, ATTRIB_RANGE);
                add(UnitType::Escort, ATTRIB_SPEED);
                add(UnitType::Escort, ATTRIB_SCAN);

                add(UnitType::Corvette, ATTRIB_ATTACK);
                add(UnitType::Corvette, ATTRIB_ROUNDS);
                add(UnitType::Corvette, ATTRIB_SPEED);
                add(UnitType::Corvette, ATTRIB_SCAN);
            }
        }

        AiStrategy::ScoutHorde => {
            add(UnitType::Scout, ATTRIB_ATTACK);
            add(UnitType::Scout, ATTRIB_ARMOR);
            add(UnitType::Scout, ATTRIB_HITS);
            add(UnitType::Scout, ATTRIB_SPEED);
        }

        AiStrategy::TankHorde => {
            add(UnitType::Scout, ATTRIB_SPEED);

            add(UnitType::Tank, ATTRIB_ATTACK);
            add(UnitType::Tank, ATTRIB_ROUNDS);
            add(UnitType::Tank, ATTRIB_HITS);
            add(UnitType::Tank, ATTRIB_ARMOR);
            add(UnitType::Tank, ATTRIB_SPEED);

            if !mode {
                add(UnitType::Tank, ATTRIB_RANGE);
            }
        }

        AiStrategy::FastAttack => {
            add(UnitType::Scout, ATTRIB_SCAN);

            add(UnitType::AssaultGun, ATTRIB_ATTACK);
            add(UnitType::AssaultGun, ATTRIB_ROUNDS);
            add(UnitType::AssaultGun, ATTRIB_RANGE);

            if !mode {
                add(UnitType::AssaultGun, ATTRIB_SPEED);

                add(UnitType::Scout, ATTRIB_ATTACK);
                add(UnitType::Scout, ATTRIB_SPEED);

                add(UnitType::MobileAntiAir, ATTRIB_ATTACK);
                add(UnitType::MobileAntiAir, ATTRIB_ROUNDS);
                add(UnitType::MobileAntiAir, ATTRIB_RANGE);
                add(UnitType::MobileAntiAir, ATTRIB_SPEED);

                add(UnitType::Bomber, ATTRIB_ATTACK);

                add(UnitType::Escort, ATTRIB_ATTACK);
                add(UnitType::Escort, ATTRIB_ROUNDS);
                add(UnitType::Escort, ATTRIB_RANGE);
                add(UnitType::Escort, ATTRIB_SPEED);
                add(UnitType::Escort, ATTRIB_SCAN);

                add(UnitType::Corvette, ATTRIB_RANGE);
                add(UnitType::Corvette, ATTRIB_ROUNDS);
                add(UnitType::Corvette, ATTRIB_ATTACK);
                add(UnitType::Corvette, ATTRIB_SPEED);
                add(UnitType::Corvette, ATTRIB_SCAN);
            }
        }

        AiStrategy::CombinedArms => {
            add(UnitType::MissileCrawler, ATTRIB_RANGE);

            add(UnitType::Tank, ATTRIB_ATTACK);
            add(UnitType::Tank, ATTRIB_ARMOR);
            add(UnitType::Tank, ATTRIB_HITS);

            if !mode {
                add(UnitType::Scanner, ATTRIB_SCAN);
                add(UnitType::Scanner, ATTRIB_SPEED);

                add(UnitType::MissileCrawler, ATTRIB_ATTACK);
                add(UnitType::MissileCrawler, ATTRIB_SPEED);

                add(UnitType::Tank, ATTRIB_ROUNDS);
                add(UnitType::Tank, ATTRIB_SPEED);
            }
        }

        AiStrategy::Espionage => {
            add(UnitType::Infiltrator, ATTRIB_SCAN);
            add(UnitType::Infiltrator, ATTRIB_SPEED);

            add(UnitType::MissileCrawler, ATTRIB_RANGE);

            if !mode {
                add(UnitType::Submarine, ATTRIB_ATTACK);
                add(UnitType::Submarine, ATTRIB_SCAN);

                add(UnitType::Scanner, ATTRIB_SCAN);

                add(UnitType::Awac, ATTRIB_SCAN);

                add(UnitType::MissileCruiser, ATTRIB_RANGE);

                add(UnitType::ArmouredPersonnelCarrier, ATTRIB_SPEED);
            }
        }

        AiStrategy::Random => {}
    }
}

/// `AiPlayer::ChooseGenericBuildOrders` (`aiplayer.cpp:1969`) — the fallback
/// order list shared by every strategy.
pub(crate) fn choose_generic_build_orders(
    player: &AiPlayer,
    world: &crate::game::GameState,
) -> Vec<BuildOrder> {
    let mut result = Vec::new();

    let mut add = |unit_type: UnitType, attribute: u8| {
        add_build_order(&mut result, unit_type, attribute);
    };

    add(UnitType::MissileLauncher, ATTRIB_RANGE);
    add(UnitType::MissileLauncher, ATTRIB_ATTACK);
    add(UnitType::MissileLauncher, ATTRIB_ROUNDS);

    add(UnitType::Radar, ATTRIB_SCAN);

    add(UnitType::GunTurret, ATTRIB_ATTACK);
    add(UnitType::GunTurret, ATTRIB_ROUNDS);
    add(UnitType::GunTurret, ATTRIB_ARMOR);
    add(UnitType::GunTurret, ATTRIB_HITS);
    add(UnitType::GunTurret, ATTRIB_RANGE);

    add(UnitType::AntiAir, ATTRIB_ROUNDS);
    add(UnitType::AntiAir, ATTRIB_RANGE);
    add(UnitType::AntiAir, ATTRIB_ATTACK);

    add(UnitType::MissileCrawler, ATTRIB_RANGE);
    add(UnitType::MissileCrawler, ATTRIB_ATTACK);
    add(UnitType::MissileCrawler, ATTRIB_ROUNDS);

    add(UnitType::Scanner, ATTRIB_SCAN);

    add(UnitType::MissileCruiser, ATTRIB_RANGE);
    add(UnitType::MissileCruiser, ATTRIB_ATTACK);
    add(UnitType::MissileCruiser, ATTRIB_ROUNDS);

    add(UnitType::Escort, ATTRIB_SCAN);
    add(UnitType::Escort, ATTRIB_ATTACK);
    add(UnitType::Escort, ATTRIB_ROUNDS);
    add(UnitType::Escort, ATTRIB_RANGE);

    add(UnitType::Fighter, ATTRIB_ATTACK);
    add(UnitType::Fighter, ATTRIB_ROUNDS);
    add(UnitType::Fighter, ATTRIB_RANGE);
    add(UnitType::Fighter, ATTRIB_HITS);
    add(UnitType::Fighter, ATTRIB_ARMOR);

    add(UnitType::Bomber, ATTRIB_ATTACK);
    add(UnitType::Bomber, ATTRIB_ROUNDS);
    add(UnitType::Bomber, ATTRIB_HITS);
    add(UnitType::Bomber, ATTRIB_ARMOR);

    add(UnitType::Awac, ATTRIB_SCAN);

    add(UnitType::Gunboat, ATTRIB_ATTACK);
    add(UnitType::Gunboat, ATTRIB_ARMOR);
    add(UnitType::Gunboat, ATTRIB_RANGE);
    add(UnitType::Gunboat, ATTRIB_HITS);
    add(UnitType::Gunboat, ATTRIB_ROUNDS);

    add(UnitType::Submarine, ATTRIB_ATTACK);
    add(UnitType::Submarine, ATTRIB_SCAN);
    add(UnitType::Submarine, ATTRIB_ROUNDS);

    add(UnitType::Corvette, ATTRIB_ATTACK);
    add(UnitType::Corvette, ATTRIB_ROUNDS);
    add(UnitType::Corvette, ATTRIB_SCAN);
    add(UnitType::Corvette, ATTRIB_RANGE);

    add(UnitType::Scout, ATTRIB_SPEED);
    add(UnitType::Scout, ATTRIB_SCAN);

    add(UnitType::Tank, ATTRIB_ATTACK);
    add(UnitType::Tank, ATTRIB_ROUNDS);
    add(UnitType::Tank, ATTRIB_HITS);
    add(UnitType::Tank, ATTRIB_ARMOR);

    add(UnitType::AssaultGun, ATTRIB_ATTACK);
    add(UnitType::AssaultGun, ATTRIB_ROUNDS);
    add(UnitType::AssaultGun, ATTRIB_RANGE);
    add(UnitType::AssaultGun, ATTRIB_SPEED);

    add(UnitType::MobileAntiAir, ATTRIB_ATTACK);
    add(UnitType::MobileAntiAir, ATTRIB_ROUNDS);
    add(UnitType::MobileAntiAir, ATTRIB_RANGE);

    add(UnitType::Infiltrator, ATTRIB_SCAN);
    add(UnitType::Infiltrator, ATTRIB_SPEED);

    process_build_orders(player, world, &mut result);

    result
}

impl AiPlayer {
    /// `AiPlayer::GetWeightTable` (`aiplayer.cpp:945`).
    #[cfg(test)]
    pub(crate) fn get_weight_table(
        &self,
        world: &crate::game::GameState,
        unit_type: Option<UnitType>,
    ) -> WeightTable {
        get_weight_table(self, world, unit_type)
    }

    /// `AiPlayer::GetFilteredWeightTable` (`aiplayer.cpp:3387`).
    pub(crate) fn get_filtered_weight_table(
        &self,
        world: &crate::game::GameState,
        unit_type: Option<UnitType>,
        flags: u8,
    ) -> WeightTable {
        get_filtered_weight_table(self, world, unit_type, flags)
    }

    /// `AiPlayer::GetExtendedWeightTable` (`aiplayer.cpp:3459`).
    pub(crate) fn get_extended_weight_table(
        &self,
        world: &crate::game::GameState,
        target: &UnitInfo,
        flags: u8,
    ) -> WeightTable {
        get_extended_weight_table(self, world, target, flags)
    }

    /// `AiPlayer::ChooseStrategicBuildOrders` (`aiplayer.cpp:1700`).
    pub(crate) fn choose_strategic_build_orders(
        &self,
        world: &crate::game::GameState,
        mode: bool,
    ) -> Vec<BuildOrder> {
        choose_strategic_build_orders(self, world, mode)
    }

    /// `AiPlayer::ChooseGenericBuildOrders` (`aiplayer.cpp:1969`).
    pub(crate) fn choose_generic_build_orders(&self, world: &crate::game::GameState) -> Vec<BuildOrder> {
        choose_generic_build_orders(self, world)
    }
}

#[cfg(test)]
#[path = "build_orders_tests.rs"]
mod tests;
