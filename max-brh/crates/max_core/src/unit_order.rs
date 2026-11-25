//! Order machine: `UnitOrder` (32) Г— `UnitOrderState` (47).
//!
//! Oracle: `max-cpp-port/src/units_manager.cpp` (`UnitsManager_SetNewOrderInt`,
//! `UnitsManager_SetNewOrder`, `UnitsManager_ProcessOrder`,
//! `UnitsManager_ProcessUnitOrders`, 32 `UnitsManager_ProcessOrder*` functions,
//! `UnitsManager_EndAttack`, `UnitsManager_PinAttackTargets`,
//! `UnitsManager_ScaleUnit`, `UnitsManager_NewOrderWhileScaling`,
//! `UnitsManager_FinishUnitScaling`, `UnitsManager_LoadUnit`), `unitinfo.cpp`
//! (`UnitInfo::RestoreOrders`,
//! `ExecuteMove`, `Move`, `MoveFinished`, `BlockedOnPathRequest`, `StopMove`,
//! `StopMovement`, `Land`, `Take`, `ProcessLanding`, `ProcessLoading`,
//! `ProcessUnloading`, `PositionInTape`, `UpdateAngle`, `StartBuilding`,
//! `CancelBuilding`,
//! `PowerUp`, `PowerDown`, `Reload`, `Repair`, `Transfer`, `Upgrade`,
//! `CheckIfDestroyed`), `groundpath.cpp` (`GroundPath::Execute`,
//! `GroundPath::UpdateUnitAngle`, `CancelMovement`),
//! `airpath.cpp` (`AirPath::Execute`, `Paths_FinishMove`), `paths.cpp`
//! (`Paths_GetAngle`, `Paths_UpdateAngle`,
//! `Paths_CalculateStep`), DESIGN.md В§5.2.
//!
//! There is no separate `unitorder.cpp` file in the C++ port: the order machine lives in
//! `units_manager.cpp` (dispatcher and processors) and `unitinfo.cpp` (the
//! `UnitInfo` methods implementing the transitions). The port repeats this split: processors
//! and the dispatcher вЂ” here, operations on the unit вЂ” [`UnitInfo`] methods,
//! list traversal and inter-unit effects вЂ” [`crate::game::GameState`].
//!
//! # Model
//!
//! An order is processed over a **working copy** of the unit ([`OrderContext`]):
//! `UnitInfo` is cloned, the machine changes the copy, and at the end the copy is written back.
//! In C++ all changes go through pointers; the copy is equivalent because
//! `ProcessOrders` visits each list exactly once and does not re-execute
//! the same unit in the same pass. Inter-unit effects (parent,
//! target, live attack, lists) are performed by explicit `GameState` methods, so
//! they are visible to other units immediately вЂ” as in C++.
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
//! `PerformAutoSurvey` вЂ” `Ai_EnableAutoSurvey`, `ScheduleDelayedTasks` вЂ”
//! `TaskManager::ScheduleDelayedTasks`. The remaining `TODO(P3)` are economy and
//! complexes; `TODO(P5)` at points that require `UnitsManager_DeployUnit`
//! of concrete types (master builder, activation).
//!
//! # Divergences from C++
//!
//! 1. **Movement** (DESIGN В§5.1, divergence 3): C++ moves the unit pixel by pixel
//!    (`UnitInfo::Move`: `velocity`/`moved`/`GFX_MAP_TILE_SIZE`) and in a network
//!    game completes at most one cell per tick. `max_core` is lockstep, and
//!    [`unit_move`] completes exactly one cell per tick: the path step is considered
//!    traversed, the offset is subtracted from the current `PathStep` (as in C++
//!    `Move()`: `path_steps[step_index]->x -= offset_x`). The pixel animation вЂ”
//!    P5.
//! 2. `Paths_CalculateStep` is ported in full (`speed` spend, `move_fraction`,
//!    `shots`, `max_velocity`), so the per-turn movement range matches C++
//!    with the same attributes.
//! 3. `Animate`/`RefreshScreen`/`AddToDrawList` rendering вЂ” UI, not ported.
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

/// `GFX_MAP_TILE_SIZE` вЂ” the cell side in pixels (`unitinfo.cpp`).
pub const GFX_MAP_TILE_SIZE: i32 = 64;

// ---------------------------------------------------------------------------
// Bridges in AI (P6)
// ---------------------------------------------------------------------------

/// `UnitInfo::Animate` / `RefreshScreen` вЂ” UI animation and dirty regions (not needed by P4).
fn animate(_unit: &UnitInfo) {}

/// `UnitInfo::RefreshScreen` вЂ” UI.
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

/// `UnitInfo::ScheduleDelayedTasks` вЂ” P6: remind the unit's top task
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

/// `UnitInfo::SetSpriteFrameForTerrain` (`unitinfo.cpp:5025`).
///
/// Amphibious units have two sprite blocks: the submerged/water pose and the
/// surface pose. For `CLNTRANS` (the personnel carrier) water is the hidden pose
/// (`image_base = 0`) and land the surfaced one (`8`); for every other amphibious
/// type water is `8` and land `0`. `COMMANDO` always returns to `0` вЂ” it is the
/// sprites' ambush pose, not terrain.
pub(crate) fn set_sprite_frame_for_terrain(state: &GameState, unit: &mut UnitInfo, grid: Point) {
    let amphibious =
        unit.flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT) == (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT);

    if amphibious {
        let surface = access::modified_surface_type(state, grid, false);
        let water = surface == access::SURFACE_TYPE_WATER;

        unit.image_base = if unit.unit_type == UnitType::ArmouredPersonnelCarrier {
            if water {
                0
            } else {
                8
            }
        } else if water {
            8
        } else {
            0
        };

        unit.image_index = unit.image_base.wrapping_add(i16::from(unit.angle));
    } else if unit.unit_type == UnitType::Infiltrator {
        unit.image_base = 0;
        unit.image_index = i16::from(unit.angle);
    }
}

/// `CLNTRANS` sprite block on the terrain (`GroundPath::Execute`,
/// `groundpath.cpp:327-360`).
///
/// On water the carrier is drawn submerged (`0`) while no enemy's sea-stealth
/// coverage reveals it; once a sea heat map has stealth coverage on the cell, or
/// the unit is detected and the cell is covered (`GetComplete`), the surfaced
/// block `8` is used. On land the surface block is `8` as well.
pub(crate) fn terrain_image_base(state: &GameState, unit: &UnitInfo, grid: Point) -> i16 {
    let surface = access::modified_surface_type(state, grid, false);
    let water = surface == access::SURFACE_TYPE_WATER;

    if unit.unit_type != UnitType::ArmouredPersonnelCarrier {
        return if water { 8 } else { 0 };
    }

    if !water {
        return 8;
    }

    let grid_x = i32::from(grid.x);
    let grid_y = i32::from(grid.y);

    for (index, team_info) in state.teams.iter().enumerate() {
        if team_info.team_type == crate::team::TeamKind::None {
            continue;
        }

        let team = crate::team::TeamId(index as u8);

        if team == unit.team {
            continue;
        }

        if state.ai_heat_map_stealth_sea(team, grid_x, grid_y) != 0
            || (unit.is_detected_by_team(team)
                && state.ai_heat_map_complete(team, grid_x, grid_y) != 0)
        {
            return 8;
        }
    }

    0
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

/// `UnitsManager_ScaleUnit` (`units_manager.cpp:1847`) вЂ” start the building
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
        // of its branches (order/packet 08) do it themselves вЂ” see
        // [`crate::complex::update_connectors`]. A pure function cannot.
    } else {
        unit.scaler_adjust = 4;
        finish_unit_scaling(unit);
    }
}

