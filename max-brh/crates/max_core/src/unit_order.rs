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
//! `ProcessUnloading`, `PositionInTape`, `UpdateAngle`, `StartBuilding`,
//! `CancelBuilding`,
//! `PowerUp`, `PowerDown`, `Reload`, `Repair`, `Transfer`, `Upgrade`,
//! `CheckIfDestroyed`), `groundpath.cpp` (`GroundPath::Execute`,
//! `GroundPath::UpdateUnitAngle`, `CancelMovement`), `paths.cpp`
//! (`Paths_GetAngle`, `Paths_UpdateAngle`,
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
//! # Интеграция P5/P6
//!
//! Бой (`crate::combat`), проходимость/занятость (`crate::access`) и пути
//! (`crate::pathfinding`) подключены к машине приказов: `ProcessOrderFire`
//! использует `AimAtTarget`/`PrepareFire`/`ProgressFire`/`Attack`,
//! `InitUnitPath` ставит запросы в `PathsManager`-очередь, `PursueEnemy` и
//! `MoveToAttack` ищут путь к позиции атаки, `IsEngaged`/`CanReceiveUnits`/
//! `SetUnitDestination` берутся из `access`. ИИ-хуки P6 подключены:
//! `MoveFinished` вызывает `Ai_EvaluateAttackTargets` и `Ai_CheckMines`,
//! `PerformAutoSurvey` — `Ai_EnableAutoSurvey`, `ScheduleDelayedTasks` —
//! `TaskManager::ScheduleDelayedTasks`. Оставшиеся `TODO(P3)` — экономика и
//! комплексы; `TODO(P5)` в точках, где требуется `UnitsManager_DeployUnit`
//! конкретных типов (мастер-билдер, активация).
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

/// `GFX_MAP_TILE_SIZE` — сторона клетки в пикселях (`unitinfo.cpp`).
pub const GFX_MAP_TILE_SIZE: i32 = 64;

// ---------------------------------------------------------------------------
// Мосты в ИИ (P6)
// ---------------------------------------------------------------------------

/// `UnitInfo::Animate` / `RefreshScreen` — UI-анимация и грязные зоны (P4 не нужны).
fn animate(_unit: &UnitInfo) {}

/// `UnitInfo::RefreshScreen` — UI.
fn refresh_screen(_unit: &UnitInfo) {}

/// `Ai_SetTasksPendingFlag(name)` (`ai.cpp:140`).
///
/// C++ выставляет флаг «у компьютерной команды есть работа» на каждом тике,
/// в котором юнит реально что-то делает (`UnitsManager_ProcessOrder*` и
/// `UnitInfo::ExecuteMove`/`PositionInTape`); `Ai_ClearTasksPendingFlags`
/// сбрасывает его в начале `UnitsManager_ProcessOrders`. Именно этот флаг, а
/// не «есть ли незавершённые задачи», решает, может ли ИИ закончить ход
/// (`AiPlayer::CheckEndTurn`): юнит, у которого кончилась скорость, флаг не
/// выставляет, и ход закрывается.
fn ai_task_flag(state: &mut GameState, _name: &str) {
    crate::ai::ai_set_tasks_pending_flag(state);
}

/// `UnitInfo::ScheduleDelayedTasks` — P6: напоминание верхней задаче юнита
/// о завершении перегона (C++ `Task_RemindMoveFinished`).
fn schedule_delayed_tasks(state: &mut GameState, id: UnitId, priority: bool) {
    state.with_task_manager(|task_manager, world| {
        task_manager.schedule_delayed_tasks(world, id, priority);
    });
}

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
/// Возвращает `true`, если вызывающий должен снять запрос пути
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
        // `UnitsManager_UpdateConnectors(unit)` требует `GameState`; вызывающие
        // её ветки (приказ/пакет 08) делают это сами — см.
        // [`crate::complex::update_connectors`]. Чистая функция не может.
    } else {
        unit.scaler_adjust = 4;
        finish_unit_scaling(unit);
    }
}

/// `UnitsManager_FinishUnitScaling` (`units_manager.cpp:2471`) — версия без
/// состояния партии (для пакетного пути `NewOrderWhileScaling`); «контейнер
/// не принимает» — безопасный вариант, иначе юнит остался бы в `ORDER_IDLE`.
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

/// `UnitsManager_FinishUnitScaling` с доступом к партии: если контейнер может
/// принять юнита (`Access_CanReceiveUnits`, `storage < ATTRIB_STORAGE`, та же
/// команда), юнит уходит в трюм (`ORDER_IDLE` + `Hash_MapHash.Remove`).
pub fn finish_unit_scaling_in(state: &mut GameState, unit: &mut UnitInfo) {
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
///
/// `Ai_CheckMines` тоже требует партии, поэтому вызывается из
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
pub fn power_down(state: &mut GameState, unit: &mut UnitInfo) {
    ai_task_flag(state, "Power Down");
    power_up(unit, 1);

    if unit.unit_type == UnitType::ResearchCentre {
        // TODO(риск 1): ResearchMenu_UpdateResearchProgress(team, topic, -1).
    }
}

/// `UnitInfo::StartBuilding` (`unitinfo.cpp:2751`).
///
/// Мобильному строителю ставит стройплощадку — [`crate::production::
/// deploy_construction_site`] (`DeployConstructionSiteUtilities` +
/// `PrepareConstructionSite`): лента/конусы/плита в клетке `target_grid`.
/// Функция идемпотентна: `ProductionManager::commit` (нажатие `Done` в
/// `BuildMenu`) и `events::apply_command` (пакет 08/P4 после клика по карте)
/// уже ставят площадку синхронно, поэтому здесь на следующем тике повторного
/// спавна нет — только `ORDER_STATE_BUILD_IN_PROGRESS` и `PositionInTape`.
pub fn start_building(state: &mut GameState, unit: &mut UnitInfo) {
    // `SDL_assert(build_list.GetCount() > 0)`: первый тип в очереди.
    let Some(unit_to_build) = unit.build_list.first().copied() else {
        return;
    };

    if !unit.is_mobile() {
        // TODO(P3): списание материалов/энергии/рабочих комплекса
        // (`Cargo_GetRawConsumptionRate`, `Cargo_GetPowerConsumptionRate`,
        // `Cargo_GetLifeConsumptionRate`).
        unit.state = UnitOrderState::ExecutingOrder;
    } else {
        // `DeployConstructionSiteUtilities(unit_to_build)` +
        // `PrepareConstructionSite`: площадка в клетке `move_to`/`target_grid`.
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
            // `DrawSpriteFrame(image_index + 16)` — рабочая поза инженера;
            // кадр выбирает `max_app::render::unit_frame_index` по
            // `ORDER_BUILD`/`BUILD_IN_PROGRESS`.
        }

        // `ClearUnitList()` — снятие группового приказа (P5 access).
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
        // `Access_DestroyUtilities` требует `GameState`; вызывается
        // процессором приказа (`cancel_building` — чистая операция).
        unit.path = None;
        unit.moved = 0;
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
        // TODO(P3/P5): возврат конструктора/бульдозера в комплексная логика.
    }
}

