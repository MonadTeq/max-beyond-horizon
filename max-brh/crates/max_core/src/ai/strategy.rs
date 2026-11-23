//! The AI player — C++ `class AiPlayer` (`aiplayer.cpp/.hpp`).
//!
//! # What is ported
//!
//! * `AiPlayer::Init` (fields, weight tables, `need_init`), `SelectStrategy`
//!   **without map regeneration** (DESIGN: `Ai_SetupStrategy` — strategy selection
//!   only): continents are computed by flood-fill over `SurfaceMap`,
//!   strategy scores — by `aiplayer.cpp:3770` formulas, the choice — by one weighted
//!   RNG roll, as in C++;
//! * `SelectTeamClan`, `RollGreenhouseRatio`, `RollMinefieldDensity` — a port
//!   of the RNG call order and branches;
//! * `BeginTurn` (in full by structure): threat map reset, attack task
//!   rating (`UpdatePriorityTasks`), first-turn task initialization
//!   (`CheckAssaults`, `Survey`/`Explore`, `MineAssistant`, `UpdateTerrain`,
//!   `Scavenge`, `AssistMove`, `ManageBuildings`), attack selection against spotted
//!   units (`DetermineAttack`), powering buildings, registering
//!   idle units (`RegisterIdleUnits`), movement/attack reminders,
//!   `TaskManager::BeginTurn`;
//! * `CheckEndTurn` (`end_turn_retry_counter` 10/20/30/60/120), `CreateBuilding`
//!   (`FindManager` + `TaskManageBuildings::CreateBuilding`), `FindManager`,
//!   `UnitSpotted`/`UnitHidden`/`RemoveUnit`, `AddMilitaryUnit`,
//!   `GetMilitaryValue`, `DetermineAttack`, `IsKeyFacility`, `FindMines`,
//!   `GetDamagePotential(Map)`, `GetPredictedAttack`, `InvalidateThreatMaps`,
//!   `UpdateMap`/`UpdateThreatMaps`/`DetermineDefenses`/`DetermineThreats`/
//!   `NormalizeThreatMap`/`SumUpMaps` (threat maps), `SetInfoMapPoint`,
//!   `MarkMineMapPoint`, `UpdateMineMap`, `GetMineMapEntry`, `GuessEnemyAttackDirections`
//!   and `PlanMinefields` (simplified), `GetFilteredWeightTable`,
//!   `GetExtendedWeightTable`, `ShouldUpgradeUnit`, `MatchPath`.
//!
//! # Intentional simplifications (documented in code)
//!
//! 1. `ChooseStrategicBuildOrders`/`ChooseGenericBuildOrders`/`ChooseUpgrade`/
//!    `UpgradeUnitType`/`DetermineResearchProjects` rely on production data
//!    (`BuildMenu_*`, `Builder_*`, upgrade tables — P3/P7) and are
//!    therefore not ported in P6: `begin_turn` does not purchase factories and does not
//!    upgrade units; the hooks themselves are present.
//! 2. `RollTeamMissionSupplies`/`SelectLandingSite` (map setup) — P2, not P6.
//! 3. `GetThreatMap` in C++ caches 10 maps and lives on the global
//!    `AiPlayer_ThreatMaps` array; here the map is computed on demand, so
//!    reading does not require `&mut`. This does not change the result (the map is a pure
//!    function of state), but may cost more on large maps.

use crate::classification::UnitTypeClassify;
use crate::enums::{
    unit_flags, AiStrategy, CautionLevel, Opponent, ResearchTopic, TeamClan, UnitOrder,
    UnitOrderState, UnitType, AI_STRATEGY_MAX, INFO_MAP_EXPLORED, INFO_MAP_MINE_FIELD,
    INFO_MAP_NO_INFO,
};
use crate::game::{GameState, PLAYER_TEAM_COUNT};
use crate::geo::{Point, MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo, UnitListKind};

use super::task::{
    TASK_PRIORITY_ASSIST_MOVE, TASK_PRIORITY_ATTACK_DEFAULT, TASK_PRIORITY_ATTACK_MOBILE,
    TASK_PRIORITY_ATTACK_PRIORITY_TARGET, TASK_PRIORITY_CHECK_ASSAULTS, TASK_PRIORITY_ESCORT,
    TASK_PRIORITY_EXPLORE, TASK_PRIORITY_FIND_MINES, TASK_PRIORITY_FRONTIER,
    TASK_PRIORITY_MANAGE_BUILDINGS, TASK_PRIORITY_SCAVENGE,
};
use super::tasks::{TaskCtx, TaskKind};
use super::threat::ThreatMap;
use super::weight::WeightTable;

/// `AIPLAYER_THREAT_MAP_CACHE_ENTRIES` (`aiplayer.cpp:62`).
pub const THREAT_MAP_CACHE_ENTRIES: usize = 10;
/// `AiPlayer::AttackTaskLimitMin`.
pub const ATTACK_TASK_LIMIT_MIN: i32 = 3;
/// `AiPlayer::AttackTaskLimitMax`.
pub const ATTACK_TASK_LIMIT_MAX: i32 = 8;
/// `AiPlayer::AttackValue`.
pub const ATTACK_VALUE: i32 = 600;
/// `AI_SAFE_ENEMY_DISTANCE` (`ai.cpp:41`).
pub const AI_SAFE_ENEMY_DISTANCE: i32 = 400;

/// A spotted unit — C++ `class SpottedUnit` (`spottedunit.cpp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpottedUnit {
    /// Unit.
    pub unit: Option<UnitId>,
    /// The task working on this unit (`GetTask`).
    pub task: Option<u32>,
    /// Observer team.
    pub team: TeamId,
    /// Last known position.
    pub last_position: Point,
    /// Whether it is currently visible (`visible_to_team`).
    pub visible_to_team: bool,
}

impl SpottedUnit {
    /// `SpottedUnit::SpottedUnit(unit, team)`.
    pub fn new(unit: &UnitInfo, team: TeamId) -> Self {
        Self {
            unit: Some(unit.id),
            task: None,
            team,
            visible_to_team: unit.is_visible_to_team(team),
            last_position: unit.grid,
        }
    }

    /// `SpottedUnit::GetLastPosition` — visible → current cell, otherwise the last one.
    pub fn last_position_of(&self, world: &GameState) -> Point {
        let Some(unit_id) = self.unit else {
            return self.last_position;
        };

        match world.unit(unit_id) {
            Some(unit) if unit.is_visible_to_team(self.team) => unit.grid,
            _ => self.last_position,
        }
    }

    /// `SpottedUnit::UpdatePosition`.
    pub fn update_position(&mut self, world: &GameState) {
        if let Some(unit) = self.unit.and_then(|unit_id| world.unit(unit_id)) {
            self.last_position = unit.grid;
        }
    }

    /// `SpottedUnit::SetVisible`.
    pub fn set_visible(&mut self, visible: bool, world: &GameState) {
        self.visible_to_team = visible;
        self.update_position(world);
    }
}

/// The AI player — C++ `class AiPlayer`.
#[derive(Debug, Clone)]
pub struct AiPlayer {
    /// `player_team`.
    pub player_team: TeamId,
    /// `strategy`.
    pub strategy: AiStrategy,
    /// `greenhouse_ratio` (-1 — not rolled).
    pub greenhouse_ratio: i16,
    /// `minefield_density` (-1 — not rolled).
    pub minefield_density: i16,
    /// `upgrade_cost`.
    pub upgrade_cost: i16,
    /// `need_init`.
    pub need_init: bool,
    /// `tasks_pending`.
    pub tasks_pending: bool,
    /// `end_turn_retry_counter`.
    pub end_turn_retry_counter: u8,
    /// `task_clear_ground_zone`/`task_clear_air_zone` — zone clearing task ids.
    pub task_clear_ground_zone: Option<u32>,
    /// Air-zone clearing task.
    pub task_clear_air_zone: Option<u32>,
    /// `task_list`.
    pub task_list: Vec<u32>,
    /// `check_assaults_task`.
    pub check_assaults_task: Option<u32>,
    /// `defense_reserve_task`.
    pub defense_reserve_task: Option<u32>,
    /// `find_mines_task`.
    pub find_mines_task: Option<u32>,
    /// `place_mines_task`.
    pub place_mines_task: Option<u32>,
    /// `attack_reserve_task`.
    pub attack_reserve_task: Option<u32>,
    /// `attack_tasks` — from most urgent to least urgent.
    pub attack_tasks: Vec<Option<u32>>,
    /// `attack_value_share`.
    pub attack_value_share: i32,
    /// `spotted_units` (detection order).
    pub spotted_units: Vec<SpottedUnit>,
    /// `air_force`.
    pub air_force: Vec<UnitId>,
    /// `ground_forces`.
    pub ground_forces: Vec<UnitId>,
    /// `dimension_x`.
    pub dimension_x: i16,
    /// `info_map` (`INFO_MAP_*`).
    pub info_map: Vec<u8>,
    /// `mine_map` (mine density per cell).
    pub mine_map: Vec<i8>,
    /// `transport_orders` — simplified: `(client, transport, target)`.
    pub transport_orders: Vec<(UnitId, UnitId, Point)>,
    /// `target_location`.
    pub target_location: Point,
    /// Weight tables (`WeightTable` × 18, `aiplayer.hpp:93`).
    pub weight_table_ground_defense: WeightTable,
    /// `weight_table_scout`.
    pub weight_table_scout: WeightTable,
    /// `weight_table_tanks`.
    pub weight_table_tanks: WeightTable,
    /// `weight_table_assault_guns`.
    pub weight_table_assault_guns: WeightTable,
    /// `weight_table_missile_launcher`.
    pub weight_table_missile_launcher: WeightTable,
    /// `weight_table_air_defense`.
    pub weight_table_air_defense: WeightTable,
    /// `weight_table_aircrafts`.
    pub weight_table_aircrafts: WeightTable,
    /// `weight_table_bomber`.
    pub weight_table_bomber: WeightTable,
    /// `weight_table_9`.
    pub weight_table_9: WeightTable,
    /// `weight_table_fastboat`.
    pub weight_table_fastboat: WeightTable,
    /// `weight_table_corvette`.
    pub weight_table_corvette: WeightTable,
    /// `weight_table_submarine`.
    pub weight_table_submarine: WeightTable,
    /// `weight_table_battleships`.
    pub weight_table_battleships: WeightTable,
    /// `weight_table_missileboat`.
    pub weight_table_missileboat: WeightTable,
    /// `weight_table_support_ships`.
    pub weight_table_support_ships: WeightTable,
    /// `weight_table_commando`.
    pub weight_table_commando: WeightTable,
    /// `weight_table_infantry`.
    pub weight_table_infantry: WeightTable,
    /// `weight_table_generic`.
    pub weight_table_generic: WeightTable,
    /// Threat map cache (`AiPlayer_ThreatMaps`; filled on demand).
    pub threat_maps: Vec<ThreatMap>,
}

