//! AI task classes (`ai/tasks/*`) — a port of `task*.cpp` / `task*.hpp`.
//!
//! Each task class is a [`TaskKind`] variant with the state fields of
//! the corresponding `task*.hpp`, plus lifecycle handlers
//! (`Init`/`BeginTurn`/`EndTurn`/`Execute`/`AddUnit`/…). Handlers
//! are grouped by file:
//!
//! | File | C++ classes |
//! |---|---|
//! | [`create`] | `taskcreate.cpp`, `taskcreateunit.cpp`, `taskcreatebuilding.cpp`, `taskobtainunits.cpp`, `taskactivate.cpp` |
//! | [`combat`] | `taskattack.cpp`, `taskkillunit.cpp`, `taskfrontalattack.cpp`, `taskwaittoattack.cpp`, `tasksupportattack.cpp`, `taskattackreserve.cpp`, `taskdefensereserve.cpp`, `taskescort.cpp`, `taskretreat.cpp` |
//! | [`move`] | `taskmove.cpp`, `taskmovehome.cpp`, `taskfindpath.cpp`, `tasksearchdestination.cpp`, `taskrendezvous.cpp`, `taskassistmove.cpp`, `tasktransport.cpp`, `taskdump.cpp` |
//! | [`build`] | `taskmanagebuildings.cpp`, `taskpowerassistant.cpp`, `taskhabitatassistant.cpp`, `taskradarassistant.cpp`, `taskconnectionassistant.cpp`, `taskdefenseassistant.cpp`, `taskfrontierassistant.cpp`, `taskmineassistant.cpp`, `taskupdateterrain.cpp` |
//! | [`misc`] | `taskgetresource.cpp`, `taskgetmaterials.cpp`, `taskrepair.cpp`, `taskreload.cpp`, `taskupgrade.cpp`, `taskremoverubble.cpp`, `taskremovemines.cpp`, `taskfindmines.cpp`, `taskplacemines.cpp`, `taskscavenge.cpp`, `taskabstractsearch.cpp`, `tasksurvey.cpp`, `taskexplore.cpp`, `taskautosurvey.cpp`, `taskclearzone.cpp`, `taskcheckassaults.cpp` |
//!
//! # How tasks access the game
//!
//! A task lives in [`crate::ai::task_manager::TaskManager`], which is in
//! `GameState.ai.task_manager`. Mutating handlers receive
//! [`TaskCtx`] — `&mut TaskManager` + `&mut GameState` at once; `TaskKind`
//! itself is taken from a clone of the task and written back after the call
//! (`TaskManager::write_kind`). This repeats the semantics of C++ pointers
//! (`SmartPointer<Task>`) and avoids borrowing `Self` because of the game
//! state.

use crate::enums::{CautionLevel, UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{Cargo, TeamId};
use crate::unit::{Rect, UnitId, UnitInfo, UnitListKind};
use crate::unit_order;

use super::task::{
    TaskHeader, TASK_PRIORITY_MOVE, TASK_TYPE_ACTIVATE, TASK_TYPE_ASSIST_MOVE, TASK_TYPE_ATTACK,
    TASK_TYPE_ATTACK_RESERVE, TASK_TYPE_AUTO_SURVEY, TASK_TYPE_CHECK_ASSAULTS,
    TASK_TYPE_CLEAR_ZONE, TASK_TYPE_CONNECTION_ASSISTANT, TASK_TYPE_CREATE_BUILDING,
    TASK_TYPE_CREATE_UNIT, TASK_TYPE_DEFENSE_ASSISTANT, TASK_TYPE_DEFENSE_RESERVE, TASK_TYPE_DUMP,
    TASK_TYPE_ESCORT, TASK_TYPE_EXPLORE, TASK_TYPE_FIND_MINES, TASK_TYPE_FIND_PATH,
    TASK_TYPE_FRONTAL_ATTACK, TASK_TYPE_FRONTIER_ASSISTANT, TASK_TYPE_GET_MATERIALS,
    TASK_TYPE_HABITAT_ASSISTANT, TASK_TYPE_KILL_UNIT, TASK_TYPE_MANAGE_BUILDINGS,
    TASK_TYPE_MINE_ASSISTANT, TASK_TYPE_MOVE, TASK_TYPE_MOVE_HOME, TASK_TYPE_OBTAIN_UNITS,
    TASK_TYPE_PLACE_MINES, TASK_TYPE_POWER_ASSISTANT, TASK_TYPE_RADAR_ASSISTANT, TASK_TYPE_RELOAD,
    TASK_TYPE_REMOVE_MINES, TASK_TYPE_REMOVE_RUBBLE, TASK_TYPE_RENDEZVOUS, TASK_TYPE_REPAIR,
    TASK_TYPE_RETREAT, TASK_TYPE_SCAVENGE, TASK_TYPE_SEARCH_DESTINATION, TASK_TYPE_SUPPORT_ATTACK,
    TASK_TYPE_SURVEY, TASK_TYPE_TRANSPORT, TASK_TYPE_UPDATE_TERRAIN, TASK_TYPE_UPGRADE,
    TASK_TYPE_WAIT_TO_ATTACK,
};
use super::task_manager::TaskManager;
use super::Task;

pub mod build;
pub mod combat;
pub mod create;
pub mod misc;
pub mod move_tasks;
pub mod search;

// ---------------------------------------------------------------------------
// Task class state (fields from task*.hpp)
// ---------------------------------------------------------------------------

/// `ATTACK_STATE_*` (`taskattack.hpp:33`).
pub mod attack_state {
    /// `ATTACK_STATE_INIT`.
    pub const INIT: u8 = 0;
    /// `ATTACK_STATE_WAIT`.
    pub const WAIT: u8 = 1;
    /// `ATTACK_STATE_GATHER_FORCES`.
    pub const GATHER_FORCES: u8 = 2;
    /// `ATTACK_STATE_ADVANCE`.
    pub const ADVANCE: u8 = 3;
    /// `ATTACK_STATE_ADVANCE_USING_TRANSPORT`.
    pub const ADVANCE_USING_TRANSPORT: u8 = 4;
    /// `ATTACK_STATE_NORMAL_SEARCH`.
    pub const NORMAL_SEARCH: u8 = 5;
    /// `ATTACK_STATE_BOLD_SEARCH`.
    pub const BOLD_SEARCH: u8 = 6;
    /// `ATTACK_STATE_ATTACK`.
    pub const ATTACK: u8 = 7;
}

/// `CREATE_UNIT_STATE_*` (`taskcreateunit.cpp:33`).
pub mod create_unit_state {
    /// `CREATE_UNIT_STATE_INITIALIZING`.
    pub const INITIALIZING: u8 = 0;
    /// `CREATE_UNIT_STATE_OBTAININING_BUILDER`.
    pub const OBTAINING_BUILDER: u8 = 1;
    /// `CREATE_UNIT_STATE_WAITING_FOR_MATERIALS`.
    pub const WAITING_FOR_MATERIALS: u8 = 2;
    /// `CREATE_UNIT_STATE_BUILDING`.
    pub const BUILDING: u8 = 3;
    /// `CREATE_UNIT_STATE_ACTIVATING`.
    pub const ACTIVATING: u8 = 4;
    /// `CREATE_UNIT_STATE_FINISHED`.
    pub const FINISHED: u8 = 5;
}

/// `CREATE_BUILDING_STATE_*` (`taskcreatebuilding.cpp:50`).
pub mod create_building_state {
    /// `CREATE_BUILDING_STATE_INITIALIZING`.
    pub const INITIALIZING: u8 = 0;
    /// `CREATE_BUILDING_STATE_WAITING_FOR_PLATFORM`.
    pub const WAITING_FOR_PLATFORM: u8 = 1;
    /// `CREATE_BUILDING_STATE_REMOVING_MINE`.
    pub const REMOVING_MINE: u8 = 2;
    /// `CREATE_BUILDING_STATE_GETTING_BUILDER`.
    pub const GETTING_BUILDER: u8 = 3;
    /// `CREATE_BUILDING_STATE_GETTING_MATERIALS`.
    pub const GETTING_MATERIALS: u8 = 4;
    /// `CREATE_BUILDING_STATE_EVALUTING_SITE`.
    pub const EVALUATING_SITE: u8 = 5;
    /// `CREATE_BUILDING_STATE_SITE_BLOCKED`.
    pub const SITE_BLOCKED: u8 = 6;
    /// `CREATE_BUILDING_STATE_MOVING_TO_SITE`.
    pub const MOVING_TO_SITE: u8 = 7;
    /// `CREATE_BUILDING_STATE_CLEARING_SITE`.
    pub const CLEARING_SITE: u8 = 8;
    /// `CREATE_BUILDING_STATE_BUILDING`.
    pub const BUILDING: u8 = 9;
    /// `CREATE_BUILDING_STATE_MOVING_OFF_SITE`.
    pub const MOVING_OFF_SITE: u8 = 10;
    /// `CREATE_BUILDING_STATE_FINISHED`.
    pub const FINISHED: u8 = 11;
}

/// `TaskAttack` — attack state (`taskattack.hpp:44`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AttackState {
    /// `target_team`.
    pub target_team: Option<TeamId>,
    /// `op_state`.
    pub op_state: u8,
    /// `primary_targets` — `TaskKillUnit` tasks.
    pub primary_targets: Vec<u32>,
    /// `secondary_targets`.
    pub secondary_targets: Vec<u32>,
    /// `SpottedUnit` index the attack was started on (`TaskAttack` constructor).
    pub initial_target: Option<u32>,
    /// `kill_unit_task`.
    pub kill_unit_task: Option<u32>,
    /// `support_attack_task`.
    pub support_attack_task: Option<u32>,
    /// `leader`.
    pub leader: Option<UnitId>,
    /// `leader_task`.
    pub leader_task: Option<u32>,
    /// `access_flags`.
    pub access_flags: u32,
    /// `recon_unit`.
    pub recon_unit: Option<UnitId>,
    /// `managed_unit_types`.
    pub managed_unit_types: Vec<UnitType>,
    /// `attack_zone_reached`.
    pub attack_zone_reached: bool,
    /// `fire_target` (id of the target that was fired at).
    pub fire_target: Option<UnitId>,
    /// `fire_turn`.
    pub fire_turn: i32,
    /// `defenders_fire`.
    pub defenders_fire: i32,
    /// Last computed safe scan distance against air targets
    /// (`TaskAttack::GetSafeDistances`).
    pub safe_distance_air: i32,
    /// Last computed safe scan distance against ground targets.
    pub safe_distance_ground: i32,
    /// Rally site (`DeterminePosition`).
    pub site: Point,
}

