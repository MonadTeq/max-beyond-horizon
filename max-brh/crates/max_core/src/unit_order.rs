//! Order machine: `UnitOrder` (32) × `UnitOrderState` (47).
//!
//! Oracle: `max-cpp-port/src/units_manager.cpp` (`UnitsManager_SetNewOrderInt`,
//! `UnitsManager_SetNewOrder`, `UnitsManager_ProcessOrder`,
//! `UnitsManager_ProcessUnitOrders`, 32 `UnitsManager_ProcessOrder*` functions,
//! `UnitsManager_EndAttack`, `UnitsManager_PinAttackTargets`,
//! `UnitsManager_ScaleUnit`, `UnitsManager_NewOrderWhileScaling`,
//! `UnitsManager_FinishUnitScaling`), `unitinfo.cpp` (`UnitInfo::RestoreOrders`,
//! `ExecuteMove`, `Move`, `MoveFinished`, `BlockedOnPathRequest`, `StopMove`,
//! `StopMovement`, `Land`, `Take`, `ProcessLanding`, `ProcessLoading`,
//! `ProcessUnloading`, `PositionInTape`, `UpdateAngle`, `StartBuilding`,
//! `CancelBuilding`,
//! `PowerUp`, `PowerDown`, `Reload`, `Repair`, `Transfer`, `Upgrade`,
//! `CheckIfDestroyed`), `groundpath.cpp` (`GroundPath::Execute`,
//! `GroundPath::UpdateUnitAngle`, `CancelMovement`), `paths.cpp`
//! (`Paths_GetAngle`, `Paths_UpdateAngle`,
//! `Paths_CalculateStep`), DESIGN.md §5.2.
//!
//! There is no separate `unitorder.cpp` file in the C++ port: the order machine lives in
//! `units_manager.cpp` (dispatcher and processors) and `unitinfo.cpp` (the
//! `UnitInfo` methods implementing the transitions). The port repeats this split: processors
//! and the dispatcher — here, operations on the unit — [`UnitInfo`] methods,
//! list traversal and inter-unit effects — [`crate::game::GameState`].
//!
//! # Model
//!
//! An order is processed over a **working copy** of the unit ([`OrderContext`]):
//! `UnitInfo` is cloned, the machine changes the copy, and at the end the copy is written back.
//! In C++ all changes go through pointers; the copy is equivalent because
//! `ProcessOrders` visits each list exactly once and does not re-execute
//! the same unit in the same pass. Inter-unit effects (parent,
//! target, live attack, lists) are performed by explicit `GameState` methods, so
//! they are visible to other units immediately — as in C++.
//!
//! # P5/P6 integration
//!
//! Combat (`crate::combat`), passability/occupancy (`crate::access`) and paths
//! (`crate::pathfinding`) are connected to the order machine: `ProcessOrderFire`
//! uses `AimAtTarget`/`PrepareFire`/`ProgressFire`/`Attack`,
//! `InitUnitPath` puts requests into the `PathsManager` queue, `PursueEnemy` and
//! `MoveToAttack` search for a path to the attack position, `IsEngaged`/`CanReceiveUnits`/
//! `SetUnitDestination` come from `access`. P6 AI hooks are connected:
//! `MoveFinished` calls `Ai_EvaluateAttackTargets` and `Ai_CheckMines`,
//! `PerformAutoSurvey` — `Ai_EnableAutoSurvey`, `ScheduleDelayedTasks` —
//! `TaskManager::ScheduleDelayedTasks`. The remaining `TODO(P3)` are economy and
//! complexes; `TODO(P5)` at points that require `UnitsManager_DeployUnit`
//! of concrete types (master builder, activation).
//!
//! # Divergences from C++
//!
//! 1. **Movement** (DESIGN §5.1, divergence 3): C++ moves the unit pixel by pixel
//!    (`UnitInfo::Move`: `velocity`/`moved`/`GFX_MAP_TILE_SIZE`) and in a network
//!    game completes at most one cell per tick. `max_core` is lockstep, and
//!    [`unit_move`] completes exactly one cell per tick: the path step is considered
//!    traversed, the offset is subtracted from the current `PathStep` (as in C++
//!    `Move()`: `path_steps[step_index]->x -= offset_x`). The pixel animation —
//!    P5.
//! 2. `Paths_CalculateStep` is ported in full (`speed` spend, `move_fraction`,
//!    `shots`, `max_velocity`), so the per-turn movement range matches C++
//!    with the same attributes.
//! 3. `Animate`/`RefreshScreen`/`AddToDrawList` rendering — UI, not ported.
//! 4. The `Effects` setting (construction animation, `UnitInfo::Animate`) is
//!    accounted for in the simulation only where it changes state fields
//!    (`moved` in `ORDER_BUILD`); see [`crate::game::GameState::settings_effects`].

use crate::access;
use crate::combat;
use crate::enums::{unit_flags::*, UnitOrder, UnitOrderState, UnitType};
use crate::game::{GameState, OrderData};
use crate::geo::{PathStep, Point, DIRECTION_OFFSETS};
use crate::pathfinding;
use crate::production;
use crate::unit::{AirPath, GroundPath, UnitId, UnitInfo, UnitListKind, UnitPath};
use crate::unit_values::{ATTRIB_AMMO, ATTRIB_MOVE_AND_FIRE, ATTRIB_ROUNDS, ATTRIB_SPEED};

/// `UnitInfo::MOVEMENT_WAIT_LIMIT` (`unitinfo.hpp:400`).
pub const MOVEMENT_WAIT_LIMIT: u16 = 512;

/// `GFX_MAP_TILE_SIZE` — the cell side in pixels (`unitinfo.cpp`).
pub const GFX_MAP_TILE_SIZE: i32 = 64;

// ---------------------------------------------------------------------------
// Bridges in AI (P6)
// ---------------------------------------------------------------------------

/// `UnitInfo::Animate` / `RefreshScreen` — UI animation and dirty regions (not needed by P4).
fn animate(_unit: &UnitInfo) {}

/// `UnitInfo::RefreshScreen` — UI.
fn refresh_screen(_unit: &UnitInfo) {}

/// `Ai_SetTasksPendingFlag(name)` (`ai.cpp:140`).
///
/// C++ sets the "the computer team has work" flag on every tick
/// in which a unit actually does something (`UnitsManager_ProcessOrder*` and
/// `UnitInfo::ExecuteMove`/`PositionInTape`); `Ai_ClearTasksPendingFlags`
/// resets it at the start of `UnitsManager_ProcessOrders`. It is this flag, not
/// "are there unfinished tasks", that decides whether the AI can end the turn
/// (`AiPlayer::CheckEndTurn`): a unit that ran out of speed does not set the flag,
/// and the turn closes.
fn ai_task_flag(state: &mut GameState, _name: &str) {
    crate::ai::ai_set_tasks_pending_flag(state);
}

/// `UnitInfo::ScheduleDelayedTasks` — P6: remind the unit's top task
/// about movement completion (C++ `Task_RemindMoveFinished`).
fn schedule_delayed_tasks(state: &mut GameState, id: UnitId, priority: bool) {
    state.with_task_manager(|task_manager, world| {
        task_manager.schedule_delayed_tasks(world, id, priority);
    });
}

// ---------------------------------------------------------------------------
// Pure operations on UnitInfo (port of UnitInfo methods)
// ---------------------------------------------------------------------------

/// `UnitInfo::IsBridgeElevated` (`unitinfo.cpp:3030`).
pub fn is_bridge_elevated(unit: &UnitInfo) -> bool {
    unit.unit_type == UnitType::Bridge && unit.image_index != unit.image_base
}

/// `UnitInfo::RestoreOrders` (`unitinfo.cpp:3783`).
pub fn restore_orders(unit: &mut UnitInfo) {
    if unit.prior_orders == unit.orders
        && unit.unit_type != UnitType::GoldRefinery
        && unit.unit_type != UnitType::MiningStation
        && unit.unit_type != UnitType::Habitat
        && unit.unit_type != UnitType::PowerGenerator
        && unit.unit_type != UnitType::PowerStation
        && unit.unit_type != UnitType::ResearchCentre
    {
        unit.prior_orders = UnitOrder::Await;
        unit.prior_state = UnitOrderState::ExecutingOrder;
    }

    unit.orders = unit.prior_orders;
    unit.state = unit.prior_state;
}

/// `UnitsManager_SetNewOrderInt` (`units_manager.cpp:1945`).
///
/// Returns `true` if the caller must remove the path request
/// (`ResourceManager_GetPathsManager().RemoveRequest(unit)`).
pub fn set_new_order_int(unit: &mut UnitInfo, order: UnitOrder, state: UnitOrderState) -> bool {
    if unit.orders == UnitOrder::Explode || unit.state == UnitOrderState::Destroy {
        return false;
    }

    let mut delete_path = false;

    if unit.orders == UnitOrder::AwaitScaling {
        new_order_while_scaling(unit);
    }

    if unit.state == UnitOrderState::NewOrder {
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;

        delete_path = true;
    }

    if order == UnitOrder::Explode || unit.orders != UnitOrder::Fire {
        if unit.orders != UnitOrder::NewAllocate {
            unit.prior_orders = unit.orders;
            unit.prior_state = unit.state;
        }

        unit.orders = order;
        unit.state = state;
    }

    delete_path
}

/// `UnitsManager_ScaleUnit` (`units_manager.cpp:1847`) — start the building
/// scaling animation; returns `delete_path` from `SetNewOrderInt`.
pub fn scale_unit(unit: &mut UnitInfo, state: UnitOrderState) -> bool {
    set_new_order_int(unit, UnitOrder::AwaitScaling, state)
}

/// `UnitsManager_NewOrderWhileScaling` (`units_manager.cpp:2529`).
pub fn new_order_while_scaling(unit: &mut UnitInfo) {
    if unit.orders != UnitOrder::AwaitScaling {
        return;
    }
    if unit.state == UnitOrderState::Expand {
        unit.scaler_adjust = 0;
        // `UnitsManager_UpdateConnectors(unit)` requires `GameState`; the callers
        // of its branches (order/packet 08) do it themselves — see
        // [`crate::complex::update_connectors`]. A pure function cannot.
    } else {
        unit.scaler_adjust = 4;
        finish_unit_scaling(unit);
    }
}

/// `UnitsManager_FinishUnitScaling` (`units_manager.cpp:2471`) — version without
/// game state (for the `NewOrderWhileScaling` packet path); "container does not
/// accept" is the safe variant, otherwise the unit would stay in `ORDER_IDLE`.
pub fn finish_unit_scaling(unit: &mut UnitInfo) {
    restore_orders(unit);

    if unit.orders == UnitOrder::Idle {
        unit.parent_unit = None;

        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;

        if unit.is_air() {
            set_new_order_int(unit, UnitOrder::TakeOff, UnitOrderState::Init);
        }

        scale_unit(unit, UnitOrderState::Expand);
    }
}

/// `UnitsManager_FinishUnitScaling` with game access: if the container can
/// accept the unit (`Access_CanReceiveUnits`, `storage < ATTRIB_STORAGE`, same
/// team), the unit goes into the hold (`ORDER_IDLE` + `Hash_MapHash.Remove`).
pub(crate) fn finish_unit_scaling_in(state: &mut GameState, unit: &mut UnitInfo) {
    restore_orders(unit);

    if unit.orders != UnitOrder::Idle {
        return;
    }

    let parent_id = unit.parent_unit;
    let mut stored = false;

    if let Some(parent_id) = parent_id {
        if let Some(mut parent) = state.unit(parent_id).cloned() {
            let storage_limit = parent
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_STORAGE);
            if access::can_receive_units(&parent)
                && i32::from(parent.storage) < storage_limit
                && parent.team == unit.team
            {
                unit.orders = UnitOrder::Await;
                state.map_hash_remove(unit.id, unit.grid, unit.flags);
                unit.orders = UnitOrder::Idle;
                parent.storage = parent.storage.wrapping_add(1);
                state.write_unit(parent_id, parent);
                stored = true;
            }
        }
    }

    if stored {
        return;
    }

    let parent_alive = parent_id
        .and_then(|parent_id| state.unit(parent_id))
        .map(access::can_receive_units)
        .unwrap_or(false);

    if !parent_alive {
        unit.parent_unit = None;
    }

    unit.orders = UnitOrder::Await;
    unit.state = UnitOrderState::ExecutingOrder;

    if unit.is_air() {
        set_new_order_int(unit, UnitOrder::TakeOff, UnitOrderState::Init);
    }

    scale_unit(unit, UnitOrderState::Expand);
}

/// `UnitInfo::BlockedOnPathRequest` (`unitinfo.cpp:3628`).
///
/// `disable_fire` — the setting that disables reaction fire (DESIGN §6.5:
/// game settings are input facts; in max_core — a `GameState` field).
pub fn blocked_on_path_request(unit: &mut UnitInfo, mode: bool, disable_fire: bool) {
    unit.path = None;

    if unit.orders != UnitOrder::MoveToAttack || unit.state == UnitOrderState::NewOrder {
        unit.orders = UnitOrder::Await;
    }

    unit.state = UnitOrderState::ExecutingOrder;

    if !disable_fire {
        unit.delayed_reaction = 1;
    }

    move_finished(unit, mode, disable_fire);
}

/// `UnitInfo::MoveFinished` (`unitinfo.cpp:3654`).
///
/// Setting `delayed_reaction` and adding to the delayed reaction list
/// are done by the caller layer ([`OrderContext::move_finished`]) because
/// they need game state access. Here — reset of the movement counters,
/// which are entirely local to the unit.
///
/// `Ai_CheckMines` also requires the game, so it is called from
/// [`OrderContext::move_finished`].
pub fn move_finished(unit: &mut UnitInfo, mode: bool, disable_fire: bool) {
    unit.velocity = 0;
    unit.moved = 0;

    clear_movement_wait(unit);

    if unit.unit_type == UnitType::Infantry || unit.unit_type == UnitType::Infiltrator {
        // `UpdateSpriteFrame(0, image_index_max)` — UI.
    }

    unit.in_transit = false;

    if mode && !disable_fire {
        unit.delayed_reaction = 1;
    }
}

/// `UnitInfo::StopMove` (`unitinfo.cpp:3703`).
pub fn stop_move(unit: &mut UnitInfo, disable_fire: bool) {
    if unit.has_move_order() {
        move_finished(unit, false, disable_fire);
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::ExecutingOrder;
    }
}

/// `UnitInfo::StopMovement` (`unitinfo.cpp:3222`) — emergency stop.
pub fn stop_movement(unit: &mut UnitInfo, disable_fire: bool) {
    if unit.orders == UnitOrder::Move && unit.path.is_some() {
        if unit.state == UnitOrderState::InProgress || unit.state == UnitOrderState::InTransition {
            cancel_movement(unit, disable_fire);
        }
    }
}

/// `GroundPath::CancelMovement` (`groundpath.cpp:161`).
fn cancel_movement(unit: &mut UnitInfo, disable_fire: bool) {
    if unit.state == UnitOrderState::InProgress {
        blocked_on_path_request(unit, true, disable_fire);
    } else if let Some(UnitPath::Ground(path)) = unit.path.as_mut() {
        path.step_index = 0;
        path.steps.clear();
        path.steps.push(PathStep::new(0, 0));
    }
}

/// `UnitInfo::RegisterMovementWait` (`unitinfo.cpp:3618`).
pub fn register_movement_wait(unit: &mut UnitInfo) -> bool {
    if unit.movement_wait_count < MOVEMENT_WAIT_LIMIT {
        unit.movement_wait_count = unit.movement_wait_count.wrapping_add(1);
    }
    unit.movement_wait_count >= MOVEMENT_WAIT_LIMIT
}

/// `UnitInfo::ClearMovementWait` (`unitinfo.cpp:3626`).
pub fn clear_movement_wait(unit: &mut UnitInfo) {
    unit.movement_wait_count = 0;
}

/// `UnitInfo::CheckIfDestroyed` (`unitinfo.cpp:4988`).
pub fn check_if_destroyed(unit: &mut UnitInfo) {
    if unit.hits == 0 {
        unit.engine = 0;
        unit.weapon = 0;
    }
}

/// `UnitInfo::Land` (`unitinfo.cpp:3829`) — landing animation, `moved` grows
/// by 1 per call; `true` on the 9th call.
pub fn land(unit: &mut UnitInfo) -> bool {
    refresh_screen(unit);
    unit.moved = unit.moved.wrapping_add(1) & 0x3F;

    if unit.moved > 8 {
        unit.flags &= !HOVERING;
        true
    } else {
        false
    }
}

/// `UnitInfo::Take` (`unitinfo.cpp:3862`) — takeoff animation, `true` on the 5th
/// call.
pub fn take(unit: &mut UnitInfo) -> bool {
    refresh_screen(unit);
    unit.moved = unit.moved.wrapping_add(1) & 0x3F;

    if unit.moved > 4 {
        unit.flags |= HOVERING;
        true
    } else {
        false
    }
}

/// `UnitInfo::PowerUp` (`unitinfo.cpp:5094`).
///
/// TODO(P3): `Cargo_UpdateResourceLevels` (complex mining/power levels).
pub fn power_up(unit: &mut UnitInfo, _factor: i32) {
    unit.state = UnitOrderState::ExecutingOrder;
}

/// `UnitInfo::PowerDown` (`unitinfo.cpp:5123`).
pub(crate) fn power_down(state: &mut GameState, unit: &mut UnitInfo) {
    ai_task_flag(state, "Power Down");
    power_up(unit, 1);

    if unit.unit_type == UnitType::ResearchCentre {
        // TODO(risk 1): ResearchMenu_UpdateResearchProgress(team, topic, -1).
    }
}

/// `UnitInfo::StartBuilding` (`unitinfo.cpp:2751`).
///
/// For a mobile builder it places the construction site — [`crate::production::
/// deploy_construction_site`] (`DeployConstructionSiteUtilities` +
/// `PrepareConstructionSite`): tape/cones/slab on the `target_grid` cell.
/// The function is idempotent: `ProductionManager::commit` (the `Done` click in
/// `BuildMenu`) and `events::apply_command` (packet 08/P4 after a map click)
/// already place the site synchronously, so here on the next tick there is no
/// repeated spawn — only `ORDER_STATE_BUILD_IN_PROGRESS` and `PositionInTape`.
pub(crate) fn start_building(state: &mut GameState, unit: &mut UnitInfo) {
    // `SDL_assert(build_list.GetCount() > 0)`: the first type in the queue.
    let Some(unit_to_build) = unit.build_list.first().copied() else {
        return;
    };

    if !unit.is_mobile() {
        // TODO(P3): charging the complex's materials/power/workers
        // (`Cargo_GetRawConsumptionRate`, `Cargo_GetPowerConsumptionRate`,
        // `Cargo_GetLifeConsumptionRate`).
        unit.state = UnitOrderState::ExecutingOrder;
    } else {
        // `DeployConstructionSiteUtilities(unit_to_build)` +
        // `PrepareConstructionSite`: the site on the `move_to`/`target_grid` cell.
        let _ = production::deploy_construction_site(
            state,
            unit.id,
            unit_to_build,
            unit.target_grid,
        );

        unit.moved = 0;
        unit.state = UnitOrderState::BuildInProgress;

        if unit.unit_type == UnitType::Constructor {
            set_new_order_int(
                unit,
                UnitOrder::AwaitTapePositioning,
                UnitOrderState::TapePositioningInit,
            );
        } else {
            // `DrawSpriteFrame(image_index + 16)` — the engineer's working pose;
            // the frame is chosen by `max_app::render::unit_frame_index` from
            // `ORDER_BUILD`/`BUILD_IN_PROGRESS`.
        }

        // `ClearUnitList()` — removing the group order (P5 access).
    }
}

/// `UnitInfo::CancelBuilding` (`unitinfo.cpp:4231`).
pub fn cancel_building(unit: &mut UnitInfo) {
    if !unit.is_mobile() {
        if unit.orders == UnitOrder::Build {
            if unit.state == UnitOrderState::BuildCancel {
                unit.orders = UnitOrder::HaltBuilding;
            } else {
                unit.orders = UnitOrder::HaltBuilding2;
            }
            unit.state = UnitOrderState::ExecutingOrder;
            // TODO(P3): Cargo_UpdateResourceLevels(this, 1).
        } else {
            unit.orders = UnitOrder::Await;
            unit.state = UnitOrderState::ExecutingOrder;
            unit.build_list.clear();
            unit.build_time = 0;
        }
    } else {
        unit.build_list.clear();
        unit.build_time = 0;
        // `Access_DestroyUtilities` requires `GameState`; it is called by
        // the order processor (`cancel_building` is a pure operation).
        unit.path = None;
        unit.moved = 0;
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
        // TODO(P3/P5): returning the constructor/bulldozer to complex logic.
    }
}

/// Active construction of a mobile builder: the site cell (`target_grid`).
///
/// C++ `ORDER_BUILD` of a mobile builder passes states
/// `INIT → IN_PROGRESS → BUILD_IN_PROGRESS`, and the constructor also
/// `ORDER_AWAIT_TAPE_POSITIONING` with `TAPE_POSITIONING_*` states. The site by
/// that moment already stands (`UnitInfo::StartBuilding`), progress is in
/// `build_list`/`build_time`. `SELECT_SITE`/`UNIT_READY` are not included:
/// in the first there is no site yet (the `GameManager_TempTape` ghost), in the second the building
/// is already built and the builder waits for an order to leave.
///
/// Returns `None` if the unit is not a mobile builder or construction is not running.
pub fn active_build_site(unit: &UnitInfo) -> Option<Point> {
    if !production::is_mobile_builder(unit.unit_type) {
        return None;
    }

    if !matches!(
        unit.orders,
        UnitOrder::Build | UnitOrder::AwaitTapePositioning
    ) {
        return None;
    }

    if matches!(
        unit.state,
        UnitOrderState::SelectSite
            | UnitOrderState::UnitReady
            | UnitOrderState::BuildCancel
            | UnitOrderState::BuildAbort
    ) {
        return None;
    }

    // Construction progress lives in the queue: `SpawnNewUnit` clears it on
    // completion, `CancelBuilding` — on cancellation. An empty queue means that
    // `AWAIT_TAPE_POSITIONING` is already the builder **leaving** a completed site
    // (`TAPE_POSITIONING_DEINIT`, `ActivateEngineer`), not construction: utilities and the slab
    // under the completed building must not be removed.
    if unit.build_list.is_empty() {
        return None;
    }

    Some(unit.target_grid)
}

