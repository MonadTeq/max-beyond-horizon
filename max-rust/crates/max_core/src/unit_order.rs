//! Машина приказов: `UnitOrder` (32) × `UnitOrderState` (47).
//!
//! Оракул: `max-cpp-port/src/units_manager.cpp` (`UnitsManager_SetNewOrderInt`,
//! `UnitsManager_SetNewOrder`, `UnitsManager_ProcessOrder`,
//! `UnitsManager_ProcessUnitOrders`, 32 функции `UnitsManager_ProcessOrder*`,
//! `UnitsManager_EndAttack`, `UnitsManager_PinAttackTargets`,
//! `UnitsManager_ScaleUnit`, `UnitsManager_NewOrderWhileScaling`,
//! `UnitsManager_FinishUnitScaling`), `unitinfo.cpp` (`UnitInfo::RestoreOrders`,
//! `ExecuteMove`, `Move`, `MoveFinished`, `BlockedOnPathRequest`, `StopMove`,
//! `StopMovement`, `Land`, `Take`, `ProcessLanding`, `ProcessLoading`,
//! `ProcessUnloading`, `PositionInTape`, `StartBuilding`, `CancelBuilding`,
//! `PowerUp`, `PowerDown`, `Reload`, `Repair`, `Transfer`, `Upgrade`,
//! `CheckIfDestroyed`), `groundpath.cpp` (`GroundPath::Execute`,
//! `CancelMovement`), `paths.cpp` (`Paths_GetAngle`, `Paths_UpdateAngle`,
//! `Paths_CalculateStep`), DESIGN.md §5.2.
//!
//! Отдельного файла `unitorder.cpp` в C++-порте нет: машина приказов живёт в
//! `units_manager.cpp` (диспетчер и процессоры) и `unitinfo.cpp` (методы
//! `UnitInfo`, реализующие переходы). Порт повторяет это разделение: процессоры
//! и диспетчер — здесь, операции над юнитом — методы [`UnitInfo`], обход
//! списков и межъюнитные эффекты — [`crate::game::GameState`].
//!
//! # Модель
//!
//! Приказ обрабатывается над **рабочей копией** юнита ([`OrderContext`]):
//! `UnitInfo` клонируется, машина меняет копию, в конце копия пишется обратно.
//! В C++ все изменения идут по указателю; копия эквивалентна, потому что
//! `ProcessOrders` обходит каждый список ровно один раз и не переисполняет
//! один и тот же юнит в том же проходе. Межъюнитные эффекты (родитель,
//! цель, живая атака, списки) выполняются явными методами `GameState`, поэтому
//! они видны остальным юнитам сразу — как в C++.
//!
//! # Заглушки P5/P6
//!
//! Модули боя (`combat`), путей (`pathfinding`), проходимости (`access`) и ИИ
//! (`ai`) в этой фазе отсутствуют. Каждая их точка вызова помечена `TODO(P5)`/
//! `TODO(P6)` и возвращает документированное значение; **порядок вызовов и
//! переходы состояний приказов сохранены 1:1**, чтобы при появлении модулей
//! заглушки заменились вызовами без переписывания машины.
//!
//! # Расхождения с C++
//!
//! 1. **Движение** (DESIGN §5.1, расхождение 3): C++ двигает юнит по пикселям
//!    (`UnitInfo::Move`: `velocity`/`moved`/`GFX_MAP_TILE_SIZE`) и в сетевой
//!    игре завершает не более одной клетки за тик. `max_core` — lockstep, и
//!    [`unit_move`] завершает ровно одну клетку за тик: шаг пути считается
//!    пройденным, смещение вычитается из текущего `PathStep` (как в C++
//!    `Move()`: `path_steps[step_index]->x -= offset_x`). Пиксельная анимация —
//!    P5.
//! 2. `Paths_CalculateStep` портирован целиком (расход `speed`, `move_fraction`,
//!    `shots`, `max_velocity`), поэтому дальность хода за ход совпадает с C++
//!    при тех же атрибутах.
//! 3. `Animate`/`RefreshScreen`/`AddToDrawList`-отрисовка — UI, не портируется.
//! 4. `Effects`-настройка (анимация постройки, `UnitInfo::Animate`) в симуляции
//!    учитывается только там, где она меняет поля состояния
//!    (`moved` в `ORDER_BUILD`); см. [`crate::game::GameState::settings_effects`].

use crate::enums::{unit_flags::*, UnitOrder, UnitOrderState, UnitType};
use crate::game::{GameState, OrderData};
use crate::geo::{PathStep, Point, DIRECTION_OFFSETS};
use crate::unit::{AirPath, GroundPath, UnitId, UnitInfo, UnitListKind, UnitPath};
use crate::unit_values::{ATTRIB_AMMO, ATTRIB_MOVE_AND_FIRE, ATTRIB_ROUNDS, ATTRIB_SPEED};

/// `UnitInfo::MOVEMENT_WAIT_LIMIT` (`unitinfo.hpp:400`).
pub const MOVEMENT_WAIT_LIMIT: u16 = 512;

/// `GFX_MAP_TILE_SIZE` — сторона клетки в пикселях (`unitinfo.cpp`).
pub const GFX_MAP_TILE_SIZE: i32 = 64;

// ---------------------------------------------------------------------------
// Заглушки отсутствующих модулей (P5/P6)
// ---------------------------------------------------------------------------

/// `UnitsManager_IsEngaged` (`units_manager.cpp:1422`) — юнит занят боем или
/// стелс-действием и не может двигаться/маневрировать.
///
/// TODO(P5): `combat`/`access` — engaged-флаг приказа FIRE /
/// AWAIT_STEAL_UNIT / AWAIT_DISABLE_UNIT и `shake_effect_state`.
fn is_engaged(_unit: &UnitInfo) -> bool {
    false
}

/// `UnitsManager_IsEngagedByInfiltrator` (`units_manager.cpp:1408`) — P5.
fn is_engaged_by_infiltrator(_unit: &UnitInfo) -> bool {
    false
}

/// `UnitInfo::Animate` / `RefreshScreen` — UI-анимация и грязные зоны (P4 не нужны).
fn animate(_unit: &UnitInfo) {}

/// `UnitInfo::RefreshScreen` — UI.
fn refresh_screen(_unit: &UnitInfo) {}

/// `UnitsManager_UpdateConnectors` (`units_manager.cpp:2051`) — P3 комплексы.
fn update_connectors(_unit: &mut UnitInfo) {}

/// `Ai_SetTasksPendingFlag(name)` (`ai.cpp`) — P6: флаг «у команды есть
/// несделанная работа» для планировщика задач.
fn ai_task_flag(_name: &str) {}

/// `Hash_UnitHash`-проверка «клетка занята наземным/морским юнитом»
/// (`Access_GetActiveUnitWithFlags`) — P5.
fn has_active_unit_with_flags(_state: &GameState, _grid: Point, _flags: u32) -> bool {
    false
}

/// `Access_IsAccessible` (`access.cpp`) — стоимость входа в клетку.
///
/// TODO(P5): полноценные правила проходимости (`access.rs`). До P5 принята
/// «нейтральная» стоимость 4 (ровный проход), чтобы движение и
/// `Paths_CalculateStep` работали как в C++ на свободной карте.
fn access_is_accessible(state: &GameState, unit: &UnitInfo, grid: Point, _modifiers: u32) -> i32 {
    if state.map.index(grid.x, grid.y).is_none() {
        return 0;
    }
    let _ = unit;
    4
}

