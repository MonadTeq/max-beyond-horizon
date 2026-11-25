use super::*;
use crate::access::{unit_type_flags, UNIT_TYPE_PROPERTIES};
use crate::enums::unit_flags::HOVERING;
use crate::team::TeamId;
use crate::unit::UnitId;

/// Direct `SC_UNITS` flag-bit check — independent of the classifier.
fn has_flag(unit_type: UnitType, mask: u32) -> bool {
    UNIT_TYPE_PROPERTIES[unit_type as usize].flags & mask != 0
}

/// Direct `SC_ATTRI` attribute check.
fn base_attack(unit_type: UnitType) -> u16 {
    base_unit_values(unit_type).attack
}

/// Expected form by flags and the turret list (from `object-classifier.md`).
fn expected_mobility(unit_type: UnitType) -> Mobility {
    let is_building = has_flag(unit_type, BUILDING)
        || matches!(
            unit_type,
            UnitType::GunTurret
                | UnitType::Artillery
                | UnitType::AntiAir
                | UnitType::MissileLauncher
        );

    if is_building {
        Mobility::Building
    } else if has_flag(unit_type, STATIONARY) {
        Mobility::Stationary
    } else {
        Mobility::Mobile
    }
}

/// Expected footprint by the `BUILDING` flag (`object-classifier.md`).
fn expected_footprint(unit_type: UnitType) -> Footprint {
    if has_flag(unit_type, BUILDING) {
        Footprint::Quad
    } else {
        Footprint::Single
    }
}

/// Expected environment by flags (`MOBILE_*`).
fn expected_environment(unit_type: UnitType) -> Option<Environment> {
    let air = has_flag(unit_type, MOBILE_AIR_UNIT);
    let sea = has_flag(unit_type, MOBILE_SEA_UNIT);
    let land = has_flag(unit_type, MOBILE_LAND_UNIT);

    if air {
        Some(Environment::Air)
    } else if sea && land {
        Some(Environment::Amphibious)
    } else if land {
        Some(Environment::Ground)
    } else if sea {
        Some(Environment::Sea)
    } else {
        None
    }
}

#[test]
fn combat_role_matches_attack_for_all_93_types() {
    assert_eq!(UnitType::ALL.len(), 93);

    let mut combat = 0;
    for &unit_type in UnitType::ALL {
        let attack = base_attack(unit_type);
        let expected = if attack > 0 {
            CombatRole::Combat
        } else {
            CombatRole::NonCombat
        };

        assert_eq!(
            CombatRole::classify(unit_type),
            expected,
                "{unit_type:?}: attack = {attack}"
        );
        assert_eq!(unit_type.combat_role(), expected, "{unit_type:?}");
        assert_eq!(
            CombatRole::from_attack(attack),
            expected,
                "{unit_type:?}: from_attack"
        );
        assert_eq!(expected.is_combat(), attack > 0, "{unit_type:?}");

        if expected == CombatRole::Combat {
            combat += 1;
        }
    }

    // 25 combat types in `SC_ATTRI` (turrets, mines, mobile vehicles).
    assert_eq!(combat, 25);
}

#[test]
fn mobility_matches_shape_rules_for_all_93_types() {
    let (mut building, mut stationary, mut mobile) = (0, 0, 0);

    for &unit_type in UnitType::ALL {
        let expected = expected_mobility(unit_type);

        assert_eq!(
            Mobility::classify(unit_type),
            expected,
                "{unit_type:?}: flags = {:#010X}",
            unit_type_flags(unit_type)
        );
        assert_eq!(unit_type.mobility(), expected, "{unit_type:?}");

        // Data invariant: `BUILDING ⟹ STATIONARY` (`object-classifier.md`).
        if has_flag(unit_type, BUILDING) {
            assert!(
                has_flag(unit_type, STATIONARY),
                    "{unit_type:?}: BUILDING без STATIONARY"
            );
        }

        match expected {
            Mobility::Building => building += 1,
            Mobility::Stationary => stationary += 1,
            Mobility::Mobile => mobile += 1,
        }
    }

    assert_eq!((building, stationary, mobile), (26, 24, 43));

    // Turrets: the `BUILDING` flag is cleared, so only the semantic
    // exception (the owner's decision) makes them buildings.
    for &turret in &TURRET_UNIT_TYPES {
        assert!(!has_flag(turret, BUILDING), "{turret:?}");
        assert!(has_flag(turret, STATIONARY), "{turret:?}");
        assert_eq!(Mobility::classify(turret), Mobility::Building, "{turret:?}");
    }
    assert!(is_turret(UnitType::GunTurret));
    assert!(!is_turret(UnitType::LandMine));
}

