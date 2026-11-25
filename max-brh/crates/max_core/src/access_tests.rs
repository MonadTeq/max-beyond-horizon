use super::*;
use crate::enums::unit_flags::{
    ELECTRONIC_UNIT, GROUND_COVER, MOBILE_LAND_UNIT, MOBILE_SEA_UNIT, SELECTABLE,
};
use crate::enums::{Surface, TeamClan};
use crate::team::TeamKind;

fn state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(
    state: &mut GameState,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
    flags: u32,
) -> UnitId {
    state
        .spawn_unit(unit_type, team, grid, flags)
        .expect("spawn")
}

#[test]
fn unit_type_table_matches_resource_units() {
    assert_eq!(UNIT_TYPE_PROPERTIES.len(), 93);
    assert_eq!(unit_type_flags(UnitType::GoldRefinery), 0x0160_4210);
    assert_eq!(
        unit_type_land_type(UnitType::GoldRefinery),
        SURFACE_TYPE_LAND
    );
    assert_eq!(unit_type_land_type(UnitType::Fighter), SURFACE_TYPE_ALL);
    assert_eq!(unit_type_land_type(UnitType::ArmouredPersonnelCarrier), 0x7);
    assert_eq!(
        unit_type_flags(UnitType::Tank) & MOBILE_LAND_UNIT,
        MOBILE_LAND_UNIT
    );
    assert_eq!(
        unit_type_flags(UnitType::GoldRefinery) & ELECTRONIC_UNIT,
        ELECTRONIC_UNIT
    );
    assert_eq!(
        unit_type_flags(UnitType::Rocket) & crate::enums::unit_flags::MISSILE_UNIT,
        crate::enums::unit_flags::MISSILE_UNIT
    );
}

#[test]
fn surface_rules_follow_land_type() {
    let mut state = state();
    let same = ACCESS_MODIFIER_SAME_CLASS_BLOCKS;

    // Land: a ground unit passes.
    assert_eq!(
        is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(5, 5), same),
        4
    );

    // Water: a tank — no, the surveyor (LAND|WATER|COAST) — yes.
    state.map.surface.set(6, 5, Surface::WATER);
    assert_eq!(
        is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(6, 5), same),
        0
    );
    assert_eq!(
        is_accessible(
            &state,
            UnitType::Surveyor,
            TeamId::RED,
            Point::new(6, 5),
            same
        ),
        4
    );
    // An air unit (land_type = ALL) passes over water.
    assert_eq!(
        is_accessible(
            &state,
            UnitType::Fighter,
            TeamId::RED,
            Point::new(6, 5),
            same
        ),
        4
    );
    // A ship on land — no, the CLNTRANS amphibian (0x7) — yes.
    assert_eq!(
        is_accessible(
            &state,
            UnitType::Corvette,
            TeamId::RED,
            Point::new(5, 6),
            same
        ),
        0
    );
    assert_eq!(
        is_accessible(
            &state,
            UnitType::ArmouredPersonnelCarrier,
            TeamId::RED,
            Point::new(5, 6),
            same
        ),
        4
    );

    // Off the map — 0.
    assert_eq!(
        is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(-1, 0), same),
        0
    );
    assert_eq!(
        is_accessible(
            &state,
            UnitType::Tank,
            TeamId::RED,
            Point::new(112, 0),
            same
        ),
        0
    );
}