// Moving a mobile builder away from an unfinished site is **impossible** without an explicit
// cancellation — this is an excluded path from C++.
//
// In C++ a movement order to the builder during construction is not issued via the map
// (`GameManager_GetBuilderUnitCursor`, `game_manager.cpp:5927-5931`: for
// `BUILD_IN_PROGRESS` the cursor is `CURSOR_UNIT_NO_GO`), and cancellation goes only through
// the popup `stop` (`UnitsManager_Popup_OnClick_BuildStop` →
// `GameManager_SelectBuildSite` → `ORDER_BUILD`/`BUILD_CANCEL` →
// `UnitInfo::CancelBuilding`). The former port behavior "auto-cancel on move-away"
// contradicted the oracle and was removed: a new order (packet 08/38, AI, group)
// does not cancel construction — the order just changes `orders`, and the site/queue
// remain until an explicit `stop`.
//
// A completed site (`ORDER_STATE_UNIT_READY`) is the exception: there the exit by order
// is allowed and handled by `ORDER_ACTIVATE` (see
// [`crate::production::builder_awaiting_exit`]), because the building is already
// completed.

/// `UnitInfo::ClearBuildListAndPath` (`unitinfo.cpp:1681`).
pub fn clear_build_list_and_path(unit: &mut UnitInfo) {
    unit.path = None;

    if unit.parent_unit.is_some() {
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
    } else {
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    unit.build_list.clear();
}

/// `UnitInfo::GetTargetUnitAngle` (`unitinfo.cpp:4147`) — the direction in
/// which the builder enters the tape center and leaves it.
///
/// * `TAPE_POSITIONING_ENTER` — by cells: the tape is on the builder's cell
///   (`Access_GetConstructionUtility(team, grid_x, grid_y)`), its `grid` is the 2×2
///   anchor. Comparing `grid_x/grid_y` with the anchor yields exactly one of the four
///   diagonal corners: both axes match — `SE`, only `x` — `NE`, only
///   `y` — `SW`, otherwise `NW`. A 2×2 builder always stands in one of the four
///   tape cells, so normally the angle is always diagonal (45°).
/// * the other states (`LEAVE`) — toward `move_to` (C++
///   `move_to_grid_x/move_to_grid_y`, in the port — `OrderData::move_to`): the sign
///   of the difference on each axis independently; the builder's pixel position — the center
///   of its cell (`UnitInfo::SetPosition`; in the port `x/y` are not recomputed when
///   walking, so the cell is used).
pub(crate) fn get_target_unit_angle(state: &GameState, unit: &UnitInfo, move_to: Point) -> i32 {
    const UNIT_ANGLE_NE: i32 = 1;
    const UNIT_ANGLE_SE: i32 = 3;
    const UNIT_ANGLE_SW: i32 = 5;
    const UNIT_ANGLE_NW: i32 = 7;

    if unit.state == UnitOrderState::TapePositioningEnter {
        // `Access_GetConstructionUtility(team, grid_x, grid_y)`: for a building
        // the tape is registered in the hash in all four 2×2 cells, but its
        // `grid` is the anchor (top-left corner).
        let tape_grid = production::construction_utility(state, unit.team, unit.grid)
            .and_then(|id| state.unit(id))
            .map(|tape| tape.grid)
            .unwrap_or(unit.target_grid);

        if unit.grid.x == tape_grid.x {
            if unit.grid.y == tape_grid.y {
                UNIT_ANGLE_SE
            } else {
                UNIT_ANGLE_NE
            }
        } else if unit.grid.y == tape_grid.y {
            UNIT_ANGLE_SW
        } else {
            UNIT_ANGLE_NW
        }
    } else {
        let target_x = i32::from(move_to.x) * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2;
        let target_y = i32::from(move_to.y) * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2;
        let x = i32::from(unit.grid.x) * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2;
        let y = i32::from(unit.grid.y) * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2;

        if target_x > x {
            if target_y > y {
                UNIT_ANGLE_SE
            } else {
                UNIT_ANGLE_NE
            }
        } else if target_y > y {
            UNIT_ANGLE_SW
        } else {
            UNIT_ANGLE_NW
        }
    }
}

/// The sign of the builder's "entry" into the tape on each axis — how many pixels it has
/// already traveled from the center of its cell to the site center
/// (`OffsetDrawZones(DIRECTION_OFFSETS[angle] * 8)`, `TAPE_STEPS` ticks).
///
/// For a 2×2 tape the builder stands in one of its four cells, so entry is
/// always diagonal: `−1` on the side where the anchor matched its cell.
/// For a 1×1 tape (engineer) the builder is exactly on its cell — there is no entry.
fn tape_enter_direction(state: &GameState, unit: &UnitInfo) -> (i32, i32) {
    let Some(tape) = production::construction_utility(state, unit.team, unit.grid)
        .and_then(|id| state.unit(id))
    else {
        return (0, 0);
    };

    if tape.unit_type != UnitType::LargeTape || !production::site_covers(tape.unit_type, tape.grid, unit.grid)
    {
        return (0, 0);
    }

    (
        if unit.grid.x == tape.grid.x { 1 } else { -1 },
        if unit.grid.y == tape.grid.y { 1 } else { -1 },
    )
}

/// Where the builder's cell shifts after four steps of 8 px
/// (`PositionInTape`: `grid_x = x / GFX_MAP_TILE_SIZE`).
///
/// The builder's pixel is the center of its cell plus `32 * enter` (entry into the tape) plus
/// `32 * leave` (exit). `x / 64` of that sum gives the cell shift:
/// `enter + leave >= 1` → `+1`, `<= -2` → `-1`, otherwise the cell does not change.
fn tape_leave_shift(enter: i32, leave: i32) -> i16 {
    match enter + leave {
        sum if sum >= 1 => 1,
        sum if sum <= -2 => -1,
        _ => 0,
    }
}


/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4414`).
///
/// The positioning loop is ported: `INIT → ENTER`, `DEINIT → LEAVE`,
/// final rotation `Paths_UpdateAngle` toward [`get_target_unit_angle`], `moved` up to 4, then
/// `RestoreOrders`. As in C++, while the builder has not turned to the center, `moved`
/// **does not grow** — first rotation in place, then sliding (four steps
/// `OffsetDrawZones` of 8 px).
///
/// `move_to` — the exit target (C++ `move_to_grid_x/move_to_grid_y`): on
/// LEAVE completion C++ recomputes the cell from the pixel position
/// (`grid_x = x / GFX_MAP_TILE_SIZE`) — the builder ends up in the tape cell
/// closest to the target, and then walks (`ActivateEngineer` → `ORDER_MOVE`).
pub(crate) fn position_in_tape(state: &mut GameState, unit: &mut UnitInfo, move_to: Point) {
    ai_task_flag(state, "Positioning in tape");

    if unit.state == UnitOrderState::TapePositioningInit {
        unit.moved = 0;
        unit.state = UnitOrderState::TapePositioningEnter;
    } else if unit.state == UnitOrderState::TapePositioningDeinit {
        unit.moved = 0;
        unit.state = UnitOrderState::TapePositioningLeave;
    }

    refresh_screen(unit);

    if unit.moved >= 4 {
        // TODO(P3/P5): checks of `UnitsManager_IsEngaged` and site occupancy
        // (`UnitInfo_IsSiteEngaged` requires `Access_GetConstructionUtility`).
        let old_state = unit.state;
        restore_orders(unit);
        unit.moved = 0;

        if unit.unit_type == UnitType::MasterBuilder {
            // `PrepareConstructionSite` + `ScaleUnit(this, SHRINK)` — P3/P5.
            let _ = scale_unit(unit, UnitOrderState::Shrink);
        } else if unit.unit_type == UnitType::Constructor
            && old_state == UnitOrderState::TapePositioningEnter
        {
            // `PrepareConstructionSite` has already been done by `StartBuilding`
            // (the site is placed immediately on order, see
            // `production::deploy_construction_site`). C++ turns the builder once more by 45°
            // (`Paths_UpdateAngle(this, (angle + 1) & 0x07)`) and
            // draws the working pose `DrawSpriteFrame(image_index + 16)`; the frame
            // is chosen by the renderer from `ORDER_BUILD`/`BUILD_IN_PROGRESS`.
            let target_angle = (i32::from(unit.angle) + 1) & 0x07;
            let _ = paths_update_angle(unit, target_angle);
        } else if old_state != UnitOrderState::TapePositioningEnter {
            // `Hash_MapHash.Remove(this)` → `grid_x = x / GFX_MAP_TILE_SIZE` →
            // `Hash_MapHash.Add(this)`: the cell is recomputed from the pixel
            // position after four 8 px shifts.
            let angle = get_target_unit_angle(state, unit, move_to);
            let leave = DIRECTION_OFFSETS[usize::from((angle & 0x07) as u8)];
            let (enter_x, enter_y) = tape_enter_direction(state, unit);

            unit.grid = Point::new(
                unit.grid
                    .x
                    .wrapping_add(tape_leave_shift(enter_x, i32::from(leave.x))),
                unit.grid
                    .y
                    .wrapping_add(tape_leave_shift(enter_y, i32::from(leave.y))),
            );
            refresh_screen(unit);

            // back in the middle of its site after a failed way off, the builder
            // is ready to leave again (`ScheduleDelayedTasks(true)`).
            if unit.orders == UnitOrder::Build {
                schedule_delayed_tasks(state, unit.id, true);
            }
        }
    } else {
        // C++: `if (moved || !Paths_UpdateAngle(this, target_angle))`.
        // While `moved == 0`, each tick is one rotation step; the shift and counter
        // start only after the angle matches the target.
        let step = if unit.moved != 0 {
            true
        } else {
            let target_angle = get_target_unit_angle(state, unit, move_to);
            !paths_update_angle(unit, target_angle)
        };

        if step {
            unit.moved = unit.moved.wrapping_add(1);
            refresh_screen(unit);
        }
    }
}

// ---------------------------------------------------------------------------
// Paths: `Paths_GetAngle`, `Paths_UpdateAngle`, `Paths_CalculateStep`
// ---------------------------------------------------------------------------

/// `Paths_GetAngle` (`paths.cpp`).
pub fn paths_get_angle(x: i32, y: i32) -> i32 {
    if x != 0 {
        if x <= 0 {
            if y <= 0 {
                if y != 0 {
                    7
                } else {
                    6
                }
            } else {
                5
            }
        } else if y >= 0 {
            if y != 0 {
                3
            } else {
                2
            }
        } else {
            1
        }
    } else if y >= 0 {
        4
    } else {
        0
    }
}

/// `Paths_UpdateAngle` (`paths.cpp:258`): one rotation step toward `angle`;
/// `true` if the unit is still turning (movement does not start this tick).
///
/// Copies `UnitInfo::UpdateAngle` (`unitinfo.cpp:1161`) in the part affecting
/// the simulation: for a unit with `TURRET_SPRITE` the turret rotates with the hull
/// (`turret_angle = (turret_angle + image_diff) & 7`, where `image_diff` is the same
/// 45° step). The frame cache (`image_index`/`turret_image_index`/draw zones) — UI,
/// not stored in max_core.
pub fn paths_update_angle(unit: &mut UnitInfo, angle: i32) -> bool {
    if i32::from(unit.angle) == angle {
        return false;
    }

    let mut delta_a = angle - i32::from(unit.angle);
    let mut delta_b = i32::from(unit.angle) - angle;

    if delta_a < 0 {
        delta_a += 8;
    }
    if delta_b < 0 {
        delta_b += 8;
    }

    let direction = if delta_a >= delta_b { -1 } else { 1 };

    unit.angle = ((i32::from(unit.angle) + direction) & 7) as u8;

    if unit.flags & TURRET_SPRITE != 0 {
        unit.turret_angle = ((i32::from(unit.turret_angle) + direction) & 7) as u8;
    }

    true
}

/// `GroundPath::UpdateUnitAngle` (`groundpath.cpp:430`).
///
/// C++ calls this from `UnitInfo::Move` when a cell completes
/// (`moved == GFX_MAP_TILE_SIZE`): the hull makes **one** 45° step toward
/// the next route step (if any), rather than turning instantly.
/// The next `GroundPath::Execute` finishes turning the unit and only then
/// starts movement — so rotation and movement are always consistent.
///
/// The function is extracted for tests: in [`unit_move`] the same step is performed on
/// a `PathStep` snapshot, to avoid holding a `unit.path` borrow while
/// changing `unit.angle`.
pub fn ground_path_update_unit_angle(unit: &mut UnitInfo, path: &GroundPath) -> bool {
    let next = path.step_index as usize + 1;

    if let Some(step) = path.steps.get(next) {
        paths_update_angle(unit, paths_get_angle(i32::from(step.x), i32::from(step.y)))
    } else {
        false
    }
}

/// `Paths_CalculateStep` (`paths.cpp`) — charging the step cost from `speed`.
///
/// Returns `false` if the unit lacks speed (the step does not start).
/// `real_time` — the `GameManager_RealTime` setting: the step does not spend `speed`.
pub fn paths_calculate_step(
    unit: &mut UnitInfo,
    cost: i32,
    is_diagonal_step: bool,
    fast_movement: bool,
    real_time: bool,
) -> bool {
    let mut max_velocity = if fast_movement { 16 } else { 8 };

    if unit.unit_type == UnitType::Infiltrator || unit.unit_type == UnitType::Infantry {
        max_velocity >>= 1;
        unit.velocity = max_velocity as u8;
    }

    if cost > 4 {
        max_velocity /= 2;
    } else if cost < 4 {
        max_velocity *= 2;
    }
    unit.max_velocity = max_velocity as u8;

    let mut cost = cost;
    if is_diagonal_step {
        cost = (cost * 3) / 2;
    }

    if i32::from(unit.move_fraction) > cost {
        unit.move_fraction = cost as u8;
    }

    cost -= i32::from(unit.move_fraction);

    let mut normalized_cost = (cost + 3) / 4;

    if normalized_cost > i32::from(unit.speed) {
        unit.move_fraction = unit
            .move_fraction
            .wrapping_add((i32::from(unit.speed) * 4) as u8);
        unit.speed = 0;

        if unit.group_speed != 0 {
            unit.group_speed = 1;
        }

        false
    } else {
        let base = unit.base_values;

        if real_time {
            normalized_cost = 0;
        }

        unit.speed = i32::from(unit.speed).wrapping_sub(normalized_cost) as u16;

        if unit.group_speed != 0 {
            let group_speed = i32::from(unit.group_speed) - normalized_cost;
            unit.group_speed = group_speed.max(1) as u16;
        }

        if base.get_attribute(ATTRIB_MOVE_AND_FIRE) == 0 {
            let rounds = base.get_attribute(ATTRIB_ROUNDS);
            let speed = base.get_attribute(ATTRIB_SPEED);
            if speed != 0 {
                let shots = (rounds * i32::from(unit.speed)) / speed;
                if shots < i32::from(unit.shots) {
                    unit.shots = shots as u8;
                }
            }
        }

        unit.move_fraction = (normalized_cost * 4 - cost) as u8;

        true
    }
}

// ---------------------------------------------------------------------------
// Order processing context
// ---------------------------------------------------------------------------

/// Working copy of the unit and game state access during order processing.
pub struct OrderContext<'a> {
    /// The game (for inter-unit effects).
    pub(crate) world: &'a mut GameState,
    /// Id of the unit being processed.
    pub id: UnitId,
    /// Working copy of the unit.
    pub unit: UnitInfo,
    /// Order data (`move_to`/`fire_on`).
    pub data: OrderData,
}

impl<'a> OrderContext<'a> {
    /// `UnitsManager_SetNewOrderInt` for the current unit.
    pub fn set_new_order(&mut self, order: UnitOrder, state: UnitOrderState) -> bool {
        let delete_path = set_new_order_int(&mut self.unit, order, state);

        if delete_path {
            self.world.remove_path_request(self.id);
        }

        delete_path
    }

    /// Writes the working copy to the state without finishing the context (needed before
    /// calls to combat functions that work with `GameState`).
    pub fn sync_to_world(&mut self) {
        if self.world.has_unit(self.id) {
            self.world.write_unit(self.id, self.unit.clone());
            self.world.set_order_data(self.id, self.data);
        }
    }

    /// Re-reads the working copy from the state (after combat functions).
    pub fn sync_from_world(&mut self) {
        if let Some(unit) = self.world.unit(self.id) {
            self.unit = unit.clone();
        }
        self.data = self.world.order_data(self.id);
    }

    /// `UnitsManager_ScaleUnit` for the current unit.
    pub fn scale(&mut self, state: UnitOrderState) -> bool {
        self.set_new_order(UnitOrder::AwaitScaling, state)
    }

    /// `RestoreOrders` of the current unit.
    pub fn restore_orders(&mut self) {
        restore_orders(&mut self.unit);
    }

    /// `BlockedOnPathRequest` of the current unit (+ delayed reaction).
    pub fn blocked_on_path_request(&mut self, mode: bool) {
        self.world.remove_path_request(self.id);
        blocked_on_path_request(&mut self.unit, mode, self.world.disable_fire);
        self.world.add_to_delayed_reaction_list(self.id);
    }

    /// `MoveFinished` of the current unit (+ delayed reaction and target evaluation).
    pub fn move_finished(&mut self, mode: bool) {
        move_finished(&mut self.unit, mode, self.world.disable_fire);

        // `Ai_CheckMines(this)` (`ai.cpp:365`) — the minelayer reports to teams.
        if matches!(
            self.unit.unit_type,
            UnitType::MineLayer | UnitType::SeaMineLayer
        ) {
            crate::ai::ai_check_mines(self.world, self.id);
        }

        if mode && !self.world.disable_fire {
            self.unit.delayed_reaction = 1;
            self.world.add_to_delayed_reaction_list(self.id);

            if self.unit.ammo > 0 {
                // `Ai_EvaluateAttackTargets` (`ai.cpp:361`).
                self.world.ai_evaluate_attack_targets(self.id);
            }
        }
    }

    /// `UnitsManager_EndAttack` for the current unit.
    pub fn end_attack(&mut self) {
        self.world.end_attack(self.id);
        self.unit.live_attack = 0;
    }

    /// `UnitsManager_PinAttackTargets` for the current unit.
    pub fn pin_attack_targets(&mut self) {
        self.sync_to_world();
        self.world.pin_attack_targets(self.id);
        self.unit.live_attack = self
            .world
            .unit(self.id)
            .map(|unit| unit.live_attack)
            .unwrap_or(0);
    }

    /// Copy of another unit (`Hash_UnitHash[id]`).
    pub fn other(&self, id: UnitId) -> Option<UnitInfo> {
        self.world.unit(id).cloned()
    }

    /// Parent of the current unit.
    pub fn parent(&self) -> Option<UnitInfo> {
        self.unit.parent_unit.and_then(|id| self.other(id))
    }

    /// `UnitsManager_ScaleUnit` of another unit.
    pub fn scale_other(&mut self, id: UnitId, state: UnitOrderState) {
        self.world.scale_unit(id, state);
    }

    /// Writes the working copy back into the game.
    pub fn commit(self) {
        if self.world.has_unit(self.id) {
            self.world.write_unit(self.id, self.unit);
            self.world.set_order_data(self.id, self.data);
        }
    }
}

// ---------------------------------------------------------------------------
// Movement (port of UnitInfo::ExecuteMove / Move and GroundPath::Execute)
// ---------------------------------------------------------------------------

/// `UnitInfo::ExecuteMove` (`unitinfo.cpp:1633`).
fn execute_move(ctx: &mut OrderContext<'_>) -> bool {
    refresh_screen(&ctx.unit);

    if access::is_engaged(ctx.world, &ctx.unit) {
        return false;
    }

    if ctx.unit.path.is_some() {
        if ctx.unit.is_air() && ctx.unit.flags & HOVERING == 0 {
            ctx.set_new_order(UnitOrder::TakeOff, UnitOrderState::Init);
            ai_task_flag(ctx.world, "plane takeoff");
            false
        } else {
            if ctx.unit.speed != 0 {
                ai_task_flag(ctx.world, "moving");
            }
            path_execute(ctx)
        }
    } else if ctx.unit.flags & MISSILE_UNIT != 0 {
        // C++ `UnitInfo::ExecuteMove`: a missile without a path explodes on the current
        // cell — the parent strikes, the missile itself is destroyed.
        if let Some(parent_id) = ctx.unit.parent_unit {
            let position = ctx.unit.grid;
            ctx.world.end_attack(ctx.id);
            combat::attack(ctx.world, parent_id, position);
            ctx.world.destroy_unit(ctx.id);
        }
        false
    } else {
        ctx.unit.orders = UnitOrder::Await;
        ctx.unit.state = UnitOrderState::ExecutingOrder;
        false
    }
}

/// `UnitPath::Execute` for the three path implementations.
fn path_execute(ctx: &mut OrderContext<'_>) -> bool {
    match ctx.unit.path.clone() {
        Some(UnitPath::Ground(_)) => ground_path_execute(ctx),
        Some(UnitPath::Air(path)) => {
            // `AirPath::Execute` (`airpath.cpp:175-295`) for a projectile
            // (`MISSILE_UNIT`) steps **pixel by pixel** along the line `m_start_* →
            // `m_end_*`; `unit_move` (a cell per tick) does not apply to projectiles.
            if ctx.unit.flags & MISSILE_UNIT != 0 {
                missile_path_execute(ctx, &path);
                false
            } else {
                air_path_execute(ctx, &path)
            }
        }
        Some(UnitPath::Builder(_)) => builder_path_execute(ctx),
        None => false,
    }
}

