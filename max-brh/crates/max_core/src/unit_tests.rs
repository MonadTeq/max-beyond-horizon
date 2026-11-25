use super::*;

#[test]
fn unit_id_bitfield_matches_cpp() {
    let id = UnitId::new(TeamId(2), 0x034);
    assert_eq!(id.team(), TeamId(2));
    assert_eq!(id.object_index(), 0x034);
    assert_eq!(id.0, (2 << 13) | 0x034);
    // The low 13 bits are the object index: higher bits are discarded.
    assert_eq!(UnitId::new(TeamId(2), 0x1234).object_index(), 0x1234);
    assert!(UnitId::NO_ID.is_no_id());
    assert_eq!(UnitId::from_v70(0xFFFF), UnitId::NO_ID);
    assert_eq!(UnitId::new(TeamId::ALIEN, 7).to_v70(), 0x8007);
}

#[test]
fn unit_info_defaults_match_cpp() {
    let unit = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId::new(TeamId::GREEN, 1));
    assert_eq!(unit.orders, UnitOrder::Await);
    // `UnitInfo::UnitInfo` (`unitinfo.cpp:201-204`): `ORDER_AWAIT`/
    // `ORDER_STATE_EXECUTING_ORDER`, prior is the same.
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert_eq!(unit.prior_orders, UnitOrder::Await);
    assert_eq!(unit.prior_state, UnitOrderState::ExecutingOrder);
    // `brightness = 255` from the C++ constructor is now in `UnitPresentation`
    // (F5.3/B) — checked in `max_types::unit::tests`.
    assert_eq!(unit.grid, Point::new(-1, -1));
    assert_eq!(unit.group_speed, 0);
    assert_eq!(unit.live_attack, 0);
    assert!(unit.tasks_enabled);
    assert_eq!(unit.build_rate, 1);
    assert_eq!(
        unit.base_values,
        crate::unit_values::base_unit_values(UnitType::Tank)
    );
    assert!(!unit.has_move_order());
    unit.visible_to_team
        .iter()
        .for_each(|visible| assert!(!visible));
}

#[test]
fn visibility_and_detection_per_team() {
    let mut unit = UnitInfo::new(UnitType::Scout, TeamId::RED, UnitId::new(TeamId::RED, 2));
    assert!(!unit.is_visible_to_team(TeamId::RED));
    unit.set_visible_to_team(TeamId::RED, true);
    assert!(unit.is_visible_to_team(TeamId::RED));
    assert!(!unit.is_visible_to_team(TeamId::BLUE));
    assert!(!unit.is_detected_by_team(TeamId::RED));
    unit.spot_by_team(TeamId::RED);
    assert!(unit.is_detected_by_team(TeamId::RED));
}

#[test]
fn complex_membership_matches_cpp_flags() {
    use crate::enums::unit_flags;
    let mut unit = UnitInfo::new(
        UnitType::PowerGenerator,
        TeamId::RED,
        UnitId::new(TeamId::RED, 5),
    );
    unit.flags = unit_flags::BUILDING;
    assert!(unit.is_complex_member());
    unit.flags |= unit_flags::GROUND_COVER;
    assert!(!unit.is_complex_member());
    unit.flags = unit_flags::STANDALONE;
    assert!(unit.is_complex_member());
    unit.id = UnitId::NO_ID;
    assert!(!unit.is_complex_member());
}

#[test]
fn list_kinds_follow_cpp_order() {
    use crate::enums::unit_flags;
    assert_eq!(UnitListKind::ALL.len(), 5);
    assert_eq!(UnitListKind::GroundCover.index(), 0);
    assert_eq!(UnitListKind::from_index(4), Some(UnitListKind::Particle));
    assert_eq!(UnitListKind::from_index(5), None);
    assert_eq!(
        UnitInfo::list_kind_for_flags(unit_flags::GROUND_COVER),
        UnitListKind::GroundCover
    );
    assert_eq!(
        UnitInfo::list_kind_for_flags(unit_flags::MOBILE_AIR_UNIT),
        UnitListKind::MobileAir
    );
    assert_eq!(
        UnitInfo::list_kind_for_flags(unit_flags::MOBILE_SEA_UNIT),
        UnitListKind::MobileLandSea
    );
    assert_eq!(
        UnitInfo::list_kind_for_flags(unit_flags::STATIONARY),
        UnitListKind::Stationary
    );
    assert_eq!(UnitInfo::list_kind_for_flags(0), UnitListKind::Particle);
}

