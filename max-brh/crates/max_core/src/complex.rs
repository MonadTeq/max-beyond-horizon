//! Комплексы зданий и коннекторы — порт `complex.cpp` + коннекторной части
//! `units_manager.cpp`/`unitinfo.cpp`.
//!
//! # Оракул
//!
//! * `units_manager.cpp:2051-2225` — `UnitsManager_UpdateConnectors`: по восьми
//!   направлениям ищет здания команды (`Access_GetTeamBuilding`,
//!   `access.cpp:2157`), выставляет биты `CONNECTOR_*` себе и соседу (у
//!   `CONNECTOR_UNIT`/`STANDALONE` — всегда «левый/верхний» вариант пары),
//!   затем `AttachToPrimaryComplex` и `Access_UpdateResourcesTotal`;
//! * `unitinfo.cpp:1978-2052` — `UnitInfo::AttachComplex`: рекурсивно
//!   перевешивает здание и всё связное подграфство на один `Complex`
//!   (`Complex::Grow`/`Shrink`);
//! * `unitinfo.cpp:2054-2131` — `UnitInfo::AttachToPrimaryComplex`: из
//!   соседних комплексов выбирается комплекс с **наименьшим id**, иначе
//!   создаётся новый (`TeamUnits::CreateComplex`);
//! * `unitinfo.cpp:2133-2203` — `UnitInfo::TestConnections`: обход графа с
//!   пометкой `CONNECTION_BEING_TESTED`, чтобы не зациклиться;
//! * `unitinfo.cpp:2205-2273` — `UnitInfo::GetFirstUntestedConnection`;
//! * `unitinfo.cpp:2275-2313` — `UnitInfo::DetachComplex`: здание выходит из
//!   комплекса, оставшийся граф распадается на связные группы, каждая получает
//!   свой `Complex` (`CreateComplex` + `AttachComplex`), запасы пересчитываются
//!   `Access_UpdateResourcesTotal` (`access.cpp:912`);
//! * `units_manager.cpp:1556-1709` — `UnitsManager_RemoveConnections`: снятие
//!   парных битов у соседей перед уничтожением/переносом здания;
//! * `complex.cpp:136-203` — `Complex::GetCargoInfo`/`TransferCargo`/`Transfer`:
//!   ёмкость и запас комплекса — **сумма** подключённых хранилищ, а списание
//!   идёт по складам членов в порядке `UnitsManager_StationaryUnits`.
//!
//! # Модель
//!
//! Комплекс — это связная (через коннекторы) группа зданий/`CONNECTOR_UNIT`/
//! `STANDALONE` одной команды. `UnitInfo::complex` хранит id комплекса, а
//! `UnitInfo::connectors` — биты `CONNECTOR_*` (они входят в снапшот/CRC).
//! Запасы и ёмкость комплекса вычисляются из зданий
//! ([`crate::alloc::get_cargo_info`]), а `Complex::{material,fuel,gold,power,
//! workers}` — кэш на текущий ход (`Access_UpdateResourcesTotal`).

use crate::alloc::update_resources_total;
use crate::enums::{
    unit_flags::{BUILDING, CONNECTOR_UNIT, STANDALONE},
    UnitOrder, CONNECTION_BEING_TESTED, CONNECTOR_EAST_BOTTOM, CONNECTOR_EAST_TOP,
    CONNECTOR_NORTH_LEFT, CONNECTOR_NORTH_RIGHT, CONNECTOR_SOUTH_LEFT, CONNECTOR_SOUTH_RIGHT,
    CONNECTOR_WEST_BOTTOM, CONNECTOR_WEST_TOP,
};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{ComplexId, TeamId};
use crate::unit::{UnitId, UnitInfo, UnitListKind};

/// Порядок обхода коннекторов в C++ (`UnitInfo::AttachComplex`,
/// `TestConnections`, `AttachToPrimaryComplex`).
pub const CONNECTOR_ORDER: [u16; 8] = [
    CONNECTOR_NORTH_LEFT,
    CONNECTOR_NORTH_RIGHT,
    CONNECTOR_EAST_TOP,
    CONNECTOR_EAST_BOTTOM,
    CONNECTOR_SOUTH_RIGHT,
    CONNECTOR_SOUTH_LEFT,
    CONNECTOR_WEST_BOTTOM,
    CONNECTOR_WEST_TOP,
];

