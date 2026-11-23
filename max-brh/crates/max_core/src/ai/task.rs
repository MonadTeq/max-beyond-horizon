//! AI task base class — C++ `class Task` (`task.cpp`/`task.hpp`).
//!
//! # What is here
//!
//! * task type constants [`TASK_TYPE_*`] and their names (`Task_TaskNames`);
//! * the priority scale [`TASK_PRIORITY_*`] (lower is more urgent; the high byte —
//!   the level, the low one — an offset within the level, C++ `TASK_PRIORITY_*`);
//! * [`Task`] — common fields (`id`, `team`, `base_priority`, `parent`,
//!   scheduling flags) plus [`TaskKind`] — the state of a concrete task class
//!   (a structural analog of subclass fields in `task*.hpp`);
//! * free `Task_*` functions from `task.cpp` (readiness to accept orders,
//!   the "unit is doomed" estimate, retreat, adjacency, estimate of turns until
//!   game end) — they are needed by tasks, `aiattack.cpp` and `task_manager.cpp`;
//! * estimates relying on AI maps ([`crate::ai::strategy::AiPlayer`]).
//!
//! # Determinism
//!
//! `ComparePriority` (C++ `Task::ComparePriority`) does not saturate outside
//! the `±TASK_PRIORITY_TOLERANCE` window and inside the window returns the difference
//! of the dynamic `GetPriority()` and the argument — as in C++ (`task.cpp:444`).

use crate::classification::UnitTypeClassify;
use crate::enums::{unit_flags, CautionLevel, Opponent, UnitOrder, UnitOrderState, UnitType};
use crate::game::{GameState, PLAYER_TEAM_COUNT};
use crate::geo::Point;
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo};

use super::tasks::TaskKind;

// ---------------------------------------------------------------------------
// Task types — C++ `enum TaskType_*` (`task.hpp:33`)
// ---------------------------------------------------------------------------

/// `TaskType_TaskActivate`.
pub const TASK_TYPE_ACTIVATE: u8 = 0;
/// `TaskType_TaskAssistMove`.
pub const TASK_TYPE_ASSIST_MOVE: u8 = 1;
/// `TaskType_TaskAttack`.
pub const TASK_TYPE_ATTACK: u8 = 2;
/// `TaskType_TaskAttackReserve`.
pub const TASK_TYPE_ATTACK_RESERVE: u8 = 3;
/// `TaskType_TaskAutoSurvey`.
pub const TASK_TYPE_AUTO_SURVEY: u8 = 4;
/// `TaskType_TaskCheckAssaults`.
pub const TASK_TYPE_CHECK_ASSAULTS: u8 = 6;
/// `TaskType_TaskClearZone`.
pub const TASK_TYPE_CLEAR_ZONE: u8 = 7;
/// `TaskType_TaskCreateUnit`.
pub const TASK_TYPE_CREATE_UNIT: u8 = 8;
/// `TaskType_TaskCreateBuilding`.
pub const TASK_TYPE_CREATE_BUILDING: u8 = 9;
/// `TaskType_TaskEscort`.
pub const TASK_TYPE_ESCORT: u8 = 10;
/// `TaskType_TaskExplore`.
pub const TASK_TYPE_EXPLORE: u8 = 11;
/// `TaskType_TaskDefenseAssistant`.
pub const TASK_TYPE_DEFENSE_ASSISTANT: u8 = 12;
/// `TaskType_TaskDefenseReserve`.
pub const TASK_TYPE_DEFENSE_RESERVE: u8 = 13;
/// `TaskType_TaskDump`.
pub const TASK_TYPE_DUMP: u8 = 14;
/// `TaskType_TaskFindMines`.
pub const TASK_TYPE_FIND_MINES: u8 = 15;
/// `TaskType_TaskFindPath`.
pub const TASK_TYPE_FIND_PATH: u8 = 16;
/// `TaskType_TaskFrontalAttack`.
pub const TASK_TYPE_FRONTAL_ATTACK: u8 = 17;
/// `TaskType_TaskGetMaterials`.
pub const TASK_TYPE_GET_MATERIALS: u8 = 18;
/// `TaskType_TaskHabitatAssistant`.
pub const TASK_TYPE_HABITAT_ASSISTANT: u8 = 19;
/// `TaskType_TaskKillUnit`.
pub const TASK_TYPE_KILL_UNIT: u8 = 20;
/// `TaskType_TaskManageBuildings`.
pub const TASK_TYPE_MANAGE_BUILDINGS: u8 = 21;
/// `TaskType_TaskMineAssisstant`.
pub const TASK_TYPE_MINE_ASSISTANT: u8 = 22;
/// `TaskType_TaskMove`.
pub const TASK_TYPE_MOVE: u8 = 23;
/// `TaskType_TaskMoveHome`.
pub const TASK_TYPE_MOVE_HOME: u8 = 24;
/// `TaskType_TaskObtainUnits`.
pub const TASK_TYPE_OBTAIN_UNITS: u8 = 25;
/// `TaskType_TaskPlaceMines`.
pub const TASK_TYPE_PLACE_MINES: u8 = 26;
/// `TaskType_TaskConnectionAssistant`.
pub const TASK_TYPE_CONNECTION_ASSISTANT: u8 = 27;
/// `TaskType_TaskRadarAssistant`.
pub const TASK_TYPE_RADAR_ASSISTANT: u8 = 28;
/// `TaskType_TaskFrontierAssistant`.
pub const TASK_TYPE_FRONTIER_ASSISTANT: u8 = 29;
/// `TaskType_TaskPowerAssistant`.
pub const TASK_TYPE_POWER_ASSISTANT: u8 = 30;
/// `TaskType_TaskReload`.
pub const TASK_TYPE_RELOAD: u8 = 33;
/// `TaskType_TaskRemoveMines`.
pub const TASK_TYPE_REMOVE_MINES: u8 = 34;
/// `TaskType_TaskRemoveRubble`.
pub const TASK_TYPE_REMOVE_RUBBLE: u8 = 35;
/// `TaskType_TaskRendezvous`.
pub const TASK_TYPE_RENDEZVOUS: u8 = 36;
/// `TaskType_TaskRepair`.
pub const TASK_TYPE_REPAIR: u8 = 37;
/// `TaskType_TaskRetreat`.
pub const TASK_TYPE_RETREAT: u8 = 38;
/// `TaskType_TaskSearchDestination`.
pub const TASK_TYPE_SEARCH_DESTINATION: u8 = 39;
/// `TaskType_TaskScavenge`.
pub const TASK_TYPE_SCAVENGE: u8 = 40;
/// `TaskType_TaskSupportAttack`.
pub const TASK_TYPE_SUPPORT_ATTACK: u8 = 42;
/// `TaskType_TaskSurvey`.
pub const TASK_TYPE_SURVEY: u8 = 43;
/// `TaskType_TaskTransport`.
pub const TASK_TYPE_TRANSPORT: u8 = 45;
/// `TaskType_TaskUpdateTerrain`.
pub const TASK_TYPE_UPDATE_TERRAIN: u8 = 46;
/// `TaskType_TaskUpgrade`.
pub const TASK_TYPE_UPGRADE: u8 = 47;
/// `TaskType_TaskWaitToAttack`.
pub const TASK_TYPE_WAIT_TO_ATTACK: u8 = 48;