impl Default for AiPlayer {
    fn default() -> Self {
        Self::new(TeamId::RED)
    }
}

impl AiPlayer {
    /// `AiPlayer::AiPlayer` — an empty team player.
    pub fn new(player_team: TeamId) -> Self {
        let team = player_team.0;

        Self {
            player_team,
            strategy: AiStrategy::Random,
            greenhouse_ratio: -1,
            minefield_density: -1,
            upgrade_cost: 0,
            need_init: true,
            tasks_pending: true,
            end_turn_retry_counter: 0,
            task_clear_ground_zone: None,
            task_clear_air_zone: None,
            task_list: Vec::new(),
            check_assaults_task: None,
            defense_reserve_task: None,
            find_mines_task: None,
            place_mines_task: None,
            attack_reserve_task: None,
            attack_tasks: vec![None; ATTACK_TASK_LIMIT_MIN as usize],
            attack_value_share: ATTACK_VALUE,
            spotted_units: Vec::new(),
            air_force: Vec::new(),
            ground_forces: Vec::new(),
            dimension_x: MAP_WIDTH,
            info_map: vec![INFO_MAP_NO_INFO; MAP_TILES],
            mine_map: vec![0; MAP_TILES],
            transport_orders: Vec::new(),
            target_location: Point::ZERO,
            weight_table_ground_defense: WeightTable::new(team),
            weight_table_scout: WeightTable::new(team),
            weight_table_tanks: WeightTable::new(team),
            weight_table_assault_guns: WeightTable::new(team),
            weight_table_missile_launcher: WeightTable::new(team),
            weight_table_air_defense: WeightTable::new(team),
            weight_table_aircrafts: WeightTable::new(team),
            weight_table_bomber: WeightTable::new(team),
            weight_table_9: WeightTable::new(team),
            weight_table_fastboat: WeightTable::new(team),
            weight_table_corvette: WeightTable::new(team),
            weight_table_submarine: WeightTable::new(team),
            weight_table_battleships: WeightTable::new(team),
            weight_table_missileboat: WeightTable::new(team),
            weight_table_support_ships: WeightTable::new(team),
            weight_table_commando: WeightTable::new(team),
            weight_table_infantry: WeightTable::new(team),
            weight_table_generic: WeightTable::new(team),
            threat_maps: Vec::new(),
        }
    }

    /// `AiPlayer::Init` (`aiplayer.cpp:2902`) — reset the player state.
    pub fn init(&mut self) {
        self.strategy = AiStrategy::Random;
        self.greenhouse_ratio = -1;
        self.minefield_density = -1;
        self.spotted_units.clear();
        self.air_force.clear();
        self.ground_forces.clear();
        self.need_init = true;
        self.tasks_pending = true;
        self.task_list.clear();
        self.find_mines_task = None;
        self.place_mines_task = None;
        self.task_clear_ground_zone = None;
        self.task_clear_air_zone = None;
        self.check_assaults_task = None;
        self.attack_reserve_task = None;
        self.defense_reserve_task = None;
        self.attack_tasks = vec![None; ATTACK_TASK_LIMIT_MIN as usize];
        self.attack_value_share = ATTACK_VALUE;
        self.end_turn_retry_counter = 0;
        self.transport_orders.clear();
        self.target_location = Point::ZERO;
        self.info_map = vec![INFO_MAP_NO_INFO; MAP_TILES];
        self.mine_map = vec![0; MAP_TILES];
        self.threat_maps.clear();
    }

    /// `AiPlayer::GetStrategy`.
    pub fn get_strategy(&self) -> AiStrategy {
        self.strategy
    }

    /// `AiPlayer::GetAttackValueShare`.
    pub fn get_attack_value_share(&self) -> i32 {
        self.attack_value_share
    }

    /// `AiPlayer::GetTargetLocation`.
    pub fn get_target_location(&self) -> Point {
        self.target_location
    }

    /// `AiPlayer::ChangeTasksPendingFlag`.
    pub fn change_tasks_pending_flag(&mut self, value: bool) {
        self.tasks_pending = value;
    }

    /// `AiPlayer::IsEnemyTeam` (`aiplayer.cpp:2208`).
    pub fn is_enemy_team(&self, world: &GameState, team: TeamId) -> bool {
        team != self.player_team && super::task::is_enemy_team(world, team)
    }

    // -----------------------------------------------------------------------
    // Information and mine maps
    // -----------------------------------------------------------------------

    fn map_index(site: Point) -> Option<usize> {
        if site.x < 0 || site.y < 0 || site.x >= MAP_WIDTH || site.y >= MAP_HEIGHT {
            return None;
        }

        Some(site.y as usize * MAP_WIDTH as usize + site.x as usize)
    }

    /// `AiPlayer::SetInfoMapPoint` (`aiplayer.cpp:3411`).
    pub fn set_info_map_point(&mut self, site: Point) {
        if let Some(index) = Self::map_index(site) {
            if self.info_map[index] == INFO_MAP_NO_INFO {
                self.info_map[index] = INFO_MAP_EXPLORED;
            }
        }
    }

    /// `AiPlayer::MarkMineMapPoint` (`aiplayer.cpp:3423`).
    pub fn mark_mine_map_point(&mut self, site: Point) {
        if let Some(index) = Self::map_index(site) {
            if self.info_map[index] != INFO_MAP_MINE_FIELD {
                self.info_map[index] |= INFO_MAP_MINE_FIELD;
            }
        }
    }

    /// `AiPlayer::UpdateMineMap` (`aiplayer.cpp:3417`).
    pub fn update_mine_map(&mut self, site: Point) {
        if let Some(index) = Self::map_index(site) {
            if self.mine_map[index] > 0 {
                if self.info_map[index] & INFO_MAP_MINE_FIELD == 0 {
                    self.info_map[index] |= INFO_MAP_MINE_FIELD;
                }
            } else {
                self.info_map[index] &= !INFO_MAP_MINE_FIELD;
            }
        }
    }

    /// `AiPlayer::GetMineMapEntry` (`aiplayer.cpp:3429`).
    pub fn get_mine_map_entry(&self, site: Point) -> i8 {
        Self::map_index(site)
            .map(|index| self.mine_map[index])
            .unwrap_or(0)
    }

    /// `AiPlayer::ClearZone` (`aiplayer.cpp:3367`) — zone clearing task.
    pub fn clear_zone(&mut self, world: &mut GameState, unit: UnitId) {
        let Some(info) = world.unit(unit).cloned() else {
            return;
        };

        let air = info.is_air();
        let slot = if air {
            &mut self.task_clear_air_zone
        } else {
            &mut self.task_clear_ground_zone
        };

        if slot.is_none() {
            let kind = TaskKind::ClearZone {
                state: 0,
                zones: vec![info.grid],
                moving_unit: None,
            };

            let task_id = world.ai_append_task(
                self.player_team,
                None,
                super::task::TASK_PRIORITY_EMERGENCY,
                kind,
            );

            *slot = Some(task_id);
        }
    }

    // -----------------------------------------------------------------------
    // Observations
    // -----------------------------------------------------------------------

    /// `AiPlayer::UnitSpotted` (`aiplayer.cpp:3606`).
    pub fn unit_spotted(&mut self, world: &mut GameState, unit: UnitId) {
        let Some(info) = world.unit(unit).cloned() else {
            return;
        };

        if info.team == self.player_team {
            return;
        }

        if info.flags & unit_flags::GROUND_COVER != 0
            && info.unit_type != UnitType::LandMine
            && info.unit_type != UnitType::SeaMine
        {
            return;
        }

        self.invalidate_threat_maps();

        // Update the existing record (no duplicates).
        if let Some(spot) = self
            .spotted_units
            .iter_mut()
            .find(|spotted| spotted.unit == Some(unit))
        {
            spot.set_visible(true, world);
            return;
        }

        let spotted = SpottedUnit::new(&info, self.player_team);
        self.spotted_units.push(spotted);

        if info.unit_type == UnitType::LandMine || info.unit_type == UnitType::SeaMine {
            self.mine_spotted(world, &info);
        }

        let has_key_facility = self.spotted_units.iter().any(|spotted| {
            spotted
                .unit
                .and_then(|unit_id| world.unit(unit_id))
                .map(|unit| Self::is_key_facility(unit.unit_type))
                .unwrap_or(false)
        });

        if (Self::is_key_facility(info.unit_type)
            || (!has_key_facility && info.unit_type == UnitType::Constructor))
            && self.is_enemy_team(world, info.team)
        {
            let index = (self.spotted_units.len() - 1) as u32;
            self.determine_attack(world, index, TASK_PRIORITY_ATTACK_DEFAULT);
        }
    }

    /// `AiPlayer::UnitHidden` (`aiplayer.cpp:3648`).
    pub fn unit_hidden(&mut self, world: &GameState, unit: UnitId) {
        if let Some(spot) = self
            .spotted_units
            .iter_mut()
            .find(|spotted| spotted.unit == Some(unit))
        {
            spot.set_visible(false, world);
        }
    }

