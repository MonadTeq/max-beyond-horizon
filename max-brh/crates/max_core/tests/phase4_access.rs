//! Phase 4 — `access.rs` surface/passability integration (I + D layer).
//!
//! The module unit tests already cover the `SC_UNITS` property table, the basic
//! surface rules, bridges/platforms, pins and target selection. This file covers
//! what those tests do not:
//!
//! * every `AccessModifier` flag (`IgnoreSurfaceClass`, `IgnoreVisibility`,
//!   `SameClassBlocks`, `EnemySameClassBlocks`, `MovesUnderBridge`) and their
//!   interaction with visibility/detection and `ORDER_IDLE`;
//! * the stationary-object semantics of `Access_IsAccessible` (road/slab,
//!   rubble, mines, connectors, water platform, buildings, `WALDO`);
//! * air/ground/sea blocking rules, including landed vs hovering aircraft;
//! * map boundaries;
//! * determinism: the accessibility cost field of the two real reference maps is
//!   hashed to a pinned CRC (D layer — terrain + unit + surface logic over
//!   12,544 cells × 4 unit classes).
//!
//! Oracles: `access.cpp` (`Access_IsAccessible`,
//! `Access_GetModifiedSurfaceType`, `Access_GetSquaredDistance`),
//! `unitinfo.cpp` (`IsBridgeElevated`), `SC_UNITS`.

use max_core::access::{
    ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS, ACCESS_MODIFIER_IGNORE_SURFACE_CLASS,
    ACCESS_MODIFIER_IGNORE_VISIBILITY, ACCESS_MODIFIER_MOVES_UNDER_BRIDGE,
    ACCESS_MODIFIER_SAME_CLASS_BLOCKS, SURFACE_TYPE_COAST, SURFACE_TYPE_LAND, SURFACE_TYPE_WATER,
};
use max_core::crc;
use max_core::enums::unit_flags::{
    GROUND_COVER, HOVERING, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, SELECTABLE, STATIONARY,
};
use max_core::enums::{Surface, TeamClan, UnitOrder, UnitType};
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::save::SaveFile;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::GameUniverse;
use max_core::UnitId;

