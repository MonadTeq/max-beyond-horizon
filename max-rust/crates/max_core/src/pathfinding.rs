//! Поиск путей: наземный/воздушный/строителя, очередь запросов, PathFill.
//!
//! Оракул: `max-cpp-port/src/` — `paths.cpp` (`Paths_RequestPath`,
//! `Paths_GetAirPath`, `Paths_GetAngle`, `Paths_UpdateAngle`,
//! `Paths_CalculateStep`, `Paths_IsOccupied`, `Paths_IsAccessible`-семантика),
//! `groundpath.cpp` (`GroundPath::Execute`, `AppendLinearSteps`,
//! `GetPosition`, `GetMovementCost`), `airpath.cpp` (`AirPath`), `builderpath.cpp`,
//! `searcher.cpp`/`paths_manager.cpp` (`Searcher` — поиск по карте стоимости),
//! `pathrequest.cpp` (`PathRequest::Finish`, `AssignGroundPath`),
//! `accessmap.cpp` (`AccessMap::Init` — карта стоимостей),
//! `pathfill.cpp` (`PathFill`), `maxfloodfill.cpp`, DESIGN.md §1.4 п.2
//! (**синхронно, без потоков**) и риск 2 (результаты применяются в порядке
//! постановки запросов).
//!
//! # Синхронная модель
//!
//! C++ ставит `PathRequest` в `PathsManager` и обрабатывает их в рабочих
//! потоках. `max_core` не использует потоки: [`request_path`] кладёт запрос в
//! очередь [`crate::game::GameState::path_requests`], [`poll_results`] считает и
//! применяет **все** запросы в порядке очереди (`PushFront` для людей,
//! `PushBack` для компьютеров — как в `Paths_RequestPath`). Порядок применения
//! поэтому совпадает с порядком постановки, а не с порядком завершения
//! воркеров (DESIGN риск 2).
//!
//! # Поиск
//!
//! C++ `Searcher` — двунаправленный A* по `AccessMap` с инициализацией
//! прямолинейным маршрутом. Здесь портирован детерминированный A*-поиск по той
//! же карте стоимости: направления перебираются строго `0..8`
//! (`DIRECTION_OFFSETS`), очередь — по `(стоимость, порядковый номер вставки)`,
//! поэтому равнозначные пути разрешаются всегда одинаково. Диагональ стоит
//! `cost * 3 / 2`, как в `Searcher::ForwardSearch`. Воздушные пути — линейный
//! отрезок (`Paths_GetAirPath`) без обхода препятствий.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::access;
use crate::enums::unit_flags::{BUILDING, HOVERING, MOBILE_AIR_UNIT, STATIONARY};
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::{PathStep, Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use crate::unit::{AirPath, GroundPath, UnitId, UnitInfo, UnitListKind, UnitPath};
use crate::unit_order;
use crate::unit_values::ATTRIB_RANGE;

/// `PathRequest_CautionLevels[0]` — без осторожности (P5 не моделирует
/// damage-карты ИИ; предупреждения об опасности считаются нулевыми).
pub const CAUTION_LEVEL_NONE: i32 = 0;

/// `SHRT_MAX` — «нет ограничения» в `Paths_GetAirPath`.
const SHRT_MAX: i32 = 32767;

/// Запрос пути — C++ `class PathRequest` (`pathrequest.hpp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRequest {
    /// Юнит-клиент (`client`).
    pub unit: UnitId,
    /// Флаги `AccessModifier_*` (`flags`).
    pub flags: u32,
    /// Пункт назначения на момент постановки (`point`).
    pub destination: Point,
    /// Минимальная дистанция до цели (`minimum_distance`) — радиус атаки в
    /// квадрате для `MOVE_TO_ATTACK`.
    pub minimum_distance: i32,
    /// Запрос на посадку в транспорт (`board_transport`).
    pub board_transport: bool,
    /// Ограничение стоимости (`max_cost`; C++ по умолчанию `INT16_MAX`).
    pub max_cost: i32,
}

impl PathRequest {
    /// `PathRequest::PathRequest` (`pathrequest.cpp:34`).
    pub fn new(unit: UnitId, flags: u32, destination: Point) -> Self {
        Self {
            unit,
            flags,
            destination,
            minimum_distance: 0,
            board_transport: false,
            max_cost: i32::from(i16::MAX),
        }
    }
}

/// `Paths_RequestPath` (`paths.cpp:46`).
///
/// Возвращает `true`, если запрос поставлен в очередь. При невалидном
/// пункте назначения юнит получает `BlockedOnPathRequest(false)` и функция
/// возвращает `false` (как C++).
pub fn request_path(state: &mut GameState, id: UnitId, mode: u32) -> bool {
    let Some(unit) = state.unit(id).cloned() else {
        return false;
    };

    let mut data = state.order_data(id);
    let destination = if unit.orders == UnitOrder::MoveToAttack {
        match unit.enemy_unit.and_then(|enemy| state.unit(enemy).map(|enemy| enemy.grid)) {
            Some(enemy_grid) => {
                data.move_to = enemy_grid;
                state.set_order_data(id, data);
                enemy_grid
            }
            None => data.move_to,
        }
    } else {
        data.move_to
    };

    if destination.x < 0
        || destination.y < 0
        || destination.x >= MAP_WIDTH
        || destination.y >= MAP_HEIGHT
    {
        if let Some(mut unit) = state.unit(id).cloned() {
            unit_order::blocked_on_path_request(&mut unit, false, state.disable_fire);
            state.write_unit(id, unit);
        }
        return false;
    }

    let mut request = PathRequest::new(id, mode, destination);
    request.board_transport = unit.orders == UnitOrder::MoveToUnit;

    if mode & access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0 {
        request.minimum_distance = 3;
    }

    if unit.orders == UnitOrder::MoveToAttack {
        let range = unit.base_values.get_attribute(ATTRIB_RANGE);
        request.minimum_distance = range * range;
    }

    if unit.state == UnitOrderState::NewOrder {
        state.path_requests.retain(|queued| queued.unit != id);
    }

    // Одинаковые запросы одного юнита не дублируются (иначе `PursueEnemy`
    // ставил бы по запросу на тик, пока путь не готов).
    if state
        .path_requests
        .iter()
        .any(|queued| queued.unit == id && queued.destination == destination)
    {
        return true;
    }

    if state.teams[unit.team.0 as usize].team_type == crate::team::TeamKind::Computer {
        state.path_requests.push_back(request);
    } else {
        state.path_requests.push_front(request);
    }

    true
}