/// `TaskKillUnit::UnitDamage` — a unit's contribution to damage against the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitDamage {
    /// Unit.
    pub unit: UnitId,
    /// Expected damage.
    pub damage: i32,
}

/// `TaskKillUnit` — target destruction state (`taskkillunit.hpp:42`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KillUnitState {
    /// `unit_requests`.
    pub unit_requests: u16,
    /// `spotted_unit` — the `SpottedUnit` id in `AiPlayer::spotted_units`.
    pub spotted_unit: Option<u32>,
    /// `units`.
    pub units: Vec<UnitId>,
    /// `unit_damages`.
    pub unit_damages: Vec<UnitDamage>,
    /// `required_damage`.
    pub required_damage: u16,
    /// `hits`.
    pub hits: u16,
    /// `managed_unit`.
    pub managed_unit: Option<UnitId>,
    /// `weight_table` — the types used to finish the target.
    pub weight_table: super::weight::WeightTable,
    /// `seek_target`.
    pub seek_target: bool,
    /// `recruit_turn`.
    pub recruit_turn: i32,
    /// Target position (`spotted_unit->GetLastPosition`).
    pub target_position: Point,
    /// Target type.
    pub target_type: Option<UnitType>,
    /// Active recruit gathering this turn (`HoldRecruitsThisTurn`).
    pub recruits_held: bool,
}

/// `TaskMove` — movement state (`taskmove.hpp:41`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveState {
    /// `passenger`.
    pub passenger: Option<UnitId>,
    /// `minimum_distance`.
    pub minimum_distance: u16,
    /// `caution_level`.
    pub caution_level: CautionLevel,
    /// `passenger_waypoint`.
    pub passenger_waypoint: Point,
    /// `transport_waypoint`.
    pub transport_waypoint: Point,
    /// `destination_waypoint`.
    pub destination_waypoint: Point,
    /// `transporter_unit_type`.
    pub transporter_unit_type: Option<UnitType>,
    /// `passenger_destination`.
    pub passenger_destination: Point,
    /// `planned_path`.
    pub planned_path: Vec<Point>,
    /// `finish_on_arrival`.
    pub finish_on_arrival: bool,
    /// `is_optional`.
    pub is_optional: bool,
    /// `move_order_issued`.
    pub move_order_issued: bool,
    /// `path_result`.
    pub path_result: u8,
    /// Destination (`GetDestination`).
    pub destination: Point,
    /// The unit that moves.
    pub unit: Option<UnitId>,
    /// The order was issued and a path was requested.
    pub started: bool,
    /// The request's real destination before the transport split
    /// (C++ `passenger_destination`/`destination_waypoint`; `AttemptTransport`
    /// rewrites `destination_waypoint` to the boarding cell).
    pub final_destination: Point,
    /// The move needs a transporter (`TaskMove::AttemptTransport`).
    pub transport_needed: bool,
    /// The passenger is in the transporter's hold (C++ `ORDER_IDLE` +
    /// `parent_unit == transporter`); without the `EventUnitLoaded` event the
    /// task polls this flag.
    pub transport_loaded: bool,
    /// The `TaskTransport` that carries the passenger.
    pub transport_task: Option<u32>,
}

impl Default for MoveState {
    fn default() -> Self {
        Self {
            passenger: None,
            minimum_distance: 0,
            caution_level: CautionLevel::AvoidNextTurnsFire,
            passenger_waypoint: Point::ZERO,
            transport_waypoint: Point::ZERO,
            destination_waypoint: Point::ZERO,
            transporter_unit_type: None,
            passenger_destination: Point::ZERO,
            planned_path: Vec::new(),
            finish_on_arrival: false,
            is_optional: false,
            move_order_issued: false,
            path_result: TASKMOVE_RESULT_NONE,
            destination: Point::ZERO,
            unit: None,
            started: false,
            final_destination: Point::ZERO,
            transport_needed: false,
            transport_loaded: false,
            transport_task: None,
        }
    }
}

/// "No result" for `TaskMove::path_result`.
pub const TASKMOVE_RESULT_NONE: u8 = 0xFF;

/// `TaskAbstractSearch` — abstract search state (`taskabstractsearch.hpp`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchState {
    /// `point`.
    pub point: Point,
    /// `units`.
    pub units: Vec<UnitId>,
    /// `requestors`.
    pub requestors: i16,
}

/// `TaskRepair` (base for `TaskReload`/`TaskUpgrade`) — `taskrepair.hpp:43`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepairState {
    /// `target_unit`.
    pub target_unit: Option<UnitId>,
    /// `operator_unit`.
    pub operator_unit: Option<UnitId>,
    /// Waiting for the operator to arrive.
    pub operator_requested: bool,
}

/// `TaskRemoveRubble` (base for `TaskRemoveMines`) — `taskremoverubble.hpp:33`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoveRubbleState {
    /// `unit`.
    pub unit: Option<UnitId>,
    /// `target`.
    pub target: Option<UnitId>,
    /// Target cell (`target->grid`).
    pub site: Point,
    /// A unit request was made.
    pub unit_requested: bool,
}

/// `TaskGetResource` — supply state (`taskgetresource.hpp:32`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GetResourceState {
    /// `requestor` — who it is delivered to.
    pub requestor: Option<UnitId>,
    /// `supplier` — who delivers.
    pub supplier: Option<UnitId>,
    /// `source` — from where.
    pub source: Option<UnitId>,
    /// Transportation started.
    pub started: bool,
}

/// `TaskRendezvous` completion mode — the result of the `Primary/Secondary` callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendezvousMode {
    /// Both units are in place.
    Success,
    /// Failed.
    Failed,
}

/// `TaskDump`/`TaskTransport` — unload direction.
pub const DUMP_NONE: i16 = -1;

// ---------------------------------------------------------------------------
// TaskKind
// ---------------------------------------------------------------------------

