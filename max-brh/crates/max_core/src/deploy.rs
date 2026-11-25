//! Deployment of starting team loadouts — `GameManager_ProcessTeamMissionSupplyUnits`.
//!
//! # Why this is needed
//!
//! `GameManager_InitGame` (`game_manager.cpp:3056`) not only generates the map but
//! also lands the starting armies: without them `menu_check_end_game_conditions` sees
//! teams without "life" (`HasConstructionInfrastructure`/`HasAttackPower` are false)
//! and ends the game right after the first turn. Therefore deployment is part of
//! game creation, not of the UI.
//!
//! # What exactly is created (custom game, not a mission)
//!
//! Order and composition — verbatim `GameManager_ProcessTeamMissionSupplyUnits`
//! (`game_manager.cpp:5137` in the fork with a dedicated server; in the oracle
//! `max-cpp-port/src/game_manager.cpp:4967`):
//!
//! 1. `team_units->SetGold(team_gold)`; `stats_units_built += units.GetCount()`;
//! 2. mine `MININGST` at `GameManager_GetStartPositionMiningStation(team)`,
//!    `storage = 0`, `stats_mines_built`/`stats_buildings_built` +1;
//! 3. large slab `LRGSLAB` on the mine cell, frame —
//!    `Randomizer_Generate(image_count(LRGSLAB))`;
//! 4. `UnitsManager_SetInitialMining` for the mine;
//! 5. generator `POWGEN` at `GameManager_GetStartingPositionPowerGenerator(...)`,
//!    order `ORDER_POWER_ON`/`ORDER_STATE_INIT`;
//! 6. small slab `SMLSLAB` on the generator cell, frame —
//!    `Randomizer_Generate(image_count(SMLSLAB))`;
//! 7. `UnitsManager_UpdateConnectors(power_generator)` — in this C++ version the
//!    generator has no complex (`DeployUnit` ignores the `complex` parameter),
//!    so `IsComplexMember()` is false and the call does nothing;
//! 8. supply units: `Access_FindReachableSpot` from the mine cell, then
//!    `DeployUnit` and `storage = cargo`.
//!
//! Where the unit list comes from in a custom game (missions are a separate branch):
//! `TeamMissionSupplies::units/cargos` is filled by `GetMissionLoadout`, which for
//! custom/multiplayer calls the Lua function `max_get_mission_loadout` from
//! `assets/PATCHES.RES` (`SC_L0001`). Its default loadout is
//! `CONSTRCT` (cargo 40), `ENGINEER` (cargo 20), `SURVEYOR` (cargo 0); this is
//! [`DEFAULT_MISSION_LOADOUT`].
//!
//! # RNG
//!
//! The only RNG calls during deployment are the frames of the two slabs, strictly in this
//! order and **two per team** (in ascending team index order).
//! `image_count` comes from the unit data (`D_LRGSLA`/`D_DEFALT` in `MAX.RES`,
//! the second byte of the record): 5 and 1. Since `Randomizer_Generate(1)` still
//! consumes one value, the stream matches C++.
//!
//! # Intentional limitations
//!
//! * The loadout for AI teams (`AiPlayer::RollTeamMissionSupplies`, `aiplayer.cpp`)
//!   is not ported in this task: it pulls in strategy selection and
//!   `ChooseInitialUpgrades`, which are currently outside the M3.2 scope. All teams
//!   receive [`DEFAULT_MISSION_LOADOUT`] — the same as a human who changed nothing
//!   in the supply menu.
//! * Missions (`MissionCategory != Custom`) receive supplies via packet 12 and
//!   separate scripts; here exactly the custom branch is ported.
//! * The slab frame (`angle`/`image_index`) is a UI field: `max_core` keeps
//!   `angle` (the RNG value), while `image_base`/`image_index` stay 0, as for
//!   other units (sprite fields are outside the simulation).

use crate::access;
use crate::enums::{
    GameStateType, UnitOrder, UnitOrderState, UnitType, CARGO_FUEL, CARGO_GOLD, CARGO_MASK,
};
use crate::game::{GameState, PLAYER_TEAM_COUNT};
use crate::geo::{Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_WIDTH};
use crate::setup::TeamStart;
use crate::team::{TeamId, TeamKind};
use crate::unit::UnitId;
use crate::unit_values::UnitValues;
use crate::victory::cargo_fuel_consumption_rate;