/// `GroundPath::Execute` (`groundpath.cpp:210`).
///
/// The pixel part (`Paths_CalculateStep` speed spend) is preserved;
/// the cell transition is performed by [`unit_move`] (divergence 1 in the
/// module header).
fn ground_path_execute(ctx: &mut OrderContext<'_>) -> bool {
    let (end_x, end_y, step_index) = match ctx.unit.path.as_ref() {
        Some(UnitPath::Ground(path)) => (path.end_x, path.end_y, path.step_index),
        _ => return false,
    };

    ctx.data.move_to = Point::new(end_x, end_y);
    ctx.unit.target_grid = ctx.data.move_to;

    let mut step_index = step_index as usize;
    let step;

    // Skipping zero steps (`for (step = m_steps[m_step_index]; step->x == 0 ...)`).
    loop {
        let steps = match ctx.unit.path.as_ref() {
            Some(UnitPath::Ground(path)) => &path.steps,
            _ => return false,
        };

        if step_index >= steps.len() {
            ctx.blocked_on_path_request(true);
            if let Some(UnitPath::Ground(path)) = ctx.unit.path.as_mut() {
                path.step_index = step_index as u32;
            }
            return false;
        }

        let candidate = steps[step_index];
        if candidate.x != 0 || candidate.y != 0 {
            step = candidate;
            break;
        }

        step_index += 1;
    }

    if let Some(UnitPath::Ground(path)) = ctx.unit.path.as_mut() {
        path.step_index = step_index as u32;
    }

    let angle = paths_get_angle(i32::from(step.x), i32::from(step.y));
    let offset = DIRECTION_OFFSETS[(angle & 7) as usize];

    let mut speed = ctx.unit.speed;
    if ctx.unit.group_speed > 0 {
        speed = ctx.unit.group_speed - 1;
    }

    if speed == 0 || ctx.unit.engine == 0 {
        if ctx.unit.engine != 0 {
            ctx.unit.state = UnitOrderState::ExecutingOrder;
            ctx.move_finished(true);
        } else {
            // `ClearUnitList()` — removing the group order (P5 access).
            ctx.blocked_on_path_request(true);
        }
        return false;
    }

    if paths_update_angle(&mut ctx.unit, angle) {
        return false;
    }

    let grid = ctx.unit.grid;
    let target = Point::new(grid.x + offset.x, grid.y + offset.y);

    if ctx.world.map.index(target.x, target.y).is_none() {
        ctx.blocked_on_path_request(true);
        return false;
    }

    // `ORDER_MOVE_TO_UNIT`: receiving cargo on the target cell.
    if ctx.unit.orders == UnitOrder::MoveToUnit {
        if let Some(receiver) = access::get_receiver_unit(ctx.world, &ctx.unit, target) {
            let receiver_unit = ctx.other(receiver);
            let stored = receiver_unit
                .as_ref()
                .map(|unit| access::get_stored_unit_count(ctx.world, unit))
                .unwrap_or(0);
            let storable = receiver_unit
                .as_ref()
                .map(|unit| {
                    unit.base_values
                        .get_attribute(crate::unit_values::ATTRIB_STORAGE)
                })
                .unwrap_or(0);

            if stored >= storable {
                ctx.blocked_on_path_request(true);
            } else {
                ctx.unit.orders = UnitOrder::Idle;
                let receiver_type = receiver_unit.as_ref().map(|unit| unit.unit_type);
                if receiver_type == Some(UnitType::SeaTransport)
                    || receiver_type == Some(UnitType::ArmouredPersonnelCarrier)
                {
                    ctx.unit.state = UnitOrderState::PrepareStore;
                } else {
                    ctx.unit.state = UnitOrderState::Store;
                }
                ctx.unit.parent_unit = Some(receiver);
                ctx.unit.path = None;
                ctx.scale(UnitOrderState::Shrink);
            }
            return false;
        }
    }

    access::paths_is_occupied(ctx.world, target, i32::from(ctx.unit.angle), ctx.unit.team);

    let cost = access::is_accessible_unit(
        ctx.world,
        &ctx.unit,
        target,
        access::ACCESS_MODIFIER_IGNORE_VISIBILITY
            | access::ACCESS_MODIFIER_MOVES_UNDER_BRIDGE
            | access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
    ) as i32;

    if cost != 0 {
        let is_diagonal_step = offset.x != 0 && offset.y != 0;

        if paths_calculate_step(
            &mut ctx.unit,
            cost,
            is_diagonal_step,
            false,
            ctx.world.real_time,
        ) {
            ctx.unit.moved = 0;
            clear_movement_wait(&mut ctx.unit);
            ctx.unit.state = UnitOrderState::InTransition;
            ctx.unit.in_transit = true;
            true
        } else {
            ctx.unit.state = UnitOrderState::ExecutingOrder;
            ctx.move_finished(true);
            false
        }
    } else {
        // `Access_SetUnitDestination` (`groundpath.cpp:398`).
        let mode = !ctx.unit.is_sea() && !ctx.unit.is_amphibious();
        if access::set_unit_destination(ctx.world, &ctx.unit, grid, target, mode) {
            if register_movement_wait(&mut ctx.unit) {
                ctx.blocked_on_path_request(true);
            }
        } else {
            clear_movement_wait(&mut ctx.unit);

            if ctx.unit.orders == UnitOrder::Build
                || ctx.world.teams[ctx.unit.team.0 as usize].team_type
                    == crate::team::TeamKind::Eliminated
                || ctx.world.game_state == crate::enums::GameStateType::EndTurn
            {
                ctx.blocked_on_path_request(true);
            } else if ctx.world.teams[ctx.unit.team.0 as usize].team_type
                == crate::team::TeamKind::Remote
                || pathfinding::request_path(
                    ctx.world,
                    ctx.id,
                    access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                )
            {
                ctx.unit.state = UnitOrderState::NewOrder;
            }
        }
        false
    }
}

/// `AirPath::Execute` (`airpath.cpp`) — a step toward the end cell by `m_angle`;
/// the pixel interpolation of the segment (`m_delta_*`) is replaced in the lockstep
/// model by a one-cell-per-tick transition (divergence 1 in the module header).
fn air_path_execute(ctx: &mut OrderContext<'_>, path: &AirPath) -> bool {
    ctx.data.move_to = Point::new(path.end_x, path.end_y);
    ctx.unit.target_grid = ctx.data.move_to;

    let grid = ctx.unit.grid;
    let dx = i32::from(path.end_x) - i32::from(grid.x);
    let dy = i32::from(path.end_y) - i32::from(grid.y);

    if dx == 0 && dy == 0 {
        if ctx.unit.flags & MISSILE_UNIT != 0 {
            if let Some(parent_id) = ctx.unit.parent_unit {
                let position = ctx.unit.grid;
                ctx.world.end_attack(ctx.id);
                combat::attack(ctx.world, parent_id, position);
                ctx.world.destroy_unit(ctx.id);
            }
        } else {
            ctx.blocked_on_path_request(true);
        }
        return false;
    }

    if ctx.unit.is_air() && (ctx.unit.speed == 0 || ctx.unit.engine == 0) {
        // C++ `AirPath::Execute` (`airpath.cpp:186-199`): the speed and
        // engine check is **inside** `if (unit->flags & MOBILE_AIR_UNIT)`. For
        // projectiles (`MISSILE_UNIT`, `speed = 0` from `SC_ATTRI`) there is no
        // such check — they fly by `m_length` and finish with `Paths_FinishMove`.
        ctx.unit.state = UnitOrderState::ExecutingOrder;
        ctx.move_finished(true);
        return false;
    }

    if paths_update_angle(&mut ctx.unit, paths_get_angle(dx, dy)) {
        return false;
    }

    ctx.unit.state = UnitOrderState::InTransition;
    ctx.unit.in_transit = true;
    ctx.unit.moved = 0;
    true
}

/// `BuilderPath::Execute` (`builderpath.cpp`) — the builder's step to the target.
fn builder_path_execute(ctx: &mut OrderContext<'_>) -> bool {
    let target = ctx.data.move_to;
    let Some(step) = pathfinding::builder_path_step(ctx.world, &ctx.unit, target) else {
        ctx.blocked_on_path_request(true);
        return false;
    };

    let angle = paths_get_angle(i32::from(step.x), i32::from(step.y));
    if paths_update_angle(&mut ctx.unit, angle) {
        return false;
    }

    ctx.unit.state = UnitOrderState::InTransition;
    ctx.unit.in_transit = true;
    ctx.unit.moved = 0;
    true
}

/// `AirPath::Execute` (`airpath.cpp:175-295`) for a **projectile**: a pixel step
/// along the line from the launch point to the target.
///
/// C++ `AirPath::Execute` on each call:
///
/// ```text
/// m_step_x += m_delta_x;                      // airpath.cpp:219-220
/// m_step_y += m_delta_y;
/// position_x = (m_step_x >> 16) + m_start_x;  // :222-223
/// position_y = (m_step_y >> 16) + m_start_y;
/// grid_x = (position_x >> 6) - unit->grid_x;  // :226-227
/// if (grid_x || grid_y) {                     // :229
///     if (MISSILE_UNIT) Paths_DrawMissile(unit, position_x, position_y);  // :231
/// }
/// unit->OffsetDrawZones(offset_x, offset_y);  // :240 — x/y := position
/// unit->grid_x = unit->x >> 6;                 // :241-242
/// --m_length;                                  // :244
/// ```
///
/// `m_delta_*` — the 22-bit step fraction (pixels/tick), `m_start_*` — the pixel
/// of the launch cell center, `m_length` — the number of ticks (≈ the number of cells of distance).
/// Result: the projectile traverses **the same segment** as in the original, not an 8-direction
/// polyline over cells. The cell (`grid`) is recomputed from the pixel, and
/// the trail (`Paths_DrawMissile`) is placed from "was → became" — also along the line.
fn missile_path_execute(ctx: &mut OrderContext<'_>, path: &AirPath) {
    let mut path = *path;

    // The projectile does not turn (`AirPath::AirPath`: `m_angle = unit->angle`),
    // so `Paths_UpdateAngle` in C++ returns false immediately; the step
    // is performed only when the angle already matches.
    if paths_update_angle(&mut ctx.unit, i32::from(path.angle)) {
        if let Some(UnitPath::Air(stored)) = ctx.unit.path.as_mut() {
            *stored = path;
        }

        return;
    }

    path.step_x = path.step_x.wrapping_add(path.delta_x);
    path.step_y = path.step_y.wrapping_add(path.delta_y);

    let position_x = (path.step_x >> 16) + i32::from(path.start_x);
    let position_y = (path.step_y >> 16) + i32::from(path.start_y);

    let old_grid = ctx.unit.grid;
    let old_pixel = (i32::from(ctx.unit.x), i32::from(ctx.unit.y));

    // `OffsetDrawZones`: the pixel position of the projectile sprite.
    ctx.unit.x = position_x.clamp(0, i32::from(u16::MAX)) as u16;
    ctx.unit.y = position_y.clamp(0, i32::from(u16::MAX)) as u16;

    let grid = Point::new(
        (position_x >> 6).clamp(0, i32::from(crate::geo::MAP_WIDTH) - 1) as i16,
        (position_y >> 6).clamp(0, i32::from(crate::geo::MAP_HEIGHT) - 1) as i16,
    );

    if grid != old_grid {
        ctx.unit.grid = grid;
        // `Paths_DrawMissile(unit, position_x, position_y)` (`airpath.cpp:231`,
        // `:362-406`): three particles in the **left** cell (`grid_x/grid_y` are still
        // old: C++ calls this before updating `unit->grid_*`) on the segment "old
        // pixel → new pixel".
        spawn_missile_trail(
            ctx.world,
            &ctx.unit,
            old_grid,
            old_pixel,
            (position_x, position_y),
        );
    }

    path.length = path.length.saturating_sub(1);

    if path.length > 0 {
        if let Some(UnitPath::Air(stored)) = ctx.unit.path.as_mut() {
            *stored = path;
        }

        return;
    }

    // `Paths_FinishMove` (`airpath.cpp:408-420`) for `MISSILE_UNIT`:
    // `UnitsManager_EndAttack` → `parent->Attack(grid_x, grid_y)` →
    // `UnitsManager_DestroyUnit(unit)`.
    ctx.unit.path = None;

    if let Some(parent_id) = ctx.unit.parent_unit {
        let position = ctx.unit.grid;
        ctx.world.end_attack(ctx.id);
        combat::attack(ctx.world, parent_id, position);
        ctx.world.destroy_unit(ctx.id);
    }
}

/// `Paths_DrawMissile` (`airpath.cpp:362-406`) — the trail behind a flying projectile.
///
/// C++ on every projectile cell transition (`AirPath::Execute`,
/// `airpath.cpp:229-232`, the `grid_x || grid_y` branch for `MISSILE_UNIT`) calls
/// `Paths_DrawMissile` and places **three** effect units on the cell the projectile
/// leaves:
/// * `ROCKET` → `RKTSMOKE` (5 frames `D_RKTSMOKE`), `TORPEDO` → `TRPBUBLE`
///   (3 frames `D_TRPBUBLE`);
/// * `UnitsManager_DeployUnit(..., skip_map_status_update = true)` with flags from
///   `SC_UNITS`: both have `EXPLODING|ANIMATED|MISSILE_UNIT` (`TRPBUBLE` also
///   `GROUND_COVER`), so `UnitInfo` immediately gets
///   `ORDER_EXPLODE`/`ORDER_STATE_DESTROY` (`units_manager.cpp:2390-2393`);
/// * frames are advanced by `UnitsManager_ProgressExplosion` (`units_manager.cpp:4006`):
///   `moved & 1` → `image_index + 1`, removal at `image_index ==
///   `image_index_max` (4 for smoke, 2 for bubbles);
/// * `OffsetDrawZones` spreads the three particles along the segment "old projectile pixel
///   → new pixel" at 0, 1/3 and 2/3 — this is drawing only (`unit->x/y` of the
///   particles themselves does not change).
///
/// The port stores the spread in the particles' pixel `x/y` (the effect renderer takes them
/// instead of the cell), the cell itself stays the C++ one (left). The spawn does not depend
/// on visibility: in C++ `Paths_DrawMissile` is called only with
/// `IsVisibleToTeam(GameManager_PlayerTeam)`, but `GameManager_PlayerTeam` is a
/// local UI field, and binding to it would diverge client states
/// (DESIGN §6.1). Particle visibility is set by `UnitInfo::InitStealthStatus`
/// (own team; `skip_map_status_update = true`), so foreign trails are not
/// drawn — as in C++.
fn spawn_missile_trail(
    state: &mut GameState,
    unit: &UnitInfo,
    spawn_grid: Point,
    from_pixel: (i32, i32),
    to_pixel: (i32, i32),
) {
    let particle_type = match unit.unit_type {
        UnitType::Rocket => UnitType::RocketSmoke,
        UnitType::Torpedo => UnitType::TorpedoBubble,
        _ => return,
    };

    let (start_x, start_y) = from_pixel;
    let delta_x = to_pixel.0 - start_x;
    let delta_y = to_pixel.1 - start_y;

    if delta_x == 0 && delta_y == 0 {
        return;
    }

    // `index = 3` in C++: exactly three particles per cell.
    for index in 0..3 {
        let Some(particle_id) = state.spawn_unit(particle_type, unit.team, spawn_grid, 0) else {
            continue;
        };

        if let Some(mut particle) = state.unit(particle_id).cloned() {
            // `OffsetDrawZones(unit->x + (delta >> 16) - particle->x)`:
            // 0, 1/3 and 2/3 of the segment.
            particle.x = (start_x + delta_x * index / 3) as u16;
            particle.y = (start_y + delta_y * index / 3) as u16;
            // Visibility — like `UnitInfo::InitStealthStatus` at
            // `UnitsManager_DeployUnit`: the particle is seen only by its own team
            // (`Paths_DrawMissile` skips `Access_UpdateMapStatus`).
            state.write_unit(particle_id, particle);
        }
    }
}

/// `UnitInfo::Move` (`unitinfo.cpp:1730`) — completing a cell transition.
///
/// In P4 one call completes one cell (divergence 1 in the module header).
fn unit_move(ctx: &mut OrderContext<'_>) {
    if access::is_engaged(ctx.world, &ctx.unit) {
        return;
    }

    refresh_screen(&ctx.unit);

    if ctx.unit.state != UnitOrderState::InTransition {
        return;
    }

    let offset = DIRECTION_OFFSETS[(ctx.unit.angle & 7) as usize];
    let grid = ctx.unit.grid;
    let target = Point::new(grid.x + offset.x, grid.y + offset.y);
    if ctx.world.map.index(target.x, target.y).is_none() {
        ctx.blocked_on_path_request(true);
        return;
    }

    ctx.unit.grid = target;
    ctx.unit.moved = 0;
    ctx.unit.in_transit = false;

    // Projectiles do not reach here: [`path_execute`] moves them pixel by pixel
    // ([`missile_path_execute`]) and the trail is placed from the pixel segment
    // (`Paths_DrawMissile`, `airpath.cpp:229-232`).

    // C++ `Move()` at `moved == GFX_MAP_TILE_SIZE`:
    // `path_steps[step_index]->x -= offset_x` — the step is "eaten" by the transition.
    if let Some(UnitPath::Ground(path)) = ctx.unit.path.as_mut() {
        if let Some(step) = path.steps.get_mut(path.step_index as usize) {
            step.x = step.x.wrapping_sub(offset.x as i8);
            step.y = step.y.wrapping_sub(offset.y as i8);
        }
    }

    // C++ `UnitInfo::Move` (`unitinfo.cpp:1835`): when a cell completes
    // `path->UpdateUnitAngle(this)` makes one rotation step toward the next route
    // step. A temporary step snapshot is needed to avoid holding a
    // `ctx.unit.path` borrow at the same time as `&mut ctx.unit`.
    if let Some(UnitPath::Ground(path)) = ctx.unit.path.as_ref() {
        if let Some(next) = path.steps.get(path.step_index as usize + 1).copied() {
            paths_update_angle(
                &mut ctx.unit,
                paths_get_angle(i32::from(next.x), i32::from(next.y)),
            );
        }
    }

    ctx.unit.state = UnitOrderState::InProgress;

    if ctx.unit.orders == UnitOrder::Build && !ctx.unit.build_list.is_empty() {
        // C++: `Access_IsAccessible(build_list[0], ...)` → `Build()`.
        // TODO(P3/P5): site accessibility check and `UnitInfo::Build`.
    }

    if ctx.unit.has_move_order() {
        // `FollowUnit()` — group order (P5 access).
    }

    // The C++ repeat-transition loop (`while (IN_TRANSITION &&
    // !visible)`) is not executed in network mode: one cell per tick.
}

// ---------------------------------------------------------------------------
// Order processors
// ---------------------------------------------------------------------------

/// `UnitsManager_PerformAutoSurvey`, simplified to state decisions.
fn perform_auto_survey(ctx: &mut OrderContext<'_>) {
    if ctx.world.play_mode == crate::enums::PlayMode::Unknown {
        return;
    }
    if !ctx.world.is_active_turn(ctx.unit.team) {
        return;
    }
    if !ctx.unit.auto_survey || ctx.unit.speed == 0 {
        return;
    }

    let team_info = &ctx.world.teams[ctx.unit.team.0 as usize];
    if ctx.world.network_game && team_info.finished_turn {
        return;
    }

    let team_type = team_info.team_type;
    if team_type == crate::team::TeamKind::Computer || team_type == crate::team::TeamKind::Remote {
        ctx.unit.auto_survey = false;
    } else {
        // `Ai_EnableAutoSurvey` (`ai.cpp:184`) creates `TaskAutoSurvey`.
        ctx.world.ai_enable_auto_survey(ctx.id);
        schedule_delayed_tasks(ctx.world, ctx.id, false);
    }
}

/// `UnitsManager_ProcessOrderAwait` (`units_manager.cpp:2606`).
fn process_order_await(ctx: &mut OrderContext<'_>) {
    if ctx.unit.unit_type == UnitType::Road || ctx.unit.unit_type == UnitType::WaterPlatform {
        return;
    }

    match ctx.unit.state {
        UnitOrderState::ExecutingOrder => {
            if ctx.unit.unit_type == UnitType::Bridge && is_bridge_elevated(&ctx.unit) {
                if access::get_active_unit_with_flags(ctx.world, ctx.unit.grid, MOBILE_SEA_UNIT)
                    .is_none()
                {
                    ctx.set_new_order(UnitOrder::Move, UnitOrderState::Lower);
                }
            }

            perform_auto_survey(ctx);
            animate(&ctx.unit);
        }
        UnitOrderState::ReadyToExecuteOrder => {
            perform_auto_survey(ctx);
            animate(&ctx.unit);
        }
        UnitOrderState::ClearPath => {
            ctx.unit.path = None;
            ctx.unit.state = UnitOrderState::ExecutingOrder;

            if ctx.unit.unit_type == UnitType::Bridge
                && is_bridge_elevated(&ctx.unit)
                && access::get_active_unit_with_flags(ctx.world, ctx.unit.grid, MOBILE_SEA_UNIT)
                    .is_none()
            {
                ctx.set_new_order(UnitOrder::Move, UnitOrderState::Lower);
            }

            perform_auto_survey(ctx);
            animate(&ctx.unit);
        }
        _ => {}
    }
}

/// `UnitsManager_ProcessOrderTransform` (`units_manager.cpp:2646`).
fn process_order_transform(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Transform");
    refresh_screen(&ctx.unit);

    match ctx.unit.state {
        UnitOrderState::Init => {
            // `UnitsManager_DeployMasterBuilderInit`:
            //   DeployConstructionSiteUtilities(MININGST) — P5 access;
            //   SetOrderState(PROGRESS_TRANSFORMING);
            //   SetNewOrderInt(AWAIT_TAPE_POSITIONING, TAPE_POSITIONING_INIT).
            ctx.unit.state = UnitOrderState::ProgressTransforming;
            ctx.set_new_order(
                UnitOrder::AwaitTapePositioning,
                UnitOrderState::TapePositioningInit,
            );
        }
        UnitOrderState::ProgressTransforming => {
            // TODO(P3/P5): `UnitsManager_DeployMasterBuilder` — destruction
            // of the constructor and placement of the mine/generator/slab.
        }
        UnitOrderState::FinishTransforming => {
            ctx.unit.orders = UnitOrder::PowerOn;
            ctx.unit.state = UnitOrderState::Init;

            if ctx.world.teams[ctx.unit.team.0 as usize].team_type == crate::team::TeamKind::Player
            {
                // TODO(P3): GameManager_OptimizeProduction + AutoSelectNext.
            }
        }
        _ => {}
    }
}