/// State and type of a concrete task — the analog of `class Task*` from `task*.hpp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskKind {
    /// `TaskActivate`.
    Activate {
        /// `unit_to_activate`.
        unit_to_activate: Option<UnitId>,
        /// `unit_parent`.
        unit_parent: Option<UnitId>,
        /// Zone cleared (`EventZoneCleared`).
        zone_cleared: bool,
        /// Activation ordered.
        activate_issued: bool,
    },
    /// `TaskAssistMove`.
    AssistMove {
        /// `transporters`.
        transporters: Vec<UnitId>,
    },
    /// `TaskAttack`.
    Attack(Box<AttackState>),
    /// `TaskAttackReserve`.
    AttackReserve {
        /// `units`.
        units: Vec<UnitId>,
        /// `site`.
        site: Point,
        /// `total_worth`.
        total_worth: i32,
    },
    /// `TaskAutoSurvey`.
    AutoSurvey {
        /// `central_site`.
        central_site: Point,
        /// `unit`.
        unit: Option<UnitId>,
        /// `radius`.
        radius: u16,
        /// `direction`.
        direction: u16,
        /// `position`.
        position: Point,
        /// `destination`.
        destination: Point,
        /// `direction2`.
        direction2: u16,
        /// `continue_search`.
        continue_search: bool,
        /// `location_found`.
        location_found: bool,
        /// `current_radius`.
        current_radius: u16,
    },
    /// `TaskCheckAssaults`.
    CheckAssaults {
        /// Index of the current unit in the list (the C++ iterator is replaced by an index).
        index: usize,
        /// There are unfinished checks.
        active: bool,
    },
    /// `TaskClearZone`.
    ClearZone {
        /// `state` (`CLEARZONE_STATE_*`).
        state: u8,
        /// `zones` — sites requiring clearing.
        zones: Vec<Point>,
        /// `moving_unit`.
        moving_unit: Option<UnitId>,
    },
    /// `TaskCreateUnit`.
    CreateUnit {
        /// `unit_type`.
        unit_type: UnitType,
        /// `site`.
        site: Point,
        /// `op_state`.
        op_state: u8,
        /// `builder` (`TaskCreate::builder`).
        builder: Option<UnitId>,
    },
    /// `TaskCreateBuilding`.
    CreateBuilding {
        /// `unit_type`.
        unit_type: UnitType,
        /// `site`.
        site: Point,
        /// `op_state`.
        op_state: u8,
        /// `builder`.
        builder: Option<UnitId>,
        /// `building`.
        building: Option<UnitId>,
        /// `manager` — the `TaskManageBuildings` id (or `None`).
        manager: Option<u32>,
        /// `needs_water_platforms`.
        needs_water_platforms: bool,
        /// `tasks` — child tasks (rubble/mine removal, platform).
        tasks: Vec<u32>,
    },
    /// `TaskEscort`.
    Escort {
        /// `target`.
        target: Option<UnitId>,
        /// `escort`.
        escort: Option<UnitId>,
        /// `unit_type`.
        unit_type: UnitType,
        /// `is_escort_requested`.
        is_escort_requested: bool,
    },
    /// `TaskExplore`.
    Explore {
        /// `TaskAbstractSearch`.
        search: SearchState,
        /// `obtain_requests[UNIT_END]`.
        obtain_requests: Vec<UnitType>,
    },
    /// `TaskDefenseAssistant`.
    DefenseAssistant {
        /// `manager`.
        manager: Option<u32>,
        /// `unit_type`.
        unit_type: UnitType,
    },
    /// `TaskDefenseReserve`.
    DefenseReserve {
        /// `units`.
        units: Vec<UnitId>,
        /// `site`.
        site: Point,
    },
    /// `TaskDump`.
    Dump {
        /// `task_move`.
        task_move: Option<u32>,
        /// `transporter_unit`.
        transporter_unit: Option<UnitId>,
        /// `steps_limit`.
        steps_limit: i16,
        /// `direction`.
        direction: i16,
        /// `steps_counter`.
        steps_counter: i16,
        /// `destination`.
        destination: Point,
        /// `keep_searching`.
        keep_searching: bool,
    },
    /// `TaskFindMines`.
    FindMines {
        /// `TaskAbstractSearch`.
        search: SearchState,
    },
    /// `TaskFindPath`.
    FindPath {
        /// Request unit.
        unit: Option<UnitId>,
        /// Destination.
        destination: Point,
        /// Parent task waiting for a path.
        parent_move: Option<u32>,
        /// Request completed.
        finished: bool,
    },
    /// `TaskFrontalAttack`.
    FrontalAttack {
        /// `movement_in_progress`.
        movement_in_progress: bool,
        /// `spotted_unit`.
        spotted_unit: Option<u32>,
        /// `units1`.
        units1: Vec<UnitId>,
        /// `units2`.
        units2: Vec<UnitId>,
        /// `caution_level`.
        caution_level: CautionLevel,
        /// Target cell.
        site: Point,
    },
    /// `TaskGetMaterials`.
    GetMaterials {
        /// `TaskGetResource`.
        resource: GetResourceState,
        /// `materials_needed`.
        materials_needed: u16,
    },
    /// `TaskHabitatAssistant`.
    HabitatAssistant {
        /// `manager`.
        manager: Option<u32>,
    },
    /// `TaskKillUnit`.
    KillUnit(Box<KillUnitState>),
    /// `TaskManageBuildings`.
    ManageBuildings {
        /// `building_site`.
        building_site: Point,
        /// `units`.
        units: Vec<UnitId>,
        /// `tasks` — child `TaskCreateBuilding`s.
        tasks: Vec<u32>,
        /// `cargo_demand`.
        cargo_demand: Cargo,
        /// Primary initialization done.
        initialized: bool,
    },
    /// `TaskMineAssistant`.
    MineAssistant {
        /// `manager`.
        manager: Option<u32>,
    },
    /// `TaskMove`.
    Move(Box<MoveState>),
    /// `TaskMoveHome`.
    MoveHome {
        /// `unit`.
        unit: Option<UnitId>,
        /// Target cell.
        site: Point,
    },
    /// `TaskObtainUnits`.
    ObtainUnits {
        /// `units` — required types.
        units: Vec<UnitType>,
        /// `point`.
        point: Point,
        /// `init`.
        init: bool,
        /// `reinit`.
        reinit: bool,
    },
    /// `TaskPlaceMines`.
    PlaceMines {
        /// `units`.
        units: Vec<UnitId>,
        /// `mine_layer_count`.
        mine_layer_count: u16,
        /// `sea_mine_layer_count`.
        sea_mine_layer_count: u16,
    },
    /// `TaskConnectionAssistant`.
    ConnectionAssistant {
        /// `manager`.
        manager: Option<u32>,
    },
    /// `TaskRadarAssistant`.
    RadarAssistant {
        /// `manager`.
        manager: Option<u32>,
    },
    /// `TaskFrontierAssistant`.
    FrontierAssistant {
        /// The site around which the frontier is searched.
        site: Point,
    },
    /// `TaskPowerAssistant`.
    PowerAssistant {
        /// `manager`.
        manager: Option<u32>,
    },
    /// `TaskReload` (a `TaskRepair` subclass).
    Reload {
        /// `TaskRepair`.
        repair: RepairState,
    },
    /// `TaskRemoveMines` (a `TaskRemoveRubble` subclass).
    RemoveMines {
        /// `TaskRemoveRubble`.
        rubble: RemoveRubbleState,
    },
    /// `TaskRemoveRubble`.
    RemoveRubble(RemoveRubbleState),
    /// `TaskRendezvous`.
    Rendezvous {
        /// `unit1`.
        unit1: Option<UnitId>,
        /// `unit2`.
        unit2: Option<UnitId>,
        /// Completion mode.
        mode: RendezvousMode,
    },
    /// `TaskRepair`.
    Repair {
        /// `TaskRepair`.
        repair: RepairState,
    },
    /// `TaskRetreat`.
    Retreat {
        /// `unit_to_retreat`.
        unit_to_retreat: Option<UnitId>,
        /// `search_radius`.
        search_radius: i16,
        /// `position`.
        position: Point,
        /// `direction`.
        direction: i16,
        /// `steps_in_direction`.
        steps_in_direction: i16,
        /// `caution_level`.
        caution_level: CautionLevel,
        /// `found_valid_position`.
        found_valid_position: i8,
        /// Found retreat site.
        destination: Point,
    },
    /// `TaskSearchDestination`.
    SearchDestination {
        /// `unit`.
        unit: Option<UnitId>,
        /// `initial_position`.
        initial_position: Point,
        /// `search_task` — id `TaskAbstractSearch`.
        search_task: Option<u32>,
        /// `search_radius`.
        search_radius: u16,
        /// `index`.
        index: i16,
        /// `continue_search_at_radius`.
        continue_search_at_radius: bool,
        /// `is_doomed`.
        is_doomed: bool,
        /// `valid_sites`.
        valid_sites: i16,
        /// `searched_sites`.
        searched_sites: i16,
        /// `enterable_sites`.
        enterable_sites: i16,
        /// Found site.
        destination: Point,
    },
    /// `TaskScavenge`.
    Scavenge {
        /// `units`.
        units: Vec<UnitId>,
        /// `wait_for_unit`.
        wait_for_unit: bool,
    },
    /// `TaskSupportAttack`.
    SupportAttack {
        /// `units`.
        units: Vec<UnitId>,
        /// `unit_type1`.
        unit_type1: UnitType,
        /// `unit_type2`.
        unit_type2: UnitType,
        /// `unit_flags`.
        unit_flags: u32,
    },
    /// `TaskSurvey`.
    Survey {
        /// `TaskAbstractSearch`.
        search: SearchState,
    },
    /// `TaskTransport`.
    Transport {
        /// `transporter_unit_type`.
        transporter_unit_type: UnitType,
        /// `unit_transporter`.
        unit_transporter: Option<UnitId>,
        /// `move_tasks` — clients.
        move_tasks: Vec<u32>,
        /// `task_move`.
        task_move: Option<u32>,
        /// `task_obtain_units`.
        task_obtain_units: Option<u32>,
        /// The transporter's own `TaskMove` child (C++ `TaskMove` created in
        /// `Execute`); kept so that a new one is not appended every turn.
        transporter_move: Option<u32>,
        /// The cell the transporter unloads the current client on
        /// (`TaskTransport_FinishTransport`); the release is observed here because
        /// the port has no `EventUnitUnloaded` notification.
        unload_site: Option<Point>,
    },
    /// `TaskUpdateTerrain`.
    UpdateTerrain {
        /// `location`.
        location: Point,
    },
    /// `TaskUpgrade` (a `TaskRepair` subclass).
    Upgrade {
        /// `TaskRepair`.
        repair: RepairState,
    },
    /// `TaskWaitToAttack`.
    WaitToAttack {
        /// `attacker`.
        attacker: Option<UnitId>,
    },
}

