use super::*;
use max_core::unit::UnitId;

/// 23 non-units: effects, projectiles/trails, debris/decorations, service types.
/// The manual does not describe them, they are not shown in the UI (no UFN).
const NON_UNITS: [UnitType; 23] = [
    UnitType::LargeRubble,
    UnitType::SmallRubble,
    UnitType::LargeTape,
    UnitType::SmallTape,
    UnitType::LargeSlab,
    UnitType::SmallSlab,
    UnitType::LargeCones,
    UnitType::SmallCones,
    UnitType::LandExplosion,
    UnitType::AirExplosion,
    UnitType::SeaExplosion,
    UnitType::BuildingExplosion,
    UnitType::HitExplosion,
    UnitType::MasterBuilder,
    UnitType::Rocket,
    UnitType::Torpedo,
    UnitType::AlienMissile,
    UnitType::AlienTankBall,
    UnitType::AlienArtilleryBall,
    UnitType::RocketSmoke,
    UnitType::TorpedoBubble,
    UnitType::Harvester,
    UnitType::DeadWaldo,
];

#[test]
fn table_matches_canon() {
    assert_eq!(UnitType::GoldRefinery.tag(), "COMMTWR");
    assert_eq!(
        unit_user_friendly_name(UnitType::GoldRefinery),
            "Gold Refinery"
    );
    assert_eq!(
        unit_user_friendly_name(UnitType::MaterialStorage),
            "Material Storage"
    );
    assert_eq!(
        unit_user_friendly_name(UnitType::FuelStorage),
            "Fuel Storage"
    );
    assert_eq!(unit_user_friendly_name(UnitType::GoldVault), "Gold Vault");
    assert_eq!(
        unit_user_friendly_name(UnitType::ResearchCentre),
            "Research Centre"
    );
    assert_eq!(unit_user_friendly_name(UnitType::EcoSphere), "Eco-Sphere");
    assert_eq!(
        unit_user_friendly_name(UnitType::MissileLauncher),
            "Missile Launcher"
    );
    assert_eq!(unit_user_friendly_name(UnitType::Artillery), "Artillery");
    assert_eq!(
        unit_user_friendly_name(UnitType::ConcreteBlock),
            "Concrete Block"
    );
    assert_eq!(
        unit_user_friendly_name(UnitType::ArmouredPersonnelCarrier),
            "Armoured Personnel Carrier"
    );
    assert_eq!(
        unit_user_friendly_name(UnitType::Infiltrator),
            "Infiltrator"
    );
    assert_eq!(unit_user_friendly_name(UnitType::Escort), "Escort");
    assert_eq!(unit_user_friendly_name(UnitType::Gunboat), "Gunboat");
    assert_eq!(
        unit_user_friendly_name(UnitType::MissileCruiser),
            "Missile Cruiser"
    );
    assert_eq!(
        unit_user_friendly_name(UnitType::Bomber),
            "Ground Attack Plane"
    );
    assert_eq!(unit_user_friendly_name(UnitType::Awac), "AWAC");
    assert_eq!(
        unit_user_friendly_name(UnitType::SeaTransport),
            "Sea Transport"
    );
    assert_eq!(unit_user_friendly_name(UnitType::CargoShip), "Cargo Ship");
}

#[test]
fn table_covers_70_units_and_23_non_units() {
    let table = unit_names();
    assert_eq!(table.units.len(), 70);
    assert_eq!(NON_UNITS.len() + table.units.len(), UnitType::COUNT);

    // Every key is a token of an existing type (no "dangling" records).
    for token in table.units.keys() {
        assert!(
            UnitType::ALL.iter().any(|unit_type| unit_type.tag() == token),
                "ключ «{token}» не соответствует ни одному UnitType::tag()"
        );
    }

    // Units: a record exists, the UFN is non-empty (`Awac` is a canon exception:
    // its User-Friendly Name «AWAC» coincides with the `AWAC` token).
    for unit_type in UnitType::ALL {
        if NON_UNITS.contains(unit_type) {
            continue;
        }
        assert!(
            table.units.contains_key(unit_type.tag()),
                "{unit_type:?}: нет записи UFN (токен {})",
            unit_type.tag()
        );
        let name = unit_user_friendly_name(*unit_type);
        assert!(!name.is_empty(), "{unit_type:?}: пустая UFN");
    }

    // Non-units: no record, the function returns tag().
    for unit_type in NON_UNITS {
        assert!(
            !table.units.contains_key(unit_type.tag()),
                "{unit_type:?}: у эффекта/декорации не должно быть UFN"
        );
        assert_eq!(unit_user_friendly_name(unit_type), unit_type.tag());
    }
}

/// `UnitInfo::CalcRomanDigit`/`GetVersion` (`unitinfo.cpp:805-826`, `:915-925`):
/// the mark level in Roman numerals, including the subtractive 4/9 forms.
#[test]
fn roman_marks_match_calc_roman_digit() {
    let cases = [
        (1, "I"),
        (2, "II"),
        (3, "III"),
        (4, "IV"),
        (5, "V"),
        (6, "VI"),
        (7, "VII"),
        (8, "VIII"),
        (9, "IX"),
        (10, "X"),
        (11, "XI"),
        (14, "XIV"),
        (19, "XIX"),
        (20, "XX"),
        (30, "XXX"),
        (40, "XL"),
        (49, "XLIX"),
        (50, "L"),
        (90, "XC"),
        (99, "XCIX"),
        (100, "C"),
        (400, "CD"),
        (900, "CM"),
    ];

    for (version, expected) in cases {
        assert_eq!(roman_version(version), expected, "version {version}");
    }
}

/// `UnitInfo::GetName` (`unitinfo.cpp:757-779`): the user name, otherwise
/// `"%s %i"` — the type UFN and the instance number.
#[test]
fn get_name_uses_the_instance_number_of_an_unnamed_unit() {
    let mut unit = UnitInfo::new(UnitType::Scout, max_core::TeamId::RED, UnitId(1));
    unit.unit_id = 7;

    assert_eq!(unit_name(&unit), "Scout 7");

    // A user name replaces the whole string — the number is not appended.
    unit.set_name("Oddball");
    assert_eq!(unit_name(&unit), "Oddball");
}

/// `UnitInfo::GetDisplayName` (`unitinfo.cpp:781-803`): `"Mk %s %s"` — the mark
/// of the unit's own values plus `GetName`. The owner's canonical form is
/// `"Mk II Scout 7"`.
#[test]
fn display_name_is_mark_plus_name() {
    let mut unit = UnitInfo::new(UnitType::Scout, max_core::TeamId::RED, UnitId(1));
    unit.unit_id = 7;
    assert_eq!(unit.base_values.version, 1, "the default mark is 1");
    assert_eq!(unit_display_name(&unit), "Mk I Scout 7");

    // An upgrade copies the values into the instance and bumps `version`
    // (`UnitValues::UpdateVersion` only advances an occupied slot).
    unit.base_values.version = 2;
    assert_eq!(unit_display_name(&unit), "Mk II Scout 7");

    // Repeat upgrades give the Roman forms of `CalcRomanDigit`.
    unit.base_values.version = 4;
    assert_eq!(unit_display_name(&unit), "Mk IV Scout 7");

    // A named unit keeps the prefix: `Mk <roman> <user name>`.
    unit.set_name("Oddball");
    assert_eq!(unit_display_name(&unit), "Mk IV Oddball");
}