/// `PathsManager::DispatchJobs` — в синхронной модели работа выполняется в
/// [`poll_results`]; точка вызова сохранена для порядка тика (DESIGN §5.1).
pub fn dispatch_jobs(_state: &mut GameState) {}

/// `PathsManager::PollResults` — считает и применяет все запросы в порядке
/// постановки.
pub fn poll_results(state: &mut GameState) {
    let mut requests: Vec<PathRequest> = Vec::new();
    while let Some(request) = state.path_requests.pop_front() {
        requests.push(request);
    }

    for request in requests {
        apply_request(state, &request);
    }
}

/// Считает путь для запроса и применяет его через `PathRequest::Finish`.
fn apply_request(state: &mut GameState, request: &PathRequest) {
    let Some(unit) = state.unit(request.unit).cloned() else {
        return;
    };

    if unit.hits == 0 {
        return;
    }

    let path = find_ground_path_inner(
        state,
        &unit,
        request.destination,
        request.flags,
        request.minimum_distance,
        request.board_transport,
    );
    finish_path(state, request, path);
}

/// `PathRequest::Finish` (`pathrequest.cpp:145`).
fn finish_path(state: &mut GameState, request: &PathRequest, path: Option<GroundPath>) {
    let Some(unit) = state.unit(request.unit).cloned() else {
        return;
    };

    let order = unit.orders;
    let order_state_accepts = matches!(
        unit.state,
        UnitOrderState::NewOrder | UnitOrderState::MoveGettingPath
    ) || (order == UnitOrder::MoveToAttack && unit.state == UnitOrderState::ExecutingOrder);

    if unit.hits == 0
        || !matches!(
            order,
            UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::Build | UnitOrder::MoveToAttack
        )
        || !order_state_accepts
    {
        return;
    }

    let previous_state = unit.state;
    let mut unit = unit;
    let path_steps = path.as_ref().map(|path| path.steps.clone());
    unit.path = path.map(UnitPath::Ground);

    if order == UnitOrder::MoveToAttack {
        if let Some(steps) = path_steps {
            assign_ground_path(&mut unit, &steps, request.destination);
        }
    }

    if unit.path.is_none() {
        if order == UnitOrder::MoveToAttack {
            let range = unit.base_values.get_attribute(ATTRIB_RANGE);
            if access::squared_distance(unit.grid, request.destination) <= range * range {
                unit_order::set_new_order_int(
                    &mut unit,
                    UnitOrder::MoveToAttack,
                    UnitOrderState::ExecutingOrder,
                );
                state.write_unit(request.unit, unit);
                return;
            }
        }

        unit_order::blocked_on_path_request(&mut unit, false, state.disable_fire);

        if previous_state == UnitOrderState::NewOrder {
            access::clear_unit_group(state, request.unit);
        }
    } else if order == UnitOrder::Build {
        // `UnitsManager_StartBuild` — P3-логика площадки; состояние не меняется.
        unit.state = UnitOrderState::InProgress;
    } else if previous_state == UnitOrderState::MoveGettingPath {
        if order == UnitOrder::MoveToAttack {
            unit.state = UnitOrderState::ExecutingOrder;
        } else {
            unit_order::restore_orders(&mut unit);
        }
    } else {
        unit.state = UnitOrderState::InProgress;
    }

    state.write_unit(request.unit, unit);

    if state.group_members(request.unit).is_some() {
        access::process_new_group_order(state, request.unit);
    }
}

/// `PathRequest::AssignGroundPath` (`pathrequest.cpp:115`): путь обрезается
/// там, откуда цель уже в радиусе атаки.
fn assign_ground_path(unit: &mut UnitInfo, steps: &[PathStep], destination: Point) {
    let range = unit.base_values.get_attribute(ATTRIB_RANGE);
    let range = range * range;
    let mut position = unit.grid;
    let mut used = 0usize;

    while used < steps.len() && access::squared_distance(position, destination) > range {
        position.x += i16::from(steps[used].x);
        position.y += i16::from(steps[used].y);
        used += 1;
    }

    if used == 0 {
        unit.path = None;
        return;
    }

    let mut path = GroundPath {
        end_x: position.x,
        end_y: position.y,
        step_index: 0,
        steps: steps[..used].to_vec(),
    };

    if unit.flags & MOBILE_AIR_UNIT != 0 {
        // C++: `SetEndXY(position)` + `Paths_GetAirPath(unit)`.
        path.end_x = position.x;
        path.end_y = position.y;
    }

    unit.path = Some(UnitPath::Ground(path));
}

// ---------------------------------------------------------------------------
// Карта стоимости (AccessMap)
// ---------------------------------------------------------------------------

/// Строит карту стоимостей входа для юнита — порт `AccessMap::Init`
/// (`accessmap.cpp:372`). `0` — непроходимо.
pub fn build_access_costs(state: &GameState, unit: &UnitInfo, flags: u32) -> [u8; MAP_TILES] {
    let mut costs = [0u8; MAP_TILES];
    let is_air = unit.flags & MOBILE_AIR_UNIT != 0;

    if is_air {
        costs.fill(4);
        process_mobile_air_units(state, unit, flags, &mut costs);
        return costs;
    }

    let land_type = access::unit_type_land_type(unit.unit_type);
    let water_value = if land_type & access::SURFACE_TYPE_LAND != 0
        && unit.unit_type != UnitType::Surveyor
    {
        8
    } else {
        4
    };

    for index in 0..MAP_TILES {
        let x = (index % MAP_WIDTH as usize) as i16;
        let y = (index / MAP_WIDTH as usize) as i16;
        let surface = state.map.surface_at(x, y).bits();
        costs[index] = if surface == access::SURFACE_TYPE_LAND {
            if land_type & access::SURFACE_TYPE_LAND != 0 {
                4
            } else {
                0
            }
        } else if surface == access::SURFACE_TYPE_COAST {
            if land_type & access::SURFACE_TYPE_COAST != 0 {
                4
            } else {
                0
            }
        } else if surface == access::SURFACE_TYPE_WATER {
            if land_type & access::SURFACE_TYPE_WATER != 0 {
                water_value
            } else {
                0
            }
        } else {
            0
        };
    }

    process_ground_cover(state, unit, land_type, &mut costs);
    process_mobile_land_sea_units(state, unit, flags, &mut costs);
    process_mobile_air_units(state, unit, flags, &mut costs);
    process_stationary_units(state, unit, &mut costs);

    costs
}