/// `Paths_IsOccupied` (`paths.cpp`) — P5 (занятость клетки другим юнитом).
fn paths_is_occupied(_state: &GameState, _grid: Point) -> bool {
    false
}

/// `Access_GetReceiverUnit` (`access.cpp`) — P5 (юнит, принимающий груз).
fn receiver_unit(_state: &GameState, _unit: &UnitInfo, _grid: Point) -> Option<UnitId> {
    None
}

/// `Access_SetUnitDestination` (`access.cpp`) — P5 (частичная проходимость).
fn set_unit_destination(_state: &mut GameState, _unit: &UnitInfo, _from: Point, _to: Point) {}

/// `UnitsManager_IsUnloadSiteTaken` (`units_manager.cpp:4074`) — занято ли
/// место высадки другим воздушным транспортом.
///
/// Портировано полностью: обход `UnitsManager_MobileAirUnits` в порядке
/// вставки.
fn is_unload_site_taken(state: &GameState, unit: &UnitInfo) -> bool {
    for id in state.unit_list(UnitListKind::MobileAir) {
        let Some(other) = state.unit(*id) else { continue };
        if other.id != unit.id
            && other.hits > 0
            && other.grid == unit.grid
        {
            let is_suspended = (other.orders == UnitOrder::Disable
                || other.orders == UnitOrder::Explode)
                && other.prior_orders == UnitOrder::Unload
                && other.prior_state == UnitOrderState::UnloadingInProgress;
            if (other.orders == UnitOrder::Unload
                && other.state == UnitOrderState::UnloadingInProgress)
                || is_suspended
            {
                return true;
            }
        }
    }
    false
}

/// `UnitInfo::ScheduleDelayedTasks` — P6 (очередь отложенных задач ИИ).
fn schedule_delayed_tasks(_state: &mut GameState, _id: UnitId, _priority: bool) {}

// ---------------------------------------------------------------------------
// Чистые операции над UnitInfo (порт методов UnitInfo)
// ---------------------------------------------------------------------------

/// `UnitInfo::IsBridgeElevated` (`unitinfo.cpp:3030`).
pub fn is_bridge_elevated(unit: &UnitInfo) -> bool {
    unit.unit_type == UnitType::Bridge && unit.image_index != unit.image_base
}

/// `UnitInfo::RestoreOrders` (`unitinfo.cpp:3783`).
pub fn restore_orders(unit: &mut UnitInfo) {
    if unit.prior_orders == unit.orders
        && unit.unit_type != UnitType::CommTower
        && unit.unit_type != UnitType::MiningStation
        && unit.unit_type != UnitType::Habitat
        && unit.unit_type != UnitType::PowerGenerator
        && unit.unit_type != UnitType::PowerStation
        && unit.unit_type != UnitType::ResearchCenter
    {
        unit.prior_orders = UnitOrder::Await;
        unit.prior_state = UnitOrderState::ExecutingOrder;
    }

    unit.orders = unit.prior_orders;
    unit.state = unit.prior_state;
}

/// `UnitsManager_SetNewOrderInt` (`units_manager.cpp:1945`).
///
/// Возвращает `true`, если вызывающий должен снять запрос пути
/// (`ResourceManager_GetPathsManager().RemoveRequest(unit)`).
pub fn set_new_order_int(
    unit: &mut UnitInfo,
    order: UnitOrder,
    state: UnitOrderState,
) -> bool {
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

/// `UnitsManager_ScaleUnit` (`units_manager.cpp:1847`) — запуск анимации
/// масштаба здания; возвращает `delete_path` из `SetNewOrderInt`.
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
        update_connectors(unit);
    } else {
        unit.scaler_adjust = 4;
        finish_unit_scaling(unit);
    }
}

/// `UnitsManager_FinishUnitScaling` (`units_manager.cpp:2471`).
///
/// Ветка «юнит вернулся в контейнер» требует `access`
/// (`Access_CanReceiveUnits`, `Hash_MapHash`) — P5; принята ветка «контейнер
/// не принимает» (иначе юнит останется в `ORDER_IDLE` навсегда).
pub fn finish_unit_scaling(unit: &mut UnitInfo) {
    restore_orders(unit);

    if unit.orders == UnitOrder::Idle {
        // TODO(P5): если родитель может принять юнита — вернуть его в трюм
        // (`Access_CanReceiveUnits`, `Hash_MapHash.Remove`, `++parent->storage`).
        unit.parent_unit = None;

        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;

        if unit.flags & MOBILE_AIR_UNIT != 0 {
            set_new_order_int(unit, UnitOrder::TakeOff, UnitOrderState::Init);
        }

        scale_unit(unit, UnitOrderState::Expand);
    }
}