/// Габарит здания в клетках: `(flags & BUILDING) ? 2 : 1`
/// (`units_manager.cpp:2056`).
#[inline]
pub fn unit_size(flags: u32) -> i16 {
    if flags & BUILDING != 0 {
        2
    } else {
        1
    }
}

/// `Access_GetTeamBuilding(team, grid_x, grid_y)` (`access.cpp:2157`): первое
/// здание команды в клетке, которое ещё не в `ORDER_IDLE` и является членом
/// комплекса (`UnitInfo::IsComplexMember`).
///
/// `ORDER_IDLE` отсекается намеренно: достроенное здание на площадке
/// (`ORDER_IDLE`/`BUILDING_READY`) в комплекс не входит, пока
/// `UnitsManager_BuildingReady` не вернёт ему приказ.
pub fn team_building_at(state: &GameState, team: TeamId, grid: Point) -> Option<UnitId> {
    state.units_at(grid).iter().copied().find(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.team == team && unit.orders != UnitOrder::Idle && unit.is_complex_member()
        })
    })
}

/// `UnitInfo::GetConnectedBuilding(connector)` (`unitinfo.cpp:1938-1976`).
///
/// Смещения — ровно C++-switch: для здания (`grid_size = 2`) юго-восточные
/// пары сдвинуты на 2 клетки.
pub fn connected_building(state: &GameState, unit: &UnitInfo, connector: u16) -> Option<UnitId> {
    let size = unit_size(unit.flags);
    let x = unit.grid.x;
    let y = unit.grid.y;

    let (grid_x, grid_y) = match connector {
        CONNECTOR_NORTH_LEFT => (x, y - 1),
        CONNECTOR_NORTH_RIGHT => (x + 1, y - 1),
        CONNECTOR_EAST_TOP => (x + size, y),
        CONNECTOR_EAST_BOTTOM => (x + size, y + 1),
        CONNECTOR_SOUTH_LEFT => (x, y + size),
        CONNECTOR_SOUTH_RIGHT => (x + 1, y + size),
        CONNECTOR_WEST_TOP => (x - 1, y),
        CONNECTOR_WEST_BOTTOM => (x - 1, y + 1),
        _ => return None,
    };

    team_building_at(state, unit.team, Point::new(grid_x, grid_y))
}

// ---------------------------------------------------------------------------
// Complex::Grow/Shrink через TeamUnits
// ---------------------------------------------------------------------------

/// `Complex::Grow` (`complex.cpp:205`).
fn complex_grow(state: &mut GameState, team: TeamId, complex: ComplexId) {
    if let Some(units) = state.team_units.get_mut(team.0 as usize) {
        if let Some(target) = units.get_complex_mut(complex) {
            target.grow();
        }
    }
}

/// `Complex::Shrink` (`complex.cpp:207`): при нуле зданий комплекс удаляется
/// (`TeamUnits::RemoveComplex`).
fn complex_shrink(state: &mut GameState, team: TeamId, complex: ComplexId) {
    let Some(units) = state.team_units.get_mut(team.0 as usize) else {
        return;
    };

    let empty = units
        .get_complex_mut(complex)
        .map(|target| target.shrink())
        .unwrap_or(false);

    if empty {
        units.remove_complex(complex);
    }
}

// ---------------------------------------------------------------------------
// AttachComplex / AttachToPrimaryComplex / TestConnections
// ---------------------------------------------------------------------------