#[test]
fn stationary_units_change_cost_and_block() {
    let mut state = state();
    let same = ACCESS_MODIFIER_SAME_CLASS_BLOCKS;

    // Slab: driving over it is cheaper (2).
    spawn(
        &mut state,
        UnitType::SmallSlab,
        TeamId::RED,
        Point::new(7, 5),
        STATIONARY | GROUND_COVER,
    );
    assert_eq!(
        is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(7, 5), same),
        2
    );
    // Road: cheaper for wheeled movement, a second road is not allowed.
    spawn(
        &mut state,
        UnitType::Road,
        TeamId::RED,
        Point::new(8, 5),
        STATIONARY | GROUND_COVER,
    );
    assert_eq!(
        is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(8, 5), same),
        2
    );
    assert_eq!(
        is_accessible(&state, UnitType::Road, TeamId::RED, Point::new(8, 5), same),
        0
    );

    // A detected enemy mine blocks the cell.
    let mine = spawn(
        &mut state,
        UnitType::LandMine,
        TeamId::GREEN,
        Point::new(9, 5),
        crate::enums::unit_flags::GROUND_COVER | STATIONARY,
    );
    {
        let unit = state.unit_mut(mine).unwrap();
        unit.spot_by_team(TeamId::RED);
    }
    assert_eq!(
        is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(9, 5), same),
        0
    );
    // An own mine does not block.
    assert_ne!(
        is_accessible(
            &state,
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(9, 5),
            same
        ),
        0
    );
}

#[test]
fn bridge_and_platform_modify_surface() {
    let mut state = state();
    let same = ACCESS_MODIFIER_SAME_CLASS_BLOCKS;
    state.map.surface.set(10, 5, Surface::WATER);
    spawn(
        &mut state,
        UnitType::Bridge,
        TeamId::RED,
        Point::new(10, 5),
        STATIONARY | GROUND_COVER,
    );

    // Ground: across the bridge (2), the surface is land.
    assert_eq!(
        is_accessible(&state, UnitType::Tank, TeamId::RED, Point::new(10, 5), same),
        2
    );
    assert_eq!(
        modified_surface_type(&state, Point::new(10, 5), false),
        SURFACE_TYPE_LAND
    );
    // Naval: the bridge is water, cost 4.
    assert_eq!(
        is_accessible(
            &state,
            UnitType::Corvette,
            TeamId::RED,
            Point::new(10, 5),
            same
        ),
        4
    );
    assert_eq!(
        modified_surface_type(&state, Point::new(10, 5), true),
        SURFACE_TYPE_WATER
    );

    // `Access_GetBridge` (`access.cpp:1533`): the bridge on the cell; a naval
    // unit released there raises it (`units_manager.cpp:3891-3897`).
    let bridge = get_bridge(&state, Point::new(10, 5)).expect("bridge on the cell");
    assert_eq!(
        state.unit(bridge).map(|unit| unit.unit_type),
        Some(UnitType::Bridge)
    );
    assert_eq!(get_bridge(&state, Point::new(5, 5)), None);
    assert_eq!(get_bridge(&state, Point::new(-1, 0)), None);
}