/// `UnitsManager_FinishUnitScaling` (`units_manager.cpp:2471`) вЂ” version without
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
/// `disable_fire` вЂ” the setting that disables reaction fire (DESIGN В§6.5:
/// game settings are input facts; in max_core вЂ” a `GameState` field).
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
/// they need game state access. Here вЂ” reset of the movement counters,
/// which are entirely local to the unit.
///
/// `Ai_CheckMines` also requires the game, so it is called from
/// [`OrderContext::move_finished`].
pub fn move_finished(unit: &mut UnitInfo, mode: bool, disable_fire: bool) {
    unit.velocity = 0;
    unit.moved = 0;

    clear_movement_wait(unit);

    if unit.unit_type == UnitType::Infantry || unit.unit_type == UnitType::Infiltrator {
        // `UpdateSpriteFrame(0, image_index_max)` вЂ” UI.
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

/// `UnitInfo::StopMovement` (`unitinfo.cpp:3222`) вЂ” emergency stop.
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

/// `UnitInfo::Land` (`unitinfo.cpp:3829`) вЂ” landing animation, `moved` grows
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

/// `UnitInfo::Take` (`unitinfo.cpp:3862`) вЂ” takeoff animation, `true` on the 5th
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

/// `UnitInfo::Redraw` (`unitinfo.cpp:1452-1459`) вЂ” the shadow shift of the unit.
///
/// In C++ the shadow is drawn at `point - shadow_offset` (`RenderShadow`,
/// `unitinfo.cpp:3067`), so a **hovering aircraft** (`MOBILE_AIR_UNIT` +
/// `HOVERING`) gets `shadow_offset = (-64, -64)` вЂ” the shadow one cell
/// down-right of the hull, which is what makes a flying aircraft look airborne;
/// every other unit has `(0, 0)` (the shadow under the hull). The value is stored
/// in [`max_types::unit::UnitPresentation::shadow_offset`] (the C++ `UnitInfo`
/// field, also in the V70 save) and is **not** part of the snapshot/CRC.
pub fn air_shadow_offset(flags: u32) -> Point {
    if flags & MOBILE_AIR_UNIT != 0 && flags & HOVERING != 0 {
        Point::new(-(GFX_MAP_TILE_SIZE as i16), -(GFX_MAP_TILE_SIZE as i16))
    } else {
        Point::ZERO
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
/// For a mobile builder it places the construction site вЂ” [`crate::production::
/// deploy_construction_site`] (`DeployConstructionSiteUtilities` +
/// `PrepareConstructionSite`): tape/cones/slab on the `target_grid` cell.
/// The function is idempotent: `ProductionManager::commit` (the `Done` click in
/// `BuildMenu`) and `events::apply_command` (packet 08/P4 after a map click)
/// already place the site synchronously, so here on the next tick there is no
/// repeated spawn вЂ” only `ORDER_STATE_BUILD_IN_PROGRESS` and `PositionInTape`.
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
        let _ =
            production::deploy_construction_site(state, unit.id, unit_to_build, unit.target_grid);

        unit.moved = 0;
        unit.state = UnitOrderState::BuildInProgress;

        if unit.unit_type == UnitType::Constructor {
            set_new_order_int(
                unit,
                UnitOrder::AwaitTapePositioning,
                UnitOrderState::TapePositioningInit,
            );
        } else {
            // `DrawSpriteFrame(image_index + 16)` вЂ” the engineer's working pose;
            // the frame is chosen by `max_app::render::unit_frame_index` from
            // `ORDER_BUILD`/`BUILD_IN_PROGRESS`.
        }

        // `ClearUnitList()` вЂ” removing the group order (P5 access).
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
/// `INIT в†’ IN_PROGRESS в†’ BUILD_IN_PROGRESS`, and the constructor also
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
    // completion, `CancelBuilding` вЂ” on cancellation. An empty queue means that
    // `AWAIT_TAPE_POSITIONING` is already the builder **leaving** a completed site
    // (`TAPE_POSITIONING_DEINIT`, `ActivateEngineer`), not construction: utilities and the slab
    // under the completed building must not be removed.
    if unit.build_list.is_empty() {
        return None;
    }

    Some(unit.target_grid)
}

// Moving a mobile builder away from an unfinished site is **impossible** without an explicit
// cancellation вЂ” this is an excluded path from C++.
//
// In C++ a movement order to the builder during construction is not issued via the map
// (`GameManager_GetBuilderUnitCursor`, `game_manager.cpp:5927-5931`: for
// `BUILD_IN_PROGRESS` the cursor is `CURSOR_UNIT_NO_GO`), and cancellation goes only through
// the popup `stop` (`UnitsManager_Popup_OnClick_BuildStop` в†’
// `GameManager_SelectBuildSite` в†’ `ORDER_BUILD`/`BUILD_CANCEL` в†’
// `UnitInfo::CancelBuilding`). The former port behavior "auto-cancel on move-away"
// contradicted the oracle and was removed: a new order (packet 08/38, AI, group)
// does not cancel construction вЂ” the order just changes `orders`, and the site/queue
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

/// `UnitInfo::GetTargetUnitAngle` (`unitinfo.cpp:4147`) вЂ” the direction in
/// which the builder enters the tape center and leaves it.
///
/// * `TAPE_POSITIONING_ENTER` вЂ” by cells: the tape is on the builder's cell
///   (`Access_GetConstructionUtility(team, grid_x, grid_y)`), its `grid` is the 2Г—2
///   anchor. Comparing `grid_x/grid_y` with the anchor yields exactly one of the four
///   diagonal corners: both axes match вЂ” `SE`, only `x` вЂ” `NE`, only
///   `y` вЂ” `SW`, otherwise `NW`. A 2Г—2 builder always stands in one of the four
///   tape cells, so normally the angle is always diagonal (45В°).
/// * the other states (`LEAVE`) вЂ” toward `move_to` (C++
///   `move_to_grid_x/move_to_grid_y`, in the port вЂ” `OrderData::move_to`): the sign
///   of the difference on each axis independently; the builder's pixel position вЂ” the center
///   of its cell (`UnitInfo::SetPosition`; in the port `x/y` are not recomputed when
///   walking, so the cell is used).
pub(crate) fn get_target_unit_angle(state: &GameState, unit: &UnitInfo, move_to: Point) -> i32 {
    const UNIT_ANGLE_NE: i32 = 1;
    const UNIT_ANGLE_SE: i32 = 3;
    const UNIT_ANGLE_SW: i32 = 5;
    const UNIT_ANGLE_NW: i32 = 7;

    if unit.state == UnitOrderState::TapePositioningEnter {
        // `Access_GetConstructionUtility(team, grid_x, grid_y)`: for a building
        // the tape is registered in the hash in all four 2Г—2 cells, but its
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

/// The sign of the builder's "entry" into the tape on each axis вЂ” how many pixels it has
/// already traveled from the center of its cell to the site center
/// (`OffsetDrawZones(DIRECTION_OFFSETS[angle] * 8)`, `TAPE_STEPS` ticks).
///
/// For a 2Г—2 tape the builder stands in one of its four cells, so entry is
/// always diagonal: `в€’1` on the side where the anchor matched its cell.
/// For a 1Г—1 tape (engineer) the builder is exactly on its cell вЂ” there is no entry.
fn tape_enter_direction(state: &GameState, unit: &UnitInfo) -> (i32, i32) {
    let Some(tape) =
        production::construction_utility(state, unit.team, unit.grid).and_then(|id| state.unit(id))
    else {
        return (0, 0);
    };

    if tape.unit_type != UnitType::LargeTape
        || !production::site_covers(tape.unit_type, tape.grid, unit.grid)
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
/// `enter + leave >= 1` в†’ `+1`, `<= -2` в†’ `-1`, otherwise the cell does not change.
fn tape_leave_shift(enter: i32, leave: i32) -> i16 {
    match enter + leave {
        sum if sum >= 1 => 1,
        sum if sum <= -2 => -1,
        _ => 0,
    }
}

/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4414`).
///
/// The positioning loop is ported: `INIT в†’ ENTER`, `DEINIT в†’ LEAVE`,
/// final rotation `Paths_UpdateAngle` toward [`get_target_unit_angle`], `moved` up to 4, then
/// `RestoreOrders`. As in C++, while the builder has not turned to the center, `moved`
/// **does not grow** вЂ” first rotation in place, then sliding (four steps
/// `OffsetDrawZones` of 8 px).
///
/// `move_to` вЂ” the exit target (C++ `move_to_grid_x/move_to_grid_y`): on
/// LEAVE completion C++ recomputes the cell from the pixel position
/// (`grid_x = x / GFX_MAP_TILE_SIZE`) вЂ” the builder ends up in the tape cell
/// closest to the target, and then walks (`ActivateEngineer` в†’ `ORDER_MOVE`).
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
            // `PrepareConstructionSite` + `ScaleUnit(this, SHRINK)` вЂ” P3/P5.
            let _ = scale_unit(unit, UnitOrderState::Shrink);
        } else if unit.unit_type == UnitType::Constructor
            && old_state == UnitOrderState::TapePositioningEnter
        {
            // `PrepareConstructionSite` has already been done by `StartBuilding`
            // (the site is placed immediately on order, see
            // `production::deploy_construction_site`). C++ turns the builder once more by 45В°
            // (`Paths_UpdateAngle(this, (angle + 1) & 0x07)`) and
            // draws the working pose `DrawSpriteFrame(image_index + 16)`; the frame
            // is chosen by the renderer from `ORDER_BUILD`/`BUILD_IN_PROGRESS`.
            let target_angle = (i32::from(unit.angle) + 1) & 0x07;
            let _ = paths_update_angle(unit, target_angle);
        } else if old_state != UnitOrderState::TapePositioningEnter {
            // `Hash_MapHash.Remove(this)` в†’ `grid_x = x / GFX_MAP_TILE_SIZE` в†’
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
/// 45В° step). The frame cache (`image_index`/`turret_image_index`/draw zones) вЂ” UI,
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
/// (`moved == GFX_MAP_TILE_SIZE`): the hull makes **one** 45В° step toward
/// the next route step (if any), rather than turning instantly.
/// The next `GroundPath::Execute` finishes turning the unit and only then
/// starts movement вЂ” so rotation and movement are always consistent.
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

/// One arrow of the unit route preview вЂ” `GroundPath::Draw`/`AirPath::Draw`
/// (`groundpath.cpp:436-556`, `airpath.cpp:297-360`) feed
/// `Paths_DrawMarker(window, angle, x, y, color)` (`paths.cpp:376`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathMarker {
    /// Anchor of the marker in **world pixels** (cell center for a ground route).
    pub x: i32,
    /// Anchor in world pixels.
    pub y: i32,
    /// Direction 0..7 (`Paths_GetAngle` order) вЂ” the triangle points this way.
    pub angle: u8,
    /// `true` вЂ” reachable within the current turn's movement points (`COLOR_GREEN`),
    /// `false` вЂ” a later turn (`COLOR_BLUE`).
    pub this_turn: bool,
}

/// The marker triangle vertices for one direction (`Paths_8DirPointsArrayMarkerA/B/C`,
/// `paths.cpp:37-44`), in world pixels at zoom 64.
pub const PATH_MARKER_POINTS: [[(i32, i32); 3]; 8] = [
    [(0, -10), (-10, 10), (10, 10)],
    [(8, -8), (-12, 0), (0, 12)],
    [(10, 0), (-10, -10), (-10, 10)],
    [(8, 8), (0, -12), (-12, 0)],
    [(0, 10), (10, -10), (-10, -10)],
    [(-8, 8), (12, 0), (0, -12)],
    [(-10, 0), (10, 10), (10, -10)],
    [(-8, -8), (0, 12), (12, 0)],
];

/// Route preview markers of a unit вЂ” `GroundPath::Draw` (`groundpath.cpp:436`) and
/// `AirPath::Draw` (`airpath.cpp:297`).
///
/// The marker colors encode the turn in which the unit reaches the cell: C++ walks
/// the remaining path spending the **current** movement points
/// (`limited_speed = speed * 4 + move_fraction`, the quarter-point scale of
/// `Access_IsAccessible`, diagonal steps cost 1.5Г—); once the points run out the cell
/// is drawn `COLOR_BLUE` (a later turn) and the next turn refills them from
/// `ATTRIB_SPEED * 4`. Cells reachable this turn are `COLOR_GREEN`.
///
/// `GameManager_RenderScanRangeIndicators` (`game_manager.cpp:936-951`) calls the
/// path's `Draw` for the **selected** unit when it is visible to the player and
/// belongs to them; the caller applies that gate.
pub(crate) fn path_markers(state: &GameState, unit: &UnitInfo) -> Vec<PathMarker> {
    let Some(path) = unit.path.as_ref() else {
        return Vec::new();
    };

    // Raw movement points of the unit type (`ATTRIB_SPEED`): the ground route works in
    // quarter-points (`Г— 4`, `groundpath.cpp:466`), an air route in whole points.
    let base_speed = unit.base_values.get_attribute(ATTRIB_SPEED).max(0);

    match path {
        UnitPath::Ground(ground) => {
            let mut limited_speed = if unit.group_speed != 0 {
                i32::from(unit.group_speed) - 1
            } else {
                i32::from(unit.speed)
            };
            limited_speed = limited_speed * 4 + i32::from(unit.move_fraction);

            let mut grid = unit.grid;
            let mut step = (0i32, 0i32);
            let mut markers = Vec::new();
            let steps = ground
                .steps
                .get(ground.step_index as usize..)
                .unwrap_or(&[]);

            for index in 0..=steps.len() {
                if index != steps.len() {
                    step = (i32::from(steps[index].x), i32::from(steps[index].y));

                    if limited_speed < 0 {
                        limited_speed += base_speed * 4;
                    }

                    grid = Point::new(
                        grid.x.wrapping_add(step.0 as i16),
                        grid.y.wrapping_add(step.1 as i16),
                    );

                    let mut cost =
                        access::is_accessible_unit(state, unit, grid, access::ACCESS_MODIFIER_NONE)
                            as i32;

                    if step.0 != 0 && step.1 != 0 {
                        cost = (cost * 3) / 2;
                    }

                    limited_speed -= cost;
                } else {
                    // `groundpath.cpp:496` вЂ” the destination marker of a route is
                    // always "this turn" (the loop forces `limited_speed = -1`).
                    limited_speed = -1;
                }

                if step.0 != 0 || step.1 != 0 {
                    markers.push(PathMarker {
                        x: i32::from(grid.x) * 64 + 32,
                        y: i32::from(grid.y) * 64 + 32,
                        angle: paths_get_angle(i32::from(step.0), i32::from(step.1)) as u8,
                        this_turn: limited_speed < 0,
                    });
                }
            }

            markers
        }
        UnitPath::Air(air) => {
            if unit.max_velocity == 0 {
                return Vec::new();
            }

            let steps = i32::from(air.length) / i32::from(unit.max_velocity);

            if steps == 0 {
                return Vec::new();
            }

            let mut max_speed = if unit.group_speed != 0 {
                i32::from(unit.group_speed) - 1
            } else {
                i32::from(unit.speed)
            };
            let mut step_x = air.step_x;
            let mut step_y = air.step_y;
            let mut markers = Vec::new();

            for _ in 0..=steps {
                markers.push(PathMarker {
                    x: (step_x >> 16) + i32::from(air.start_x),
                    y: (step_y >> 16) + i32::from(air.start_y),
                    angle: air.angle as u8,
                    this_turn: max_speed == 0,
                });

                if max_speed == 0 {
                    max_speed = base_speed;
                }

                max_speed -= 1;
                step_x = step_x.wrapping_add(i32::from(unit.max_velocity) * air.delta_x);
                step_y = step_y.wrapping_add(i32::from(unit.max_velocity) * air.delta_y);
            }

            markers
        }
        UnitPath::Builder(_) => Vec::new(),
    }
}

///
/// Returns `false` if the unit lacks speed (the step does not start).
/// `real_time` вЂ” the `GameManager_RealTime` setting: the step does not spend `speed`.
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

/// `Paths_TakeStep` (`airpath.cpp:458`) вЂ” charging one cell of **air** movement.
///
/// `AirPath::Execute` spends `speed` not per pixel but once per
/// `m_length % max_velocity == 0` step (`airpath.cpp:272-278`): `m_length` counts
/// quarter-cells (`Paths_GetAirPath`: `length = steps_distance * max_velocity`,
/// `max_velocity = 4`), so exactly **one** `speed` point is spent per crossed cell.
/// In the port's one-cell-per-tick model ([`air_path_execute`]) the call therefore
/// happens once per cell вЂ” the aircraft flies `speed` cells, then the `speed == 0`
/// check at the top of `AirPath::Execute` (`airpath.cpp:186-199`) stops it.
///
/// Unlike [`paths_calculate_step`] (ground: `Paths_CalculateStep`, `paths.cpp:292`)
/// there is no `move_fraction`/`max_velocity` recalculation and no "not enough
/// speed" branch. `real_time` вЂ” the `GameManager_RealTime` debug setting: the step
/// costs nothing.
pub fn paths_take_step(unit: &mut UnitInfo, cost: i32, real_time: bool) {
    let cost = if real_time { 0 } else { cost };

    unit.speed = i32::from(unit.speed).wrapping_sub(cost) as u16;

    if unit.group_speed != 0 {
        let group_speed = i32::from(unit.group_speed) - cost;
        unit.group_speed = group_speed.max(1) as u16;
    }

    let base = unit.base_values;

    if base.get_attribute(ATTRIB_MOVE_AND_FIRE) == 0 {
        let rounds = base.get_attribute(ATTRIB_ROUNDS);
        let speed = base.get_attribute(ATTRIB_SPEED);

        // C++ divides by `ATTRIB_SPEED` unguarded; the port keeps the
        // `speed != 0` guard of [`paths_calculate_step`] (a type table entry with
        // zero speed would be a data error, not a rule).
        if speed != 0 {
            let shots = (rounds * i32::from(unit.speed)) / speed;

            if shots < i32::from(unit.shots) {
                unit.shots = shots as u8;
            }
        }
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

    /// `UnitInfo::Land` (`unitinfo.cpp:3829`) including the presentation part.
    ///
    /// C++ `Land()` shifts `shadow_offset` by `+8` on every animation step
    /// (`unitinfo.cpp:3848-3849`) and on the final step clears `HOVERING` and calls
    /// `Redraw()` (`:1452-1459`), which sets the offset from the flag вЂ” `(-64, -64)`
    /// for a hovering aircraft, `(0, 0)` otherwise. The offset lives in
    /// [`max_types::unit::UnitPresentation`] (the C++ `UnitInfo` field, V70), so the
    /// update goes through the presentation table, not [`UnitInfo`].
    pub(crate) fn land(&mut self) -> bool {
        let result = land(&mut self.unit);
        let flags = self.unit.flags;

        if let Some(presentation) = self.world.presentation_mut(self.id) {
            if result {
                // `Redraw()` on the final step.
                presentation.shadow_offset = air_shadow_offset(flags);
            } else {
                presentation.shadow_offset.x += 8;
                presentation.shadow_offset.y += 8;
            }
        }

        result
    }

    /// `UnitInfo::Take` (`unitinfo.cpp:3862`) including the presentation part.
    ///
    /// C++ `Take()` shifts `shadow_offset` by `-16` per animation step
    /// (`unitinfo.cpp:3879-3880`): four steps accumulate `-64`, the final step only
    /// sets `HOVERING` (no `Redraw`), which is exactly the `Redraw()` value of a
    /// hovering aircraft.
    pub(crate) fn take(&mut self) -> bool {
        let result = take(&mut self.unit);

        if !result {
            if let Some(presentation) = self.world.presentation_mut(self.id) {
                presentation.shadow_offset.x -= 16;
                presentation.shadow_offset.y -= 16;
            }
        }

        result
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

        // `Ai_CheckMines(this)` (`ai.cpp:365`) вЂ” the minelayer reports to teams.
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
        // cell вЂ” the parent strikes, the missile itself is destroyed.
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
            // (`MISSILE_UNIT`) steps **pixel by pixel** along the line `m_start_* в†’
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
            // `ClearUnitList()` вЂ” removing the group order (P5 access).
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

/// `AirPath::Execute` (`airpath.cpp`) вЂ” a step toward the end cell by `m_angle`;
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
            // `AirPath::Execute` (`airpath.cpp:281-283`): the last pixel step calls
            // `Paths_FinishMove` вЂ” the arrival is not a plain "blocked" end of move.
            paths_finish_move(ctx);
        }
        return false;
    }

    if ctx.unit.is_air() {
        // C++ `AirPath::Execute` (`airpath.cpp:186-199`): the speed and engine
        // check is **inside** `if (unit->flags & MOBILE_AIR_UNIT)`, and the speed
        // compared is the usable one (with a group order вЂ” `group_speed - 1`). For
        // projectiles (`MISSILE_UNIT`, `speed = 0` from `SC_ATTRI`) there is no
        // such check вЂ” they fly by `m_length` and finish with `Paths_FinishMove`.
        let mut speed = ctx.unit.speed;

        if ctx.unit.group_speed > 0 {
            speed = ctx.unit.group_speed - 1;
        }

        if speed == 0 || ctx.unit.engine == 0 {
            ctx.unit.state = UnitOrderState::ExecutingOrder;
            ctx.move_finished(true);
            return false;
        }
    }

    if paths_update_angle(&mut ctx.unit, paths_get_angle(dx, dy)) {
        return false;
    }

    if ctx.unit.is_air() {
        // `Paths_TakeStep(unit, 1)` (`airpath.cpp:272-278`): one `speed` point per
        // crossed cell. Without it an aircraft flew its whole `speed` allowance on
        // every order and never ran out of it within a turn (defect 3 of the
        // manual 1.2 run).
        paths_take_step(&mut ctx.unit, 1, ctx.world.real_time);
    }

    ctx.unit.state = UnitOrderState::InTransition;
    ctx.unit.in_transit = true;
    ctx.unit.moved = 0;
    true
}

/// `Paths_FinishMove` (`airpath.cpp:408-456`) вЂ” the air path is exhausted
/// (`m_length == 0`): the unit lands into a bay, starts loading into a transport, or
/// finishes its move.
///
/// The port used to go straight to [`OrderContext::blocked_on_path_request`], which is
/// the last resort of the C++ function only: an air transport that reached the passenger
/// it was sent to load stayed in `ORDER_AWAIT` with `parent = passenger` and the
/// passenger was never taken aboard.
///
/// Divergence from C++: the `path->GetEndX() != grid_x` branch (re-requesting an air path
/// for the rest of a speed-limited route) is not reachable here вЂ” [`air_path_execute`]
/// reaches its end cell along `path.end_x/end_y` or stops earlier on `speed == 0`, so
/// "at the path end" always means "at the order destination".
fn paths_finish_move(ctx: &mut OrderContext<'_>) {
    // `unit->Redraw()` вЂ” the shadow of a hovering aircraft; the presentation layer
    // recomputes it from `HOVERING` (`air_shadow_offset`).
    ctx.unit.path = None;

    // `if (!UnitsManager_LoadUnit(unit)) { вЂ¦ unit->BlockedOnPathRequest(); }`
    if load_unit(ctx) {
        return;
    }

    if ctx.unit.unit_type == UnitType::AirTransport {
        if let Some(passenger_id) = ctx.unit.parent_unit {
            if i32::from(ctx.unit.storage)
                < ctx
                    .unit
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_STORAGE)
            {
                if access::get_team_unit(ctx.world, ctx.unit.grid, ctx.unit.team, MOBILE_LAND_UNIT)
                    == Some(passenger_id)
                {
                    // `unit->SetOrder(ORDER_LOAD); unit->SetOrderState(ORDER_STATE_INIT)`
                    // (plain setters in C++, not `UnitsManager_SetNewOrder`).
                    ctx.unit.orders = UnitOrder::Load;
                    ctx.unit.state = UnitOrderState::Init;

                    return;
                }
            } else {
                // The hold is full: the transport is no longer waiting for this passenger.
                ctx.unit.parent_unit = None;
            }
        }
    }

    ctx.blocked_on_path_request(true);
}

/// `UnitsManager_LoadUnit` (`units_manager.cpp:5027`) вЂ” an air unit that finished its
/// path over its own landing pad or hangar starts landing instead of stopping.
///
/// Returns `true` when the order was taken over; the caller then skips the rest of
/// `Paths_FinishMove`.
fn load_unit(ctx: &mut OrderContext<'_>) -> bool {
    let grid = ctx.unit.grid;
    let Some(shop_id) = access::get_receiver_unit(ctx.world, &ctx.unit, grid) else {
        return false;
    };
    let Some(shop) = ctx.other(shop_id) else {
        return false;
    };

    if shop.unit_type == UnitType::LandingPad
        && access::get_unit2(ctx.world, grid, ctx.unit.team).is_none()
    {
        ctx.unit.parent_unit = None;
        ctx.unit.orders = UnitOrder::Land;
        ctx.unit.state = UnitOrderState::Init;

        return true;
    }

    if shop.unit_type == UnitType::Hangar && ctx.unit.orders == UnitOrder::MoveToUnit {
        let stored_units = access::get_stored_unit_count(ctx.world, &shop);
        let storable_units = shop
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_STORAGE);

        if stored_units == storable_units {
            ctx.unit.orders = UnitOrder::Await;
            ctx.unit.state = UnitOrderState::ExecutingOrder;
        } else {
            ctx.unit.parent_unit = Some(shop_id);
            ctx.unit.orders = UnitOrder::Land;
            ctx.unit.state = UnitOrderState::Init;
        }

        return true;
    }

    false
}

/// `BuilderPath::Execute` (`builderpath.cpp`) вЂ” the builder's step to the target.
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
/// unit->OffsetDrawZones(offset_x, offset_y);  // :240 вЂ” x/y := position
/// unit->grid_x = unit->x >> 6;                 // :241-242
/// --m_length;                                  // :244
/// ```
///
/// `m_delta_*` вЂ” the 22-bit step fraction (pixels/tick), `m_start_*` вЂ” the pixel
/// of the launch cell center, `m_length` вЂ” the number of ticks (в‰€ the number of cells of distance).
/// Result: the projectile traverses **the same segment** as in the original, not an 8-direction
/// polyline over cells. The cell (`grid`) is recomputed from the pixel, and
/// the trail (`Paths_DrawMissile`) is placed from "was в†’ became" вЂ” also along the line.
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
        // pixel в†’ new pixel".
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
    // `UnitsManager_EndAttack` в†’ `parent->Attack(grid_x, grid_y)` в†’
    // `UnitsManager_DestroyUnit(unit)`.
    ctx.unit.path = None;

    if let Some(parent_id) = ctx.unit.parent_unit {
        let position = ctx.unit.grid;
        ctx.world.end_attack(ctx.id);
        combat::attack(ctx.world, parent_id, position);
        ctx.world.destroy_unit(ctx.id);
    }
}