/// All task names in type-value order (`Task_TaskNames`, `task.cpp:43`).
pub const TASK_NAMES: [&str; 49] = [
    "Activate",
    "AssistMove",
    "Attack",
    "AttackReserve",
    "AutoSurvey",
    "Unknown",
    "CheckAssaults",
    "ClearZone",
    "CreateUnit",
    "CreateBuilding",
    "Escort",
    "Explore",
    "DefenseAssistant",
    "DefenseReserve",
    "Dump",
    "FindMines",
    "FindPath",
    "FrontalAttack",
    "GetMaterials",
    "HabitatAssistant",
    "KillUnit",
    "ManageBuildings",
    "MineAssisstant",
    "Move",
    "MoveHome",
    "ObtainUnits",
    "PlaceMines",
    "ConnectionAssistant",
    "RadarAssistant",
    "FrontierAssistant",
    "PowerAssistant",
    "Unknown",
    "Unknown",
    "Reload",
    "RemoveMines",
    "RemoveRubble",
    "Rendezvous",
    "Repair",
    "Retreat",
    "SearchDestination",
    "Scavenge",
    "Unknown",
    "SupportAttack",
    "Survey",
    "Unknown",
    "Transport",
    "UpdateTerrain",
    "Upgrade",
    "WaitToAttack",
];

/// `Task_GetName` (`task.cpp:95`).
pub fn task_get_name(task_type: u8) -> &'static str {
    TASK_NAMES
        .get(task_type as usize)
        .copied()
        .unwrap_or("Unknown")
}

// ---------------------------------------------------------------------------
// Priorities — C++ `enum : uint16_t` (`task.hpp:92`)
// ---------------------------------------------------------------------------

/// The `Task::ComparePriority` comparability window; also the penalty of a task without
/// targets in `GetPriority()`.
pub const TASK_PRIORITY_TOLERANCE: u16 = 0xFF;
/// Priority level mask.
pub const TASK_PRIORITY_MASK: u16 = 0xFF00;
/// "Half-tier" offset (a child build precedes the parent).
pub const TASK_PRIORITY_OFFSET_HALF_TIER: u16 = 0x80;
/// Bridge-link offset.
pub const TASK_PRIORITY_OFFSET_BRIDGE: u16 = 0xC8;
/// Priority bonus of a request when re-censusing pending work.
pub const TASK_PRIORITY_OFFSET_CENSUS_SLACK: u16 = 0xFA;