/// `UnitInfo::AttachComplex(complex)` (`unitinfo.cpp:1978`).
///
/// Перевешивает здание на `complex` (с `Grow` нового и `Shrink` старого,
/// пустой старый комплекс удаляется) и рекурсивно — всех связных соседей.
/// Рекурсия останавливается на здании, уже привязанном к `complex`, поэтому
/// циклы в графе коннекторов безопасны.
pub fn attach_complex(state: &mut GameState, id: UnitId, complex: ComplexId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    if unit.complex == Some(complex) {
        return;
    }

    let team = unit.team;
    complex_grow(state, team, complex);

    if let Some(previous) = unit.complex {
        complex_shrink(state, team, previous);
    }

    if let Some(target) = state.unit_mut(id) {
        target.complex = Some(complex);
    }

    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    for connector in CONNECTOR_ORDER {
        if unit.connectors & connector == 0 {
            continue;
        }

        if let Some(building) = connected_building(state, &unit, connector) {
            attach_complex(state, building, complex);
        }
    }
}

/// `UnitInfo::AttachToPrimaryComplex()` (`unitinfo.cpp:2054`).
///
/// Из комплексов соседних зданий выбирается комплекс с наименьшим id
/// (`Complex::GetId`); если соседей нет — создаётся новый
/// (`TeamUnits::CreateComplex`). Затем [`attach_complex`] сливает всё связное
/// подграфство в выбранный комплекс.
pub fn attach_to_primary_complex(state: &mut GameState, id: UnitId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    if !unit.is_complex_member() {
        return;
    }

    let mut primary: Option<ComplexId> = None;

    for connector in CONNECTOR_ORDER {
        if unit.connectors & connector == 0 {
            continue;
        }

        let Some(building) = connected_building(state, &unit, connector) else {
            continue;
        };
        let Some(candidate) = state.unit(building).and_then(|building| building.complex) else {
            continue;
        };

        if primary.is_none_or(|current| candidate.0 < current.0) {
            primary = Some(candidate);
        }
    }

    let target = primary.unwrap_or_else(|| {
        state
            .team_units
            .get_mut(unit.team.0 as usize)
            .map(|units| units.create_complex())
            .unwrap_or(ComplexId(0))
    });

    if target.0 == 0 {
        return;
    }

    attach_complex(state, id, target);
}

/// `UnitInfo::TestConnections()` (`unitinfo.cpp:2133`): пометить
/// `CONNECTION_BEING_TESTED` всё связное подграфство, начиная с `id`.
pub fn test_connections(state: &mut GameState, id: UnitId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    if unit.connectors & CONNECTION_BEING_TESTED != 0 {
        return;
    }

    if let Some(target) = state.unit_mut(id) {
        target.connectors |= CONNECTION_BEING_TESTED;
    }

    for connector in CONNECTOR_ORDER {
        if unit.connectors & connector == 0 {
            continue;
        }

        if let Some(building) = connected_building(state, &unit, connector) {
            test_connections(state, building);
        }
    }
}

/// `UnitInfo::GetFirstUntestedConnection()` (`unitinfo.cpp:2205`).
pub fn first_untested_connection(state: &GameState, unit: &UnitInfo) -> Option<UnitId> {
    for connector in CONNECTOR_ORDER {
        if unit.connectors & connector == 0 {
            continue;
        }

        let Some(building) = connected_building(state, unit, connector) else {
            continue;
        };
        let Some(target) = state.unit(building) else {
            continue;
        };

        if target.connectors & CONNECTION_BEING_TESTED == 0 {
            return Some(building);
        }
    }

    None
}