#[test]
fn ground_path_tracks_remaining_steps() {
    let path = UnitPath::Ground(GroundPath {
        end_x: 10,
        end_y: 12,
        step_index: 1,
        steps: vec![
            PathStep::new(1, 1),
            PathStep::new(2, 2),
            PathStep::new(3, 3),
        ],
    });
    assert_eq!(path.end(), (10, 12));
    assert_eq!(path.remaining_steps(), 2);
    assert_eq!(UnitPath::Builder(BuilderPath::default()).end(), (1, 1));
}

/// `UnitPath::remaining_steps` is saturated: a stale `step_index` past the
/// end (possible after a save/normalization) reports zero, never underflows.
/// Air and builder paths have no step list.
#[test]
fn unit_path_remaining_steps_boundaries() {
    let steps = vec![PathStep::new(1, 0), PathStep::new(0, 1)];

    for (step_index, expected) in [(0, 2), (1, 1), (2, 0), (3, 0), (u32::MAX, 0)] {
        let path = UnitPath::Ground(GroundPath {
            end_x: 1,
            end_y: 1,
            step_index,
            steps: steps.clone(),
        });
        assert_eq!(path.remaining_steps(), expected, "step_index {step_index}");
    }

    let empty = UnitPath::Ground(GroundPath {
        end_x: 0,
        end_y: 0,
        step_index: 0,
        steps: Vec::new(),
    });
    assert_eq!(empty.remaining_steps(), 0);

    assert_eq!(UnitPath::Air(AirPath::default()).remaining_steps(), 0);
    assert_eq!(UnitPath::Air(AirPath::default()).end(), (0, 0));
    assert_eq!(
        UnitPath::Builder(BuilderPath::default()).remaining_steps(),
        0
    );
}

/// `UnitInfo::new` takes the frame header from [`crate::frames::frame_info`] and
/// starts with no orders progress, no ammo/fuel and the C++ "attached unit"
/// flags (state-model §3.9, `unitinfo.cpp:276-294`).
#[test]
fn unit_new_initializes_frames_and_readiness_defaults() {
    let frames = crate::frames::frame_info(UnitType::Tank);
    let unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::new(TeamId::RED, 1));

    // Frame header.
    assert_eq!(unit.image_base, frames.image_base);
    assert_eq!(unit.turret_image_base, frames.turret_image_base);
    assert_eq!(unit.image_index, frames.image_base);
    assert_eq!(unit.turret_image_index, frames.turret_image_base);
    assert_eq!(
        unit.image_index_max,
        crate::frames::image_index_max(UnitType::Tank, 0)
    );

    // Ammo/fuel/readiness: a bare `UnitInfo` has no combat resources yet;
    // `spawn_unit` fills them from `SC_ATTRI`.
    assert_eq!(unit.ammo, 0);
    assert_eq!(unit.shots, 0);
    assert_eq!(unit.fuel_distance, 0);
    assert_eq!(unit.storage, 0);
    assert_eq!(unit.experience, 0);
    assert_eq!(unit.hits, 0);
    assert_eq!(unit.speed, 0);
    assert_eq!(unit.moved, 0);
    assert_eq!(unit.movement_wait_count, 0);
    assert_eq!(unit.disabled_turns_remaining, 0);
    assert!(!unit.damaged_this_turn);
    assert!(!unit.disabled_reaction_fire);
    assert!(!unit.build_paused);
    assert_eq!(unit.unit_list, UnitListKind::Stationary);
    assert!(unit.path.is_none());
    assert!(unit.build_list.is_empty());
}

/// `UnitInfo::HasMoveOrder` covers exactly the three movement orders.
#[test]
fn has_move_order_covers_move_variants_only() {
    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::new(TeamId::RED, 1));

    for order in [
        UnitOrder::Move,
        UnitOrder::MoveToUnit,
        UnitOrder::MoveToAttack,
    ] {
        unit.orders = order;
        assert!(unit.has_move_order(), "{order:?}");
    }

    for order in [
        UnitOrder::Await,
        UnitOrder::Fire,
        UnitOrder::Build,
        UnitOrder::Sentry,
        UnitOrder::Idle,
        UnitOrder::Explode,
        UnitOrder::Order17,
    ] {
        unit.orders = order;
        assert!(!unit.has_move_order(), "{order:?}");
    }
}