/// `Paths_DrawMissile` (`airpath.cpp:362-406`) вЂ” the trail behind a flying projectile.
///
/// C++ on every projectile cell transition (`AirPath::Execute`,
/// `airpath.cpp:229-232`, the `grid_x || grid_y` branch for `MISSILE_UNIT`) calls
/// `Paths_DrawMissile` and places **three** effect units on the cell the projectile
/// leaves:
/// * `ROCKET` в†’ `RKTSMOKE` (5 frames `D_RKTSMOKE`), `TORPEDO` в†’ `TRPBUBLE`
///   (3 frames `D_TRPBUBLE`);
/// * `UnitsManager_DeployUnit(..., skip_map_status_update = true)` with flags from
///   `SC_UNITS`: both have `EXPLODING|ANIMATED|MISSILE_UNIT` (`TRPBUBLE` also
///   `GROUND_COVER`), so `UnitInfo` immediately gets
///   `ORDER_EXPLODE`/`ORDER_STATE_DESTROY` (`units_manager.cpp:2390-2393`);
/// * frames are advanced by `UnitsManager_ProgressExplosion` (`units_manager.cpp:4006`):
///   `moved & 1` в†’ `image_index + 1`, removal at `image_index ==
///   `image_index_max` (4 for smoke, 2 for bubbles);
/// * `OffsetDrawZones` spreads the three particles along the segment "old projectile pixel
///   в†’ new pixel" at 0, 1/3 and 2/3 вЂ” this is drawing only (`unit->x/y` of the
///   particles themselves does not change).
///
/// The port stores the spread in the particles' pixel `x/y` (the effect renderer takes them
/// instead of the cell), the cell itself stays the C++ one (left). The spawn does not depend
/// on visibility: in C++ `Paths_DrawMissile` is called only with
/// `IsVisibleToTeam(GameManager_PlayerTeam)`, but `GameManager_PlayerTeam` is a
/// local UI field, and binding to it would diverge client states
/// (DESIGN В§6.1). Particle visibility is set by `UnitInfo::InitStealthStatus`
/// (own team; `skip_map_status_update = true`), so foreign trails are not
/// drawn вЂ” as in C++.
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
            // Visibility вЂ” like `UnitInfo::InitStealthStatus` at
            // `UnitsManager_DeployUnit`: the particle is seen only by its own team
            // (`Paths_DrawMissile` skips `Access_UpdateMapStatus`).
            state.write_unit(particle_id, particle);
        }
    }
}