/// Активная стройка мобильного строителя: клетка площадки (`target_grid`).
///
/// C++ `ORDER_BUILD` у мобильного строителя проходит состояния
/// `INIT → IN_PROGRESS → BUILD_IN_PROGRESS`, а у конструктора ещё
/// `ORDER_AWAIT_TAPE_POSITIONING` с состояниями `TAPE_POSITIONING_*`. Площадка к
/// этому моменту уже стоит (`UnitInfo::StartBuilding`), прогресс — в
/// `build_list`/`build_time`. `SELECT_SITE`/`UNIT_READY` сюда не входят: в
/// первом площадки ещё нет (призрак `GameManager_TempTape`), во втором здание
/// уже построено и строитель ждёт приказа уйти.
///
/// Возвращает `None`, если юнит не мобильный строитель или стройка не идёт.
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

    // Прогресс стройки живёт в очереди: `SpawnNewUnit` очищает её на
    // достройке, `CancelBuilding` — на отмене. Пустая очередь означает, что
    // `AWAIT_TAPE_POSITIONING` — это уже **уход** строителя с готовой площадки
    // (`TAPE_POSITIONING_DEINIT`, `ActivateEngineer`), а не стройка: снимать
    // утилиты и плиту под готовым зданием нельзя.
    if unit.build_list.is_empty() {
        return None;
    }

    Some(unit.target_grid)
}

// Увод мобильного строителя с недостроенной площадки **невозможен** без явной
// отмены — это исключённый путь из C++.
//
// В C++ приказ движения строителю во время стройки через карту не отдаётся
// (`GameManager_GetBuilderUnitCursor`, `game_manager.cpp:5927-5931`: для
// `BUILD_IN_PROGRESS` курсор — `CURSOR_UNIT_NO_GO`), а отмена идёт только через
// `stop` попапа (`UnitsManager_Popup_OnClick_BuildStop` →
// `GameManager_SelectBuildSite` → `ORDER_BUILD`/`BUILD_CANCEL` →
// `UnitInfo::CancelBuilding`). Прежняя портовая «автоотмена при уводе»
// противоречила оракулу и удалена: новый приказ (пакет 08/38, ИИ, группа)
// стройку не снимает — приказ просто меняет `orders`, а площадка/очередь
// остаются до явного `stop`.
//
// Готовая площадка (`ORDER_STATE_UNIT_READY`) — исключение: там уход по приказу
// разрешён и обрабатывается `ORDER_ACTIVATE` (см.
// [`crate::production::builder_awaiting_exit`]), потому что здание уже
// достроено.

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