impl Default for TaskKind {
    fn default() -> Self {
        Self::MineAssistant { manager: None }
    }
}

// ---------------------------------------------------------------------------
// Task class constructors (C++ `Task*::Task*` constructors)
// ---------------------------------------------------------------------------

impl TaskKind {
    /// `TaskCreateUnit::TaskCreateUnit(unit_type, task, site)`.
    pub fn create_unit(unit_type: UnitType, site: Point) -> Self {
        Self::CreateUnit {
            unit_type,
            site,
            op_state: create_unit_state::INITIALIZING,
            builder: None,
        }
    }

    /// `TaskCreateBuilding::TaskCreateBuilding(task, priority, unit_type, site, manager)`.
    pub fn create_building(unit_type: UnitType, site: Point, manager: Option<u32>) -> Self {
        Self::CreateBuilding {
            unit_type,
            site,
            op_state: create_building_state::INITIALIZING,
            builder: None,
            building: None,
            manager,
            needs_water_platforms: false,
            tasks: Vec::new(),
        }
    }

    /// `TaskTransport::TaskTransport(TaskMove*, transporter)`
    /// (`tasktransport.cpp:36`) — the task starts with its first client and the
    /// transporter type it needs; `Init` requests the transporter when the team
    /// does not own one yet.
    pub fn transport(client_task: u32, transporter_unit_type: UnitType) -> Self {
        Self::Transport {
            transporter_unit_type,
            unit_transporter: None,
            move_tasks: vec![client_task],
            task_move: Some(client_task),
            task_obtain_units: None,
            transporter_move: None,
            unload_site: None,
        }
    }

    /// `TaskObtainUnits::TaskObtainUnits(task, point)` (`taskobtainunits.cpp:26`):
    /// the C++ constructor sets `init = reinit = true`.
    pub fn obtain_units(point: Point) -> Self {
        Self::ObtainUnits {
            units: Vec::new(),
            point,
            init: true,
            reinit: true,
        }
    }

    /// `TaskMove::TaskMove(unit, task, minimum_distance, caution_level, destination, callback)`.
    pub fn move_to(
        unit: UnitId,
        destination: Point,
        minimum_distance: u16,
        caution_level: CautionLevel,
    ) -> Self {
        let mut state = MoveState::default();
        state.unit = Some(unit);
        state.destination = destination;
        state.destination_waypoint = destination;
        state.minimum_distance = minimum_distance;
        state.caution_level = caution_level;
        Self::Move(Box::new(state))
    }

    /// `TaskRetreat::TaskRetreat(unit, task, priority, caution_level)`.
    pub fn retreat(unit: UnitId, _task: Option<u32>, caution_level: CautionLevel) -> Self {
        Self::Retreat {
            unit_to_retreat: Some(unit),
            search_radius: 3,
            position: Point::ZERO,
            direction: 0,
            steps_in_direction: 0,
            caution_level,
            found_valid_position: 0,
            destination: Point::ZERO,
        }
    }

    /// `TaskRepair::TaskRepair(unit)`.
    pub fn repair(unit: UnitId) -> Self {
        Self::Repair {
            repair: RepairState {
                target_unit: Some(unit),
                operator_unit: None,
                operator_requested: false,
            },
        }
    }

    /// `TaskReload::TaskReload(unit)`.
    pub fn reload(unit: UnitId) -> Self {
        Self::Reload {
            repair: RepairState {
                target_unit: Some(unit),
                operator_unit: None,
                operator_requested: false,
            },
        }
    }

    /// `TaskUpgrade::TaskUpgrade(unit)`.
    pub fn upgrade(unit: UnitId) -> Self {
        Self::Upgrade {
            repair: RepairState {
                target_unit: Some(unit),
                operator_unit: None,
                operator_requested: false,
            },
        }
    }

    /// `TaskWaitToAttack::TaskWaitToAttack(unit)`.
    pub fn wait_to_attack(unit: UnitId) -> Self {
        Self::WaitToAttack {
            attacker: Some(unit),
        }
    }

    /// `TaskAttack::TaskAttack(spotted_unit, priority)`.
    pub fn attack(target_team: TeamId, site: Point, initial_target: Option<u32>) -> Self {
        let mut state = AttackState::default();
        state.target_team = Some(target_team);
        state.site = site;
        state.initial_target = initial_target;
        // C++ sets `access_flags = 0` in the constructor; `Init` sets the mobile
        // mask and `BeginTurn` narrows it to land or sea.
        state.access_flags = 0;
        Self::Attack(Box::new(state))
    }

    /// `TaskKillUnit::TaskKillUnit(task_attack, spotted_unit, priority)`.
    pub fn kill_unit(target: UnitId, target_type: UnitType, position: Point) -> Self {
        let mut state = KillUnitState::default();
        state.target_position = position;
        state.target_type = Some(target_type);
        let _ = target;
        state.required_damage = u16::MAX;
        state.recruit_turn = -1;
        let _ = TASK_PRIORITY_MOVE;
        Self::KillUnit(Box::new(state))
    }

    /// `TaskManageBuildings::TaskManageBuildings(team, site)`.
    pub fn manage_buildings(site: Point) -> Self {
        Self::ManageBuildings {
            building_site: site,
            units: Vec::new(),
            tasks: Vec::new(),
            cargo_demand: Cargo::init(),
            initialized: false,
        }
    }

    /// `TaskMineAssistant::TaskMineAssistant(team)`.
    pub fn mine_assistant() -> Self {
        Self::MineAssistant { manager: None }
    }

    /// `TaskSurvey::TaskSurvey(team, point)`.
    pub fn survey(point: Point) -> Self {
        Self::Survey {
            search: SearchState {
                point,
                ..SearchState::default()
            },
        }
    }

    /// `TaskExplore::TaskExplore(team, point)`.
    pub fn explore(point: Point) -> Self {
        Self::Explore {
            search: SearchState {
                point,
                ..SearchState::default()
            },
            obtain_requests: Vec::new(),
        }
    }

    /// Prototype `TaskFindMines::TaskFindMines(team, point)`.
    pub fn find_mines(point: Point) -> Self {
        Self::FindMines {
            search: SearchState {
                point,
                ..SearchState::default()
            },
        }
    }

    /// `TaskUpdateTerrain::TaskUpdateTerrain(team)`.
    pub fn update_terrain() -> Self {
        Self::UpdateTerrain {
            location: Point::ZERO,
        }
    }

    /// `TaskScavenge::TaskScavenge(team)`.
    pub fn scavenge() -> Self {
        Self::Scavenge {
            units: Vec::new(),
            wait_for_unit: false,
        }
    }

    /// `TaskPlaceMines::TaskPlaceMines(team)`.
    pub fn place_mines() -> Self {
        Self::PlaceMines {
            units: Vec::new(),
            mine_layer_count: 0,
            sea_mine_layer_count: 0,
        }
    }