fn tile_index(grid: Point) -> Option<usize> {
    if grid.x < 0 || grid.y < 0 || grid.x >= MAP_WIDTH || grid.y >= MAP_HEIGHT {
        None
    } else {
        Some(grid.y as usize * MAP_WIDTH as usize + grid.x as usize)
    }
}

fn block_cell(costs: &mut [u8; MAP_TILES], grid: Point, value: u8) {
    if let Some(index) = tile_index(grid) {
        costs[index] = value;
    }
}

fn block_building(costs: &mut [u8; MAP_TILES], grid: Point, value: u8) {
    block_cell(costs, grid, value);
    block_cell(costs, Point::new(grid.x + 1, grid.y), value);
    block_cell(costs, Point::new(grid.x, grid.y + 1), value);
    block_cell(costs, Point::new(grid.x + 1, grid.y + 1), value);
}

fn is_visible_or_detected(unit: &UnitInfo, team: crate::team::TeamId) -> bool {
    unit.is_visible_to_team(team) || unit.is_detected_by_team(team)
}

/// `AccessMap::ProcessGroundCover` (`accessmap.cpp:196`).
fn process_ground_cover(
    state: &GameState,
    unit: &UnitInfo,
    land_type: u8,
    costs: &mut [u8; MAP_TILES],
) {
    let team = unit.team;
    let process_paved = land_type & access::SURFACE_TYPE_LAND != 0
        && unit.laying_state != 2
        && unit.laying_state != 1;

    for id in state.unit_list(UnitListKind::GroundCover) {
        let Some(cover) = state.unit(*id) else { continue };
        if !is_visible_or_detected(cover, team) {
            continue;
        }

        match cover.unit_type {
            UnitType::Bridge => {
                if land_type & (access::SURFACE_TYPE_LAND | access::SURFACE_TYPE_WATER) != 0 {
                    block_cell(costs, cover.grid, 4);
                }
            }
            UnitType::WaterPlatform => {
                if land_type & access::SURFACE_TYPE_LAND != 0 {
                    block_cell(costs, cover.grid, 4);
                } else {
                    block_cell(costs, cover.grid, 0);
                }
            }
            UnitType::Road | UnitType::SmallSlab | UnitType::LargeSlab => {
                if process_paved {
                    block_cell(costs, cover.grid, 2);
                    if cover.flags & BUILDING != 0 {
                        block_building(costs, cover.grid, 2);
                    }
                }
            }
            UnitType::LargeTape | UnitType::SmallTape => {
                block_cell(costs, cover.grid, 0);
                if cover.flags & BUILDING != 0 {
                    block_building(costs, cover.grid, 0);
                }
            }
            UnitType::LandMine | UnitType::SeaMine => {
                if cover.team != team && cover.is_detected_by_team(team) {
                    block_cell(costs, cover.grid, 0);
                }
            }
            _ => {}
        }
    }
}

/// `AccessMap::ProcessMobileUnits` для наземных/морских.
fn process_mobile_land_sea_units(
    state: &GameState,
    unit: &UnitInfo,
    flags: u32,
    costs: &mut [u8; MAP_TILES],
) {
    let team = unit.team;

    for id in state.unit_list(UnitListKind::MobileLandSea) {
        let Some(other) = state.unit(*id) else { continue };
        if other.orders != UnitOrder::Idle && other.is_visible_to_team(team) {
            if flags & access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0
                || (flags & access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0
                    && other.team != team)
            {
                block_cell(costs, other.grid, 0);

                if other.path.is_some()
                    && other.state != UnitOrderState::ExecutingOrder
                    && other.id != unit.id
                {
                    let position = ground_path_position(state, other);
                    block_cell(costs, position, 0);
                }
            }
        }
    }
}

/// `AccessMap::ProcessMobileUnits` для воздуха (и блокировка приземлившихся).
fn process_mobile_air_units(
    state: &GameState,
    unit: &UnitInfo,
    flags: u32,
    costs: &mut [u8; MAP_TILES],
) {
    let team = unit.team;
    let is_air_pathfinder = unit.flags & MOBILE_AIR_UNIT != 0;

    for id in state.unit_list(UnitListKind::MobileAir) {
        let Some(other) = state.unit(*id) else { continue };

        if other.orders != UnitOrder::Idle && other.is_visible_to_team(team) {
            if flags & access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0
                || (flags & access::ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS != 0
                    && other.team != team)
            {
                if other.flags & MOBILE_AIR_UNIT != 0
                    && !is_air_pathfinder
                    && other.flags & HOVERING != 0
                {
                    continue;
                }

                block_cell(costs, other.grid, 0);

                if other.path.is_some()
                    && other.state != UnitOrderState::ExecutingOrder
                    && other.id != unit.id
                {
                    if let Some(UnitPath::Air(path)) = other.path.as_ref() {
                        block_cell(costs, air_path_position(other, path), 0);
                    }
                }
            }
        }

        if !is_air_pathfinder
            && other.flags & MOBILE_AIR_UNIT != 0
            && other.flags & HOVERING == 0
            && other.is_visible_to_team(team)
            && (other.orders != UnitOrder::Idle || other.flags & STATIONARY != 0)
        {
            block_cell(costs, other.grid, 0);
        }
    }
}