/// Team-indexed visibility is bounds-checked: teams outside `0..TEAM_SLOTS`
/// read as invisible and writes are ignored (no panic, no wraparound).
#[test]
fn visibility_access_outside_team_slots_is_guarded() {
    let mut unit = UnitInfo::new(UnitType::Scout, TeamId::RED, UnitId::new(TeamId::RED, 2));

    for raw in 0..=4u8 {
        unit.set_visible_to_team(TeamId(raw), true);
        unit.spot_by_team(TeamId(raw));
        assert!(unit.is_visible_to_team(TeamId(raw)), "team {raw}");
        assert!(unit.is_detected_by_team(TeamId(raw)), "team {raw}");
    }

    for raw in [5u8, 6, u8::MAX] {
        unit.set_visible_to_team(TeamId(raw), true);
        unit.spot_by_team(TeamId(raw));
        assert!(!unit.is_visible_to_team(TeamId(raw)), "team {raw}");
        assert!(!unit.is_detected_by_team(TeamId(raw)), "team {raw}");
    }

    assert_eq!(unit.visible_to_team, [true; 5]);
    assert_eq!(unit.spotted_by_team, [true; 5]);
}

/// `UnitInfo::GetUnitList` classification priority is preserved:
/// ground cover → air → land/sea → stationary → particle.
#[test]
fn list_kind_priority_matches_cpp_allocation_order() {
    use crate::enums::unit_flags::{
        GROUND_COVER, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, MOBILE_SEA_UNIT, STATIONARY,
    };

    assert_eq!(
        UnitInfo::list_kind_for_flags(GROUND_COVER | MOBILE_AIR_UNIT | STATIONARY),
        UnitListKind::GroundCover
    );
    assert_eq!(
        UnitInfo::list_kind_for_flags(MOBILE_AIR_UNIT | MOBILE_LAND_UNIT),
        UnitListKind::MobileAir
    );
    assert_eq!(
        UnitInfo::list_kind_for_flags(MOBILE_LAND_UNIT | STATIONARY),
        UnitListKind::MobileLandSea
    );
    assert_eq!(
        UnitInfo::list_kind_for_flags(MOBILE_SEA_UNIT),
        UnitListKind::MobileLandSea
    );
    assert_eq!(
        UnitInfo::list_kind_for_flags(STATIONARY),
        UnitListKind::Stationary
    );
    assert_eq!(UnitInfo::list_kind_for_flags(0), UnitListKind::Particle);
    assert_eq!(
        UnitInfo::list_kind_for_flags(crate::enums::unit_flags::ANIMATED),
        UnitListKind::Particle
    );
}

/// `is_complex_member` requires a real id, one of the three structural flags
/// and absence of `GROUND_COVER` (`UnitInfo::IsComplexMember`, `unitinfo.hpp:255`).
#[test]
fn complex_membership_boundaries() {
    use crate::enums::unit_flags::{BUILDING, CONNECTOR_UNIT, GROUND_COVER, STANDALONE};

    let mut unit = UnitInfo::new(
        UnitType::PowerStation,
        TeamId::RED,
        UnitId::new(TeamId::RED, 9),
    );
    assert!(!unit.is_complex_member(), "no structural flag");

    for flag in [BUILDING, CONNECTOR_UNIT, STANDALONE] {
        unit.flags = flag;
        assert!(unit.is_complex_member(), "{flag:#x}");
    }

    unit.flags = BUILDING | CONNECTOR_UNIT | STANDALONE | GROUND_COVER;
    assert!(!unit.is_complex_member(), "ground cover is not a member");

    unit.flags = BUILDING;
    unit.id = UnitId::NO_ID;
    assert!(!unit.is_complex_member(), "NO_ID is not a member");

    // A combat vehicle carries no structural flag.
    let tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::new(TeamId::RED, 10));
    assert!(!tank.is_complex_member());

    // `SC_UNITS` marks fixed turrets `STANDALONE`; C++ `IsComplexMember` therefore
    // counts them as members (only `GROUND_COVER` and `NO_ID` are excluded).
    let mut turret = UnitInfo::new(
        UnitType::GunTurret,
        TeamId::RED,
        UnitId::new(TeamId::RED, 11),
    );
    turret.flags = crate::access::unit_type_flags(UnitType::GunTurret);
    assert_eq!(turret.flags & STANDALONE, STANDALONE);
    assert!(turret.is_complex_member());
}