/// Default `start_gold` — `settings.cpp:118` (150).
pub const DEFAULT_START_GOLD: u32 = 150;

/// `image_count` of `LRGSLAB` — the second byte of the `D_LRGSLA` resource in `MAX.RES`.
pub const LARGE_SLAB_IMAGE_COUNT: i32 = 5;
/// `image_count` of `SMLSLAB` — the second byte of the `D_DEFALT` resource in `MAX.RES`.
pub const SMALL_SLAB_IMAGE_COUNT: i32 = 1;

/// Cells in the scan radius of `Survey_GetTotalResourcesInArea`/`SetInitialMining`.
pub const SURVEY_RADIUS: i32 = 1;
/// Per-resource cap in `Survey_GetResourcesInArea` (`survey.cpp:193-203`):
/// raw, gold and fuel are each clamped to 255 independently, not as a shared
/// sum. `Survey_GetTotalResourcesInArea` passes this value as `resource_limit`.
pub const SURVEY_RESOURCE_LIMIT: i32 = 255;
/// `Access_FindReachableSpot` searches across the whole map width during landing
/// (`ResourceManager_MapSize.x`).
pub const DEPLOY_SEARCH_RANGE: i32 = MAP_WIDTH as i32;

/// One starting-loadout unit — `TeamMissionSupplies::units[i]`/`cargos[i]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamSupply {
    /// Unit type (`MAX_UNIT.*`).
    pub unit_type: UnitType,
    /// `cargo` — the `storage` value after landing.
    pub cargo: u16,
}

/// Default loadout for a custom game (`SC_L0001`, `max_get_mission_loadout`).
pub const DEFAULT_MISSION_LOADOUT: [TeamSupply; 3] = [
    TeamSupply {
        unit_type: UnitType::Constructor,
        cargo: 40,
    },
    TeamSupply {
        unit_type: UnitType::Engineer,
        cargo: 20,
    },
    TeamSupply {
        unit_type: UnitType::Surveyor,
        cargo: 0,
    },
];

/// `Survey_GetResourcesInArea(grid_x, grid_y, radius, 255, ..., mode = true, team)`
/// (`survey.cpp:157`): the sum of resources in the square `[grid, grid + radius)`.
///
/// Returns `(raw, gold, fuel)` — the C++ output-parameter order. `mode =
/// true` in C++ means `mask = 0xFFFF`, i.e. the team's explored state does not matter.
pub(crate) fn survey_resources_in_area(state: &GameState, grid: Point, radius: i32) -> (i32, i32, i32) {
    let mut raw = 0i32;
    let mut gold = 0i32;
    let mut fuel = 0i32;

    let max_x = MAP_WIDTH as i32 - 1;
    let max_y = MAP_HEIGHT as i32 - 1;
    let start_x = i32::from(grid.x);
    let start_y = i32::from(grid.y);
    let end_x = max_x.min(start_x.saturating_add(radius));
    let end_y = max_y.min(start_y.saturating_add(radius));

    let mut x = start_x;

    while x <= end_x {
        let mut y = start_y;

        while y <= end_y {
            let value = state.map.resources.get(x as i16, y as i16).bits();

            // `mode = true` → `mask = 0xFFFF`; `Access_GetModifiedSurfaceType`
            // without a new argument (mode = false), as in `survey.cpp`.
            if value != 0
                && access::modified_surface_type(state, Point::new(x as i16, y as i16), false)
                    != access::SURFACE_TYPE_AIR
            {
                let amount = i32::from(value & u16::from(CARGO_MASK)).min(16);

                if value & u16::from(CARGO_GOLD) != 0 {
                    gold += amount;
                } else if value & u16::from(CARGO_FUEL) != 0 {
                    fuel += amount;
                } else {
                    raw += amount;
                }
            }

            y += 1;
        }

        x += 1;
    }

    (
        raw.min(SURVEY_RESOURCE_LIMIT),
        gold.min(SURVEY_RESOURCE_LIMIT),
        fuel.min(SURVEY_RESOURCE_LIMIT),
    )
}

