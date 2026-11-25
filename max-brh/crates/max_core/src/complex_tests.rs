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

#[test]
fn connector_order_matches_the_cpp_traversal() {
    // C++ `UnitInfo::AttachComplex`/`TestConnections`/`AttachToPrimaryComplex`
    // walk the eight bits in exactly this order (`unitinfo.cpp`).
    assert_eq!(
        CONNECTOR_ORDER,
        [
            CONNECTOR_NORTH_LEFT,
            CONNECTOR_NORTH_RIGHT,
            CONNECTOR_EAST_TOP,
            CONNECTOR_EAST_BOTTOM,
            CONNECTOR_SOUTH_RIGHT,
            CONNECTOR_SOUTH_LEFT,
            CONNECTOR_WEST_BOTTOM,
            CONNECTOR_WEST_TOP,
        ]
    );
    assert_eq!(CONNECTOR_ORDER.len(), 8);
    // The bit list is pairwise distinct.
    for (index, bit) in CONNECTOR_ORDER.iter().enumerate() {
        assert!(
            !CONNECTOR_ORDER[index + 1..].contains(bit),
                "duplicate connector bit {bit:#x}"
        );
    }
}

#[test]
fn connected_building_offsets_match_cpp_for_a_quad() {
    let mut state = test_state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(20, 20));

    // C++ `UnitInfo::GetConnectedBuilding` offsets for `grid_size = 2`.
    let cells = [
        (CONNECTOR_NORTH_LEFT, Point::new(20, 19)),
        (CONNECTOR_NORTH_RIGHT, Point::new(21, 19)),
        (CONNECTOR_EAST_TOP, Point::new(22, 20)),
        (CONNECTOR_EAST_BOTTOM, Point::new(22, 21)),
        (CONNECTOR_SOUTH_LEFT, Point::new(20, 22)),
        (CONNECTOR_SOUTH_RIGHT, Point::new(21, 22)),
        (CONNECTOR_WEST_TOP, Point::new(19, 20)),
        (CONNECTOR_WEST_BOTTOM, Point::new(19, 21)),
    ];

    let mut expected = Vec::new();
    for (connector, grid) in cells {
        expected.push((connector, spawn(&mut state, UnitType::Connector, grid)));
    }

    let unit = state.unit(mine).expect("mine").clone();
    for (connector, id) in expected {
        assert_eq!(
            connected_building(&state, &unit, connector),
            Some(id),
                "connector {connector:#x}"
        );
    }

    // A bit that is not one of the eight directions has no cell at all.
    assert_eq!(connected_building(&state, &unit, 0x0200), None);
}

#[test]
fn attach_complex_merges_and_deletes_the_empty_source() {
    let mut state = test_state();
    let first = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let second = spawn(&mut state, UnitType::MaterialStorage, Point::new(30, 30));

    let first_complex = state.unit(first).expect("first").complex.expect("complex");
    let second_complex = state.unit(second).expect("second").complex.expect("complex");
    assert_ne!(first_complex, second_complex);

    attach_complex(&mut state, second, first_complex);

    assert_eq!(state.unit(second).expect("second").complex, Some(first_complex));
    assert_eq!(
        state.team_units[0]
            .get_complex(first_complex)
            .expect("target")
            .buildings,
        2,
            "Complex::Grow on the target"
    );
    assert!(
        state.team_units[0].get_complex(second_complex).is_none(),
            "Complex::Shrink removes a complex with zero buildings"
    );
}

#[test]
fn detach_complex_of_an_isolated_building_removes_its_complex() {
    let mut state = test_state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let complex = state.unit(mine).expect("mine").complex.expect("complex");

    detach_complex(&mut state, mine);

    assert_eq!(state.unit(mine).expect("mine").complex, None);
    assert!(state.team_units[0].get_complex(complex).is_none());
    assert!(state.team_units[0].complexes().is_empty());
}

#[test]
fn first_untested_connection_skips_marked_buildings() {
    let mut state = test_state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let dump = spawn(&mut state, UnitType::MaterialStorage, Point::new(12, 10));

    let mine_unit = state.unit(mine).expect("mine").clone();
    assert_eq!(
        first_untested_connection(&state, &mine_unit),
        Some(dump),
            "the east neighbor is untested"
    );

    // `TestConnections` marks the whole subgraph as BEING_TESTED; the same
    // neighbor is then invisible to `GetFirstUntestedConnection`.
    test_connections(&mut state, mine);
    assert_eq!(
        state.unit(dump).expect("dump").connectors & CONNECTION_BEING_TESTED,
        CONNECTION_BEING_TESTED
    );
    let mine_unit = state.unit(mine).expect("mine").clone();
    assert_eq!(first_untested_connection(&state, &mine_unit), None);
}