/// `AccessMap::ProcessStationaryUnits` (`accessmap.cpp:62`).
fn process_stationary_units(state: &GameState, unit: &UnitInfo, costs: &mut [u8; MAP_TILES]) {
    let team = unit.team;

    for id in state.unit_list(UnitListKind::Stationary) {
        let Some(other) = state.unit(*id) else { continue };
        if other.unit_type != UnitType::Connector && is_visible_or_detected(other, team) {
            block_cell(costs, other.grid, 0);
            if other.flags & BUILDING != 0 {
                block_building(costs, other.grid, 0);
            }
        }
    }
}

/// `GroundPath::GetPosition` (`groundpath.cpp:94`) — прогноз позиции идущего
/// юнита на текущий ход (для блокировки «будущей» клетки).
pub fn ground_path_position(state: &GameState, unit: &UnitInfo) -> Point {
    let mut position = unit.grid;
    let speed = i32::from(unit.speed) * 4;

    let Some(UnitPath::Ground(path)) = unit.path.as_ref() else {
        return position;
    };

    if unit.orders == UnitOrder::Build
        || unit.state != UnitOrderState::ExecutingOrder
        || speed <= 0
    {
        return position;
    }

    let mut move_fraction = i32::from(unit.move_fraction);

    for step in path.steps.iter().skip(path.step_index as usize) {
        if step.x == 0 && step.y == 0 {
            continue;
        }

        let point = Point::new(position.x + i16::from(step.x), position.y + i16::from(step.y));
        if point.x < 0 || point.y < 0 || point.x >= MAP_WIDTH || point.y >= MAP_HEIGHT {
            continue;
        }

        let mut step_cost =
            access::is_accessible_unit(state, unit, point, access::ACCESS_MODIFIER_NONE) as i32;

        if step.x != 0 && step.y != 0 {
            step_cost = (step_cost * 3) / 2;
        }

        if move_fraction > step_cost {
            move_fraction = step_cost;
        }

        step_cost -= move_fraction;
        move_fraction = 0;

        if step_cost <= speed {
            position = point;
        } else {
            break;
        }
    }

    position
}

// ---------------------------------------------------------------------------
// Поиск наземного пути
// ---------------------------------------------------------------------------

/// Ищет путь `unit.grid → destination` по карте стоимости.
///
/// `minimum_distance` — квадрат радиуса: поиск останавливается на любой клетке,
/// откуда цель уже достижима (C++ `BuildAccessMap` + `IsProcessed`).
pub fn find_ground_path(
    state: &GameState,
    unit: &UnitInfo,
    destination: Point,
    flags: u32,
    minimum_distance: i32,
) -> Option<GroundPath> {
    find_ground_path_inner(
        state,
        unit,
        destination,
        flags,
        minimum_distance,
        false,
    )
}

/// То же с флагом посадки в транспорт (`PathRequest::board_transport`): клетка
/// получателя помечается проходимой, как `PathsManager::BuildAccessMap`.
fn find_ground_path_inner(
    state: &GameState,
    unit: &UnitInfo,
    destination: Point,
    flags: u32,
    minimum_distance: i32,
    board_transport: bool,
) -> Option<GroundPath> {
    if destination.x < 0
        || destination.y < 0
        || destination.x >= MAP_WIDTH
        || destination.y >= MAP_HEIGHT
    {
        return None;
    }

    let start = unit.grid;
    if start == destination {
        return None;
    }

    let start_index = tile_index(start)?;
    let destination_index = tile_index(destination)?;
    let mut costs = build_access_costs(state, unit, flags);

    if board_transport {
        if let Some(receiver) = access::get_receiver_unit(state, unit, destination) {
            if let Some(receiver_unit) = state.unit(receiver) {
                if receiver_unit.flags & BUILDING != 0 {
                    block_building(&mut costs, receiver_unit.grid, 2);
                } else {
                    block_cell(&mut costs, receiver_unit.grid, 2);
                }
            }
        }
    }

    // Тривиальный случай: соседняя клетка (`paths_manager.cpp:415`).
    if access::squared_distance(start, destination) <= 2 {
        let receiver_reachable = board_transport
            && access::get_receiver_unit(state, unit, destination).is_some();
        let viable = receiver_reachable
            || (if flags & access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS != 0 {
                let mut costs = costs;
                costs[start_index] = 2;
                costs[destination_index] != 0
            } else {
                access::is_accessible(state, unit.unit_type, unit.team, destination, flags) != 0
            });

        return viable.then(|| GroundPath {
            end_x: destination.x,
            end_y: destination.y,
            step_index: 0,
            steps: vec![PathStep::new(
                (destination.x - start.x) as i8,
                (destination.y - start.y) as i8,
            )],
        });
    }

    let mut cost_map = [u32::MAX; MAP_TILES];
    let mut direction_map = [i8::MIN; MAP_TILES];
    let mut explored = [false; MAP_TILES];
    let mut heap: BinaryHeap<Reverse<(u32, u32, usize)>> = BinaryHeap::new();
    let mut sequence = 0u32;

    cost_map[start_index] = 0;
    heap.push(Reverse((0, sequence, start_index)));

    let in_range = |index: usize| -> bool {
        if minimum_distance <= 0 {
            return index == destination_index;
        }
        let x = (index % MAP_WIDTH as usize) as i16;
        let y = (index / MAP_WIDTH as usize) as i16;
        access::squared_distance(Point::new(x, y), destination) <= minimum_distance
    };

    let mut reached = None;

    while let Some(Reverse((cost, _, index))) = heap.pop() {
        if explored[index] {
            continue;
        }
        explored[index] = true;

        if index != start_index && in_range(index) {
            reached = Some(index);
            break;
        }

        let x = (index % MAP_WIDTH as usize) as i16;
        let y = (index / MAP_WIDTH as usize) as i16;

        for direction in 0..8usize {
            let offset = DIRECTION_OFFSETS[direction];
            let next = Point::new(x + offset.x, y + offset.y);
            let Some(next_index) = tile_index(next) else { continue };
            if explored[next_index] {
                continue;
            }

            let step_cost = costs[next_index];

            if step_cost == 0 {
                continue;
            }

            let mut step_cost = u32::from(step_cost);
            if direction & 1 != 0 {
                step_cost = (step_cost * 3) / 2;
            }

            let next_cost = cost.saturating_add(step_cost);
            if next_cost < cost_map[next_index] {
                cost_map[next_index] = next_cost;
                direction_map[next_index] = direction as i8;
                sequence = sequence.wrapping_add(1);
                heap.push(Reverse((next_cost, sequence, next_index)));
            }
        }
    }

    let reached = reached?;

    let mut reversed = Vec::new();
    let mut position = reached;
    while position != start_index {
        let direction = direction_map[position];
        if direction < 0 {
            return None;
        }
        let offset = DIRECTION_OFFSETS[(direction as usize) & 7];
        reversed.push(PathStep::new(offset.x as i8, offset.y as i8));
        position = tile_index(Point::new(
            (position % MAP_WIDTH as usize) as i16 - offset.x,
            (position / MAP_WIDTH as usize) as i16 - offset.y,
        ))?;
    }
    reversed.reverse();

    if reversed.is_empty() {
        return None;
    }

    Some(GroundPath {
        end_x: destination.x,
        end_y: destination.y,
        step_index: 0,
        steps: reversed,
    })
}