#[test]
fn pins_and_infiltrator_make_unit_engaged() {
    let mut state = state();
    let tank = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(5, 5),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    let mut unit = state.unit(tank).unwrap().clone();
    assert!(!is_engaged(&state, &unit));

    unit.pin_count = 1;
    assert!(is_engaged(&state, &unit));
    assert!(are_there_pins(&unit));

    unit.pin_count = 0;
    unit.orders = UnitOrder::Await;
    state.write_unit(tank, unit);

    // A saboteur "holding" the target blocks it too.
    let commando = spawn(
        &mut state,
        UnitType::Infiltrator,
        TeamId::GREEN,
        Point::new(5, 6),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    {
        let unit = state.unit_mut(commando).unwrap();
        unit.parent_unit = Some(tank);
        unit.orders = UnitOrder::AwaitStealUnit;
        unit.state = UnitOrderState::InProgress;
    }
    let tank_unit = state.unit(tank).unwrap().clone();
    assert!(is_engaged(&state, &tank_unit));
    assert!(is_engaged_by_infiltrator(&state, &tank_unit));
}

#[test]
fn receiver_unit_accepts_commando() {
    let mut state = state();
    let barracks = spawn(
        &mut state,
        UnitType::Barracks,
        TeamId::RED,
        Point::new(5, 5),
        STATIONARY | crate::enums::unit_flags::BUILDING,
    );
    {
        let unit = state.unit_mut(barracks).unwrap();
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::Init;
    }
    let commando = state
        .spawn_unit(
            UnitType::Infiltrator,
            TeamId::RED,
            Point::new(5, 6),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    let unit = state.unit(commando).unwrap().clone();
    assert_eq!(
        get_receiver_unit(&state, &unit, Point::new(5, 5)),
        Some(barracks)
    );
    assert!(can_receive_units(state.unit(barracks).unwrap()));

    // A tank does not enter the barracks.
    let tank = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(5, 7),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    let tank_unit = state.unit(tank).unwrap().clone();
    assert_eq!(
        get_receiver_unit(&state, &tank_unit, Point::new(5, 5)),
        None
    );
}

#[test]
fn attack_target_selection_respects_state_and_flags() {
    let mut state = state();
    let attacker = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(5, 5),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    let defender = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(5, 6),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    // The target is visible to the attacker (in game — after scanning/reconnaissance).
    state.update_visibility_status(false);
    {
        let unit = state.unit_mut(defender).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    let attacker_unit = state.unit(attacker).unwrap().clone();
    assert_eq!(
        get_attack_target(&state, &attacker_unit, Point::new(5, 6), false),
        Some(defender)
    );

    // An IDLE unit is not a target.
    state.unit_mut(defender).unwrap().orders = UnitOrder::Idle;
    assert_eq!(
        get_attack_target(&state, &attacker_unit, Point::new(5, 6), false),
        None
    );

    // An empty/off-map cell.
    assert_eq!(
        get_attack_target(&state, &attacker_unit, Point::new(9, 9), false),
        None
    );
    assert_eq!(
        get_attack_target(&state, &attacker_unit, Point::new(-1, 5), false),
        None
    );
}

#[test]
fn group_order_is_dissolved_when_members_are_cleared() {
    let mut state = state();
    let leader = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(5, 5),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    let wing = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(6, 5),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    assign_unit_group(&mut state, leader, &[wing]);
    assert_eq!(state.group_members(leader), Some(vec![leader, wing]));

    clear_unit_group(&mut state, wing);
    assert_eq!(state.group_members(leader), Some(vec![leader]));
    clear_unit_group(&mut state, leader);
    assert_eq!(state.group_members(leader), None);
}

#[test]
fn team_unit_by_environments_is_bit_equivalent_to_raw_mobile_masks() {
    let mut state = state();

    // One unit per `SC_UNITS` environment; `spawn_unit` adds the type's
    // flags, so the environment bits of the instances are present.
    let ground = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5), 0);
    let amphibious = spawn(
        &mut state,
        UnitType::Scout,
        TeamId::RED,
        Point::new(6, 5),
        0,
    );
    let air = spawn(
        &mut state,
        UnitType::Fighter,
        TeamId::RED,
        Point::new(7, 5),
        0,
    );
    let sea = spawn(
        &mut state,
        UnitType::Corvette,
        TeamId::RED,
        Point::new(8, 5),
        0,
    );

    // `Access_GetTeamUnit` skips `ORDER_IDLE`.
    for id in [ground, amphibious, air, sea] {
        state.unit_mut(id).expect("unit").orders = UnitOrder::Await;
    }

    let grids = [
        Point::new(5, 5),
        Point::new(6, 5),
        Point::new(7, 5),
        Point::new(8, 5),
        Point::new(9, 5),
    ];

    // Each typed filter over the whole bit band is exactly its raw
    // C++ bit (`SC_UNITS` flags are consistent: without `MOBILE_AIR` together with
    // `MOBILE_LAND/SEA`).
    for (environments, mask) in [
        (
            &[Environment::Ground, Environment::Amphibious][..],
            MOBILE_LAND_UNIT,
        ),
        (&[Environment::Air][..], MOBILE_AIR_UNIT),
        (
            &[Environment::Sea, Environment::Amphibious][..],
            MOBILE_SEA_UNIT,
        ),
        (&[][..], 0),
    ] {
        for grid in grids {
            assert_eq!(
                get_team_unit_by_environments(&state, grid, TeamId::RED, environments),
                get_team_unit(&state, grid, TeamId::RED, mask),
                    "{environments:?} / {mask:#05X} / ({}, {})",
                grid.x,
                grid.y
            );
        }
    }

    let found = |grid: Point, environments: &[Environment]| {
        get_team_unit_by_environments(&state, grid, TeamId::RED, environments)
    };

    // The `MOBILE_LAND_UNIT` bit includes amphibians, while `Environment::Ground` —
    // only pure ground units.
    assert_eq!(
        found(Point::new(5, 5), &[Environment::Ground]),
        Some(ground)
    );
    assert_eq!(found(Point::new(6, 5), &[Environment::Ground]), None);
    assert_eq!(found(Point::new(6, 5), &[Environment::Sea]), None);
    assert_eq!(
        found(
            Point::new(6, 5),
            &[Environment::Ground, Environment::Amphibious],
        ),
        Some(amphibious)
    );

    // Air and sea have their own bits without expansion.
    assert_eq!(found(Point::new(7, 5), &[Environment::Air]), Some(air));
    assert_eq!(found(Point::new(8, 5), &[Environment::Sea]), Some(sea));
    assert_eq!(found(Point::new(8, 5), &[Environment::Air]), None);
}

/// `Access_GetApproximateDistance` / `Access_GetSquaredDistance` boundaries.
#[test]
fn distance_helpers_boundaries() {
    assert_eq!(approximate_distance(0, 0), 0);
    assert_eq!(approximate_distance(1, 0), 2);
    assert_eq!(approximate_distance(0, 1), 2);
    assert_eq!(approximate_distance(3, 4), 11);
    assert_eq!(approximate_distance(-3, -4), 11);
    assert_eq!(approximate_distance(-5, 2), 12);
    assert_eq!(approximate_distance(2, -5), 12);
    assert_eq!(approximate_distance_points(Point::new(10, 10), Point::new(10, 10)), 0);
    assert_eq!(
        approximate_distance_points(Point::new(10, 10), Point::new(13, 14)),
        11
    );

    assert_eq!(squared_distance(Point::new(0, 0), Point::new(0, 0)), 0);
    assert_eq!(squared_distance(Point::new(0, 0), Point::new(1, 0)), 1);
    assert_eq!(squared_distance(Point::new(0, 0), Point::new(3, 4)), 25);
    assert_eq!(squared_distance(Point::new(5, 5), Point::new(5, 5)), 0);
    // The maximum map diagonal (111,111) still fits in `i32`.
    assert_eq!(
        squared_distance(Point::new(0, 0), Point::new(111, 111)),
        111 * 111 + 111 * 111
    );
}

/// `Access_IsWithinAttackRange` boundaries and the line-of-fire rule for
/// submarines/corvettes.
#[test]
fn attack_range_boundaries() {
    let mut state = state();
    let tank = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::RED,
        Point::new(10, 10),
        MOBILE_LAND_UNIT | SELECTABLE,
    );
    let unit = state.unit(tank).expect("tank").clone();
    // The exact edge is inside (`distance > attack_range` rejects).
    assert!(is_within_attack_range(&state, &unit, Point::new(13, 10), 3));
    assert!(!is_within_attack_range(&state, &unit, Point::new(14, 10), 3));
    assert!(is_within_attack_range(&state, &unit, Point::new(12, 12), 3));
    assert!(!is_within_attack_range(&state, &unit, Point::new(12, 12), 2));
    // Range 0 — only the own cell.
    assert!(is_within_attack_range(&state, &unit, Point::new(10, 10), 0));
    assert!(!is_within_attack_range(&state, &unit, Point::new(11, 10), 0));

    // A submarine may not fire across land; the same cell distance is fine
    // over water.
    let submarine = spawn(
        &mut state,
        UnitType::Submarine,
        TeamId::RED,
        Point::new(20, 20),
        MOBILE_SEA_UNIT | SELECTABLE,
    );
    let submarine = state.unit(submarine).expect("submarine").clone();
    assert!(
        !is_within_attack_range(&state, &submarine, Point::new(23, 20), 3),
            "a land cell blocks the line of fire"
    );
    for y in 18..=22 {
        for x in 18..=22 {
            state.map.surface.set(x, y, Surface::WATER);
        }
    }
    assert!(
        is_within_attack_range(&state, &submarine, Point::new(23, 20), 3),
            "over water the line of fire is clear"
    );
    // A coast cell is allowed as well (`SURFACE_TYPE_COAST`).
    state.map.surface.set(21, 20, Surface::COAST);
    assert!(is_within_attack_range(&state, &submarine, Point::new(23, 20), 3));
}