/// `UnitInfo::DetachComplex()` (`unitinfo.cpp:2275`): здание выходит из
/// комплекса, а оставшиеся связные группы получают собственные комплексы
/// (`CreateComplex` + `AttachComplex`) и пересчитанные запасы.
pub fn detach_complex(state: &mut GameState, id: UnitId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };
    let Some(complex) = unit.complex else {
        return;
    };
    let team = unit.team;

    complex_shrink(state, team, complex);

    if let Some(target) = state.unit_mut(id) {
        target.complex = None;
    }

    // Первый непроверенный сосед определяет «главную» группу: она остаётся в
    // прежнем комплексе (C++ `building_complex`).
    let first = first_untested_connection(state, &unit);

    if let Some(target) = state.unit_mut(id) {
        target.connectors |= CONNECTION_BEING_TESTED;
    }

    let mut main_complex = None;

    if let Some(building) = first {
        main_complex = state.unit(building).and_then(|target| target.complex);
        test_connections(state, building);
    }

    loop {
        let Some(unit) = state.unit(id).cloned() else {
            return;
        };
        let Some(building) = first_untested_connection(state, &unit) else {
            break;
        };

        let Some(new_complex) = state
            .team_units
            .get_mut(team.0 as usize)
            .map(|units| units.create_complex())
        else {
            break;
        };

        // C++ `DetachComplex` (`unitinfo.cpp:2292-2301`) временно привязывает
        // **само отделяемое здание** к новому комплексу (`complex = new_complex`)
        // без `Grow`: рекурсия `AttachComplex` группы натыкается на него,
        // видит `this->complex == complex` и останавливается — иначе новый
        // комплекс «захватил» бы через биты отделяемого здания весь прежний
        // граф. После пересчёта привязка снимается (`complex = nullptr`).
        if let Some(target) = state.unit_mut(id) {
            target.complex = Some(new_complex);
        }

        test_connections(state, building);
        attach_complex(state, building, new_complex);
        update_resources_total(state, team, new_complex);

        if let Some(target) = state.unit_mut(id) {
            target.complex = None;
        }
    }

    if let Some(main_complex) = main_complex {
        update_resources_total(state, team, main_complex);
    }

    for id in state.unit_list(UnitListKind::Stationary).to_vec() {
        if let Some(target) = state.unit_mut(id) {
            target.connectors &= !CONNECTION_BEING_TESTED;
        }
    }
}

