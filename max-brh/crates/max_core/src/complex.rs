//! Building complexes and connectors — a port of `complex.cpp` + the connector part
//! `units_manager.cpp`/`unitinfo.cpp`.
//!
//! # Oracle
//!
//! * `units_manager.cpp:2051-2225` — `UnitsManager_UpdateConnectors`: in eight
//!   directions it searches for team buildings (`Access_GetTeamBuilding`,
//!   `access.cpp:2157`), sets `CONNECTOR_*` bits on itself and the neighbor (for
//!   `CONNECTOR_UNIT`/`STANDALONE` — always the "left/top" variant of the pair),
//!   then `AttachToPrimaryComplex` and `Access_UpdateResourcesTotal`;
//! * `unitinfo.cpp:1978-2052` — `UnitInfo::AttachComplex`: recursively
//!   moves the building and the whole connected subgraph to one `Complex`
//!   (`Complex::Grow`/`Shrink`);
//! * `unitinfo.cpp:2054-2131` — `UnitInfo::AttachToPrimaryComplex`: from the
//!   neighboring complexes the one with the **lowest id** is chosen, otherwise
//!   a new one is created (`TeamUnits::CreateComplex`);
//! * `unitinfo.cpp:2133-2203` — `UnitInfo::TestConnections`: graph traversal with
//!   marking `CONNECTION_BEING_TESTED` to avoid looping;
//! * `unitinfo.cpp:2205-2273` — `UnitInfo::GetFirstUntestedConnection`;
//! * `unitinfo.cpp:2275-2313` — `UnitInfo::DetachComplex`: the building leaves the
//!   complex, the remaining graph splits into connected groups, each gets
//!   its own `Complex` (`CreateComplex` + `AttachComplex`), stocks are recomputed
//!   `Access_UpdateResourcesTotal` (`access.cpp:912`);
//! * `units_manager.cpp:1556-1709` — `UnitsManager_RemoveConnections`: clearing
//!   the paired bits on neighbors before destroying/moving a building;
//! * `complex.cpp:136-203` — `Complex::GetCargoInfo`/`TransferCargo`/`Transfer`:
//!   the capacity and stock of a complex are the **sum** of connected storages, and charging
//!   goes over member storages in `UnitsManager_StationaryUnits` order.
//!
//! # Model
//!
//! A complex is a connected (via connectors) group of buildings/`CONNECTOR_UNIT`/
//! `STANDALONE` of one team. `UnitInfo::complex` stores the complex id, and
//! `UnitInfo::connectors` — `CONNECTOR_*` bits (they enter the snapshot/CRC).
//! The stocks and capacity of a complex are computed from the buildings
//! ([`crate::alloc::get_cargo_info`]), while `Complex::{raw,fuel,gold,power,
//! workers}` is a cache for the current turn (`Access_UpdateResourcesTotal`).

use crate::alloc::update_resources_total;
use crate::classification::Footprint;
use crate::enums::{
    unit_flags::{CONNECTOR_UNIT, STANDALONE},
    UnitOrder, CONNECTION_BEING_TESTED, CONNECTOR_EAST_BOTTOM, CONNECTOR_EAST_TOP,
    CONNECTOR_NORTH_LEFT, CONNECTOR_NORTH_RIGHT, CONNECTOR_SOUTH_LEFT, CONNECTOR_SOUTH_RIGHT,
    CONNECTOR_WEST_BOTTOM, CONNECTOR_WEST_TOP,
};
use crate::game::GameState;
use crate::geo::Point;
use crate::team::{ComplexId, TeamId};
use crate::unit::{UnitId, UnitInfo, UnitListKind};

/// Connector traversal order in C++ (`UnitInfo::AttachComplex`,
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

/// Structure size in cells by footprint: `Quad` — 2, `Single` — 1
/// (`units_manager.cpp:2056`: `(flags & BUILDING) ? 2 : 1`).
#[inline]
pub fn unit_size(footprint: Footprint) -> i16 {
    if footprint.is_quad() {
        2
    } else {
        1
    }
}

/// `Access_GetTeamBuilding(team, grid_x, grid_y)` (`access.cpp:2157`): the first
/// team building on the cell that is not yet in `ORDER_IDLE` and is a complex
/// member (`UnitInfo::IsComplexMember`).
///
/// `ORDER_IDLE` is filtered out on purpose: a completed building on a site
/// (`ORDER_IDLE`/`BUILDING_READY`) does not join the complex until
/// `UnitsManager_BuildingReady` returns an order to it.
pub fn team_building_at(state: &GameState, team: TeamId, grid: Point) -> Option<UnitId> {
    state.units_at(grid).iter().copied().find(|id| {
        state.unit(*id).is_some_and(|unit| {
            unit.team == team && unit.orders != UnitOrder::Idle && unit.is_complex_member()
        })
    })
}

