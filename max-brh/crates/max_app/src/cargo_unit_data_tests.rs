use super::*;

/// All 93 types are present in the resource, the order matches `UnitType`.
#[test]
fn resource_carries_all_unit_types_in_enum_order() {
    assert!(is_loaded(), "JSON не разобрался: {} записей", units().len());

    for index in 0..UNIT_END {
        let unit = UnitType::try_from(index).expect("UnitType 0..92");
        let data = unit_data(unit).expect("запись типа");
        assert!(!data.token.is_empty(), "{unit:?}");
    }

    // C++ tokens of special types (verification of `SC_UNITS`/`UnitType` order).
    assert_eq!(unit_token(UnitType::GoldRefinery), "COMMTWR");
    assert_eq!(unit_token(UnitType::HeavyVehiclePlant), "LANDPLT");
    assert_eq!(unit_token(UnitType::MineLayer), "MINELAYR");
    assert_eq!(unit_token(UnitType::DeadWaldo), "WALDO");
}

/// A portrait exists for all except the 22 non-units of the original (`INVALID_ID`).
#[test]
fn portraits_match_sc_units() {
    let with_portrait = (0..UNIT_END)
        .map(|index| UnitType::try_from(index).unwrap())
        .filter(|unit| unit_portrait(*unit).is_some())
        .count();

    assert_eq!(with_portrait, 71, "SC_UNITS: 71 ресурс P_*");
    assert_eq!(unit_portrait(UnitType::Tank), Some("P_TANK"));
    assert_eq!(unit_portrait(UnitType::GoldRefinery), Some("P_TRANSP"));
    assert_eq!(unit_portrait(UnitType::HeavyVehiclePlant), Some("P_HVYPLT"));
    assert_eq!(unit_portrait(UnitType::Constructor), Some("P_CONTRC"));
    // Non-units: `portrait` = `INVALID_ID` in `SC_UNITS`.
    for unit in [
        UnitType::Rocket,
        UnitType::Torpedo,
        UnitType::Harvester,
        UnitType::DeadWaldo,
        UnitType::LargeRubble,
    ] {
        assert_eq!(unit_portrait(unit), None, "{unit:?}");
    }
}

/// Descriptions: 69 non-empty; every type reachable in `CargoMenu` is described.
#[test]
fn descriptions_cover_every_cargo_menu_unit() {
    let described = (0..UNIT_END)
        .map(|index| UnitType::try_from(index).unwrap())
        .filter(|unit| !unit_description(*unit).is_empty())
        .count();
    assert_eq!(described, 69, "69 описаний в language.json");

    // The texts are verbatim `language.json` (en-US).
    assert!(unit_description(UnitType::Tank).starts_with("Heavily armored fighting vehicle."));
    assert!(unit_description(UnitType::Scout)
        .starts_with("High speed scouting vehicle for exploration and spotting enemy units."));
    assert_eq!(
        unit_description(UnitType::Road),
            "Units move twice as fast over road squares."
    );

    // 24 types without a description — debris/explosions/projectiles/`HARVSTER`/`WALDO`;
    // none passes `IsUnitFiltered` (checked in `mobilize`).
    for unit in [
        UnitType::Rocket,
        UnitType::Torpedo,
        UnitType::LandExplosion,
        UnitType::LargeRubble,
        UnitType::Harvester,
        UnitType::DeadWaldo,
        UnitType::SupportPlant,
        UnitType::RecreationCenter,
    ] {
        assert_eq!(unit_description(unit), "", "{unit:?}");
    }
}

/// `Unit::GetSprite()`/`GetShadow()`: all 93 types have a body,
/// shadow — for 77 (missiles/explosions/films/`WALDO` have `INVALID_ID` in `SC_UNITS`).
#[test]
fn sprite_and_shadow_follow_sc_units() {
    let sprites = (0..UNIT_END)
        .map(|index| UnitType::try_from(index).unwrap())
        .filter(|unit| unit_sprite(*unit).is_some())
        .count();
    let shadows = (0..UNIT_END)
        .map(|index| UnitType::try_from(index).unwrap())
        .filter(|unit| unit_shadow(*unit).is_some())
        .count();

    assert_eq!(sprites, 93, "SC_UNITS: sprite у всех 93 типов");
    assert_eq!(shadows, 77, "SC_UNITS: shadow у 77 типов");

    assert_eq!(unit_sprite(UnitType::Tank), Some("TANK"));
    assert_eq!(unit_shadow(UnitType::Tank), Some("S_TANK"));
    assert_eq!(unit_sprite(UnitType::GunTurret), Some("GUNTURRT"));
    assert_eq!(unit_shadow(UnitType::GunTurret), Some("S_GUNTUR"));
    assert_eq!(unit_sprite(UnitType::MasterBuilder), Some("MASTER"));
    assert_eq!(unit_shadow(UnitType::MasterBuilder), Some("S_MASTER"));
    // Submarine: `SC_UNITS` references `S_SUBMAR`, which is absent from
    // MAX.RES/PATCHES.RES — the asset loader checks the resource.
    assert_eq!(unit_shadow(UnitType::Submarine), Some("S_SUBMAR"));
    // Films/missiles/explosions and `WALDO` — without a shadow in the original.
    for unit in [
        UnitType::LargeTape,
        UnitType::SmallTape,
        UnitType::SeaMine,
        UnitType::Rocket,
        UnitType::AlienMissile,
        UnitType::DeadWaldo,
    ] {
        assert_eq!(unit_shadow(unit), None, "{unit:?}");
    }
}

/// `Unit::GetFrameInfo()`: all 93 types have a `D_*` resource; the tank,
/// the mobile AA and the escort — a separate turret.
#[test]
fn frame_data_covers_all_units() {
    let with_data = (0..UNIT_END)
        .map(|index| UnitType::try_from(index).unwrap())
        .filter(|unit| unit_frame_data(*unit).is_some())
        .count();
    assert_eq!(with_data, 93, "SC_UNITS: 93 ресурса D_*");

    assert_eq!(unit_frame_data(UnitType::Tank), Some("D_TANK"));
    assert_eq!(unit_frame_data(UnitType::MobileAntiAir), Some("D_SP_FLK"));
    assert_eq!(unit_frame_data(UnitType::Escort), Some("D_ESCORT"));
    assert_eq!(unit_frame_data(UnitType::GunTurret), Some("D_FIXED"));
    assert_eq!(unit_frame_data(UnitType::Scanner), Some("D_SCANNR"));
}

/// The table is non-empty even on a JSON failure — but then `is_loaded()` is false.
#[test]
fn json_resource_is_the_generated_file() {
    assert!(CARGO_UNIT_DATA_JSON.contains("\"COMMTWR\""));
    assert!(CARGO_UNIT_DATA_JSON.contains("\"P_CONTRC\""));
    assert!(CARGO_UNIT_DATA_JSON.contains("\"D_TANK\""));
    assert!(CARGO_UNIT_DATA_JSON.len() > 15_000);
}