/// `UnitInfo::Move` (`unitinfo.cpp:1730`) вЂ” completing a cell transition.
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

    // `GroundPath::Execute` (`groundpath.cpp:327-360`): an amphibious unit
    // switches its sprite block with the terrain it just entered вЂ” the carrier
    // submerges on water and surfaces on land, `SUBMARNE` keeps its underwater
    // block (the surfaced one is set by `SpotByTeam`).
    if ctx.unit.flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT) == (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT)
    {
        let base = terrain_image_base(ctx.world, &ctx.unit, target);

        if ctx.unit.image_base != base {
            ctx.unit.image_base = base;
            ctx.unit.image_index = base.wrapping_add(i16::from(ctx.unit.angle));

            if let Some(presentation) = ctx.world.presentation_mut(ctx.id) {
                presentation.firing_image_base =
                    if ctx.unit.unit_type == UnitType::ArmouredPersonnelCarrier {
                        base
                    } else {
                        base.wrapping_add(16)
                    };
            }
        }
    }

    // Projectiles do not reach here: [`path_execute`] moves them pixel by pixel
    // ([`missile_path_execute`]) and the trail is placed from the pixel segment
    // (`Paths_DrawMissile`, `airpath.cpp:229-232`).

    // C++ `Move()` at `moved == GFX_MAP_TILE_SIZE`:
    // `path_steps[step_index]->x -= offset_x` вЂ” the step is "eaten" by the transition.
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

    // C++ `UnitInfo::Move` (`unitinfo.cpp:1835-1844`): when a cell is completed and
    // the order is `ORDER_BUILD` with a non-empty queue, the builder starts the next
    // building on the cell it just entered вЂ” `UnitInfo::Build`:
    // `IssueBuildOrder(this, &grid_x, &grid_y, *build_list[0])` (the four anchors
    // around the builder, `units_manager.cpp:4998`), then `build_time` from
    // `BuildMenu_GetTurnsToBuild` and `StartBuilding()` at that site. This is the
    // repeated construction of a `Path` order (`GameManager_PathBuild`).
    if ctx.unit.orders == UnitOrder::Build && !ctx.unit.build_list.is_empty() {
        let head = ctx.unit.build_list[0];

        if let Some(site) =
            production::initial_build_site(ctx.world, ctx.unit.team, head, ctx.unit.grid, &[ctx.id])
        {
            ctx.unit.target_grid = site;
            ctx.data.move_to = site;
            ctx.unit.build_time = production::turns_to_build(ctx.world, ctx.unit.team, head)
                .clamp(0, i32::from(u8::MAX)) as u8;
            ctx.unit.state = UnitOrderState::BuildInProgress;

            // `DeployConstructionSiteUtilities` + `PrepareConstructionSite`.
            let _ = production::deploy_construction_site(ctx.world, ctx.id, head, site);

            if ctx.unit.unit_type == UnitType::Constructor {
                // `StartBuilding`: the 2Г—2 constructor first centers on the tape.
                ctx.set_new_order(
                    UnitOrder::AwaitTapePositioning,
                    UnitOrderState::TapePositioningInit,
                );
            }
        } else {
            // `UnitInfo::Build` with a path: `IssueBuildOrder` failed (no passable
            // anchor around the builder) в†’ `SetParent(nullptr)` +
            // `ClearBuildListAndPath`.
            ctx.unit.parent_unit = None;
            clear_build_list_and_path(&mut ctx.unit);
        }
    }

    if ctx.unit.has_move_order() {
        // `FollowUnit()` вЂ” group order (P5 access).
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
        // C++ `UnitsManager_PerformAutoSurvey` (`units_manager.cpp:2584-2603`):
        // `if (!unit->GetTask()) Ai_EnableAutoSurvey(unit);` вЂ” the task is created
        // **once**; the surveyor then owns it and is re-entered through the
        // "movement finished" reminder. The port used to create a fresh
        // `TaskAutoSurvey` on every tick (two tasks per move), so the surveyor
        // accumulated tasks and lost the spiral state.
        if ctx.world.ai.task_manager.unit_task(ctx.id).is_none() {
            ctx.world.ai_enable_auto_survey(ctx.id);
        }

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
            //   DeployConstructionSiteUtilities(MININGST) вЂ” P5 access;
            //   SetOrderState(PROGRESS_TRANSFORMING);
            //   SetNewOrderInt(AWAIT_TAPE_POSITIONING, TAPE_POSITIONING_INIT).
            ctx.unit.state = UnitOrderState::ProgressTransforming;
            ctx.set_new_order(
                UnitOrder::AwaitTapePositioning,
                UnitOrderState::TapePositioningInit,
            );
        }
        UnitOrderState::ProgressTransforming => {
            // TODO(P3/P5): `UnitsManager_DeployMasterBuilder` вЂ” destruction
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
/// (`Paths_RequestPath`) вЂ” P5, attack range (`Access_IsWithinAttackRange`) вЂ”
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
        // `Paths_RequestPath(unit, SameClassBlocks)` вЂ” the path to the attack position.
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

/// `Access_GetSquaredDistance` (`access.cpp`) вЂ” the squared distance in cells.
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
        // `FollowUnit()` + `Redraw()` вЂ” P5/UI.
        ctx.unit.state = UnitOrderState::InProgress;

        if ctx.world.network_game && ctx.unit.is_air() {
            // `Remote_SendNetPacket_38(unit)` вЂ” network (max_net), not max_core.
            return false;
        }
    }

    ctx.unit.state == UnitOrderState::InProgress
}