/// `GameManager_IsValidStartingPosition` (`game_manager.cpp:4360`).
///
/// The 4×4 area around the point (`x-1..=x+2`) must be land according to
/// `Access_GetModifiedSurfaceType` (including alien-debris bridges).
pub(crate) fn is_valid_starting_position(state: &GameState, position: Point) -> bool {
    let grid_x = i32::from(position.x);
    let grid_y = i32::from(position.y);

    if grid_x < 1
        || grid_y < 1
        || (grid_x + 3) > MAP_WIDTH as i32
        || (grid_y + 3) > MAP_HEIGHT as i32
    {
        return false;
    }

    for y in (grid_y - 1)..(grid_y + 3) {
        for x in (grid_x - 1)..(grid_x + 3) {
            if access::modified_surface_type(state, Point::new(x as i16, y as i16), false)
                != access::SURFACE_TYPE_LAND
            {
                return false;
            }
        }
    }

    true
}

/// `GameManager_FindValidStartingPosition` (`game_manager.cpp:4712`), the variant with
/// `GameState` (takes into account units already on the map).
#[allow(dead_code)] // Oracle trace: state-aware C++ variant; setup uses the SurfaceMap one.
pub(crate) fn find_valid_starting_position(state: &GameState, position: &mut Point) {
    if is_valid_starting_position(state, *position) {
        return;
    }

    let mut range = 2i32;

    loop {
        position.x = position.x.wrapping_sub(1);
        position.y = position.y.wrapping_add(1);

        for direction in (0..8usize).step_by(2) {
            for _ in 0..range {
                *position += DIRECTION_OFFSETS[direction];

                if is_valid_starting_position(state, *position) {
                    return;
                }
            }
        }

        range += 2;
    }
}

/// `GameManager_GetStartPositionMiningStation` (`game_manager.cpp:1794`).
///
/// If the starting cell (and around it) lacks materials/fuel, the point
/// is searched in an expanding spiral, as in C++. `max_resources` is the generation
/// setting (`GenerationSettings::max_resources`).
pub(crate) fn start_position_mining_station(
    state: &GameState,
    starting_position: Point,
    max_resources: i32,
) -> Point {
    let minimum_fuel = (max_resources * 8 + 10) / 20;
    let minimum_materials = (max_resources * 12 + 10) / 20;
    let mut point = starting_position;

    let (raw, _gold, fuel) = survey_resources_in_area(state, point, SURVEY_RADIUS);

    if !is_valid_starting_position(state, point) || raw < minimum_materials || fuel < minimum_fuel {
        for range in 1..10 {
            point = Point::new(
                starting_position.x.wrapping_sub(range as i16),
                starting_position.y.wrapping_add(range as i16),
            );

            for direction in (0..8usize).step_by(2) {
                for _ in 0..(2 * range) {
                    point += DIRECTION_OFFSETS[direction];

                    if is_valid_starting_position(state, point) {
                        let (raw, _gold, fuel) =
                            survey_resources_in_area(state, point, SURVEY_RADIUS);

                        if raw >= minimum_materials && fuel >= minimum_fuel {
                            return point;
                        }
                    }
                }
            }
        }
    }

    point
}

/// `GameManager_GetStartingPositionPowerGenerator` (`game_manager.cpp:1833`).
///
/// Cells are returned exactly as in C++ (RNG is not used here, the order of
/// `spawn_unit` calls does not change). The generator may stand **next to** the
/// mine — this is a normal base layout; the visual sprite "overlap"
/// is handled in the renderer: buildings with the `BUILDING` flag have their anchor shifted by +31 px
/// (`max_app::render::BUILDING_ANCHOR_OFFSET`, C++ `UnitInfo::SetPosition`).
pub(crate) fn starting_position_power_generator(
    state: &GameState,
    team: TeamId,
    mut point: Point,
) -> Point {
    point.x = point.x.wrapping_sub(1);
    point.y = point.y.wrapping_add(2);

    for direction in (0..8usize).step_by(2) {
        for _ in 0..2 {
            point += DIRECTION_OFFSETS[direction];

            if access::is_accessible(
                state,
                UnitType::PowerGenerator,
                team,
                point,
                access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) != 0
            {
                return point;
            }
        }

        point += DIRECTION_OFFSETS[direction];
    }

    point.y = point.y.wrapping_sub(1);

    point
}