#[test]
fn footprint_matches_building_bit_for_all_93_types() {
    let (mut quad, mut single) = (0, 0);

    for &unit_type in UnitType::ALL {
        let expected = expected_footprint(unit_type);

        assert_eq!(
            Footprint::classify(unit_type),
            expected,
                "{unit_type:?}: flags = {:#010X}",
            unit_type_flags(unit_type)
        );
        assert_eq!(unit_type.footprint(), expected, "{unit_type:?}");
        assert_eq!(unit_type.is_quad(), expected.is_quad(), "{unit_type:?}");
        assert_eq!(
            Footprint::classify_flags(unit_type, unit_type_flags(unit_type)),
            expected,
                "{unit_type:?}"
        );

        if expected.is_quad() {
            quad += 1;
        } else {
            single += 1;
        }
    }

    // 26 buildings by `Mobility`, of which 4 turrets have `BUILDING` cleared: 22 + 71.
    assert_eq!((quad, single), (22, 71));

    // Stable serialization tags — explicit discriminants, not order.
    assert_eq!(Footprint::Single as u8, 0);
    assert_eq!(Footprint::Quad as u8, 1);
}

#[test]
fn footprint_quad_implies_mobility_building_for_all_93_types() {
    for &unit_type in UnitType::ALL {
        if unit_type.is_quad() {
            assert_eq!(
                Mobility::classify(unit_type),
                Mobility::Building,
                    "{unit_type:?}: Footprint::Quad без Mobility::Building"
            );
        }
    }

    // The converse is false: turrets are `Building` but `Single`.
    assert!(TURRET_UNIT_TYPES.iter().any(|turret| turret.is_building()));
}

#[test]
fn turrets_are_single_footprint_but_building_mobility() {
    for &turret in &TURRET_UNIT_TYPES {
        assert_eq!(
            turret.footprint(),
            Footprint::Single,
                "{turret:?}: турель занимает 1×1"
        );
        assert!(!turret.is_quad(), "{turret:?}");
        assert_eq!(
            Mobility::classify(turret),
            Mobility::Building,
                "{turret:?}: турель — здание семантически"
        );
        assert!(!has_flag(turret, BUILDING), "{turret:?}");
    }
}

#[test]
fn footprint_control_cases_for_decorations() {
    // 2×2 decorations carry `BUILDING` (`SC_UNITS`) — they are `Quad`.
    for decoration in [
        UnitType::LargeRubble,
        UnitType::LargeTape,
        UnitType::LargeSlab,
        UnitType::LargeCones,
    ] {
        assert_eq!(
            decoration.footprint(),
            Footprint::Quad,
                "{decoration:?}: {:#010X}",
            unit_type_flags(decoration)
        );
        assert!(decoration.is_quad(), "{decoration:?}");
    }

    // 1×1 decorations — `Single`.
    for decoration in [
        UnitType::SmallRubble,
        UnitType::SmallTape,
        UnitType::SmallSlab,
        UnitType::SmallCones,
    ] {
        assert_eq!(
            decoration.footprint(),
            Footprint::Single,
                "{decoration:?}: {:#010X}",
            unit_type_flags(decoration)
        );
        assert!(decoration.footprint().is_single(), "{decoration:?}");
    }

    // Instance: footprint by instance flags.
    let mut mine = UnitInfo::new(UnitType::MiningStation, TeamId::RED, UnitId::NO_ID);
    mine.flags = unit_type_flags(UnitType::MiningStation);
    assert!(mine.is_quad());
    mine.flags &= !BUILDING;
    assert!(!mine.is_quad());
    assert!(mine.footprint().is_single());
}