/// `UnitsManager_DisableUnit` — the saboteur formula boundaries.
#[test]
fn disable_turns_boundaries() {
    let mut infiltrator =
        UnitInfo::new(UnitType::Infiltrator, TeamId::RED, UnitId::NO_ID);
    infiltrator.experience = 0;

    // `ATTRIB_TURNS == 0` is unreachable for game types; C++ would divide by
    // zero, the port returns 1.
    let mut target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId::NO_ID);
    target.base_values.turns = 0;
    assert_eq!(disable_turns(&infiltrator, &target), 1);

    // A stationary target (`ATTRIB_SPEED == 0`) uses the other branch.
    target.base_values.turns = 10;
    target.base_values.speed = 0;
    assert!(disable_turns(&infiltrator, &target) >= 1);

    // A moving target: the result is clamped to at least one turn.
    target.base_values.speed = 5;
    target.base_values.turns = 4;
    target.speed = 20;
    assert_eq!(disable_turns(&infiltrator, &target), 1);
    // More experience never shortens the disable below one turn.
    for experience in [0i16, 1, 4, 8, 16, 100, 1000] {
        infiltrator.experience = experience;
        assert!(disable_turns(&infiltrator, &target) >= 1, "experience {experience}");
    }
}

/// `Access_GetModifiedSurfaceType` returns the raw byte when no overlay is
/// present — including `AIR` and `NONE`.
#[test]
fn modified_surface_without_overlay() {
    let mut state = state();
    for (grid, value, expected) in [
        (Point::new(0, 0), Surface::NONE, 0u8),
        (Point::new(1, 0), Surface::LAND, SURFACE_TYPE_LAND),
        (Point::new(2, 0), Surface::WATER, SURFACE_TYPE_WATER),
        (Point::new(3, 0), Surface::COAST, SURFACE_TYPE_COAST),
        (Point::new(4, 0), Surface::AIR, crate::access::SURFACE_TYPE_AIR),
    ] {
        state.map.surface.set(grid.x, grid.y, value);
        assert_eq!(surface_type(&state, grid), expected);
        assert_eq!(modified_surface_type(&state, grid, false), expected);
        assert_eq!(modified_surface_type(&state, grid, true), expected);
    }
    // Off-map: `NONE`.
    assert_eq!(modified_surface_type(&state, Point::new(-1, -1), false), 0);
}

/// Invariant C3 (concrete scenario): a `SUBMARNE` is classified as
/// `TARGET_CLASS_SEA_STEALTH` until a team actually sees it; only then does
/// it become an ordinary water target.
///
/// Oracle: `max-cpp-port/src/access.cpp:724-760` (`Access_GetTargetClass`).
#[test]
fn invariant_c3_submarine_target_class_is_sea_stealth_until_spotted() {
    use crate::enums::unit_flags::{MOBILE_SEA_UNIT, SELECTABLE};

    let mut state = state();
    let submarine = spawn(
        &mut state,
        UnitType::Submarine,
        TeamId::GREEN,
        Point::new(5, 5),
        MOBILE_SEA_UNIT | SELECTABLE,
    );

    assert_eq!(
        target_class(&state, state.unit(submarine).unwrap()),
        TARGET_CLASS_SEA_STEALTH,
            "C3: an unseen submarine must be classified as sea stealth"
    );

    crate::combat::spot_unit(&mut state, submarine, TeamId::RED);

    assert_eq!(
        target_class(&state, state.unit(submarine).unwrap()),
        TARGET_CLASS_WATER,
            "C3: a spotted submarine must become an ordinary water target"
    );
}