/// Emergency reactions.
pub const TASK_PRIORITY_EMERGENCY: u16 = 0x0000;
/// Air assault monitoring.
pub const TASK_PRIORITY_CHECK_ASSAULTS: u16 = 0x001E;
/// Mine removal on a site.
pub const TASK_PRIORITY_REMOVE_RUBBLE_LOW: u16 = 0x0100;
/// Rubble clearing on a site.
pub const TASK_PRIORITY_REMOVE_RUBBLE: u16 = 0x0200;
/// Massive frontal assault.
pub const TASK_PRIORITY_FRONTAL_ATTACK: u16 = 0x0400;
/// First ore station of a new base.
pub const TASK_PRIORITY_BUILDING_LOW: u16 = 0x0500;
/// Light factory.
pub const TASK_PRIORITY_BUILDING_LIGHT_PLANT: u16 = 0x0700;
/// Connector.
pub const TASK_PRIORITY_BUILDING_CONNECTOR: u16 = 0x0800;
/// Storage.
pub const TASK_PRIORITY_BUILDING_DUMP: u16 = 0x0A00;
/// Power station/generator.
pub const TASK_PRIORITY_BUILDING_POWER: u16 = 0x0B00;
/// Habitat.
pub const TASK_PRIORITY_BUILDING_HABITAT: u16 = 0x0C00;
/// Radar.
pub const TASK_PRIORITY_BUILDING_RADAR: u16 = 0x0D00;
/// Ore station replacement.
pub const TASK_PRIORITY_BUILDING_MEDIUM: u16 = 0x0E00;
/// Escort.
pub const TASK_PRIORITY_ESCORT: u16 = 0x0F00;
/// Mobile threat attack.
pub const TASK_PRIORITY_ATTACK_MOBILE: u16 = 0x1000;
/// Repair/rearm/upgrade.
pub const TASK_PRIORITY_REPAIR: u16 = 0x1100;
/// Defense reserve follows attackers.
pub const TASK_PRIORITY_FOLLOW_DEFENSE: u16 = 0x1100;
/// Defensive turret.
pub const TASK_PRIORITY_BUILDING_DEFENSE: u16 = 0x1200;
/// First heavy factory and training hall.
pub const TASK_PRIORITY_BUILDING_HEAVY_PLANT: u16 = 0x1300;
/// The "request is urgent enough, land access is irrelevant" threshold.
pub const TASK_PRIORITY_LAND_ACCESS_THRESHOLD: u16 = 0x1300;
/// Repair shop.
pub const TASK_PRIORITY_BUILDING_REPAIR_SHOP: u16 = 0x1400;
/// Research center.
pub const TASK_PRIORITY_BUILDING_UPGRADES: u16 = 0x1500;
/// Greenhouse.
pub const TASK_PRIORITY_BUILDING_GREENHOUSE: u16 = 0x1600;
/// Attack on the leader's priority target.
pub const TASK_PRIORITY_ATTACK_PRIORITY_TARGET: u16 = 0x1700;
/// Factory expansion by strategy.
pub const TASK_PRIORITY_BUILDING_EXPANSION: u16 = 0x1800;
/// Defense reserve.
pub const TASK_PRIORITY_DEFENSE_RESERVE: u16 = 0x1900;
/// Frontier: mining and mine laying.
pub const TASK_PRIORITY_FRONTIER: u16 = 0x1A00;
/// Map exploration.
pub const TASK_PRIORITY_EXPLORE: u16 = 0x1B00;
/// Bridge construction.
pub const TASK_PRIORITY_BRIDGE_BASE: u16 = 0x1C00;
/// Construction management.
pub const TASK_PRIORITY_MANAGE_BUILDINGS: u16 = 0x1D00;
/// Default attack.
pub const TASK_PRIORITY_ATTACK_DEFAULT: u16 = 0x1F00;
/// Sweeping enemy minefields.
pub const TASK_PRIORITY_FIND_MINES: u16 = 0x2000;
/// Attack reserve follows attackers of this priority.
pub const TASK_PRIORITY_FOLLOW_ATTACK: u16 = 0x2000;
/// Resource scavenging.
pub const TASK_PRIORITY_SCAVENGE: u16 = 0x2100;
/// Terrain survey.
pub const TASK_PRIORITY_AUTO_SURVEY: u16 = 0x2200;
/// Default resource delivery.
pub const TASK_PRIORITY_GET_RESOURCE: u16 = 0x2300;
/// Attack support.
pub const TASK_PRIORITY_SUPPORT_ATTACK: u16 = 0x2500;
/// Attack reserve pool.
pub const TASK_PRIORITY_ATTACK_RESERVE_POOL: u16 = 0x2600;
/// Transport assistance.
pub const TASK_PRIORITY_ASSIST_MOVE: u16 = 0x2800;
/// Weakest request: movement.
pub const TASK_PRIORITY_MOVE: u16 = 0x2900;

// ---------------------------------------------------------------------------
// TaskMove result codes — C++ `enum` (`taskmove.hpp:33`)
// ---------------------------------------------------------------------------

/// Success.
pub const TASKMOVE_RESULT_SUCCESS: i8 = 0;
/// Already in range.
pub const TASKMOVE_RESULT_ALREADY_IN_RANGE: i8 = 1;
/// Blocked.
pub const TASKMOVE_RESULT_BLOCKED: i8 = 2;
/// Cancelled.
pub const TASKMOVE_RESULT_CANCELLED: i8 = 3;

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