/// `UnitsManager_RemoveConnections(unit)` (`units_manager.cpp:1556`): снять
/// парные биты коннекторов у соседей и разделить комплекс, в котором состояло
/// здание.
///
/// Вызывается до удаления здания: `DetachComplex` идёт первым (ему нужны биты
/// самого здания), затем у соседей снимаются биты пары — ровно порядок C++.
pub fn remove_connections(state: &mut GameState, id: UnitId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    if !unit.is_complex_member() {
        return;
    }

    let team = unit.team;
    let size = unit_size(unit.flags);
    let grid_x = unit.grid.x;
    let grid_y = unit.grid.y;

    detach_complex(state, id);

    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    // Пары очищаются по тем же правилам, что выставлялись в
    // `UnitsManager_UpdateConnectors`.
    let clear_neighbor = |state: &mut GameState, building: Option<UnitId>, bit: u16| {
        let Some(building) = building else {
            return;
        };
        if let Some(mut target) = state.unit(building).cloned() {
            target.connectors &= !bit;
            state.write_unit(building, target);
        }
    };

    if unit.connectors & CONNECTOR_NORTH_LEFT != 0 {
        let building = team_building_at(state, team, Point::new(grid_x, grid_y - 1));
        let bit = match building {
            Some(building) if is_standalone_or_connector(state, building) => CONNECTOR_SOUTH_LEFT,
            Some(building)
                if Some(building)
                    == team_building_at(state, team, Point::new(grid_x + 1, grid_y - 1)) =>
            {
                CONNECTOR_SOUTH_LEFT
            }
            _ => CONNECTOR_SOUTH_RIGHT,
        };
        clear_neighbor(state, building, bit);
    }

    if unit.connectors & CONNECTOR_NORTH_RIGHT != 0 {
        let building = team_building_at(state, team, Point::new(grid_x + 1, grid_y - 1));
        let bit = match building {
            Some(building) if is_standalone_or_connector(state, building) => CONNECTOR_SOUTH_LEFT,
            Some(building)
                if Some(building)
                    == team_building_at(state, team, Point::new(grid_x, grid_y - 1)) =>
            {
                CONNECTOR_SOUTH_RIGHT
            }
            _ => CONNECTOR_SOUTH_LEFT,
        };
        clear_neighbor(state, building, bit);
    }

    if unit.connectors & CONNECTOR_EAST_TOP != 0 {
        let building = team_building_at(state, team, Point::new(grid_x + size, grid_y));
        let bit = match building {
            Some(building) if is_standalone_or_connector(state, building) => CONNECTOR_WEST_TOP,
            Some(building)
                if Some(building)
                    == team_building_at(state, team, Point::new(grid_x + size, grid_y + 1)) =>
            {
                CONNECTOR_WEST_TOP
            }
            _ => CONNECTOR_WEST_BOTTOM,
        };
        clear_neighbor(state, building, bit);
    }

    if unit.connectors & CONNECTOR_EAST_BOTTOM != 0 {
        let building = team_building_at(state, team, Point::new(grid_x + size, grid_y + 1));
        let bit = match building {
            Some(building) if is_standalone_or_connector(state, building) => CONNECTOR_WEST_TOP,
            Some(building)
                if Some(building)
                    == team_building_at(state, team, Point::new(grid_x + size, grid_y)) =>
            {
                CONNECTOR_WEST_BOTTOM
            }
            _ => CONNECTOR_WEST_TOP,
        };
        clear_neighbor(state, building, bit);
    }

    if unit.connectors & CONNECTOR_SOUTH_LEFT != 0 {
        let building = team_building_at(state, team, Point::new(grid_x, grid_y + size));
        let bit = match building {
            Some(building) if is_standalone_or_connector(state, building) => CONNECTOR_NORTH_LEFT,
            Some(building)
                if Some(building)
                    == team_building_at(state, team, Point::new(grid_x + 1, grid_y + size)) =>
            {
                CONNECTOR_NORTH_LEFT
            }
            _ => CONNECTOR_NORTH_RIGHT,
        };
        clear_neighbor(state, building, bit);
    }

    if unit.connectors & CONNECTOR_SOUTH_RIGHT != 0 {
        let building = team_building_at(state, team, Point::new(grid_x + 1, grid_y + size));
        let bit = match building {
            Some(building) if is_standalone_or_connector(state, building) => CONNECTOR_NORTH_LEFT,
            Some(building)
                if Some(building)
                    == team_building_at(state, team, Point::new(grid_x, grid_y + size)) =>
            {
                CONNECTOR_NORTH_RIGHT
            }
            _ => CONNECTOR_NORTH_LEFT,
        };
        clear_neighbor(state, building, bit);
    }

    if unit.connectors & CONNECTOR_WEST_TOP != 0 {
        let building = team_building_at(state, team, Point::new(grid_x - 1, grid_y));
        let bit = match building {
            Some(building) if is_standalone_or_connector(state, building) => CONNECTOR_EAST_TOP,
            Some(building)
                if Some(building)
                    == team_building_at(state, team, Point::new(grid_x - 1, grid_y + 1)) =>
            {
                CONNECTOR_EAST_TOP
            }
            _ => CONNECTOR_EAST_BOTTOM,
        };
        clear_neighbor(state, building, bit);
    }

    if unit.connectors & CONNECTOR_WEST_BOTTOM != 0 {
        let building = team_building_at(state, team, Point::new(grid_x - 1, grid_y + 1));
        let bit = match building {
            Some(building) if is_standalone_or_connector(state, building) => CONNECTOR_EAST_TOP,
            Some(building)
                if Some(building)
                    == team_building_at(state, team, Point::new(grid_x - 1, grid_y)) =>
            {
                CONNECTOR_EAST_BOTTOM
            }
            _ => CONNECTOR_EAST_TOP,
        };
        clear_neighbor(state, building, bit);
    }
}

/// `flags & (CONNECTOR_UNIT | STANDALONE)`.
fn is_standalone_or_connector(state: &GameState, building: UnitId) -> bool {
    state
        .unit(building)
        .is_some_and(|unit| unit.flags & (CONNECTOR_UNIT | STANDALONE) != 0)
}

