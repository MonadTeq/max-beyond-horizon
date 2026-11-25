use super::*;

#[test]
fn frame_table_matches_resource_data() {
    // Explosions/projectiles — values from `D_LNDEXPD`/`D_AIREXPD`/`D_SEAEXPLD`/
    // `D_BLDEXPLD`/`D_HITEXPD`/`D_ROCKET`/`D_ALNMISS`.
    assert_eq!(frame_info(UnitType::LandExplosion).image_count, 14);
    assert_eq!(frame_info(UnitType::AirExplosion).image_count, 14);
    assert_eq!(frame_info(UnitType::SeaExplosion).image_count, 14);
    assert_eq!(frame_info(UnitType::BuildingExplosion).image_count, 28);
    assert_eq!(frame_info(UnitType::HitExplosion).image_count, 5);
    assert_eq!(frame_info(UnitType::Rocket).image_count, 16);
    assert_eq!(frame_info(UnitType::Torpedo).image_count, 16);
    assert_eq!(frame_info(UnitType::AlienMissile).image_count, 32);
    // Turrets: `D_FIXED`/`D_TANK` (`turret_image_base = 1/8`).
    assert_eq!(frame_info(UnitType::GunTurret).turret_image_count, 8);
    assert_eq!(frame_info(UnitType::Tank).turret_image_base, 8);
}

#[test]
fn image_index_max_follows_unitinfo_constructor() {
    assert_eq!(image_index_max(UnitType::LandExplosion, 0), 13);
    assert_eq!(image_index_max(UnitType::BuildingExplosion, 0), 27);
    assert_eq!(image_index_max(UnitType::HitExplosion, 0), 4);
    // Infantry/infiltrator — fixed 103 (`unitinfo.cpp:288-291`).
    assert_eq!(image_index_max(UnitType::Infiltrator, 0), 103);
    assert_eq!(image_index_max(UnitType::Infantry, 0), 103);
    // Hull rotation shifts `image_index` (`image_base + angle`).
    assert_eq!(image_index_max(UnitType::Tank, 3), 10);
}

/// The table is game data for **all** 93 types (`UNIT_END`), in `UnitType` order,
/// and every lookup returns the table row (`Unit::GetFrameInfo`).
#[test]
fn frame_table_covers_every_unit_type_in_enum_order() {
    assert_eq!(UNIT_FRAME_INFO.len(), crate::enums::UNIT_END as usize);
    assert_eq!(UNIT_FRAME_INFO.len(), UnitType::COUNT);

    for (index, info) in UNIT_FRAME_INFO.iter().enumerate() {
        let unit_type = UnitType::try_from(index as u16).expect("index < UNIT_END");
        assert_eq!(frame_info(unit_type), *info, "{unit_type:?}");
        // `D_*` rows always describe at least the idle frame.
        assert!(info.image_count >= 1, "{unit_type:?}: empty frame row");
        assert!(info.turret_image_count >= 0, "{unit_type:?}");
        // A turret-less type has no turret base either (`Unit::GetFrameInfo`).
        if info.turret_image_count == 0 {
            assert_eq!(info.turret_image_base, 0, "{unit_type:?}");
        }
    }
}

/// Turret frames come from `D_*` and are the source of `image_index_max` for
/// spinning turrets (`UnitsManager_DeployUnit`, `units_manager.cpp:2443`).
#[test]
fn turret_frame_rows_match_the_resource_data() {
    assert_eq!(
        (
            frame_info(UnitType::Tank).turret_image_base,
            frame_info(UnitType::Tank).turret_image_count
        ),
        (8, 8)
    );
    // Fixed turrets use `D_FIXED`: base 1, 8 rotations.
    for turret in [
        UnitType::GunTurret,
        UnitType::AntiAir,
        UnitType::Artillery,
        UnitType::MissileLauncher,
    ] {
        let info = frame_info(turret);
        assert_eq!(
            (info.turret_image_base, info.turret_image_count),
            (1, 8),
                "{turret:?}"
        );
    }
    assert_eq!(frame_info(UnitType::Scanner).turret_image_count, 16);
    assert_eq!(frame_info(UnitType::Awac).turret_image_count, 30);
}

/// `image_index_max = image_count + image_base + angle - 1` for all 8 hull
/// angles; infantry and the infiltrator are the fixed-`103` exception
/// (`unitinfo.cpp:288-294`).
#[test]
fn image_index_max_covers_all_hull_angles_and_exceptions() {
    for unit_type in [UnitType::Tank, UnitType::LandExplosion, UnitType::Scout] {
        let info = frame_info(unit_type);
        for angle in 0..=7u8 {
            assert_eq!(
                image_index_max(unit_type, angle),
                info.image_count + info.image_base + i16::from(angle) - 1,
                    "{unit_type:?} angle {angle}"
            );
        }
        // The first frame is `image_base` and the last one at angle 0 is
        // `image_base + image_count - 1` — the animation range is never inverted.
        assert!(image_index_max(unit_type, 0) >= info.image_base);
    }

    for unit_type in [UnitType::Infantry, UnitType::Infiltrator] {
        for angle in 0..=7u8 {
            assert_eq!(image_index_max(unit_type, angle), 103, "{unit_type:?}");
        }
    }

    // One-frame effect types: `image_index_max == image_base` (no animation).
    for unit_type in [UnitType::DeadWaldo, UnitType::SmallSlab] {
        let info = frame_info(unit_type);
        assert_eq!(info.image_count, 1, "{unit_type:?}");
        assert_eq!(image_index_max(unit_type, 0), info.image_base);
    }
}