/// `UnitsManager_ProcessOrderMove` (`units_manager.cpp:2672`).
fn process_order_move(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::IssuingPath {
        ctx.unit.state = UnitOrderState::InProgress;
    }

    match ctx.unit.state {
        UnitOrderState::ExecutingOrder => {
            if !pursue_enemy(ctx) {
                perform_auto_survey(ctx);
                animate(&ctx.unit);
            }
        }
        UnitOrderState::Store => {
            ai_task_flag(ctx.world, "Storing");
            store_unit(ctx);
        }
        UnitOrderState::InProgress => {
            if execute_move(ctx) {
                ai_task_flag(ctx.world, "Moving");
                unit_move(ctx);
            }
        }
        UnitOrderState::InTransition => {
            ai_task_flag(ctx.world, "Moving");
            unit_move(ctx);
        }
        UnitOrderState::PathRequestCancel => {
            ctx.unit.enemy_unit = None;
            ctx.blocked_on_path_request(false);
        }
        UnitOrderState::Init | UnitOrderState::MoveInit => {
            if init_unit_move(ctx) && execute_move(ctx) {
                ai_task_flag(ctx.world, "Moving");
                unit_move(ctx);
            }
        }
        UnitOrderState::MoveGettingPath => {
            if !pursue_enemy(ctx) {
                perform_auto_survey(ctx);
                animate(&ctx.unit);
            }
        }
        UnitOrderState::Elevate => {
            if ctx.unit.image_index == ctx.unit.image_index_max {
                ctx.set_new_order(UnitOrder::Await, UnitOrderState::ExecutingOrder);
            } else {
                if !is_bridge_elevated(&ctx.unit) {
                    // A bridge raised again before the end of lowering is still in the
                    // stationary unit list.
                    if remove_from_list(ctx.world, ctx.id, UnitListKind::GroundCover) {
                        ctx.world
                            .add_to_draw_list_with(ctx.id, STATIONARY | UPGRADABLE | SELECTABLE);
                    }
                }
                ctx.unit.image_index = ctx.unit.image_index.wrapping_add(1);
            }
        }
        UnitOrderState::Lower => {
            if access::get_active_unit_with_flags(ctx.world, ctx.unit.grid, MOBILE_SEA_UNIT)
                .is_some()
            {
                ctx.set_new_order(UnitOrder::Move, UnitOrderState::Elevate);
            } else if is_bridge_elevated(&ctx.unit) {
                ctx.unit.image_index = ctx.unit.image_index.wrapping_sub(1);
            } else {
                if remove_from_list(ctx.world, ctx.id, UnitListKind::Stationary) {
                    ctx.world.add_to_draw_list(ctx.id);
                }
                ctx.set_new_order(UnitOrder::Await, UnitOrderState::ExecutingOrder);
            }
        }
        UnitOrderState::NewOrder => {
            // C++ sets `PendingAirGroupLeader` only for a member of a group
            // list (`UnitInfo::GetUnitList() != nullptr`).
            if ctx.unit.is_air()
                && ctx.world.teams[ctx.unit.team.0 as usize].team_type
                    == crate::team::TeamKind::Player
                && ctx.world.group_members(ctx.id).is_some()
            {
                ctx.world.pending_air_group_leader = Some(ctx.id);
            }

            if !pursue_enemy(ctx) {
                perform_auto_survey(ctx);
                animate(&ctx.unit);
            }
        }
        _ => {}
    }
}

/// Removing a unit from a concrete list (C++ `SmartList::Remove`).
fn remove_from_list(state: &mut GameState, id: UnitId, kind: UnitListKind) -> bool {
    state.remove_from_list(kind, id)
}

/// `UnitsManager_PursueEnemy` (`units_manager.cpp:3516`).
///
/// The order conditions and transitions are ported; the path search itself
/// (`Paths_RequestPath`) — P5, attack range (`Access_IsWithinAttackRange`) —
/// P5 (replaced by the squared distance in cells).
fn pursue_enemy(ctx: &mut OrderContext<'_>) -> bool {
    if ctx.unit.orders != UnitOrder::MoveToAttack
        || ctx.unit.state != UnitOrderState::ExecutingOrder
        || ctx.unit.delayed_reaction != 0
        || ctx.world.units_active[ctx.unit.team.0 as usize].is_some()
    {
        return false;
    }

    let Some(enemy_id) = ctx.unit.enemy_unit else {
        return false;
    };
    let Some(enemy) = ctx.other(enemy_id) else {
        return false;
    };

    let unit_range = ctx
        .unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_RANGE);
    let position = attack_position(ctx, &enemy);
    let enemy_range = i32::from(enemy.base_values.get_attribute(ATTRIB_SPEED));

    let can_pursue = enemy.hits > 0
        && ctx.unit.ammo > 0
        && ctx.unit.team != enemy.team
        && enemy.orders != UnitOrder::Idle
        && (enemy.is_visible_to_team(ctx.unit.team) || enemy_range == 0);

    if !can_pursue {
        schedule_delayed_tasks(ctx.world, ctx.id, true);
        return false;
    }

    let team_type = ctx.world.teams[ctx.unit.team.0 as usize].team_type;
    let is_computer = team_type == crate::team::TeamKind::Computer;
    let is_eliminated = team_type == crate::team::TeamKind::Eliminated;
    let is_remote = team_type == crate::team::TeamKind::Remote;
    let unit_has_no_speed = ctx.unit.base_values.get_attribute(ATTRIB_SPEED) == 0;
    let squared_distance = squared_distance(ctx.unit.grid, position);

    if (is_computer || unit_has_no_speed) && squared_distance > unit_range * unit_range {
        return false;
    }
    if is_eliminated {
        return false;
    }

    let move_to = ctx.data.move_to;
    let enemy_is_moving = matches!(
        enemy.orders,
        UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
    );

    if squared_distance > unit_range * unit_range
        && (ctx.unit.path.is_none() || move_to.x != enemy.grid.x || move_to.y != enemy.grid.y)
        && (!enemy_is_moving || enemy.state == UnitOrderState::ExecutingOrder)
    {
        // `Paths_RequestPath(unit, SameClassBlocks)` — the path to the attack position.
        pathfinding::request_path(ctx.world, ctx.id, access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS);
        return false;
    }

    if ctx.unit.shots > 0
        && access::is_within_attack_range(ctx.world, &ctx.unit, position, unit_range)
    {
        if !ctx.world.unit_list(UnitListKind::Particle).is_empty() {
            return false;
        }

        ctx.world.units_active[ctx.unit.team.0 as usize] = Some(ctx.id);
        ctx.world.delayed_reactions_pending = true;
        true
    } else {
        if !is_remote {
            ctx.unit.enemy_unit = None;
            ctx.set_new_order(UnitOrder::Await, UnitOrderState::ClearPath);
            schedule_delayed_tasks(ctx.world, ctx.id, false);
        }
        false
    }
}

/// `UnitsManager_GetAttackPosition` (`units_manager.cpp:4847`).
fn attack_position(ctx: &OrderContext<'_>, enemy: &UnitInfo) -> Point {
    access::get_attack_position(ctx.world, &ctx.unit, enemy)
}

/// `Access_GetSquaredDistance` (`access.cpp`) — the squared distance in cells.
fn squared_distance(first: Point, second: Point) -> i32 {
    let dx = i32::from(first.x) - i32::from(second.x);
    let dy = i32::from(first.y) - i32::from(second.y);
    dx * dx + dy * dy
}

/// `UnitsManager_InitUnitMove` (`units_manager.cpp:3605`) + `InitUnitPath`
/// (part of `UnitsManager_InitUnitPath`).
fn init_unit_move(ctx: &mut OrderContext<'_>) -> bool {
    refresh_screen(&ctx.unit);
    init_unit_path(ctx);

    if ctx.unit.state == UnitOrderState::Init {
        // `FollowUnit()` + `Redraw()` — P5/UI.
        ctx.unit.state = UnitOrderState::InProgress;

        if ctx.world.network_game && ctx.unit.is_air() {
            // `Remote_SendNetPacket_38(unit)` — network (max_net), not max_core.
            return false;
        }
    }

    ctx.unit.state == UnitOrderState::InProgress
}

/// `UnitsManager_InitUnitPath` (`units_manager.cpp:4776`).
///
/// The air path is computed immediately (`Paths_GetAirPath`); the ground one is put into
/// the [`pathfinding::request_path`] queue (synchronous pathfinding, DESIGN
/// §1.4/risk 2), and the order moves to `MOVE_GETTING_PATH`/`NEW_ORDER` until
/// the result is applied in `poll_results`.
fn init_unit_path(ctx: &mut OrderContext<'_>) {
    ctx.unit.path = None;

    if ctx.unit.flags & MISSILE_UNIT != 0 {
        let fire_on = ctx.data.fire_on;
        ctx.unit.path = pathfinding::get_missile_path(&ctx.unit, fire_on).map(UnitPath::Air);
        return;
    }

    if ctx.world.teams[ctx.unit.team.0 as usize].team_type == crate::team::TeamKind::Remote {
        ctx.unit.state = UnitOrderState::NewOrder;
        return;
    }

    if ctx.unit.state == UnitOrderState::MoveInit {
        if ctx.unit.is_air() && ctx.unit.orders != UnitOrder::MoveToAttack {
            let target = ctx.data.move_to;
            ctx.unit.path = pathfinding::paths_get_air_path(ctx.world, &mut ctx.unit, target)
                .map(UnitPath::Air);
            ctx.restore_orders();
        } else if pathfinding::request_path(
            ctx.world,
            ctx.id,
            access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
        ) {
            ctx.unit.state = UnitOrderState::MoveGettingPath;
        }
    } else if ctx.world.group_members(ctx.id).is_some() {
        access::group_attack_order(ctx.world, ctx.id, true);
    } else if ctx.unit.is_air() && ctx.unit.orders != UnitOrder::MoveToAttack {
        let target = ctx.data.move_to;
        ctx.unit.path =
            pathfinding::paths_get_air_path(ctx.world, &mut ctx.unit, target).map(UnitPath::Air);
    } else if pathfinding::request_path(
        ctx.world,
        ctx.id,
        access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
    ) {
        ctx.unit.state = UnitOrderState::NewOrder;
    }
}

/// Linear route (`GroundPath::AppendLinearSteps`) — used by
/// the network layer/tests (packet 38), not by the simulation search.
#[allow(dead_code)]
fn build_linear_path(ctx: &mut OrderContext<'_>) {
    let start = ctx.unit.grid;
    let target = ctx.data.move_to;
    let steps = pathfinding::append_linear_steps(start, target);

    ctx.unit.path = Some(UnitPath::Ground(GroundPath {
        end_x: target.x,
        end_y: target.y,
        step_index: 0,
        steps,
    }));
}

/// `UnitsManager_Store` (`units_manager.cpp:3625`).
fn store_unit(ctx: &mut OrderContext<'_>) {
    let Some(parent_id) = ctx.unit.parent_unit else {
        return;
    };
    let Some(mut parent) = ctx.other(parent_id) else {
        return;
    };

    if parent.storage
        < parent
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_STORAGE) as i16
    {
        parent.storage = parent.storage.wrapping_add(1);
        ctx.world.write_unit(parent_id, parent);
    }

    // `ClearUnitList()` — P5.
    ctx.unit.orders = UnitOrder::Idle;
    ctx.unit.state = UnitOrderState::Store;
}

/// `UnitsManager_ProcessOrderFire` (`units_manager.cpp:2781`).
fn process_order_fire(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Firing");
    ctx.world.combat_effects_active = true;

    match ctx.unit.state {
        UnitOrderState::Init => {
            ctx.world.pending_attacks.push(ctx.id);
            ctx.pin_attack_targets();
            ctx.unit.state = UnitOrderState::AttackPending;
        }
        UnitOrderState::InProgress => {
            ctx.world.orders_pending = true;

            if combat::aim_at_target(ctx.world, &mut ctx.unit, ctx.data.fire_on) {
                ctx.world.orders_pending = true;
                combat::prepare_fire(ctx.world, &mut ctx.unit);
            }
        }
        UnitOrderState::ReadyToFire => {
            ctx.world.orders_pending = true;
            combat::prepare_fire(ctx.world, &mut ctx.unit);
        }
        UnitOrderState::FireInProgress => {
            ctx.world.orders_pending = true;
            let fire_on = ctx.data.fire_on;
            let finished = combat::progress_fire(ctx.world, &mut ctx.unit);
            if finished {
                // `Attack(fire_on)` works with game state: first
                // we write the working copy, then re-read it after combat.
                ctx.sync_to_world();
                combat::attack(ctx.world, ctx.id, fire_on);
                ctx.sync_from_world();
            }
        }
        UnitOrderState::AttackPending => {}
        UnitOrderState::AttackBeginning => {
            ctx.unit.state = UnitOrderState::InProgress;
            ctx.world.orders_pending = true;

            if combat::aim_at_target(ctx.world, &mut ctx.unit, ctx.data.fire_on) {
                ctx.world.orders_pending = true;
                combat::prepare_fire(ctx.world, &mut ctx.unit);
            }
        }
        _ => {}
    }
}

/// `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2834`).
fn process_order_build(ctx: &mut OrderContext<'_>) {
    match ctx.unit.state {
        UnitOrderState::Init => {
            ai_task_flag(ctx.world, "Build start");
            start_building(ctx.world, &mut ctx.unit);
        }
        UnitOrderState::InProgress => {
            if execute_move(ctx) {
                ai_task_flag(ctx.world, "Moving");
                unit_move(ctx);
            }
        }
        UnitOrderState::InTransition => {
            ai_task_flag(ctx.world, "Moving");
            unit_move(ctx);
        }
        UnitOrderState::BuildInProgress => {
            if ctx.world.settings_effects && ctx.unit.unit_type == UnitType::Constructor {
                // The construction animation advances `moved` (as in C++).
                ctx.unit.moved = ctx.unit.moved.wrapping_add(1);
            }
        }
        UnitOrderState::BuildCancel | UnitOrderState::BuildAbort => {
            ai_task_flag(ctx.world, "Build cancel");
            cancel_building(&mut ctx.unit);
            // `Access_DestroyUtilities(grid, true, false, false, false)`
            // (`unitinfo.cpp:4269`): only the site utilities and the slab.
            // In C++ the builder already stands on the site; in the port it stays on
            // the original cell, so we remove the site from `target_grid`.
            access::destroy_utilities(ctx.world, ctx.unit.target_grid, true, false, false, false);
        }
        UnitOrderState::SelectSite => {
            animate(&ctx.unit);
        }
        UnitOrderState::BuildClearing => {
            ai_task_flag(ctx.world, "Build clearing");
            // TODO(P3/P5): UnitsManager_BuildClearing — destruction
            // of the construction, rubble, utilities, unit return.
        }
        UnitOrderState::UnitReady => {
            // C++ `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2891`):
            // `if (unit->path) UnitsManager_BuildNext(unit); else unit->Animate();`
            // — a builder that completed a building just stands and animates;
            // there is no automatic exit. The exit order is given by the
            // **player** clicking a cell (`game_manager.cpp:6879-6890`:
            // `ORDER_STATE_UNIT_READY` → `ORDER_ACTIVATE`/`IN_TRANSITION`,
            // `ORDER_ACTIVATE` arrives here), by the AI — the task
            // `TaskActivate` (`crate::unit_order::issue_builder_exit`).
            if ctx.unit.path.is_some() {
                // TODO(P3): UnitsManager_BuildNext — storage/materials check.
                clear_build_list_and_path(&mut ctx.unit);
            } else {
                animate(&ctx.unit);
            }
        }
        _ => {}
    }
}

/// `UnitsManager_ProcessOrderActivate` (`units_manager.cpp:2908`).
fn process_order_activate(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Activation");

    match ctx.unit.state {
        UnitOrderState::ExecutingOrder => {
            // `UnitsManager_ActivateUnit(unit)` (`units_manager.cpp:3826`):
            // `unit` — the container (factory/transport), `unit->GetParent()` —
            // the ready unit the player brings out by clicking (packet 08/P3) or the AI
            // (`issue_factory_exit`).
            activate_unit(ctx);
        }
        UnitOrderState::InTransition => {
            if ctx.unit.unit_type == UnitType::Constructor {
                ctx.unit.state = UnitOrderState::BuildInProgress;
                ctx.set_new_order(
                    UnitOrder::AwaitTapePositioning,
                    UnitOrderState::TapePositioningDeinit,
                );
            } else {
                activate_engineer(ctx);
            }
        }
        UnitOrderState::BuildInProgress => {
            activate_engineer(ctx);
        }
        _ => {}
    }
}

/// `UnitsManager_ActivateUnit` (`units_manager.cpp:3826-3913`) — bringing a ready
/// unit out of the container.
///
/// Called for the **container**: at a factory `parent_unit` is the ready unit
/// (`UnitInfo::SpawnNewUnit`, `unitinfo.cpp:3513`:
/// `SetParent(factory_unit.Get())`), at a transport — the passenger. C++
/// (`units_manager.cpp:3827-3936`):
///
/// ```text
/// client = unit->GetParent();                      // the ready unit
/// if ((client->MOBILE_AIR_UNIT && client->speed > 0) ||
///     (!Paths_IsOccupied(move_to) && !client->MOBILE_AIR_UNIT &&
///      Access_IsAccessible(client->type, move_to, SameClassBlocks))) {
///     --unit->storage; unit->SetParent(nullptr);
///     if (unit->GetBuildList().GetCount() > 0) { ORDER_BUILD/EXECUTING_ORDER;
///         build_time = turns; StartBuilding(); }       // the next order
///     else { ORDER_AWAIT/EXECUTING_ORDER; }
///     if (client->MOBILE_AIR_UNIT) {
///         client->SetParent(nullptr); client->move_to = unit->move_to;
///         client->SetOrder(ORDER_MOVE); SetOrderState(INIT);
///         ScaleUnit(client, EXPAND);                   // takeoff from the factory
///     } else {
///         Access_TriggerEnemyMine(move_to);
///         UnitsManager_UpdateMapHash(client, move_to); // the unit on the click cell
///         client->SetOrder(ORDER_AWAIT); SetOrderState(EXECUTING_ORDER);
///         client->InitStealthStatus(); ScaleUnit(client, EXPAND);
///     }
/// } else { unit->RestoreOrders(); }
/// ```
///
/// An air unit exits **by itself** (`ORDER_MOVE`/`INIT` → `ExecuteMove` →
/// `ORDER_TAKE_OFF`), a ground/naval one is moved to the click cell. If the
/// cell is occupied/impassable — the order returns to the deferred one (the factory stays
/// in `ORDER_BUILD`/`UNIT_READY`), exactly as in C++.
fn activate_unit(ctx: &mut OrderContext<'_>) {
    let Some(client_id) = ctx.unit.parent_unit else {
        ctx.restore_orders();
        return;
    };
    let Some(client) = ctx.other(client_id) else {
        ctx.restore_orders();
        return;
    };

    let move_to = ctx.data.move_to;
    let is_air = client.is_air();
    let destination_ok = if is_air {
        // An air unit does not need a site — only fuel/engine:
        // `(client->flags & MOBILE_AIR_UNIT) && client->speed > 0`.
        client.speed > 0
    } else {
        !access::paths_is_occupied(ctx.world, move_to, 0, ctx.unit.team)
            && access::is_accessible(
                ctx.world,
                client.unit_type,
                ctx.unit.team,
                move_to,
                access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) > 0
    };

    if !destination_ok {
        ctx.restore_orders();
        return;
    }

    // `--unit->storage; unit->SetParent(nullptr);` — the container released the unit.
    ctx.unit.storage = ctx.unit.storage.saturating_sub(1);
    ctx.unit.parent_unit = None;

    if let Some(&next) = ctx.unit.build_list.first() {
        // `SetOrder(ORDER_BUILD); SetOrderState(EXECUTING_ORDER);
        //  build_time = BuildMenu_GetTurnsToBuild(...); StartBuilding();`
        let turns = production::turns_to_build(ctx.world, ctx.unit.team, next).max(1);
        ctx.unit.build_time = turns.min(i32::from(u8::MAX)) as u8;
        ctx.unit.build_paused = false;
        ctx.unit.orders = UnitOrder::Build;
        ctx.unit.state = UnitOrderState::ExecutingOrder;
        start_building(ctx.world, &mut ctx.unit);
    } else {
        ctx.unit.orders = UnitOrder::Await;
        ctx.unit.state = UnitOrderState::ExecutingOrder;
    }

    if is_air {
        // `client->SetParent(nullptr); client->move_to = unit->move_to;
        //  SetOrder(ORDER_MOVE); SetOrderState(ORDER_STATE_INIT);`
        let mut client = client;
        client.parent_unit = None;
        client.target_grid = move_to;
        client.orders = UnitOrder::Move;
        client.state = UnitOrderState::Init;
        ctx.world.write_unit(client_id, client);

        let mut data = ctx.world.order_data(client_id);
        data.move_to = move_to;
        ctx.world.set_order_data(client_id, data);

        // `UnitsManager_ScaleUnit(client, ORDER_STATE_EXPAND)`: the aircraft
        // "grows" out of the factory (`scaler_adjust` 4 → 0), then
        // `RestoreOrders` returns `ORDER_MOVE`/`INIT` → takeoff.
        ctx.world.scale_unit(client_id, UnitOrderState::Expand);
    } else {
        // `UnitsManager_UpdateMapHash(client, move_to)` +
        // `UnitInfo::UpdateGridPosition` (`unitinfo.cpp:4995-5011`): the cell and
        // the pixel center of the cell.
        let mut client = client;
        client.grid = move_to;
        client.target_grid = move_to;
        client.x = (i32::from(move_to.x) * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2) as u16;
        client.y = (i32::from(move_to.y) * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2) as u16;
        client.parent_unit = None;
        client.orders = UnitOrder::Await;
        client.state = UnitOrderState::ExecutingOrder;
        ctx.world.write_unit(client_id, client);

        let mut data = ctx.world.order_data(client_id);
        data.move_to = move_to;
        ctx.world.set_order_data(client_id, data);

        ctx.world.scale_unit(client_id, UnitOrderState::Expand);
    }
}

/// `UnitsManager_ActivateEngineer` (`units_manager.cpp:3407`).
fn activate_engineer(ctx: &mut OrderContext<'_>) {
    // `Access_DestroyUtilities(grid, false, false, false, false)` — only
    // the construction site tapes/cones are removed.
    access::destroy_utilities(ctx.world, ctx.unit.grid, false, false, false, false);
    ctx.unit.prior_orders = UnitOrder::Await;
    ctx.unit.prior_state = UnitOrderState::ExecutingOrder;
    ctx.unit.orders = UnitOrder::Move;
    ctx.unit.state = UnitOrderState::Init;

    if let Some(parent) = ctx.unit.parent_unit {
        // `unit->GetParent()->SetParent(unit); unit->SetParent(nullptr);`
        if let Some(mut parent_unit) = ctx.other(parent) {
            parent_unit.parent_unit = Some(ctx.id);
            ctx.world.write_unit(parent, parent_unit);
        }
        ctx.unit.parent_unit = None;
    }
}

