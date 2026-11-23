//! Unit-type frame data — a port of `Unit::GetFrameInfo()`.
//!
//! In C++, `UnitInfo::UnitInfo` (`unitinfo.cpp:276-294`) reads the `D_*` resource
//! (`Unit::m_frame_info`, `unit.cpp:51-64`) and sets
//! `image_index = image_base + angle`,
//! `image_index_max = image_count + image_index - 1` (for `COMMANDO`/`INFANTRY`
//! there is a special case — `103`). `UnitsManager_DeployUnit` (`units_manager.cpp:2443`)
//! overrides `image_index_max` for turrets from `turret_image_*`.
//!
//! The table is **game data**: the values are taken from `D_*` in `MAX.RES`/
//! `PATCHES.RES` (`FrameInfo::decode`, the same bytes that
//! `Unit::FileLoad` reads), not fitted. `image_count` in C++ is `int8_t`
//! (`unit.cpp:57-64`): for `COMMANDO`/`INFANTRY` 208/200 become
//! negative, but these two types have a separate `103` branch, so
//! the table keeps the source values.
//!
//! Why the simulation needs this: `UnitsManager_ProgressExplosion`
//! (`units_manager.cpp:4006`) advances `image_index` up to `image_index_max` and
//! only then removes the effect unit; `ProcessOrderMove` (`ORDER_STATE_ELEVATE`)
//! and `ProcessOrderSentry` compare the same fields. Without the table
//! `image_index_max` stayed zero, and every explosion was removed on the first tick.

use crate::enums::UnitType;

/// Unit frame header (`Unit::FrameInfo`, `D_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameInfo {
    /// Hull frame base (`FrameInfo.image_base`).
    pub image_base: i16,
    /// Hull frame count (`FrameInfo.image_count`).
    pub image_count: i16,
    /// Turret frame base.
    pub turret_image_base: i16,
    /// Turret frame count.
    pub turret_image_count: i16,
}

impl FrameInfo {
    /// Const constructor for the table.
    pub const fn new(
        image_base: i16,
        image_count: i16,
        turret_image_base: i16,
        turret_image_count: i16,
    ) -> Self {
        Self {
            image_base,
            image_count,
            turret_image_base,
            turret_image_count,
        }
    }
}

/// `image_index_max` from the `UnitInfo` constructor (`unitinfo.cpp:288-294`).
///
/// `image_index = image_base + angle`; for infantry and the infiltrator C++ substitutes
/// a fixed `103` regardless of `image_count`.
pub fn image_index_max(unit_type: UnitType, angle: u8) -> i16 {
    let info = frame_info(unit_type);

    if matches!(unit_type, UnitType::Infiltrator | UnitType::Infantry) {
        103
    } else {
        info.image_count + info.image_base + i16::from(angle) - 1
    }
}