/// `GroundPath::AppendLinearSteps` (`groundpath.cpp:582`) — прямолинейный
/// маршрут (P4-совместимый helper; в симуляции используется как fallback).
pub fn append_linear_steps(from: Point, to: Point) -> Vec<PathStep> {
    let mut steps = Vec::new();
    let mut x = i32::from(to.x) - i32::from(from.x);
    let mut y = i32::from(to.y) - i32::from(from.y);

    while x != 0 || y != 0 {
        let step_x = x.signum() as i8;
        let step_y = y.signum() as i8;
        steps.push(PathStep::new(step_x, step_y));
        x -= i32::from(step_x);
        y -= i32::from(step_y);
    }

    steps
}

// ---------------------------------------------------------------------------
// Воздушные пути и ракеты
// ---------------------------------------------------------------------------

/// Целочисленный квадратный корень (`isqrt`), без `f64` (DESIGN §1.4 п.2).
pub fn isqrt(value: u32) -> u32 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

/// Целочисленный квадратный корень для `u64`.
fn isqrt_u64(value: u64) -> u64 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

/// `sqrt(dx² + dy²) * 4 + 0.5` в целых числах (`paths.cpp`).
pub fn scaled_distance4(distance_x: i32, distance_y: i32) -> i32 {
    let squared = i64::from(distance_x) * i64::from(distance_x)
        + i64::from(distance_y) * i64::from(distance_y);
    let squared = squared as u64;
    let root = isqrt_u64(squared * 16);
    // floor(4*sqrt(d) + 0.5): 4*sqrt(d) >= root + 0.5
    //   ⇔ 64*d >= 4*root² + 4*root + 1.
    let round_up = u128::from(squared) * 64
        >= 4 * u128::from(root) * u128::from(root) + 4 * u128::from(root) + 1;
    (root + u64::from(round_up)) as i32
}

/// `Paths_GetAirPath` (`paths.cpp:97`) — линейный воздушный отрезок.
///
/// Мутирует `speed`/`move_fraction`/`max_velocity` юнита, как C++.
pub fn paths_get_air_path(
    state: &GameState,
    unit: &mut UnitInfo,
    target: Point,
) -> Option<AirPath> {
    let mut end_x = target.x;
    let mut end_y = target.y;

    if unit.path.is_some() {
        let (x, y) = unit.path.as_ref().map(UnitPath::end).unwrap_or((end_x, end_y));
        end_x = x;
        end_y = y;
        unit.path = None;
    }

    let distance_x = i32::from(end_x) - i32::from(unit.grid.x);
    let distance_y = i32::from(end_y) - i32::from(unit.grid.y);
    // `AirPath::AirPath`: воздушный юнит получает угол на цель, остальные —
    // текущий угол (ракета летит туда, куда её развернул `PrepareFire`).
    let angle = if unit.flags & MOBILE_AIR_UNIT != 0 {
        crate::combat::get_target_angle(distance_x, distance_y)
    } else {
        i32::from(unit.angle)
    };
    let mut distance = scaled_distance4(distance_x, distance_y);

    if distance == 0 {
        return None;
    }

    distance -= i32::from(unit.move_fraction);
    if distance < 2 {
        distance = 2;
    }

    let max_distance;
    let mut steps_distance;
    let mut distance_x = distance_x;
    let mut distance_y = distance_y;

    if state.real_time {
        max_distance = distance;
    } else {
        let mut value = i32::from(unit.speed);
        if unit.group_speed != 0 {
            value = i32::from(unit.group_speed) - 1;
        }
        max_distance = if value != 0 {
            value * 4
        } else {
            SHRT_MAX
        };
    }

    if max_distance < distance {
        let target_grid_x = (distance_x * max_distance) / distance + i32::from(unit.grid.x);
        let target_grid_y = (distance_y * max_distance) / distance + i32::from(unit.grid.y);

        distance_x = target_grid_x - i32::from(unit.grid.x);
        distance_y = target_grid_y - i32::from(unit.grid.y);
        distance = scaled_distance4(distance_x, distance_y);

        if distance > max_distance {
            distance = max_distance;
        }

        steps_distance = max_distance / 4;
        unit.move_fraction = (max_distance - distance).clamp(0, 3) as u8;

        if steps_distance == 0 || distance == 0 {
            unit.speed = 0;
            if unit.group_speed > 1 {
                unit.group_speed = 1;
            }

            distance_x = i32::from(end_x) - i32::from(unit.grid.x);
            distance_y = i32::from(end_y) - i32::from(unit.grid.y);
            distance = scaled_distance4(distance_x, distance_y);
            steps_distance = (distance + 3) / 4;
        }
    } else {
        steps_distance = (distance + 3) / 4;
        if max_distance != SHRT_MAX {
            unit.move_fraction = (distance % 4) as u8;
        }
    }

    unit.max_velocity = 4;
    let euclidean_distance = steps_distance * i32::from(unit.max_velocity);

    if euclidean_distance <= 0 {
        return None;
    }

    Some(AirPath {
        length: euclidean_distance as i16,
        angle: angle as i8,
        start_x: unit.x as i16,
        start_y: unit.y as i16,
        end_x,
        end_y,
        step_x: 0,
        step_y: 0,
        delta_x: (distance_x << 22) / euclidean_distance,
        delta_y: (distance_y << 22) / euclidean_distance,
    })
}