/// `UnitsManager_SetInitialMining` (`units_manager.cpp:1484`).
pub(crate) fn set_initial_mining(state: &mut GameState, id: UnitId, grid: Point) {
    let (raw, gold, fuel) = survey_resources_in_area(state, grid, SURVEY_RADIUS);
    let fuel_mining = cargo_fuel_consumption_rate(UnitType::PowerGenerator).min(fuel);
    let mut free_capacity = 16 - fuel_mining;
    let raw_mining = raw.min(free_capacity);
    free_capacity -= raw_mining;
    let fuel_extra = (fuel - fuel_mining).min(free_capacity);
    free_capacity -= fuel_extra;
    let gold_mining = gold.min(free_capacity);

    if let Some(unit) = state.unit_mut(id) {
        unit.raw_mining_max = raw.clamp(0, i32::from(u8::MAX)) as u8;
        unit.fuel_mining_max = fuel.clamp(0, i32::from(u8::MAX)) as u8;
        unit.gold_mining_max = gold.clamp(0, i32::from(u8::MAX)) as u8;
        unit.raw_mining = raw_mining.clamp(0, i32::from(u8::MAX)) as u8;
        unit.fuel_mining = (fuel_mining + fuel_extra).clamp(0, i32::from(u8::MAX)) as u8;
        unit.gold_mining = gold_mining.clamp(0, i32::from(u8::MAX)) as u8;
        unit.total_mining = (raw_mining + fuel_mining + fuel_extra + gold_mining)
            .clamp(0, i32::from(u8::MAX)) as u8;
    }
}

/// `Access_FindReachableSpotInt` (`access.cpp:261`), mode 0 (`IsAccessible`).
fn find_reachable_spot_int(
    state: &GameState,
    unit_type: UnitType,
    team: TeamId,
    position: &mut Point,
    range_limit: i32,
    mode: i32,
    direction: usize,
) -> bool {
    let (offset_x, offset_y) = match direction {
        0 => (1i16, 0i16),
        1 => (0, 1),
        2 => (-1, 0),
        3 => (0, -1),
        _ => return false,
    };

    for _ in 0..range_limit {
        if position.x >= 0
            && position.y >= 0
            && (position.x as i32) < MAP_WIDTH as i32
            && (position.y as i32) < MAP_HEIGHT as i32
        {
            match mode {
                0 => {
                    if access::is_accessible(
                        state,
                        unit_type,
                        team,
                        *position,
                        access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                    ) != 0
                    {
                        return true;
                    }
                }
                // Mode 1 (attack-target search) is not used during landing.
                _ => {}
            }
        }

        position.x = position.x.wrapping_add(offset_x);
        position.y = position.y.wrapping_add(offset_y);
    }

    false
}

/// `Access_FindReachableSpot` (`access.cpp:460`) in the landing scope (`mode = 0`).
///
/// Mutates `position` as C++ does: on each iteration `--x`, `--y`, then
/// a walk of four directions. Returns `true` if a cell is found.
pub(crate) fn find_reachable_spot(
    state: &GameState,
    unit_type: UnitType,
    team: TeamId,
    position: &mut Point,
    range: i32,
    exclusion_zone: i32,
) -> bool {
    for i in 1..=range {
        position.x = position.x.wrapping_sub(1);
        position.y = position.y.wrapping_sub(1);

        for direction in 0..4 {
            if find_reachable_spot_int(
                state,
                unit_type,
                team,
                position,
                exclusion_zone + 2 * i,
                0,
                direction,
            ) {
                return true;
            }
        }
    }

    false
}

/// Brings the orders of a landed unit to the values of the C++ constructor
/// `UnitInfo::UnitInfo(unit_type, team, id, angle)` plus the `switch` from
/// `UnitsManager_DeployUnit`.
fn apply_deploy_orders(
    state: &mut GameState,
    id: UnitId,
    orders: UnitOrder,
    order_state: UnitOrderState,
    prior_orders: UnitOrder,
    prior_state: UnitOrderState,
) {
    if let Some(unit) = state.unit_mut(id) {
        unit.orders = orders;
        unit.state = order_state;
        unit.prior_orders = prior_orders;
        unit.prior_state = prior_state;
    }
}