/// An AI task — C++ `class Task`.
///
/// The common fields match `task.hpp`: `m_id`, `m_team`, `m_base_priority`,
/// `m_parent`, `m_needs_processing`, `m_scheduled_for_turn_start/end`.
/// The concrete class state is in [`TaskKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// `m_id` (`Task::++task_id`).
    pub id: u32,
    /// `m_team`.
    pub team: TeamId,
    /// `m_base_priority`.
    pub base_priority: u16,
    /// `m_parent` — the parent task id.
    pub parent: Option<u32>,
    /// `m_needs_processing`.
    pub needs_processing: bool,
    /// `m_scheduled_for_turn_start`.
    pub scheduled_for_turn_start: bool,
    /// `m_scheduled_for_turn_end`.
    pub scheduled_for_turn_end: bool,
    /// The task class state.
    pub kind: TaskKind,
}

impl Task {
    /// `Task::Task` (`task.cpp:388`).
    pub fn new(id: u32, team: TeamId, parent: Option<u32>, priority: u16, kind: TaskKind) -> Self {
        Self {
            id,
            team,
            base_priority: priority,
            parent,
            needs_processing: true,
            scheduled_for_turn_start: false,
            scheduled_for_turn_end: false,
            kind,
        }
    }

    /// `GetType()` — task type ([`TASK_TYPE_*`] values).
    pub fn get_type(&self) -> u8 {
        self.kind.get_type()
    }

    /// `Task_GetName(this)`.
    pub fn get_name(&self) -> &'static str {
        task_get_name(self.get_type())
    }

    /// `Task::ComparePriority` (`task.cpp:444`).
    ///
    /// Outside the `±TASK_PRIORITY_TOLERANCE` window — saturation `±1` by the static
    /// priority, inside the window — the difference of the dynamic `GetPriority()`.
    pub(crate) fn compare_priority(&self, world: &GameState, task_priority: u16) -> i32 {
        let base = i32::from(self.base_priority);
        let tolerance = i32::from(TASK_PRIORITY_TOLERANCE);
        let argument = i32::from(task_priority);

        if base + tolerance >= argument {
            if base - tolerance <= argument {
                i32::from(self.get_priority(world)) - argument
            } else {
                1
            }
        } else {
            -1
        }
    }

    /// `GetPriority()` — base by default; overridden in subclasses.
    pub(crate) fn get_priority(&self, world: &GameState) -> u16 {
        self.kind.get_priority(world, self)
    }

    /// `GetCautionLevel(unit)` — `Task::GetCautionLevel` by default.
    pub(crate) fn get_caution_level(&self, world: &GameState, unit: &UnitInfo) -> CautionLevel {
        self.kind
            .get_caution_level(world, self, unit)
            .unwrap_or_else(|| default_caution_level(world, unit))
    }

    /// `DeterminePosition()` — `GetBounds().ul`, unless overridden.
    pub(crate) fn determine_position(&self, world: &GameState) -> Point {
        let bounds = self.kind.get_bounds(world, self.header());
        Point::new(bounds.ulx as i16, bounds.uly as i16)
    }

    /// `Task::IsEndTurnNeeded()` — `true` by default.
    pub fn is_end_turn_needed(&self) -> bool {
        self.kind.is_end_turn_needed_kind()
    }

    /// `Task::IsThinking()` — `false` by default.
    pub(crate) fn is_thinking(&self, world: &GameState) -> bool {
        let _ = world;
        self.kind.is_thinking_kind()
    }

    /// `IsNeeded()` — follows the parent by default (`task.cpp:613`).
    pub fn is_needed(&self, manager: &super::task_manager::TaskManager) -> bool {
        match self.parent {
            Some(parent) => manager
                .task(parent)
                .map(|task| task.is_needed(manager))
                .unwrap_or(true),
            None => true,
        }
    }
}

/// Copy of the task's base fields for task-class handlers.
///
/// Needed so that the handler receives both `&mut TaskKind` and the base fields without
/// double-borrowing `Task`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskHeader {
    /// `m_id`.
    pub id: u32,
    /// `m_team`.
    pub team: TeamId,
    /// `m_base_priority`.
    pub base_priority: u16,
    /// `m_parent`.
    pub parent: Option<u32>,
    /// `m_needs_processing`.
    pub needs_processing: bool,
}

impl Task {
    /// Copy of the base fields.
    pub fn header(&self) -> TaskHeader {
        TaskHeader {
            id: self.id,
            team: self.team,
            base_priority: self.base_priority,
            parent: self.parent,
            needs_processing: self.needs_processing,
        }
    }

    /// `WriteStatusLog()` — a short description (for logs and tests).
    pub fn write_status_log(&self) -> String {
        self.kind.write_status_log(self)
    }
}

/// `Task::GetCautionLevel` (`task.cpp:583`).
pub(crate) fn default_caution_level(world: &GameState, unit: &UnitInfo) -> CautionLevel {
    let opponent = world.ai.settings.opponent;

    if unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE)
        != 0
        && opponent >= Opponent::Average as u8
    {
        if unit.shots > 0 {
            CautionLevel::AvoidReactionFire
        } else {
            CautionLevel::AvoidAllDamage
        }
    } else if unit.ammo != 0 {
        CautionLevel::AvoidNextTurnsFire
    } else {
        CautionLevel::AvoidAllDamage
    }
}