    /// Task type (`GetType`).
    pub fn get_type(&self) -> u8 {
        match self {
            Self::Activate { .. } => TASK_TYPE_ACTIVATE,
            Self::AssistMove { .. } => TASK_TYPE_ASSIST_MOVE,
            Self::Attack(_) => TASK_TYPE_ATTACK,
            Self::AttackReserve { .. } => TASK_TYPE_ATTACK_RESERVE,
            Self::AutoSurvey { .. } => TASK_TYPE_AUTO_SURVEY,
            Self::CheckAssaults { .. } => TASK_TYPE_CHECK_ASSAULTS,
            Self::ClearZone { .. } => TASK_TYPE_CLEAR_ZONE,
            Self::CreateUnit { .. } => TASK_TYPE_CREATE_UNIT,
            Self::CreateBuilding { .. } => TASK_TYPE_CREATE_BUILDING,
            Self::Escort { .. } => TASK_TYPE_ESCORT,
            Self::Explore { .. } => TASK_TYPE_EXPLORE,
            Self::DefenseAssistant { .. } => TASK_TYPE_DEFENSE_ASSISTANT,
            Self::DefenseReserve { .. } => TASK_TYPE_DEFENSE_RESERVE,
            Self::Dump { .. } => TASK_TYPE_DUMP,
            Self::FindMines { .. } => TASK_TYPE_FIND_MINES,
            Self::FindPath { .. } => TASK_TYPE_FIND_PATH,
            Self::FrontalAttack { .. } => TASK_TYPE_FRONTAL_ATTACK,
            Self::GetMaterials { .. } => TASK_TYPE_GET_MATERIALS,
            Self::HabitatAssistant { .. } => TASK_TYPE_HABITAT_ASSISTANT,
            Self::KillUnit(_) => TASK_TYPE_KILL_UNIT,
            Self::ManageBuildings { .. } => TASK_TYPE_MANAGE_BUILDINGS,
            Self::MineAssistant { .. } => TASK_TYPE_MINE_ASSISTANT,
            Self::Move(_) => TASK_TYPE_MOVE,
            Self::MoveHome { .. } => TASK_TYPE_MOVE_HOME,
            Self::ObtainUnits { .. } => TASK_TYPE_OBTAIN_UNITS,
            Self::PlaceMines { .. } => TASK_TYPE_PLACE_MINES,
            Self::ConnectionAssistant { .. } => TASK_TYPE_CONNECTION_ASSISTANT,
            Self::RadarAssistant { .. } => TASK_TYPE_RADAR_ASSISTANT,
            Self::FrontierAssistant { .. } => TASK_TYPE_FRONTIER_ASSISTANT,
            Self::PowerAssistant { .. } => TASK_TYPE_POWER_ASSISTANT,
            Self::Reload { .. } => TASK_TYPE_RELOAD,
            Self::RemoveMines { .. } => TASK_TYPE_REMOVE_MINES,
            Self::RemoveRubble(_) => TASK_TYPE_REMOVE_RUBBLE,
            Self::Rendezvous { .. } => TASK_TYPE_RENDEZVOUS,
            Self::Repair { .. } => TASK_TYPE_REPAIR,
            Self::Retreat { .. } => TASK_TYPE_RETREAT,
            Self::SearchDestination { .. } => TASK_TYPE_SEARCH_DESTINATION,
            Self::Scavenge { .. } => TASK_TYPE_SCAVENGE,
            Self::SupportAttack { .. } => TASK_TYPE_SUPPORT_ATTACK,
            Self::Survey { .. } => TASK_TYPE_SURVEY,
            Self::Transport { .. } => TASK_TYPE_TRANSPORT,
            Self::UpdateTerrain { .. } => TASK_TYPE_UPDATE_TERRAIN,
            Self::Upgrade { .. } => TASK_TYPE_UPGRADE,
            Self::WaitToAttack { .. } => TASK_TYPE_WAIT_TO_ATTACK,
        }
    }
}

// ---------------------------------------------------------------------------
// TaskCtx — task + game at once
// ---------------------------------------------------------------------------

/// Task handler context: the task manager and the game state.
pub struct TaskCtx<'a> {
    /// Task manager (queues, lists, registration).
    pub tm: &'a mut TaskManager,
    /// Game state.
    pub world: &'a mut GameState,
}

impl<'a> TaskCtx<'a> {
    /// Creates the context.
    pub fn new(tm: &'a mut TaskManager, world: &'a mut GameState) -> Self {
        Self { tm, world }
    }

    /// Unit copy (`Hash_UnitHash[...]`).
    pub fn unit(&self, id: UnitId) -> Option<UnitInfo> {
        self.world.unit(id).cloned()
    }

    /// Whether the unit exists.
    pub fn has_unit(&self, id: UnitId) -> bool {
        self.world.has_unit(id)
    }

    /// Writes the unit.
    pub fn write_unit(&mut self, id: UnitId, unit: UnitInfo) {
        self.world.write_unit(id, unit);
    }

    /// Order data (`move_to`/`fire_on`).
    pub fn order_data(&self, id: UnitId) -> crate::game::OrderData {
        self.world.order_data(id)
    }

    /// Sets the order data.
    pub fn set_order_data(&mut self, id: UnitId, data: crate::game::OrderData) {
        self.world.set_order_data(id, data);
    }

    /// `UnitsManager_SetNewOrder` (`units_manager.cpp:1945`) for a unit.
    pub fn set_new_order(&mut self, id: UnitId, order: UnitOrder, state: UnitOrderState) {
        self.world.set_order(id, order, state);
    }

    /// `Task_IsReadyToTakeOrders` for a unit.
    pub fn ready_to_take_orders(&self, unit: UnitId) -> bool {
        super::task::task_is_ready_to_take_orders(self.world, unit)
    }

    /// `Task_RemindMoveFinished`.
    pub fn remind_move_finished(&mut self, unit: UnitId, priority: bool) {
        self.tm.remind_move_finished(self.world, unit, priority);
    }

    /// `Task::RemindTurnEnd` for a task.
    pub fn remind_turn_end(&mut self, task_id: u32, priority: bool) {
        self.tm.remind_turn_end(task_id, priority);
    }

    /// `Task::RemindTurnStart` for a task.
    pub fn remind_turn_start(&mut self, task_id: u32, priority: bool) {
        self.tm.remind_turn_start(task_id, priority);
    }

    /// `UnitInfo::AddTask` — the unit's "top" task.
    pub fn attach(&mut self, task_id: u32, unit: UnitId) {
        self.tm.attach_task_to_unit(unit, task_id);
    }

    /// `UnitInfo::RemoveTask` (with a reminder to the new top task if `mode`).
    pub fn detach(&mut self, task_id: u32, unit: UnitId, mode: bool) {
        self.tm
            .detach_task_from_unit(self.world, task_id, unit, mode);
    }

    /// `TaskManager::ClearUnitTasksAndRemindAvailable`.
    pub fn clear_unit_tasks(&mut self, unit: UnitId, priority: bool) {
        self.tm
            .clear_unit_tasks_and_remind_available(self.world, unit, priority);
    }

    /// Registers a task (`TaskManager::AppendTask`).
    pub fn append_task(
        &mut self,
        team: TeamId,
        parent: Option<u32>,
        priority: u16,
        kind: TaskKind,
    ) -> u32 {
        self.tm
            .append_task(self.world, Task::new(0, team, parent, priority, kind))
    }

    /// `Task::RemoveSelf` for a task.
    pub fn remove_self(&mut self, task_id: u32) {
        self.tm.remove_task(self.world, task_id);
    }

    /// Task base priority.
    pub fn priority_of(&self, task_id: u32) -> u16 {
        self.tm
            .task(task_id)
            .map(|task| task.base_priority)
            .unwrap_or(0)
    }

    /// Sets the priority (`Task::SetPriority`).
    pub fn set_priority(&mut self, task_id: u32, priority: u16) {
        if let Some(task) = self.tm.task_mut(task_id) {
            task.base_priority = priority;
        }
    }

    /// Sets the parent (`Task::SetParent`).
    pub fn set_parent(&mut self, task_id: u32, parent: Option<u32>) {
        if let Some(task) = self.tm.task_mut(task_id) {
            task.parent = parent;
        }
    }

    /// `Task::SetProcessingNeeded`.
    pub fn set_processing_needed(&mut self, task_id: u32, value: bool) {
        if let Some(task) = self.tm.task_mut(task_id) {
            task.needs_processing = value;
        }
    }

    /// Registers a unit as "having lost its task" (`RemindAvailable`).
    pub fn remind_available(&mut self, unit: UnitId, priority: bool) {
        self.tm.append_reminder(
            self.world,
            super::reminders::Reminder::Available { unit },
            priority,
        );
    }

    /// `AiPlayer::CreateBuilding` for the team.
    pub fn create_building(
        &mut self,
        team: TeamId,
        unit_type: UnitType,
        site: Point,
        task: Option<u32>,
    ) {
        super::strategy::create_building(self.world, team, unit_type, site, task);
    }

    /// Issues an `ORDER_MOVE` with a target (like `UnitsManager_SetNewOrder`).
    pub fn issue_move(&mut self, unit_id: UnitId, site: Point) -> bool {
        let Some(mut unit) = self.unit(unit_id) else {
            return false;
        };

        if !self.ready_to_take_orders(unit_id)
            && unit.orders != UnitOrder::Move
            && unit.orders != UnitOrder::MoveToAttack
        {
            return false;
        }

        unit.target_grid = site;
        self.write_unit(unit_id, unit);

        let mut data = self.order_data(unit_id);
        data.move_to = site;
        self.set_order_data(unit_id, data);

        self.set_new_order(unit_id, UnitOrder::Move, UnitOrderState::Init);

        true
    }

    /// `UnitInfo::BuildOrder` (`unitinfo.cpp:1380`): build queue + `ORDER_BUILD`.
    pub fn build_order(
        &mut self,
        unit_id: UnitId,
        unit_type: UnitType,
        site: Point,
        build_rate: u16,
    ) -> bool {
        let Some(mut unit) = self.unit(unit_id) else {
            return false;
        };

        let turns = super::strategy::turns_to_build(self.world, unit.team, unit_type);
        unit.build_time = turns as u8;
        unit.build_rate = build_rate;
        unit.build_list.clear();
        unit.build_list.push(unit_type);
        unit.target_grid = site;
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
        self.write_unit(unit_id, unit);

        let mut data = self.order_data(unit_id);
        data.move_to = site;
        self.set_order_data(unit_id, data);

        self.set_new_order(unit_id, UnitOrder::Build, UnitOrderState::Init);

        let _ = unit_order::start_building;
        true
    }