/// `UnitsManager_UpdateConnectors(unit)` (`units_manager.cpp:2051`).
///
/// Выставляет биты `CONNECTOR_*` юниту и соседям, затем сливает комплексы
/// (`AttachToPrimaryComplex`) и пересчитывает запасы
/// (`Access_UpdateResourcesTotal`).
///
/// Статистика постройки (`stats_factories_built`/`stats_mines_built`/
/// `stats_buildings_built`) в C++ тоже инкрементируется здесь; порт считает её
/// при высадке ([`crate::deploy`]), потому что вызовов `UpdateConnectors` в
/// модели больше одного (спавн + завершение масштабирования), и повторный
/// инкремент разошёлся бы с оракулом.
pub fn update_connectors(state: &mut GameState, id: UnitId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    if !unit.is_complex_member() {
        return;
    }

    let team = unit.team;
    let size = unit_size(unit.flags);
    let grid_x = unit.grid.x;
    let grid_y = unit.grid.y;
    let mut connectors = unit.connectors;

    // Пара (бит юнита, «левый/верхний» бит, «правый/нижний» бит) по каждому
    // направлению — 1:1 C++.
    let mut link = |state: &mut GameState,
                    target: Option<UnitId>,
                    own_bit: u16,
                    first_bit: u16,
                    second_bit: u16,
                    compare: Point| {
        let Some(target) = target else {
            return;
        };

        connectors |= own_bit;

        let neighbor_is_edge = is_standalone_or_connector(state, target);
        let same = Some(target) == team_building_at(state, team, compare);
        let bit = if neighbor_is_edge || same {
            first_bit
        } else {
            second_bit
        };

        if let Some(mut neighbor) = state.unit(target).cloned() {
            neighbor.connectors |= bit;
            state.write_unit(target, neighbor);
        }
    };

    // `CONNECTOR_NORTH_LEFT` — `(grid_x, grid_y - 1)`.
    let target = team_building_at(state, team, Point::new(grid_x, grid_y - 1));
    link(
        state,
        target,
        CONNECTOR_NORTH_LEFT,
        CONNECTOR_SOUTH_LEFT,
        CONNECTOR_SOUTH_RIGHT,
        Point::new(grid_x + 1, grid_y - 1),
    );

    // `CONNECTOR_SOUTH_LEFT` — `(grid_x, grid_y + unit_size)`.
    let target = team_building_at(state, team, Point::new(grid_x, grid_y + size));
    link(
        state,
        target,
        CONNECTOR_SOUTH_LEFT,
        CONNECTOR_NORTH_LEFT,
        CONNECTOR_NORTH_RIGHT,
        Point::new(grid_x + 1, grid_y + size),
    );

    if unit.flags & BUILDING != 0 {
        // `CONNECTOR_NORTH_RIGHT` — `(grid_x + 1, grid_y - 1)`.
        let target = team_building_at(state, team, Point::new(grid_x + 1, grid_y - 1));
        link(
            state,
            target,
            CONNECTOR_NORTH_RIGHT,
            CONNECTOR_SOUTH_LEFT,
            CONNECTOR_SOUTH_RIGHT,
            Point::new(grid_x, grid_y - 1),
        );

        // `CONNECTOR_SOUTH_RIGHT` — `(grid_x + 1, grid_y + unit_size)`.
        let target = team_building_at(state, team, Point::new(grid_x + 1, grid_y + size));
        link(
            state,
            target,
            CONNECTOR_SOUTH_RIGHT,
            CONNECTOR_NORTH_LEFT,
            CONNECTOR_NORTH_RIGHT,
            Point::new(grid_x, grid_y + size),
        );
    }

    // `CONNECTOR_EAST_TOP` — `(grid_x + unit_size, grid_y)`.
    let target = team_building_at(state, team, Point::new(grid_x + size, grid_y));
    link(
        state,
        target,
        CONNECTOR_EAST_TOP,
        CONNECTOR_WEST_TOP,
        CONNECTOR_WEST_BOTTOM,
        Point::new(grid_x + size, grid_y + 1),
    );

    // `CONNECTOR_WEST_TOP` — `(grid_x - 1, grid_y)`.
    let target = team_building_at(state, team, Point::new(grid_x - 1, grid_y));
    link(
        state,
        target,
        CONNECTOR_WEST_TOP,
        CONNECTOR_EAST_TOP,
        CONNECTOR_EAST_BOTTOM,
        Point::new(grid_x - 1, grid_y + 1),
    );

    if unit.flags & BUILDING != 0 {
        // `CONNECTOR_EAST_BOTTOM` — `(grid_x + unit_size, grid_y + 1)`.
        let target = team_building_at(state, team, Point::new(grid_x + size, grid_y + 1));
        link(
            state,
            target,
            CONNECTOR_EAST_BOTTOM,
            CONNECTOR_WEST_TOP,
            CONNECTOR_WEST_BOTTOM,
            Point::new(grid_x + size, grid_y),
        );

        // `CONNECTOR_WEST_BOTTOM` — `(grid_x - 1, grid_y + 1)`.
        let target = team_building_at(state, team, Point::new(grid_x - 1, grid_y + 1));
        link(
            state,
            target,
            CONNECTOR_WEST_BOTTOM,
            CONNECTOR_EAST_TOP,
            CONNECTOR_EAST_BOTTOM,
            Point::new(grid_x - 1, grid_y),
        );
    }

    if connectors != unit.connectors {
        if let Some(mut target) = state.unit(id).cloned() {
            target.connectors = connectors;
            state.write_unit(id, target);
        }
    }

    attach_to_primary_complex(state, id);

    if let Some(complex) = state.unit(id).and_then(|unit| unit.complex) {
        update_resources_total(state, team, complex);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{TeamClan, UnitType};
    use crate::team::TeamUnits;

    fn test_state() -> GameState {
        let mut state = GameState::new();
        state.teams[0].team_type = crate::team::TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
        state
    }

    fn spawn(state: &mut GameState, unit_type: UnitType, grid: Point) -> UnitId {
        state
            .spawn_unit(unit_type, TeamId::RED, grid, 0)
            .expect("unit")
    }

    #[test]
    fn adjacent_buildings_share_one_complex_and_capacity() {
        let mut state = test_state();
        let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
        let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(12, 10));

        let mine_complex = state.unit(mine).unwrap().complex;
        assert!(mine_complex.is_some());
        assert_eq!(
            state.unit(dump).unwrap().complex,
            mine_complex,
            "соседние здания должны слиться в один комплекс"
        );
        assert_eq!(
            state.unit(mine).unwrap().connectors & CONNECTOR_EAST_TOP,
            CONNECTOR_EAST_TOP,
            "у шахты — восточный верхний бит (C++ UpdateConnectors)"
        );
        assert_eq!(
            state.unit(dump).unwrap().connectors & CONNECTOR_WEST_TOP,
            CONNECTOR_WEST_TOP,
            "у ADUMP — западный верхний бит"
        );
        assert_eq!(
            state.team_units[0]
                .get_complex(mine_complex.unwrap())
                .unwrap()
                .buildings,
            2
        );

        let (materials, capacity) =
            crate::alloc::get_cargo_info(&state, TeamId::RED, mine_complex.unwrap());
        assert_eq!(capacity.raw, 25 + 50, "ёмкость складов комплекса");
        assert_eq!(materials.raw, 0);
    }

    #[test]
    fn connector_units_merge_two_complexes() {
        let mut state = test_state();
        let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
        let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(15, 10));

        assert_ne!(
            state.unit(mine).unwrap().complex,
            state.unit(dump).unwrap().complex,
            "до коннекторов комплексы разные"
        );

        spawn(&mut state, UnitType::Connector, Point::new(12, 10));
        spawn(&mut state, UnitType::Connector, Point::new(13, 10));
        spawn(&mut state, UnitType::Connector, Point::new(14, 10));

        assert_eq!(
            state.unit(dump).unwrap().complex,
            state.unit(mine).unwrap().complex,
            "цепочка коннекторов сливает комплексы"
        );
        assert_eq!(
            state.team_units[0]
                .get_complex(state.unit(mine).unwrap().complex.unwrap())
                .unwrap()
                .buildings,
            5
        );
    }

    #[test]
    fn destroying_a_connector_splits_the_complex() {
        let mut state = test_state();
        let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
        let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(15, 10));
        let bridge = spawn(&mut state, UnitType::Connector, Point::new(12, 10));
        spawn(&mut state, UnitType::Connector, Point::new(13, 10));
        spawn(&mut state, UnitType::Connector, Point::new(14, 10));

        let merged = state.unit(mine).unwrap().complex.unwrap();
        assert_eq!(state.unit(dump).unwrap().complex, Some(merged));

        state.destroy_unit(bridge);

        let mine_complex = state.unit(mine).unwrap().complex.unwrap();
        let dump_complex = state.unit(dump).unwrap().complex.unwrap();
        assert_ne!(mine_complex, dump_complex, "комплекс распался");
        assert_eq!(
            state.team_units[0]
                .get_complex(mine_complex)
                .unwrap()
                .buildings,
            1,
            "шахта осталась одна"
        );
        assert_eq!(
            state.team_units[0]
                .get_complex(dump_complex)
                .unwrap()
                .buildings,
            3,
            "ADUMP + два оставшихся коннектора"
        );

        // Соседний коннектор не должен помнить уничтоженный.
        let neighbor = state
            .unit_ids_in_order()
            .into_iter()
            .find(|id| state.unit(*id).unwrap().grid == Point::new(13, 10))
            .unwrap();
        assert_eq!(
            state.unit(neighbor).unwrap().connectors & CONNECTOR_EAST_TOP,
            CONNECTOR_EAST_TOP,
            "у (13,10) связь с (14,10) осталась"
        );
        assert_eq!(
            state.unit(neighbor).unwrap().connectors & CONNECTOR_WEST_TOP,
            0,
            "но связи с уничтоженным (12,10) уже нет"
        );
    }

    #[test]
    fn connectors_and_complex_membership_are_in_the_snapshot_and_crc() {
        fn build() -> (GameState, UnitId, UnitId) {
            let mut state = test_state();
            let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
            let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(12, 10));
            state.unit_mut(mine).unwrap().storage = 9;
            state.unit_mut(dump).unwrap().storage = 4;
            let complex = state.unit(mine).unwrap().complex.expect("complex");
            update_resources_total(&mut state, TeamId::RED, complex);
            (state, mine, dump)
        }

        let (first, mine, dump) = build();
        let (second, _, _) = build();

        // Два одинаковых прогона — одинаковый снапшот и CRC (`state_digest`
        // считается по `snapshot_bytes`).
        assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
        assert_eq!(
            crate::crc::state_digest(&first),
            crate::crc::state_digest(&second)
        );

        // Связь комплекса (unit.complex) входит в снапшот: разрыв связи меняет
        // и байты, и CRC.
        let mut detached = first.clone();
        detached.unit_mut(dump).unwrap().complex = None;
        assert_ne!(detached.snapshot_bytes(), first.snapshot_bytes());
        assert_ne!(
            crate::crc::state_digest(&detached),
            crate::crc::state_digest(&first)
        );

        // Биты коннекторов тоже входят (C++ `UnitInfo::connectors`).
        let mut reconnected = first.clone();
        reconnected.unit_mut(mine).unwrap().connectors ^= CONNECTOR_EAST_TOP;
        assert_ne!(reconnected.snapshot_bytes(), first.snapshot_bytes());
        assert_ne!(
            crate::crc::state_digest(&reconnected),
            crate::crc::state_digest(&first)
        );

        // Запас комплекса (`Complex::material` как кэш `Access_UpdateResourcesTotal`)
        // входит в снапшот: `Complex::Transfer` и UI читают именно его.
        let mut changed = first.clone();
        let complex = changed.unit(mine).unwrap().complex.expect("complex");
        if let Some(target) = changed.complex_mut(TeamId::RED, complex.0 as u16) {
            target.material = target.material.wrapping_add(1);
        }
        assert_ne!(changed.snapshot_bytes(), first.snapshot_bytes());
        assert_ne!(
            crate::crc::state_digest(&changed),
            crate::crc::state_digest(&first)
        );
    }
}