    /// `AiPlayer::RemoveUnit` (`aiplayer.cpp:3592`).
    pub fn remove_unit(&mut self, unit: UnitId) {
        self.air_force.retain(|id| *id != unit);
        self.ground_forces.retain(|id| *id != unit);
        self.spotted_units
            .retain(|spotted| spotted.unit != Some(unit));
    }

    /// `AiPlayer::AddMilitaryUnit` (`aiplayer.cpp:2195`).
    pub fn add_military_unit(&mut self, world: &GameState, unit: UnitId) {
        let Some(info) = world.unit(unit) else {
            return;
        };

        if info.team != self.player_team {
            return;
        }

        let list = if info.is_air() {
            &mut self.air_force
        } else {
            &mut self.ground_forces
        };

        if !list.contains(&unit) {
            list.push(unit);
        }
    }

    /// `AiPlayer::IsKeyFacility` (`aiplayer.cpp:326`).
    pub fn is_key_facility(unit_type: UnitType) -> bool {
        matches!(
            unit_type,
            UnitType::MiningStation
                | UnitType::EcoSphere
                | UnitType::GoldRefinery
                | UnitType::ResearchCentre
                | UnitType::LightVehiclePlant
                | UnitType::HeavyVehiclePlant
                | UnitType::Shipyard
                | UnitType::AirUnitsPlant
                | UnitType::TrainingHall
                | UnitType::PowerGenerator
                | UnitType::PowerStation
        )
    }

    /// `AiPlayer::MineSpotted` (`aiplayer.cpp:1078`) — mine density.
    pub fn mine_spotted(&mut self, world: &GameState, unit: &UnitInfo) {
        let mut walker = super::threat::ZoneWalker::new(unit.grid, 1);
        let density = self.minefield_density.max(0) as i32;

        loop {
            let site = walker.current_location();

            if let Some(index) = Self::map_index(site) {
                let value = self.mine_map[index] as i32 + density.max(1);
                self.mine_map[index] = value.min(i8::MAX as i32) as i8;
            }

            if !walker.find_next() {
                break;
            }
        }

        let _ = world;
    }

    /// `AiPlayer::FindMines` (`aiplayer.cpp:3442`) — create `TaskFindMines`.
    pub fn find_mines(&mut self, world: &mut GameState, unit: UnitId) {
        let Some(info) = world.unit(unit).cloned() else {
            return;
        };

        let has_mines = self.mine_map.iter().any(|value| *value > 0);

        if !has_mines {
            return;
        }

        if self.find_mines_task.is_none() {
            let kind = TaskKind::find_mines(info.grid);
            let task_id =
                world.ai_append_task(self.player_team, None, TASK_PRIORITY_FIND_MINES, kind);

            self.find_mines_task = Some(task_id);
        }
    }

    // -----------------------------------------------------------------------
    // Military value and attacks
    // -----------------------------------------------------------------------

    /// `AiPlayer::GetMilitaryValue` (`aiplayer.cpp:242`).
    pub fn get_military_value(&self, world: &GameState) -> i32 {
        let mut result = 0;

        for kind in [UnitListKind::MobileLandSea, UnitListKind::MobileAir] {
            for id in world.unit_list(kind) {
                let Some(unit) = world.unit(*id) else {
                    continue;
                };

                if unit.team == self.player_team
                    && unit.hits > 0
                    && unit
                        .base_values
                        .get_attribute(crate::unit_values::ATTRIB_ATTACK)
                        > 0
                {
                    result += normal_rate_build_cost(world, unit.team, unit.unit_type);
                }
            }
        }

        result
    }

    /// `AiPlayer::UpdatePriorityTasks` (`aiplayer.cpp:258`).
    pub fn update_priority_tasks(&mut self, world: &mut GameState) {
        let military_value = self.get_military_value(world);
        let task_limit = ((military_value + ATTACK_VALUE - 1) / ATTACK_VALUE)
            .clamp(ATTACK_TASK_LIMIT_MIN, ATTACK_TASK_LIMIT_MAX);

        self.attack_value_share = ATTACK_VALUE.max((2 * military_value) / task_limit.max(1));
        self.attack_tasks = vec![None; task_limit as usize];

        let ids: Vec<u32> = world
            .ai
            .task_manager
            .tasks()
            .iter()
            .filter(|task| {
                task.team == self.player_team && task.get_type() == super::task::TASK_TYPE_ATTACK
            })
            .map(|task| task.id)
            .collect();

        for task_id in ids {
            let Some(task) = world.ai.task_manager.task(task_id) else {
                continue;
            };

            let task_priority = task.base_priority;
            let mut index = 0;

            while index < task_limit
                && self.attack_tasks[index as usize]
                    .and_then(|slot| world.ai.task_manager.task(slot))
                    .map(|slot| slot.compare_priority(world, task_priority) <= 0)
                    .unwrap_or(false)
            {
                index += 1;
            }

            if index == task_limit {
                world.ai_task_remove_self(task_id);
            } else {
                if let Some(last) = self.attack_tasks[(task_limit - 1) as usize] {
                    world.ai_task_remove_self(last);
                }

                let mut slot = task_limit - 1;
                while slot > index {
                    self.attack_tasks[slot as usize] = self.attack_tasks[(slot - 1) as usize];
                    slot -= 1;
                }

                self.attack_tasks[index as usize] = Some(task_id);
            }
        }
    }

    /// `AiPlayer::DetermineAttack` (`aiplayer.cpp:194`).
    pub fn determine_attack(
        &mut self,
        world: &mut GameState,
        spotted_index: u32,
        mut task_priority: u16,
    ) {
        let already_tasked = self
            .spotted_units
            .get(spotted_index as usize)
            .map(|spotted| spotted.task.is_some())
            .unwrap_or(true);

        if already_tasked {
            return;
        }

        let Some((target_id, position, target_type)) =
            super::tasks::combat::spotted_target_for_ai(world, self.player_team, spotted_index)
        else {
            return;
        };

        let Some(target) = world.unit(target_id).cloned() else {
            return;
        };

        if task_priority > TASK_PRIORITY_ATTACK_MOBILE
            && super::attack::is_priority_target(world, &target, self.player_team)
        {
            task_priority = TASK_PRIORITY_ATTACK_PRIORITY_TARGET;
        }

        let target_flags =
            super::attack::get_target_flags(world, &target, &target, self.player_team) as u16;
        let target_task_priority = target_flags
            .wrapping_add(task_priority)
            .wrapping_add(super::task::TASK_PRIORITY_OFFSET_CENSUS_SLACK);

        let task_limit = self.attack_tasks.len() as i32;

        let mut task_index = 0;
        while task_index < task_limit && self.attack_tasks[task_index as usize].is_some() {
            task_index += 1;
        }

        if task_index == task_limit {
            task_index -= 1;
            while task_index >= 0
                && self.attack_tasks[task_index as usize]
                    .and_then(|slot| world.ai.task_manager.task(slot))
                    .map(|slot| slot.compare_priority(world, target_task_priority) <= 0)
                    .unwrap_or(false)
            {
                task_index -= 1;
            }
        }

        if task_index < 0 {
            return;
        }

        if let Some(previous) = self.attack_tasks[task_index as usize] {
            world.ai_task_remove_self(previous);
        }

        let kind = TaskKind::attack(target.team, position);
        let task_id = world.ai_append_task(self.player_team, None, task_priority, kind);

        self.attack_tasks[task_index as usize] = Some(task_id);

        if let Some(spot) = self.spotted_units.get_mut(spotted_index as usize) {
            spot.task = Some(task_id);
        }

        let _ = target_type;
    }

    /// `AiPlayer::GetTotalProjectedDamage` (`aiplayer.cpp:531`).
    pub fn get_total_projected_damage(
        world: &GameState,
        unit: &UnitInfo,
        caution_level: CautionLevel,
        team: TeamId,
        kind: UnitListKind,
    ) -> i32 {
        let mut result = 0;

        for id in world.unit_list(kind) {
            let Some(other) = world.unit(*id) else {
                continue;
            };
            if other.team == team {
                result += get_projected_damage(world, other, unit, caution_level);
            }
        }

        result
    }

    /// `AiPlayer::GetPredictedAttack` (`aiplayer.cpp:3307`).
    pub fn get_predicted_attack(
        &self,
        world: &GameState,
        unit: &UnitInfo,
        caution: CautionLevel,
    ) -> i32 {
        Self::get_total_projected_damage(
            world,
            unit,
            caution,
            self.player_team,
            UnitListKind::MobileLandSea,
        ) + Self::get_total_projected_damage(
            world,
            unit,
            caution,
            self.player_team,
            UnitListKind::MobileAir,
        )
    }

    // -----------------------------------------------------------------------
    // Strategy
    // -----------------------------------------------------------------------