    /// `UnitsManager_SetNewOrder(unit, ORDER_HALT_BUILDING(_2), BUILD_CANCEL)`.
    pub fn halt_building(&mut self, unit_id: UnitId) {
        self.set_new_order(
            unit_id,
            UnitOrder::HaltBuilding,
            UnitOrderState::BuildCancel,
        );
    }

    /// Team unit list with flags (`Task_GetReadyUnitsCount` filter).
    pub fn team_units_in(&self, team: TeamId, kind: UnitListKind) -> Vec<UnitId> {
        self.world
            .unit_list(kind)
            .iter()
            .copied()
            .filter(|id| {
                self.world
                    .unit(*id)
                    .map(|unit| unit.team == team)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Copy of a list of units.
    pub fn list(&self, kind: UnitListKind) -> Vec<UnitId> {
        self.world.unit_list(kind).to_vec()
    }

    /// `AiPlayer::IsEnemyTeam`.
    pub fn is_enemy_team(&self, team: TeamId) -> bool {
        super::task::is_enemy_team(self.world, team)
    }

    /// `Access_IsAccessible` for a type.
    pub fn accessible(&self, unit_type: UnitType, team: TeamId, grid: Point, flags: u32) -> bool {
        crate::access::is_accessible(self.world, unit_type, team, grid, flags) != 0
    }
}

/// Task registration (a free function for `task.rs`).
pub fn append_task(
    manager: &mut TaskManager,
    world: &mut GameState,
    team: TeamId,
    parent: Option<u32>,
    priority: u16,
    kind: TaskKind,
) -> u32 {
    manager.append_task(world, Task::new(0, team, parent, priority, kind))
}

/// `RemindTurnStart` handler (a wrapper for `reminders.rs`).
pub fn task_begin_turn(manager: &mut TaskManager, world: &mut GameState, task_id: u32) {
    manager.task_begin_turn(world, task_id);
}

/// `RemindTurnEnd` handler (a wrapper for `reminders.rs`).
pub fn task_end_turn(manager: &mut TaskManager, world: &mut GameState, task_id: u32) {
    manager.task_end_turn(world, task_id);
}

/// `RemindMoveFinished` handler (a wrapper for `reminders.rs`).
pub fn task_execute(manager: &mut TaskManager, world: &mut GameState, task_id: u32, unit: UnitId) {
    manager.task_execute(world, task_id, unit);
}

/// `TaskRetreat` state (`taskretreat.hpp:28` fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetreatState {
    /// `unit_to_retreat`.
    pub unit_to_retreat: Option<UnitId>,
    /// `search_radius`.
    pub search_radius: i16,
    /// `position`.
    pub position: Point,
    /// `direction`.
    pub direction: i16,
    /// `steps_in_direction`.
    pub steps_in_direction: i16,
    /// `caution_level`.
    pub caution_level: CautionLevel,
    /// `found_valid_position`.
    pub found_valid_position: i8,
    /// Found retreat site.
    pub destination: Point,
}

impl Default for RetreatState {
    fn default() -> Self {
        Self {
            unit_to_retreat: None,
            search_radius: 0,
            position: Point::ZERO,
            direction: 0,
            steps_in_direction: 0,
            caution_level: CautionLevel::None,
            found_valid_position: 0,
            destination: Point::ZERO,
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle: dispatch by task class
// ---------------------------------------------------------------------------

impl TaskKind {
    /// `Task::GetPriority()` — base by default; overridden in classes.
    pub fn get_priority(&self, world: &GameState, task: &Task) -> u16 {
        match self {
            // `TaskCreateUnit::GetPriority` (`taskcreateunit.cpp:56`).
            Self::CreateUnit { .. } => match task.parent {
                Some(parent) => world
                    .ai
                    .task_manager
                    .task(parent)
                    .map(|parent| parent.kind.get_priority(world, parent))
                    .unwrap_or(task.base_priority),
                None => task.base_priority,
            },
            // `TaskObtainUnits::GetPriority` (`taskobtainunits.cpp:157`).
            Self::ObtainUnits { .. } => match task.parent {
                Some(parent) => world
                    .ai
                    .task_manager
                    .task(parent)
                    .map(|parent| parent.kind.get_priority(world, parent))
                    .unwrap_or(super::task::TASK_PRIORITY_MOVE),
                None => super::task::TASK_PRIORITY_MOVE,
            },
            // `TaskGetMaterials::GetPriority` (`taskgetmaterials.cpp:39`) — the
            // parent's effective priority; without a parent the base one.
            Self::GetMaterials { .. } => match task.parent {
                Some(parent) => world
                    .ai
                    .task_manager
                    .task(parent)
                    .map(|parent| parent.kind.get_priority(world, parent))
                    .unwrap_or(task.base_priority),
                None => task.base_priority,
            },
            // `TaskAttack::GetPriority` (`taskattack.cpp:101`) — the first
            // target's priority while there is a target, otherwise the
            // "least urgent in my tier" penalty.
            Self::Attack(state) => {
                let target = state
                    .secondary_targets
                    .first()
                    .copied()
                    .or(state.kill_unit_task);

                match target.and_then(|task_id| world.ai.task_manager.task(task_id)) {
                    Some(target) => target.kind.get_priority(world, target),
                    None => task
                        .base_priority
                        .wrapping_add(super::task::TASK_PRIORITY_TOLERANCE),
                }
            }
            // `TaskKillUnit::GetPriority` (`taskkillunit.cpp:553`) — the base
            // priority plus the target flags computed without an attacker
            // (`AiAttack_GetTargetFlags(nullptr, target, team)`); without a
            // target the task gets the `TASK_PRIORITY_TOLERANCE` penalty.
            Self::KillUnit(state) => {
                let target = state
                    .spotted_unit
                    .and_then(|index| {
                        world
                            .ai
                            .players
                            .get(task.team.0 as usize)
                            .and_then(|player| player.spotted_units.get(index as usize))
                    })
                    .and_then(|spotted| spotted.unit)
                    .and_then(|unit_id| world.unit(unit_id));

                match target {
                    Some(target) => task.base_priority.wrapping_add(
                        super::attack::get_target_flags(world, None, target, task.team) as u16,
                    ),
                    None => task
                        .base_priority
                        .wrapping_add(super::task::TASK_PRIORITY_TOLERANCE),
                }
            }
            _ => task.base_priority,
        }
    }

    /// `Task::GetCautionLevel(unit)` — class overrides.
    pub fn get_caution_level(
        &self,
        world: &GameState,
        task: &Task,
        unit: &UnitInfo,
    ) -> Option<CautionLevel> {
        let _ = task;
        match self {
            Self::Attack(state) => Some(combat::attack_caution_level(world, state, unit)),
            Self::FrontalAttack { caution_level, .. } => Some(*caution_level),
            Self::Retreat { caution_level, .. } => Some(*caution_level),
            Self::Move(state) => Some(state.caution_level),
            _ => {
                let _ = (world, unit);
                None
            }
        }
    }

    /// `Task::GetBounds(Rect*)`.
    pub fn get_bounds(&self, world: &GameState, header: TaskHeader) -> Rect {
        let _ = world;
        let _ = header;
        match self {
            Self::CreateBuilding {
                site, unit_type, ..
            } => {
                let size = if crate::access::unit_type_flags(*unit_type)
                    & crate::enums::unit_flags::BUILDING
                    != 0
                {
                    2
                } else {
                    1
                };
                Rect {
                    ulx: i32::from(site.x),
                    uly: i32::from(site.y),
                    lrx: i32::from(site.x) + size,
                    lry: i32::from(site.y) + size,
                }
            }
            Self::CreateUnit { site, .. } => Rect {
                ulx: i32::from(site.x),
                uly: i32::from(site.y),
                lrx: i32::from(site.x) + 1,
                lry: i32::from(site.y) + 1,
            },
            Self::Move(state) => Rect {
                ulx: i32::from(state.destination.x),
                uly: i32::from(state.destination.y),
                lrx: i32::from(state.destination.x),
                lry: i32::from(state.destination.y),
            },
            Self::KillUnit(state) => Rect {
                ulx: i32::from(state.target_position.x),
                uly: i32::from(state.target_position.y),
                lrx: i32::from(state.target_position.x),
                lry: i32::from(state.target_position.y),
            },
            Self::Attack(state) => Rect {
                ulx: i32::from(state.site.x),
                uly: i32::from(state.site.y),
                lrx: i32::from(state.site.x),
                lry: i32::from(state.site.y),
            },
            Self::ManageBuildings { building_site, .. } => Rect {
                ulx: i32::from(building_site.x),
                uly: i32::from(building_site.y),
                lrx: i32::from(building_site.x),
                lry: i32::from(building_site.y),
            },
            // `TaskTransport::GetBounds` (`tasktransport.cpp:400`): the
            // transporter's cell, or the last boarding waypoint.
            Self::Transport {
                unit_transporter,
                task_move,
                ..
            } => {
                let site = unit_transporter
                    .and_then(|id| world.unit(id).map(|unit| unit.grid))
                    .or_else(|| {
                        task_move.and_then(|id| match world.ai.task_manager.task(id) {
                            Some(task) => match &task.kind {
                                Self::Move(state) => Some(state.transport_waypoint),
                                _ => None,
                            },
                            None => None,
                        })
                    })
                    .unwrap_or(Point::ZERO);

                Rect {
                    ulx: i32::from(site.x),
                    uly: i32::from(site.y),
                    lrx: i32::from(site.x) + 1,
                    lry: i32::from(site.y) + 1,
                }
            }
            Self::SupportAttack { .. } | Self::Escort { .. } | Self::DefenseReserve { .. } => {
                Rect::default()
            }
            _ => Rect::default(),
        }
    }

    /// `Task::IsUnitUsable(unit)` — `false` by default.
    pub fn is_unit_usable(&self, world: &GameState, header: TaskHeader, unit: UnitId) -> bool {
        match self {
            Self::Attack(state) => combat::attack_is_unit_usable(world, header, state, unit),
            Self::KillUnit(state) => combat::kill_unit_is_unit_usable(world, header, state, unit),
            Self::AttackReserve { .. } => {
                combat::attack_reserve_is_unit_usable(world, header, unit)
            }
            Self::ManageBuildings { .. } => {
                build::manage_buildings_is_unit_usable(world, header, unit)
            }
            Self::ObtainUnits { .. } => {
                create::obtain_units_is_unit_usable(world, header, self, unit)
            }
            Self::DefenseReserve { .. } => {
                combat::defense_reserve_is_unit_usable(world, header, unit)
            }
            Self::PlaceMines { .. } => misc::place_mines_is_unit_usable(world, header, unit),
            Self::Scavenge { .. } => misc::scavenge_is_unit_usable(world, header, unit),
            Self::SupportAttack { .. } => {
                combat::support_attack_is_unit_usable(world, header, self, unit)
            }
            Self::AssistMove { .. } => move_tasks::assist_move_is_unit_usable(world, header, unit),
            Self::Explore { .. } => search::explore_is_unit_usable(world, header, self, unit),
            _ => {
                let _ = (world, header, unit);
                false
            }
        }
    }

    /// `Task::IsUnitTransferable(unit)` — `true` by default, considering the parent.
    pub fn is_unit_transferable(&self, world: &GameState, task: &Task, unit: &UnitInfo) -> bool {
        match self {
            Self::Attack(_)
            | Self::KillUnit(_)
            | Self::FrontalAttack { .. }
            | Self::Retreat { .. } => combat::is_unit_transferable(world, task, unit),
            Self::Repair { .. } | Self::Reload { .. } | Self::Upgrade { .. } => false,
            // `TaskTransport::IsUnitTransferable` (`tasktransport.cpp:362`):
            // every unit except the transporter itself is transferable.
            Self::Transport {
                unit_transporter, ..
            } => unit_transporter.map(|id| id != unit.id).unwrap_or(true),
            _ => match task.parent {
                Some(parent) => match world.ai.task_manager.task(parent) {
                    Some(parent_task) => {
                        parent_task
                            .kind
                            .is_unit_transferable(world, parent_task, unit)
                    }
                    None => true,
                },
                None => true,
            },
        }
    }

    /// `Task::IsNeeded()` — overrides (CreateBuilding, ObtainUnits, …).
    pub fn is_needed_kind(&self, world: &GameState) -> bool {
        let _ = world;
        match self {
            Self::Attack(_) => true,
            Self::KillUnit(_) => true,
            Self::CreateBuilding { op_state, .. } => *op_state < create_building_state::BUILDING,
            // `TaskTransport::IsNeeded` (`tasktransport.cpp:419`).
            Self::Transport {
                unit_transporter,
                move_tasks,
                ..
            } => unit_transporter.is_none() && !move_tasks.is_empty(),
            _ => true,
        }
    }

    /// `Task::IsEndTurnNeeded()` — `true` by default.
    pub fn is_end_turn_needed_kind(&self) -> bool {
        match self {
            // `TaskCreateBuilding::IsEndTurnNeeded` (`taskcreatebuilding.cpp:699`).
            Self::CreateBuilding {
                builder,
                op_state,
                tasks,
                ..
            } => {
                builder.is_some()
                    || (*op_state != create_building_state::GETTING_BUILDER
                        && *op_state < create_building_state::BUILDING
                        && tasks.is_empty())
            }
            Self::CreateUnit { .. } => true,
            Self::ObtainUnits { .. } => true,
            // `TaskTransport::IsEndTurnNeeded` (`tasktransport.cpp:494`).
            Self::Transport {
                unit_transporter, ..
            } => unit_transporter.is_some(),
            _ => true,
        }
    }

    /// `Task::IsThinking()` — `false` by default.
    pub fn is_thinking_kind(&self) -> bool {
        matches!(
            self,
            Self::CheckAssaults { .. }
                | Self::ClearZone { .. }
                | Self::FindPath { .. }
                | Self::WaitToAttack { .. }
        )
    }

    /// `WriteStatusLog()`.
    pub fn write_status_log(&self, task: &Task) -> String {
        let name = super::task::task_get_name(self.get_type());
        match self {
            Self::Move(state) => format!(
                "Move to [{},{}]",
                state.destination.x + 1,
                state.destination.y + 1
            ),
            Self::CreateBuilding {
                unit_type,
                site,
                op_state,
                ..
            } => format!(
                "Create {:?} at [{},{}] state {}",
                unit_type,
                site.x + 1,
                site.y + 1,
                op_state
            ),
            Self::CreateUnit {
                unit_type, site, ..
            } => {
                format!(
                    "Create a {:?} at [{},{}]",
                    unit_type,
                    site.x + 1,
                    site.y + 1
                )
            }
            Self::Attack(_) => "Attack".to_string(),
            // `TaskExplore::WriteStatusLog` (`taskexplore.cpp:81`).
            Self::Explore { .. } => "Explore map.".to_string(),
            // `TaskTransport::WriteStatusLog` (`tasktransport.cpp:364`).
            Self::Transport {
                unit_transporter, ..
            } => {
                if unit_transporter.is_some() {
                    "Transport units: working.".to_string()
                } else {
                    "Transport units: Waiting for a transporter.".to_string()
                }
            }
            Self::KillUnit(state) => format!(
                "Kill unit at [{},{}]",
                state.target_position.x + 1,
                state.target_position.y + 1
            ),
            Self::ManageBuildings { building_site, .. } => {
                format!(
                    "Manage buildings at [{},{}]",
                    building_site.x + 1,
                    building_site.y + 1
                )
            }
            _ => format!("{} ({})", name, task.base_priority),
        }
    }

    /// `Task::IsUnitTransferable` — base implementation for subclasses.
    fn _unused(&self) {}

    /// `TaskObtainUnits::CountInstancesOfUnitType`.
    pub fn count_instances_of_unit_type(&self, unit_type: UnitType) -> u32 {
        match self {
            Self::ObtainUnits { units, .. } => units
                .iter()
                .filter(|candidate| **candidate == unit_type)
                .count() as u32,
            _ => 0,
        }
    }

    /// `TaskCreate::GetUnitType` — the type being created (for the census).
    pub fn create_unit_type(&self) -> Option<UnitType> {
        match self {
            Self::CreateUnit { unit_type, .. } | Self::CreateBuilding { unit_type, .. } => {
                Some(*unit_type)
            }
            _ => None,
        }
    }

    /// `TaskCreate::IsActivelyBuilding`.
    pub fn is_actively_building(&self) -> bool {
        match self {
            Self::CreateUnit { op_state, .. } => *op_state >= create_unit_state::BUILDING,
            Self::CreateBuilding { op_state, .. } => *op_state >= create_building_state::BUILDING,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle: each module handles its variants and reports it
// ---------------------------------------------------------------------------

impl TaskKind {
    /// `Task::Init()` — `RemindTurnStart(true)` by default.
    pub fn init(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
        // The `TaskCreate*`/`TaskObtainUnits`/`TaskActivate` classes override
        // `Task::Init` and schedule the first reminder themselves.
        if create::init(self, ctx, header) {
            return;
        }

        // The rest inherit `Task::Init` → `RemindTurnStart(true)`.
        let _ = move_tasks::init(self, ctx, header);
        let _ = combat::init(self, ctx, header);
        let _ = build::init(self, ctx, header);
        let _ = misc::init(self, ctx, header);
        let _ = search::init(self, ctx, header);

        ctx.remind_turn_start(header.id, true);
    }

    /// `Task::BeginTurn()` — `EndTurn()` by default.
    pub fn begin_turn(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
        if create::begin_turn(self, ctx, header)
            || move_tasks::begin_turn(self, ctx, header)
            || combat::begin_turn(self, ctx, header)
            || build::begin_turn(self, ctx, header)
            || misc::begin_turn(self, ctx, header)
            || search::begin_turn(self, ctx, header)
        {
            return;
        }

        self.end_turn(ctx, header);
    }

    /// `Task::EndTurn()` — empty by default.
    pub fn end_turn(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
        if create::end_turn(self, ctx, header)
            || move_tasks::end_turn(self, ctx, header)
            || combat::end_turn(self, ctx, header)
            || build::end_turn(self, ctx, header)
            || misc::end_turn(self, ctx, header)
            || search::end_turn(self, ctx, header)
        {
            return;
        }

        let _ = (ctx, header);
    }

    /// `Task::Execute(unit)` — `false` by default.
    pub fn execute(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) -> bool {
        create::execute(self, ctx, header, unit)
            || move_tasks::execute(self, ctx, header, unit)
            || combat::execute(self, ctx, header, unit)
            || build::execute(self, ctx, header, unit)
            || misc::execute(self, ctx, header, unit)
            || search::execute(self, ctx, header, unit)
    }

    /// `Task::AddUnit(unit)` — empty by default.
    ///
    /// Each module handles its variants and silently skips others,
    /// so the calls go sequentially.
    pub fn add_unit(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
        create::add_unit(self, ctx, header, unit);
        move_tasks::add_unit(self, ctx, header, unit);
        combat::add_unit(self, ctx, header, unit);
        build::add_unit(self, ctx, header, unit);
        misc::add_unit(self, ctx, header, unit);
        search::add_unit(self, ctx, header, unit);
    }

    /// `Task::RemoveUnit(unit)` — empty by default.
    pub fn remove_unit(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
        create::remove_unit(self, ctx, header, unit);
        move_tasks::remove_unit(self, ctx, header, unit);
        combat::remove_unit(self, ctx, header, unit);
        build::remove_unit(self, ctx, header, unit);
        misc::remove_unit(self, ctx, header, unit);
        search::remove_unit(self, ctx, header, unit);
    }

    /// `Task::RemoveSelf()` — parent reset and deletion by default.
    pub fn remove_self(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
        if create::remove_self(self, ctx, header)
            || move_tasks::remove_self(self, ctx, header)
            || combat::remove_self(self, ctx, header)
            || build::remove_self(self, ctx, header)
            || misc::remove_self(self, ctx, header)
            || search::remove_self(self, ctx, header)
        {
            return;
        }

        ctx.set_parent(header.id, None);
        ctx.remove_self(header.id);
    }

    /// `Task::ChildComplete(task)`.
    pub fn child_complete(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader, child: u32) {
        if create::child_complete(self, ctx, header, child)
            || build::child_complete(self, ctx, header, child)
            || move_tasks::child_complete(self, ctx, header, child)
            || combat::child_complete(self, ctx, header, child)
            || misc::child_complete(self, ctx, header, child)
            || search::child_complete(self, ctx, header, child)
        {
            return;
        }

        let _ = (ctx, header, child);
    }

    /// Notification about a child `TaskMove` completion.
    pub fn move_finished(
        &mut self,
        ctx: &mut TaskCtx<'_>,
        header: TaskHeader,
        unit: UnitId,
        result: i8,
    ) {
        if create::move_finished(self, ctx, header, unit, result)
            || move_tasks::move_finished(self, ctx, header, unit, result)
            || combat::move_finished(self, ctx, header, unit, result)
            || build::move_finished(self, ctx, header, unit, result)
            || misc::move_finished(self, ctx, header, unit, result)
            || search::move_finished(self, ctx, header, unit, result)
        {
            return;
        }

        let _ = (ctx, header, unit, result);
    }

    /// `Task::CheckReactions()` — `false` by default.
    pub fn check_reactions(&mut self, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
        combat::check_reactions(self, ctx, header)
            || build::check_reactions(self, ctx, header)
            || misc::check_reactions(self, ctx, header)
            || move_tasks::check_reactions(self, ctx, header)
            || create::check_reactions(self, ctx, header)
            || search::check_reactions(self, ctx, header)
    }

    /// `Task::ExchangeOperator(unit)` — `false` by default.
    pub fn exchange_operator(
        &mut self,
        ctx: &mut TaskCtx<'_>,
        header: TaskHeader,
        unit: UnitId,
    ) -> bool {
        create::exchange_operator(self, ctx, header, unit)
            || build::exchange_operator(self, ctx, header, unit)
            || move_tasks::exchange_operator(self, ctx, header, unit)
            || combat::exchange_operator(self, ctx, header, unit)
            || misc::exchange_operator(self, ctx, header, unit)
            || search::exchange_operator(self, ctx, header, unit)
    }

    /// `Task::EventUnitDestroyed(unit)`.
    pub fn event_unit_destroyed(
        &mut self,
        ctx: &mut TaskCtx<'_>,
        header: TaskHeader,
        unit: UnitId,
    ) {
        if create::event_unit_destroyed(self, ctx, header, unit)
            || build::event_unit_destroyed(self, ctx, header, unit)
            || move_tasks::event_unit_destroyed(self, ctx, header, unit)
            || combat::event_unit_destroyed(self, ctx, header, unit)
            || misc::event_unit_destroyed(self, ctx, header, unit)
            || search::event_unit_destroyed(self, ctx, header, unit)
        {
            return;
        }

        let _ = (ctx, header, unit);
    }

    /// `Task::EventEnemyUnitSpotted(unit)`.
    pub fn event_enemy_unit_spotted(
        &mut self,
        ctx: &mut TaskCtx<'_>,
        header: TaskHeader,
        unit: UnitId,
    ) {
        if combat::event_enemy_unit_spotted(self, ctx, header, unit)
            || create::event_enemy_unit_spotted(self, ctx, header, unit)
            || build::event_enemy_unit_spotted(self, ctx, header, unit)
            || move_tasks::event_enemy_unit_spotted(self, ctx, header, unit)
            || misc::event_enemy_unit_spotted(self, ctx, header, unit)
            || search::event_enemy_unit_spotted(self, ctx, header, unit)
        {
            return;
        }

        let _ = (ctx, header, unit);
    }

    /// `Task::EventCargoTransfer(unit)`.
    pub fn event_cargo_transfer(
        &mut self,
        ctx: &mut TaskCtx<'_>,
        header: TaskHeader,
        unit: UnitId,
    ) {
        if misc::event_cargo_transfer(self, ctx, header, unit)
            || create::event_cargo_transfer(self, ctx, header, unit)
            || combat::event_cargo_transfer(self, ctx, header, unit)
            || build::event_cargo_transfer(self, ctx, header, unit)
            || move_tasks::event_cargo_transfer(self, ctx, header, unit)
            || search::event_cargo_transfer(self, ctx, header, unit)
        {
            return;
        }

        let _ = (ctx, header, unit);
    }

    /// `Task::EventUnitLoaded(unit1, unit2)`.
    pub fn event_unit_loaded(
        &mut self,
        ctx: &mut TaskCtx<'_>,
        header: TaskHeader,
        unit1: UnitId,
        unit2: UnitId,
    ) {
        if move_tasks::event_unit_loaded(self, ctx, header, unit1, unit2)
            || create::event_unit_loaded(self, ctx, header, unit1, unit2)
            || combat::event_unit_loaded(self, ctx, header, unit1, unit2)
            || build::event_unit_loaded(self, ctx, header, unit1, unit2)
            || misc::event_unit_loaded(self, ctx, header, unit1, unit2)
            || search::event_unit_loaded(self, ctx, header, unit1, unit2)
        {
            return;
        }

        let _ = (ctx, header, unit1, unit2);
    }

    /// `Task::EventUnitUnloaded(unit1, unit2)`.
    pub fn event_unit_unloaded(
        &mut self,
        ctx: &mut TaskCtx<'_>,
        header: TaskHeader,
        unit1: UnitId,
        unit2: UnitId,
    ) {
        if move_tasks::event_unit_unloaded(self, ctx, header, unit1, unit2)
            || create::event_unit_unloaded(self, ctx, header, unit1, unit2)
            || combat::event_unit_unloaded(self, ctx, header, unit1, unit2)
            || build::event_unit_unloaded(self, ctx, header, unit1, unit2)
            || misc::event_unit_unloaded(self, ctx, header, unit1, unit2)
            || search::event_unit_unloaded(self, ctx, header, unit1, unit2)
        {
            return;
        }

        let _ = (ctx, header, unit1, unit2);
    }
}