/// `UnitsManager_GetMissilePath` (`units_manager.cpp:4759`).
pub fn get_missile_path(unit: &UnitInfo, fire_on: Point) -> Option<AirPath> {
    let distance_x = i32::from(fire_on.x) - i32::from(unit.grid.x);
    let distance_y = i32::from(fire_on.y) - i32::from(unit.grid.y);
    let distance = scaled_distance4(distance_x, distance_y);

    if distance <= 0 {
        return None;
    }

    let length = distance / 4;
    if length <= 0 {
        return None;
    }

    Some(AirPath {
        length: length as i16,
        angle: unit.angle as i8,
        start_x: unit.x as i16,
        start_y: unit.y as i16,
        end_x: fire_on.x,
        end_y: fire_on.y,
        step_x: 0,
        step_y: 0,
        delta_x: (distance_x << 22) / length,
        delta_y: (distance_y << 22) / length,
    })
}

// ---------------------------------------------------------------------------
// PathFill / MAXFloodFill
// ---------------------------------------------------------------------------

/// `PathFill` (`pathfill.cpp`) — заливка связной области по карте стоимости.
///
/// `mark` — бит `0x20` «клетка обработана заливкой»; `0x1F` — стоимость,
/// `0x80` — «непроходимо/не обрабатывать». Возвращает число помеченных клеток.
pub fn path_fill(costs: &[u8; MAP_TILES], start: Point) -> ([u8; MAP_TILES], u32) {
    let mut map = *costs;
    let Some(start_index) = tile_index(start) else {
        return (map, 0);
    };

    if map[start_index] & 0x1F == 0 || map[start_index] & 0x20 != 0 {
        return (map, 0);
    }

    let mut stack: VecDeque<usize> = VecDeque::new();
    stack.push_back(start_index);
    let mut count = 0;

    while let Some(index) = stack.pop_front() {
        if map[index] & 0x1F == 0 || map[index] & 0x20 != 0 {
            continue;
        }
        map[index] |= 0x20;
        count += 1;

        let x = (index % MAP_WIDTH as usize) as i16;
        let y = (index / MAP_WIDTH as usize) as i16;

        // Порядок обхода: 4 ортогональных направления `0,2,4,6`, как в
        // `MAXFloodFill` (N, E, S, W).
        for direction in [0usize, 2, 4, 6] {
            let offset = DIRECTION_OFFSETS[direction];
            let neighbor = Point::new(x + offset.x, y + offset.y);
            if let Some(neighbor_index) = tile_index(neighbor) {
                if map[neighbor_index] & 0x1F != 0 && map[neighbor_index] & 0x20 == 0 {
                    stack.push_back(neighbor_index);
                }
            }
        }
    }

    (map, count)
}

/// `AccessMap::IsProcessed` (`accessmap.cpp:52`).
pub fn is_processed(costs: &[u8; MAP_TILES], grid: Point) -> bool {
    match tile_index(grid) {
        Some(index) => costs[index] != 0 && costs[index] & 0x80 == 0,
        None => false,
    }
}

/// `Paths_CalculateStep` (см. [`crate::unit_order::paths_calculate_step`]) —
/// повторно экспортируется для вызывающих модулей пути.
pub use crate::unit_order::paths_calculate_step;

/// `Paths_IsOccupied` — повторный экспорт из [`crate::access`] (занятость
/// клетки скрытным юнитом; DESIGN §2 относит это к access).
pub use crate::access::paths_is_occupied;

// ---------------------------------------------------------------------------
// BuilderPath (P5: минимальный порт)
// ---------------------------------------------------------------------------

/// `BuilderPath::Execute` (`builderpath.cpp`) — путь строителя: движение к
/// цели по одной оси (направление `(1,1)` по умолчанию). Возвращает шаг к
/// цели, если он проходим.
pub fn builder_path_step(state: &GameState, unit: &UnitInfo, target: Point) -> Option<PathStep> {
    let dx = i32::from(target.x) - i32::from(unit.grid.x);
    let dy = i32::from(target.y) - i32::from(unit.grid.y);

    if dx == 0 && dy == 0 {
        return None;
    }

    let step = PathStep::new(dx.signum() as i8, dy.signum() as i8);
    let next = Point::new(
        unit.grid.x + i16::from(step.x),
        unit.grid.y + i16::from(step.y),
    );

    (access::is_accessible_unit(state, unit, next, access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS) != 0)
        .then_some(step)
}

/// Стоимость пути `GroundPath::GetMovementCost` (`groundpath.cpp:178`).
pub fn ground_path_movement_cost(state: &GameState, unit: &UnitInfo) -> i32 {
    let Some(UnitPath::Ground(path)) = unit.path.as_ref() else {
        return 0;
    };

    let mut x = unit.grid.x;
    let mut y = unit.grid.y;
    let mut result = 0;

    for step in path.steps.iter().skip(path.step_index as usize) {
        if step.x != 0 || step.y != 0 {
            x += i16::from(step.x);
            y += i16::from(step.y);
            let mut step_cost =
                access::is_accessible_unit(state, unit, Point::new(x, y), access::ACCESS_MODIFIER_NONE)
                    as i32;
            if step.x != 0 && step.y != 0 {
                step_cost = (step_cost * 3) / 2;
            }
            result += step_cost;
        }
    }

    result
}