/// `D_*` table in [`UnitType`] order (`0..UNIT_END`).
///
/// The order matches `SC_UNITS`; `max_core::frames::tests` checks the extracted
/// values of key types, and `max_app/tests/unit_sprites.rs::
/// frame_info_table_matches_game_data` compares the whole table against the actual
/// game resources (`Unit::GetFrameInfo`).
pub const UNIT_FRAME_INFO: [FrameInfo; crate::enums::UNIT_END as usize] = [
    FrameInfo::new(0, 2, 0, 0),   // 0x00 COMMTWR
    FrameInfo::new(0, 2, 0, 0),   // 0x01 POWERSTN
    FrameInfo::new(0, 2, 0, 0),   // 0x02 POWGEN
    FrameInfo::new(0, 2, 0, 0),   // 0x03 BARRACKS
    FrameInfo::new(0, 2, 0, 0),   // 0x04 SHIELD
    FrameInfo::new(0, 16, 0, 0),  // 0x05 RADAR
    FrameInfo::new(0, 2, 0, 0),   // 0x06 ADUMP
    FrameInfo::new(0, 2, 0, 0),   // 0x07 FDUMP
    FrameInfo::new(0, 2, 0, 0),   // 0x08 GOLDSM
    FrameInfo::new(0, 2, 0, 0),   // 0x09 DEPOT
    FrameInfo::new(0, 2, 0, 0),   // 0x0A HANGAR
    FrameInfo::new(0, 2, 0, 0),   // 0x0B DOCK
    FrameInfo::new(0, 2, 0, 0),   // 0x0C CONNECTR
    FrameInfo::new(0, 2, 0, 0),   // 0x0D LRGRUBLE
    FrameInfo::new(0, 5, 0, 0),   // 0x0E SMLRUBLE
    FrameInfo::new(0, 2, 0, 0),   // 0x0F LRGTAPE
    FrameInfo::new(0, 2, 0, 0),   // 0x10 SMLTAPE
    FrameInfo::new(0, 5, 0, 0),   // 0x11 LRGSLAB
    FrameInfo::new(0, 1, 0, 0),   // 0x12 SMLSLAB
    FrameInfo::new(0, 1, 0, 0),   // 0x13 LRGCONES
    FrameInfo::new(0, 1, 0, 0),   // 0x14 SMLCONES
    FrameInfo::new(0, 1, 0, 0),   // 0x15 ROAD
    FrameInfo::new(0, 1, 0, 0),   // 0x16 LANDPLT
    FrameInfo::new(0, 2, 0, 0),   // 0x17 SHIPYARD
    FrameInfo::new(0, 2, 0, 0),   // 0x18 LIGHTPL
    FrameInfo::new(0, 2, 0, 0),   // 0x19 HVYPLT
    FrameInfo::new(0, 2, 0, 0),   // 0x1A SUPLPLT
    FrameInfo::new(0, 2, 0, 0),   // 0x1B AIRPLT
    FrameInfo::new(0, 2, 0, 0),   // 0x1C HABITAT
    FrameInfo::new(0, 2, 0, 0),   // 0x1D RESEARCH
    FrameInfo::new(0, 2, 0, 0),   // 0x1E GREENHSE
    FrameInfo::new(0, 2, 0, 0),   // 0x1F RECREATE
    FrameInfo::new(0, 2, 0, 0),   // 0x20 TRAINHAL
    FrameInfo::new(0, 1, 0, 0),   // 0x21 WTRPLTFM
    FrameInfo::new(0, 1, 1, 8),   // 0x22 GUNTURRT
    FrameInfo::new(0, 1, 1, 8),   // 0x23 ANTIAIR
    FrameInfo::new(0, 1, 1, 8),   // 0x24 ARTYTRRT
    FrameInfo::new(0, 1, 1, 8),   // 0x25 ANTIMSSL
    FrameInfo::new(0, 2, 0, 0),   // 0x26 BLOCK
    FrameInfo::new(0, 4, 0, 0),   // 0x27 BRIDGE
    FrameInfo::new(0, 16, 0, 0),  // 0x28 MININGST
    FrameInfo::new(0, 1, 0, 0),   // 0x29 LANDMINE
    FrameInfo::new(0, 1, 0, 0),   // 0x2A SEAMINE
    FrameInfo::new(0, 14, 0, 0),  // 0x2B LNDEXPLD
    FrameInfo::new(0, 14, 0, 0),  // 0x2C AIREXPLD
    FrameInfo::new(0, 14, 0, 0),  // 0x2D SEAEXPLD
    FrameInfo::new(0, 28, 0, 0),  // 0x2E BLDEXPLD
    FrameInfo::new(0, 5, 0, 0),   // 0x2F HITEXPLD
    FrameInfo::new(0, 8, 0, 0),   // 0x30 MASTER
    FrameInfo::new(0, 16, 0, 0),  // 0x31 CONSTRCT
    FrameInfo::new(0, 16, 0, 0),  // 0x32 SCOUT
    FrameInfo::new(0, 8, 8, 8),   // 0x33 TANK
    FrameInfo::new(0, 8, 0, 0),   // 0x34 ARTILLRY
    FrameInfo::new(0, 8, 0, 0),   // 0x35 ROCKTLCH
    FrameInfo::new(0, 8, 0, 0),   // 0x36 MISSLLCH
    FrameInfo::new(0, 8, 8, 8),   // 0x37 SP_FLAK
    FrameInfo::new(0, 8, 0, 0),   // 0x38 MINELAYR
    FrameInfo::new(0, 16, 0, 0),  // 0x39 SURVEYOR
    FrameInfo::new(0, 8, 8, 16),  // 0x3A SCANNER
    FrameInfo::new(0, 8, 0, 0),   // 0x3B SPLYTRCK
    FrameInfo::new(0, 8, 0, 0),   // 0x3C GOLDTRCK
    FrameInfo::new(0, 16, 0, 0),  // 0x3D ENGINEER
    FrameInfo::new(0, 8, 0, 0),   // 0x3E BULLDOZR
    FrameInfo::new(0, 8, 0, 0),   // 0x3F REPAIR
    FrameInfo::new(0, 8, 0, 0),   // 0x40 FUELTRCK
    FrameInfo::new(0, 8, 0, 0),   // 0x41 CLNTRANS
    FrameInfo::new(0, 208, 0, 0), // 0x42 COMMANDO (C++ int8 → special 103)
    FrameInfo::new(0, 200, 0, 0), // 0x43 INFANTRY (C++ int8 → special 103)
    FrameInfo::new(0, 8, 8, 8),   // 0x44 FASTBOAT
    FrameInfo::new(0, 8, 0, 0),   // 0x45 CORVETTE
    FrameInfo::new(0, 8, 8, 8),   // 0x46 BATTLSHP
    FrameInfo::new(0, 16, 0, 0),  // 0x47 SUBMARNE
    FrameInfo::new(0, 8, 0, 0),   // 0x48 SEATRANS
    FrameInfo::new(0, 8, 0, 0),   // 0x49 MSSLBOAT
    FrameInfo::new(0, 8, 0, 0),   // 0x4A SEAMNLYR
    FrameInfo::new(0, 8, 0, 0),   // 0x4B CARGOSHP
    FrameInfo::new(0, 8, 0, 0),   // 0x4C FIGHTER
    FrameInfo::new(0, 8, 0, 0),   // 0x4D BOMBER
    FrameInfo::new(0, 8, 0, 0),   // 0x4E AIRTRANS
    FrameInfo::new(0, 8, 8, 30),  // 0x4F AWAC
    FrameInfo::new(0, 8, 0, 0),   // 0x50 JUGGRNT
    FrameInfo::new(0, 8, 8, 8),   // 0x51 ALNTANK
    FrameInfo::new(0, 8, 0, 0),   // 0x52 ALNASGUN
    FrameInfo::new(0, 8, 0, 0),   // 0x53 ALNPLANE
    FrameInfo::new(0, 16, 0, 0),  // 0x54 ROCKET
    FrameInfo::new(0, 16, 0, 0),  // 0x55 TORPEDO
    FrameInfo::new(0, 32, 0, 0),  // 0x56 ALNMISSL
    FrameInfo::new(0, 2, 0, 0),   // 0x57 ALNTBALL
    FrameInfo::new(0, 2, 0, 0),   // 0x58 ALNABALL
    FrameInfo::new(0, 5, 0, 0),   // 0x59 RKTSMOKE
    FrameInfo::new(0, 3, 0, 0),   // 0x5A TRPBUBLE
    FrameInfo::new(0, 1, 0, 0),   // 0x5B HARVSTER
    FrameInfo::new(0, 1, 0, 0),   // 0x5C WALDO
];

/// `Unit::GetFrameInfo()` for a type.
#[inline]
pub fn frame_info(unit_type: UnitType) -> FrameInfo {
    UNIT_FRAME_INFO
        .get(unit_type as usize)
        .copied()
        .unwrap_or(FrameInfo::new(0, 1, 0, 0))
}

#[cfg(test)]
mod tests {
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
}