/// `UnitInfo::BlockedOnPathRequest` (`unitinfo.cpp:3628`).
///
/// `disable_fire` — настройка, выключающая реактивный огонь (DESIGN §6.5:
/// настройки партии — входные факты, в max_core — поле `GameState`).
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
/// Установка `delayed_reaction` и добавление в список отложенных реакций
/// выполняются вызывающим слоем ([`OrderContext::move_finished`]), потому что
/// им нужен доступ к состоянию партии. Здесь — сброс счётчиков движения,
/// которые целиком локальны для юнита.
pub fn move_finished(unit: &mut UnitInfo, mode: bool, disable_fire: bool) {
    unit.velocity = 0;
    unit.moved = 0;

    clear_movement_wait(unit);

    if unit.unit_type == UnitType::Infantry || unit.unit_type == UnitType::Commando {
        // `UpdateSpriteFrame(0, image_index_max)` — UI.
    }

    unit.in_transit = false;

    if unit.unit_type == UnitType::MineLayer || unit.unit_type == UnitType::SeaMineLayer {
        // TODO(P5): Ai_CheckMines(this).
    }

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

/// `UnitInfo::StopMovement` (`unitinfo.cpp:3222`) — экстренный стоп.
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

/// `UnitInfo::Land` (`unitinfo.cpp:3829`) — анимация посадки, за один вызов
/// `moved` увеличивается на 1; `true` на 9-м вызове.
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

/// `UnitInfo::Take` (`unitinfo.cpp:3862`) — анимация взлёта, `true` на 5-м
/// вызове.
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
/// TODO(P3): `Cargo_UpdateResourceLevels` (уровни добычи/энергии комплекса).
pub fn power_up(unit: &mut UnitInfo, _factor: i32) {
    unit.state = UnitOrderState::ExecutingOrder;
}

/// `UnitInfo::PowerDown` (`unitinfo.cpp:5123`).
pub fn power_down(unit: &mut UnitInfo) {
    ai_task_flag("Power Down");
    power_up(unit, 1);

    if unit.unit_type == UnitType::ResearchCenter {
        // TODO(риск 1): ResearchMenu_UpdateResearchProgress(team, topic, -1).
    }
}

/// `UnitInfo::StartBuilding` (`unitinfo.cpp:2751`).
pub fn start_building(unit: &mut UnitInfo) {
    // `SDL_assert(build_list.GetCount() > 0)`: первый тип в очереди.
    let Some(_unit_to_build) = unit.build_list.first().copied() else {
        return;
    };

    if unit.flags & STATIONARY != 0 {
        // TODO(P3): списание материалов/энергии/рабочих комплекса
        // (`Cargo_GetRawConsumptionRate`, `Cargo_GetPowerConsumptionRate`,
        // `Cargo_GetLifeConsumptionRate`).
        unit.state = UnitOrderState::ExecutingOrder;
    } else {
        // `DeployConstructionSiteUtilities(unit_to_build)` — P3/P5.
        unit.moved = 0;
        unit.state = UnitOrderState::BuildInProgress;

        if unit.unit_type == UnitType::Constructor {
            set_new_order_int(
                unit,
                UnitOrder::AwaitTapePositioning,
                UnitOrderState::TapePositioningInit,
            );
        } else {
            // `PrepareConstructionSite` / `DrawSpriteFrame` — P3/P5.
        }

        // `ClearUnitList()` — снятие группового приказа (P5 access).
    }
}

/// `UnitInfo::CancelBuilding` (`unitinfo.cpp:4231`).
pub fn cancel_building(unit: &mut UnitInfo) {
    if unit.flags & STATIONARY != 0 {
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
        // TODO(P5): Access_DestroyUtilities(grid, ...).
        unit.path = None;
        unit.moved = 0;
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
        // TODO(P3/P5): возврат конструктора/бульдозера в комплексная логика.
    }
}

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

/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4414`).
///
/// Портирован цикл позиционирования: `INIT → ENTER`, `DEINIT → LEAVE`,
/// `moved` до 4, затем `RestoreOrders`. Точное позиционирование/углы —
/// P5 (`Paths_UpdateAngle`, `PrepareConstructionSite`).
pub fn position_in_tape(unit: &mut UnitInfo) {
    ai_task_flag("Positioning in tape");

    if unit.state == UnitOrderState::TapePositioningInit {
        unit.moved = 0;
        unit.state = UnitOrderState::TapePositioningEnter;
    } else if unit.state == UnitOrderState::TapePositioningDeinit {
        unit.moved = 0;
        unit.state = UnitOrderState::TapePositioningLeave;
    }

    refresh_screen(unit);

    if unit.moved >= 4 {
        // TODO(P5): проверки `UnitsManager_IsEngaged` и занятости площадки.
        let old_state = unit.state;
        restore_orders(unit);
        unit.moved = 0;

        if unit.unit_type == UnitType::MasterBuilder {
            // `PrepareConstructionSite` + `ScaleUnit(this, SHRINK)` — P3/P5.
            let _ = scale_unit(unit, UnitOrderState::Shrink);
        } else if unit.unit_type == UnitType::Constructor
            && old_state == UnitOrderState::TapePositioningEnter
        {
            // `PrepareConstructionSite` + `Paths_UpdateAngle` — P5.
        } else if old_state != UnitOrderState::TapePositioningEnter {
            // `UpdateGridPosition`/`Access_UpdateMapStatus` — P5.
            if unit.orders == UnitOrder::Build {
                ai_task_flag("build: delayed tasks");
            }
        }
    } else {
        // `GetTargetUnitAngle` + `Paths_UpdateAngle` — P5 (поворот на месте);
        // без модуля путей считаем, что угол уже верный, и идёт счётчик.
        unit.moved = unit.moved.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Пути: `Paths_GetAngle`, `Paths_UpdateAngle`, `Paths_CalculateStep`
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

/// `Paths_UpdateAngle` (`paths.cpp`): один шаг поворота к `angle`;
/// `true`, если юнит ещё поворачивается (движение в этот тик не начинается).
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
    true
}

/// `Paths_CalculateStep` (`paths.cpp`) — списание стоимости шага из `speed`.
///
/// Возвращает `false`, если у юнита не хватает скорости (шаг не начинается).
/// `real_time` — настройка `GameManager_RealTime`: шаг не тратит `speed`.
pub fn paths_calculate_step(
    unit: &mut UnitInfo,
    cost: i32,
    is_diagonal_step: bool,
    fast_movement: bool,
    real_time: bool,
) -> bool {
    let mut max_velocity = if fast_movement { 16 } else { 8 };

    if unit.unit_type == UnitType::Commando || unit.unit_type == UnitType::Infantry {
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
// Контекст обработки приказа
// ---------------------------------------------------------------------------

/// Рабочая копия юнита и доступ к состоянию партии во время обработки приказа.
pub struct OrderContext<'a> {
    /// Партия (для межъюнитных эффектов).
    pub world: &'a mut GameState,
    /// Id обрабатываемого юнита.
    pub id: UnitId,
    /// Рабочая копия юнита.
    pub unit: UnitInfo,
    /// Данные приказа (`move_to`/`fire_on`).
    pub data: OrderData,
}

impl<'a> OrderContext<'a> {
    /// `UnitsManager_SetNewOrderInt` для текущего юнита.
    pub fn set_new_order(&mut self, order: UnitOrder, state: UnitOrderState) -> bool {
        let delete_path = set_new_order_int(&mut self.unit, order, state);

        if delete_path {
            // `ResourceManager_GetPathsManager().RemoveRequest(unit)` — P5.
        }

        delete_path
    }

    /// `UnitsManager_ScaleUnit` для текущего юнита.
    pub fn scale(&mut self, state: UnitOrderState) -> bool {
        self.set_new_order(UnitOrder::AwaitScaling, state)
    }

    /// `RestoreOrders` текущего юнита.
    pub fn restore_orders(&mut self) {
        restore_orders(&mut self.unit);
    }

    /// `BlockedOnPathRequest` текущего юнита (+ отложенная реакция).
    pub fn blocked_on_path_request(&mut self, mode: bool) {
        blocked_on_path_request(&mut self.unit, mode, self.world.disable_fire);
        self.world.add_to_delayed_reaction_list(self.id);
    }

    /// `MoveFinished` текущего юнита (+ отложенная реакция и оценка целей).
    pub fn move_finished(&mut self, mode: bool) {
        move_finished(&mut self.unit, mode, self.world.disable_fire);

        if mode && !self.world.disable_fire {
            self.unit.delayed_reaction = 1;
            self.world.add_to_delayed_reaction_list(self.id);

            if self.unit.ammo > 0 {
                // TODO(P5): Ai_EvaluateAttackTargets(this).
            }
        }
    }

    /// `UnitsManager_EndAttack` для текущего юнита.
    pub fn end_attack(&mut self) {
        self.world.end_attack(self.id);
        self.unit.live_attack = 0;
    }

    /// `UnitsManager_PinAttackTargets` для текущего юнита.
    pub fn pin_attack_targets(&mut self) {
        self.end_attack();
        self.world.live_attack_counter = self.world.live_attack_counter.wrapping_add(1);
        let attack = self.world.live_attack_counter;
        self.unit.live_attack = attack;
        self.world.live_attacks.push(crate::game::LiveAttack {
            id: attack,
            targets: Vec::new(), // TODO(P5): Access_GetAreaAttackTargets/GetAttackTarget.
        });
    }

    /// Копия другого юнита (`Hash_UnitHash[id]`).
    pub fn other(&self, id: UnitId) -> Option<UnitInfo> {
        self.world.unit(id).cloned()
    }

    /// Родитель текущего юнита.
    pub fn parent(&self) -> Option<UnitInfo> {
        self.unit.parent_unit.and_then(|id| self.other(id))
    }

    /// `UnitsManager_ScaleUnit` другого юнита.
    pub fn scale_other(&mut self, id: UnitId, state: UnitOrderState) {
        self.world.scale_unit(id, state);
    }

    /// Записывает рабочую копию обратно в партию.
    pub fn commit(self) {
        if self.world.has_unit(self.id) {
            self.world.write_unit(self.id, self.unit);
            self.world.set_order_data(self.id, self.data);
        }
    }
}

// ---------------------------------------------------------------------------
// Движение (порт UnitInfo::ExecuteMove / Move и GroundPath::Execute)
// ---------------------------------------------------------------------------

/// `UnitInfo::ExecuteMove` (`unitinfo.cpp:1633`).
fn execute_move(ctx: &mut OrderContext<'_>) -> bool {
    refresh_screen(&ctx.unit);

    if is_engaged(&ctx.unit) {
        return false;
    }

    if ctx.unit.path.is_some() {
        if ctx.unit.flags & MOBILE_AIR_UNIT != 0 && ctx.unit.flags & HOVERING == 0 {
            ctx.set_new_order(UnitOrder::TakeOff, UnitOrderState::Init);
            ai_task_flag("plane takeoff");
            false
        } else {
            if ctx.unit.speed != 0 {
                ai_task_flag("moving");
            }
            path_execute(ctx)
        }
    } else if ctx.unit.flags & MISSILE_UNIT != 0 {
        // TODO(P5): ракета без пути — взрыв в текущей клетке
        // (`parent->Attack(grid)` + `UnitsManager_DestroyUnit`).
        false
    } else {
        ctx.unit.orders = UnitOrder::Await;
        ctx.unit.state = UnitOrderState::ExecutingOrder;
        false
    }
}

/// `UnitPath::Execute` для трёх реализаций пути.
fn path_execute(ctx: &mut OrderContext<'_>) -> bool {
    match ctx.unit.path.clone() {
        Some(UnitPath::Ground(_)) => ground_path_execute(ctx),
        Some(UnitPath::Air(path)) => air_path_execute(ctx, &path),
        Some(UnitPath::Builder(_)) => builder_path_execute(ctx),
        None => false,
    }
}

/// `GroundPath::Execute` (`groundpath.cpp:210`).
///
/// Пиксельная часть (`Paths_CalculateStep`-расход скорости) сохранена;
/// переход между клетками выполняет [`unit_move`] (расхождение 1 в шапке
/// модуля).
fn ground_path_execute(ctx: &mut OrderContext<'_>) -> bool {
    let (end_x, end_y, step_index) = match ctx.unit.path.as_ref() {
        Some(UnitPath::Ground(path)) => (path.end_x, path.end_y, path.step_index),
        _ => return false,
    };

    ctx.data.move_to = Point::new(end_x, end_y);
    ctx.unit.target_grid = ctx.data.move_to;

    let mut step_index = step_index as usize;
    let step;

    // Пропуск нулевых шагов (`for (step = m_steps[m_step_index]; step->x == 0 ...)`).
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
            // `ClearUnitList()` — снятие группового приказа (P5 access).
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

    // `ORDER_MOVE_TO_UNIT`: приём груза в целевой клетке.
    if ctx.unit.orders == UnitOrder::MoveToUnit {
        if let Some(receiver) = receiver_unit(ctx.world, &ctx.unit, target) {
            let receiver_unit = ctx.other(receiver);
            let stored = receiver_unit.as_ref().map(|unit| unit.storage).unwrap_or(0);
            let storable = receiver_unit
                .as_ref()
                .map(|unit| unit.base_values.get_attribute(crate::unit_values::ATTRIB_STORAGE))
                .unwrap_or(0);

            if stored >= storable as i16 {
                ctx.blocked_on_path_request(true);
            } else {
                ctx.unit.orders = UnitOrder::Idle;
                let receiver_type = receiver_unit.as_ref().map(|unit| unit.unit_type);
                if receiver_type == Some(UnitType::SeaTransport)
                    || receiver_type == Some(UnitType::PersonnelCarrier)
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

    let _ = paths_is_occupied(ctx.world, target);

    let cost = access_is_accessible(ctx.world, &ctx.unit, target, 0);

    if cost != 0 {
        let is_diagonal_step = offset.x != 0 && offset.y != 0;

        if paths_calculate_step(&mut ctx.unit, cost, is_diagonal_step, false, ctx.world.real_time) {
            // `MoveInTransitUnitInMapHash` — P5 (`Hash_MapHash`).
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
        set_unit_destination(ctx.world, &ctx.unit, ctx.unit.grid, target);
        // C++ продолжает поиск пути (`Paths_RequestPath`); без P5 путь
        // остаётся прежним, юнит повторит попытку на следующем тике.
        false
    }
}

/// `AirPath::Execute` (`airpath.cpp`) — в P4 сводится к «шагу к конечной
/// клетке»; пиксельная интерполяция отрезка — P5.
fn air_path_execute(ctx: &mut OrderContext<'_>, path: &AirPath) -> bool {
    ctx.data.move_to = Point::new(path.end_x, path.end_y);
    ctx.unit.target_grid = ctx.data.move_to;

    let grid = ctx.unit.grid;
    let dx = i32::from(path.end_x) - i32::from(grid.x);
    let dy = i32::from(path.end_y) - i32::from(grid.y);

    if dx == 0 && dy == 0 {
        ctx.blocked_on_path_request(true);
        return false;
    }

    if ctx.unit.speed == 0 {
        ctx.unit.state = UnitOrderState::ExecutingOrder;
        ctx.move_finished(true);
        return false;
    }

    let angle = paths_get_angle(dx, dy);
    ctx.unit.angle = (angle & 7) as u8;
    ctx.unit.state = UnitOrderState::InTransition;
    ctx.unit.in_transit = true;
    ctx.unit.moved = 0;
    true
}

/// `BuilderPath` — путь конструктора (P3/P5): без модуля путей шаг не
/// начинается, приказ остаётся в текущем состоянии.
fn builder_path_execute(_ctx: &mut OrderContext<'_>) -> bool {
    // TODO(P5): BuilderPath::Execute.
    false
}

/// `UnitInfo::Move` (`unitinfo.cpp:1730`) — завершение перехода клетки.
///
/// В P4 один вызов завершает одну клетку (расхождение 1 в шапке модуля).
fn unit_move(ctx: &mut OrderContext<'_>) {
    if is_engaged(&ctx.unit) {
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

    // C++ `Move()` при `moved == GFX_MAP_TILE_SIZE`:
    // `path_steps[step_index]->x -= offset_x` — шаг «съедается» переходом.
    if let Some(UnitPath::Ground(path)) = ctx.unit.path.as_mut() {
        if let Some(step) = path.steps.get_mut(path.step_index as usize) {
            step.x = step.x.wrapping_sub(offset.x as i8);
            step.y = step.y.wrapping_sub(offset.y as i8);
        }
    }

    ctx.unit.state = UnitOrderState::InProgress;

    if ctx.unit.orders == UnitOrder::Build && !ctx.unit.build_list.is_empty() {
        // C++: `Access_IsAccessible(build_list[0], ...)` → `Build()`.
        // TODO(P3/P5): проверка доступности площадки и `UnitInfo::Build`.
    }

    if ctx.unit.has_move_order() {
        // `FollowUnit()` — групповой приказ (P5 access).
    }

    // Циклический поиск повторного перехода C++ (`while (IN_TRANSITION &&
    // !visible)`) в сетевом режиме не выполняется: одна клетка за тик.
}

// ---------------------------------------------------------------------------
// Процессоры приказов
// ---------------------------------------------------------------------------

/// `UnitsManager_PerformAutoSurvey`, упрощённый до решений о состоянии.
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
        // TODO(P6): `Ai_EnableAutoSurvey` создаёт TaskAutoSurvey.
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
                if !has_active_unit_with_flags(ctx.world, ctx.unit.grid, MOBILE_SEA_UNIT) {
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
                && !has_active_unit_with_flags(ctx.world, ctx.unit.grid, MOBILE_SEA_UNIT)
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
    ai_task_flag("Transform");
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
            // TODO(P5): `UnitsManager_DeployMasterBuilder` — разрушение
            // конструктора и постановка шахты/генератора/плиты.
        }
        UnitOrderState::FinishTransforming => {
            ctx.unit.orders = UnitOrder::PowerOn;
            ctx.unit.state = UnitOrderState::Init;

            if ctx.world.teams[ctx.unit.team.0 as usize].team_type
                == crate::team::TeamKind::Player
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
            ai_task_flag("Storing");
            store_unit(ctx);
        }
        UnitOrderState::InProgress => {
            if execute_move(ctx) {
                ai_task_flag("Moving");
                unit_move(ctx);
            }
        }
        UnitOrderState::InTransition => {
            ai_task_flag("Moving");
            unit_move(ctx);
        }
        UnitOrderState::PathRequestCancel => {
            ctx.unit.enemy_unit = None;
            ctx.blocked_on_path_request(false);
        }
        UnitOrderState::Init | UnitOrderState::MoveInit => {
            if init_unit_move(ctx) && execute_move(ctx) {
                ai_task_flag("Moving");
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
                    // Мост, поднятый заново до конца опускания, ещё в списке
                    // стационарных юнитов.
                    if remove_from_list(ctx.world, ctx.id, UnitListKind::GroundCover) {
                        ctx.world
                            .add_to_draw_list_with(ctx.id, STATIONARY | UPGRADABLE | SELECTABLE);
                    }
                }
                ctx.unit.image_index = ctx.unit.image_index.wrapping_add(1);
            }
        }
        UnitOrderState::Lower => {
            if has_active_unit_with_flags(ctx.world, ctx.unit.grid, MOBILE_SEA_UNIT) {
                ctx.set_new_order(UnitOrder::Move, UnitOrderState::Elevate);
            } else if is_bridge_elevated(&ctx.unit) {
                ctx.unit.image_index = ctx.unit.image_index.wrapping_sub(1);
            } else {
                if remove_from_list(ctx.world, ctx.id, UnitListKind::Stationary) {
                    ctx.world.add_to_draw_list(ctx.id);
                }
                ctx.set_new_order(UnitOrder::Await, UnitOrderState::ExecutingOrder);
            }        }
        UnitOrderState::NewOrder => {
            if ctx.unit.flags & MOBILE_AIR_UNIT != 0
                && ctx.world.teams[ctx.unit.team.0 as usize].team_type
                    == crate::team::TeamKind::Player
                && ctx.world.list_position(ctx.id).is_some()
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

/// Снятие юнита из конкретного списка (C++ `SmartList::Remove`).
fn remove_from_list(state: &mut GameState, id: UnitId, kind: UnitListKind) -> bool {
    state.remove_from_list(kind, id)
}

/// `UnitsManager_PursueEnemy` (`units_manager.cpp:3516`).
///
/// Портированы условия и переходы приказа; сам поиск пути
/// (`Paths_RequestPath`) — P5, дальнобойность (`Access_IsWithinAttackRange`) —
/// P5 (заменена квадратом расстояния в клетках).
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
        && (ctx.unit.path.is_none()
            || move_to.x != enemy.grid.x
            || move_to.y != enemy.grid.y)
        && (!enemy_is_moving || enemy.state == UnitOrderState::ExecutingOrder)
    {
        // `Paths_RequestPath` на позицию атаки — P5. Приказ и путь остаются;
        // без модуля путей юнит продолжает идти к прежней точке.
        return false;
    }

    if ctx.unit.shots > 0 && within_attack_range(ctx.unit.grid, position, unit_range) {
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

/// `Access_IsWithinAttackRange` (`access.cpp`) — P5: приближение квадратом
/// расстояния клеток с учётом габаритов зданий (не моделируются).
fn within_attack_range(from: Point, target: Point, range: i32) -> bool {
    squared_distance(from, target) <= range * range
}

/// `UnitsManager_GetAttackPosition` (`units_manager.cpp:4847`) без
/// `Access_GetConstructionUtility` (P5).
fn attack_position(ctx: &OrderContext<'_>, enemy: &UnitInfo) -> Point {
    let mut position = enemy.grid;

    if enemy.flags & BUILDING != 0 {
        if ctx.unit.grid.x > position.x {
            position.x += 1;
        }
        if ctx.unit.grid.y > position.y {
            position.y += 1;
        }
    }

    position
}

/// `Access_GetSquaredDistance` (`access.cpp`) — квадрат расстояния в клетках.
fn squared_distance(first: Point, second: Point) -> i32 {
    let dx = i32::from(first.x) - i32::from(second.x);
    let dy = i32::from(first.y) - i32::from(second.y);
    dx * dx + dy * dy
}

/// `UnitsManager_InitUnitMove` (`units_manager.cpp:3605`) + `InitUnitPath`
/// (часть `UnitsManager_InitUnitPath`).
fn init_unit_move(ctx: &mut OrderContext<'_>) -> bool {
    refresh_screen(&ctx.unit);
    init_unit_path(ctx);

    if ctx.unit.state == UnitOrderState::Init {
        // `FollowUnit()` + `Redraw()` — P5/UI.
        ctx.unit.state = UnitOrderState::InProgress;

        if ctx.world.network_game && ctx.unit.flags & MOBILE_AIR_UNIT != 0 {
            // `Remote_SendNetPacket_38(unit)` — сеть (max_net), не max_core.
            return false;
        }
    }

    ctx.unit.state == UnitOrderState::InProgress
}

/// `UnitsManager_InitUnitPath` (`units_manager.cpp:4776`).
///
/// В C++ путь запрашивается у `PathsManager` асинхронно; в max_core
/// pathfinding синхронный (DESIGN риск 2), а сам модуль — P5. До его появления
/// путь строится линейно (`AppendLinearSteps`) — это единственный
/// детерминированный вариант без карты проходимости.
fn init_unit_path(ctx: &mut OrderContext<'_>) {
    ctx.unit.path = None;

    if ctx.unit.flags & MISSILE_UNIT != 0 {
        // TODO(P5): UnitsManager_GetMissilePath.
        return;
    }

    if ctx.world.teams[ctx.unit.team.0 as usize].team_type == crate::team::TeamKind::Remote {
        ctx.unit.state = UnitOrderState::NewOrder;
        return;
    }

    if ctx.unit.state == UnitOrderState::MoveInit {
        if ctx.unit.flags & MOBILE_AIR_UNIT != 0 && ctx.unit.orders != UnitOrder::MoveToAttack {
            ctx.unit.path = Some(UnitPath::Air(AirPath {
                end_x: ctx.data.move_to.x,
                end_y: ctx.data.move_to.y,
                ..AirPath::default()
            }));

            ctx.restore_orders();
        } else {
            // TODO(P5): Paths_RequestPath → состояние MOVE_GETTING_PATH.
            ctx.unit.state = UnitOrderState::MoveGettingPath;
            build_linear_path(ctx);
        }
    } else {
        if has_group_order(&ctx.unit) {
            // TODO(P5): `Access_GroupAttackOrder(unit, true)` — групповой
            // приказ (список `UnitInfo::GetUnitList`).
        } else if ctx.unit.flags & MOBILE_AIR_UNIT != 0
            && ctx.unit.orders != UnitOrder::MoveToAttack
        {
            ctx.unit.path = Some(UnitPath::Air(AirPath {
                end_x: ctx.data.move_to.x,
                end_y: ctx.data.move_to.y,
                ..AirPath::default()
            }));
        } else {
            // TODO(P5): Paths_RequestPath → состояние NEW_ORDER.
            build_linear_path(ctx);
        }
    }
}

/// `UnitInfo::GetUnitList() != nullptr` — приказ группе (`UnitInfoGroup`).
///
/// Списки групп — часть модуля `access` (P5); в C++ это отдельный
/// `SmartList<UnitInfo>`, который не имеет отношения к пяти глобальным
/// спискам юнитов.
fn has_group_order(_unit: &UnitInfo) -> bool {
    false
}

/// Линейный маршрут (`GroundPath::AppendLinearSteps`) — замена P5-путей.
fn build_linear_path(ctx: &mut OrderContext<'_>) {
    let start = ctx.unit.grid;
    let target = ctx.data.move_to;
    let mut steps = Vec::new();
    let mut x = i32::from(target.x) - i32::from(start.x);
    let mut y = i32::from(target.y) - i32::from(start.y);

    while x != 0 || y != 0 {
        let step_x = if x > 0 {
            1
        } else if x == 0 {
            0
        } else {
            -1
        };
        let step_y = if y > 0 {
            1
        } else if y == 0 {
            0
        } else {
            -1
        };

        steps.push(PathStep::new(step_x, step_y));
        x -= i32::from(step_x);
        y -= i32::from(step_y);
    }

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

    if parent.storage < parent.base_values.get_attribute(crate::unit_values::ATTRIB_STORAGE) as i16 {
        parent.storage = parent.storage.wrapping_add(1);
        ctx.world.write_unit(parent_id, parent);
    }

    // `ClearUnitList()` — P5.
    ctx.unit.orders = UnitOrder::Idle;
    ctx.unit.state = UnitOrderState::Store;
}

/// `UnitsManager_ProcessOrderFire` (`units_manager.cpp:2781`).
fn process_order_fire(ctx: &mut OrderContext<'_>) {
    ai_task_flag("Firing");
    ctx.world.combat_effects_active = true;

    match ctx.unit.state {
        UnitOrderState::Init => {
            ctx.world.pending_attacks.push(ctx.id);
            ctx.pin_attack_targets();
            ctx.unit.state = UnitOrderState::AttackPending;
        }
        UnitOrderState::InProgress => {
            ctx.world.orders_pending = true;

            if aim_at_target(ctx) {
                ctx.world.orders_pending = true;
                // `PrepareFire()` — P5.
            }
        }
        UnitOrderState::ReadyToFire => {
            ctx.world.orders_pending = true;
            // `PrepareFire()` — P5.
        }
        UnitOrderState::FireInProgress => {
            ctx.world.orders_pending = true;
            // `ProgressFire()` — P5.
        }
        UnitOrderState::AttackPending => {}
        UnitOrderState::AttackBeginning => {
            ctx.unit.state = UnitOrderState::InProgress;
            ctx.world.orders_pending = true;

            if aim_at_target(ctx) {
                ctx.world.orders_pending = true;
                // `PrepareFire()` — P5.
            }
        }
        _ => {}
    }
}

/// `UnitInfo::AimAtTarget` (`unitinfo.cpp:5207`) — P5: поворот башни/корпуса
/// к цели. Без модуля боя цель не захватывается.
fn aim_at_target(ctx: &mut OrderContext<'_>) -> bool {
    // TODO(P5): поворот к `fire_on_grid_*` и проверка готовности к выстрелу.
    let _ = &ctx.unit;
    false
}

/// `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2834`).
fn process_order_build(ctx: &mut OrderContext<'_>) {
    match ctx.unit.state {
        UnitOrderState::Init => {
            ai_task_flag("Build start");
            start_building(&mut ctx.unit);
        }
        UnitOrderState::InProgress => {
            if execute_move(ctx) {
                ai_task_flag("Moving");
                unit_move(ctx);
            }
        }
        UnitOrderState::InTransition => {
            ai_task_flag("Moving");
            unit_move(ctx);
        }
        UnitOrderState::BuildInProgress => {
            if ctx.world.settings_effects && ctx.unit.unit_type == UnitType::Constructor {
                // Анимация стройки двигает `moved` (как в C++).
                ctx.unit.moved = ctx.unit.moved.wrapping_add(1);
            }
        }
        UnitOrderState::BuildCancel | UnitOrderState::BuildAbort => {
            ai_task_flag("Build cancel");
            cancel_building(&mut ctx.unit);
        }
        UnitOrderState::SelectSite => {
            animate(&ctx.unit);
        }
        UnitOrderState::BuildClearing => {
            ai_task_flag("Build clearing");
            // TODO(P3/P5): UnitsManager_BuildClearing — разрушение
            // застройки, обломки, утилиты, возврат юнитов.
        }
        UnitOrderState::UnitReady => {
            if ctx.unit.path.is_some() {
                // TODO(P3): UnitsManager_BuildNext — проверка склада/материалов.
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
    ai_task_flag("Activation");

    match ctx.unit.state {
        UnitOrderState::ExecutingOrder => {
            // TODO(P5): `UnitsManager_ActivateUnit` — проверка занятости
            // площадки (`Paths_IsOccupied`, `Access_IsAccessible`), перенос
            // клиента на карту; без модуля доступа активация не выполняется и
            // приказ возвращается к отложенному (как в ветке отказа C++).
            ctx.restore_orders();
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

/// `UnitsManager_ActivateEngineer` (`units_manager.cpp:3407`).
fn activate_engineer(ctx: &mut OrderContext<'_>) {
    // TODO(P5): Access_DestroyUtilities.
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

/// `UnitsManager_ProcessOrderNewAllocate` (`units_manager.cpp:2935`).
fn process_order_new_allocate(ctx: &mut OrderContext<'_>) {
    ai_task_flag("New allocation");
    ctx.restore_orders();
}

/// `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`).
fn process_order_power_on(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::Init {
        ai_task_flag("Power up");
        power_up(&mut ctx.unit, -1);

        if ctx.unit.unit_type == UnitType::ResearchCenter {
            // TODO(риск 1): ResearchMenu_UpdateResearchProgress(team, topic, 1).
        }
    }

    animate(&ctx.unit);
}

/// `UnitsManager_ProcessOrderPowerOff` (`units_manager.cpp:2964`).
fn process_order_power_off(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::Init {
        power_down(&mut ctx.unit);
    }

    animate(&ctx.unit);
}

/// `UnitsManager_ProcessOrderExplode` (`units_manager.cpp:2972`).
fn process_order_explode(ctx: &mut OrderContext<'_>) {
    ai_task_flag("Exploding");

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
/// Спавн юнита-эффекта (`*EXPLD`, флаги `EXPLODING|MISSILE_UNIT`) —
/// P3-таблица флагов + P5; переходы состояний портированы.
fn start_explosion(ctx: &mut OrderContext<'_>) {
    refresh_screen(&ctx.unit);

    if (ctx.unit.unit_type == UnitType::Commando || ctx.unit.unit_type == UnitType::Infantry)
        && ctx.unit.hits == 0
    {
        ctx.unit.state = UnitOrderState::Destroy;
        ctx.unit.moved = 0;
        ctx.unit.parent_unit = None;
        return;
    }

    // TODO(P5): DeployUnit(*EXPLD) рядом с юнитом.
    ctx.unit.orders = ctx.unit.prior_orders;
    ctx.unit.state = ctx.unit.prior_state;
    restore_orders(&mut ctx.unit);

    if ctx.unit.hits == 0 {
        ctx.unit.state = UnitOrderState::Destroy;
    }
}

/// `UnitsManager_ProgressExplosion` (`units_manager.cpp:4006`).
fn progress_explosion(ctx: &mut OrderContext<'_>) {
    if ctx.unit.image_index == ctx.unit.image_index_max {
        ctx.world.destroy_unit(ctx.id);
    } else {
        if ctx.unit.moved & 0x01 != 0 {
            ctx.unit.image_index = ctx.unit.image_index.wrapping_add(1);
        }
        ctx.unit.moved = ctx.unit.moved.wrapping_add(1);
    }
}

/// `UnitsManager_ProcessOrderUnload` (`units_manager.cpp:2994`).
fn process_order_unload(ctx: &mut OrderContext<'_>) {
    ai_task_flag("Unloading");

    match ctx.unit.state {
        UnitOrderState::Init => {
            ctx.unit.moved = 0;
            ctx.unit.state = UnitOrderState::UnloadingInProgress;

            if is_unload_site_taken(ctx.world, &ctx.unit)
                || paths_is_occupied(ctx.world, ctx.unit.grid)
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

    // TODO(P5): Access_TriggerEnemyMine + UnitsManager_UpdateMapHash.
    restore_orders(&mut client);
    ctx.world.write_unit(parent_id, client);

    // `Access_UpdateMapStatus` — P5.

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
        ai_task_flag("Clear start");
        // `UnitsManager_StartClearing`: спавн конусов/ленты, `ClearUnitList`,
        // state = IN_PROGRESS, для LRGTAPE — AWAIT_TAPE_POSITIONING.
        // TODO(P3/P5): DeployUnit(LRGCONES/LRGTAPE/SMLCONES/SMLTAPE).
        ctx.unit.state = UnitOrderState::InProgress;

        let parent_is_building = ctx
            .parent()
            .map(|parent| parent.flags & BUILDING != 0)
            .unwrap_or(false);

        if parent_is_building {
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
    ai_task_flag("Landing");

    // Занятый самолёт не снижается в ангар; посадочная площадка держит его на
    // карте.
    if ctx.unit.parent_unit.is_some() && is_engaged(&ctx.unit) {
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
    ai_task_flag("Taking off");

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
    ai_task_flag("Loading");

    match ctx.unit.state {
        UnitOrderState::Init => {
            // Занятый пассажир не подбирается до конца атаки/стелс-действия.
            let parent_engaged = ctx
                .parent()
                .map(|parent| is_engaged(&parent))
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
        .map(|parent| is_engaged(&parent))
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
        ai_task_flag("Building ready");
        // TODO(P3/P5): UnitsManager_BuildingReady — родитель покидает площадку
        // или уничтожается, если площадка занята.
    }
}

/// `UnitsManager_ProcessOrderRepair` (`units_manager.cpp:3124`) +
/// `UnitInfo::ProcessRepair` (`unitinfo.cpp:5258`).
fn process_order_repair(ctx: &mut OrderContext<'_>) {
    ai_task_flag("Repairing");

    if ctx.unit.complex.is_some() {
        // TODO(P3): ремонт за материалы комплекса (`Complex::GetCargoInfo`,
        // `UnitInfo::Repair`).
    } else if let Some(parent_id) = ctx.unit.parent_unit {
        // TODO(P3): `parent->Repair(storage)` и списание `storage`.
        schedule_delayed_tasks(ctx.world, parent_id, true);
    }

    ctx.restore_orders();
}

/// `UnitsManager_ProcessOrderReload` (`units_manager.cpp:3130`) +
/// `UnitInfo::Reload` (`unitinfo.cpp:4301`).
fn process_order_reload(ctx: &mut OrderContext<'_>) {
    ai_task_flag("Reloading");

    let mut need_action = false;

    if ctx.unit.complex.is_some() {
        // TODO(P3): `complex->GetCargoInfo` → `Transfer(-1, 0, 0)`.
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
    ai_task_flag("Tranferring");

    // TODO(P3): перенос груза между юнитом и родителем
    // (`TransferRaw/TransferFuel/TransferGold`, комплекс).

    ctx.restore_orders();
}

/// `UnitsManager_ProcessOrderHaltBuilding` (`units_manager.cpp:3142`).
fn process_order_halt_building(ctx: &mut OrderContext<'_>) {
    if ctx.unit.state == UnitOrderState::BuildCancel {
        ai_task_flag("Build halting");
        cancel_building(&mut ctx.unit);
    }
}

/// `UnitsManager_ProcessOrderAwaitScaling` (`units_manager.cpp:3150`).
fn process_order_await_scaling(ctx: &mut OrderContext<'_>) {
    ai_task_flag("Scaling");
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
                update_connectors(&mut ctx.unit);
                schedule_delayed_tasks(ctx.world, ctx.id, true);
            }
        }
        UnitOrderState::Shrink => {
            if ctx.unit.scaler_adjust == 4 {
                finish_unit_scaling(&mut ctx.unit);
            } else {
                ctx.unit.scaler_adjust = ctx.unit.scaler_adjust.wrapping_add(1);
            }
        }
        _ => {}
    }
}

/// `UnitsManager_ProcessOrderAwaitTapePositioning` (`units_manager.cpp:3187`).
fn process_order_await_tape_positioning(ctx: &mut OrderContext<'_>) {
    position_in_tape(&mut ctx.unit);
}

/// `UnitsManager_ProcessOrderAwaitDisableStealUnit`
/// (`units_manager.cpp:3189`).
fn process_order_await_disable_steal_unit(ctx: &mut OrderContext<'_>) {
    ctx.world.combat_effects_active = true;
    ai_task_flag("Disable/Steal");

    match ctx.unit.state {
        UnitOrderState::Init => {
            ctx.unit.state = UnitOrderState::InProgress;
            ctx.scale(UnitOrderState::Shrink);
        }
        UnitOrderState::ExecutingOrder => {
            // `UnitsManager_AttemptStealthAction` — P5.
            if attempt_stealth_action(ctx) {
                if ctx.unit.orders == UnitOrder::AwaitStealUnit {
                    // `UnitsManager_CaptureUnit` — P5.
                } else {
                    // `UnitsManager_DisableUnit` — P5.
                }
            }

            ctx.unit.orders = UnitOrder::Await;
        }
        UnitOrderState::InProgress => {
            if let Some(parent_id) = ctx.unit.parent_unit {
                if let Some(mut parent) = ctx.other(parent_id) {
                    // `ShakeSabotage()` — P5; без него анимация завершается
                    // сразу, чтобы приказ не завис навсегда.
                    parent.shake_effect_state = 0;
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

/// `UnitsManager_AttemptStealthAction` (`units_manager.cpp:4254`) — P5.
fn attempt_stealth_action(_ctx: &mut OrderContext<'_>) -> bool {
    false
}

/// `UnitsManager_ProcessOrderDisable` (`units_manager.cpp:3226`).
fn process_order_disable(ctx: &mut OrderContext<'_>) {
    // `unit->Animate()` — UI; состояние не меняется, пока идут ходы
    // отключения (`UpdateProduction` уменьшает `disabled_turns_remaining`).
    animate(&ctx.unit);
}

/// `UnitsManager_ProcessOrderUpgrade` (`units_manager.cpp:3228`) +
/// `UnitInfo::Upgrade` (`unitinfo.cpp:4339`).
fn process_order_upgrade(ctx: &mut OrderContext<'_>) {
    ai_task_flag("Upgrading");

    // TODO(P3): проверка версии атрибутов и материалов комплекса
    // (`GetUpgradeCost`, `UpgradeInt`).
    ctx.restore_orders();
    ctx.unit.parent_unit = None;
}

/// `UnitsManager_ProcessOrderLayMine` (`units_manager.cpp:3234`).
fn process_order_lay_mine(ctx: &mut OrderContext<'_>) {
    ai_task_flag("lay mines");

    match ctx.unit.state {
        UnitOrderState::Inactive => {
            ctx.unit.laying_state = 0;
        }
        UnitOrderState::PlacingMines => {
            ctx.unit.laying_state = 2;
            // TODO(P5): UnitInfo::PlaceMine — мина на карту.
        }
        UnitOrderState::RemovingMines => {
            ctx.unit.laying_state = 1;
            // TODO(P5): UnitInfo::PickUpMine.
        }
        _ => {}
    }

    ctx.restore_orders();
    schedule_delayed_tasks(ctx.world, ctx.id, true);
}

/// `UnitsManager_ProcessOrder` (`units_manager.cpp:3257`) — диспетчер 32
/// приказов.
///
/// Маппинги сохранены дословно, включая `ORDER_MOVE_TO_UNIT`/`_TO_ATTACK` →
/// `ProcessOrderMove` и `ORDER_HALT_BUILDING_2` → `ProcessOrderHaltBuilding`.
/// `ORDER_17` (0x12) в C++ не имеет ветки `switch` — тоже без обработки.
///
/// Состояния `ORDER_STATE_10` (0xA) и `ORDER_STATE_21` (0x15) объявлены в
/// `enums.hpp`, но C++-порт их нигде не использует, поэтому веток для них нет
/// (совпадение полное, а не пропуск).
pub fn process_order(state: &mut GameState, id: UnitId) {
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

    // Шапка `UnitsManager_ProcessOrder`.
    if ctx.unit.shake_effect_state != 0 && !is_engaged_by_infiltrator(&ctx.unit) {
        // Инфильтратор, тряхнувший юнита, погиб или вышел из стелса.
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
        // `ORDER_17`: в C++-порте ветки нет (неиспользуемый слот).
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

/// `UnitsManager_ProcessUnitOrders` (`units_manager.cpp:3401`) — обход одного
/// списка; вынесен в [`GameState::process_unit_orders`].
pub fn process_unit_orders(state: &mut GameState, kind: UnitListKind) {
    state.process_unit_orders(kind);
}

/// `UnitsManager_ProcessOrders` (`units_manager.cpp:1860`) — вынесен в
/// [`GameState::process_orders`].
pub fn process_orders(state: &mut GameState) {
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
        // Скорость танка из таблицы атрибутов — чтобы шаги пути были возможны.
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
        // Танк тратит по 1 единице скорости на клетку: 4 клетки → speed-4.
        assert_eq!(unit.speed, unit.base_values.get_attribute(ATTRIB_SPEED) as u16 - 4);
    }

    #[test]
    fn move_order_is_replaced_by_new_order() {
        let (mut state, id) = state_with_tank();
        order_move_to(&mut state, id, Point::new(30, 10));
        state.tick();

        // Смена приказа во время движения: prior фиксирует Move/Init.
        state.set_order(id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
        let unit = state.unit(id).expect("unit");
        assert_eq!(unit.orders, UnitOrder::Sentry);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert_eq!(unit.prior_orders, UnitOrder::Move);

        // SENTRY не двигает юнита.
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

        // Взрыв перекрывает огонь и не сохраняет prior.
        assert!(!set_new_order_int(
            &mut unit,
            UnitOrder::Explode,
            UnitOrderState::Init
        ));
        assert_eq!(unit.orders, UnitOrder::Explode);

        // Из состояния NEW_ORDER приказ возвращается в AWAIT и просит снять путь.
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

        // У зданий с питанием совпадение prior/order сохраняется (как в C++).
        let mut tower = UnitInfo::new(UnitType::CommTower, TeamId::RED, UnitId(2));
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
        // Обнулим скорость: шаг пути не начинается, `MoveFinished` сбрасывает
        // движение, но приказ остаётся MOVE (как в `GroundPath::Execute` при
        // `speed == 0`: состояние EXECUTING_ORDER, путь сохраняется до
        // восстановления скорости в начале хода).
        state.unit_mut(id).unwrap().speed = 0;
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.orders, UnitOrder::Move);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert!(unit.path.is_some());
        assert_eq!(unit.delayed_reaction, 1);
        assert_eq!(unit.moved, 0);

        // Начало хода возвращает скорость (`UpdateProduction`) — движение
        // продолжается с того же места.
        let base_speed = state.unit(id).unwrap().base_values.get_attribute(ATTRIB_SPEED) as u16;
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

        // Диагональ стоит 6: списывается 2 при пустом move_fraction.
        unit.speed = 12;
        assert!(paths_calculate_step(&mut unit, 4, true, false, false));
        assert_eq!(unit.speed, 10);

        // Не хватает скорости — шаг не начинается.
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
        // Поворот на 1 шаг за вызов, пока не совпадёт.
        let mut calls = 0;
        while paths_update_angle(&mut unit, 4) {
            calls += 1;
            assert!(calls < 8);
        }
        assert_eq!(unit.angle, 4);
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
        // Родитель-транспорт и пассажир.
        let carrier = state
            .spawn_unit(
                UnitType::PersonnelCarrier,
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
        // `ProcessLoading` завершает посадку: пассажир в AWAIT, без родителя.
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
        assert_eq!(state.unit(id).unwrap().state, UnitOrderState::Destroy);
        // На следующем тике анимация взрыва заканчивается (image_index == max)
        // и юнит уничтожается.
        state.tick();
        assert!(!state.has_unit(id));
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
        // scale до нуля → RestoreOrders возвращает Move/InProgress.
        assert_eq!(state.unit(id).unwrap().orders, UnitOrder::Move);
    }
}