/// `GameManager_ProcessTeamMissionSupplyUnits(team)` (`game_manager.cpp:5137`).
///
/// `team_gold` — `TeamMissionSupplies::team_gold` after purchases/upgrades;
/// `loadout` — `units`/`cargos`; `starting_position` — the team's landing point;
/// `max_resources` — the generation setting (`start_gold`-independent: the
/// resource minimums at the mine are computed from it).
///
/// Returns the ids of landed units in C++ order (`DeployUnit`).
pub(crate) fn deploy_team_supplies(
    state: &mut GameState,
    team: TeamId,
    team_gold: u32,
    loadout: &[TeamSupply],
    starting_position: Point,
    max_resources: i32,
) -> Vec<UnitId> {
    let mut deployed = Vec::with_capacity(loadout.len() + 4);
    let team_index = team.0 as usize;

    // Defensive divergence: C++ `GameManager_ProcessTeamMissionSupplyUnits`
    // has no bounds check on `team` and indexes `UnitsManager_TeamMissionSupplies`
    // directly; every call site (`game_manager.cpp:3110`, `i < PLAYER_TEAM_MAX - 1`)
    // stays inside the playable range and never reaches `ALIEN`. The guard keeps
    // the Rust API safe without changing observable behaviour.
    if team_index >= PLAYER_TEAM_COUNT {
        return deployed;
    }

    // `GameManager_GameState = GAME_STATE_12_DEPLOYING_UNITS` — in C++ the
    // zeroing of `storage` in `DeployUnit` (`units_manager.cpp:2382-2384`) depends on it:
    // during landing the storage is **not** zeroed, and the cargo is set explicitly below
    // (`mining_station->storage = 0`, `unit->storage = cargo`). `spawn_unit`
    // repeats the same phase check.
    let previous_state = state.game_state;
    state.game_state = GameStateType::DeployingUnits;

    if let Some(units) = state.team_units.get_mut(team_index) {
        units.set_gold(team_gold);
    }

    if let Some(info) = state.teams.get_mut(team_index) {
        info.stats_units_built = info.stats_units_built.wrapping_add(loadout.len() as i16);
    }

    let mining_station_location =
        start_position_mining_station(state, starting_position, max_resources);

    let Some(mining_station) =
        state.spawn_unit(UnitType::MiningStation, team, mining_station_location, 0)
    else {
        state.game_state = previous_state;

        return deployed;
    };

    deployed.push(mining_station);

    // `mining_station->storage = 0` (C++), orders — `ORDER_POWER_ON`/`INIT`
    // from `UnitsManager_DeployUnit`.
    if let Some(unit) = state.unit_mut(mining_station) {
        unit.storage = 0;
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::Init;
        unit.prior_orders = UnitOrder::PowerOn;
        unit.prior_state = UnitOrderState::Init;
    }

    if let Some(info) = state.teams.get_mut(team_index) {
        info.stats_mines_built = info.stats_mines_built.wrapping_add(1);
        info.stats_buildings_built = info.stats_buildings_built.wrapping_add(1);
    }

    // `LRGSLAB` on top of the mine with a frame from RNG.
    let large_slab_angle = state.rng.generate_i32(LARGE_SLAB_IMAGE_COUNT) as u8;

    if let Some(id) = state.spawn_unit(UnitType::LargeSlab, team, mining_station_location, 0) {
        if let Some(unit) = state.unit_mut(id) {
            unit.angle = large_slab_angle;
            unit.storage = 0;
        }
        apply_deploy_orders(
            state,
            id,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        );
        deployed.push(id);
    }

    // `UnitsManager_SetInitialMining` (DeployUnit calls it for MININGST too —
    // the repeated call is idempotent, as in C++).
    set_initial_mining(state, mining_station, mining_station_location);

    let power_generator_location =
        starting_position_power_generator(state, team, mining_station_location);

    let Some(power_generator) =
        state.spawn_unit(UnitType::PowerGenerator, team, power_generator_location, 0)
    else {
        state.game_state = previous_state;

        return deployed;
    };

    deployed.push(power_generator);
    apply_deploy_orders(
        state,
        power_generator,
        UnitOrder::PowerOn,
        UnitOrderState::Init,
        UnitOrder::Await,
        UnitOrderState::ExecutingOrder,
    );

    // `SMLSLAB` on top of the generator with a frame from RNG (always 0, but the call is mandatory).
    let small_slab_angle = state.rng.generate_i32(SMALL_SLAB_IMAGE_COUNT) as u8;

    if let Some(id) = state.spawn_unit(UnitType::SmallSlab, team, power_generator_location, 0) {
        if let Some(unit) = state.unit_mut(id) {
            unit.angle = small_slab_angle;
            unit.storage = 0;
        }
        apply_deploy_orders(
            state,
            id,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        );
        deployed.push(id);
    }

    // `UnitsManager_UpdateConnectors(power_generator)`: the generator has no
    // complex (C++ passes `complex` to `DeployUnit`, but it ignores it),
    // so the call is a no-op, as in the oracle.

    for supply in loadout {
        let mut position = mining_station_location;
        find_reachable_spot(
            state,
            supply.unit_type,
            team,
            &mut position,
            DEPLOY_SEARCH_RANGE,
            0,
        );

        let Some(id) = state.spawn_unit(supply.unit_type, team, position, 0) else {
            continue;
        };

        if let Some(unit) = state.unit_mut(id) {
            unit.storage = supply.cargo as i16;
        }
        apply_deploy_orders(
            state,
            id,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        );
        deployed.push(id);
    }

    state.game_state = previous_state;

    deployed
}