    /// `AiPlayer::SelectStrategy` (`aiplayer.cpp:3719`) — **without map
    /// regeneration**: continents are computed by flood-fill over the ready `SurfaceMap`.
    ///
    /// Returns `false` if there is no suitable continent on the map: in C++ this
    /// switches the team to `TEAM_TYPE_NONE` (and `Ai_SetupStrategy` returns
    /// `false`).
    pub fn select_strategy(&mut self, world: &mut GameState) -> bool {
        let continents = land_continents(world);
        let mut strategy_scores = [0i32; AI_STRATEGY_MAX as usize];

        let opponent = world.ai.settings.opponent;

        let mut continent_size = 0;
        for continent in &continents {
            if continent.size > continent_size {
                continent_size = continent.size;
            }
        }

        if opponent >= Opponent::Average as u8 {
            let mut continent_score = 0;
            let mut largest = 0;

            for continent in &continents {
                if continent.size > largest {
                    largest = continent.size;
                }
            }

            for continent in &continents {
                if largest <= 80 || continent.size >= 80 {
                    if continent.viable {
                        continent_score += 1;
                    } else {
                        continent_score = 100;
                    }
                }
            }

            if continent_score == 1 {
                let score = match opponent {
                    value if value == Opponent::Average as u8 => 1,
                    value if value == Opponent::Expert as u8 => 6,
                    value if value == Opponent::Master as u8 => 12,
                    value if value == Opponent::God as u8 => 24,
                    _ => 0,
                };
                strategy_scores[AiStrategy::TankHorde as usize] = score;
            }
        }

        let mut continent_size = 0;
        for continent in &continents {
            if continent.viable && continent.size > continent_size {
                continent_size = continent.size;
            }
        }

        let viable: Vec<&Continent> = continents
            .iter()
            .filter(|continent| continent.viable)
            .collect();

        let isolated_continents =
            viable.iter().filter(|continent| continent.isolated).count() as i32;
        let close_proximity = viable
            .iter()
            .filter(|continent| continent.close_proximity)
            .count() as i32;
        let count = viable.len() as i32;

        strategy_scores[AiStrategy::Missiles as usize] = count;

        if opponent >= Opponent::Apprentice as u8 {
            strategy_scores[AiStrategy::Defensive as usize] = count;
        }

        if opponent >= Opponent::Average as u8
            && strategy_scores[AiStrategy::TankHorde as usize] == 0
        {
            strategy_scores[AiStrategy::Defensive as usize] = count;

            strategy_scores[AiStrategy::ScoutHorde as usize] =
                if isolated_continents > 0 { 1 } else { count };

            if opponent >= Opponent::Master as u8 {
                strategy_scores[AiStrategy::ScoutHorde as usize] *= 2;
            }

            if opponent >= Opponent::God as u8 {
                strategy_scores[AiStrategy::ScoutHorde as usize] *= 2;
            }
        }

        strategy_scores[AiStrategy::Air as usize] = close_proximity * 2;

        if opponent >= Opponent::Expert as u8 {
            strategy_scores[AiStrategy::Espionage as usize] = count;
        }

        strategy_scores[AiStrategy::Sea as usize] = isolated_continents * 2;
        strategy_scores[AiStrategy::FastAttack as usize] = count - close_proximity;
        strategy_scores[AiStrategy::CombinedArms as usize] = (count - close_proximity) * 2;

        self.strategy = AiStrategy::Random;

        // Teams choose different strategies (C++ excludes foreign ones).
        let mut potential_strategies =
            strategy_scores.iter().filter(|score| **score > 0).count() as i32;

        for team in 0..PLAYER_TEAM_COUNT {
            let team_id = TeamId(team as u8);
            if team_id == self.player_team {
                continue;
            }

            let other = &world.ai.players[team];
            if world.teams[team].team_type == TeamKind::Computer
                && other.strategy != AiStrategy::Random
            {
                potential_strategies -= 1;
            }
        }

        if potential_strategies > 0 {
            for team in 0..PLAYER_TEAM_COUNT {
                let team_id = TeamId(team as u8);
                if team_id == self.player_team {
                    continue;
                }

                if world.teams[team].team_type == TeamKind::Computer {
                    let other_strategy = world.ai.players[team].strategy;
                    if other_strategy != AiStrategy::Random {
                        strategy_scores[other_strategy as usize] = 0;
                    }
                }
            }
        }

        let total_score: i32 = strategy_scores.iter().sum();

        if total_score > 0 {
            // One RNG roll, as in C++ (`Randomizer_Generate(total_score)`).
            let mut roll = world.rng.generate_i32(total_score) + 1;
            let mut index = 0usize;

            loop {
                if index >= strategy_scores.len() {
                    break;
                }

                roll -= strategy_scores[index];

                if roll <= 0 {
                    break;
                }

                index += 1;
            }

            let index = index.min(AI_STRATEGY_MAX as usize - 1);
            self.strategy = AiStrategy::try_from(index as u8).unwrap_or(AiStrategy::CombinedArms);
        }

        let total_continent_size: i32 = viable.iter().map(|continent| continent.size).sum();

        if total_continent_size > 0 {
            // C++ rolls a continent and a landing point; in P6 (without setup)
            // only the strategy matters — we pin the target point to the map center.
            self.target_location = Point::new(MAP_WIDTH / 2, MAP_HEIGHT / 2);

            let clan = self.select_team_clan(world);
            world.teams[self.player_team.0 as usize].team_clan = clan;
            apply_clan(world, self.player_team, clan);

            if self.greenhouse_ratio < 0 {
                self.roll_greenhouse_ratio(world);
            }

            if self.minefield_density < 0 {
                self.roll_minefield_density(world);
            }

            true
        } else {
            world.teams[self.player_team.0 as usize].team_type = TeamKind::None;
            false
        }
    }

    /// `AiPlayer::SelectTeamClan` (`aiplayer.cpp:1109`).
    pub fn select_team_clan(&mut self, world: &mut GameState) -> TeamClan {
        let clan = match self.strategy {
            AiStrategy::Defensive => {
                if world.rng.generate_i32(7) != 0 {
                    TeamClan::SevenKnights
                } else {
                    TeamClan::AxisInc
                }
            }
            AiStrategy::Missiles => {
                if world.rng.generate_i32(7) != 0 {
                    TeamClan::AyersHand
                } else {
                    TeamClan::AxisInc
                }
            }
            AiStrategy::Air => {
                if world.rng.generate_i32(7) != 0 {
                    TeamClan::TheChosen
                } else {
                    TeamClan::AxisInc
                }
            }
            AiStrategy::Sea => {
                if world.rng.generate_i32(7) != 0 {
                    TeamClan::CrimsonPath
                } else {
                    TeamClan::AxisInc
                }
            }
            AiStrategy::ScoutHorde => TeamClan::SacredEight,
            AiStrategy::TankHorde => TeamClan::Musashi,
            AiStrategy::FastAttack => TeamClan::SacredEight,
            AiStrategy::CombinedArms => {
                if world.rng.generate_i32(7) != 0 {
                    let offset = world.rng.generate_i32(3) as u8;
                    match offset {
                        0 => TeamClan::VonGriffin,
                        1 => TeamClan::AyersHand,
                        _ => TeamClan::Musashi,
                    }
                } else {
                    TeamClan::AxisInc
                }
            }
            AiStrategy::Espionage => {
                if world.rng.generate_i32(7) != 0 {
                    TeamClan::CrimsonPath
                } else {
                    TeamClan::VonGriffin
                }
            }
            _ => TeamClan::Random,
        };

        // `menu_team_clan_setting` — an entry setting; in max_core it is stored in
        // `TeamInfo::team_clan` (P3) and applied by the caller layer.
        clan
    }

    /// `AiPlayer::RollGreenhouseRatio` (`aiplayer.cpp:1191`).
    pub fn roll_greenhouse_ratio(&mut self, world: &mut GameState) {
        self.greenhouse_ratio = match self.strategy {
            AiStrategy::Defensive | AiStrategy::Air | AiStrategy::Sea => {
                world.rng.generate_i32(6) as i16 + 7
            }
            AiStrategy::ScoutHorde | AiStrategy::TankHorde => 5,
            AiStrategy::Missiles
            | AiStrategy::FastAttack
            | AiStrategy::CombinedArms
            | AiStrategy::Espionage => world.rng.generate_i32(4) as i16 + 4,
            _ => 0,
        };
    }

    /// `AiPlayer::RollMinefieldDensity` (`aiplayer.cpp:1217`).
    pub fn roll_minefield_density(&mut self, world: &mut GameState) {
        if world.ai.settings.opponent >= Opponent::Average as u8 {
            self.minefield_density = match self.strategy {
                AiStrategy::Air | AiStrategy::Sea => {
                    if world.rng.generate_i32(100) + 1 < 50 {
                        let first = world.rng.generate_i32(5);
                        let second = world.rng.generate_i32(5);
                        ((first + second) * 6) as i16
                    } else {
                        0
                    }
                }
                AiStrategy::Defensive => {
                    let first = world.rng.generate_i32(7);
                    let second = world.rng.generate_i32(7);
                    ((first + second) * 12) as i16
                }
                AiStrategy::ScoutHorde | AiStrategy::TankHorde => 0,
                AiStrategy::Missiles
                | AiStrategy::FastAttack
                | AiStrategy::CombinedArms
                | AiStrategy::Espionage => {
                    if world.rng.generate_i32(100) + 1 < 25 {
                        let first = world.rng.generate_i32(5);
                        let second = world.rng.generate_i32(5);
                        ((first + second) * 6) as i16
                    } else {
                        0
                    }
                }
                _ => 0,
            };
        } else {
            self.minefield_density = 0;
        }
    }

    // -----------------------------------------------------------------------
    // Turn
    // -----------------------------------------------------------------------