#[test]
fn environment_matches_mobile_flags_for_all_93_types() {
    let mut counts = [0usize; 5];

    for &unit_type in UnitType::ALL {
        let expected = expected_environment(unit_type);

        assert_eq!(
            Environment::classify(unit_type),
            expected,
                "{unit_type:?}: flags = {:#010X}",
            unit_type_flags(unit_type)
        );
        assert_eq!(unit_type.environment(), expected, "{unit_type:?}");

        let air = has_flag(unit_type, MOBILE_AIR_UNIT);
        if air {
            // Air does not combine with land-sea flags — otherwise
            // the `Air` priority would hide an amphibian.
            assert!(
                !has_flag(unit_type, MOBILE_SEA_UNIT | MOBILE_LAND_UNIT),
                    "{unit_type:?}: MOBILE_AIR_UNIT вместе с SEA/LAND"
            );
        }

        // The environment is defined only for mobile types.
        if expected.is_some() {
            assert_eq!(
                Mobility::classify(unit_type),
                Mobility::Mobile,
                    "{unit_type:?}: среда у немобильного типа"
            );
        }

        counts[match expected {
            None => 0,
            Some(Environment::Air) => 1,
            Some(Environment::Amphibious) => 2,
            Some(Environment::Ground) => 3,
            Some(Environment::Sea) => 4,
        }] += 1;
    }

    // Air=5, Amphibious=5, Ground=17, Sea=9, no environment=57.
    assert_eq!(counts, [57, 5, 5, 17, 9]);

    // Projectiles and trails are mobile by flag, but have no environment.
    for projectile in [
        UnitType::Rocket,
        UnitType::Torpedo,
        UnitType::AlienMissile,
        UnitType::AlienTankBall,
        UnitType::AlienArtilleryBall,
        UnitType::RocketSmoke,
        UnitType::TorpedoBubble,
    ] {
        assert_eq!(Mobility::classify(projectile), Mobility::Mobile);
        assert_eq!(Environment::classify(projectile), None);
    }
}

#[test]
fn producing_matches_cargo_get_net_production_for_all_93_types() {
    // An independent list — exactly the `Cargo_GetNetProduction` branches
    // (`cargo.cpp:70`).
    let expected: [UnitType; 12] = [
        UnitType::Shipyard,
        UnitType::LightVehiclePlant,
        UnitType::HeavyVehiclePlant,
        UnitType::TrainingHall,
        UnitType::AirUnitsPlant,
        UnitType::GoldRefinery,
        UnitType::PowerStation,
        UnitType::PowerGenerator,
        UnitType::Habitat,
        UnitType::ResearchCentre,
        UnitType::EcoSphere,
        UnitType::MiningStation,
    ];

    let mut producing = 0;
    for &unit_type in UnitType::ALL {
        let is_producing = expected.contains(&unit_type);

        assert_eq!(unit_type.is_producing(), is_producing, "{unit_type:?}");
        assert_eq!(
            PRODUCING_UNIT_TYPES.contains(&unit_type),
            is_producing,
                "{unit_type:?}: PRODUCING_UNIT_TYPES"
        );

        if is_producing {
            producing += 1;
            // Producing buildings are non-combat (`object-classifier.md`).
            assert_eq!(
                CombatRole::classify(unit_type),
                CombatRole::NonCombat,
                    "{unit_type:?}"
            );
        }
    }
    assert_eq!(producing, 12);
    assert_eq!(PRODUCING_UNIT_TYPES.len(), 12);

    // `BARRACKS`/`SUPRTPLT` are not producing: they are absent from
    // `Cargo_GetNetProduction` (repair/alien building).
    assert!(!UnitType::Barracks.is_producing());
    assert!(!UnitType::SupportPlant.is_producing());
}