/// `GroundPath::IsEndStep` (`groundpath.cpp:561`).
pub fn ground_path_is_end_step(unit: &UnitInfo) -> bool {
    match unit.path.as_ref() {
        Some(UnitPath::Ground(path)) => {
            path.step_index as usize + 1 == path.steps.len()
        }
        _ => false,
    }
}

/// `AirPath::GetMovementCost` (`airpath.cpp:200`).
pub fn air_path_movement_cost(unit: &UnitInfo) -> i32 {
    match unit.path.as_ref() {
        Some(UnitPath::Air(path)) if unit.max_velocity != 0 => {
            (i32::from(path.length) / i32::from(unit.max_velocity)) * 4
        }
        _ => 0,
    }
}

/// `UnitsManager_InitUnitPath`-часть для ракеты: путь к `fire_on`.
pub fn missile_path(unit: &UnitInfo, fire_on: Point) -> Option<UnitPath> {
    get_missile_path(unit, fire_on).map(UnitPath::Air)
}

/// Список юнитов, которым сейчас нужен путь (для отладки/тестов).
pub fn pending_request_count(state: &GameState) -> usize {
    state.path_requests.len()
}

/// Очередь запросов без изменений (для тестов/дампа).
pub fn request_for(state: &GameState, unit: UnitId) -> Option<&PathRequest> {
    state.path_requests.iter().find(|request| request.unit == unit)
}