/// A team ready for landing: landing point, remaining credits and loadout.
///
/// This is `UnitsManager_TeamMissionSupplies[team]` after the start phases
/// (`CargoMenu` + landing point selection, see `GameManager_PlayerMissionSetup`):
/// `position` — the point chosen by the player (or AI), `gold` — `team_gold` after
/// purchases, `supplies` — the final `units`/`cargos` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamDeployment {
    /// Team.
    pub team: TeamId,
    /// Landing point (`starting_position`).
    pub position: Point,
    /// Team credits at landing time (`team_gold`).
    pub gold: u32,
    /// Loadout: the free mission loadout plus purchased units.
    pub supplies: Vec<TeamSupply>,
    /// Attribute upgrades purchased in `CargoMenu`
    /// (`TeamUnits::SetCurrentUnitValues` before landing).
    pub upgrades: Vec<(UnitType, UnitValues)>,
    /// The team's `stats_gold_spent_on_upgrades` (`CargoMenu::CommitUpgradeChanges`).
    pub gold_spent_on_upgrades: u32,
}

impl TeamDeployment {
    /// A team with the standard `SC_L0001` loadout and the given gold.
    pub fn default_loadout(team: TeamId, position: Point, gold: u32) -> Self {
        Self {
            team,
            position,
            gold,
            supplies: DEFAULT_MISSION_LOADOUT.to_vec(),
            upgrades: Vec::new(),
            gold_spent_on_upgrades: 0,
        }
    }
}

/// Landing of the starting armies of all playing teams — the loop
/// `for (i = RED; i < MAX - 1; ++i)` from `GameManager_InitGame`, in which
/// `GameManager_ProcessTeamMissionSupplyUnits(i)` is called for
/// `team_type != TEAM_TYPE_NONE`.
///
/// `teams` — starting positions from [`crate::setup::generate_map`] (they have already passed
/// `FindValidStartingPosition`/`Optimize`); a team without an entry in `teams` is not
/// landed (the caller layer gave it no starting point).
///
/// Returns the number of landed units (all teams).
pub(crate) fn deploy_starting_units(
    state: &mut GameState,
    start_gold: u32,
    max_resources: i32,
    teams: &[TeamStart],
) -> usize {
    let deployments: Vec<TeamDeployment> = teams
        .iter()
        .map(|start| TeamDeployment::default_loadout(start.team, start.position, start_gold))
        .collect();

    deploy_starting_units_with(state, max_resources, &deployments)
}

/// The same, but with ready [`TeamDeployment`]s — landing points, credits and
/// loadouts returned by the start phases (`CargoMenu`/site select).
///
/// Traversal order is ascending team index (as in `GameManager_InitGame`),
/// so the RNG stream (slab frames, two per team) matches
/// [`deploy_starting_units`].
pub(crate) fn deploy_starting_units_with(
    state: &mut GameState,
    max_resources: i32,
    deployments: &[TeamDeployment],
) -> usize {
    let mut total = 0;

    for index in 0..PLAYER_TEAM_COUNT {
        let team = TeamId(index as u8);

        if state.teams[index].team_type == TeamKind::None {
            continue;
        }

        let Some(deployment) = deployments.iter().find(|entry| entry.team == team) else {
            continue;
        };

        total += deploy_team_supplies(
            state,
            team,
            deployment.gold,
            &deployment.supplies,
            deployment.position,
            max_resources,
        )
        .len();
    }

    total
}

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod tests;