/// `TaskActivate::Activate` — the first passable cell around the container.
///
/// C++ (`taskactivate.cpp:106-167`): start at
/// `(container.grid_x - 1, container.grid_y + unit_size)`, where `unit_size` is
/// the **container** size (2 for a building, 1 for a transport), then four
/// directions (`N, E, S, W` — `direction += 2`) for `unit_size + 1` steps
/// cumulatively; the first cell passable for the **exiting** type
/// (`Access_IsAccessible(client->type, …, SameClassBlocks)`) becomes the target.
pub(crate) fn activation_exit_cell(
    state: &GameState,
    client: &UnitInfo,
    container: &UnitInfo,
) -> Option<Point> {
    // **Container** size: 2 for a 2×2 structure, 1 for 1×1 (including a turret —
    // `Footprint::Single`, although semantically it is a building).
    let unit_size: i16 = if container.is_quad() { 2 } else { 1 };

    let mut position = Point::new(
        container.grid.x.wrapping_sub(1),
        container.grid.y.wrapping_add(unit_size),
    );

    for direction in (0..8).step_by(2) {
        for _ in 0..=unit_size {
            let step = DIRECTION_OFFSETS[direction as usize];
            position = Point::new(
                position.x.wrapping_add(step.x),
                position.y.wrapping_add(step.y),
            );

            if position == client.grid {
                continue;
            }

            if access::is_accessible(
                state,
                client.unit_type,
                client.team,
                position,
                access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) > 0
            {
                return Some(position);
            }
        }
    }

    None
}

/// A builder leaves a completed site — `TaskActivate::Activate`
/// (`taskactivate.cpp:104-167`), the **AI** path.
///
/// In C++ for the player the exit order is born in the click handler
/// (`game_manager.cpp:6879-6890`): `move_to_grid` = the cell under the cursor and
/// `UnitsManager_SetNewOrder(selected, ORDER_ACTIVATE, ORDER_STATE_IN_TRANSITION)`;
/// the port sends the same by packet 08/P3 (`hud::build_builder_exit_order`).
/// The AI has no click: `TaskCreateBuilding::Activate` attaches `TaskActivate`, which
/// searches the first passable cell around the building (`builder_exit_cell`) and issues
/// the same order. `true` — the order was set.
pub(crate) fn issue_builder_exit(state: &mut GameState, builder_id: UnitId) -> bool {
    let Some(builder) = state.unit(builder_id).cloned() else {
        return false;
    };
    let Some(building_id) = production::builder_awaiting_exit(state, builder_id) else {
        return false;
    };
    let Some(building) = state.unit(building_id).cloned() else {
        return false;
    };
    let Some(exit) = production::builder_exit_cell(state, &builder, &building) else {
        return false;
    };

    let mut data = state.order_data(builder_id);
    data.move_to = exit;
    data.fire_on = Point::ZERO;
    state.set_order_data(builder_id, data);

    // `unit_to_activate->move_to_grid_x/y = position;`
    if let Some(mut unit) = state.unit(builder_id).cloned() {
        unit.target_grid = exit;
        state.write_unit(builder_id, unit);
    }

    // `UnitsManager_SetNewOrder(unit_to_activate.Get(), ORDER_ACTIVATE,
    //  ORDER_STATE_IN_TRANSITION);` — `prior` = `ORDER_BUILD`/`UNIT_READY`.
    state.set_order(
        builder_id,
        UnitOrder::Activate,
        UnitOrderState::InTransition,
    );

    true
}

/// A factory brings out a ready unit — `TaskCreateUnit::BeginTurn`
/// (`taskcreateunit.cpp:147-155`) via `TaskActivate::Activate`
/// (`taskactivate.cpp:135-144`), the **AI** path.
///
/// For the player the same order is born by clicking a cell at the factory
/// (`game_manager.cpp:6918-6926`, `CURSOR_ACTIVATE` branch) — the port sends it
/// by packet 08/P3 (`hud::build_activate_order`). The AI has no click: the task searches
/// the first passable cell around the factory ([`activation_exit_cell`]) and sets
/// **the factory** to `ORDER_ACTIVATE`/`ORDER_STATE_EXECUTING_ORDER` (at the factory
/// `parent_unit` is the ready unit, `UnitInfo::SpawnNewUnit`,
/// `unitinfo.cpp:3513`), after which `UnitsManager_ActivateUnit` brings the unit out.
///
/// `true` — the order was set.
pub(crate) fn issue_factory_exit(state: &mut GameState, factory_id: UnitId) -> bool {
    let Some(factory) = state.unit(factory_id).cloned() else {
        return false;
    };

    // `TaskActivate::Activate`: the order only for a factory in
    // `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` (`taskactivate.cpp:70`);
    // the ready unit is in `ORDER_IDLE` (`unitinfo.cpp:3499`).
    if factory.orders != UnitOrder::Build || factory.state != UnitOrderState::UnitReady {
        return false;
    }

    let Some(client_id) = factory.parent_unit else {
        return false;
    };
    let Some(client) = state.unit(client_id).cloned() else {
        return false;
    };

    if client.orders != UnitOrder::Idle {
        return false;
    }

    let Some(exit) = activation_exit_cell(state, &client, &factory) else {
        return false;
    };

    let mut data = state.order_data(factory_id);
    data.move_to = exit;
    state.set_order_data(factory_id, data);

    if let Some(mut unit) = state.unit(factory_id).cloned() {
        unit.target_grid = exit;
        state.write_unit(factory_id, unit);
    }

    // `unit_parent->SetParent(unit_to_activate)` — at the factory the parent is already the ready
    // unit (`SpawnNewUnit`), the link does not change.
    state.set_order(
        factory_id,
        UnitOrder::Activate,
        UnitOrderState::ExecutingOrder,
    );

    true
}

/// `UnitsManager_BuildingReady` (`units_manager.cpp:4183`) —
/// `UnitsManager_ProcessOrderIdle` for a completed building.
///
/// The building stands on the site in `ORDER_IDLE`/`BUILDING_READY` (and is not drawn)
/// until its builder parent leaves the building footprint. Then
/// the tape and cones are removed (the slab remains with `remove_slabs = false` as the
/// foundation), orders are restored, `scaler_adjust = 4`, and the building
/// unfolds via `ScaleUnit(EXPAND)` — exactly the C++ order.
fn building_ready(ctx: &mut OrderContext<'_>) {
    let parent = ctx.unit.parent_unit.and_then(|id| ctx.other(id));
    let is_found;

    match parent {
        Some(parent) if parent.orders != UnitOrder::Idle => {
            if parent.hits > 0 {
                if parent.orders == UnitOrder::Move
                    && parent.state != UnitOrderState::ExecutingOrder
                {
                    // The builder is still moving — the building waits.
                    is_found = false;
                } else {
                    is_found = !production::unit_bounds_contains(&ctx.unit, parent.grid);

                    // The builder returned into the 2×2 footprint (the path did not take it
                    // off the site): C++ `ReturnToConstructionSite` returns it
                    // to the center so that it leaves again.
                    if !is_found
                        && ctx.unit.is_quad()
                        && parent.orders == UnitOrder::Await
                        && parent.state == UnitOrderState::ExecutingOrder
                    {
                        return_to_construction_site(ctx, parent);
                        return;
                    }
                }
            } else {
                is_found = !production::unit_bounds_contains(&ctx.unit, parent.grid);

                if !is_found {
                    // The builder died on the site: the utilities and the building are removed
                    // (`Access_DestroyUtilities(grid, true, ...)`, then
                    // `UnitsManager_DestroyUnit`).
                    access::destroy_utilities(ctx.world, ctx.unit.grid, true, false, false, false);
                    ctx.world.destroy_unit(ctx.id);
                    return;
                }
            }
        }
        _ => {
            is_found = true;
        }
    }

    if !is_found {
        return;
    }

    if ctx.unit.unit_type != UnitType::Connector {
        let remove_road = ctx.unit.unit_type != UnitType::Road;

        access::destroy_utilities(ctx.world, ctx.unit.grid, false, false, true, remove_road);

        if ctx.unit.is_quad() {
            let other_cells = [
                Point::new(ctx.unit.grid.x.wrapping_add(1), ctx.unit.grid.y),
                Point::new(ctx.unit.grid.x, ctx.unit.grid.y.wrapping_add(1)),
                Point::new(
                    ctx.unit.grid.x.wrapping_add(1),
                    ctx.unit.grid.y.wrapping_add(1),
                ),
            ];

            for cell in other_cells {
                access::destroy_utilities(ctx.world, cell, false, false, true, remove_road);
            }
        }
    }

    ctx.restore_orders();
    ctx.unit.scaler_adjust = 4;
    ctx.unit.parent_unit = None;
    ctx.unit.target_grid = ctx.unit.grid;
    ctx.set_new_order(UnitOrder::AwaitScaling, UnitOrderState::Expand);

    // `Access_UpdateMapStatus(unit, true)` — coverage is recomputed by
    // `GameState::write_unit` on order/cell change; a separate call is not
    // required in the port.
}

/// `UnitInfo::ReturnToConstructionSite` (`unitinfo.cpp:4488`).
fn return_to_construction_site(ctx: &mut OrderContext<'_>, builder: UnitInfo) {
    // A builder without speed will not move: leave the building waiting
    // (otherwise the repeats "return to center → leave" spin without progress and do not
    // let the AI finish the turn).
    if builder.speed == 0 {
        return;
    }

    let building_grid = ctx.unit.grid;
    let mut builder_unit = builder;

    builder_unit.target_grid = Point::new(
        if builder_unit.grid.x == building_grid.x {
            building_grid.x.wrapping_add(1)
        } else {
            building_grid.x
        },
        if builder_unit.grid.y == building_grid.y {
            building_grid.y.wrapping_add(1)
        } else {
            building_grid.y
        },
    );

    builder_unit.parent_unit = Some(ctx.id);
    builder_unit.orders = UnitOrder::Build;
    builder_unit.state = UnitOrderState::UnitReady;
    ctx.world.write_unit(builder_unit.id, builder_unit.clone());

    let mut data = ctx.world.order_data(builder_unit.id);
    data.move_to = builder_unit.target_grid;
    ctx.world.set_order_data(builder_unit.id, data);

    // `UnitsManager_SetNewOrderInt(this, ORDER_AWAIT_TAPE_POSITIONING,
    // ORDER_STATE_TAPE_POSITIONING_DEINIT)`.
    ctx.world.set_order(
        builder_unit.id,
        UnitOrder::AwaitTapePositioning,
        UnitOrderState::TapePositioningDeinit,
    );
}

/// `UnitsManager_ProcessOrderNewAllocate` (`units_manager.cpp:2935`).
fn process_order_new_allocate(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "New allocation");
    ctx.restore_orders();
}

/// `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`).
fn process_order_power_on(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::Init {
        ai_task_flag(ctx.world, "Power up");
        power_up(&mut ctx.unit, -1);

        if ctx.unit.unit_type == UnitType::ResearchCentre {
            // `ResearchMenu_UpdateResearchProgress(team, topic, +1)`
            // (`units_manager.cpp:2956`).
            ctx.world
                .update_research_progress(ctx.unit.team, ctx.unit.research_topic, 1);
        }
    }

    animate(&ctx.unit);
}

/// `UnitsManager_ProcessOrderPowerOff` (`units_manager.cpp:2964`).
fn process_order_power_off(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::Init {
        power_down(ctx.world, &mut ctx.unit);

        if ctx.unit.unit_type == UnitType::ResearchCentre {
            // `UnitInfo::PowerDown` (`unitinfo.cpp:5134`).
            ctx.world
                .update_research_progress(ctx.unit.team, ctx.unit.research_topic, -1);
        }
    }

    animate(&ctx.unit);
}

/// `UnitsManager_ProcessOrderExplode` (`units_manager.cpp:2972`).
fn process_order_explode(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Exploding");

    match ctx.unit.state {
        UnitOrderState::Init => start_explosion(ctx),
        UnitOrderState::Destroy => progress_explosion(ctx),
        UnitOrderState::Explode => {
            ctx.unit.hits = 0;
            check_if_destroyed(&mut ctx.unit);
            start_explosion(ctx);
        }
        _ => {}
    }
}

/// `UnitsManager_StartExplosion` (`units_manager.cpp:3915`).
///
/// Spawns an effect unit (`HITEXPLD`/`BLDEXPLD`/`LNDEXPLD`/`AIREXPLD`/`SEAEXPLD`)
/// with class flags, as C++ (`base_unit.SetFlags(...)`).
fn start_explosion(ctx: &mut OrderContext<'_>) {
    refresh_screen(&ctx.unit);

    // `UnitsManager_StartExplosion` (`units_manager.cpp:3915-3930`):
    // `if (unit->hits > 0) PlaySfx(unit, SFX_TYPE_HIT);   // hit
    //  else                PlaySfx(unit, SFX_TYPE_EXPLOAD); // explosion`
    // The sound belongs to the **unit itself** (its `SC_UNITS` table), so in the
    // event — the target id; resource/volume is chosen by the client (`max_app::sfx`).
    ctx.world.unit_sounds.push(crate::game::UnitSound {
        unit: ctx.id,
        unit_type: ctx.unit.unit_type,
        kind: if ctx.unit.hits > 0 {
            crate::game::UnitSoundKind::Hit
        } else {
            crate::game::UnitSoundKind::Explode
        },
    });

    if (ctx.unit.unit_type == UnitType::Infiltrator || ctx.unit.unit_type == UnitType::Infantry)
        && ctx.unit.hits == 0
    {
        // `units_manager.cpp:3932-3946`: infantry/saboteur die with their own
        // 8-frame animation `image_base = 168 + (angle / 2) * 8`.
        let image_base = 168 + (i16::from(ctx.unit.angle) / 2) * 8;
        ctx.unit.image_base = image_base;
        ctx.unit.image_index = image_base;
        ctx.unit.image_index_max = image_base + 7;
        ctx.unit.state = UnitOrderState::Destroy;
        ctx.unit.moved = 0;
        ctx.unit.parent_unit = None;
        return;
    }

    let unit_flags = ctx.unit.flags;
    let explosion_type = if ctx.unit.hits > 0 {
        UnitType::HitExplosion
    } else if ctx.unit.is_quad() {
        // Layout footprint (`Footprint::Quad`), not `is_building()`: for turrets
        // (`Building` + `Single`) the explosion is ground (`LandExplosion`), as in C++.
        UnitType::BuildingExplosion
    } else if ctx.unit.is_ground() || ctx.unit.is_amphibious() {
        UnitType::LandExplosion
    } else if ctx.unit.is_air() {
        UnitType::AirExplosion
    } else if ctx.unit.is_sea() || ctx.unit.is_amphibious() {
        UnitType::SeaExplosion
    } else {
        UnitType::LandExplosion
    };

    // The explosion effect's own flags are part of serializable state (`SC_UNITS`
    // of the explosion): the mask is built from the dead unit's bits, not classified.
    let explosion_flags = if unit_flags & GROUND_COVER != 0 {
        MOBILE_LAND_UNIT
    } else if unit_flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT) != 0 {
        STATIONARY
    } else if unit_flags & STATIONARY != 0 {
        MOBILE_AIR_UNIT
    } else {
        MISSILE_UNIT
    };

    let explosion = ctx.world.spawn_unit(
        explosion_type,
        ctx.unit.team,
        ctx.unit.grid,
        explosion_flags,
    );

    // The explosion is a unit of the dead unit's team, but those who saw
    // the target itself must see it (`UnitInfo::InitStealthStatus` leaves the effect visible only
    // to its own team, and `Access_UpdateMapStatus` is skipped for it in C++:
    // `DeployUnit(..., skip_map_status_update = true)`). We transfer the target's
    // visibility to the effect — otherwise the hit animation is not drawn for the attacker.
    if let Some(explosion_id) = explosion {
        let visible = ctx.unit.visible_to_team;
        if let Some(mut effect) = ctx.world.unit(explosion_id).cloned() {
            effect.visible_to_team = visible;
            ctx.world.write_unit(explosion_id, effect);
        }
    }

    ctx.unit.orders = ctx.unit.prior_orders;
    ctx.unit.state = ctx.unit.prior_state;
    restore_orders(&mut ctx.unit);

    if ctx.unit.hits == 0 {
        if let Some(explosion_id) = explosion {
            if let Some(mut explosion_unit) = ctx.world.unit(explosion_id).cloned() {
                explosion_unit.parent_unit = Some(ctx.id);
                ctx.world.write_unit(explosion_id, explosion_unit);
            }
        }
        ctx.unit.state = UnitOrderState::Destroy;
    }
}

/// `UnitsManager_ProgressExplosion` (`units_manager.cpp:4006`).
fn progress_explosion(ctx: &mut OrderContext<'_>) {
    if ctx.unit.image_index == ctx.unit.image_index_max {
        ctx.world.destroy_unit(ctx.id);
    } else {
        // `units_manager.cpp:4015-4023`: in the middle of the animation the explosion releases
        // the parent (the dead unit); if its `hits == 0`, it is deleted by
        // `UnitsManager_BuildClearing(parent, true)`. Without this the dead unit
        // would remain in `ORDER_STATE_DESTROY` forever, and `UnitsManager_ProcessOrder`
        // would set `OrdersPending` every tick — the AI turn would not close.
        //
        // `BuildClearing` (not `DestroyUnit`) — this is exactly where C++ leaves
        // rubble: `SMLRUBLE` for a unit and `LRGRUBLE` for a building
        // (`access::build_clearing`).
        if let Some(parent_id) = ctx.unit.parent_unit {
            if ctx.unit.image_index == ctx.unit.image_index_max / 2 {
                ctx.unit.parent_unit = None;

                if ctx
                    .world
                    .unit(parent_id)
                    .is_some_and(|parent| parent.hits == 0)
                {
                    access::build_clearing(ctx.world, parent_id, true);
                }
            }
        }

        if ctx.unit.moved & 0x01 != 0 {
            ctx.unit.image_index = ctx.unit.image_index.wrapping_add(1);
        }
        ctx.unit.moved = ctx.unit.moved.wrapping_add(1);
    }
}

/// `UnitsManager_ProcessOrderUnload` (`units_manager.cpp:2994`).
fn process_order_unload(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Unloading");

    match ctx.unit.state {
        UnitOrderState::Init => {
            ctx.unit.moved = 0;
            ctx.unit.state = UnitOrderState::UnloadingInProgress;

            if access::is_unload_site_taken(ctx.world, &ctx.unit)
                || access::paths_is_occupied(
                    ctx.world,
                    ctx.unit.grid,
                    i32::from(ctx.unit.angle),
                    ctx.unit.team,
                )
            {
                ctx.unit.orders = UnitOrder::Await;
                ctx.unit.state = UnitOrderState::ExecutingOrder;
            } else {
                progress_unloading(ctx);
            }
        }
        UnitOrderState::UnloadingInProgress => progress_unloading(ctx),
        UnitOrderState::FinishUnloading => process_unloading(ctx),
        _ => {}
    }
}

/// `UnitsManager_ProgressUnloading` (`units_manager.cpp:4035`).
fn progress_unloading(ctx: &mut OrderContext<'_>) {
    let landed = land(&mut ctx.unit);
    if !landed {
        return;
    }

    let Some(parent_id) = ctx.unit.parent_unit else {
        return;
    };
    let Some(mut client) = ctx.other(parent_id) else {
        return;
    };

    // `Access_TriggerEnemyMine` (`units_manager.cpp:4035`): a mine under
    // a landed unit explodes.
    let landing_grid = client.grid;
    let landing_team = client.team;
    access::trigger_enemy_mine(ctx.world, landing_team, landing_grid);

    restore_orders(&mut client);
    ctx.world.write_unit(parent_id, client);

    // `Access_UpdateMapStatus` — P5 (visibility map); the cell is already in MapHash.

    ctx.scale_other(parent_id, UnitOrderState::Expand);

    ctx.unit.storage = ctx.unit.storage.saturating_sub(1);
    ctx.unit.moved = 0;
    ctx.unit.state = UnitOrderState::FinishUnloading;
}

/// `UnitInfo::ProcessUnloading` (`unitinfo.cpp:5184`).
fn process_unloading(ctx: &mut OrderContext<'_>) {
    if take(&mut ctx.unit) {
        ctx.unit.orders = UnitOrder::Await;
        ctx.unit.state = UnitOrderState::ExecutingOrder;
        ctx.unit.parent_unit = None;
    }
}

/// `UnitsManager_ProcessOrderClear` (`units_manager.cpp:3021`).
fn process_order_clear(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::Init {
        ai_task_flag(ctx.world, "Clear start");
        // `UnitsManager_StartClearing`: spawning cones/tape, `ClearUnitList`,
        // state = IN_PROGRESS, for LRGTAPE — AWAIT_TAPE_POSITIONING.
        // TODO(P3/P5): DeployUnit(LRGCONES/LRGTAPE/SMLCONES/SMLTAPE).
        ctx.unit.state = UnitOrderState::InProgress;

        // The large tape (`LRGTAPE`) — only for a 2×2 parent (footprint, not
        // semantics: a turret has `SMLTAPE`).
        let parent_is_quad = ctx.parent().map(|parent| parent.is_quad()).unwrap_or(false);

        if parent_is_quad {
            ctx.set_new_order(
                UnitOrder::AwaitTapePositioning,
                UnitOrderState::TapePositioningInit,
            );
        }
    }
}

/// `UnitsManager_ProcessOrderSentry` (`units_manager.cpp:3029`).
fn process_order_sentry(ctx: &mut OrderContext<'_>) {
    if ctx.unit.flags & ANIMATED != 0 && ctx.world.settings_effects {
        if ctx.unit.image_index + 1 <= ctx.unit.image_index_max {
            ctx.unit.image_index = ctx.unit.image_index.wrapping_add(1);
        } else {
            ctx.unit.image_index = ctx.unit.image_base;
        }
    } else {
        animate(&ctx.unit);
    }
}

/// `UnitsManager_ProcessOrderLand` (`units_manager.cpp:3043`).
fn process_order_land(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Landing");

    // An occupied aircraft does not descend into the hangar; the landing site keeps it
    // on the map.
    if ctx.unit.parent_unit.is_some() && access::is_engaged(ctx.world, &ctx.unit) {
        return;
    }

    match ctx.unit.state {
        UnitOrderState::Init => {
            ctx.unit.moved = 0;
            ctx.unit.state = UnitOrderState::FinishLanding;
            process_landing(ctx);
        }
        UnitOrderState::FinishLanding => process_landing(ctx),
        _ => {}
    }
}