/// `UnitInfo::GetConnectedBuilding(connector)` (`unitinfo.cpp:1938-1976`).
///
/// Offsets — exactly the C++ switch: for a building (`grid_size = 2`) the south-east
/// pairs are shifted by 2 cells.
pub fn connected_building(state: &GameState, unit: &UnitInfo, connector: u16) -> Option<UnitId> {
    let size = unit_size(unit.footprint());
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
// Complex::Grow/Shrink via TeamUnits
// ---------------------------------------------------------------------------

/// `Complex::Grow` (`complex.cpp:205`).
fn complex_grow(state: &mut GameState, team: TeamId, complex: ComplexId) {
    if let Some(units) = state.team_units.get_mut(team.0 as usize) {
        if let Some(target) = units.get_complex_mut(complex) {
            target.grow();
        }
    }
}

/// `Complex::Shrink` (`complex.cpp:207`): with zero buildings the complex is deleted
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
/// Moves the building to `complex` (with `Grow` of the new and `Shrink` of the old one,
/// an empty old complex is deleted) and recursively — all connected neighbors.
/// The recursion stops at a building already attached to `complex`, so
/// cycles in the connector graph are safe.
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
/// From the complexes of neighboring buildings the one with the lowest id is chosen
/// (`Complex::GetId`); if there are no neighbors, a new one is created
/// (`TeamUnits::CreateComplex`). Then [`attach_complex`] merges the whole connected
/// subgraph into the chosen complex.
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

/// `UnitInfo::TestConnections()` (`unitinfo.cpp:2133`): mark
/// the whole connected subgraph as `CONNECTION_BEING_TESTED`, starting from `id`.
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

/// `UnitInfo::DetachComplex()` (`unitinfo.cpp:2275`): the building leaves the
/// complex, and the remaining connected groups get their own complexes
/// (`CreateComplex` + `AttachComplex`) and recomputed stocks.
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

    // The first untested neighbor determines the "main" group: it stays in
    // the old complex (C++ `building_complex`).
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

        // C++ `DetachComplex` (`unitinfo.cpp:2292-2301`) temporarily attaches
        // **the detached building itself** to the new complex (`complex = new_complex`)
        // without `Grow`: the group's `AttachComplex` recursion hits it,
        // sees `this->complex == complex` and stops — otherwise the new
        // complex would "capture" the whole old graph through the detached building's
        // bits. After recomputation the attachment is removed (`complex = nullptr`).
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

/// `UnitsManager_RemoveConnections(unit)` (`units_manager.cpp:1556`): clear
/// the paired connector bits on neighbors and split the complex the
/// building belonged to.
///
/// Called before the building is deleted: `DetachComplex` goes first (it needs the
/// building's own bits), then the pair bits are cleared on neighbors — exactly the C++ order.
pub fn remove_connections(state: &mut GameState, id: UnitId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    if !unit.is_complex_member() {
        return;
    }

    let team = unit.team;
    let size = unit_size(unit.footprint());
    let grid_x = unit.grid.x;
    let grid_y = unit.grid.y;

    detach_complex(state, id);

    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    // Pairs are cleared by the same rules as they were set in
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
/// Sets `CONNECTOR_*` bits on the unit and neighbors, then merges complexes
/// (`AttachToPrimaryComplex`) and recomputes stocks
/// (`Access_UpdateResourcesTotal`).
///
/// Construction statistics (`stats_factories_built`/`stats_mines_built`/
/// `stats_buildings_built`) are incremented here in C++ too; the port counts it
/// during deployment ([`crate::deploy`]), because in the model there is more than one
/// `UpdateConnectors` call (spawn + scaling completion), and a repeated
/// increment would diverge from the oracle.
pub fn update_connectors(state: &mut GameState, id: UnitId) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    if !unit.is_complex_member() {
        return;
    }

    let team = unit.team;
    let size = unit_size(unit.footprint());
    let grid_x = unit.grid.x;
    let grid_y = unit.grid.y;
    let mut connectors = unit.connectors;

    // The pair (unit bit, "left/top" bit, "right/bottom" bit) for each
    // direction — 1:1 C++.
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

    // "Right/bottom" pairs exist only for 2×2 structures. A turret is `Mobility::
    // Building`, but `Footprint::Single` and not a complex member (`IsComplexMember`),
    // so this branch does not run for it: layout `is_quad()`, not semantics.
    if unit.is_quad() {
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

    if unit.is_quad() {
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
    use crate::classification::UnitTypeClassify;
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

        // The neighboring connector must not remember the destroyed one.
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

        // Two identical runs — the same snapshot and CRC (`state_digest`
        // is computed over `snapshot_bytes`).
        assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
        assert_eq!(
            crate::crc::state_digest(&first),
            crate::crc::state_digest(&second)
        );

        // The complex link (unit.complex) enters the snapshot: breaking the link changes
        // both the bytes and the CRC.
        let mut detached = first.clone();
        detached.unit_mut(dump).unwrap().complex = None;
        assert_ne!(detached.snapshot_bytes(), first.snapshot_bytes());
        assert_ne!(
            crate::crc::state_digest(&detached),
            crate::crc::state_digest(&first)
        );

        // Connector bits enter too (C++ `UnitInfo::connectors`).
        let mut reconnected = first.clone();
        reconnected.unit_mut(mine).unwrap().connectors ^= CONNECTOR_EAST_TOP;
        assert_ne!(reconnected.snapshot_bytes(), first.snapshot_bytes());
        assert_ne!(
            crate::crc::state_digest(&reconnected),
            crate::crc::state_digest(&first)
        );

        // The complex stock (`Complex::raw` as the `Access_UpdateResourcesTotal` cache)
        // enters the snapshot: `Complex::Transfer` and the UI read exactly it.
        let mut changed = first.clone();
        let complex = changed.unit(mine).unwrap().complex.expect("complex");
        if let Some(target) = changed.complex_mut(TeamId::RED, complex.0 as u16) {
            target.raw = target.raw.wrapping_add(1);
        }
        assert_ne!(changed.snapshot_bytes(), first.snapshot_bytes());
        assert_ne!(
            crate::crc::state_digest(&changed),
            crate::crc::state_digest(&first)
        );
    }

    #[test]
    fn connector_unit_size_uses_layout_footprint() {
        // A turret is a building semantically, but 1×1: connector offsets and the anchor
        // must not blow it up to 2×2.
        assert_eq!(unit_size(UnitType::GunTurret.footprint()), 1);
        assert_eq!(unit_size(UnitType::MiningStation.footprint()), 2);
        assert!(UnitType::GunTurret.is_building());
        assert!(!UnitType::GunTurret.is_quad());
    }
}