#[test]
fn cargo_kind_matches_cargo_type_for_all_93_types() {
    // Independent lists by `CARGO_TYPE_*` groups (`unit.hpp`).
    let resource: [UnitType; 14] = [
        UnitType::MaterialStorage,
        UnitType::MiningStation,
        UnitType::Constructor,
        UnitType::MineLayer,
        UnitType::SupplyTruck,
        UnitType::Engineer,
        UnitType::Bulldozer,
        UnitType::RepairUnit,
        UnitType::SeaMineLayer,
        UnitType::CargoShip,
        UnitType::FuelStorage,
        UnitType::FuelTruck,
        UnitType::GoldVault,
        UnitType::GoldTruck,
    ];
    let transport: [UnitType; 8] = [
        UnitType::Barracks,
        UnitType::Depot,
        UnitType::ArmouredPersonnelCarrier,
        UnitType::SeaTransport,
        UnitType::AirTransport,
        UnitType::Hangar,
        UnitType::LandingPad,
        UnitType::Dock,
    ];

    let mut counts = [0usize; 3];
    for &unit_type in UnitType::ALL {
        let expected = if resource.contains(&unit_type) {
            CargoKind::Resource
        } else if transport.contains(&unit_type) {
            CargoKind::Unit
        } else {
            CargoKind::None
        };

        assert_eq!(
            CargoKind::classify(unit_type),
            expected,
                "{unit_type:?}: cargo_type = {:?}",
            cargo_type_of(unit_type)
        );
        assert_eq!(unit_type.cargo_kind(), expected, "{unit_type:?}");

        counts[match expected {
            CargoKind::None => 0,
            CargoKind::Resource => 1,
            CargoKind::Unit => 2,
        }] += 1;
    }

    // None=71, Resource=14, Unit=8.
    assert_eq!(counts, [71, 14, 8]);

    // Grouping of `CARGO_TYPE_*` — all 7 C++ values.
    assert_eq!(CargoKind::from_cargo_type(CargoType::None), CargoKind::None);
    for cargo_type in [CargoType::Raw, CargoType::Fuel, CargoType::Gold] {
        assert_eq!(
            CargoKind::from_cargo_type(cargo_type),
            CargoKind::Resource,
                "{cargo_type:?}"
        );
    }
    for cargo_type in [CargoType::Land, CargoType::Sea, CargoType::Air] {
        assert_eq!(
            CargoKind::from_cargo_type(cargo_type),
            CargoKind::Unit,
                "{cargo_type:?}"
        );
    }
}

#[test]
fn documented_control_cases() {
    // Scout — amphibious combat.
    assert_eq!(Mobility::classify(UnitType::Scout), Mobility::Mobile);
    assert_eq!(
        Environment::classify(UnitType::Scout),
        Some(Environment::Amphibious)
    );
    assert_eq!(CombatRole::classify(UnitType::Scout), CombatRole::Combat);

    // Awac — air non-combat.
    assert_eq!(
        Environment::classify(UnitType::Awac),
        Some(Environment::Air)
    );
    assert_eq!(CombatRole::classify(UnitType::Awac), CombatRole::NonCombat);

    // GunTurret — a building by the owner's decision, although `BUILDING` is cleared.
    assert!(!has_flag(UnitType::GunTurret, BUILDING));
    assert_eq!(Mobility::classify(UnitType::GunTurret), Mobility::Building);
    assert_eq!(
        CombatRole::classify(UnitType::GunTurret),
        CombatRole::Combat
    );

    // LandMine — a stationary combat unit (not a building).
    assert_eq!(Mobility::classify(UnitType::LandMine), Mobility::Stationary);
    assert_eq!(CombatRole::classify(UnitType::LandMine), CombatRole::Combat);

    // Production: the gold refinery — yes, the barracks — no.
    assert!(UnitType::GoldRefinery.is_producing());
    assert!(!UnitType::Barracks.is_producing());
}

#[test]
fn instance_classification_matches_type_when_flags_are_copied() {
    for &unit_type in UnitType::ALL {
        let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId::NO_ID);
        unit.flags = unit_type_flags(unit_type);

        assert_eq!(unit.combat_role(), CombatRole::classify(unit_type));
        assert_eq!(unit.mobility(), Mobility::classify(unit_type));
        assert_eq!(unit.environment(), Environment::classify(unit_type));
        assert_eq!(unit.cargo_kind(), CargoKind::classify(unit_type));
        assert_eq!(unit.is_producing(), unit_type.is_producing());
    }
}