/// `AirPath::GetPosition` (`airpath.cpp:104`) — прогноз клетки воздушного
/// юнита (для карты стоимости).
pub fn air_path_position(unit: &UnitInfo, path: &AirPath) -> Point {
    if unit.flags & MOBILE_AIR_UNIT == 0 {
        return unit.grid;
    }

    let mut pixel_x = i32::from(path.length) * path.delta_x + path.step_x;
    pixel_x >>= 16;
    pixel_x += i32::from(path.start_x);
    pixel_x >>= 6;

    let mut pixel_y = i32::from(path.length) * path.delta_y + path.step_y;
    pixel_y >>= 16;
    pixel_y += i32::from(path.start_y);
    pixel_y >>= 6;

    Point::new(pixel_x as i16, pixel_y as i16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
    use crate::enums::{Surface, TeamClan};
    use crate::team::{TeamId, TeamKind};

    fn state() -> GameState {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Player;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        state.team_units[1] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        state
    }

    fn tank_at(state: &mut GameState, grid: Point) -> UnitId {
        state
            .spawn_unit(
                UnitType::Tank,
                TeamId::RED,
                grid,
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap()
    }

    fn cumulative(steps: &[PathStep], start: Point) -> Vec<Point> {
        let mut points = vec![start];
        let mut point = start;
        for step in steps {
            point.x += i16::from(step.x);
            point.y += i16::from(step.y);
            points.push(point);
        }
        points
    }

    #[test]
    fn integer_distance_helpers_match_cpp_math() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(24), 4);
        assert_eq!(isqrt(25), 5);
        // sqrt(25)*4 = 20, sqrt(2)*4 ≈ 5.66 → 6.
        assert_eq!(scaled_distance4(3, 4), 20);
        assert_eq!(scaled_distance4(1, 1), 6);
        assert_eq!(scaled_distance4(0, 0), 0);
    }

    #[test]
    fn shortest_path_is_a_straight_line() {
        let mut state = state();
        let tank = tank_at(&mut state, Point::new(5, 5));
        let unit = state.unit(tank).unwrap().clone();

        let path = find_ground_path(&state, &unit, Point::new(5, 8), 0, 0).expect("path");
        assert_eq!((path.end_x, path.end_y), (5, 8));
        assert_eq!(path.steps.len(), 3);
        for step in &path.steps {
            assert_eq!((step.x, step.y), (0, 1));
        }

        // Старт == финиш — пути нет (C++ `CompleteRequest(nullptr)`).
        assert!(find_ground_path(&state, &unit, unit.grid, 0, 0).is_none());
    }

    #[test]
    fn path_goes_around_water_wall() {
        let mut state = state();
        // Вода стеной x = 7, y = 0..=10, проход только y = 11.
        for y in 0..=10 {
            state.map.surface.set(7, y, Surface::WATER);
        }

        let tank = tank_at(&mut state, Point::new(5, 5));
        let unit = state.unit(tank).unwrap().clone();
        let path = find_ground_path(&state, &unit, Point::new(9, 5), 0, 0).expect("path");

        // Путь не может быть короче манхэттенского расстояния и обязан
        // обходить стену (по y до 11 включительно и обратно).
        assert!(path.steps.len() >= 12);
        for point in cumulative(&path.steps, unit.grid) {
            assert_eq!(
                state.map.surface_at(point.x, point.y),
                Surface::LAND,
                "путь зашёл в воду: {point:?}"
            );
        }
        assert_eq!(cumulative(&path.steps, unit.grid).last().copied(), Some(Point::new(9, 5)));
    }

    #[test]
    fn search_is_deterministic() {
        let mut state = state();
        for y in 2..=12 {
            state.map.surface.set(8, y, Surface::WATER);
        }
        let tank = tank_at(&mut state, Point::new(4, 6));
        let unit = state.unit(tank).unwrap().clone();

        let first = find_ground_path(&state, &unit, Point::new(12, 6), 0, 0).expect("path");
        let second = find_ground_path(&state, &unit, Point::new(12, 6), 0, 0).expect("path");
        assert_eq!(first, second);
    }

    #[test]
    fn request_queue_is_applied_in_placement_order() {
        let mut state = state();
        let first = tank_at(&mut state, Point::new(5, 5));
        let second = tank_at(&mut state, Point::new(5, 9));
        for (unit, target) in [(first, Point::new(9, 5)), (second, Point::new(9, 9))] {
            state.set_order_data(
                unit,
                crate::game::OrderData {
                    move_to: target,
                    fire_on: Point::ZERO,
                },
            );
            state.set_order(unit, UnitOrder::Move, UnitOrderState::Init);
        }

        state.tick();
        // Оба запроса поставлены в одном тике. Мобильные наземные юниты
        // вставляются в список `PushFront` (`AddToDrawList`), поэтому второй
        // юнит обходится первым; `Paths_RequestPath` для игроков делает
        // `PushFront` — итоговый порядок очереди: первый, второй.
        assert_eq!(state.path_requests.len(), 2);
        assert_eq!(state.path_requests[0].unit, first);
        assert_eq!(state.path_requests[1].unit, second);

        state.tick();
        assert!(state.path_requests.is_empty());
        for unit in [first, second] {
            let unit = state.unit(unit).unwrap();
            assert!(unit.path.is_some());
            assert_eq!(unit.state, UnitOrderState::InProgress);
        }
    }

    #[test]
    fn board_transport_path_reaches_receiver_cell() {
        let mut state = state();
        let commando = state
            .spawn_unit(
                UnitType::Commando,
                TeamId::RED,
                Point::new(5, 6),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        let transport = state
            .spawn_unit(
                UnitType::SeaTransport,
                TeamId::RED,
                Point::new(9, 6),
                crate::enums::unit_flags::MOBILE_SEA_UNIT | crate::enums::unit_flags::MOBILE_LAND_UNIT,
            )
            .unwrap();
        state.unit_mut(transport).unwrap().orders = UnitOrder::Sentry;
        state.unit_mut(transport).unwrap().state = UnitOrderState::ExecutingOrder;
        state.unit_mut(transport).unwrap().storage = 0;

        state.set_order_data(
            commando,
            crate::game::OrderData {
                move_to: Point::new(9, 6),
                fire_on: Point::ZERO,
            },
        );
        state.set_order(commando, UnitOrder::MoveToUnit, UnitOrderState::Init);
        state.tick();
        state.tick();

        let unit = state.unit(commando).unwrap();
        let Some(UnitPath::Ground(path)) = unit.path.as_ref() else {
            panic!("путь к транспорту не найден: {:?}", unit.state);
        };
        assert_eq!((path.end_x, path.end_y), (9, 6));
    }

    #[test]
    fn move_to_attack_requests_path_to_enemy() {
        let mut state = state();
        let red = tank_at(&mut state, Point::new(5, 5));
        let green = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::GREEN,
                Point::new(12, 5),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        state.unit_mut(green).unwrap().orders = UnitOrder::Sentry;
        state.unit_mut(green).unwrap().state = UnitOrderState::ExecutingOrder;

        {
            let unit = state.unit_mut(red).unwrap();
            unit.enemy_unit = Some(green);
            unit.orders = UnitOrder::MoveToAttack;
            unit.state = UnitOrderState::Init;
        }
        // Путь движущегося к атаке юнита обрезается радиусом (`AssignGroundPath`).
        for _ in 0..12 {
            state.tick();
        }
        let unit = state.unit(red).unwrap();
        assert!(unit.grid.x > 5, "юнит должен приблизиться к цели");
        assert!(unit.grid.x <= 12);
        assert!(state.path_requests.len() <= 1, "очередь путей не растёт");
    }

    #[test]
    fn air_path_is_a_linear_segment() {
        let mut state = state();
        let fighter = state
            .spawn_unit(
                UnitType::Fighter,
                TeamId::RED,
                Point::new(5, 5),
                MOBILE_AIR_UNIT | SELECTABLE,
            )
            .unwrap();
        let mut unit = state.unit(fighter).unwrap().clone();
        let path = paths_get_air_path(&state, &mut unit, Point::new(9, 8)).expect("air path");
        assert_eq!((path.end_x, path.end_y), (9, 8));
        assert_eq!(path.start_x, unit.x as i16);
        assert_eq!(path.start_y, unit.y as i16);
        // Направление дельты — знак расстояния.
        assert!(path.delta_x > 0);
        assert!(path.delta_y > 0);
        assert!(path.length > 0);

        let missile = get_missile_path(&unit, Point::new(8, 5)).expect("missile path");
        assert_eq!((missile.end_x, missile.end_y), (8, 5));
    }

    #[test]
    fn path_fill_marks_only_connected_cells() {
        let mut costs = [4u8; MAP_TILES];
        // Стена по x = 6 (юниты не проходят).
        for y in 0..MAP_HEIGHT {
            let index = y as usize * MAP_WIDTH as usize + 6;
            costs[index] = 0;
        }

        let (filled, count) = path_fill(&costs, Point::new(2, 2));
        assert!(count > 0);
        assert!(count < MAP_TILES as u32);
        let start_index = 2 * MAP_WIDTH as usize + 2;
        assert_ne!(filled[start_index] & 0x20, 0);
        // Клетка за стеной не залита.
        let beyond = 2 * MAP_WIDTH as usize + 8;
        assert_eq!(filled[beyond] & 0x20, 0);
        assert!(is_processed(&costs, Point::new(2, 2)));

        // Повторный прогон даёт тот же результат (детерминизм заливки).
        let (again, again_count) = path_fill(&costs, Point::new(2, 2));
        assert_eq!(count, again_count);
        assert_eq!(filled, again);
    }

    #[test]
    fn ground_and_air_path_costs_are_ported() {
        let mut state = state();
        let tank = tank_at(&mut state, Point::new(5, 5));
        let mut unit = state.unit(tank).unwrap().clone();
        unit.path = Some(UnitPath::Ground(GroundPath {
            end_x: 7,
            end_y: 5,
            step_index: 0,
            steps: vec![PathStep::new(1, 0), PathStep::new(1, 0)],
        }));
        assert_eq!(ground_path_movement_cost(&state, &unit), 8);
        assert!(ground_path_is_end_step(&unit) == false);

        let mut air = state
            .spawn_unit(
                UnitType::Fighter,
                TeamId::RED,
                Point::new(6, 6),
                MOBILE_AIR_UNIT,
            )
            .map(|id| state.unit(id).unwrap().clone())
            .unwrap();
        air.max_velocity = 4;
        air.path = Some(UnitPath::Air(AirPath {
            length: 12,
            ..AirPath::default()
        }));
        assert_eq!(air_path_movement_cost(&air), 12);
    }
}