/// `UnitsManager_InitUnitPath` (`units_manager.cpp:4776`).
///
/// The air path is computed immediately (`Paths_GetAirPath`); the ground one is put into
/// the [`pathfinding::request_path`] queue (synchronous pathfinding, DESIGN
/// В§1.4/risk 2), and the order moves to `MOVE_GETTING_PATH`/`NEW_ORDER` until
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

/// Linear route (`GroundPath::AppendLinearSteps`) вЂ” used by
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

    // `ClearUnitList()` вЂ” P5.
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
            // TODO(P3/P5): UnitsManager_BuildClearing вЂ” destruction
            // of the construction, rubble, utilities, unit return.
        }
        UnitOrderState::UnitReady => {
            // C++ `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2891`):
            // `if (unit->path) UnitsManager_BuildNext(unit); else unit->Animate();`
            // вЂ” a builder that completed a building with a **Path** queue starts the
            // next site of the route; without a path it just stands and animates.
            // The exit order is given by the **player** clicking a cell
            // (`game_manager.cpp:6879-6890`: `ORDER_STATE_UNIT_READY` в†’
            // `ORDER_ACTIVATE`/`IN_TRANSITION`), by the AI вЂ” the task
            // `TaskActivate` (`crate::unit_order::issue_builder_exit`).
            if ctx.unit.path.is_some() {
                // `UnitsManager_BuildNext`: continue while material is left and the
                // route end is not reached; otherwise `ClearBuildListAndPath`
                // (handled inside `production::build_next`).
                ctx.sync_to_world();
                let _ = production::build_next(ctx.world, ctx.id);
                ctx.sync_from_world();
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
            // `unit` вЂ” the container (factory/transport), `unit->GetParent()` вЂ”
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

/// `UnitsManager_ActivateUnit` (`units_manager.cpp:3826-3913`) вЂ” bringing a ready
/// unit out of the container.
///
/// Called for the **container**: at a factory `parent_unit` is the ready unit
/// (`UnitInfo::SpawnNewUnit`, `unitinfo.cpp:3513`:
/// `SetParent(factory_unit.Get())`), at a transport вЂ” the passenger. C++
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
/// An air unit exits **by itself** (`ORDER_MOVE`/`INIT` в†’ `ExecuteMove` в†’
/// `ORDER_TAKE_OFF`), a ground/naval one is moved to the click cell. If the
/// cell is occupied/impassable вЂ” the order returns to the deferred one (the factory stays
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
        // An air unit does not need a site вЂ” only fuel/engine:
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

    // `--unit->storage; unit->SetParent(nullptr);` вЂ” the container released the unit.
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
        //  SetOrder(ORDER_MOVE); SetOrderState(ORDER_STATE_INIT);
        //  Access_UpdateMapStatus(client, true); ScaleUnit(EXPAND);`
        let mut client = client;
        client.parent_unit = None;
        client.target_grid = move_to;
        client.orders = UnitOrder::Move;
        client.state = UnitOrderState::Init;
        ctx.world.write_unit(client_id, client);
        ctx.world.update_map_status(client_id, true);

        let mut data = ctx.world.order_data(client_id);
        data.move_to = move_to;
        ctx.world.set_order_data(client_id, data);

        // `UnitsManager_ScaleUnit(client, ORDER_STATE_EXPAND)`: the aircraft
        // "grows" out of the factory (`scaler_adjust` 4 в†’ 0), then
        // `RestoreOrders` returns `ORDER_MOVE`/`INIT` в†’ takeoff.
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

        // `SetSpriteFrameForTerrain` + `InitStealthStatus`
        // (`units_manager.cpp:3884-3900`): the released amphibious unit takes the
        // pose of the cell and its stealth visibility is initialized for the new
        // position. A naval unit released onto a bridge raises it so that it can
        // pass underneath.
        set_sprite_frame_for_terrain(ctx.world, &mut client, move_to);
        combat::init_stealth_status(&mut client, ctx.world.ai.all_visible);

        let client_flags = client.flags;

        ctx.world.write_unit(client_id, client);

        if client_flags & (MOBILE_SEA_UNIT | MOBILE_LAND_UNIT) == MOBILE_SEA_UNIT {
            if let Some(bridge_id) = access::get_bridge(ctx.world, move_to) {
                if let Some(mut bridge) = ctx.world.unit(bridge_id).cloned() {
                    set_new_order_int(&mut bridge, UnitOrder::Move, UnitOrderState::Elevate);
                    ctx.world.write_unit(bridge_id, bridge);
                }
            }
        }

        let mut data = ctx.world.order_data(client_id);
        data.move_to = move_to;
        ctx.world.set_order_data(client_id, data);

        ctx.world.update_map_status(client_id, true);
        ctx.world.scale_unit(client_id, UnitOrderState::Expand);
    }
}