/// `UnitInfo::ProcessLanding` (`unitinfo.cpp:5139`).
fn process_landing(ctx: &mut OrderContext<'_>) {
    if land(&mut ctx.unit) {
        if ctx.unit.parent_unit.is_some() {
            ctx.unit.orders = UnitOrder::Idle;
            ctx.unit.state = UnitOrderState::Store;
            ctx.scale(UnitOrderState::Shrink);
        } else {
            let id = ctx.id;
            // `UnitsManager_RemoveUnitFromUnitLists(this)` + `AddToDrawList()`.
            remove_from_list(ctx.world, id, ctx.unit.unit_list);
            ctx.world.add_to_draw_list(id);

            ctx.unit.orders = UnitOrder::Await;
            ctx.unit.state = UnitOrderState::ExecutingOrder;
        }
    }
}

/// `UnitsManager_ProcessOrderTakeOff` (`units_manager.cpp:3065`).
fn process_order_take_off(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Taking off");

    match ctx.unit.state {
        UnitOrderState::Init => {
            ctx.unit.moved = 0;
            ctx.unit.state = UnitOrderState::FinishTakeOff;

            if take(&mut ctx.unit) {
                let id = ctx.id;
                remove_from_list(ctx.world, id, ctx.unit.unit_list);
                ctx.world.add_to_draw_list(id);
                ctx.restore_orders();
            }
        }
        UnitOrderState::FinishTakeOff => {
            if take(&mut ctx.unit) {
                let id = ctx.id;
                remove_from_list(ctx.world, id, ctx.unit.unit_list);
                ctx.world.add_to_draw_list(id);
                ctx.restore_orders();
            }
        }
        _ => {}
    }
}

/// `UnitsManager_ProcessOrderLoad` (`units_manager.cpp:3092`).
fn process_order_load(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Loading");

    match ctx.unit.state {
        UnitOrderState::Init => {
            // An occupied passenger is not picked up until the attack/stealth action ends.
            let parent_engaged = ctx
                .parent()
                .map(|parent| access::is_engaged(ctx.world, &parent))
                .unwrap_or(false);

            if !parent_engaged {
                ctx.unit.moved = 0;
                ctx.unit.state = UnitOrderState::LoadingInProgress;

                if let Some(parent) = ctx.unit.parent_unit {
                    ctx.scale_other(parent, UnitOrderState::Shrink);
                }

                progress_loading(ctx);
            }
        }
        UnitOrderState::LoadingInProgress => progress_loading(ctx),
        UnitOrderState::FinishLoading => process_loading(ctx),
        _ => {}
    }
}

/// `UnitsManager_ProgressLoading` (`units_manager.cpp:4131`).
fn progress_loading(ctx: &mut OrderContext<'_>) {
    let parent_engaged = ctx
        .parent()
        .map(|parent| access::is_engaged(ctx.world, &parent))
        .unwrap_or(false);
    if parent_engaged {
        return;
    }

    if !land(&mut ctx.unit) {
        return;
    }

    if let Some(parent_id) = ctx.unit.parent_unit {
        if let Some(mut parent) = ctx.other(parent_id) {
            if parent.hits > 0
                && parent.orders != UnitOrder::Fire
                && parent.orders != UnitOrder::Explode
                && parent.state != UnitOrderState::Destroy
            {
                parent.path = None;
                parent.parent_unit = Some(ctx.id);

                if parent.orders != UnitOrder::Disable {
                    parent.orders = UnitOrder::Await;
                    parent.state = UnitOrderState::ExecutingOrder;
                }

                ctx.world.write_unit(parent_id, parent);

                // `UnitsManager_SetNewOrderInt(&*parent, ORDER_IDLE,
                // ORDER_STATE_PREPARE_STORE)`.
                ctx.world
                    .set_order(parent_id, UnitOrder::Idle, UnitOrderState::PrepareStore);

                ctx.unit.storage = ctx.unit.storage.wrapping_add(1);
            }
        }
    }

    ctx.unit.moved = 0;
    ctx.unit.state = UnitOrderState::FinishLoading;
}

/// `UnitInfo::ProcessLoading` (`unitinfo.cpp:5161`).
fn process_loading(ctx: &mut OrderContext<'_>) {
    if take(&mut ctx.unit) {
        ctx.unit.orders = UnitOrder::Await;
        ctx.unit.state = UnitOrderState::ExecutingOrder;
        ctx.unit.parent_unit = None;
    }
}

/// `UnitsManager_ProcessOrderIdle` (`units_manager.cpp:3118`).
fn process_order_idle(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::BuildingReady {
        building_ready(ctx);
    }
}

/// `UnitsManager_ProcessOrderRepair` (`units_manager.cpp:3124`) +
/// `UnitInfo::ProcessRepair` (`unitinfo.cpp:5258`).
fn process_order_repair(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Repairing");

    if ctx.unit.complex.is_some() {
        // TODO(P3): repair for complex materials (`Complex::GetCargoInfo`,
        // `UnitInfo::Repair`).
    } else if let Some(parent_id) = ctx.unit.parent_unit {
        // TODO(P3): `parent->Repair(storage)` and charging `storage`.
        schedule_delayed_tasks(ctx.world, parent_id, true);
    }

    ctx.restore_orders();
}

/// `UnitsManager_ProcessOrderReload` (`units_manager.cpp:3130`) +
/// `UnitInfo::Reload` (`unitinfo.cpp:4301`).
fn process_order_reload(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Reloading");

    let mut need_action = false;

    if let Some(complex) = ctx.unit.complex {
        let (materials, _) = crate::alloc::get_cargo_info(ctx.world, ctx.unit.team, complex);

        if materials.raw > 0 {
            need_action = true;
            crate::transfer::complex_transfer(ctx.world, ctx.unit.team, complex, -1, 0, 0);
        }
    } else if ctx.unit.storage > 0 {
        need_action = true;
        ctx.unit.storage -= 1;
    }

    if need_action {
        if let Some(parent_id) = ctx.unit.parent_unit {
            if let Some(mut parent) = ctx.other(parent_id) {
                parent.ammo = parent.base_values.get_attribute(ATTRIB_AMMO) as u8;
                ctx.world.write_unit(parent_id, parent);
            }
        }
    }

    ctx.restore_orders();
}

/// `UnitsManager_ProcessOrderTransfer` (`units_manager.cpp:3136`) +
/// `UnitInfo::ProcessTransfer` (`unitinfo.cpp:5294`).
fn process_order_transfer(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Tranferring");

    // Transfer of materials/fuel/gold between the unit and the parent: the sign of
    // `transfer_cargo` sets the direction, `ProcessTransfer` itself calls
    // `RestoreOrders`. `ctx.commit()` at the end writes the `ctx.unit` snapshot, so
    // we re-read the unit after the processor — otherwise the `storage`/order changes
    // would be overwritten by the stale copy.
    crate::transfer::process_transfer(ctx.world, ctx.id);

    if let Some(unit) = ctx.world.unit(ctx.id) {
        ctx.unit = unit.clone();
    }
}

/// `UnitsManager_ProcessOrderHaltBuilding` (`units_manager.cpp:3142`).
fn process_order_halt_building(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::BuildCancel {
        ai_task_flag(ctx.world, "Build halting");
        cancel_building(&mut ctx.unit);
        // See `ProcessOrderBuild`: the port does not move the builder onto the site,
        // so utilities are removed from `target_grid`, not from `grid`;
        // the flag set — as in `CancelBuilding` (`true, false, false, false`).
        access::destroy_utilities(ctx.world, ctx.unit.target_grid, true, false, false, false);
    }
}

/// `UnitsManager_ProcessOrderAwaitScaling` (`units_manager.cpp:3150`).
fn process_order_await_scaling(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Scaling");
    refresh_screen(&ctx.unit);

    if ctx.unit.prior_orders == UnitOrder::AwaitDisableUnit
        || ctx.unit.prior_orders == UnitOrder::AwaitStealUnit
    {
        ctx.world.combat_effects_active = true;
    }

    match ctx.unit.state {
        UnitOrderState::Expand => {
            if ctx.unit.scaler_adjust != 0 {
                ctx.unit.scaler_adjust -= 1;
            } else {
                ctx.restore_orders();
                // `UnitsManager_UpdateConnectors(unit)` (`units_manager.cpp:3166`):
                // the building took the final order and becomes a complex
                // member (merging via connectors).
                ctx.sync_to_world();
                crate::complex::update_connectors(ctx.world, ctx.id);
                ctx.sync_from_world();
                schedule_delayed_tasks(ctx.world, ctx.id, true);
            }
        }
        UnitOrderState::Shrink => {
            if ctx.unit.scaler_adjust == 4 {
                finish_unit_scaling_in(ctx.world, &mut ctx.unit);
            } else {
                ctx.unit.scaler_adjust = ctx.unit.scaler_adjust.wrapping_add(1);
            }
        }
        _ => {}
    }
}

/// `UnitsManager_ProcessOrderAwaitTapePositioning` (`units_manager.cpp:3187`).
fn process_order_await_tape_positioning(ctx: &mut OrderContext<'_>) {
    let move_to = ctx.data.move_to;
    position_in_tape(ctx.world, &mut ctx.unit, move_to);
}

/// `UnitsManager_ProcessOrderAwaitDisableStealUnit`
/// (`units_manager.cpp:3189`).
fn process_order_await_disable_steal_unit(ctx: &mut OrderContext<'_>) {
    ctx.world.combat_effects_active = true;
    ai_task_flag(ctx.world, "Disable/Steal");

    match ctx.unit.state {
        UnitOrderState::Init => {
            ctx.unit.state = UnitOrderState::InProgress;
            ctx.scale(UnitOrderState::Shrink);
        }
        UnitOrderState::ExecutingOrder => {
            if combat::attempt_stealth_action(ctx.world, &mut ctx.unit) {
                if ctx.unit.orders == UnitOrder::AwaitStealUnit {
                    if let Some(parent) = ctx.unit.parent_unit {
                        combat::capture_unit(ctx.world, ctx.id, parent);
                    }
                } else if let Some(parent) = ctx.unit.parent_unit {
                    combat::disable_unit(ctx.world, ctx.id, parent);
                }
            }

            ctx.unit.orders = UnitOrder::Await;
        }
        UnitOrderState::InProgress => {
            if let Some(parent_id) = ctx.unit.parent_unit {
                // `UnitInfo::ShakeSabotage` (`unitinfo.cpp:4680`): the shake
                // counter 0..15; the full cycle returns the target to its initial
                // state, then the saboteur "expands".
                if let Some(mut parent) = ctx.other(parent_id) {
                    parent.shake_effect_state = parent.shake_effect_state.wrapping_add(1) & 0x0F;
                    ctx.world.write_unit(parent_id, parent);
                }

                let parent_shaking = ctx
                    .other(parent_id)
                    .map(|parent| parent.shake_effect_state != 0)
                    .unwrap_or(false);

                if !parent_shaking {
                    ctx.unit.state = UnitOrderState::ExecutingOrder;
                    ctx.scale(UnitOrderState::Expand);
                }
            } else {
                ctx.unit.state = UnitOrderState::ExecutingOrder;
            }
        }
        _ => {}
    }
}

/// `UnitsManager_AttemptStealthAction` — a thin wrapper over
/// [`combat::attempt_stealth_action`] (see the call in the order processor).
#[allow(dead_code)]
fn attempt_stealth_action(ctx: &mut OrderContext<'_>) -> bool {
    combat::attempt_stealth_action(ctx.world, &mut ctx.unit)
}

/// `UnitsManager_ProcessOrderDisable` (`units_manager.cpp:3226`).
fn process_order_disable(ctx: &mut OrderContext<'_>) {
    // `unit->Animate()` — UI; the state does not change while disable turns
    // are passing (`UpdateProduction` decrements `disabled_turns_remaining`).
    animate(&ctx.unit);
}

/// `UnitsManager_ProcessOrderUpgrade` (`units_manager.cpp:3228`) +
/// `UnitInfo::Upgrade` (`unitinfo.cpp:4339`).
fn process_order_upgrade(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "Upgrading");

    // TODO(P3): attribute version and complex materials check
    // (`GetUpgradeCost`, `UpgradeInt`).
    ctx.restore_orders();
    ctx.unit.parent_unit = None;
}

/// `UnitsManager_ProcessOrderLayMine` (`units_manager.cpp:3234`).
fn process_order_lay_mine(ctx: &mut OrderContext<'_>) {
    ai_task_flag(ctx.world, "lay mines");

    match ctx.unit.state {
        UnitOrderState::Inactive => {
            ctx.unit.laying_state = 0;
        }
        UnitOrderState::PlacingMines => {
            ctx.unit.laying_state = 2;
            place_mine(ctx.world, &mut ctx.unit);
        }
        UnitOrderState::RemovingMines => {
            ctx.unit.laying_state = 1;
            pick_up_mine(ctx.world, &mut ctx.unit);
        }
        _ => {}
    }

    ctx.restore_orders();
    schedule_delayed_tasks(ctx.world, ctx.id, true);
}

/// `UnitInfo::PlaceMine` (`unitinfo.cpp:4500`).
fn place_mine(state: &mut GameState, unit: &mut UnitInfo) {
    if (unit.unit_type != UnitType::MineLayer && unit.unit_type != UnitType::SeaMineLayer)
        || unit.storage <= 0
    {
        return;
    }

    let mine_type = if unit.unit_type == UnitType::SeaMineLayer {
        UnitType::SeaMine
    } else {
        UnitType::LandMine
    };

    if state
        .units_at(unit.grid)
        .iter()
        .filter_map(|id| state.unit(*id))
        .any(|other| other.unit_type == mine_type)
    {
        return;
    }

    let flags = access::unit_type_flags(mine_type);
    if let Some(mine) = state.spawn_unit(mine_type, unit.team, unit.grid, flags) {
        // The mine is visible to all surveyor-unit teams in 3×3 (`SpotByTeam`).
        let mut surveyor_teams = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                let grid = Point::new(unit.grid.x + dx, unit.grid.y + dy);
                for id in state.units_at(grid) {
                    if let Some(other) = state.unit(*id) {
                        if other.unit_type == UnitType::Surveyor
                            && !surveyor_teams.contains(&other.team)
                        {
                            surveyor_teams.push(other.team);
                        }
                    }
                }
            }
        }
        for team in surveyor_teams {
            combat::spot_unit(state, mine, team);
        }
    }

    unit.storage = unit.storage.wrapping_sub(1);
    if unit.storage == 0 {
        unit.laying_state = 0;
    }
}

/// `UnitInfo::PickUpMine` (`unitinfo.cpp:4559`).
fn pick_up_mine(state: &mut GameState, unit: &mut UnitInfo) {
    if unit.unit_type != UnitType::MineLayer && unit.unit_type != UnitType::SeaMineLayer {
        return;
    }

    let storage_limit = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_STORAGE);
    if i32::from(unit.storage) >= storage_limit {
        return;
    }

    let mine_type = if unit.unit_type == UnitType::SeaMineLayer {
        UnitType::SeaMine
    } else {
        UnitType::LandMine
    };

    let mine = state.units_at(unit.grid).iter().copied().find(|id| {
        state
            .unit(*id)
            .map(|other| other.unit_type == mine_type)
            .unwrap_or(false)
    });

    if let Some(mine) = mine {
        state.destroy_unit(mine);
        unit.storage = unit.storage.wrapping_add(1);
        if i32::from(unit.storage) == storage_limit {
            unit.laying_state = 0;
        }
    }
}

/// `UnitsManager_ProcessOrder` (`units_manager.cpp:3257`) — dispatcher of 32
/// orders.
///
/// The mappings are preserved verbatim, including `ORDER_MOVE_TO_UNIT`/`_TO_ATTACK` →
/// `ProcessOrderMove` and `ORDER_HALT_BUILDING_2` → `ProcessOrderHaltBuilding`.
/// `ORDER_17` (0x12) has no `switch` branch in C++ — also unhandled.
///
/// `ORDER_STATE_10` (0xA) and `ORDER_STATE_21` (0x15) are declared in
/// `enums.hpp`, but the C++ port never uses them, so there are no branches for them
/// (a full match, not an omission).
pub(crate) fn process_order(state: &mut GameState, id: UnitId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };
    let data = state.order_data(id);
    let mut ctx = OrderContext {
        world: state,
        id,
        unit,
        data,
    };

    // Header of `UnitsManager_ProcessOrder`.
    if ctx.unit.shake_effect_state != 0 && !access::is_engaged_by_infiltrator(ctx.world, &ctx.unit)
    {
        // The infiltrator that shook the unit died or left stealth.
        ctx.unit.shake_effect_state = 0;
    }

    if ctx.unit.flags & SPINNING_TURRET != 0 && ctx.unit.orders != UnitOrder::Disable {
        // `SpinningTurretAdvanceAnimation` — UI.
    }

    if ctx.unit.flags & MISSILE_UNIT != 0 || ctx.unit.state == UnitOrderState::Destroy {
        ctx.world.orders_pending = true;
        ctx.world.combat_effects_active = true;
    }

    match ctx.unit.orders {
        UnitOrder::Await => process_order_await(&mut ctx),
        UnitOrder::Transform => process_order_transform(&mut ctx),
        UnitOrder::Move => process_order_move(&mut ctx),
        UnitOrder::Fire => process_order_fire(&mut ctx),
        UnitOrder::Build => process_order_build(&mut ctx),
        UnitOrder::Activate => process_order_activate(&mut ctx),
        UnitOrder::NewAllocate => process_order_new_allocate(&mut ctx),
        UnitOrder::PowerOn => process_order_power_on(&mut ctx),
        UnitOrder::PowerOff => process_order_power_off(&mut ctx),
        UnitOrder::Explode => process_order_explode(&mut ctx),
        UnitOrder::Unload => process_order_unload(&mut ctx),
        UnitOrder::Clear => process_order_clear(&mut ctx),
        UnitOrder::Sentry => process_order_sentry(&mut ctx),
        UnitOrder::Land => process_order_land(&mut ctx),
        UnitOrder::TakeOff => process_order_take_off(&mut ctx),
        UnitOrder::Load => process_order_load(&mut ctx),
        UnitOrder::Idle => process_order_idle(&mut ctx),
        UnitOrder::Repair => process_order_repair(&mut ctx),
        // `ORDER_17`: no branch in the C++ port (unused slot).
        UnitOrder::Order17 => {}
        UnitOrder::Reload => process_order_reload(&mut ctx),
        UnitOrder::Transfer => process_order_transfer(&mut ctx),
        UnitOrder::HaltBuilding => process_order_halt_building(&mut ctx),
        UnitOrder::AwaitScaling => process_order_await_scaling(&mut ctx),
        UnitOrder::AwaitTapePositioning => process_order_await_tape_positioning(&mut ctx),
        UnitOrder::AwaitStealUnit | UnitOrder::AwaitDisableUnit => {
            process_order_await_disable_steal_unit(&mut ctx)
        }
        UnitOrder::Disable => process_order_disable(&mut ctx),
        UnitOrder::MoveToUnit => process_order_move(&mut ctx),
        UnitOrder::Upgrade => process_order_upgrade(&mut ctx),
        UnitOrder::LayMine => process_order_lay_mine(&mut ctx),
        UnitOrder::MoveToAttack => process_order_move(&mut ctx),
        UnitOrder::HaltBuilding2 => process_order_halt_building(&mut ctx),
    }

    ctx.commit();
}

/// `UnitsManager_ProcessUnitOrders` (`units_manager.cpp:3401`) — a walk over one
/// list; extracted into [`GameState::process_unit_orders`].
pub(crate) fn process_unit_orders(state: &mut GameState, kind: UnitListKind) {
    state.process_unit_orders(kind);
}