    /// `AiPlayer::BeginTurn` (`aiplayer.cpp:2213`).
    pub fn begin_turn(&mut self, world: &mut GameState) {
        if world.teams[self.player_team.0 as usize].team_type != TeamKind::Computer {
            return;
        }

        self.invalidate_threat_maps();
        self.end_turn_retry_counter = 0;
        self.transport_orders.clear();

        let position = team_cluster_coordinates(world, self.player_team);
        self.determine_target_location(world, position);
        self.update_priority_tasks(world);

        if self.need_init {
            self.init_tasks(world, position);
            self.need_init = false;
        }

        // Attacks on spotted units.
        let spotted: Vec<usize> = (0..self.spotted_units.len()).collect();
        for index in spotted {
            let has_task = self
                .spotted_units
                .get(index)
                .map(|spotted| spotted.task.is_some())
                .unwrap_or(true);

            if has_task {
                continue;
            }

            let Some(target_unit) = self.spotted_units[index]
                .unit
                .and_then(|unit_id| world.unit(unit_id).cloned())
            else {
                continue;
            };

            if super::attack::is_within_reach(world, &target_unit, self.player_team) {
                self.determine_attack(world, index as u32, TASK_PRIORITY_ATTACK_MOBILE);
            } else if self.is_enemy_team(world, target_unit.team)
                || target_unit.team == TeamId::ALIEN
            {
                if (target_unit.unit_type == UnitType::Constructor
                    && target_unit.orders == UnitOrder::Build)
                    || Self::is_key_facility(target_unit.unit_type)
                    || (target_unit.team == TeamId::ALIEN && target_unit.environment().is_some())
                {
                    self.determine_attack(world, index as u32, TASK_PRIORITY_ATTACK_DEFAULT);
                }
            }
        }

        // Powering buildings (structurally, as in C++: ore stations always ON).
        let mut fuel_reserves = 0;
        for id in world.unit_list(UnitListKind::Stationary) {
            let Some(unit) = world.unit(*id) else {
                continue;
            };
            if unit.team == self.player_team && unit.unit_type == UnitType::FuelStorage {
                fuel_reserves += i32::from(unit.storage);
            }
        }

        let mut power_on = Vec::new();
        for id in world.unit_list(UnitListKind::Stationary) {
            let Some(unit) = world.unit(*id) else {
                continue;
            };
            if unit.team != self.player_team {
                continue;
            }

            let should_power = match unit.unit_type {
                UnitType::MiningStation => {
                    unit.orders != UnitOrder::PowerOn && unit.orders != UnitOrder::Idle
                }
                UnitType::GoldRefinery | UnitType::EcoSphere | UnitType::ResearchCentre => {
                    fuel_reserves >= 10
                        && unit.orders != UnitOrder::PowerOn
                        && unit.orders != UnitOrder::Idle
                }
                _ => false,
            };

            if should_power {
                power_on.push(*id);
            }
        }

        for id in power_on {
            world.set_order(id, UnitOrder::PowerOn, UnitOrderState::Init);
        }

        self.register_idle_units(world);

        // Reminders: movement and attack over mobile lists in C++ order.
        for kind in [UnitListKind::MobileLandSea, UnitListKind::MobileAir] {
            let ids: Vec<UnitId> = world.unit_list(kind).to_vec();
            for id in ids {
                let Some(unit) = world.unit(id).cloned() else {
                    continue;
                };
                if unit.team != self.player_team {
                    continue;
                }

                if world.ai.task_manager.unit_task(id).is_some() {
                    world.ai_remind_move_finished(id, false);
                }

                if unit.shots > 0 {
                    world.ai_append_reminder(super::reminders::Reminder::Attack { unit: id }, true);
                }
            }
        }

        {
            let mut tm = std::mem::take(&mut world.ai.task_manager);
            tm.begin_turn(world, self.player_team);
            world.ai.task_manager = tm;
        }
    }

    /// First-turn task initialization (`need_init`).
    fn init_tasks(&mut self, world: &mut GameState, position: Point) {
        let team = self.player_team;

        // `TaskCheckAssaults`.
        let kind = TaskKind::CheckAssaults {
            index: 0,
            active: false,
        };
        let task_id = world.ai_append_task(team, None, TASK_PRIORITY_CHECK_ASSAULTS, kind);
        self.check_assaults_task = Some(task_id);

        // Clan strategy (C++ `strategy == RANDOM` → by clan).
        if self.strategy == AiStrategy::Random {
            self.strategy = match world.teams[team.0 as usize].team_clan {
                TeamClan::TheChosen => AiStrategy::Air,
                TeamClan::CrimsonPath => AiStrategy::Sea,
                TeamClan::AyersHand => AiStrategy::Missiles,
                TeamClan::SacredEight => AiStrategy::FastAttack,
                TeamClan::SevenKnights => AiStrategy::Defensive,
                _ => AiStrategy::CombinedArms,
            };
        }

        if self.greenhouse_ratio < 0 {
            self.roll_greenhouse_ratio(world);
        }

        if self.minefield_density < 0 {
            self.roll_minefield_density(world);
        }

        // `info_map` from the team heat map.
        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                if world.ai_heat_map_complete(team, i32::from(x), i32::from(y)) > 0 {
                    if let Some(index) = Self::map_index(Point::new(x, y)) {
                        self.info_map[index] = INFO_MAP_EXPLORED;
                    }
                }
            }
        }

        // Reconnaissance and survey.
        if position.x != 0 || position.y != 0 {
            let survey_kind = TaskKind::survey(position);
            world.ai_append_task(
                team,
                None,
                super::task::TASK_PRIORITY_AUTO_SURVEY,
                survey_kind,
            );

            let explore_kind = TaskKind::explore(position);
            world.ai_append_task(team, None, TASK_PRIORITY_EXPLORE, explore_kind);
        }

        // Construction manager.
        let manage_kind = TaskKind::manage_buildings(position);
        world.ai_append_task(team, None, TASK_PRIORITY_MANAGE_BUILDINGS, manage_kind);

        // Mine assistant and terrain update.
        let mine_kind = TaskKind::mine_assistant();
        world.ai_append_task(team, None, TASK_PRIORITY_FRONTIER, mine_kind);

        let terrain_kind = TaskKind::update_terrain();
        world.ai_append_task(team, None, super::task::TASK_PRIORITY_EMERGENCY, terrain_kind);

        if world.ai.settings.opponent >= Opponent::Apprentice as u8 {
            let scavenge_kind = TaskKind::scavenge();
            world.ai_append_task(team, None, TASK_PRIORITY_SCAVENGE, scavenge_kind);

            let assist_kind = TaskKind::AssistMove {
                transporters: Vec::new(),
            };
            world.ai_append_task(team, None, TASK_PRIORITY_ASSIST_MOVE, assist_kind);
        }

        // Defense: a reserve for every team.
        if self.defense_reserve_task.is_none() && position.x != 0 {
            let kind = TaskKind::DefenseReserve {
                units: Vec::new(),
                site: position,
            };
            let task_id =
                world.ai_append_task(team, None, super::task::TASK_PRIORITY_DEFENSE_RESERVE, kind);
            self.defense_reserve_task = Some(task_id);
        }

        // Vehicle escorts (C++ BeginTurn).
        let mut escorts = Vec::new();
        for kind_list in [UnitListKind::MobileAir, UnitListKind::MobileLandSea] {
            for id in world.unit_list(kind_list) {
                let Some(unit) = world.unit(*id) else {
                    continue;
                };
                if unit.team != team {
                    continue;
                }

                let escort_type = match unit.unit_type {
                    UnitType::Awac | UnitType::Bomber => Some(UnitType::Fighter),
                    UnitType::MissileCrawler | UnitType::Scanner => Some(UnitType::Tank),
                    UnitType::MobileAntiAir | UnitType::Escort => Some(UnitType::Fighter),
                    _ => None,
                };

                if let Some(escort_type) = escort_type {
                    escorts.push((*id, escort_type));
                }

                if unit
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_AMMO)
                    > 0
                {
                    self.add_military_unit(world, *id);
                }
            }
        }

        for (target, unit_type) in escorts {
            let kind = TaskKind::Escort {
                target: Some(target),
                escort: None,
                unit_type,
                is_escort_requested: false,
            };
            world.ai_append_task(team, None, TASK_PRIORITY_ESCORT, kind);
        }

        // Military units of stationary buildings.
        for id in world.unit_list(UnitListKind::Stationary) {
            let Some(unit) = world.unit(*id) else {
                continue;
            };
            if unit.team == team
                && unit
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_AMMO)
                    > 0
            {
                self.add_military_unit(world, *id);
            }
        }

        // Enemy key objects → attack.
        let spotted: Vec<usize> = (0..self.spotted_units.len()).collect();
        for index in spotted {
            let Some(unit_id) = self.spotted_units[index].unit else {
                continue;
            };
            let Some(unit) = world.unit(unit_id) else {
                continue;
            };

            if Self::is_key_facility(unit.unit_type) && self.is_enemy_team(world, unit.team) {
                self.determine_attack(world, index as u32, TASK_PRIORITY_ATTACK_DEFAULT);
            }
        }

        // Minefields.
        if self.find_mines_task.is_none() && self.mine_map.iter().any(|value| *value > 0) {
            let kind = TaskKind::find_mines(position);
            let task_id = world.ai_append_task(team, None, TASK_PRIORITY_FIND_MINES, kind);
            self.find_mines_task = Some(task_id);
        }

        if self.place_mines_task.is_none() {
            let kind = TaskKind::place_mines();
            let task_id = world.ai_append_task(team, None, TASK_PRIORITY_FRONTIER, kind);
            self.place_mines_task = Some(task_id);
        }
    }

    /// `AiPlayer::RegisterIdleUnits` (`aiplayer.cpp:504`).
    fn register_idle_units(&mut self, world: &mut GameState) {
        let mut idle = Vec::new();

        for kind in [UnitListKind::MobileLandSea, UnitListKind::MobileAir] {
            for id in world.unit_list(kind) {
                let Some(unit) = world.unit(*id) else {
                    continue;
                };
                if unit.team != self.player_team || unit.hits == 0 {
                    continue;
                }

                if world.ai.task_manager.unit_task(*id).is_none()
                    && (unit.orders == UnitOrder::Await || unit.orders == UnitOrder::Sentry)
                {
                    idle.push(*id);
                }
            }
        }

        for unit in idle {
            world.ai_append_reminder(super::reminders::Reminder::Available { unit }, false);
        }
    }

    /// `AiPlayer::DetermineTargetLocation` (`aiplayer.cpp:298`).
    pub fn determine_target_location(&mut self, world: &GameState, position: Point) {
        if self.spotted_units.is_empty() {
            self.target_location = position;
            return;
        }

        let mut best: Option<(i32, Point)> = None;

        for spotted in &self.spotted_units {
            let Some(unit_id) = spotted.unit else {
                continue;
            };
            let Some(unit) = world.unit(unit_id) else {
                continue;
            };

            if unit.hits == 0 || !self.is_enemy_team(world, unit.team) {
                continue;
            }

            let last_position = spotted.last_position_of(world);
            let distance = crate::access::squared_distance(position, last_position);

            if best
                .map(|(best_distance, _)| distance < best_distance)
                .unwrap_or(true)
            {
                best = Some((distance, last_position));
            }
        }

        self.target_location = best.map(|(_, site)| site).unwrap_or(position);
    }

    /// `AiPlayer::CheckEndTurn` (`aiplayer.cpp:2828`).
    ///
    /// `tasks_pending` — the same flag as in C++: `Ai_SetTasksPendingFlag`
    /// is set from `UnitsManager_ProcessOrder*`/`UnitInfo::ExecuteMove` on
    /// every tick in which the unit actually does something, and is reset at
    /// the start of `UnitsManager_ProcessOrders` (`Ai_ClearTasksPendingFlags`).
    /// Therefore a unit that ran out of speed does not hold the turn.
    pub fn check_end_turn(&mut self, world: &mut GameState) -> bool {
        if self.need_init {
            self.begin_turn(world);
            return true;
        }

        if self.tasks_pending {
            self.end_turn_retry_counter = 0;
            return false;
        }

        if self.end_turn_retry_counter == 0 {
            world
                .ai
                .task_manager
                .mark_tasks_for_processing(self.player_team);
        }

        self.end_turn_retry_counter = self.end_turn_retry_counter.saturating_add(1);

        if self.end_turn_retry_counter < 10 {
            return false;
        }

        if self.end_turn_retry_counter != 10 || self.are_actions_pending(world) {
            match self.end_turn_retry_counter {
                20 => {
                    self.check_attacks(world);
                    true
                }
                30 => {
                    self.register_ready_and_able_units(world, UnitListKind::MobileLandSea);
                    self.register_ready_and_able_units(world, UnitListKind::MobileAir);
                    true
                }
                60 => {
                    world.with_task_manager(|task_manager, world| {
                        task_manager.end_turn(world, self.player_team);
                    });
                    true
                }
                value if value > 120 => false,
                _ => false,
            }
        } else {
            world.with_task_manager(|task_manager, world| {
                task_manager.end_turn(world, self.player_team);
            });
            true
        }
    }

    /// `AiPlayer::AreActionsPending` (`aiplayer.cpp:489`) — simplified: there are
    /// reminders or unfinished orders.
    fn are_actions_pending(&self, world: &GameState) -> bool {
        world.ai.task_manager.reminders_count() > 0
            || world
                .unit_list(UnitListKind::MobileLandSea)
                .iter()
                .chain(world.unit_list(UnitListKind::MobileAir).iter())
                .any(|id| {
                    world
                        .unit(*id)
                        .map(|unit| {
                            unit.team == self.player_team
                                && matches!(
                                    unit.state,
                                    crate::enums::UnitOrderState::InProgress
                                        | crate::enums::UnitOrderState::InTransition
                                )
                        })
                        .unwrap_or(false)
                })
    }

    /// `AiPlayer::RegisterReadyAndAbleUnits` (`aiplayer.cpp:481`).
    fn register_ready_and_able_units(&self, world: &mut GameState, kind: UnitListKind) {
        let ids: Vec<UnitId> = world.unit_list(kind).to_vec();

        for id in ids {
            let Some(unit) = world.unit(id).cloned() else {
                continue;
            };
            if unit.team != self.player_team || unit.hits == 0 || unit.speed == 0 {
                continue;
            }

            if super::task::task_is_ready_to_take_orders(world, id) {
                world.ai_append_reminder(super::reminders::Reminder::Available { unit: id }, true);
            }
        }
    }

    /// `AiPlayer::CheckAttacks` (`aiplayer.cpp:452`) — structurally: recompute
    /// the rating of attack tasks.
    fn check_attacks(&mut self, world: &mut GameState) {
        self.update_priority_tasks(world);
    }

    /// `AiPlayer::CreateBuilding` (`aiplayer.cpp:2897`).
    pub fn create_building(
        &mut self,
        world: &mut GameState,
        unit_type: UnitType,
        position: Point,
        task: Option<u32>,
    ) -> bool {
        let Some(manager) = self.find_manager(world, position) else {
            return false;
        };

        let priority = task
            .and_then(|task_id| world.ai.task_manager.task(task_id))
            .map(|task| task.base_priority)
            .unwrap_or(TASK_PRIORITY_MANAGE_BUILDINGS);

        let mut kind = world
            .ai
            .task_manager
            .task(manager)
            .map(|task| task.kind.clone())
            .unwrap_or_else(|| TaskKind::manage_buildings(position));

        let header = super::TaskHeader {
            id: manager,
            team: self.player_team,
            base_priority: priority,
            parent: None,
            needs_processing: true,
        };

        let mut tm = std::mem::take(&mut world.ai.task_manager);
        let result = {
            let mut ctx = TaskCtx::new(&mut tm, world);
            super::tasks::build::manage_create_building(
                &mut kind, &mut ctx, header, unit_type, priority,
            )
            .is_some()
        };
        world.ai.task_manager = tm;

        result
    }

    /// `AiPlayer::FindManager` (`aiplayer.cpp:2141`) — the nearest
    /// `TaskManageBuildings` of the team.
    pub fn find_manager(&self, world: &GameState, site: Point) -> Option<u32> {
        let mut best: Option<(i32, u32)> = None;

        for task in world.ai.task_manager.tasks() {
            if task.team != self.player_team {
                continue;
            }

            if let TaskKind::ManageBuildings { building_site, .. } = &task.kind {
                let distance = crate::access::squared_distance(*building_site, site);

                if best
                    .map(|(best_distance, _)| distance < best_distance)
                    .unwrap_or(true)
                {
                    best = Some((distance, task.id));
                }
            }
        }

        best.map(|(_, id)| id)
    }

    /// `AiPlayer::ShouldUpgradeUnit` (`aiplayer.cpp:3519`) — structurally: the unit is not
    /// in perfect condition and the upgrade is not yet maxed.
    pub fn should_upgrade_unit(&self, world: &GameState, unit: &UnitInfo) -> bool {
        if unit.flags & unit_flags::REGENERATING_UNIT != 0 {
            return false;
        }

        let targets =
            world.team_units[unit.team.0 as usize].current_unit_values(unit.unit_type as u16);

        targets.get_attribute(crate::unit_values::ATTRIB_HITS)
            > unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_HITS)
            || targets.get_attribute(crate::unit_values::ATTRIB_ATTACK)
                > unit
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_ATTACK)
    }

    /// `AiPlayer::InvalidateThreatMaps` (`aiplayer.cpp:620`).
    pub fn invalidate_threat_maps(&mut self) {
        for map in &mut self.threat_maps {
            map.set_risk_level(0);
        }
    }

    /// `AiPlayer::MatchPath` (`aiplayer.cpp:3660`) — simplified: the request
    /// is accepted if the path leads to the task's target.
    pub fn match_path(&self, world: &GameState, request: &crate::pathfinding::PathRequest) -> bool {
        let Some(unit) = world.unit(request.unit) else {
            return false;
        };

        let _ = unit;
        let _ = self;

        true
    }
}