#[test]
fn instance_classification_follows_instance_flags() {
    // A ground tank.
    let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::NO_ID);
    tank.flags = unit_type_flags(UnitType::Tank);
    assert_eq!(tank.mobility(), Mobility::Mobile);
    assert_eq!(tank.environment(), Some(Environment::Ground));

    // Runtime: air flags instead of ground ones — the instance is classified
    // by its own flags, the type table does not participate.
    tank.flags = (tank.flags & !(MOBILE_SEA_UNIT | MOBILE_LAND_UNIT)) | MOBILE_AIR_UNIT;
    assert_eq!(tank.mobility(), Mobility::Mobile);
    assert_eq!(tank.environment(), Some(Environment::Air));

    // `HOVERING` is a flight state, not an environment.
    tank.flags |= HOVERING;
    assert_eq!(tank.environment(), Some(Environment::Air));

    // The building has `BUILDING`/`STATIONARY` dynamically cleared — the instance is
    // "mobile", although the type remains a building.
    let mut barracks = UnitInfo::new(UnitType::Barracks, TeamId::RED, UnitId::NO_ID);
    barracks.flags = unit_type_flags(UnitType::Barracks);
    assert_eq!(barracks.mobility(), Mobility::Building);
    barracks.flags &= !(BUILDING | STATIONARY);
    assert_eq!(barracks.mobility(), Mobility::Mobile);
    assert_eq!(barracks.environment(), None);
    assert_eq!(Mobility::classify(UnitType::Barracks), Mobility::Building);

    // Instance combat role — by its `base_values` (upgrades/clan).
    let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::NO_ID);
    assert_eq!(tank.combat_role(), CombatRole::Combat);
    tank.base_values.attack = 0;
    assert_eq!(tank.combat_role(), CombatRole::NonCombat);
    assert_eq!(CombatRole::classify(UnitType::Tank), CombatRole::Combat);

    // Production and cargo are type properties; instance flags do not change them.
    let mut refinery = UnitInfo::new(UnitType::GoldRefinery, TeamId::RED, UnitId::NO_ID);
    refinery.flags = 0;
    assert!(refinery.is_producing());
    assert_eq!(refinery.cargo_kind(), CargoKind::None);

    let mut barracks = UnitInfo::new(UnitType::Barracks, TeamId::RED, UnitId::NO_ID);
    barracks.flags = 0;
    assert_eq!(barracks.cargo_kind(), CargoKind::Unit);
}

#[test]
fn ergonomic_helpers_agree_with_axes_for_all_93_types() {
    let mut environment_counts = [0usize; 4];
    let mut mobile = 0;

    for &unit_type in UnitType::ALL {
        let environment = unit_type.environment();
        let mobility = unit_type.mobility();

        assert_eq!(unit_type.is_air(), environment == Some(Environment::Air));
        assert_eq!(
            unit_type.is_ground(),
            environment == Some(Environment::Ground)
        );
        assert_eq!(unit_type.is_sea(), environment == Some(Environment::Sea));
        assert_eq!(
            unit_type.is_amphibious(),
            environment == Some(Environment::Amphibious)
        );
        assert_eq!(unit_type.is_mobile(), mobility == Mobility::Mobile);
        assert_eq!(unit_type.is_building(), mobility == Mobility::Building);
        assert_eq!(unit_type.is_stationary(), mobility == Mobility::Stationary);
        assert_eq!(
            unit_type.is_combat(),
            unit_type.combat_role() == CombatRole::Combat
        );

        // Environments are mutually exclusive: at most one at a time.
        let flags = [
            unit_type.is_air(),
            unit_type.is_ground(),
            unit_type.is_sea(),
            unit_type.is_amphibious(),
        ];
        assert!(
            flags.iter().filter(|set| **set).count() <= 1,
                "{unit_type:?}"
        );

        if let Some(environment) = environment {
            environment_counts[match environment {
                Environment::Air => 0,
                Environment::Ground => 1,
                Environment::Sea => 2,
                Environment::Amphibious => 3,
            }] += 1;
            assert!(unit_type.is_mobile(), "{unit_type:?}");
        }

        if mobility == Mobility::Mobile {
            mobile += 1;
        }
    }

    assert_eq!(environment_counts, [5, 17, 9, 5]);
    assert_eq!(mobile, 43);

    // The `STATIONARY` bit is equivalent to `!is_mobile()` if the
    // invariant `BUILDING ⟹ STATIONARY` holds.
    for &unit_type in UnitType::ALL {
        assert_eq!(
            !unit_type.is_mobile(),
            has_flag(unit_type, STATIONARY),
                "{unit_type:?}"
        );
    }

    // A turret is a building by the owner's decision, but its `BUILDING` bit is cleared:
    // these are different questions (`is_building()` ≠ `flags & BUILDING`).
    assert!(UnitType::GunTurret.is_building());
    assert!(!has_flag(UnitType::GunTurret, BUILDING));
}

