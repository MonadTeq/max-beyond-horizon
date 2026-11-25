use super::*;

#[test]
fn tags_match_the_cpp_resource_names() {
    assert_eq!(Sfx::Click.tag(), "KCARG0");
    assert_eq!(Sfx::Done.tag(), "NDONE0");
    assert_eq!(Sfx::Cancel.tag(), "NCANC0");
    assert_eq!(Sfx::Button.tag(), "MBUTT0");
    assert_eq!(Sfx::Help.tag(), "NHELP0");
    assert_eq!(Sfx::BuildSitePlaced.tag(), "V_M049");
    assert!(Sfx::BuildSitePlaced.is_voice());
    assert!(!Sfx::Click.is_voice());
}

// -----------------------------------------------------------------------
// Combat sounds: the `SC_UNITS` table (`ParseSoundEffects`, units.cpp:228-290)
// -----------------------------------------------------------------------

#[test]
fn unit_sfx_table_covers_all_types_in_cpp_order() {
    assert_eq!(
        unit_sfx_count(),
        UNIT_END as usize,
            "таблица должна покрывать все {} типов SC_UNITS",
        UNIT_END
    );
    assert_eq!(unit_sfx_token(UnitType::GoldRefinery), "COMMTWR");
    assert_eq!(unit_sfx_token(UnitType::Tank), "TANK");
    assert_eq!(unit_sfx_token(UnitType::AirUnitsPlant), "AIRPLT");
    assert_eq!(unit_sfx_token(UnitType::RocketSmoke), "RKTSMOKE");
}

#[test]
fn unit_fire_hit_explosion_come_from_the_unit_tables() {
    // `TANK` defines its own sounds: fire = TANK14, hit = TANK15, expload = TANK16.
    let fire = unit_sound(UnitType::Tank, UnitSoundKind::Fire).expect("fire");
    assert_eq!(fire.tag, "TANK14");
    assert_eq!(fire.volume, 60);
    assert!(!fire.is_default);

    let hit = unit_sound(UnitType::Tank, UnitSoundKind::Hit).expect("hit");
    assert_eq!(hit.tag, "TANK15");
    assert!(!hit.is_default);

    let explosion = unit_sound(UnitType::Tank, UnitSoundKind::Explode).expect("expload");
    assert_eq!(explosion.tag, "TANK16");
    assert_eq!(explosion.volume, 80);
    assert!(!explosion.is_default);

    // Infantry (`INFANTRY`) — `INFAN14/15/16`.
    assert_eq!(
        unit_sound(UnitType::Infantry, UnitSoundKind::Fire)
            .expect("infantry fire")
            .tag,
            "INFAN14"
    );

    // `FIGHTER` — `FIGHT14/15/16` (aircraft).
    assert_eq!(
        unit_sound(UnitType::Fighter, UnitSoundKind::Explode)
            .expect("fighter expload")
            .tag,
            "FIGHT16"
    );
}

#[test]
fn units_without_own_sound_fall_back_to_default_table() {
    // `LNDEXPLD`/`AIREXPLD`/`SEAEXPLD`/`BLDEXPLD` have no `sound_effects`
    // of their own — `ParseSoundEffects` takes `default_sound_effects` and marks
    // `is_default` (`units.cpp:288`). `GEN_XPLD` — volume 80.
    for unit_type in [
        UnitType::LandExplosion,
        UnitType::AirExplosion,
        UnitType::SeaExplosion,
        UnitType::BuildingExplosion,
    ] {
        let explosion = unit_sound(unit_type, UnitSoundKind::Explode)
            .unwrap_or_else(|| panic!("{unit_type:?}: default expload"));
        assert_eq!(explosion.tag, "GEN_XPLD", "{unit_type:?}");
        assert_eq!(explosion.volume, 80);
        assert!(explosion.is_default, "{unit_type:?}: звук по умолчанию");
    }

    // The missile/trail also has no sounds of its own — defaults.
    assert_eq!(
        unit_sound(UnitType::Rocket, UnitSoundKind::Fire)
            .expect("rocket fire")
            .tag,
            "GEN_FIRE"
    );
}

#[test]
fn combat_sound_maps_events_to_tags_and_volume() {
    let event = UnitSound {
        unit: max_core::unit::UnitId(7),
        unit_type: UnitType::Tank,
        kind: UnitSoundKind::Fire,
    };
    let sound = combat_sound(event).expect("tank fire");
    assert_eq!(sound.tag, "TANK14");
    assert!((sound.gain - 0.6).abs() < 1e-6, "volume 60 → gain 0.6");

    let explosion = combat_sound(UnitSound {
        unit: max_core::unit::UnitId(7),
        unit_type: UnitType::LandExplosion,
        kind: UnitSoundKind::Explode,
    })
    .expect("explosion");
    assert_eq!(explosion.tag, "GEN_XPLD");
    assert!((explosion.gain - 0.8).abs() < 1e-6);
    assert!(explosion.is_default);
}

#[test]
fn unit_status_voice_follows_cpp_order() {
    use max_core::enums::{UnitOrder, UnitType};
    use max_core::geo::Point;
    use max_core::team::TeamId;

    let mut unit =
        max_core::unit::UnitInfo::new(UnitType::Tank, TeamId::RED, max_core::unit::UnitId(1));
    unit.grid = Point::new(5, 5);
    unit.flags = max_core::access::unit_type_flags(UnitType::Tank);
    unit.base_values = max_core::unit_values::base_unit_values(UnitType::Tank);
    unit.hits = unit
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_HITS) as u16;
    unit.ammo = unit
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_AMMO) as u8;
    unit.speed = unit
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_SPEED) as u16;

    // A tank without damage in AWAIT — `V_M001`.
    unit.orders = UnitOrder::Await;
    assert_eq!(unit_status_voice(&unit), ("V_START", Some("V_M001")));

    // Without ammo — `V_M142` (first in the chain).
    unit.ammo = 0;
    assert_eq!(unit_status_voice(&unit).1, Some("V_M142"));

    // Scout — `V_M191`, if everything is fine.
    let mut surveyor = max_core::unit::UnitInfo::new(
        UnitType::Surveyor,
        TeamId::RED,
        max_core::unit::UnitId(2),
    );
    surveyor.grid = Point::new(6, 6);
    surveyor.flags = max_core::access::unit_type_flags(UnitType::Surveyor);
    surveyor.base_values = max_core::unit_values::base_unit_values(UnitType::Surveyor);
    surveyor.hits = surveyor
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_HITS) as u16;
    surveyor.ammo = surveyor
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_AMMO) as u8;
    surveyor.speed = surveyor
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_SPEED) as u16;
    assert_eq!(unit_status_voice(&surveyor).1, Some("V_M191"));
}