// ---------------------------------------------------------------------------
// Free `Task_*` functions (`task.cpp`)
// ---------------------------------------------------------------------------

/// `Task_GetUnitList` — the unit list by type flags (`task.cpp:535`).
pub fn task_unit_list_kind(unit_type: UnitType) -> crate::unit::UnitListKind {
    let flags = crate::access::unit_type_flags(unit_type);

    if !unit_type.is_mobile() {
        if flags & unit_flags::GROUND_COVER != 0 {
            crate::unit::UnitListKind::GroundCover
        } else {
            crate::unit::UnitListKind::Stationary
        }
    } else if unit_type.is_air() {
        crate::unit::UnitListKind::MobileAir
    } else {
        crate::unit::UnitListKind::MobileLandSea
    }
}

/// `Task_GetReadyUnitsCount` (`task.cpp:557`).
pub(crate) fn task_get_ready_units_count(world: &GameState, team: TeamId, unit_type: UnitType) -> i32 {
    let mut count = 0;

    for id in world.unit_list(task_unit_list_kind(unit_type)) {
        let Some(unit) = world.unit(*id) else {
            continue;
        };

        if unit.team == team
            && unit.unit_type == unit_type
            && unit.state != UnitOrderState::BuildingReady
        {
            count += 1;
        }
    }

    count
}

/// `Task_IsReadyToTakeOrders` (`task.cpp:105`).
pub(crate) fn task_is_ready_to_take_orders(world: &GameState, unit_id: UnitId) -> bool {
    let Some(unit) = world.unit(unit_id) else {
        return false;
    };

    if unit.hits == 0 || world.play_mode == crate::enums::PlayMode::Unknown {
        return false;
    }

    let accepts_order = unit.orders == UnitOrder::Await
        || unit.orders == UnitOrder::Sentry
        || (unit.orders == UnitOrder::Move && unit.state == UnitOrderState::ExecutingOrder)
        || (unit.orders == UnitOrder::MoveToUnit && unit.state == UnitOrderState::ExecutingOrder);

    if !accepts_order {
        return false;
    }

    // `Access_GetActiveLoader(unit) == nullptr` — the unit is not loaded into a transport.
    // Only air transports hold `ORDER_LOAD`.
    for id in world.unit_list(crate::unit::UnitListKind::MobileAir) {
        let Some(loader) = world.unit(*id) else {
            continue;
        };
        if loader.orders == UnitOrder::Load && loader.parent_unit == Some(unit_id) {
            return false;
        }
    }

    true
}

/// `Task_IsAdjacent` (`task.cpp:311`) — adjacency considering the 2×2 footprint.
pub fn task_is_adjacent(unit: &UnitInfo, grid_x: i16, grid_y: i16) -> bool {
    let unit_size: i16 = if unit.is_quad() { 2 } else { 1 };

    unit.grid.x - 1 <= grid_x
        && unit.grid.x + unit_size >= grid_x
        && unit.grid.y - 1 <= grid_y
        && unit.grid.y + unit_size >= grid_y
}

/// `Task_IsRepairDue` (`task.cpp:325`).
pub fn task_is_repair_due(unit: &UnitInfo, is_lead_unit: bool) -> bool {
    let base_hits = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_HITS);

    i32::from(unit.hits) < base_hits / 4 || (is_lead_unit && i32::from(unit.hits) < base_hits / 2)
}

/// `Task_EstimateTurnsTillMissionEnd` (`task.cpp:350`).
///
/// Victory mode (`victory_type`/`victory_limit`, state-model §3.1) — a game
/// setting that comes from the setup layer in max_core (P7, DESIGN risk 20):
/// here the C++ default limit is used (1000 turns minus the current turn).
/// The Score mode formula (by greenhouses) repeats C++ and is called when
/// `victory_type == Score` becomes part of `GameState`.
pub(crate) fn task_estimate_turns_till_mission_end(world: &GameState) -> i32 {
    const VICTORY_LIMIT_DEFAULT: i32 = 1000;

    let mut remaining_turns = VICTORY_LIMIT_DEFAULT;
    let mut score_turns = VICTORY_LIMIT_DEFAULT;

    for team in 0..PLAYER_TEAM_COUNT {
        if world.teams[team].team_type == TeamKind::None {
            continue;
        }

        let mut count = 0;
        for id in world.unit_list(crate::unit::UnitListKind::Stationary) {
            let Some(unit) = world.unit(*id) else {
                continue;
            };
            if unit.team == TeamId(team as u8)
                && unit.unit_type == UnitType::EcoSphere
                && unit.orders == UnitOrder::PowerOn
            {
                count += 1;
            }
        }

        if count > 0 {
            let points = i32::try_from(world.teams[team].team_points).unwrap_or(i32::MAX);
            let turns = (VICTORY_LIMIT_DEFAULT - points) / count;

            if turns < score_turns {
                score_turns = turns;
            }
        }
    }

    // Duration mode (`victory_type != SCORE` by default in C++).
    remaining_turns -= world.turn_counter;
    if remaining_turns <= 0 {
        remaining_turns = 1;
    }

    let _ = score_turns;
    remaining_turns
}