/// `UnitsManager_ActivateEngineer` (`units_manager.cpp:3407`).
fn activate_engineer(ctx: &mut OrderContext<'_>) {
    // `Access_DestroyUtilities(grid, false, false, false, false)` вЂ” only
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

/// `TaskActivate::Activate` вЂ” the first passable cell around the container.
///
/// C++ (`taskactivate.cpp:106-167`): start at
/// `(container.grid_x - 1, container.grid_y + unit_size)`, where `unit_size` is
/// the **container** size (2 for a building, 1 for a transport), then four
/// directions (`N, E, S, W` вЂ” `direction += 2`) for `unit_size + 1` steps
/// cumulatively; the first cell passable for the **exiting** type
/// (`Access_IsAccessible(client->type, вЂ¦, SameClassBlocks)`) becomes the target.
pub(crate) fn activation_exit_cell(
    state: &GameState,
    client: &UnitInfo,
    container: &UnitInfo,
) -> Option<Point> {
    // **Container** size: 2 for a 2Г—2 structure, 1 for 1Г—1 (including a turret вЂ”
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

/// A builder leaves a completed site вЂ” `TaskActivate::Activate`
/// (`taskactivate.cpp:104-167`), the **AI** path.
///
/// In C++ for the player the exit order is born in the click handler
/// (`game_manager.cpp:6879-6890`): `move_to_grid` = the cell under the cursor and
/// `UnitsManager_SetNewOrder(selected, ORDER_ACTIVATE, ORDER_STATE_IN_TRANSITION)`;
/// the port sends the same by packet 08/P3 (`hud::build_builder_exit_order`).
/// The AI has no click: `TaskCreateBuilding::Activate` attaches `TaskActivate`, which
/// searches the first passable cell around the building (`builder_exit_cell`) and issues
/// the same order. `true` вЂ” the order was set.
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
    //  ORDER_STATE_IN_TRANSITION);` вЂ” `prior` = `ORDER_BUILD`/`UNIT_READY`.
    state.set_order(
        builder_id,
        UnitOrder::Activate,
        UnitOrderState::InTransition,
    );

    true
}

/// A factory brings out a ready unit вЂ” `TaskCreateUnit::BeginTurn`
/// (`taskcreateunit.cpp:147-155`) via `TaskActivate::Activate`
/// (`taskactivate.cpp:135-144`), the **AI** path.
///
/// For the player the same order is born by clicking a cell at the factory
/// (`game_manager.cpp:6918-6926`, `CURSOR_ACTIVATE` branch) вЂ” the port sends it
/// by packet 08/P3 (`hud::build_activate_order`). The AI has no click: the task searches
/// the first passable cell around the factory ([`activation_exit_cell`]) and sets
/// **the factory** to `ORDER_ACTIVATE`/`ORDER_STATE_EXECUTING_ORDER` (at the factory
/// `parent_unit` is the ready unit, `UnitInfo::SpawnNewUnit`,
/// `unitinfo.cpp:3513`), after which `UnitsManager_ActivateUnit` brings the unit out.
///
/// `true` вЂ” the order was set.
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

    // `unit_parent->SetParent(unit_to_activate)` вЂ” at the factory the parent is already the ready
    // unit (`SpawnNewUnit`), the link does not change.
    state.set_order(
        factory_id,
        UnitOrder::Activate,
        UnitOrderState::ExecutingOrder,
    );

    true
}