// ---------------------------------------------------------------------------
// Free `AiPlayer_*` functions
// ---------------------------------------------------------------------------

/// `AiPlayer_CalculateProjectedDamage` (`aiplayer.cpp`): the damage the target
/// will deal back to a friendly unit.
pub fn calculate_projected_damage(
    world: &GameState,
    friendly: &UnitInfo,
    enemy: &UnitInfo,
    caution_level: CautionLevel,
) -> i32 {
    let _ = caution_level;

    if !crate::access::is_valid_attack_target(world, enemy, friendly) {
        return 0;
    }

    let rounds = enemy
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
    let damage = crate::combat::get_attack_damage(enemy, friendly, 0);

    rounds * damage
}

/// `AiPlayer_GetProjectedDamage` — expected damage from `friendly` to `enemy`.
pub fn get_projected_damage(
    world: &GameState,
    friendly: &UnitInfo,
    enemy: &UnitInfo,
    caution_level: CautionLevel,
) -> i32 {
    let _ = caution_level;

    if !crate::access::is_valid_attack_target(world, friendly, enemy) {
        return 0;
    }

    let rounds = friendly
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
    let damage = crate::combat::get_attack_damage(friendly, enemy, 0);

    rounds * damage
}

/// `AiPlayer::GetPredictedAttack` for the team.
pub fn get_predicted_attack(
    world: &GameState,
    unit: &UnitInfo,
    caution_level: CautionLevel,
) -> i32 {
    AiPlayer::get_total_projected_damage(
        world,
        unit,
        caution_level,
        unit.team,
        UnitListKind::MobileLandSea,
    ) + AiPlayer::get_total_projected_damage(
        world,
        unit,
        caution_level,
        unit.team,
        UnitListKind::MobileAir,
    )
}

/// `AiPlayer::GetDamagePotential` (`aiplayer.cpp:3345`).
pub fn get_damage_potential(
    world: &GameState,
    team: TeamId,
    unit: &UnitInfo,
    site: Point,
    caution_level: CautionLevel,
) -> i32 {
    if caution_level == CautionLevel::None {
        return 0;
    }

    let map = get_damage_potential_map(world, team, unit, caution_level);

    match map {
        Some(map) => {
            let armor = unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_ARMOR);
            map.damage_at(i32::from(site.x), i32::from(site.y))
                + map.shots_at(i32::from(site.x), i32::from(site.y))
                    * (i32::from(map.armor) - armor)
        }
        None => 0,
    }
}

/// `AiPlayer::GetDamagePotentialMap` (`aiplayer.cpp:3312`) — damage map for
/// the unit risk type and caution level.
pub fn get_damage_potential_map(
    world: &GameState,
    team: TeamId,
    unit: &UnitInfo,
    caution_level: CautionLevel,
) -> Option<ThreatMap> {
    let risk_level = ThreatMap::get_risk_level_for_type(unit.unit_type);
    let mut map = build_threat_map(world, team, risk_level, caution_level);

    map.update(
        unit.base_values
            .get_attribute(crate::unit_values::ATTRIB_ARMOR),
    );

    Some(map)
}