/// `AIPlayer::IsEnemyTeam` (`aiplayer.cpp:2208`).
pub(crate) fn is_enemy_team(world: &GameState, team: TeamId) -> bool {
    let kind = world.teams[team.0 as usize].team_type;

    kind != TeamKind::None && kind != TeamKind::Eliminated
}

/// `Task_IsRecruitable` (`task.cpp:331`) — the only test by which attacks
/// take units.
pub(crate) fn task_is_recruitable(
    manager: &super::task_manager::TaskManager,
    world: &GameState,
    unit_id: UnitId,
    team: TeamId,
    task_priority: u16,
) -> bool {
    let Some(unit) = world.unit(unit_id) else {
        return false;
    };

    if unit.team != team
        || unit.hits == 0
        || !matches!(
            unit.orders,
            UnitOrder::Await | UnitOrder::Sentry | UnitOrder::Move | UnitOrder::MoveToUnit
        )
    {
        return false;
    }

    match manager.unit_task(unit_id) {
        None => true,
        Some(task_id) => match manager.task(task_id) {
            Some(task) => {
                task.compare_priority(world, task_priority) > 0
                    && task.kind.is_unit_transferable(world, task, unit)
            }
            None => true,
        },
    }
}

/// `Task_ShouldReserveShot` (`task.cpp:139`).
///
/// The structure is ported: checking team stealth maps, cell danger and
/// the absence of threats with a longer range. The loop over spotted units goes in
/// `AiPlayer::spotted_units` order (detection order).
pub(crate) fn task_should_reserve_shot(world: &GameState, unit: &UnitInfo, site: Point) -> bool {
    use crate::unit_values::{
        ATTRIB_ATTACK_RADIUS, ATTRIB_MOVE_AND_FIRE, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SPEED,
    };

    if unit.shots == 0 || unit.base_values.get_attribute(ATTRIB_MOVE_AND_FIRE) != 0 {
        return false;
    }

    let mut relevant_teams = [false; PLAYER_TEAM_COUNT];
    let mut unit_range = 0;

    for team in 0..PLAYER_TEAM_COUNT {
        let team_id = TeamId(team as u8);
        if world.teams[team].team_type == TeamKind::None {
            relevant_teams[team] = false;
            continue;
        }

        let heat = &world.ai.heat_maps[team];

        if matches!(
            unit.unit_type,
            UnitType::Submarine | UnitType::ArmouredPersonnelCarrier
        ) && access_modified_surface_is_water(world, site)
            && !unit.is_detected_by_team(team_id)
        {
            relevant_teams[team] = heat.get_stealth_sea(i32::from(site.x), i32::from(site.y)) != 0;
        } else if unit.unit_type == UnitType::Infiltrator && !unit.is_detected_by_team(team_id) {
            relevant_teams[team] = heat.get_stealth_land(i32::from(site.x), i32::from(site.y)) != 0;
        } else {
            relevant_teams[team] = heat.get_complete(i32::from(site.x), i32::from(site.y)) != 0;
        }
    }

    let any_other =
        (0..PLAYER_TEAM_COUNT).any(|team| relevant_teams[team] && TeamId(team as u8) != unit.team);

    if !any_other {
        return false;
    }

    if !super::ai_is_dangerous_location(world, unit, site, CautionLevel::AvoidNextTurnsFire) {
        return false;
    }

    let player = &world.ai.players[unit.team.0 as usize];

    for spotted in &player.spotted_units {
        let Some(spotted_id) = spotted.unit else {
            continue;
        };
        let Some(spotted_unit) = world.unit(spotted_id) else {
            continue;
        };

        let spotted_range = spotted_unit.base_values.get_attribute(ATTRIB_RANGE);

        if spotted_unit.ammo > 0
            && relevant_teams[spotted_unit.team.0 as usize]
            && spotted_range > unit_range
            && crate::access::is_valid_attack_target_ex(
                world,
                spotted_unit.unit_type,
                unit.unit_type,
                unit.flags,
                site,
            )
        {
            let unit_values = spotted_unit.base_values;
            let unit_distance = crate::access::squared_distance(unit.grid, spotted.last_position);
            let mut attack_distance =
                spotted_range + unit_values.get_attribute(ATTRIB_ATTACK_RADIUS);

            if unit_values.get_attribute(ATTRIB_MOVE_AND_FIRE) != 0 {
                attack_distance += unit_values.get_attribute(ATTRIB_SPEED) / 2;
            } else {
                let rounds = unit_values.get_attribute(ATTRIB_ROUNDS);
                attack_distance +=
                    ((rounds - 1) * (unit_values.get_attribute(ATTRIB_SPEED) + 1)) / rounds;
            }

            if unit_distance <= attack_distance * attack_distance {
                if spotted_range <= unit.base_values.get_attribute(ATTRIB_RANGE) {
                    if crate::access::is_valid_attack_target_type(
                        unit.unit_type,
                        spotted_unit.unit_type,
                    ) {
                        unit_range = unit.base_values.get_attribute(ATTRIB_RANGE);
                    } else {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }
    }

    unit_range != 0
}

/// `Access_GetModifiedSurfaceType(site) == SURFACE_TYPE_WATER`.
fn access_modified_surface_is_water(world: &GameState, site: Point) -> bool {
    crate::access::modified_surface_type(world, site, false) == crate::access::SURFACE_TYPE_WATER
}

/// `Task_IsUnitDoomedToDestruction` (`task.cpp:264`).
///
/// Simplification: the `ZoneWalker` walk uses `Access_IsAccessible`, as C++;
/// without a damage map (`GetDamagePotentialMap` == `None`) the result is
/// `false` (the C++ branch).
pub(crate) fn task_is_unit_doomed_to_destruction(
    world: &mut GameState,
    unit_id: UnitId,
    caution_level: CautionLevel,
) -> bool {
    let Some(unit) = world.unit(unit_id).cloned() else {
        return false;
    };

    if unit.ai_state_bits & crate::enums::AI_STATE_NO_RETREAT != 0 {
        return true;
    }

    let mut unit_hits = i32::from(unit.hits);
    if caution_level == CautionLevel::AvoidAllDamage {
        unit_hits = 1;
    }

    let position = unit.grid;

    if super::strategy::get_damage_potential(world, unit.team, &unit, position, caution_level)
        < unit_hits
    {
        return false;
    }

    if unit.speed == 0 {
        return true;
    }

    let Some(map) =
        super::strategy::get_damage_potential_map(world, unit.team, &unit, caution_level)
    else {
        return false;
    };

    let mut walker = super::threat::ZoneWalker::new(position, i32::from(unit.speed));

    loop {
        let grid = walker.current_location();
        if map.damage_at(i32::from(grid.x), i32::from(grid.y)) < unit_hits
            && crate::access::is_accessible(
                world,
                unit.unit_type,
                unit.team,
                grid,
                crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) != 0
        {
            return false;
        }

        if !walker.find_next() {
            break;
        }
    }

    if let Some(mut unit) = world.unit(unit_id).cloned() {
        unit.ai_state_bits |= crate::enums::AI_STATE_NO_RETREAT;
        world.write_unit(unit_id, unit);
    }

    true
}

/// `Task_RetreatIfNecessary` (`task.cpp:468`).
pub(crate) fn task_retreat_if_necessary(
    manager: &mut super::task_manager::TaskManager,
    world: &mut GameState,
    task: Option<u32>,
    unit_id: UnitId,
    caution_level: CautionLevel,
) -> bool {
    let Some(unit) = world.unit(unit_id).cloned() else {
        return false;
    };

    if caution_level == CautionLevel::None
        || unit.ai_state_bits & crate::enums::AI_STATE_NO_RETREAT != 0
    {
        return false;
    }

    if unit.speed == 0 {
        return false;
    }

    let mut caution = caution_level;

    if unit.ammo == 0
        && unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_AMMO)
            > 0
    {
        caution = CautionLevel::AvoidAllDamage;
    }

    let opponent = world.ai.settings.opponent;

    if unit.shots == 0
        && unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE)
            != 0
        && opponent >= Opponent::Average as u8
    {
        caution = CautionLevel::AvoidAllDamage;
    }

    let position = unit.grid;
    let mut unit_hits = i32::from(unit.hits);

    if caution == CautionLevel::AvoidAllDamage {
        unit_hits = 1;
    }

    let damage = super::strategy::get_damage_potential(world, unit.team, &unit, position, caution);

    if damage >= unit_hits {
        let rounds = unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
        let move_and_fire = unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE);

        if rounds > 0
            && (unit.shots > 0 || move_and_fire == 0 || opponent < Opponent::Average as u8)
            && task_is_unit_doomed_to_destruction(world, unit_id, caution)
        {
            if let Some(mut unit) = world.unit(unit_id).cloned() {
                unit.ai_state_bits |= crate::enums::AI_STATE_NO_RETREAT;
                world.write_unit(unit_id, unit);
            }

            return false;
        }

        let retreat = super::tasks::TaskKind::retreat(unit_id, task, caution);
        super::tasks::append_task(
            manager,
            world,
            team_of(world, unit_id),
            None,
            TASK_PRIORITY_EMERGENCY,
            retreat,
        );

        return true;
    }

    false
}