/// `max-original/SAVE1.CAM`.
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM`.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

const SAME: u32 = ACCESS_MODIFIER_SAME_CLASS_BLOCKS;

/// Two human teams, all cells land, fog of war enabled (visibility is an
/// explicit input of each test).
fn game() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.set_network_game(false);
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.teams_mut()[1].team_type = TeamKind::Player;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);
    state.ai_mut().all_visible = false;
    state
}

fn spawn(
    state: &mut GameUniverse,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
    flags: u32,
) -> UnitId {
    state
        .spawn_unit(unit_type, team, grid, flags)
        .unwrap_or_else(|| panic!("spawn {unit_type:?}"))
}

#[test]
fn ignore_surface_class_skips_water_and_occupancy_checks() {
    let mut state = game();
    state.map_mut().surface.set(3, 3, Surface::WATER);
    // A tank cannot enter water (land_type = LAND) ...
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(3, 3),
            0
        ),
        0
    );
    // ... and pays 12 without the modifier only for a passable surface; on water
    // the `land_type` mask wins first.
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(3, 3),
            ACCESS_MODIFIER_IGNORE_SURFACE_CLASS
        ),
        4,
        "IgnoreSurfaceClass must return the base cost"
    );

    // With the modifier the unit loop is skipped entirely, even for a blocker.
    state.ai_mut().all_visible = true;
    spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(4, 4),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(4, 4),
            SAME
        ),
        0
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(4, 4),
            SAME | ACCESS_MODIFIER_IGNORE_SURFACE_CLASS
        ),
        4
    );
}

#[test]
fn same_class_and_enemy_same_class_blocking() {
    let mut state = game();
    let own = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(5, 5),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    let green = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(6, 5),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    // Visibility is an input fact: C++ only considers units visible to the team
    // (or detected stationary ones). The units were spawned with fog of war off;
    // mark the enemy explicitly visible for the red team.
    state
        .unit_mut(green)
        .expect("green tank")
        .set_visible_to_team(TeamId::RED, true);

    // Not idle: the own unit blocks every land path with SameClassBlocks.
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            SAME
        ),
        0
    );
    // The enemy blocks with EnemySameClassBlocks, the own unit does not.
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(6, 5),
            ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS
        ),
        0
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            ACCESS_MODIFIER_ENEMY_SAME_CLASS_BLOCKS
        ),
        4
    );
    // Without any modifier a mobile unit does not block (`AccessModifier_None`).
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            0
        ),
        4
    );
    // An idle unit does not block (`GetOrder() != ORDER_IDLE` in C++).
    state.unit_mut(own).expect("unit").orders = UnitOrder::Idle;
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            SAME
        ),
        4
    );
}

#[test]
fn visibility_and_detection_gate_blocking() {
    let mut state = game();
    // A mine: stationary, invisible to all, detected only after being spotted.
    let mine = spawn(
        &mut state,
        UnitType::LandMine,
        TeamId::GREEN,
        Point::new(7, 7),
        GROUND_COVER | STATIONARY,
    );
    // Invisible + not detected: the mine does not participate in the check.
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(7, 7),
            SAME
        ),
        4,
        "an unknown mine must not block the path planner"
    );
    // Detection (`IsDetectedByTeam`) lets a stationary mine participate.
    state
        .unit_mut(mine)
        .expect("mine")
        .spot_by_team(TeamId::RED);
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(7, 7),
            SAME
        ),
        0
    );
    // The mine's owner is not blocked by its own mine.
    assert_ne!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::GREEN,
            Point::new(7, 7),
            SAME
        ),
        0
    );

    // `IgnoreVisibility` forces a hidden *mobile* enemy into the check.
    let enemy = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(8, 7),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    assert!(!state
        .snapshot()
        .unit(enemy)
        .expect("unit")
        .is_visible_to_team(TeamId::RED));
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(8, 7),
            SAME
        ),
        4,
        "a hidden unit must not block"
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(8, 7),
            SAME | ACCESS_MODIFIER_IGNORE_VISIBILITY
        ),
        0,
        "IgnoreVisibility must let the hidden enemy block"
    );
}

#[test]
fn air_units_block_only_air_and_landed_aircraft_block_ground() {
    let mut state = game();
    state.ai_mut().all_visible = true;
    let fighter = spawn(
        &mut state,
        UnitType::Fighter,
        TeamId::GREEN,
        Point::new(8, 8),
        MOBILE_AIR_UNIT | SELECTABLE,
    );
    let hovering = spawn(
        &mut state,
        UnitType::Fighter,
        TeamId::GREEN,
        Point::new(9, 8),
        MOBILE_AIR_UNIT | SELECTABLE | HOVERING,
    );

    // An air unit blocks an air path with SameClassBlocks.
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Fighter,
            TeamId::RED,
            Point::new(8, 8),
            SAME
        ),
        0
    );
    // A landed aircraft (no HOVERING) blocks a ground path ...
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(8, 8),
            SAME
        ),
        0
    );
    // ... a hovering one does not.
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(9, 8),
            SAME
        ),
        4
    );
    // An air path over a ground unit is not blocked by class.
    let ground = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(10, 8),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Fighter,
            TeamId::RED,
            Point::new(10, 8),
            SAME
        ),
        4
    );
    let _ = (fighter, hovering, ground);
}

#[test]
fn stationary_objects_semantics() {
    let mut state = game();
    state.map_mut().surface.set(20, 5, Surface::WATER);
    state.ai_mut().all_visible = true;

    // ROAD: cost 2 for a ground unit; a second road is not allowed on the cell.
    spawn(
        &mut state,
        UnitType::Road,
        TeamId::RED,
        Point::new(11, 5),
        STATIONARY | GROUND_COVER,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(11, 5),
            SAME
        ),
        2
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Road,
            TeamId::RED,
            Point::new(11, 5),
            SAME
        ),
        0
    );

    // SLAB: cost 2; a road cannot be built over it.
    spawn(
        &mut state,
        UnitType::LargeSlab,
        TeamId::RED,
        Point::new(12, 5),
        STATIONARY | GROUND_COVER,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(12, 5),
            SAME
        ),
        2
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Road,
            TeamId::RED,
            Point::new(12, 5),
            SAME
        ),
        0
    );

    // RUBBLE: the cell becomes land, but the water surcharge computed before the
    // loop stays (`Access_IsAccessible`: `surface_type` is updated, `result` is
    // not) — the C++ quirk is reproduced.
    spawn(
        &mut state,
        UnitType::LargeRubble,
        TeamId::RED,
        Point::new(20, 5),
        STATIONARY | GROUND_COVER,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(20, 5),
            SAME
        ),
        12,
        "rubble over water keeps the pre-loop water surcharge"
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Corvette,
            TeamId::RED,
            Point::new(20, 5),
            SAME
        ),
        0,
        "rubble over water is not sea"
    );

    // WATER PLATFORM: the cell becomes land, cost 4 for a ground unit.
    state.map_mut().surface.set(21, 5, Surface::WATER);
    spawn(
        &mut state,
        UnitType::WaterPlatform,
        TeamId::RED,
        Point::new(21, 5),
        STATIONARY | GROUND_COVER,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(21, 5),
            SAME
        ),
        4
    );

    // A building (BUILDING flag) is impassable for a ground path (C++ default).
    spawn(
        &mut state,
        UnitType::Barracks,
        TeamId::RED,
        Point::new(13, 5),
        STATIONARY | max_core::enums::unit_flags::BUILDING,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(13, 5),
            SAME
        ),
        0
    );
    // An air path ignores the building (the C++ air branch has no default arm).
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Fighter,
            TeamId::RED,
            Point::new(13, 5),
            SAME
        ),
        4
    );

    // `WALDO` is a stationary object without blocking semantics.
    spawn(
        &mut state,
        UnitType::DeadWaldo,
        TeamId::RED,
        Point::new(16, 5),
        STATIONARY | GROUND_COVER,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(16, 5),
            SAME
        ),
        4
    );

    // Connectors: a connector is not placeable on another connector; a foreign
    // connector blocks a stationary (non-connector) structure.
    spawn(
        &mut state,
        UnitType::Connector,
        TeamId::GREEN,
        Point::new(15, 5),
        STATIONARY | max_core::enums::unit_flags::CONNECTOR_UNIT,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Connector,
            TeamId::RED,
            Point::new(15, 5),
            SAME
        ),
        0
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Radar,
            TeamId::RED,
            Point::new(15, 5),
            SAME
        ),
        0,
        "a foreign connector blocks a stationary structure"
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(15, 5),
            0
        ),
        4,
        "a connector does not block ground movement without class blocking"
    );
}

#[test]
fn modified_surface_for_bridges_and_platforms() {
    let mut state = game();
    state.map_mut().surface.set(30, 5, Surface::WATER);
    state.map_mut().surface.set(31, 5, Surface::COAST);
    state.map_mut().surface.set(32, 5, Surface::LAND);

    // No overlay: the raw surface is returned, including coast.
    assert_eq!(
        state.snapshot().modified_surface_type(Point::new(30, 5), false),
        SURFACE_TYPE_WATER
    );
    assert_eq!(
        state.snapshot().modified_surface_type(Point::new(31, 5), true),
        SURFACE_TYPE_COAST
    );
    assert_eq!(
        state.snapshot().modified_surface_type(Point::new(32, 5), false),
        SURFACE_TYPE_LAND
    );

    // A (not elevated) bridge over water: land for a ground unit, water for a
    // naval one.
    let bridge = spawn(
        &mut state,
        UnitType::Bridge,
        TeamId::RED,
        Point::new(30, 5),
        STATIONARY | GROUND_COVER,
    );
    assert!(!max_core::access::is_bridge_elevated(
        state.snapshot().unit(bridge).expect("bridge")
    ));
    assert_eq!(
        state.snapshot().modified_surface_type(Point::new(30, 5), false),
        SURFACE_TYPE_LAND
    );
    assert_eq!(
        state.snapshot().modified_surface_type(Point::new(30, 5), true),
        SURFACE_TYPE_WATER
    );

    // The cost view follows: a tank crosses the bridge for 2, a ship sees water
    // (cost 4) ...
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(30, 5),
            SAME
        ),
        2
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Corvette,
            TeamId::RED,
            Point::new(30, 5),
            SAME
        ),
        4
    );
    // ... except with `MovesUnderBridge`: on a flat bridge C++ makes the cell
    // land for such a unit (`access.cpp:182-186`), so it is blocked.
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Corvette,
            TeamId::RED,
            Point::new(30, 5),
            SAME | ACCESS_MODIFIER_MOVES_UNDER_BRIDGE
        ),
        0,
        "a flat bridge has no clearance"
    );

    // An elevated bridge (`image_index != image_base`) has clearance: the naval
    // unit keeps the water surface with and without the modifier.
    {
        let unit = state.unit_mut(bridge).expect("bridge");
        unit.image_index = unit.image_base.wrapping_add(1);
    }
    assert!(max_core::access::is_bridge_elevated(
        state.snapshot().unit(bridge).expect("bridge")
    ));
    for flags in [SAME, SAME | ACCESS_MODIFIER_MOVES_UNDER_BRIDGE] {
        assert_eq!(
            state.snapshot().is_accessible(UnitType::Corvette,
                TeamId::RED,
                Point::new(30, 5),
                flags
            ),
            4,
            "elevated bridge, flags {flags:#x}"
        );
    }

    // A platform: land from both points of view for the modified surface.
    state.map_mut().surface.set(33, 5, Surface::WATER);
    spawn(
        &mut state,
        UnitType::WaterPlatform,
        TeamId::RED,
        Point::new(33, 5),
        STATIONARY | GROUND_COVER,
    );
    assert_eq!(
        state.snapshot().modified_surface_type(Point::new(33, 5), false),
        SURFACE_TYPE_LAND
    );
    assert_eq!(
        state.snapshot().modified_surface_type(Point::new(33, 5), true),
        SURFACE_TYPE_LAND,
        "the C++ `mode` branch only keeps water for a BRIDGE"
    );
}

#[test]
fn ignoring_units_matches_the_build_placement_trick() {
    let mut state = game();
    state.ai_mut().all_visible = true;
    let tank = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(40, 40),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(40, 40),
            SAME
        ),
        0
    );
    // `UnitsManager_IssueBuildOrder` removes the builder from the hash before
    // the site check; the ignoring variant reproduces that without mutating.
    assert_eq!(
        state.snapshot().is_accessible_ignoring(UnitType::Tank,
            TeamId::RED,
            Point::new(40, 40),
            SAME,
            &[tank]
        ),
        4
    );
    // Ignoring an unrelated id changes nothing.
    assert_eq!(
        state.snapshot().is_accessible_ignoring(UnitType::Tank,
            TeamId::RED,
            Point::new(40, 40),
            SAME,
            &[UnitId::NO_ID]
        ),
        0
    );
}

#[test]
fn map_boundaries() {
    let state = game();
    let flags = 0u32;
    for grid in [
        Point::new(-1, 0),
        Point::new(0, -1),
        Point::new(MAP_WIDTH as i16, 0),
        Point::new(0, MAP_HEIGHT as i16),
        Point::new(-1, -1),
    ] {
        assert_eq!(
            state.snapshot().is_accessible(UnitType::Tank, TeamId::RED, grid, flags),
            0,
            "off-map cell {grid:?}"
        );
        assert_eq!(
            state.snapshot().surface_type(grid),
            0,
            "off-map surface {grid:?}"
        );
    }
    // The last valid cell is accessible and has a surface.
    let last = Point::new(MAP_WIDTH as i16 - 1, MAP_HEIGHT as i16 - 1);
    assert_eq!(state.snapshot().surface_type(last), SURFACE_TYPE_LAND);
    assert_eq!(
        state.snapshot().is_accessible(UnitType::Tank, TeamId::RED, last, flags),
        4
    );
}

/// Builds a per-cell cost field for one unit type and hashes it.
fn costs_crc(state: &GameUniverse, unit_type: UnitType, flags: u32) -> u16 {
    let mut bytes = Vec::with_capacity((MAP_WIDTH * MAP_HEIGHT) as usize);
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let cost = state.snapshot().is_accessible(unit_type,
                TeamId::RED,
                Point::new(x as i16, y as i16),
                flags,
            );
            bytes.push(cost as u8);
        }
    }
    assert_eq!(bytes.len(), 12_544);
    crc::crc16(&bytes)
}

#[test]
fn cost_field_of_reference_maps_is_pinned() {
    for (source, tag, expected) in [
        (SAVE1, "SAVE1", SAVE1_COST_FIELD),
        (SAVE2, "SAVE2", SAVE2_COST_FIELD),
    ] {
        let state =
            GameUniverse::from_save(&SaveFile::parse(source).expect("reference save parses"));
        // One ground, one naval, one air and one amphibious class with the
        // production pathfinding modifier (`AccessModifier_SameClassBlocks`).
        let mut combined = Vec::new();
        for unit_type in [
            UnitType::Tank,
            UnitType::Corvette,
            UnitType::Fighter,
            UnitType::Scout,
        ] {
            let field = costs_crc(&state, unit_type, SAME);
            combined.push(field);
        }
        assert_eq!(
            (combined[0], combined[1], combined[2], combined[3]),
            expected,
            "{tag}: accessibility cost field changed"
        );
    }
}

/// `(Tank, Corvette, Fighter, Scout)` cost-field CRC of `SAVE1.CAM`.
const SAVE1_COST_FIELD: (u16, u16, u16, u16) = (0xBDA7, 0xAA3E, 0x86A4, 0x8B4E);
/// The same for `SAVE2.CAM`.
const SAVE2_COST_FIELD: (u16, u16, u16, u16) = (0x17D2, 0x4E6A, 0x718A, 0x8D1F);

#[test]
fn fresh_state_surface_defaults_to_land() {
    // `GameUniverse::new` fills the map with `LAND` so that unit tests can move;
    // the real map replaces it entirely (`setup::generate_map`).
    let state = game();
    for (x, y) in [
        (0, 0),
        (50, 50),
        (MAP_WIDTH as i16 - 1, MAP_HEIGHT as i16 - 1),
    ] {
        assert_eq!(state.snapshot().map().surface_at(x, y), Surface::LAND);
    }
    assert!(
        state.snapshot().is_accessible(UnitType::Tank,
            TeamId::RED,
            Point::new(50, 50),
            SAME
        ) > 0
    );
}