/// `AiPlayer::GetThreatMap` (`aiplayer.cpp:761`) — building the threat map.
///
/// C++ caches up to 10 maps by `(risk_level, caution_level)`; here the map
/// is built immediately, the traversal order — `spotted_units` (detection order) and
/// the five unit lists, as in C++.
pub fn build_threat_map(
    world: &GameState,
    team: TeamId,
    risk_level: i32,
    caution_level: CautionLevel,
) -> ThreatMap {
    let mut map = ThreatMap::new();
    map.team = team.0 as i16;
    map.risk_level = risk_level as u8;
    map.caution_level = caution_level as u8;

    let mut air_map = ThreatMap::new();

    if caution_level as u8 > CautionLevel::AvoidReactionFire as u8 {
        determine_defenses(
            world,
            team,
            UnitListKind::MobileAir,
            &mut air_map.damage_potential_map,
        );
        determine_defenses(
            world,
            team,
            UnitListKind::MobileLandSea,
            &mut map.damage_potential_map,
        );
    }

    let risk_group = match risk_level {
        2 => Some(UnitType::Surveyor),
        3 => Some(UnitType::Fighter),
        4 | 5 => Some(UnitType::Infiltrator),
        6 => Some(UnitType::Submarine),
        7 => Some(UnitType::ArmouredPersonnelCarrier),
        _ => Some(UnitType::Tank),
    };

    let player = &world.ai.players[team.0 as usize];

    for spotted in &player.spotted_units {
        let Some(unit_id) = spotted.unit else {
            continue;
        };
        let Some(unit) = world.unit(unit_id) else {
            continue;
        };

        if is_able_to_attack(world, unit, risk_group, team) {
            let position = spotted.last_position_of(world);
            determine_threats(
                world,
                team,
                unit,
                position,
                caution_level,
                &mut air_map,
                &mut map,
            );
        }
    }

    if caution_level as u8 > CautionLevel::AvoidReactionFire as u8 {
        normalize_threat_map(&mut map);
        normalize_threat_map(&mut air_map);
        sum_up_maps(&mut map.damage_potential_map, &air_map.damage_potential_map);
        sum_up_maps(&mut map.shots_map, &air_map.shots_map);
    }

    // Visibility mask over team heat maps.
    let mut visible = [false; MAP_TILES];

    for other_team in 0..PLAYER_TEAM_COUNT {
        let team_id = TeamId(other_team as u8);
        if team_id == team || world.teams[other_team].team_type == TeamKind::None {
            continue;
        }

        let heat = &world.ai.heat_maps[other_team];

        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                let index = y as usize * MAP_WIDTH as usize + x as usize;

                let is_visible = match risk_level {
                    6 => heat.get_stealth_sea(i32::from(x), i32::from(y)) > 0,
                    4 | 5 => heat.get_stealth_land(i32::from(x), i32::from(y)) > 0,
                    7 => {
                        crate::access::surface_type(world, Point::new(x, y))
                            == crate::access::SURFACE_TYPE_WATER
                            && heat.get_stealth_sea(i32::from(x), i32::from(y)) == 0
                    }
                    _ => heat.get_complete(i32::from(x), i32::from(y)) > 0,
                };

                if is_visible {
                    visible[index] = true;
                }
            }
        }
    }

    for index in 0..MAP_TILES {
        if !visible[index] {
            map.damage_potential_map[index] = 0;
        }
    }

    // Team mines.
    if !matches!(risk_level, 2 | 3) {
        let player = &world.ai.players[team.0 as usize];

        for index in 0..MAP_TILES {
            let density = i16::from(player.mine_map[index]);
            map.damage_potential_map[index] = map.damage_potential_map[index].wrapping_add(density);

            if density > 0 {
                map.shots_map[index] = map.shots_map[index].wrapping_add(1);
            }
        }
    }

    if risk_level != 3 {
        let player = &world.ai.players[team.0 as usize];
        for spotted in &player.spotted_units {
            let Some(unit_id) = spotted.unit else {
                continue;
            };
            let Some(unit) = world.unit(unit_id) else {
                continue;
            };

            if matches!(unit.unit_type, UnitType::LandMine | UnitType::SeaMine) {
                let index = unit.grid.y as usize * MAP_WIDTH as usize + unit.grid.x as usize;
                let attack = unit
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_ATTACK)
                    as i16;
                map.damage_potential_map[index] =
                    map.damage_potential_map[index].wrapping_add(attack);
                map.shots_map[index] = map.shots_map[index].wrapping_add(1);
            }
        }
    }

    map
}

/// `AiPlayer::DetermineDefenses` (`aiplayer.cpp:626`).
fn determine_defenses(world: &GameState, team: TeamId, kind: UnitListKind, map: &mut [i16]) {
    for id in world.unit_list(kind) {
        let Some(unit) = world.unit(*id) else {
            continue;
        };
        if unit.team != team || unit.shots == 0 {
            continue;
        }

        if unit.orders == UnitOrder::Idle || unit.orders == UnitOrder::Disable {
            continue;
        }

        let mut range = unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_RANGE);

        if unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE)
            == 0
        {
            range -= 3;
        }

        if range > 0 {
            let attack = -unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_ATTACK)
                * i32::from(unit.shots);
            update_map(map, unit.grid, range, attack as i16, false);
        }
    }
}

/// `AiPlayer::DetermineThreats` (`aiplayer.cpp:644`).
fn determine_threats(
    world: &GameState,
    team: TeamId,
    unit: &UnitInfo,
    position: Point,
    caution_level: CautionLevel,
    air_map: &mut ThreatMap,
    ground_map: &mut ThreatMap,
) {
    let is_air = unit.is_air();
    let _ = team;

    let base = unit.base_values;
    let unit_range = base.get_attribute(crate::unit_values::ATTRIB_RANGE);
    let attack_range = unit_range + base.get_attribute(crate::unit_values::ATTRIB_ATTACK_RADIUS);
    let unit_attack = base.get_attribute(crate::unit_values::ATTRIB_ATTACK);
    let unit_shots = base.get_attribute(crate::unit_values::ATTRIB_ROUNDS);
    let mut ammo = i32::from(unit.ammo);
    let speed = base.get_attribute(crate::unit_values::ATTRIB_SPEED);
    let move_and_fire = base.get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE);

    let target_map = if is_air { air_map } else { ground_map };

    match caution_level {
        CautionLevel::AvoidNextTurnsFire => {
            if move_and_fire != 0 {
                update_threat_maps(
                    target_map,
                    world,
                    unit,
                    position,
                    unit_range,
                    unit_attack,
                    i32::from(unit.shots),
                    &mut ammo,
                    true,
                );

                let movement_range = speed / 2;

                update_threat_maps(
                    target_map,
                    world,
                    unit,
                    position,
                    attack_range + movement_range,
                    unit_attack,
                    unit_shots,
                    &mut ammo,
                    false,
                );
            } else {
                update_threat_maps(
                    target_map,
                    world,
                    unit,
                    position,
                    unit_range,
                    unit_attack,
                    i32::from(unit.shots) + 1,
                    &mut ammo,
                    true,
                );

                let mut remaining = unit_shots;
                loop {
                    remaining -= 1;

                    if remaining <= 0 {
                        break;
                    }

                    let movement_range = ((speed + 1) * remaining) / (remaining + 1);

                    update_threat_maps(
                        target_map,
                        world,
                        unit,
                        position,
                        movement_range + attack_range,
                        unit_attack,
                        1,
                        &mut ammo,
                        false,
                    );
                }
            }
        }
        CautionLevel::AvoidAllDamage => {
            update_threat_maps(
                target_map,
                world,
                unit,
                position,
                attack_range,
                unit_attack,
                unit_shots,
                &mut ammo,
                true,
            );

            let movement_range = if move_and_fire != 0 {
                speed / 2
            } else {
                if unit_shots == 0 {
                    0
                } else {
                    ((speed + 1) * (unit_shots - 1)) / unit_shots
                }
            };

            if movement_range > 0 {
                update_threat_maps(
                    target_map,
                    world,
                    unit,
                    position,
                    movement_range + attack_range,
                    unit_attack,
                    unit_shots,
                    &mut ammo,
                    false,
                );
            }
        }
        CautionLevel::AvoidReactionFire => {
            update_threat_maps(
                target_map,
                world,
                unit,
                position,
                unit_range,
                unit_attack,
                i32::from(unit.shots),
                &mut ammo,
                false,
            );
        }
        CautionLevel::None => {}
    }
}

/// `AiPlayer::UpdateThreatMaps` (`aiplayer.cpp:590`).
fn update_threat_maps(
    map: &mut ThreatMap,
    world: &GameState,
    unit: &UnitInfo,
    position: Point,
    range: i32,
    attack: i32,
    shots: i32,
    ammo: &mut i32,
    normalize: bool,
) {
    let shots = if shots > *ammo { *ammo } else { shots };
    *ammo -= shots;

    let damage_potential = attack * shots;

    if damage_potential <= 0 {
        return;
    }

    if unit.unit_type == UnitType::Submarine || unit.unit_type == UnitType::Corvette {
        let mut walker = super::threat::ZoneWalker::new(position, range);

        loop {
            let site = walker.current_location();
            let surface = crate::access::surface_type(world, site);

            if surface & super::threat::SURFACE_WATER_OR_COAST != 0 {
                let index = site.y as usize * MAP_WIDTH as usize + site.x as usize;
                map.damage_potential_map[index] =
                    map.damage_potential_map[index].wrapping_add(damage_potential as i16);
                map.shots_map[index] = map.shots_map[index].wrapping_add(shots as i16);
            }

            if !walker.find_next() {
                break;
            }
        }
    } else {
        update_map(
            &mut map.damage_potential_map,
            position,
            range,
            damage_potential as i16,
            normalize,
        );
        update_map(&mut map.shots_map, position, range, shots as i16, false);
    }
}

/// `AiPlayer::UpdateMap` (`aiplayer.cpp:544`) — overlaying the damage square.
fn update_map(
    map: &mut [i16],
    position: Point,
    range: i32,
    damage_potential: i16,
    normalize: bool,
) {
    let mut walker = super::threat::ZoneWalker::new(position, range);

    loop {
        let site = walker.current_location();
        let index = site.y as usize * MAP_WIDTH as usize + site.x as usize;

        if normalize && map[index] < 0 {
            map[index] = 0;
        }

        map[index] = map[index].wrapping_add(damage_potential);

        if !walker.find_next() {
            break;
        }
    }
}