/// `UnitsManager_ProcessOrders` (`units_manager.cpp:1860`) — extracted into
/// [`GameState::process_orders`].
pub(crate) fn process_orders(state: &mut GameState) {
    state.process_orders();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::TeamClan;
    use crate::team::TeamId;
    use crate::team::TeamKind;

    fn state_with_tank() -> (GameState, UnitId) {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        // Tank speed from the attribute table — so path steps are possible.
        let id = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_LAND_UNIT | SELECTABLE | UPGRADABLE,
            )
            .expect("spawn");
        (state, id)
    }

    fn order_move_to(state: &mut GameState, id: UnitId, target: Point) {
        state.set_order_data(
            id,
            OrderData {
                move_to: target,
                fire_on: Point::ZERO,
            },
        );
        state.set_order(id, UnitOrder::Move, UnitOrderState::Init);
    }

    #[test]
    fn straight_line_move_reaches_target_and_restores_await() {
        let (mut state, id) = state_with_tank();
        order_move_to(&mut state, id, Point::new(14, 10));

        let mut ticks = 0;
        loop {
            state.tick();
            ticks += 1;
            let unit = state.unit(id).expect("unit");
            if unit.grid == Point::new(14, 10) && !unit.has_move_order() {
                break;
            }
            assert!(ticks < 64, "юнит не дошёл до цели за 64 тика");
        }

        let unit = state.unit(id).expect("unit");
        assert_eq!(unit.grid, Point::new(14, 10));
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert!(unit.path.is_none(), "путь снимается на завершении");
        // The tank spends 1 speed unit per cell: 4 cells → speed-4.
        assert_eq!(
            unit.speed,
            unit.base_values.get_attribute(ATTRIB_SPEED) as u16 - 4
        );
    }

    #[test]
    fn move_order_is_replaced_by_new_order() {
        let (mut state, id) = state_with_tank();
        order_move_to(&mut state, id, Point::new(30, 10));
        // The first tick places a path request (`NEW_ORDER`), the second applies it
        // (`poll_results` → `IN_PROGRESS`) — as in C++ with `PathsManager`.
        state.tick();
        state.tick();
        assert_eq!(state.unit(id).unwrap().state, UnitOrderState::InProgress);

        // Order change during movement: prior records Move/InProgress.
        state.set_order(id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
        let unit = state.unit(id).expect("unit");
        assert_eq!(unit.orders, UnitOrder::Sentry);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert_eq!(unit.prior_orders, UnitOrder::Move);

        // SENTRY does not move the unit.
        let before = state.unit(id).unwrap().grid;
        for _ in 0..4 {
            state.tick();
        }
        assert_eq!(state.unit(id).unwrap().grid, before);
    }

    #[test]
    fn set_new_order_int_keeps_prior_and_explode_override() {
        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::InProgress;

        assert!(!set_new_order_int(
            &mut unit,
            UnitOrder::Fire,
            UnitOrderState::Init
        ));
        assert_eq!(unit.orders, UnitOrder::Fire);
        assert_eq!(unit.prior_orders, UnitOrder::Move);
        assert_eq!(unit.prior_state, UnitOrderState::InProgress);

        // Explosion overrides fire and does not preserve prior.
        assert!(!set_new_order_int(
            &mut unit,
            UnitOrder::Explode,
            UnitOrderState::Init
        ));
        assert_eq!(unit.orders, UnitOrder::Explode);

        // From NEW_ORDER the order returns to AWAIT and asks to remove the path.
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::NewOrder;
        assert!(set_new_order_int(
            &mut unit,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder
        ));
        assert_eq!(unit.prior_orders, UnitOrder::Await);
    }

    #[test]
    fn restore_orders_matches_cpp_building_exception() {
        let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        tank.orders = UnitOrder::Move;
        tank.state = UnitOrderState::InProgress;
        tank.prior_orders = UnitOrder::Move;
        tank.prior_state = UnitOrderState::InProgress;
        restore_orders(&mut tank);
        assert_eq!(tank.orders, UnitOrder::Await);
        assert_eq!(tank.state, UnitOrderState::ExecutingOrder);

        // For powered buildings the prior/order match is preserved (as in C++).
        let mut tower = UnitInfo::new(UnitType::GoldRefinery, TeamId::RED, UnitId(2));
        tower.orders = UnitOrder::PowerOn;
        tower.state = UnitOrderState::Init;
        tower.prior_orders = UnitOrder::PowerOn;
        tower.prior_state = UnitOrderState::Init;
        restore_orders(&mut tower);
        assert_eq!(tower.orders, UnitOrder::PowerOn);
        assert_eq!(tower.state, UnitOrderState::Init);
    }

    #[test]
    fn all_32_orders_dispatch_without_panic() {
        let (mut state, id) = state_with_tank();
        for order in UnitOrder::ALL {
            state.set_order_data(
                id,
                OrderData {
                    move_to: Point::new(12, 10),
                    fire_on: Point::new(12, 10),
                },
            );
            state.set_order(id, *order, UnitOrderState::Init);
            state.tick();
        }
        assert!(state.has_unit(id));
    }

    #[test]
    fn path_blocked_without_speed_keeps_order_until_new_turn() {
        let (mut state, id) = state_with_tank();
        order_move_to(&mut state, id, Point::new(14, 10));
        // Zero the speed: the path step does not start, `MoveFinished` resets
        // movement, but the order stays MOVE (as in `GroundPath::Execute` with
        // `speed == 0`: state EXECUTING_ORDER, the path is kept until
        // speed is restored at the start of the turn).
        state.unit_mut(id).unwrap().speed = 0;
        // Tick 1 — path request; tick 2 — path applied and `GroundPath::Execute`
        // with `speed == 0` leaves the order MOVE with state EXECUTING_ORDER.
        state.tick();
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.orders, UnitOrder::Move);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert!(unit.path.is_some());
        // `MoveFinished` set `delayed_reaction`, but `AssessAttacks` at
        // the end of the same tick (`ProcessOrders`) cleared it: the team has no
        // delayed targets (`UnitsManager_ClearDelayedReactions`).
        assert_eq!(unit.delayed_reaction, 0);
        assert_eq!(unit.moved, 0);

        // The start of the turn restores speed (`UpdateProduction`) — movement
        // continues from the same place.
        let base_speed = state
            .unit(id)
            .unwrap()
            .base_values
            .get_attribute(ATTRIB_SPEED) as u16;
        state.unit_mut(id).unwrap().speed = base_speed;
        state.set_order(id, UnitOrder::Move, UnitOrderState::Init);
        let mut ticks = 0;
        loop {
            state.tick();
            ticks += 1;
            assert!(ticks < 64, "юнит не дошёл до цели после сброса скорости");
            let unit = state.unit(id).unwrap();
            if unit.grid == Point::new(14, 10) && !unit.has_move_order() {
                break;
            }
        }
        assert_eq!(state.unit(id).unwrap().orders, UnitOrder::Await);
    }

    #[test]
    fn air_move_reaches_target_cell() {
        use crate::enums::unit_flags::MOBILE_AIR_UNIT;
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        let id = state
            .spawn_unit(
                UnitType::Fighter,
                TeamId::RED,
                Point::new(5, 5),
                MOBILE_AIR_UNIT,
            )
            .expect("spawn");
        order_move_to(&mut state, id, Point::new(10, 10));

        let mut ticks = 0;
        loop {
            state.tick();
            ticks += 1;
            assert!(ticks < 64, "самолёт не долетел до цели");
            let unit = state.unit(id).unwrap();
            if unit.grid == Point::new(10, 10) && !unit.has_move_order() {
                break;
            }
        }
        assert_eq!(state.unit(id).unwrap().grid, Point::new(10, 10));
    }

    #[test]
    fn paths_calculate_step_spends_speed() {
        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.speed = 12;
        unit.base_values.set_attribute(ATTRIB_SPEED, 12);
        unit.base_values.set_attribute(ATTRIB_ROUNDS, 2);
        unit.base_values.set_attribute(ATTRIB_MOVE_AND_FIRE, 0);
        unit.shots = 2;

        assert!(paths_calculate_step(&mut unit, 4, false, false, false));
        assert_eq!(unit.speed, 11);
        assert_eq!(unit.move_fraction, 0);
        assert_eq!(unit.max_velocity, 8);

        // The diagonal costs 6: 2 is charged with an empty move_fraction.
        unit.speed = 12;
        assert!(paths_calculate_step(&mut unit, 4, true, false, false));
        assert_eq!(unit.speed, 10);

        // Not enough speed — the step does not start.
        unit.speed = 0;
        assert!(!paths_calculate_step(&mut unit, 4, false, false, false));
    }

    #[test]
    fn paths_angle_and_turn_match_cpp() {
        assert_eq!(paths_get_angle(1, 0), 2);
        assert_eq!(paths_get_angle(1, 1), 3);
        assert_eq!(paths_get_angle(0, 1), 4);
        assert_eq!(paths_get_angle(-1, 1), 5);
        assert_eq!(paths_get_angle(-1, 0), 6);
        assert_eq!(paths_get_angle(-1, -1), 7);
        assert_eq!(paths_get_angle(0, -1), 0);
        assert_eq!(paths_get_angle(1, -1), 1);

        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.angle = 0;
        // One rotation step per call until it matches.
        let mut calls = 0;
        while paths_update_angle(&mut unit, 4) {
            calls += 1;
            assert!(calls < 8);
        }
        assert_eq!(unit.angle, 4);
    }

    /// Angle 0..7 → step (dx, dy) (`DIRECTION_OFFSETS`, `path.hpp`).
    fn step_for_angle(angle: i32) -> PathStep {
        let offset = DIRECTION_OFFSETS[(angle & 7) as usize];
        PathStep::new(offset.x as i8, offset.y as i8)
    }

    /// A tank with a manually set ground path and a `Move/IN_PROGRESS` order.
    fn state_with_path(angle: u8, steps: Vec<PathStep>) -> (GameState, UnitId) {
        let (mut state, id) = state_with_tank();
        state.unit_mut(id).unwrap().angle = angle;
        state.set_order_data(
            id,
            OrderData {
                move_to: Point::new(10, 10),
                fire_on: Point::ZERO,
            },
        );
        state.unit_mut(id).unwrap().path = Some(UnitPath::Ground(GroundPath {
            end_x: 10,
            end_y: 10,
            step_index: 0,
            steps,
        }));
        state.set_order(id, UnitOrder::Move, UnitOrderState::InProgress);
        (state, id)
    }

    /// Circular angle difference `a - b` in the range −4..=3.
    fn angle_delta(a: u8, b: u8) -> i32 {
        ((i32::from(a) - i32::from(b) + 12) & 7) - 4
    }

    /// C++ `GroundPath::UpdateUnitAngle`: on cell completion — exactly one
    /// 45° step toward the next route step; without a next step the angle stays.
    #[test]
    fn ground_path_update_unit_angle_matches_cpp() {
        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.angle = 2;

        let path = GroundPath {
            end_x: 0,
            end_y: 0,
            step_index: 0,
            steps: vec![PathStep::new(1, 0), PathStep::new(0, -1)],
        };
        assert!(ground_path_update_unit_angle(&mut unit, &path));
        assert_eq!(unit.angle, 1, "E → NE: один шаг к следующему шагу (N)");

        // No next step — rotation does not start (the last cell).
        let last = GroundPath {
            end_x: 0,
            end_y: 0,
            step_index: 1,
            steps: vec![PathStep::new(1, 0), PathStep::new(0, -1)],
        };
        let before = unit.angle;
        assert!(!ground_path_update_unit_angle(&mut unit, &last));
        assert_eq!(unit.angle, before);
    }

    /// `UnitInfo::UpdateAngle`: the turret rotates with the hull.
    #[test]
    fn body_turn_rotates_turret_like_cpp_update_angle() {
        let mut turreted = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        turreted.flags |= TURRET_SPRITE;
        turreted.angle = 2;
        turreted.turret_angle = 0;

        assert!(paths_update_angle(&mut turreted, 3));
        assert_eq!(turreted.angle, 3);
        assert_eq!(turreted.turret_angle, 1);

        // Without the turret flag the second field is not touched.
        let mut plain = UnitInfo::new(UnitType::Scout, TeamId::RED, UnitId(2));
        plain.angle = 2;
        plain.turret_angle = 6;
        assert!(paths_update_angle(&mut plain, 3));
        assert_eq!(plain.angle, 3);
        assert_eq!(plain.turret_angle, 6);
    }

    /// Straight movement in all 8 directions: the unit turns toward the step
    /// direction, moves only "facing" the course and keeps the last angle.
    #[test]
    fn all_eight_directions_move_facing_the_step() {
        for direction in 0..8i32 {
            let step = step_for_angle(direction);
            let start_angle = ((direction + 4) & 7) as u8;
            let (mut state, id) = state_with_path(start_angle, vec![step; 3]);

            let mut previous = state.unit(id).unwrap().grid;
            let mut moves = 0;
            let mut last_angle = start_angle;

            for _ in 0..40 {
                state.tick();
                let unit = state.unit(id).unwrap();
                let changed = unit.grid != previous;

                if changed {
                    let dx = unit.grid.x - previous.x;
                    let dy = unit.grid.y - previous.y;
                    assert_eq!(
                        paths_get_angle(i32::from(dx), i32::from(dy)),
                        direction,
                        "шаг {direction} ушёл не в ту сторону: {previous:?} → {:?}",
                        unit.grid
                    );
                    // On a straight line the next step matches — the angle must be
                    // the movement direction right after the step.
                    assert_eq!(
                        i32::from(unit.angle),
                        direction,
                        "прямой ход {direction}: угол не совпал с шагом"
                    );
                    moves += 1;
                    previous = unit.grid;
                }

                // Rotation — at most one 45° step per tick.
                let delta = angle_delta(unit.angle, last_angle);
                assert!(
                    (-1..=1).contains(&delta),
                    "поворот {direction}: скачок угла {last_angle} → {} за тик",
                    unit.angle
                );
                last_angle = unit.angle;

                if moves == 3 {
                    break;
                }
            }

            assert_eq!(moves, 3, "направление {direction}: пройдено не 3 клетки");
            let unit = state.unit(id).unwrap();
            assert_eq!(
                i32::from(unit.angle),
                direction,
                "после остановки угол — последнее направление {direction}"
            );

            // After stopping the angle does not change.
            for _ in 0..8 {
                state.tick();
                assert_eq!(i32::from(state.unit(id).unwrap().angle), direction);
            }
        }
    }

    /// L-route: on completing the current cell the hull makes one step toward
    /// the next one (`GroundPath::UpdateUnitAngle`), then finishes turning in
    /// `Execute` and only after that moves. Movement is always along the course.
    #[test]
    fn turn_at_cell_boundary_precedes_the_next_step() {
        let (mut state, id) = state_with_path(
            2,
            vec![
                PathStep::new(1, 0),
                PathStep::new(1, 0),
                PathStep::new(0, -1),
                PathStep::new(0, -1),
            ],
        );

        // Tick 1: the angle is already east — step to (11,10); the next step is still
        // east, so at the cell boundary there is no rotation.
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.grid, Point::new(11, 10));
        assert_eq!(unit.angle, 2);

        // Tick 2: step to (12,10); at the boundary rotation toward the next
        // step (north) starts: E → NE.
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.grid, Point::new(12, 10));
        assert_eq!(unit.angle, 1, "на границе клетки — один шаг 45° к северу");

        // Tick 3: finish turning NE → N without movement (as `GroundPath::Execute`).
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.grid, Point::new(12, 10));
        assert_eq!(unit.angle, 0);

        // Tick 4: movement north.
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.grid, Point::new(12, 9));
        assert_eq!(unit.angle, 0);

        // On every moving tick the angle matches the transition direction
        // (tolerance ±1 — only a rotation started at the boundary toward the next step).
        let mut previous = unit.grid;
        for _ in 0..40 {
            state.tick();
            let unit = state.unit(id).unwrap();
            if unit.grid != previous {
                let dx = i32::from(unit.grid.x) - i32::from(previous.x);
                let dy = i32::from(unit.grid.y) - i32::from(previous.y);
                let moved_angle = paths_get_angle(dx, dy) as u8;
                assert!(
                    angle_delta(unit.angle, moved_angle).abs() <= 1,
                    "кадр/сим повернулся на 45°+ против движения: {:?} → {:?}, angle {}",
                    previous,
                    unit.grid,
                    unit.angle
                );
                previous = unit.grid;
            }
            if !unit.has_move_order() {
                break;
            }
        }

        assert_eq!(state.unit(id).unwrap().grid, Point::new(12, 8));
        assert_eq!(state.unit(id).unwrap().angle, 0);
    }

    /// Two identical L-route runs give byte-identical snapshots on
    /// every tick: `angle` is part of the state and changes deterministically.
    #[test]
    fn angle_updates_deterministically_in_snapshot() {
        let steps = vec![
            PathStep::new(1, 0),
            PathStep::new(1, -1),
            PathStep::new(0, -1),
            PathStep::new(0, -1),
        ];
        let (mut first, first_id) = state_with_path(7, steps.clone());
        let (mut second, second_id) = state_with_path(7, steps);
        assert_eq!(first_id, second_id);

        let mut angle_changed = false;
        for tick in 0..24 {
            first.tick();
            second.tick();
            assert_eq!(
                first.snapshot_bytes(),
                second.snapshot_bytes(),
                "снапшот разошёлся на тике {tick}"
            );
            angle_changed |= first.unit(first_id).unwrap().angle != 7;
        }

        assert!(angle_changed, "тест обязан пройти через смену угла");

        // `angle` enters the snapshot (state CRC).
        let before = first.snapshot_bytes();
        first.unit_mut(first_id).unwrap().angle ^= 1;
        assert_ne!(before, first.snapshot_bytes());
    }

    #[test]
    fn land_and_take_animation_counters() {
        let mut unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(1));
        unit.moved = 0;
        let mut calls = 0;
        while !take(&mut unit) {
            calls += 1;
            assert!(calls < 8);
        }
        assert_eq!(unit.moved, 5);
        assert_ne!(unit.flags & HOVERING, 0);

        let mut calls = 0;
        while !land(&mut unit) {
            calls += 1;
            assert!(calls < 16);
        }
        assert_eq!(unit.moved, 9);
        assert_eq!(unit.flags & HOVERING, 0);
    }

    #[test]
    fn order_move_to_unit_stores_into_idle() {
        let (mut state, id) = state_with_tank();
        // Parent transport and passenger.
        let carrier = state
            .spawn_unit(
                UnitType::ArmouredPersonnelCarrier,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_SEA_UNIT | MOBILE_LAND_UNIT,
            )
            .expect("carrier");
        state.unit_mut(id).unwrap().parent_unit = Some(carrier);
        state.unit_mut(carrier).unwrap().storage = 0;

        state.set_order(id, UnitOrder::Idle, UnitOrderState::PrepareStore);
        state.set_order(id, UnitOrder::Load, UnitOrderState::Init);
        for _ in 0..24 {
            state.tick();
        }
        let unit = state.unit(id).unwrap();
        // `ProcessLoading` completes boarding: the passenger in AWAIT, without a parent.
        assert_eq!(unit.orders, UnitOrder::Await);
        assert!(unit.parent_unit.is_none());
        assert!(state.unit(carrier).unwrap().storage >= 0);
    }

    #[test]
    fn explode_of_dying_infantry_reaches_destroy() {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        let id = state
            .spawn_unit(
                UnitType::Infantry,
                TeamId::RED,
                Point::new(3, 3),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .expect("spawn");
        {
            let unit = state.unit_mut(id).unwrap();
            unit.hits = 0;
            unit.orders = UnitOrder::Explode;
            unit.state = UnitOrderState::Init;
        }
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.state, UnitOrderState::Destroy);
        // `units_manager.cpp:3932-3946`: its own 8-frame death animation
        // (`image_base = 168 + (angle / 2) * 8`, `image_index_max = base + 7`).
        assert_eq!(unit.image_base, 168);
        assert_eq!(unit.image_index_max, 175);
        // The animation goes one frame per two ticks, then the unit is destroyed.
        let mut destroyed = false;
        for _ in 0..24 {
            state.tick();
            if !state.has_unit(id) {
                destroyed = true;
                break;
            }
        }
        assert!(destroyed);
    }

    #[test]
    fn await_scaling_expand_decrements_adjust() {
        let (mut state, id) = state_with_tank();
        {
            let unit = state.unit_mut(id).unwrap();
            unit.orders = UnitOrder::AwaitScaling;
            unit.state = UnitOrderState::Expand;
            unit.prior_orders = UnitOrder::Move;
            unit.prior_state = UnitOrderState::InProgress;
            unit.scaler_adjust = 2;
        }
        state.tick();
        assert_eq!(state.unit(id).unwrap().scaler_adjust, 1);
        state.tick();
        state.tick();
        // scale to zero → RestoreOrders returns Move/InProgress.
        assert_eq!(state.unit(id).unwrap().orders, UnitOrder::Move);
    }

    // -----------------------------------------------------------------------
    // Projectiles: a pixel line (`AirPath::Execute`, airpath.cpp:175-295)
    // -----------------------------------------------------------------------

    /// A missile at (10,10) flies to (13,14): dx=3, dy=4 — the line goes **not** along
    /// the 8-direction route (the old port walked cells and wobbled).
    fn state_with_missile_in_flight() -> (GameState, UnitId, UnitId, (i32, i32)) {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Computer;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        state.team_units[1] = crate::team::TeamUnits::with_clan(TeamClan::Random);

        let target = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::GREEN,
                Point::new(13, 14),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .expect("target");
        let attacker = state
            .spawn_unit(
                UnitType::MissileCrawler,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .expect("attacker");
        let missile = state
            .spawn_unit(UnitType::Rocket, TeamId::RED, Point::new(10, 10), 0)
            .expect("rocket");
        let start = {
            let unit = state.unit(missile).expect("rocket");
            (i32::from(unit.x), i32::from(unit.y))
        };
        let mut unit = state.unit(missile).expect("rocket").clone();
        unit.parent_unit = Some(attacker);
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::Init;
        state.write_unit(missile, unit);
        state.set_order_data(
            missile,
            OrderData {
                move_to: Point::new(13, 14),
                fire_on: Point::new(13, 14),
            },
        );
        state.set_order_data(
            attacker,
            OrderData {
                move_to: Point::new(13, 14),
                fire_on: Point::new(13, 14),
            },
        );

        (state, missile, target, start)
    }

    /// Perpendicular distance from a point to the line `start → end` (pixels).
    fn distance_to_line(point: (i32, i32), start: (i32, i32), end: (i32, i32)) -> f64 {
        let dx = f64::from(end.0 - start.0);
        let dy = f64::from(end.1 - start.1);
        let length = (dx * dx + dy * dy).sqrt();
        if length == 0.0 {
            return 0.0;
        }
        let px = f64::from(point.0 - start.0);
        let py = f64::from(point.1 - start.1);
        (px * dy - py * dx).abs() / length
    }

    #[test]
    fn missile_flies_a_pixel_straight_line() {
        let (mut state, missile, _target, start) = state_with_missile_in_flight();
        // The aim point is the cell center (13,14).
        let end = (13 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2, 14 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2);

        let mut samples = Vec::new();
        for _ in 0..12 {
            state.tick();
            let Some(unit) = state.unit(missile) else {
                break;
            };
            samples.push((i32::from(unit.x), i32::from(unit.y)));
        }

        assert!(samples.len() >= 3, "снаряд должен лететь несколько тиков");
        // The first point is still at the launch site, then strictly along the line.
        for point in &samples {
            let distance = distance_to_line(*point, start, end);
            assert!(
                distance <= 1.5,
                "точка {point:?} не на прямой {start:?}→{end:?}: отклонение {distance:.2} px"
            );
        }
        // The position really changes from tick to tick (a pixel step, not
        // "a cell per tick to the center").
        let unique: std::collections::BTreeSet<_> = samples.iter().collect();
        assert!(unique.len() >= 3, "снаряд должен двигаться попиксельно");
        // And it arrives at the target cell (the last live point — near the center).
        let last = *samples.last().expect("точки");
        assert!(
            distance_to_line(last, start, end) <= 1.5
                && (last.0 - end.0).abs() <= GFX_MAP_TILE_SIZE
                && (last.1 - end.1).abs() <= GFX_MAP_TILE_SIZE,
            "снаряд не дошёл до цели: {last:?}"
        );
    }

    #[test]
    fn missile_trail_particles_lie_on_the_line() {
        let (mut state, missile, _target, start) = state_with_missile_in_flight();
        let end = (13 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2, 14 * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2);

        // One tick: the projectile traverses the first cell and leaves three particles behind.
        state.tick();
        state.tick();
        let particles: Vec<_> = state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.unit(id).cloned())
            .filter(|unit| unit.unit_type == UnitType::RocketSmoke)
            .collect();

        assert!(
            particles.len() >= 3,
            "Paths_DrawMissile: три частицы на покинутую клетку, найдено {}",
            particles.len()
        );
        for particle in &particles {
            let point = (i32::from(particle.x), i32::from(particle.y));
            let distance = distance_to_line(point, start, end);
            assert!(
                distance <= 1.5,
                "частица следа {point:?} не на прямой (отклонение {distance:.2} px)"
            );
        }
        let _ = missile;
    }

    #[test]
    fn missile_impact_uses_the_engine_cell_and_destroys_the_round() {
        let (mut state, missile, target, _start) = state_with_missile_in_flight();

        let mut impact_cell = None;
        let mut destroyed = false;
        for _ in 0..24 {
            state.tick();

            if impact_cell.is_none() {
                impact_cell = state.unit_ids_in_order().into_iter().find_map(|id| {
                    state.unit(id).and_then(|unit| {
                        matches!(
                            unit.unit_type,
                            UnitType::HitExplosion
                                | UnitType::LandExplosion
                                | UnitType::BuildingExplosion
                                | UnitType::AirExplosion
                                | UnitType::SeaExplosion
                        )
                        .then_some(unit.grid)
                    })
                });
            }

            if !state.has_unit(missile) {
                destroyed = true;

                // The explosion is placed on the next tick (`ProcessOrderExplode`/`INIT`
                // → `UnitsManager_StartExplosion`), so we collect frames.
                if impact_cell.is_some() {
                    break;
                }
            }
        }

        assert!(destroyed, "по `m_length == 0` снаряд уничтожается");
        // `Paths_FinishMove` strikes the target cell (`13,14`) — the explosion
        // `HITEXPLD`/`LNDEXPLD` appears exactly there.
        assert_eq!(
            impact_cell,
            Some(Point::new(13, 14)),
            "`parent->Attack(grid_x, grid_y)`: удар в клетке цели"
        );
        // And the target took damage.
        let tank = state.unit(target).expect("танк");
        assert!(
            tank.hits == 0
                || tank.hits
                    < tank
                        .base_values
                        .get_attribute(crate::unit_values::ATTRIB_HITS) as u16,
            "попадание должно нанести урон"
        );
    }

    // -----------------------------------------------------------------------
    // State-machine boundaries: waiting, stopping, order override, building
    // -----------------------------------------------------------------------

    /// `UnitInfo::MovementWaitCount`: saturates at `MOVEMENT_WAIT_LIMIT`, reports
    /// `true` from that point on and never wraps; `ClearMovementWait` resets it.
    #[test]
    fn movement_wait_limit_saturates() {
        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        assert_eq!(unit.movement_wait_count, 0);
        assert!(!register_movement_wait(&mut unit));

        // One step before the limit becomes the timed-out state.
        unit.movement_wait_count = MOVEMENT_WAIT_LIMIT - 1;
        assert!(register_movement_wait(&mut unit));
        assert_eq!(unit.movement_wait_count, MOVEMENT_WAIT_LIMIT);

        // At the limit every further call reports the timeout and keeps the count.
        assert!(register_movement_wait(&mut unit));
        assert!(register_movement_wait(&mut unit));
        assert_eq!(unit.movement_wait_count, MOVEMENT_WAIT_LIMIT, "no wrap");

        clear_movement_wait(&mut unit);
        assert_eq!(unit.movement_wait_count, 0);
        assert!(!register_movement_wait(&mut unit));
    }

    /// `UnitInfo::StopMove` only converts movement orders to a stopped `MOVE`;
    /// other orders and their counters are untouched.
    #[test]
    fn stop_move_affects_only_move_orders() {
        for order in [
            UnitOrder::Move,
            UnitOrder::MoveToUnit,
            UnitOrder::MoveToAttack,
        ] {
            let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
            unit.orders = order;
            unit.state = UnitOrderState::InProgress;
            unit.path = Some(UnitPath::Ground(GroundPath::default()));
            unit.moved = 5;
            unit.velocity = 7;
            unit.in_transit = true;

            stop_move(&mut unit, false);
            assert_eq!(unit.orders, UnitOrder::Move, "{order:?}");
            assert_eq!(unit.state, UnitOrderState::ExecutingOrder, "{order:?}");
            assert_eq!(unit.moved, 0);
            assert_eq!(unit.velocity, 0);
            assert!(!unit.in_transit);
            assert_eq!(unit.delayed_reaction, 0, "mode=false");
        }

        let mut awaiting = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
        awaiting.moved = 3;
        stop_move(&mut awaiting, true);
        assert_eq!(awaiting.orders, UnitOrder::Await);
        assert_eq!(awaiting.moved, 3, "non-movement order is not reset");

        let mut firing = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(3));
        firing.orders = UnitOrder::Fire;
        firing.state = UnitOrderState::FireInProgress;
        stop_move(&mut firing, true);
        assert_eq!(firing.orders, UnitOrder::Fire);
        assert_eq!(firing.state, UnitOrderState::FireInProgress);
    }

    /// `UnitInfo::StopMovement` (emergency stop): `IN_PROGRESS` blocks the route,
    /// `IN_TRANSITION` truncates it to a zero step; other states are left alone.
    #[test]
    fn stop_movement_handles_in_progress_and_in_transition() {
        let path = || {
            Some(UnitPath::Ground(GroundPath {
                end_x: 5,
                end_y: 5,
                step_index: 1,
                steps: vec![PathStep::new(1, 0), PathStep::new(0, 1)],
            }))
        };

        let mut moving = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        moving.orders = UnitOrder::Move;
        moving.state = UnitOrderState::InProgress;
        moving.path = path();
        stop_movement(&mut moving, false);
        assert_eq!(moving.orders, UnitOrder::Await);
        assert_eq!(moving.state, UnitOrderState::ExecutingOrder);
        assert!(moving.path.is_none());
        assert_eq!(moving.delayed_reaction, 1);

        let mut transitioning = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
        transitioning.orders = UnitOrder::Move;
        transitioning.state = UnitOrderState::InTransition;
        transitioning.path = path();
        stop_movement(&mut transitioning, false);
        assert_eq!(transitioning.orders, UnitOrder::Move);
        assert_eq!(transitioning.state, UnitOrderState::InTransition);
        match transitioning.path.as_ref() {
            Some(UnitPath::Ground(path)) => {
                assert_eq!(path.step_index, 0);
                assert_eq!(path.steps, vec![PathStep::new(0, 0)]);
            }
            other => panic!("expected a truncated ground path, got {other:?}"),
        }

        // No path — nothing to stop.
        let mut idle = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(3));
        idle.orders = UnitOrder::Move;
        idle.state = UnitOrderState::InProgress;
        stop_movement(&mut idle, false);
        assert_eq!(idle.orders, UnitOrder::Move);

        // Wrong order keeps the path.
        let mut awaiting = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(4));
        awaiting.orders = UnitOrder::Await;
        awaiting.state = UnitOrderState::InProgress;
        awaiting.path = path();
        stop_movement(&mut awaiting, false);
        assert!(awaiting.path.is_some());

        // `disable_fire` suppresses the delayed reaction.
        let mut quiet = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(5));
        quiet.orders = UnitOrder::Move;
        quiet.state = UnitOrderState::InProgress;
        quiet.path = path();
        stop_movement(&mut quiet, true);
        assert_eq!(quiet.delayed_reaction, 0);
    }

    /// `UnitsManager_SetNewOrderInt`: an active `FIRE` (and terminal `EXPLODE`/`DESTROY`)
    /// cannot be replaced, `NEW_ALLOCATE` does not become the prior order, and
    /// `NEW_ORDER` collapses to `AWAIT` first.
    #[test]
    fn set_new_order_int_override_boundaries() {
        // A unit that is firing ignores a non-EXPLODE order (`units_manager.cpp:1945`).
        let mut firing = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        firing.orders = UnitOrder::Fire;
        firing.state = UnitOrderState::FireInProgress;
        firing.prior_orders = UnitOrder::Await;
        assert!(!set_new_order_int(
            &mut firing,
            UnitOrder::Move,
            UnitOrderState::Init
        ));
        assert_eq!(firing.orders, UnitOrder::Fire);
        assert_eq!(firing.state, UnitOrderState::FireInProgress);

        // EXPLODE is always accepted but keeps the firing order's prior.
        assert!(!set_new_order_int(
            &mut firing,
            UnitOrder::Explode,
            UnitOrderState::Init
        ));
        assert_eq!(firing.orders, UnitOrder::Explode);

        // Terminal states are frozen.
        let mut exploding = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
        exploding.orders = UnitOrder::Explode;
        exploding.state = UnitOrderState::Destroy;
        assert!(!set_new_order_int(
            &mut exploding,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder
        ));
        assert_eq!(exploding.orders, UnitOrder::Explode);
        assert_eq!(exploding.state, UnitOrderState::Destroy);

        // `NEW_ALLOCATE` is not recorded as the prior order.
        let mut allocating = UnitInfo::new(UnitType::GoldRefinery, TeamId::RED, UnitId(3));
        allocating.orders = UnitOrder::NewAllocate;
        allocating.state = UnitOrderState::Init;
        allocating.prior_orders = UnitOrder::Await;
        allocating.prior_state = UnitOrderState::ExecutingOrder;
        assert!(!set_new_order_int(
            &mut allocating,
            UnitOrder::Move,
            UnitOrderState::Init
        ));
        assert_eq!(allocating.orders, UnitOrder::Move);
        assert_eq!(
            allocating.prior_orders,
            UnitOrder::Await,
            "NewAllocate kept out of prior"
        );
    }

    /// `UnitInfo::RestoreOrders` exception list: powered buildings keep a prior
    /// equal to the current order; every other type resets it to `AWAIT`.
    #[test]
    fn restore_orders_building_exception_table() {
        for unit_type in [
            UnitType::GoldRefinery,
            UnitType::MiningStation,
            UnitType::Habitat,
            UnitType::PowerGenerator,
            UnitType::PowerStation,
            UnitType::ResearchCentre,
        ] {
            let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
            unit.orders = UnitOrder::Move;
            unit.state = UnitOrderState::InProgress;
            unit.prior_orders = UnitOrder::Move;
            unit.prior_state = UnitOrderState::InProgress;
            restore_orders(&mut unit);
            assert_eq!(unit.orders, UnitOrder::Move, "{unit_type:?}");
            assert_eq!(unit.state, UnitOrderState::InProgress, "{unit_type:?}");
        }

        for unit_type in [UnitType::Tank, UnitType::Scout, UnitType::Constructor] {
            let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
            unit.orders = UnitOrder::Move;
            unit.state = UnitOrderState::InProgress;
            unit.prior_orders = UnitOrder::Move;
            unit.prior_state = UnitOrderState::InProgress;
            restore_orders(&mut unit);
            assert_eq!(unit.orders, UnitOrder::Await, "{unit_type:?}");
            assert_eq!(unit.state, UnitOrderState::ExecutingOrder, "{unit_type:?}");
        }

        // A differing prior is never reset, even for a building.
        let mut building = UnitInfo::new(UnitType::Habitat, TeamId::RED, UnitId(2));
        building.orders = UnitOrder::PowerOff;
        building.state = UnitOrderState::Init;
        building.prior_orders = UnitOrder::PowerOn;
        building.prior_state = UnitOrderState::ExecutingOrder;
        restore_orders(&mut building);
        assert_eq!(building.orders, UnitOrder::PowerOn);
        assert_eq!(building.state, UnitOrderState::ExecutingOrder);
    }

    /// `UnitInfo::CancelBuilding`: mobile builders return to `AWAIT` and drop the
    /// queue/route; a stationary structure switches to the halt-building order.
    #[test]
    fn cancel_building_variants() {
        let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
        builder.flags = crate::access::unit_type_flags(UnitType::Constructor);
        builder.orders = UnitOrder::Build;
        builder.state = UnitOrderState::BuildInProgress;
        builder.build_list = vec![UnitType::PowerGenerator];
        builder.build_time = 5;
        builder.path = Some(UnitPath::Ground(GroundPath::default()));
        builder.moved = 4;
        cancel_building(&mut builder);
        assert_eq!(builder.orders, UnitOrder::Await);
        assert_eq!(builder.state, UnitOrderState::ExecutingOrder);
        assert!(builder.build_list.is_empty());
        assert_eq!(builder.build_time, 0);
        assert!(builder.path.is_none());
        assert_eq!(builder.moved, 0);

        // Stationary structure, cancel requested.
        let mut structure = UnitInfo::new(UnitType::GoldRefinery, TeamId::RED, UnitId(2));
        structure.flags = crate::access::unit_type_flags(UnitType::GoldRefinery);
        structure.orders = UnitOrder::Build;
        structure.state = UnitOrderState::BuildCancel;
        structure.build_list = vec![UnitType::Tank];
        structure.build_time = 3;
        cancel_building(&mut structure);
        assert_eq!(structure.orders, UnitOrder::HaltBuilding);
        assert_eq!(structure.state, UnitOrderState::ExecutingOrder);
        assert_eq!(
            structure.build_list,
            vec![UnitType::Tank],
            "queue kept until halt"
        );
        assert_eq!(structure.build_time, 3);

        // Stationary structure, abort without a cancel request.
        structure.orders = UnitOrder::Build;
        structure.state = UnitOrderState::BuildInProgress;
        cancel_building(&mut structure);
        assert_eq!(structure.orders, UnitOrder::HaltBuilding2);
        assert_eq!(structure.state, UnitOrderState::ExecutingOrder);

        // Any other stationary order just clears the queue.
        structure.orders = UnitOrder::Await;
        structure.state = UnitOrderState::ExecutingOrder;
        cancel_building(&mut structure);
        assert_eq!(structure.orders, UnitOrder::Await);
        assert!(structure.build_list.is_empty());
        assert_eq!(structure.build_time, 0);
    }

    /// `UnitInfo::ClearBuildListAndPath`: a parented unit waits for `ACTIVATE`,
    /// a free one returns to `AWAIT`; both lose the queue and the path.
    #[test]
    fn clear_build_list_and_path_variants() {
        let mut parented = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
        parented.parent_unit = Some(UnitId(2));
        parented.path = Some(UnitPath::Ground(GroundPath::default()));
        parented.build_list = vec![UnitType::Tank];
        clear_build_list_and_path(&mut parented);
        assert_eq!(parented.orders, UnitOrder::Build);
        assert_eq!(parented.state, UnitOrderState::UnitReady);
        assert!(parented.path.is_none());
        assert!(parented.build_list.is_empty());

        let mut free = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(3));
        free.path = Some(UnitPath::Ground(GroundPath::default()));
        free.build_list = vec![UnitType::Tank];
        clear_build_list_and_path(&mut free);
        assert_eq!(free.orders, UnitOrder::Await);
        assert_eq!(free.state, UnitOrderState::ExecutingOrder);
        assert!(free.path.is_none());
        assert!(free.build_list.is_empty());
    }

    /// `active_build_site` returns the site only for a mobile builder with a
    /// running construction and a non-empty queue; terminal states hide it.
    #[test]
    fn active_build_site_boundaries() {
        let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
        builder.flags = crate::access::unit_type_flags(UnitType::Constructor);
        builder.orders = UnitOrder::Build;
        builder.state = UnitOrderState::BuildInProgress;
        builder.build_list = vec![UnitType::PowerGenerator];
        builder.target_grid = Point::new(7, 8);
        assert_eq!(active_build_site(&builder), Some(Point::new(7, 8)));

        builder.orders = UnitOrder::AwaitTapePositioning;
        builder.state = UnitOrderState::TapePositioningEnter;
        assert_eq!(active_build_site(&builder), Some(Point::new(7, 8)));

        for state in [
            UnitOrderState::SelectSite,
            UnitOrderState::UnitReady,
            UnitOrderState::BuildCancel,
            UnitOrderState::BuildAbort,
        ] {
            builder.state = state;
            assert_eq!(active_build_site(&builder), None, "{state:?}");
        }

        builder.state = UnitOrderState::BuildInProgress;
        builder.build_list.clear();
        assert_eq!(active_build_site(&builder), None, "empty queue");

        builder.build_list = vec![UnitType::PowerGenerator];
        builder.orders = UnitOrder::Await;
        assert_eq!(active_build_site(&builder), None, "not a build order");

        let tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
        assert_eq!(active_build_site(&tank), None, "not a mobile builder");
    }

    /// `Paths_CalculateStep` cost classes: `< 4` doubles `max_velocity`, `> 4`
    /// halves it; the diagonal costs `cost * 3 / 2`; `move_fraction` carries the
    /// remainder and `shots` are reduced only for `MOVE_AND_FIRE == 0`.
    #[test]
    fn paths_calculate_step_cost_classes_and_fraction() {
        fn tank() -> UnitInfo {
            let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
            unit.base_values.set_attribute(ATTRIB_SPEED, 6);
            unit.base_values.set_attribute(ATTRIB_ROUNDS, 2);
            unit.base_values.set_attribute(ATTRIB_MOVE_AND_FIRE, 0);
            unit.shots = 2;
            unit.speed = 12;
            unit.move_fraction = 0;
            unit
        }

        // cost 2: max_velocity 8 → 16, normalized 1, remainder 2.
        let mut cheap = tank();
        assert!(paths_calculate_step(&mut cheap, 2, false, false, false));
        assert_eq!(cheap.max_velocity, 16);
        assert_eq!(cheap.speed, 11);
        assert_eq!(cheap.move_fraction, 2);
        assert_eq!(cheap.shots, 2, "2*11/6 = 3 is not below 2");

        // The carried fraction is consumed by the next step of the same cost:
        // the whole cost is covered by the fraction, so no speed is spent.
        assert!(paths_calculate_step(&mut cheap, 2, false, false, false));
        assert_eq!(cheap.move_fraction, 0);
        assert_eq!(cheap.speed, 11);

        // cost 4: max_velocity 8, normalized 1, no remainder.
        let mut normal = tank();
        assert!(paths_calculate_step(&mut normal, 4, false, false, false));
        assert_eq!(normal.max_velocity, 8);
        assert_eq!(normal.speed, 11);
        assert_eq!(normal.move_fraction, 0);

        // cost 6: max_velocity 4, normalized 2, remainder 2.
        let mut expensive = tank();
        assert!(paths_calculate_step(&mut expensive, 6, false, false, false));
        assert_eq!(expensive.max_velocity, 4);
        assert_eq!(expensive.speed, 10);
        assert_eq!(expensive.move_fraction, 2);

        // Diagonal: cost 4 is charged as 6, but the velocity class follows the
        // raw cost argument.
        let mut diagonal = tank();
        assert!(paths_calculate_step(&mut diagonal, 4, true, false, false));
        assert_eq!(diagonal.max_velocity, 8);
        assert_eq!(diagonal.speed, 10);
        assert_eq!(diagonal.move_fraction, 2);

        // `MOVE_AND_FIRE` units keep their shot count; the others recompute it.
        let mut fire_on_move = tank();
        fire_on_move
            .base_values
            .set_attribute(ATTRIB_MOVE_AND_FIRE, 1);
        fire_on_move.speed = 1;
        assert!(paths_calculate_step(
            &mut fire_on_move,
            4,
            false,
            false,
            false
        ));
        assert_eq!(fire_on_move.speed, 0);
        assert_eq!(fire_on_move.shots, 2);

        let mut stationary_fire = tank();
        stationary_fire.speed = 1;
        assert!(paths_calculate_step(
            &mut stationary_fire,
            4,
            false,
            false,
            false
        ));
        assert_eq!(stationary_fire.shots, 0, "2*0/6 = 0");

        // Not enough speed: the step does not start, the counter is charged into
        // `move_fraction`, the group speed collapses to 1.
        let mut slow = tank();
        slow.speed = 1;
        slow.group_speed = 5;
        assert!(!paths_calculate_step(&mut slow, 6, false, false, false));
        assert_eq!(slow.speed, 0);
        assert_eq!(slow.move_fraction, 4);
        assert_eq!(slow.group_speed, 1);

        // Infantry halves `max_velocity` and records `velocity`.
        let mut infantry = UnitInfo::new(UnitType::Infantry, TeamId::RED, UnitId(2));
        infantry.speed = 4;
        infantry.move_fraction = 0;
        assert!(paths_calculate_step(&mut infantry, 4, false, false, false));
        assert_eq!(infantry.max_velocity, 4);
        assert_eq!(infantry.velocity, 4);
    }

    /// `UnitInfo::Land`/`Take` animation counters are 6-bit and saturate at the
    /// 9th/5th call; `HOVERING` flips exactly on completion.
    #[test]
    fn land_and_take_boundaries_wrap() {
        let mut unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(1));
        unit.moved = 8;
        assert!(land(&mut unit));
        assert_eq!(unit.moved, 9);
        assert_eq!(unit.flags & HOVERING, 0, "landing clears HOVERING");

        unit.moved = 0x3F;
        assert!(!land(&mut unit), "0x3F wraps to 0: animation restarts");
        assert_eq!(unit.moved, 0);

        let mut unit = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(2));
        unit.moved = 3;
        assert!(!take(&mut unit));
        assert_eq!(unit.moved, 4);
        assert!(take(&mut unit));
        assert_eq!(unit.moved, 5);
        assert_ne!(unit.flags & HOVERING, 0, "takeoff sets HOVERING");

        unit.moved = 0x3F;
        assert!(!take(&mut unit));
        assert_eq!(unit.moved, 0);
    }

    /// `is_bridge_elevated`: only the bridge type, only while its frame differs
    /// from the base frame (`UnitInfo::IsBridgeElevated`).
    #[test]
    fn bridge_elevation_boundaries() {
        let mut bridge = UnitInfo::new(UnitType::Bridge, TeamId::RED, UnitId(1));
        bridge.image_base = 10;
        bridge.image_index = 10;
        assert!(!is_bridge_elevated(&bridge));
        bridge.image_index = 11;
        assert!(is_bridge_elevated(&bridge));

        let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
        tank.image_base = 0;
        tank.image_index = 1;
        assert!(!is_bridge_elevated(&tank));
    }

    /// `Paths_UpdateAngle` rotates one 45° step the short way, wraps 7↔0 and
    /// rotates the turret with the hull.
    #[test]
    fn paths_update_angle_wraps_between_7_and_0() {
        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.flags = crate::access::unit_type_flags(UnitType::Tank);
        unit.angle = 7;
        unit.turret_angle = 7;
        assert!(paths_update_angle(&mut unit, 0));
        assert_eq!(unit.angle, 0);
        assert_eq!(unit.turret_angle, 0);

        assert!(
            !paths_update_angle(&mut unit, 0),
            "already facing the target"
        );

        unit.angle = 0;
        unit.turret_angle = 0;
        assert!(paths_update_angle(&mut unit, 7));
        assert_eq!(unit.angle, 7);
        assert_eq!(unit.turret_angle, 7);
    }

    /// `UnitInfo::PowerUp`/`PowerDown` leave the unit in `EXECUTING_ORDER`.
    #[test]
    fn power_up_and_down_set_executing_order() {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);

        let mut unit = UnitInfo::new(UnitType::PowerGenerator, TeamId::RED, UnitId(1));
        unit.state = UnitOrderState::Init;
        power_up(&mut unit, 1);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);

        unit.state = UnitOrderState::Init;
        power_down(&mut state, &mut unit);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    }
}