#[test]
fn instance_helpers_follow_instance_flags() {
    let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::NO_ID);
    tank.flags = unit_type_flags(UnitType::Tank);
    assert!(!tank.is_air());
    assert!(tank.is_ground());
    assert!(tank.is_mobile());
    assert!(!tank.is_building());
    assert!(tank.is_combat());

    // Cleared `STATIONARY`/`BUILDING` on a building instance — the instance is
    // mobile, the type remains a building.
    let mut barracks = UnitInfo::new(UnitType::Barracks, TeamId::RED, UnitId::NO_ID);
    barracks.flags = unit_type_flags(UnitType::Barracks);
    assert!(barracks.is_building());
    assert!(!barracks.is_mobile());
    barracks.flags &= !(BUILDING | STATIONARY);
    assert!(!barracks.is_building());
    assert!(barracks.is_mobile());

    // Air flags instead of ground ones — the instance is "air".
    tank.flags = (tank.flags & !(MOBILE_SEA_UNIT | MOBILE_LAND_UNIT)) | MOBILE_AIR_UNIT;
    assert!(tank.is_air());
    assert!(!tank.is_ground());
}

// -----------------------------------------------------------------------
// Object kind (F4): unit / effect / decoration / plot decoration
// -----------------------------------------------------------------------

/// Effect tokens — verbatim from `object-classifier.md`, "Non-units".
const EFFECT_TOKENS: [&str; 14] = [
        "ROCKET", "TORPEDO", "ALNMISSL", "ALNTBALL", "ALNABALL", "LNDEXPLD", "AIREXPLD",
        "SEAEXPLD", "BLDEXPLD", "HITEXPLD", "LRGRUBLE", "SMLRUBLE", "RKTSMOKE", "TRPBUBLE",
];

/// Decoration tokens — verbatim from `object-classifier.md`, "Non-units".
const DECORATION_TOKENS: [&str; 6] = [
        "LRGTAPE", "SMLTAPE", "LRGSLAB", "SMLSLAB", "LRGCONES", "SMLCONES",
];

/// Plot decoration tokens — verbatim from `object-classifier.md`.
const PLOT_PROP_TOKENS: [&str; 3] = ["MASTER", "HARVSTER", "WALDO"];

/// Independent oracle: the expected kind by resource token [`UnitType::tag`].
fn expected_object_kind(unit_type: UnitType) -> ObjectKind {
    let tag = unit_type.tag();

    if EFFECT_TOKENS.contains(&tag) {
        ObjectKind::Effect
    } else if DECORATION_TOKENS.contains(&tag) {
        ObjectKind::Decoration
    } else if PLOT_PROP_TOKENS.contains(&tag) {
        ObjectKind::PlotProp
    } else {
        ObjectKind::Unit
    }
}

#[test]
fn object_kind_is_total_bijection_over_93_types() {
    let mut counts = [0usize; 4];

    for &unit_type in UnitType::ALL {
        let expected = expected_object_kind(unit_type);
        let actual = unit_type.object_kind();

        assert_eq!(
            actual,
            expected,
                "{:?}: tag = {}, flags = {:#010X}",
            unit_type,
            unit_type.tag(),
            unit_type_flags(unit_type)
        );
        assert_eq!(ObjectKind::classify(unit_type), expected, "{unit_type:?}");

        counts[actual as usize] += 1;
    }

    // 70 units + 14 effects + 6 decorations + 3 plot props = 93 (a total
    // bijection, no gaps or duplicates).
    assert_eq!(counts, [70, 14, 6, 3]);
    assert_eq!(counts.iter().sum::<usize>(), UnitType::ALL.len());
    assert_eq!(counts.iter().sum::<usize>(), 93);

    // Exception lists — without duplicates and exactly the declared length.
    assert_eq!(EFFECT_UNIT_TYPES.len(), 14);
    assert_eq!(DECORATION_UNIT_TYPES.len(), 6);
    assert_eq!(PLOT_PROP_UNIT_TYPES.len(), 3);

    for (name, list) in [
        ("EFFECT_UNIT_TYPES", &EFFECT_UNIT_TYPES[..]),
        ("DECORATION_UNIT_TYPES", &DECORATION_UNIT_TYPES[..]),
        ("PLOT_PROP_UNIT_TYPES", &PLOT_PROP_UNIT_TYPES[..]),
    ] {
        for (index, unit_type) in list.iter().enumerate() {
            assert_eq!(
                list.iter().filter(|other| *other == unit_type).count(),
                1,
                    "{name}: дубль {unit_type:?} (позиция {index})"
            );
            assert!(
                UnitType::ALL.contains(unit_type),
                    "{name}: {unit_type:?} вне `UnitType::ALL`"
            );
        }
    }

    // Unit range boundaries by token: `COMMTWR`..`ALNPLANE`.
    assert_eq!(UnitType::GoldRefinery.tag(), "COMMTWR");
    assert_eq!(UnitType::AlienPlane.tag(), "ALNPLANE");
    assert_eq!(UnitType::GoldRefinery.object_kind(), ObjectKind::Unit);
    assert_eq!(UnitType::AlienPlane.object_kind(), ObjectKind::Unit);
}