/// `UnitInfo::GetTargetUnitAngle` (`unitinfo.cpp:4147`) — направление, в
/// котором строитель въезжает в центр ленты и съезжает с неё.
///
/// * `TAPE_POSITIONING_ENTER` — по клеткам: лента находится в клетке строителя
///   (`Access_GetConstructionUtility(team, grid_x, grid_y)`), её `grid` — якорь
///   2×2. Сравнение `grid_x/grid_y` с якорем даёт ровно один из четырёх
///   диагональных углов: совпали обе оси — `SE`, только `x` — `NE`, только
///   `y` — `SW`, иначе `NW`. Строитель 2×2 всегда стоит в одной из четырёх
///   клеток ленты, поэтому штатно угол всегда диагональный (45°).
/// * остальные состояния (`LEAVE`) — к `move_to` (C++
///   `move_to_grid_x/move_to_grid_y`, в порте — `OrderData::move_to`): знак
///   разницы по каждой оси независимо; пиксельная позиция строителя — центр
///   его клетки (`UnitInfo::SetPosition`; в порте `x/y` не пересчитываются при
///   ходьбе, поэтому берётся клетка).
pub fn get_target_unit_angle(state: &GameState, unit: &UnitInfo, move_to: Point) -> i32 {
    const UNIT_ANGLE_NE: i32 = 1;
    const UNIT_ANGLE_SE: i32 = 3;
    const UNIT_ANGLE_SW: i32 = 5;
    const UNIT_ANGLE_NW: i32 = 7;

    if unit.state == UnitOrderState::TapePositioningEnter {
        // `Access_GetConstructionUtility(team, grid_x, grid_y)`: у здания
        // лента зарегистрирована в хэше во всех четырёх клетках 2×2, но её
        // `grid` — якорь (верхний левый угол).
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

/// Знак «въезда» строителя в ленту по каждой оси — сколько пикселей он уже
/// проехал от центра своей клетки к центру площадки
/// (`OffsetDrawZones(DIRECTION_OFFSETS[angle] * 8)`, `TAPE_STEPS` тиков).
///
/// Для 2×2-ленты строитель стоит в одной из четырёх её клеток, поэтому въезд
/// всегда диагональный: `−1` с той стороны, где якорь совпал с его клеткой.
/// Для 1×1-ленты (инженер) строитель ровно в её клетке — въезда нет.
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

/// Куда сдвигается клетка строителя после четырёх шагов по 8 px
/// (`PositionInTape`: `grid_x = x / GFX_MAP_TILE_SIZE`).
///
/// Пиксель строителя — центр его клетки плюс `32 * enter` (въезд в ленту) плюс
/// `32 * leave` (съезд). `x / 64` от такой суммы даёт сдвиг клетки:
/// `enter + leave >= 1` → `+1`, `<= -2` → `-1`, иначе клетка не меняется.
fn tape_leave_shift(enter: i32, leave: i32) -> i16 {
    match enter + leave {
        sum if sum >= 1 => 1,
        sum if sum <= -2 => -1,
        _ => 0,
    }
}


/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4414`).
///
/// Портирован цикл позиционирования: `INIT → ENTER`, `DEINIT → LEAVE`,
/// доворот `Paths_UpdateAngle` к [`get_target_unit_angle`], `moved` до 4, затем
/// `RestoreOrders`. Как в C++, пока строитель не довернулся к центру, `moved`
/// **не растёт** — сначала поворот на месте, потом скольжение (четыре шага
/// `OffsetDrawZones` по 8 px).
///
/// `move_to` — цель съезда (C++ `move_to_grid_x/move_to_grid_y`): на завершении
/// LEAVE C++ пересчитывает клетку от пиксельной позиции
/// (`grid_x = x / GFX_MAP_TILE_SIZE`) — строитель оказывается в клетке ленты,
/// ближайшей к цели, и дальше идёт пешком (`ActivateEngineer` → `ORDER_MOVE`).
pub fn position_in_tape(state: &mut GameState, unit: &mut UnitInfo, move_to: Point) {
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
        // TODO(P3/P5): проверки `UnitsManager_IsEngaged` и занятости площадки
        // (`UnitInfo_IsSiteEngaged` требует `Access_GetConstructionUtility`).
        let old_state = unit.state;
        restore_orders(unit);
        unit.moved = 0;

        if unit.unit_type == UnitType::MasterBuilder {
            // `PrepareConstructionSite` + `ScaleUnit(this, SHRINK)` — P3/P5.
            let _ = scale_unit(unit, UnitOrderState::Shrink);
        } else if unit.unit_type == UnitType::Constructor
            && old_state == UnitOrderState::TapePositioningEnter
        {
            // `PrepareConstructionSite` уже выполнен `StartBuilding`-ом
            // (площадка ставится сразу при заказе, см.
            // `production::deploy_construction_site`). C++ ещё раз доворачивает
            // строителя на 45° (`Paths_UpdateAngle(this, (angle + 1) & 0x07)`) и
            // рисует рабочую позу `DrawSpriteFrame(image_index + 16)`; кадр
            // выбирается рендером по `ORDER_BUILD`/`BUILD_IN_PROGRESS`.
            let target_angle = (i32::from(unit.angle) + 1) & 0x07;
            let _ = paths_update_angle(unit, target_angle);
        } else if old_state != UnitOrderState::TapePositioningEnter {
            // `Hash_MapHash.Remove(this)` → `grid_x = x / GFX_MAP_TILE_SIZE` →
            // `Hash_MapHash.Add(this)`: клетка пересчитывается от пиксельной
            // позиции после четырёх сдвигов по 8 px.
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
        // Пока `moved == 0`, каждый тик — один шаг поворота; сдвиг и счётчик
        // начинаются только после того, как угол совпал с целевым.
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

/// `Paths_UpdateAngle` (`paths.cpp:258`): один шаг поворота к `angle`;
/// `true`, если юнит ещё поворачивается (движение в этот тик не начинается).
///
/// Копирует `UnitInfo::UpdateAngle` (`unitinfo.cpp:1161`) в части, влияющей на
/// симуляцию: у юнита с `TURRET_SPRITE` башня поворачивается вместе с корпусом
/// (`turret_angle = (turret_angle + image_diff) & 7`, где `image_diff` — тот же
/// шаг в 45°). Кэш кадров (`image_index`/`turret_image_index`/draw zones) — UI,
/// в max_core не хранится.
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
/// C++ вызывает это из `UnitInfo::Move` при завершении клетки
/// (`moved == GFX_MAP_TILE_SIZE`): корпус делает **один** шаг в 45° в сторону
/// следующего шага маршрута (если он есть), а не разворачивается мгновенно.
/// Следующий `GroundPath::Execute` доразворачивает юнита и только потом
/// начинает движение — поэтому поворот и движение всегда согласованы.
///
/// Функция вынесена для тестов: в [`unit_move`] тот же шаг выполняется по
/// снимку `PathStep`, чтобы не держать заимствование `unit.path` во время
/// изменения `unit.angle`.
pub fn ground_path_update_unit_angle(unit: &mut UnitInfo, path: &GroundPath) -> bool {
    let next = path.step_index as usize + 1;

    if let Some(step) = path.steps.get(next) {
        paths_update_angle(unit, paths_get_angle(i32::from(step.x), i32::from(step.y)))
    } else {
        false
    }
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
            self.world.remove_path_request(self.id);
        }

        delete_path
    }

    /// Записывает рабочую копию в состояние, не завершая контекст (нужно перед
    /// вызовами боевых функций, которые работают с `GameState`).
    pub fn sync_to_world(&mut self) {
        if self.world.has_unit(self.id) {
            self.world.write_unit(self.id, self.unit.clone());
            self.world.set_order_data(self.id, self.data);
        }
    }

    /// Перечитывает рабочую копию из состояния (после боевых функций).
    pub fn sync_from_world(&mut self) {
        if let Some(unit) = self.world.unit(self.id) {
            self.unit = unit.clone();
        }
        self.data = self.world.order_data(self.id);
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
        self.world.remove_path_request(self.id);
        blocked_on_path_request(&mut self.unit, mode, self.world.disable_fire);
        self.world.add_to_delayed_reaction_list(self.id);
    }

    /// `MoveFinished` текущего юнита (+ отложенная реакция и оценка целей).
    pub fn move_finished(&mut self, mode: bool) {
        move_finished(&mut self.unit, mode, self.world.disable_fire);

        // `Ai_CheckMines(this)` (`ai.cpp:365`) — миноукладчик сообщает командам.
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

    /// `UnitsManager_EndAttack` для текущего юнита.
    pub fn end_attack(&mut self) {
        self.world.end_attack(self.id);
        self.unit.live_attack = 0;
    }

    /// `UnitsManager_PinAttackTargets` для текущего юнита.
    pub fn pin_attack_targets(&mut self) {
        self.sync_to_world();
        self.world.pin_attack_targets(self.id);
        self.unit.live_attack = self
            .world
            .unit(self.id)
            .map(|unit| unit.live_attack)
            .unwrap_or(0);
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
        // C++ `UnitInfo::ExecuteMove`: ракета без пути взрывается в текущей
        // клетке — родитель наносит удар, сама ракета уничтожается.
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

/// `UnitPath::Execute` для трёх реализаций пути.
fn path_execute(ctx: &mut OrderContext<'_>) -> bool {
    match ctx.unit.path.clone() {
        Some(UnitPath::Ground(_)) => ground_path_execute(ctx),
        Some(UnitPath::Air(path)) => {
            // `AirPath::Execute` (`airpath.cpp:175-295`) для снаряда
            // (`MISSILE_UNIT`) шагает **попиксельно** по прямой `m_start_* →
            // m_end_*`; `unit_move` (клетка за тик) к снарядам не применяется.
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

/// `AirPath::Execute` (`airpath.cpp`) — шаг к конечной клетке по `m_angle`;
/// пиксельная интерполяция отрезка (`m_delta_*`) в lockstep-модели заменена
/// переходом на одну клетку за тик (расхождение 1 в шапке модуля).
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
        // C++ `AirPath::Execute` (`airpath.cpp:186-199`): проверка скорости и
        // двигателя стоит **внутри** `if (unit->flags & MOBILE_AIR_UNIT)`. У
        // снарядов (`MISSILE_UNIT`, `speed = 0` из `SC_ATTRI`) этой проверки
        // нет — они летят по `m_length` и завершаются `Paths_FinishMove`.
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

/// `BuilderPath::Execute` (`builderpath.cpp`) — шаг строителя к цели.
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

/// `AirPath::Execute` (`airpath.cpp:175-295`) для **снаряда**: попиксельный шаг
/// по прямой от точки пуска к цели.
///
/// C++ `AirPath::Execute` каждый вызов:
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
/// `m_delta_*` — 22-битная дробь шага (пиксели/тик), `m_start_*` — пиксель
/// центра клетки пуска, `m_length` — число тиков (≈ число клеток дистанции).
/// Итог: снаряд проходит **тот же отрезок**, что и в оригинале, а не 8-дирек-
/// ционную ломаную по клеткам. Клетка (`grid`) пересчитывается из пикселя, а
/// след (`Paths_DrawMissile`) ставится из точки «было → стало» — тоже по линии.
fn missile_path_execute(ctx: &mut OrderContext<'_>, path: &AirPath) {
    let mut path = *path;

    // Снаряд не доворачивает (`AirPath::AirPath`: `m_angle = unit->angle`),
    // поэтому `Paths_UpdateAngle` в C++ сразу возвращает false; шаг
    // выполняется, только когда угол уже совпал.
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

    // `OffsetDrawZones`: пиксельная позиция спрайта снаряда.
    ctx.unit.x = position_x.clamp(0, i32::from(u16::MAX)) as u16;
    ctx.unit.y = position_y.clamp(0, i32::from(u16::MAX)) as u16;

    let grid = Point::new(
        (position_x >> 6).clamp(0, i32::from(crate::geo::MAP_WIDTH) - 1) as i16,
        (position_y >> 6).clamp(0, i32::from(crate::geo::MAP_HEIGHT) - 1) as i16,
    );

    if grid != old_grid {
        ctx.unit.grid = grid;
        // `Paths_DrawMissile(unit, position_x, position_y)` (`airpath.cpp:231`,
        // `:362-406`): три частицы в **покинутую** клетку (`grid_x/grid_y` ещё
        // старые: C++ зовёт это до обновления `unit->grid_*`) на отрезке «старый
        // пиксель → новый пиксель».
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

    // `Paths_FinishMove` (`airpath.cpp:408-420`) для `MISSILE_UNIT`:
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

/// `Paths_DrawMissile` (`airpath.cpp:362-406`) — след за летящим снарядом.
///
/// C++ при каждом переходе снаряда в новую клетку (`AirPath::Execute`,
/// `airpath.cpp:229-232`, ветка `grid_x || grid_y` для `MISSILE_UNIT`) зовёт
/// `Paths_DrawMissile` и ставит **три** юнита-эффекта в клетку, которую снаряд
/// покидает:
/// * `ROCKET` → `RKTSMOKE` (5 кадров `D_RKTSMOKE`), `TORPEDO` → `TRPBUBLE`
///   (3 кадра `D_TRPBUBLE`);
/// * `UnitsManager_DeployUnit(..., skip_map_status_update = true)` с флагами из
///   `SC_UNITS`: у обоих `EXPLODING|ANIMATED|MISSILE_UNIT` (у `TRPBUBLE` ещё
///   `GROUND_COVER`), поэтому `UnitInfo` сразу получает
///   `ORDER_EXPLODE`/`ORDER_STATE_DESTROY` (`units_manager.cpp:2390-2393`);
/// * кадры листает `UnitsManager_ProgressExplosion` (`units_manager.cpp:4006`):
///   `moved & 1` → `image_index + 1`, удаление на `image_index ==
///   image_index_max` (4 у дыма, 2 у пузырей);
/// * `OffsetDrawZones` разносит три частицы вдоль отрезка «старый пиксель снаряда
///   → новый пиксель» на 0, 1/3 и 2/3 — это только отрисовка (`unit->x/y` у самих
///   частиц не меняются).
///
/// Порт хранит разнесение в пиксельных `x/y` частиц (рендер эффектов берёт их
/// вместо клетки), сама клетка остаётся C++-шной (покинутая). Спавн не зависит
/// от видимости: в C++ `Paths_DrawMissile` вызывается лишь при
/// `IsVisibleToTeam(GameManager_PlayerTeam)`, но `GameManager_PlayerTeam` —
/// локальное UI-поле, и привязка к нему развела бы состояние клиентов
/// (DESIGN §6.1). Видимость частицы выставляет `UnitInfo::InitStealthStatus`
/// (своя команда; `skip_map_status_update = true`), поэтому чужие следы не
/// рисуются — как в C++.
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

    // `index = 3` в C++: ровно три частицы на клетку.
    for index in 0..3 {
        let Some(particle_id) = state.spawn_unit(particle_type, unit.team, spawn_grid, 0) else {
            continue;
        };

        if let Some(mut particle) = state.unit(particle_id).cloned() {
            // `OffsetDrawZones(unit->x + (delta >> 16) - particle->x)`:
            // 0, 1/3 и 2/3 отрезка.
            particle.x = (start_x + delta_x * index / 3) as u16;
            particle.y = (start_y + delta_y * index / 3) as u16;
            // Видимость — как `UnitInfo::InitStealthStatus` при
            // `UnitsManager_DeployUnit`: частицу видит только своя команда
            // (у `Paths_DrawMissile` `Access_UpdateMapStatus` пропускается).
            state.write_unit(particle_id, particle);
        }
    }
}

/// `UnitInfo::Move` (`unitinfo.cpp:1730`) — завершение перехода клетки.
///
/// В P4 один вызов завершает одну клетку (расхождение 1 в шапке модуля).
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

    // Снаряды сюда не доходят: [`path_execute`] ведёт их попиксельно
    // ([`missile_path_execute`]) и след ставится из пиксельного отрезка
    // (`Paths_DrawMissile`, `airpath.cpp:229-232`).

    // C++ `Move()` при `moved == GFX_MAP_TILE_SIZE`:
    // `path_steps[step_index]->x -= offset_x` — шаг «съедается» переходом.
    if let Some(UnitPath::Ground(path)) = ctx.unit.path.as_mut() {
        if let Some(step) = path.steps.get_mut(path.step_index as usize) {
            step.x = step.x.wrapping_sub(offset.x as i8);
            step.y = step.y.wrapping_sub(offset.y as i8);
        }
    }

    // C++ `UnitInfo::Move` (`unitinfo.cpp:1835`): на завершении клетки
    // `path->UpdateUnitAngle(this)` делает один шаг поворота к следующему шагу
    // маршрута. Нужен временный снимок шага, чтобы не держать заимствование
    // `ctx.unit.path` одновременно с `&mut ctx.unit`.
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
        // `Ai_EnableAutoSurvey` (`ai.cpp:184`) создаёт `TaskAutoSurvey`.
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
            // TODO(P3/P5): `UnitsManager_DeployMasterBuilder` — разрушение
            // конструктора и постановка шахты/генератора/плиты.
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
            // C++ ставит `PendingAirGroupLeader` только для члена группового
            // списка (`UnitInfo::GetUnitList() != nullptr`).
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
        && (ctx.unit.path.is_none() || move_to.x != enemy.grid.x || move_to.y != enemy.grid.y)
        && (!enemy_is_moving || enemy.state == UnitOrderState::ExecutingOrder)
    {
        // `Paths_RequestPath(unit, SameClassBlocks)` — путь к позиции атаки.
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

        if ctx.world.network_game && ctx.unit.is_air() {
            // `Remote_SendNetPacket_38(unit)` — сеть (max_net), не max_core.
            return false;
        }
    }

    ctx.unit.state == UnitOrderState::InProgress
}

/// `UnitsManager_InitUnitPath` (`units_manager.cpp:4776`).
///
/// Воздушный путь считается сразу (`Paths_GetAirPath`); наземный ставится в
/// очередь [`pathfinding::request_path`] (синхронный pathfinding, DESIGN
/// §1.4/риск 2), а приказ переходит в `MOVE_GETTING_PATH`/`NEW_ORDER` до
/// применения результата в `poll_results`.
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

/// Линейный маршрут (`GroundPath::AppendLinearSteps`) — используется
/// сетевым слоем/тестами (пакет 38), не симуляционным поиском.
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
                // `Attack(fire_on)` работает с состоянием партии: сначала
                // фиксируем рабочую копию, затем перечитываем её после боя.
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
                // Анимация стройки двигает `moved` (как в C++).
                ctx.unit.moved = ctx.unit.moved.wrapping_add(1);
            }
        }
        UnitOrderState::BuildCancel | UnitOrderState::BuildAbort => {
            ai_task_flag(ctx.world, "Build cancel");
            cancel_building(&mut ctx.unit);
            // `Access_DestroyUtilities(grid, true, false, false, false)`
            // (`unitinfo.cpp:4269`): только утилиты площадки и плита.
            // В C++ строитель уже стоит на площадке; в порте он остаётся на
            // исходной клетке, поэтому убираем площадку из `target_grid`.
            access::destroy_utilities(ctx.world, ctx.unit.target_grid, true, false, false, false);
        }
        UnitOrderState::SelectSite => {
            animate(&ctx.unit);
        }
        UnitOrderState::BuildClearing => {
            ai_task_flag(ctx.world, "Build clearing");
            // TODO(P3/P5): UnitsManager_BuildClearing — разрушение
            // застройки, обломки, утилиты, возврат юнитов.
        }
        UnitOrderState::UnitReady => {
            // C++ `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2891`):
            // `if (unit->path) UnitsManager_BuildNext(unit); else unit->Animate();`
            // — строитель, достроивший здание, просто стоит и анимируется;
            // никакого автоматического ухода нет. Приказ на уход отдаёт
            // **игрок** кликом по клетке (`game_manager.cpp:6879-6890`:
            // `ORDER_STATE_UNIT_READY` → `ORDER_ACTIVATE`/`IN_TRANSITION`,
            // сюда приходит уже `ORDER_ACTIVATE`), у ИИ — задача
            // `TaskActivate` (`crate::unit_order::issue_builder_exit`).
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
    ai_task_flag(ctx.world, "Activation");

    match ctx.unit.state {
        UnitOrderState::ExecutingOrder => {
            // `UnitsManager_ActivateUnit(unit)` (`units_manager.cpp:3826`):
            // `unit` — контейнер (завод/транспорт), `unit->GetParent()` —
            // готовый юнит, которого игрок выводит кликом (пакет 08/P3) или ИИ
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

/// `UnitsManager_ActivateUnit` (`units_manager.cpp:3826-3913`) — вывод готового
/// юнита из контейнера.
///
/// Вызывается для **контейнера**: у завода `parent_unit` — готовый юнит
/// (`UnitInfo::SpawnNewUnit`, `unitinfo.cpp:3513`:
/// `SetParent(factory_unit.Get())`), у транспорта — пассажир. C++
/// (`units_manager.cpp:3827-3936`):
///
/// ```text
/// client = unit->GetParent();                      // готовый юнит
/// if ((client->MOBILE_AIR_UNIT && client->speed > 0) ||
///     (!Paths_IsOccupied(move_to) && !client->MOBILE_AIR_UNIT &&
///      Access_IsAccessible(client->type, move_to, SameClassBlocks))) {
///     --unit->storage; unit->SetParent(nullptr);
///     if (unit->GetBuildList().GetCount() > 0) { ORDER_BUILD/EXECUTING_ORDER;
///         build_time = turns; StartBuilding(); }       // следующий заказ
///     else { ORDER_AWAIT/EXECUTING_ORDER; }
///     if (client->MOBILE_AIR_UNIT) {
///         client->SetParent(nullptr); client->move_to = unit->move_to;
///         client->SetOrder(ORDER_MOVE); SetOrderState(INIT);
///         ScaleUnit(client, EXPAND);                   // взлёт из завода
///     } else {
///         Access_TriggerEnemyMine(move_to);
///         UnitsManager_UpdateMapHash(client, move_to); // юнит в клетке клика
///         client->SetOrder(ORDER_AWAIT); SetOrderState(EXECUTING_ORDER);
///         client->InitStealthStatus(); ScaleUnit(client, EXPAND);
///     }
/// } else { unit->RestoreOrders(); }
/// ```
///
/// Воздушный юнит выходит **сам** (`ORDER_MOVE`/`INIT` → `ExecuteMove` →
/// `ORDER_TAKE_OFF`), наземный/морской — переставляется в клетку клика. Если
/// клетка занята/непроходима — приказ возвращается к отложенному (завод остаётся
/// в `ORDER_BUILD`/`UNIT_READY`), ровно как в C++.
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
        // Воздушному юниту площадка не нужна — только топливо/двигатель:
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

    // `--unit->storage; unit->SetParent(nullptr);` — контейнер отдал юнита.
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

        // `UnitsManager_ScaleUnit(client, ORDER_STATE_EXPAND)`: самолёт
        // «вырастает» из завода (`scaler_adjust` 4 → 0), затем
        // `RestoreOrders` возвращает `ORDER_MOVE`/`INIT` → взлёт.
        ctx.world.scale_unit(client_id, UnitOrderState::Expand);
    } else {
        // `UnitsManager_UpdateMapHash(client, move_to)` +
        // `UnitInfo::UpdateGridPosition` (`unitinfo.cpp:4995-5011`): клетка и
        // пиксельный центр клетки.
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
    // `Access_DestroyUtilities(grid, false, false, false, false)` — снимаются
    // только ленты/конусы строительной площадки.
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

/// `TaskActivate::Activate` — первая проходимая клетка вокруг контейнера.
///
/// C++ (`taskactivate.cpp:106-167`): старт в
/// `(container.grid_x - 1, container.grid_y + unit_size)`, где `unit_size` —
/// габарит **контейнера** (2 у здания, 1 у транспорта), затем четыре
/// направления (`N, E, S, W` — `direction += 2`) по `unit_size + 1` шагов
/// накопительно; первая клетка, проходимая для **выводимого** типа
/// (`Access_IsAccessible(client->type, …, SameClassBlocks)`), становится целью.
pub fn activation_exit_cell(
    state: &GameState,
    client: &UnitInfo,
    container: &UnitInfo,
) -> Option<Point> {
    // Габарит **контейнера**: 2 у 2×2-структуры, 1 у 1×1 (в т.ч. турель —
    // `Footprint::Single`, хотя семантически это здание).
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

/// Строитель уходит с достроенной площадки — `TaskActivate::Activate`
/// (`taskactivate.cpp:104-167`), путь **ИИ**.
///
/// C++ у игрока приказ на уход рождается в обработчике клика
/// (`game_manager.cpp:6879-6890`): `move_to_grid` = клетка под курсором и
/// `UnitsManager_SetNewOrder(selected, ORDER_ACTIVATE, ORDER_STATE_IN_TRANSITION)`;
/// порт отправляет то же самое пакетом 08/P3 (`hud::build_builder_exit_order`).
/// У ИИ клика нет: `TaskCreateBuilding::Activate` вешает `TaskActivate`, и тот
/// ищет первую проходимую клетку вокруг здания (`builder_exit_cell`) и отдаёт
/// тот же приказ. `true` — приказ выставлен.
pub fn issue_builder_exit(state: &mut GameState, builder_id: UnitId) -> bool {
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

/// Завод выводит готовый юнит — `TaskCreateUnit::BeginTurn`
/// (`taskcreateunit.cpp:147-155`) через `TaskActivate::Activate`
/// (`taskactivate.cpp:135-144`), путь **ИИ**.
///
/// У игрока тот же приказ рождается кликом по клетке у завода
/// (`game_manager.cpp:6918-6926`, ветка `CURSOR_ACTIVATE`) — порт отправляет его
/// пакетом 08/P3 (`hud::build_activate_order`). У ИИ клика нет: задача ищет
/// первую проходимую клетку вокруг завода ([`activation_exit_cell`]) и ставит
/// **заводу** `ORDER_ACTIVATE`/`ORDER_STATE_EXECUTING_ORDER` (у завода
/// `parent_unit` — готовый юнит, `UnitInfo::SpawnNewUnit`,
/// `unitinfo.cpp:3513`), после чего `UnitsManager_ActivateUnit` выводит юнита.
///
/// `true` — приказ выставлен.
pub fn issue_factory_exit(state: &mut GameState, factory_id: UnitId) -> bool {
    let Some(factory) = state.unit(factory_id).cloned() else {
        return false;
    };

    // `TaskActivate::Activate`: приказ только для завода в
    // `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` (`taskactivate.cpp:70`);
    // готовый юнит — в `ORDER_IDLE` (`unitinfo.cpp:3499`).
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

    // `unit_parent->SetParent(unit_to_activate)` — у завода родитель уже готовый
    // юнит (`SpawnNewUnit`), связь не меняется.
    state.set_order(
        factory_id,
        UnitOrder::Activate,
        UnitOrderState::ExecutingOrder,
    );

    true
}

/// `UnitsManager_BuildingReady` (`units_manager.cpp:4183`) —
/// `UnitsManager_ProcessOrderIdle` для достроенного здания.
///
/// Здание стоит на площадке в `ORDER_IDLE`/`BUILDING_READY` (и не рисуется)
/// до тех пор, пока его родитель-строитель не покинет габариты здания. Тогда
/// лента и конусы снимаются (плита `remove_slabs = false` остаётся
/// фундаментом), приказы восстанавливаются, `scaler_adjust = 4`, и здание
/// раскрывается `ScaleUnit(EXPAND)` — ровно порядок C++.
fn building_ready(ctx: &mut OrderContext<'_>) {
    let parent = ctx.unit.parent_unit.and_then(|id| ctx.other(id));
    let is_found;

    match parent {
        Some(parent) if parent.orders != UnitOrder::Idle => {
            if parent.hits > 0 {
                if parent.orders == UnitOrder::Move
                    && parent.state != UnitOrderState::ExecutingOrder
                {
                    // Строитель ещё идёт — здание ждёт.
                    is_found = false;
                } else {
                    is_found = !production::unit_bounds_contains(&ctx.unit, parent.grid);

                    // Строитель вернулся в габариты 2×2 (путь не вывел его с
                    // площадки): C++ `ReturnToConstructionSite` возвращает его
                    // в центр, чтобы он ушёл снова.
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
                    // Строитель погиб на площадке: утилиты и здание снимаются
                    // (`Access_DestroyUtilities(grid, true, ...)`, затем
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

    // `Access_UpdateMapStatus(unit, true)` — покрытие пересчитывается
    // `GameState::write_unit` при смене приказа/клетки; отдельного вызова в
    // порте не требуется.
}

/// `UnitInfo::ReturnToConstructionSite` (`unitinfo.cpp:4488`).
fn return_to_construction_site(ctx: &mut OrderContext<'_>, builder: UnitInfo) {
    // Строитель без скорости не сдвинется с места: оставить здание ждать
    // (иначе повторы «вернуть в центр → уйти» крутятся без прогресса и не
    // дают ИИ закончить ход).
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
/// Спавнит юнит-эффект (`HITEXPLD`/`BLDEXPLD`/`LNDEXPLD`/`AIREXPLD`/`SEAEXPLD`)
/// с флагами класса, как C++ (`base_unit.SetFlags(...)`).
fn start_explosion(ctx: &mut OrderContext<'_>) {
    refresh_screen(&ctx.unit);

    // `UnitsManager_StartExplosion` (`units_manager.cpp:3915-3930`):
    // `if (unit->hits > 0) PlaySfx(unit, SFX_TYPE_HIT);   // попадание
    //  else                PlaySfx(unit, SFX_TYPE_EXPLOAD); // взрыв`
    // Звук принадлежит **самому юниту** (его таблица `SC_UNITS`), поэтому в
    // событии — id цели; ресурс/громкость выбирает клиент (`max_app::sfx`).
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
        // `units_manager.cpp:3932-3946`: пехота/диверсант умирают собственной
        // 8-кадровой анимацией `image_base = 168 + (angle / 2) * 8`.
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
        // Лайаутный футпринт (`Footprint::Quad`), а не `is_building()`: у турелей
        // (`Building` + `Single`) взрыв наземный (`LandExplosion`), как в C++.
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

    // Флаги самого эффекта взрыва — часть сериализуемого состояния (`SC_UNITS`
    // взрыва): маска строится из битов погибшего юнита, а не классифицируется.
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

    // Взрыв — юнит команды погибшего, но видеть его должны те, кто видел
    // саму цель (`UnitInfo::InitStealthStatus` оставляет эффект видимым только
    // своей команде, а `Access_UpdateMapStatus` для него в C++ пропускается:
    // `DeployUnit(..., skip_map_status_update = true)`). Переносим видимость
    // цели на эффект — иначе анимация попадания не рисуется атакующему.
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
        // `units_manager.cpp:4015-4023`: на середине анимации взрыв отпускает
        // родителя (погибший юнит); если у того `hits == 0`, он удаляется
        // `UnitsManager_BuildClearing(parent, true)`. Без этого погибший юнит
        // навсегда остаётся в `ORDER_STATE_DESTROY`, а `UnitsManager_ProcessOrder`
        // каждый тик выставляет `OrdersPending` — ход ИИ не закрывается.
        //
        // `BuildClearing` (а не `DestroyUnit`) — именно здесь C++ оставляет
        // обломки: `SMLRUBLE` для юнита и `LRGRUBLE` для здания
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

    // `Access_TriggerEnemyMine` (`units_manager.cpp:4035`): мина под
    // высадившимся юнитом взрывается.
    let landing_grid = client.grid;
    let landing_team = client.team;
    access::trigger_enemy_mine(ctx.world, landing_team, landing_grid);

    restore_orders(&mut client);
    ctx.world.write_unit(parent_id, client);

    // `Access_UpdateMapStatus` — P5 (карта видимости); клетка уже в MapHash.

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
        // `UnitsManager_StartClearing`: спавн конусов/ленты, `ClearUnitList`,
        // state = IN_PROGRESS, для LRGTAPE — AWAIT_TAPE_POSITIONING.
        // TODO(P3/P5): DeployUnit(LRGCONES/LRGTAPE/SMLCONES/SMLTAPE).
        ctx.unit.state = UnitOrderState::InProgress;

        // Большая лента (`LRGTAPE`) — только у 2×2-родителя (футпринт, не
        // семантика: у турели `SMLTAPE`).
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

    // Занятый самолёт не снижается в ангар; посадочная площадка держит его на
    // карте.
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
            // Занятый пассажир не подбирается до конца атаки/стелс-действия.
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

    // Перенос материалов/топлива/золота между юнитом и родителем: знак
    // `transfer_cargo` задаёт направление, `ProcessTransfer` сам вызывает
    // `RestoreOrders`. `ctx.commit()` в конце пишет снимок `ctx.unit`, поэтому
    // перечитываем юнита после процессора — иначе правки `storage`/приказов
    // затрёт устаревшая копия.
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
        // См. `ProcessOrderBuild`: порт не двигает строителя на площадку,
        // поэтому утилиты снимаются из `target_grid`, а не из `grid`;
        // набор флагов — как в `CancelBuilding` (`true, false, false, false`).
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
                // здание заняло окончательный приказ и становится членом
                // комплекса (слияние по коннекторам).
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
                // `UnitInfo::ShakeSabotage` (`unitinfo.cpp:4680`): счётчик
                // тряски 0..15; полный цикл возвращает цель в исходное
                // состояние, затем диверсант «расширяется».
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

/// `UnitsManager_AttemptStealthAction` — тонкая обёртка над
/// [`combat::attempt_stealth_action`] (см. вызов в процессоре приказа).
#[allow(dead_code)]
fn attempt_stealth_action(ctx: &mut OrderContext<'_>) -> bool {
    combat::attempt_stealth_action(ctx.world, &mut ctx.unit)
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
    ai_task_flag(ctx.world, "Upgrading");

    // TODO(P3): проверка версии атрибутов и материалов комплекса
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
        // Мина видна командам всех surveyor-юнитов в 3×3 (`SpotByTeam`).
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
    if ctx.unit.shake_effect_state != 0 && !access::is_engaged_by_infiltrator(ctx.world, &ctx.unit)
    {
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
        assert_eq!(
            unit.speed,
            unit.base_values.get_attribute(ATTRIB_SPEED) as u16 - 4
        );
    }

    #[test]
    fn move_order_is_replaced_by_new_order() {
        let (mut state, id) = state_with_tank();
        order_move_to(&mut state, id, Point::new(30, 10));
        // Первый тик ставит запрос пути (`NEW_ORDER`), второй применяет его
        // (`poll_results` → `IN_PROGRESS`) — как в C++ с `PathsManager`.
        state.tick();
        state.tick();
        assert_eq!(state.unit(id).unwrap().state, UnitOrderState::InProgress);

        // Смена приказа во время движения: prior фиксирует Move/InProgress.
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
        // Обнулим скорость: шаг пути не начинается, `MoveFinished` сбрасывает
        // движение, но приказ остаётся MOVE (как в `GroundPath::Execute` при
        // `speed == 0`: состояние EXECUTING_ORDER, путь сохраняется до
        // восстановления скорости в начале хода).
        state.unit_mut(id).unwrap().speed = 0;
        // Тик 1 — запрос пути; тик 2 — путь применён и `GroundPath::Execute`
        // при `speed == 0` оставляет приказ MOVE с состоянием EXECUTING_ORDER.
        state.tick();
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.orders, UnitOrder::Move);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert!(unit.path.is_some());
        // `MoveFinished` выставил `delayed_reaction`, но `AssessAttacks` в
        // конце того же тика (`ProcessOrders`) очистил его: у команды нет
        // отложенных целей (`UnitsManager_ClearDelayedReactions`).
        assert_eq!(unit.delayed_reaction, 0);
        assert_eq!(unit.moved, 0);

        // Начало хода возвращает скорость (`UpdateProduction`) — движение
        // продолжается с того же места.
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

    /// Угол 0..7 → (dx, dy) шага (`DIRECTION_OFFSETS`, `path.hpp`).
    fn step_for_angle(angle: i32) -> PathStep {
        let offset = DIRECTION_OFFSETS[(angle & 7) as usize];
        PathStep::new(offset.x as i8, offset.y as i8)
    }

    /// Танк с вручную заданным наземным путём и приказом `Move/IN_PROGRESS`.
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

    /// Круговая разница углов `a - b` в диапазоне −4..=3.
    fn angle_delta(a: u8, b: u8) -> i32 {
        ((i32::from(a) - i32::from(b) + 12) & 7) - 4
    }

    /// C++ `GroundPath::UpdateUnitAngle`: на завершении клетки — ровно один
    /// шаг 45° к следующему шагу маршрута; без следующего шага угол стоит.
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

        // Следующего шага нет — поворот не начинается (последняя клетка).
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

    /// `UnitInfo::UpdateAngle`: башня поворачивается вместе с корпусом.
    #[test]
    fn body_turn_rotates_turret_like_cpp_update_angle() {
        let mut turreted = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        turreted.flags |= TURRET_SPRITE;
        turreted.angle = 2;
        turreted.turret_angle = 0;

        assert!(paths_update_angle(&mut turreted, 3));
        assert_eq!(turreted.angle, 3);
        assert_eq!(turreted.turret_angle, 1);

        // Без флага турели второе поле не трогается.
        let mut plain = UnitInfo::new(UnitType::Scout, TeamId::RED, UnitId(2));
        plain.angle = 2;
        plain.turret_angle = 6;
        assert!(paths_update_angle(&mut plain, 3));
        assert_eq!(plain.angle, 3);
        assert_eq!(plain.turret_angle, 6);
    }

    /// Прямой ход по всем 8 направлениям: юнит поворачивается к направлению
    /// шага, движется только «лицом» по ходу и сохраняет последний угол.
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
                    // На прямой следующий шаг совпадает — угол обязан быть
                    // направлением движения сразу после шага.
                    assert_eq!(
                        i32::from(unit.angle),
                        direction,
                        "прямой ход {direction}: угол не совпал с шагом"
                    );
                    moves += 1;
                    previous = unit.grid;
                }

                // Поворот — не более одного шага 45° за тик.
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

            // После остановки угол не меняется.
            for _ in 0..8 {
                state.tick();
                assert_eq!(i32::from(state.unit(id).unwrap().angle), direction);
            }
        }
    }

    /// L-маршрут: на завершении текущей клетки корпус делает один шаг к
    /// следующей (`GroundPath::UpdateUnitAngle`), затем доразворачивается в
    /// `Execute` и только после этого двигается. Движение всегда по ходу.
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

        // Тик 1: угол уже на восток — шаг на (11,10); следующий шаг ещё на
        // восток, поэтому на границе клетки поворота нет.
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.grid, Point::new(11, 10));
        assert_eq!(unit.angle, 2);

        // Тик 2: шаг на (12,10); на границе начинается поворот к следующему
        // шагу (север): E → NE.
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.grid, Point::new(12, 10));
        assert_eq!(unit.angle, 1, "на границе клетки — один шаг 45° к северу");

        // Тик 3: доразворот NE → N без движения (как `GroundPath::Execute`).
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.grid, Point::new(12, 10));
        assert_eq!(unit.angle, 0);

        // Тик 4: движение на север.
        state.tick();
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.grid, Point::new(12, 9));
        assert_eq!(unit.angle, 0);

        // На каждом движущемся тике угол совпадает с направлением перехода
        // (допуск ±1 — только начатый на границе поворот к следующему шагу).
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

    /// Два одинаковых прогона L-маршрута дают побайтово одинаковые снапшоты на
    /// каждом тике: `angle` — часть состояния и меняется детерминированно.
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

        // `angle` входит в снапшот (CRC состояния).
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
        // Родитель-транспорт и пассажир.
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
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.state, UnitOrderState::Destroy);
        // `units_manager.cpp:3932-3946`: своя 8-кадровая анимация смерти
        // (`image_base = 168 + (angle / 2) * 8`, `image_index_max = base + 7`).
        assert_eq!(unit.image_base, 168);
        assert_eq!(unit.image_index_max, 175);
        // Анимация идёт по кадру за два тика, затем юнит уничтожается.
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
        // scale до нуля → RestoreOrders возвращает Move/InProgress.
        assert_eq!(state.unit(id).unwrap().orders, UnitOrder::Move);
    }

    // -----------------------------------------------------------------------
    // Снаряды: попиксельная прямая (`AirPath::Execute`, airpath.cpp:175-295)
    // -----------------------------------------------------------------------

    /// Ракета в (10,10) летит в (13,14): dx=3, dy=4 — линия идёт **не** по
    /// 8-дирекционному маршруту (старый порт шёл по клеткам и вилял).
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

    /// Перпендикулярное расстояние точки до прямой `start → end` (пиксели).
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
        // Точка прицела — центр клетки (13,14).
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
        // Первая точка — ещё на месте пуска, дальше строго по линии.
        for point in &samples {
            let distance = distance_to_line(*point, start, end);
            assert!(
                distance <= 1.5,
                "точка {point:?} не на прямой {start:?}→{end:?}: отклонение {distance:.2} px"
            );
        }
        // Позиция действительно меняется от тика к тику (пиксельный шаг, не
        // «клетка за тик в центр»).
        let unique: std::collections::BTreeSet<_> = samples.iter().collect();
        assert!(unique.len() >= 3, "снаряд должен двигаться попиксельно");
        // И приходит в клетку цели (последняя живая точка — рядом с центром).
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

        // Один тик: снаряд проходит первую клетку и ставит за собой три частицы.
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

                // Взрыв ставится на следующем тике (`ProcessOrderExplode`/`INIT`
                // → `UnitsManager_StartExplosion`), поэтому добираем кадры.
                if impact_cell.is_some() {
                    break;
                }
            }
        }

        assert!(destroyed, "по `m_length == 0` снаряд уничтожается");
        // `Paths_FinishMove` наносит удар в клетке цели (`13,14`) — взрыв
        // `HITEXPLD`/`LNDEXPLD` появляется именно там.
        assert_eq!(
            impact_cell,
            Some(Point::new(13, 14)),
            "`parent->Attack(grid_x, grid_y)`: удар в клетке цели"
        );
        // И цель получила урон.
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
}
