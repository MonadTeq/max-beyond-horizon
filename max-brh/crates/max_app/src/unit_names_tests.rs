use super::*;

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
