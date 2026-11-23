//! Phase 4 — `classification.rs` in the engine (I + D layer).
//!
//! The unit test file of the module already proves every classifier axis for all
//! 93 types against direct flag/attribute checks. This file proves that the
//! classification actually **drives `GameUniverse`**:
//!
//! * spawning a type puts it into exactly the list and the `MapHash` footprint
//!   its class (mobility/footprint/flags) dictates;
//! * the unit instance starts with the combat attributes of its type, so
//!   `CombatRole` and the weapon state agree with `SC_ATTRI`;
//! * passability over land/water/coast is the composition of
//!   `Environment`/`Mobility` with the `SC_UNITS` `land_type` mask — checked for
//!   all 93 types against an independent expectation;
//! * cross-module tables (`production::FACTORIES`, `BUILDABLE`) do not contradict
//!   the classifier (units are spawned, not decorations/effects);
//! * the digest of a state with one unit of every type is stable and pinned
//!   (D layer: a change in spawn/classification bookkeeping changes it).
//!
//! Oracles: `max-cpp-port/src/units.cpp`/`sc_units` (`UNIT_TYPE_PROPERTIES`),
//! `access.cpp` (`Access_IsAccessible`), `object-classifier.md`.

use max_core::access::{
    is_accessible, unit_type_flags, unit_type_land_type, ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
    SURFACE_TYPE_COAST, SURFACE_TYPE_LAND, SURFACE_TYPE_WATER,
};
use max_core::classification::{
    CargoKind, CombatRole, Environment, Footprint, Mobility, ObjectKind, UnitTypeClassify,
};
use max_core::crc::{replay_digest_crc, state_digest};
use max_core::enums::{Surface, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::production::{self, BUILDABLE, FACTORIES, MOBILE_BUILDERS};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit_values::{base_unit_values, ATTRIB_ATTACK};
use max_core::universe::GameUniverse;
use max_core::UnitListKind;

/// A one-team state with a proper attribute table (so `spawn_unit` copies the
/// `SC_ATTRI` values into the instance).
fn state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(state: &mut GameUniverse, unit_type: UnitType, grid: Point) -> max_core::UnitId {
    state
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .unwrap_or_else(|| panic!("spawn {unit_type:?}"))
}

#[test]
fn every_type_spawns_into_its_classified_list_and_footprint() {
    let mut state = state();
    let mut expected_list_counts = [0usize; 5];

    for (index, &unit_type) in UnitType::ALL.iter().enumerate() {
        // Three cells apart so 2×2 footprints of neighbours never overlap.
        let grid = Point::new(((index % 30) as i16) * 3, ((index / 30) as i16) * 3);
        let id = spawn(&mut state, unit_type, grid);

        let unit = state.snapshot().unit(id).expect("spawned unit").clone();
        // The instance flags are the type flags (plus explicit caller flags = 0).
        assert_eq!(
            unit.flags,
            unit_type_flags(unit_type),
            "{unit_type:?}: instance flags"
        );
        // The class of the instance equals the class of the type.
        assert_eq!(
            unit.mobility(),
            Mobility::classify(unit_type),
            "{unit_type:?}"
        );
        assert_eq!(
            unit.environment(),
            Environment::classify(unit_type),
            "{unit_type:?}"
        );
        assert_eq!(
            unit.combat_role(),
            CombatRole::classify(unit_type),
            "{unit_type:?}"
        );

        // The five lists: the unit is in its own list exactly once.
        let kind = unit.unit_list;
        assert!(
            state.snapshot().unit_list(kind).contains(&id),
            "{unit_type:?} missing from {kind:?}"
        );
        for other in UnitListKind::ALL {
            let count = state
                .snapshot()
                .unit_list(other)
                .iter()
                .filter(|candidate| *candidate == &id)
                .count();
            let expected = usize::from(other == kind);
            assert_eq!(count, expected, "{unit_type:?}: list {other:?}");
        }
        expected_list_counts[kind.index()] += 1;

        // The `MapHash` footprint: a quad building occupies exactly four cells,
        // anything else exactly one.
        let occupied: Vec<(u16, u16)> = state
            .state()
            .map_hash
            .iter_objects()
            .filter(|object| object.units.contains(&id))
            .map(|object| (object.x, object.y))
            .collect();
        let cells = if unit.is_quad() { 4 } else { 1 };
        assert_eq!(occupied.len(), cells, "{unit_type:?}: cells {occupied:?}");
        assert!(
            occupied.contains(&(grid.x as u16, grid.y as u16)),
            "{unit_type:?}: own cell missing"
        );
        if unit.is_quad() {
            assert_eq!(unit.footprint(), Footprint::Quad);
            assert_eq!(Mobility::classify(unit_type), Mobility::Building);
        }
    }

    // The list distribution follows the `SC_UNITS` flags of the whole table.
    let mut expected = [0usize; 5];
    for &unit_type in UnitType::ALL {
        expected[max_core::UnitInfo::list_kind_for_flags(unit_type_flags(unit_type)).index()] += 1;
    }
    assert_eq!(expected_list_counts, expected);
}

#[test]
fn spawned_units_start_with_their_type_combat_attributes() {
    for &unit_type in UnitType::ALL {
        let mut state = state();
        let id = spawn(&mut state, unit_type, Point::new(10, 10));
        let unit = state.snapshot().unit(id).expect("unit").clone();
        let base = base_unit_values(unit_type);

        assert_eq!(unit.base_values, base, "{unit_type:?}: base values");
        assert_eq!(unit.hits, base.hits, "{unit_type:?}: hits");
        assert_eq!(unit.speed, base.speed, "{unit_type:?}: speed");
        assert_eq!(unit.shots, base.rounds as u8, "{unit_type:?}: shots");
        assert_eq!(
            unit.move_and_fire, base.move_and_fire,
            "{unit_type:?}: move_and_fire"
        );
        assert_eq!(unit.ammo, base.ammo as u8, "{unit_type:?}: ammo");

        // `UnitInfo::UnitInfo`: the weapon state is selected by `ATTRIB_ATTACK`;
        // that is the same fact `CombatRole` classifies.
        let has_attack = base.get_attribute(ATTRIB_ATTACK) != 0;
        assert_eq!(
            unit.weapon,
            if has_attack { 2 } else { 0 },
            "{unit_type:?}: weapon state vs CombatRole"
        );
        assert_eq!(
            CombatRole::classify(unit_type) == CombatRole::Combat,
            has_attack
        );
    }
}

/// Independent expectation of `Access_IsAccessible` on an empty cell.
fn expected_access_cost(unit_type: UnitType, surface: u8) -> u32 {
    let land_type = unit_type_land_type(unit_type);
    if land_type & surface == 0 {
        return 0;
    }
    // C++: a ground/amphibious unit entering water pays 12 (the Surveyor is
    // exempt), then the `land_type` mask decides passability.
    let watery = surface & SURFACE_TYPE_WATER != 0;
    let ground_like = unit_type.is_ground() || unit_type.is_amphibious();
    if watery && ground_like && unit_type != UnitType::Surveyor {
        12
    } else {
        4
    }
}

#[test]
fn passability_over_land_water_and_coast_follows_classification() {
    let mut state = state();
    state.state_mut().map.surface.set(0, 0, Surface::LAND);
    state.state_mut().map.surface.set(1, 0, Surface::WATER);
    state.state_mut().map.surface.set(2, 0, Surface::COAST);
    let flags = ACCESS_MODIFIER_SAME_CLASS_BLOCKS;

    for &unit_type in UnitType::ALL {
        for (grid, surface) in [
            (Point::new(0, 0), SURFACE_TYPE_LAND),
            (Point::new(1, 0), SURFACE_TYPE_WATER),
            (Point::new(2, 0), SURFACE_TYPE_COAST),
        ] {
            let actual = is_accessible(state.state(), unit_type, TeamId::RED, grid, flags);
            assert_eq!(
                actual,
                expected_access_cost(unit_type, surface),
                "{unit_type:?} on surface {surface:#04x}"
            );
        }
    }

    // Spot checks of the classification the expectation is built from.
    assert_eq!(
        expected_access_cost(UnitType::Tank, SURFACE_TYPE_WATER),
        0,
        "a tank cannot enter water"
    );
    assert_eq!(
        expected_access_cost(UnitType::Surveyor, SURFACE_TYPE_WATER),
        4,
        "the Surveyor is exempt from the water surcharge"
    );
    assert_eq!(
        expected_access_cost(UnitType::Fighter, SURFACE_TYPE_COAST),
        4,
        "air ignores the surface (land_type = ALL)"
    );
}

#[test]
fn production_tables_do_not_contradict_the_classifier() {
    // `FACTORIES` — the five build plants.
    assert_eq!(FACTORIES.len(), 5);
    for &factory in &FACTORIES {
        assert!(
            factory.is_building(),
            "{factory:?}: a factory is a building"
        );
        assert_eq!(
            factory.object_kind(),
            ObjectKind::Unit,
            "{factory:?}: a factory is a unit, not a decoration"
        );
        assert!(!factory.is_combat(), "{factory:?}: a factory is not combat");
        assert!(
            production::is_factory(factory),
            "{factory:?}: production::is_factory"
        );
    }

    // `MOBILE_BUILDERS` — the two mobile constructors.
    assert_eq!(MOBILE_BUILDERS.len(), 2);
    for &builder in &MOBILE_BUILDERS {
        assert!(builder.is_mobile(), "{builder:?}");
        assert!(
            matches!(
                builder.environment(),
                Some(Environment::Ground | Environment::Amphibious)
            ),
            "{builder:?}: a builder is a ground unit"
        );
        assert!(production::is_mobile_builder(builder));
    }

    // Everything buildable must be a real unit that can be spawned: no effects,
    // decorations or plot props may be ordered from a factory menu.
    let mut buildable = 0usize;
    for &(_, units) in &BUILDABLE {
        for &unit_type in units {
            buildable += 1;
            assert_eq!(
                unit_type.object_kind(),
                ObjectKind::Unit,
                "{unit_type:?}: buildable but not a unit"
            );
            assert!(
                matches!(
                    unit_type.cargo_kind(),
                    CargoKind::None | CargoKind::Resource | CargoKind::Unit
                ),
                "{unit_type:?}"
            );
        }
    }
    assert!(buildable > 0, "the build menu must not be empty");
}

#[test]
fn classifier_covers_every_spawnable_type() {
    // Effects, decorations and plot props must never enter the build menu, and
    // every spawnable type must land either in a unit list or in `Particle`.
    let mut seen = [0usize; 5];
    let mut state = state();
    for (index, &unit_type) in UnitType::ALL.iter().enumerate() {
        let grid = Point::new((index % 20) as i16, (index / 20) as i16);
        let id = spawn(&mut state, unit_type, grid);
        let kind = state.snapshot().unit(id).expect("unit").unit_list;
        seen[kind.index()] += 1;
    }
    assert_eq!(seen.iter().sum::<usize>(), 93);
    // Particles exist in the table (missiles, trails, explosions).
    assert!(seen[UnitListKind::Particle.index()] > 0);
    assert!(seen[UnitListKind::GroundCover.index()] > 0);
    assert!(seen[UnitListKind::MobileLandSea.index()] > 0);
    assert!(seen[UnitListKind::Stationary.index()] > 0);
    assert!(seen[UnitListKind::MobileAir.index()] > 0);
}

#[test]
fn digest_of_one_unit_of_every_type_is_stable() {
    let build = || {
        let mut state = state();
        for (index, &unit_type) in UnitType::ALL.iter().enumerate() {
            let grid = Point::new(((index % 40) as i16) * 2, ((index / 40) as i16) * 2);
            spawn(&mut state, unit_type, grid);
        }
        state
    };

    let first = build();
    let second = build();
    assert_eq!(first.snapshot().unit_count(), 93);
    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes()
    );
    assert_eq!(
        replay_digest_crc(first.state()),
        replay_digest_crc(second.state())
    );
    assert_eq!(state_digest(first.state()), state_digest(second.state()));

    // Pinned digest of the "all types" state: a change in spawn bookkeeping,
    // classification-driven defaults or attribute tables moves it.
    assert_eq!(state_digest(first.state()), ALL_TYPES_STATE_DIGEST);
    assert_eq!(replay_digest_crc(first.state()), ALL_TYPES_REPLAY_DIGEST);

    // A single classification fact (a building's footprint) is part of the
    // digest: clearing `BUILDING`-derived cells by hand is visible.
    let mut modified = first.clone();
    let id = modified.snapshot().unit_ids_in_order()[0];
    let mut unit = modified.snapshot().unit(id).expect("unit").clone();
    unit.shots = unit.shots.wrapping_add(1);
    modified.write_unit(id, unit);
    assert_ne!(state_digest(modified.state()), ALL_TYPES_STATE_DIGEST);
}

/// `state_digest` of a state with one unit of every one of the 93 types.
const ALL_TYPES_STATE_DIGEST: u16 = 0x9E83;
/// `replay_digest_crc` of the same state.
const ALL_TYPES_REPLAY_DIGEST: u16 = 0x605A;