/// `UnitsManager_BuildingReady` (`units_manager.cpp:4183`) вЂ”
/// `UnitsManager_ProcessOrderIdle` for a completed building.
///
/// The building stands on the site in `ORDER_IDLE`/`BUILDING_READY` (and is not drawn)
/// until its builder parent leaves the building footprint. Then
/// the tape and cones are removed (the slab remains with `remove_slabs = false` as the
/// foundation), orders are restored, `scaler_adjust = 4`, and the building
/// unfolds via `ScaleUnit(EXPAND)` вЂ” exactly the C++ order.
fn building_ready(ctx: &mut OrderContext<'_>) {
    let parent = ctx.unit.parent_unit.and_then(|id| ctx.other(id));
    let is_found;

    match parent {
        Some(parent) if parent.orders != UnitOrder::Idle => {
            if parent.hits > 0 {
                if parent.orders == UnitOrder::Move
                    && parent.state != UnitOrderState::ExecutingOrder
                {
                    // The builder is still moving вЂ” the building waits.
                    is_found = false;
                } else {
                    is_found = !production::unit_bounds_contains(&ctx.unit, parent.grid);

                    // The builder returned into the 2Г—2 footprint (the path did not take it
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

    // `Access_UpdateMapStatus(unit, true)` вЂ” coverage is recomputed by
    // `GameState::write_unit` on order/cell change; a separate call is not
    // required in the port.
}

/// `UnitInfo::ReturnToConstructionSite` (`unitinfo.cpp:4488`).
fn return_to_construction_site(ctx: &mut OrderContext<'_>, builder: UnitInfo) {
    // A builder without speed will not move: leave the building waiting
    // (otherwise the repeats "return to center в†’ leave" spin without progress and do not
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
    // event вЂ” the target id; resource/volume is chosen by the client (`max_app::sfx`).
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
    // visibility to the effect вЂ” otherwise the hit animation is not drawn for the attacker.
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

/// Simulation steps per explosion frame.
///
/// Oracle: `UnitsManager_ProgressExplosion` (`units_manager.cpp:4030-4036`) advances
/// the frame only when `moved & 0x01` вЂ” **one frame per two order ticks**. The
/// oracle's order tick is set by `GameManager_IsTickDue` (`game_manager.cpp:5103`):
/// `TickTimer_HaveTimeToThink(TIMER_FPS_TO_MS(24))`, i.e. `1000/24 в‰€ 42 ms`
/// (`timer.h:27`), so a frame lasts `2 Г— 42 ms в‰€ 83 ms` and `LNDEXPLD` (14 frames)
/// is visible for `28 Г— 42 ms в‰€ 1.17 s`; `BLDEXPLD` (28 frames) for ~2.3 s.
///
/// The port's simulation step is fixed at 60 Hz (`max_app::presentation::SIMULATION_DT`),
/// so one oracle tick is 2.5 port steps and the same wall-clock frame rate needs one
/// frame per five simulation steps. Advancing on `moved & 0x01` here made every
/// explosion 2.5Г— too fast (the "explosions of buildings/units are too fast" playtest
/// defect). The step count enters the snapshot (`moved`, `image_index`, the effect's
/// lifetime), so this legitimately changes the CRC of a scenario that has explosions;
/// the wall-clock animation now matches the oracle.
pub const EXPLOSION_FRAME_STEPS: u32 = 5;

/// `UnitsManager_ProgressExplosion` (`units_manager.cpp:4006`).
fn progress_explosion(ctx: &mut OrderContext<'_>) {
    if ctx.unit.image_index == ctx.unit.image_index_max {
        ctx.world.destroy_unit(ctx.id);
    } else {
        // `units_manager.cpp:4015-4023`: in the middle of the animation the explosion releases
        // the parent (the dead unit); if its `hits == 0`, it is deleted by
        // `UnitsManager_BuildClearing(parent, true)`. Without this the dead unit
        // would remain in `ORDER_STATE_DESTROY` forever, and `UnitsManager_ProcessOrder`
        // would set `OrdersPending` every tick вЂ” the AI turn would not close.
        //
        // `BuildClearing` (not `DestroyUnit`) вЂ” this is exactly where C++ leaves
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

        // `units_manager.cpp:4030-4036`: one frame per [`EXPLOSION_FRAME_STEPS`]
        // simulation steps вЂ” the oracle's two order ticks.
        if ctx.unit.moved % (EXPLOSION_FRAME_STEPS as u8) == (EXPLOSION_FRAME_STEPS as u8) - 1 {
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
    let landed = ctx.land();
    if !landed {
        return;
    }

    let Some(parent_id) = ctx.unit.parent_unit else {
        return;
    };
    let Some(mut client) = ctx.other(parent_id) else {
        return;
    };

    // `UnitsManager_ProgressUnloading` (`units_manager.cpp:4035-4069`):
    //   Access_TriggerEnemyMine(client->team, unit->grid_x, unit->grid_y);
    //   UnitsManager_UpdateMapHash(client, unit->grid_x, unit->grid_y);
    //   client->RestoreOrders(); client->SetSpriteFrameForTerrain(unit->grid);
    //   Access_UpdateMapStatus(client, true); UnitsManager_ScaleUnit(EXPAND);
    // The unloaded unit was removed from `Hash_MapHash` when it was stored, so
    // the cell of the **transport** (not the stale loading cell) must be added
    // again: otherwise the released unit is invisible and does not block paths.
    let landing_grid = ctx.unit.grid;
    let landing_team = client.team;
    access::trigger_enemy_mine(ctx.world, landing_team, landing_grid);

    ctx.world
        .map_hash_remove(parent_id, client.grid, client.flags);

    // `UnitsManager_UpdateMapHash(client, unit->grid)` moves the entry. When the
    // unload cell equals the stale loading cell (`write_unit` then skips the
    // hash), the unit still has to be added again вЂ” it was removed from the hash
    // when it was stored; adding it twice when the cell did change would
    // duplicate the id, because `MapHash::Add` does not deduplicate.
    let cell_changed = client.grid != landing_grid;

    client.grid = landing_grid;
    client.target_grid = landing_grid;
    client.x = (i32::from(landing_grid.x) * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2) as u16;
    client.y = (i32::from(landing_grid.y) * GFX_MAP_TILE_SIZE + GFX_MAP_TILE_SIZE / 2) as u16;

    if !cell_changed {
        ctx.world
            .map_hash_add(parent_id, landing_grid, client.flags);
    }

    restore_orders(&mut client);
    set_sprite_frame_for_terrain(ctx.world, &mut client, landing_grid);

    ctx.world.write_unit(parent_id, client);
    ctx.world.update_map_status(parent_id, true);

    ctx.scale_other(parent_id, UnitOrderState::Expand);

    ctx.unit.storage = ctx.unit.storage.saturating_sub(1);
    ctx.unit.moved = 0;
    ctx.unit.state = UnitOrderState::FinishUnloading;
}

/// `UnitInfo::ProcessUnloading` (`unitinfo.cpp:5184`).
fn process_unloading(ctx: &mut OrderContext<'_>) {
    if ctx.take() {
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
        // state = IN_PROGRESS, for LRGTAPE вЂ” AWAIT_TAPE_POSITIONING.
        // TODO(P3/P5): DeployUnit(LRGCONES/LRGTAPE/SMLCONES/SMLTAPE).
        ctx.unit.state = UnitOrderState::InProgress;

        // The large tape (`LRGTAPE`) вЂ” only for a 2Г—2 parent (footprint, not
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
    if ctx.land() {
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

            if ctx.take() {
                let id = ctx.id;
                remove_from_list(ctx.world, id, ctx.unit.unit_list);
                ctx.world.add_to_draw_list(id);
                ctx.restore_orders();
            }
        }
        UnitOrderState::FinishTakeOff => {
            if ctx.take() {
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

    if !ctx.land() {
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

                // `Hash_MapHash.Remove(parent)` + `Access_UpdateMapStatus(parent, false)`
                // (`units_manager.cpp:4148-4156`): the stored unit leaves the map,
                // otherwise it kept blocking/drawing on the loading cell.
                ctx.world
                    .map_hash_remove(parent_id, parent.grid, parent.flags);
                ctx.world.write_unit(parent_id, parent);
                ctx.world.update_map_status(parent_id, false);

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
    if ctx.take() {
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
    // we re-read the unit after the processor вЂ” otherwise the `storage`/order changes
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
        // the flag set вЂ” as in `CancelBuilding` (`true, false, false, false`).
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

/// `UnitsManager_AttemptStealthAction` вЂ” a thin wrapper over
/// [`combat::attempt_stealth_action`] (see the call in the order processor).
#[allow(dead_code)]
fn attempt_stealth_action(ctx: &mut OrderContext<'_>) -> bool {
    combat::attempt_stealth_action(ctx.world, &mut ctx.unit)
}

/// `UnitsManager_ProcessOrderDisable` (`units_manager.cpp:3226`).
fn process_order_disable(ctx: &mut OrderContext<'_>) {
    // `unit->Animate()` вЂ” UI; the state does not change while disable turns
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
        // The mine is visible to all surveyor-unit teams in 3Г—3 (`SpotByTeam`).
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

/// `UnitsManager_ProcessOrder` (`units_manager.cpp:3257`) вЂ” dispatcher of 32
/// orders.
///
/// The mappings are preserved verbatim, including `ORDER_MOVE_TO_UNIT`/`_TO_ATTACK` в†’
/// `ProcessOrderMove` and `ORDER_HALT_BUILDING_2` в†’ `ProcessOrderHaltBuilding`.
/// `ORDER_17` (0x12) has no `switch` branch in C++ вЂ” also unhandled.
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
        // `SpinningTurretAdvanceAnimation` вЂ” UI.
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

#[cfg(test)]
#[path = "unit_order_tests.rs"]
mod tests;