/// `AiPlayer::SumUpMaps` (`aiplayer.cpp:717`).
fn sum_up_maps(map1: &mut [i16], map2: &[i16]) {
    for index in 0..map1.len().min(map2.len()) {
        map1[index] = map1[index].wrapping_add(map2[index]);
    }
}

/// `AiPlayer::NormalizeThreatMap` (`aiplayer.cpp:725`).
fn normalize_threat_map(map: &mut ThreatMap) {
    for index in 0..map.damage_potential_map.len() {
        if map.damage_potential_map[index] < 0 {
            map.damage_potential_map[index] = 0;
            map.shots_map[index] = 0;
        }
    }
}

/// `AiPlayer::IsAbleToAttack` (`aiplayer.cpp:736`).
fn is_able_to_attack(
    world: &GameState,
    attacker: &UnitInfo,
    target_type: Option<UnitType>,
    team: TeamId,
) -> bool {
    let Some(target_type) = target_type else {
        return false;
    };

    if attacker.ammo == 0
        || attacker.orders == UnitOrder::Disable
        || attacker.orders == UnitOrder::Idle
        || attacker.hits == 0
        || !crate::access::is_valid_attack_target_type(attacker.unit_type, target_type)
    {
        return false;
    }

    if matches!(attacker.unit_type, UnitType::LandMine | UnitType::SeaMine) {
        return false;
    }

    if matches!(
        attacker.unit_type,
        UnitType::Infiltrator | UnitType::Submarine
    ) && !attacker.is_visible_to_team(team)
    {
        return false;
    }

    let _ = world;
    true
}

/// `AiPlayer::GetFilteredWeightTable` (`aiplayer.cpp:3387`).
pub fn get_filtered_weight_table(
    air_table: &WeightTable,
    ground_table: &WeightTable,
    flags: u32,
) -> WeightTable {
    let mut result = WeightTable::new(0);

    if flags & unit_flags::MOBILE_AIR_UNIT != 0 {
        result.merge(air_table);
    }

    if flags & (unit_flags::MOBILE_LAND_UNIT | unit_flags::MOBILE_SEA_UNIT) != 0 {
        result.merge(ground_table);
    }

    result
}

/// `AiPlayer::ChooseUpgrade` (`aiplayer.cpp:4194`) — structurally: choose the research
/// topic with the lowest level (deterministically).
pub fn choose_upgrade_topic(team_info: &crate::team::TeamInfo) -> ResearchTopic {
    let mut best = ResearchTopic::Attack;
    let mut best_level = u32::MAX;

    for topic in ResearchTopic::ALL.iter().copied() {
        let level = team_info.research_topics[topic as usize].research_level;

        if level < best_level {
            best_level = level;
            best = topic;
        }
    }

    best
}

/// `AiPlayer::GetStrategy` for the team.
pub fn strategy_of(world: &GameState, team: TeamId) -> AiStrategy {
    world.ai.players[team.0 as usize].strategy
}

/// `AiPlayer::GetNormalRateBuildCost` (`unitinfo.cpp:3730`) — cost in
/// raw material units: `turns * consumption`.
pub fn normal_rate_build_cost(world: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
    let turns = turns_to_build(world, team, unit_type);
    let rate = if unit_type.environment().is_some() {
        super::tasks::create::raw_consumption_rate(UnitType::HeavyVehiclePlant)
    } else {
        super::tasks::create::raw_consumption_rate(UnitType::Constructor)
    };

    turns * rate.max(1)
}

/// Turns to build a type (`BuildMenu_GetTurnsToBuild`).
pub fn turns_to_build(world: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
    super::tasks::create::turns_to_build(world, team, unit_type)
}

/// `AiPlayer::CreateBuilding` for the team (a free function).
pub fn create_building(
    world: &mut GameState,
    team: TeamId,
    unit_type: UnitType,
    site: Point,
    task: Option<u32>,
) -> bool {
    let mut player = std::mem::replace(&mut world.ai.players[team.0 as usize], AiPlayer::new(team));
    let result = player.create_building(world, unit_type, site, task);
    world.ai.players[team.0 as usize] = player;
    result
}

/// `AiPlayer::AddBuilding` (`aiplayer.cpp:68`).
pub fn add_building(world: &mut GameState, team: TeamId, unit: UnitId) {
    let Some(unit_info) = world.unit(unit).cloned() else {
        return;
    };

    if let Some(manager) = world.ai.players[team.0 as usize].find_manager(world, unit_info.grid) {
        world.with_task_manager(|task_manager, world| {
            task_manager.task_add_unit(world, manager, unit);
        });
    }
}

/// `AiPlayer::GetTeamClusterCoordinates` (`aiplayer.cpp:184`).
pub fn team_cluster_coordinates(world: &GameState, team: TeamId) -> Point {
    let mut sum_x: i64 = 0;
    let mut sum_y: i64 = 0;
    let mut count: i64 = 0;

    for kind in [
        UnitListKind::Stationary,
        UnitListKind::MobileLandSea,
        UnitListKind::MobileAir,
    ] {
        for id in world.unit_list(kind) {
            let Some(unit) = world.unit(*id) else {
                continue;
            };
            if unit.team != team {
                continue;
            }

            sum_x += i64::from(unit.grid.x);
            sum_y += i64::from(unit.grid.y);
            count += 1;
        }
    }

    if count == 0 {
        return Point::ZERO;
    }

    Point::new((sum_x / count) as i16, (sum_y / count) as i16)
}

/// `AiPlayer::GetUnitClusterCoordinates` (`aiplayer.cpp:162`).
pub fn unit_cluster_coordinates(world: &GameState, team: TeamId, kind: UnitListKind) -> Point {
    let mut sum_x: i64 = 0;
    let mut sum_y: i64 = 0;
    let mut count: i64 = 0;

    for id in world.unit_list(kind) {
        let Some(unit) = world.unit(*id) else {
            continue;
        };
        if unit.team != team {
            continue;
        }

        sum_x += i64::from(unit.grid.x);
        sum_y += i64::from(unit.grid.y);
        count += 1;
    }

    if count == 0 {
        return Point::ZERO;
    }

    Point::new((sum_x / count) as i16, (sum_y / count) as i16)
}

/// Continent for `SelectStrategy` (flood-fill over `SurfaceMap`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Continent {
    size: i32,
    viable: bool,
    isolated: bool,
    close_proximity: bool,
}

/// Land flood-fill: 4-connectivity, traversal `x`, then `y` (deterministically).
fn land_continents(world: &GameState) -> Vec<Continent> {
    let mut visited = vec![false; MAP_TILES];
    let mut continents = Vec::new();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let index = y as usize * MAP_WIDTH as usize + x as usize;
            if visited[index] {
                continue;
            }

            visited[index] = true;

            let surface = crate::access::surface_type(world, Point::new(x, y));
            if surface != crate::access::SURFACE_TYPE_LAND {
                continue;
            }

            // BFS in submission order (Vec as a queue).
            let mut queue = Vec::new();
            queue.push(Point::new(x, y));
            let mut size = 0;
            let mut head = 0;

            while head < queue.len() {
                let site = queue[head];
                head += 1;
                size += 1;

                for offset in crate::geo::DIRECTION_OFFSETS.iter().take(8).step_by(2) {
                    let next = Point::new(site.x + offset.x, site.y + offset.y);

                    if next.x < 0 || next.y < 0 || next.x >= MAP_WIDTH || next.y >= MAP_HEIGHT {
                        continue;
                    }

                    let next_index = next.y as usize * MAP_WIDTH as usize + next.x as usize;
                    if visited[next_index] {
                        continue;
                    }

                    visited[next_index] = true;

                    let next_surface = crate::access::surface_type(world, next);
                    if next_surface == crate::access::SURFACE_TYPE_LAND {
                        queue.push(next);
                    }
                }
            }

            let viable = size >= 40;
            let isolated = size < 80;
            let close_proximity = !isolated;

            continents.push(Continent {
                size,
                viable,
                isolated,
                close_proximity,
            });
        }
    }

    continents
}

/// Applying a clan to the team's unit values (`ResourceManager_InitClanUnitValues`).
///
/// Like the C++ function, this is a full re-initialization, not an increment:
/// `ClearComplexes()` + `TeamUnits::Init()` rebuild the clean `SC_ATTRI` base
/// before `SetClanUpgradesFromClans` adds the clan deltas, and the result is
/// written to both base and current values. Calling it again (or with another
/// clan) therefore cannot stack trade-offs.
pub fn apply_clan(world: &mut GameState, team: TeamId, clan: TeamClan) {
    let team_index = team.0 as usize;
    world.team_units[team_index].init_clan_unit_values(clan);
}

/// `Ai_GetNormalRateBuildCost` (`ai.cpp:54`).
pub fn ai_get_normal_rate_build_cost(world: &GameState, unit_type: UnitType, team: TeamId) -> i32 {
    normal_rate_build_cost(world, team, unit_type)
}

/// `TaskManager_NeedToReserveRawMaterials` — re-export from the create module.
pub use super::tasks::create::need_to_reserve_raw_materials;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::TeamUnits;

    /// `ResourceManager_InitClanUnitValues` starts from a clean `SC_ATTRI` base:
    /// a second call (or a clan change) must replace the table, not add to it,
    /// and the team complexes are dropped (`ClearComplexes`).
    #[test]
    fn apply_clan_reinitializes_instead_of_stacking() {
        let mut world = GameState::new();
        let team = TeamId::RED;
        let index = team.0 as usize;
        world.team_units[index] = TeamUnits::with_clan(TeamClan::Musashi);
        world.team_units[index].create_complex();

        apply_clan(&mut world, team, TeamClan::VonGriffin);
        let expected = crate::unit_values::team_base_values(TeamClan::VonGriffin);
        assert_eq!(world.team_units[index].base_values, expected);
        assert_eq!(world.team_units[index].current_values, expected);
        assert!(world.team_units[index].complexes().is_empty());

        // Idempotent: the same clan applied again keeps the same table.
        apply_clan(&mut world, team, TeamClan::VonGriffin);
        assert_eq!(world.team_units[index].base_values, expected);
        assert_eq!(world.team_units[index].current_values, expected);
    }
}