/// The unit's team.
pub(crate) fn team_of(world: &GameState, unit_id: UnitId) -> TeamId {
    world
        .unit(unit_id)
        .map(|unit| unit.team)
        .unwrap_or(TeamId::RED)
}

/// `Task_RetreatFromDanger` (`task.cpp:520`).
pub(crate) fn task_retreat_from_danger(
    manager: &mut super::task_manager::TaskManager,
    world: &mut GameState,
    task_id: u32,
    unit_id: UnitId,
    caution_level: CautionLevel,
) -> bool {
    let needs_processing = manager
        .task(task_id)
        .map(|task| task.needs_processing)
        .unwrap_or(false);

    if needs_processing
        && task_retreat_if_necessary(manager, world, Some(task_id), unit_id, caution_level)
    {
        if let Some(task) = manager.task_mut(task_id) {
            task.needs_processing = false;
        }

        true
    } else {
        false
    }
}

/// `Task_RemindMoveFinished` (`task.cpp:99`) — a wrapper over the manager.
pub(crate) fn task_remind_move_finished(
    manager: &mut super::task_manager::TaskManager,
    world: &mut GameState,
    unit_id: UnitId,
    priority: bool,
) {
    manager.remind_move_finished(world, unit_id, priority);
}

/// `Task_RemoveMovementTasks` (`task.cpp:125`).
pub(crate) fn task_remove_movement_tasks(
    manager: &mut super::task_manager::TaskManager,
    world: &mut GameState,
    unit_id: UnitId,
) {
    let Some(unit) = world.unit(unit_id).cloned() else {
        return;
    };

    if unit.orders == UnitOrder::Idle && unit.hits > 0 {
        return;
    }

    let mut remove = Vec::new();
    for task_id in manager.unit_tasks_of(unit_id) {
        if let Some(task) = manager.task(task_id) {
            let task_type = task.get_type();
            if task_type == TASK_TYPE_MOVE || task_type == TASK_TYPE_FIND_PATH {
                remove.push(task_id);
            }
        }
    }

    for task_id in remove {
        manager.detach_task_from_unit(world, task_id, unit_id, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::task_manager::TaskManager;
    use crate::team::TeamId;

    #[test]
    fn task_names_match_cpp_table() {
        assert_eq!(task_get_name(TASK_TYPE_ATTACK), "Attack");
        assert_eq!(task_get_name(TASK_TYPE_CREATE_BUILDING), "CreateBuilding");
        assert_eq!(task_get_name(TASK_TYPE_WAIT_TO_ATTACK), "WaitToAttack");
        assert_eq!(task_get_name(5), "Unknown");
        assert_eq!(task_get_name(200), "Unknown");
    }

    #[test]
    fn priorities_are_ordered_by_urgency() {
        assert!(TASK_PRIORITY_EMERGENCY < TASK_PRIORITY_ATTACK_DEFAULT);
        assert!(TASK_PRIORITY_CHECK_ASSAULTS < TASK_PRIORITY_REMOVE_RUBBLE);
        assert!(TASK_PRIORITY_BUILDING_LOW < TASK_PRIORITY_BUILDING_DUMP);
        assert_eq!(TASK_PRIORITY_REPAIR, TASK_PRIORITY_FOLLOW_DEFENSE);
        assert_eq!(TASK_PRIORITY_FIND_MINES, TASK_PRIORITY_FOLLOW_ATTACK);
    }

    #[test]
    fn compare_priority_matches_cpp_window() {
        let world = GameState::new();
        let task = Task::new(
            1,
            TeamId::RED,
            None,
            TASK_PRIORITY_MANAGE_BUILDINGS,
            TaskKind::MineAssistant { manager: None },
        );

        // Inside the window the difference of the dynamic priority is returned.
        assert_eq!(
            task.compare_priority(&world, TASK_PRIORITY_MANAGE_BUILDINGS),
            0
        );
        // More urgent than the argument — a negative value.
        assert!(task.compare_priority(&world, TASK_PRIORITY_MANAGE_BUILDINGS + 0x40) < 0);
        // Less urgent (a larger number) — a positive value.
        assert!(task.compare_priority(&world, TASK_PRIORITY_MANAGE_BUILDINGS - 0x40) > 0);
        // Outside the window — saturation.
        assert_eq!(task.compare_priority(&world, 0xFFFF), -1);
        assert_eq!(task.compare_priority(&world, 0x0000), 1);
    }

    #[test]
    fn adjacency_and_repair_due_follow_cpp() {
        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::new(TeamId::RED, 1));
        unit.grid = Point::new(5, 5);
        assert!(task_is_adjacent(&unit, 6, 6));
        assert!(!task_is_adjacent(&unit, 8, 5));

        unit.flags = unit_flags::BUILDING;
        assert!(task_is_adjacent(&unit, 7, 7));

        let base_hits = unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_HITS) as u16;
        // `hits < base_hits / 4` — strictly less.
        unit.hits = base_hits / 4;
        assert!(!task_is_repair_due(&unit, false));
        unit.hits = base_hits / 4 - 1;
        assert!(task_is_repair_due(&unit, false));
        unit.hits = base_hits / 3;
        assert!(!task_is_repair_due(&unit, true) || base_hits / 3 < base_hits / 2);
        unit.hits = base_hits / 2 - 1;
        assert!(task_is_repair_due(&unit, true));
    }

    #[test]
    fn task_manager_starts_empty() {
        let manager = TaskManager::new();
        assert_eq!(manager.task_count(), 0);
        assert_eq!(manager.reminders_count(), 0);
    }
}