#[test]
fn object_kind_variants_are_mutually_exclusive() {
    // The kinds differ: `Unit` is not equal to the others.
    for other in [
        ObjectKind::Effect,
        ObjectKind::Decoration,
        ObjectKind::PlotProp,
    ] {
        assert_ne!(ObjectKind::Unit, other, "{other:?}");
        assert!(!other.is_unit(), "{other:?}");
    }
    assert!(ObjectKind::Unit.is_unit());
    assert!(!ObjectKind::Unit.is_effect());
    assert!(!ObjectKind::Unit.is_decoration());
    assert!(!ObjectKind::Unit.is_plot_prop());

    // Exactly one helper fires for each of the 93 types.
    for &unit_type in UnitType::ALL {
        let kind = unit_type.object_kind();
        let fired = [
            unit_type.is_unit(),
            unit_type.is_effect(),
            unit_type.is_decoration(),
            unit_type.is_plot_prop(),
        ];

        assert_eq!(
            fired.iter().filter(|set| **set).count(),
            1,
                "{unit_type:?}: {fired:?}"
        );
        assert_eq!(
            unit_type.is_unit(),
            kind == ObjectKind::Unit,
                "{unit_type:?}"
        );
        assert_eq!(
            unit_type.is_effect(),
            kind == ObjectKind::Effect,
                "{unit_type:?}"
        );
        assert_eq!(
            unit_type.is_decoration(),
            kind == ObjectKind::Decoration,
                "{unit_type:?}"
        );
        assert_eq!(
            unit_type.is_plot_prop(),
            kind == ObjectKind::PlotProp,
                "{unit_type:?}"
        );
        assert_eq!(kind.is_unit(), kind == ObjectKind::Unit, "{unit_type:?}");
    }
}

#[test]
fn object_kind_control_cases() {
    // (c) Units: a producing building, the mine, a combat mobile.
    for unit_type in [
        UnitType::GoldRefinery,
        UnitType::MiningStation,
        UnitType::Tank,
    ] {
        assert_eq!(
            unit_type.object_kind(),
            ObjectKind::Unit,
                "{unit_type:?}: {}",
            unit_type.tag()
        );
        assert!(unit_type.is_unit(), "{unit_type:?}");
    }

    // Effects: explosion, trail, rubble.
    for effect in [
        UnitType::LandExplosion,
        UnitType::RocketSmoke,
        UnitType::LargeRubble,
    ] {
        assert_eq!(
            effect.object_kind(),
            ObjectKind::Effect,
                "{effect:?}: {}",
            effect.tag()
        );
        assert!(effect.is_effect(), "{effect:?}");
        assert!(!effect.is_unit(), "{effect:?}");
    }

    // Decoration: a 2×2 build site.
    assert_eq!(UnitType::LargeTape.object_kind(), ObjectKind::Decoration);
    assert!(UnitType::LargeTape.is_decoration());
    assert!(!UnitType::LargeTape.is_unit());

    // Plot decorations: `MASTER`, `HARVSTER`, `WALDO`.
    for plot_prop in [
        UnitType::MasterBuilder,
        UnitType::Harvester,
        UnitType::DeadWaldo,
    ] {
        assert_eq!(
            plot_prop.object_kind(),
            ObjectKind::PlotProp,
                "{plot_prop:?}: {}",
            plot_prop.tag()
        );
        assert!(plot_prop.is_plot_prop(), "{plot_prop:?}");
        assert!(!plot_prop.is_unit(), "{plot_prop:?}");
    }
}

#[test]
fn object_kind_discriminants_are_stable() {
    // Stable serialization tags — explicit discriminants, not order.
    assert_eq!(ObjectKind::Unit as u8, 0);
    assert_eq!(ObjectKind::Effect as u8, 1);
    assert_eq!(ObjectKind::Decoration as u8, 2);
    assert_eq!(ObjectKind::PlotProp as u8, 3);
}
