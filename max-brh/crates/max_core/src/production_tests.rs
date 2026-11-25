use super::*;
use crate::classification::Footprint;

#[test]
fn turret_layout_is_one_by_one() {
    // A turret is a building semantically, but `Footprint::Single`: the site,
    // tape/cones and slab are small (1×1), the size is one cell.
    for turret in [
        UnitType::GunTurret,
        UnitType::Artillery,
        UnitType::AntiAir,
        UnitType::MissileLauncher,
    ] {
        assert_eq!(turret.footprint(), Footprint::Single, "{turret:?}");
        assert!(turret.is_building(), "{turret:?}");
        assert_eq!(footprint(turret), 1, "{turret:?}");
        assert_eq!(
            construction_utility_types(turret),
            (UnitType::SmallTape, UnitType::SmallCones),
                "{turret:?}"
        );
        assert_eq!(
            construction_slab_type(turret),
            UnitType::SmallSlab,
                "{turret:?}"
        );
    }

    // For contrast: a 2×2 building — large utilities and slab.
    assert_eq!(footprint(UnitType::MiningStation), 2);
    assert_eq!(
        construction_utility_types(UnitType::MiningStation),
        (UnitType::LargeTape, UnitType::LargeCones)
    );
    assert_eq!(
        construction_slab_type(UnitType::MiningStation),
        UnitType::LargeSlab
    );
}

#[test]
fn turret_bounds_cover_exactly_one_cell() {
    let mut turret = UnitInfo::new(UnitType::GunTurret, TeamId::RED, UnitId::NO_ID);
    turret.flags = crate::access::unit_type_flags(UnitType::GunTurret);
    turret.grid = Point::new(10, 10);
    let mut mine = UnitInfo::new(UnitType::MiningStation, TeamId::RED, UnitId::NO_ID);
    mine.flags = crate::access::unit_type_flags(UnitType::MiningStation);
    mine.grid = Point::new(10, 10);

    assert!(unit_bounds_contains(&turret, Point::new(10, 10)));
    assert!(!unit_bounds_contains(&turret, Point::new(11, 10)));
    assert!(!unit_bounds_contains(&turret, Point::new(10, 11)));

    assert!(unit_bounds_contains(&mine, Point::new(10, 10)));
    assert!(unit_bounds_contains(&mine, Point::new(11, 11)));
}

#[test]
fn builder_tables_are_consistent() {
    for (builder, units) in BUILDABLE {
        assert!(!units.is_empty(), "{builder:?} row is empty");
        assert!(is_builder(builder), "{builder:?} row must be a builder");
        assert_eq!(buildable_units(builder), units);

        for unit_type in units {
            assert!(can_build(builder, *unit_type));
            // No type is listed in two rows: the first match is the only match.
            assert_eq!(
                builder_type(*unit_type),
                Some(builder),
                    "{unit_type:?} builder"
            );
        }
    }

    // An ordinary unit cannot build anything.
    assert!(buildable_units(UnitType::Tank).is_empty());
    assert!(!is_builder(UnitType::Tank));
    assert!(!is_mobile_builder(UnitType::Tank));
    assert!(!can_build(UnitType::Tank, UnitType::Scout));
    assert_eq!(builder_type(UnitType::Tank), Some(UnitType::HeavyVehiclePlant));

    // The factory list is exactly the stationary producers.
    for factory in FACTORIES {
        assert!(is_factory(factory), "{factory:?}");
        assert!(is_builder(factory), "{factory:?}");
        assert!(!is_mobile_builder(factory), "{factory:?}");
    }
    for builder in MOBILE_BUILDERS {
        assert!(is_mobile_builder(builder), "{builder:?}");
        assert!(is_builder(builder), "{builder:?}");
        assert!(!is_factory(builder), "{builder:?}");
    }
}

#[test]
fn build_error_messages_are_complete_and_distinct() {
    let errors = [
        BuildError::NotABuilder,
        BuildError::NotBuildable,
        BuildError::EmptyQueue,
        BuildError::NoSite,
        BuildError::InvalidTeam,
        BuildError::NotEnoughMaterials,
        BuildError::NotEnoughPower,
        BuildError::NotEnoughFuel,
        BuildError::NotEnoughWorkers,
        BuildError::NotEnoughGold,
    ];

    let messages: Vec<&str> = errors.iter().map(|error| error.message()).collect();
    for (index, message) in messages.iter().enumerate() {
        assert!(!message.is_empty(), "{:?} message", errors[index]);
        for other in &messages[index + 1..] {
            assert_ne!(message, other, "duplicate message {message:?}");
        }
    }
}

#[test]
fn cost_helpers_follow_the_cpp_rates() {
    // `Cargo_GetRawConsumptionRate` (cargo.cpp:140): CONSTRCT/ENGINEER 2 per turn,
    // TRAINHAL 1, factories 3; the rate multiplier maps 2× → 4 and 4× → 12.
    assert_eq!(cost_per_turn(UnitType::Constructor, 1), 2);
    assert_eq!(cost_per_turn(UnitType::Engineer, 1), 2);
    assert_eq!(cost_per_turn(UnitType::TrainingHall, 1), 1);
    assert_eq!(cost_per_turn(UnitType::LightVehiclePlant, 1), 3);
    assert_eq!(cost_per_turn(UnitType::HeavyVehiclePlant, 1), 3);
    assert_eq!(cost_per_turn(UnitType::AirUnitsPlant, 1), 3);
    assert_eq!(cost_per_turn(UnitType::Shipyard, 1), 3);
    assert_eq!(cost_per_turn(UnitType::TrainingHall, 2), 4);
    assert_eq!(cost_per_turn(UnitType::TrainingHall, 4), 12);
    assert_eq!(cost_per_turn(UnitType::Tank, 1), 0, "a unit is not a builder");

    assert_eq!(total_cost(UnitType::Constructor, 10), 20);
    assert_eq!(total_cost(UnitType::TrainingHall, 3), 3);
    assert_eq!(total_cost(UnitType::Constructor, 0), 0);
    assert_eq!(total_cost(UnitType::Constructor, -5), 0);
}

#[test]
fn max_possible_build_rate_boundaries() {
    // `build_time <= 1` always returns 1 (`buildmenu.cpp:733`).
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 0, 100), 1);
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 1, 100), 1);
    // An empty storage cannot pay for a 2× pair.
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 2, 0), 1);
    assert_eq!(max_possible_build_rate(UnitType::Constructor, 2, 100), 2);
    // A stationary builder is passed `INT16_MAX` for storage, so the cap does not apply.
    assert_eq!(max_possible_build_rate(UnitType::TrainingHall, 10, 0), 4);
    assert_eq!(max_possible_build_rate(UnitType::Shipyard, 10, 0), 4);
    // Air units are not ground/amphibious: storage is ignored as well.
    assert_eq!(max_possible_build_rate(UnitType::AirUnitsPlant, 10, 0), 4);
}

#[test]
fn pause_event_kind_covers_every_shortage() {
    use crate::power::Shortage;

    assert_eq!(pause_event_kind(Shortage::Raw), ProductionEventKind::PausedNoMaterials);
    assert_eq!(pause_event_kind(Shortage::Power), ProductionEventKind::PausedNoPower);
    assert_eq!(pause_event_kind(Shortage::Workers), ProductionEventKind::PausedNoWorkers);
    assert_eq!(pause_event_kind(Shortage::Fuel), ProductionEventKind::PausedNoFuel);
    assert_eq!(pause_event_kind(Shortage::Gold), ProductionEventKind::PausedNoGold);
}

#[test]
fn turns_and_consumption_terminates_on_degenerate_inputs() {
    let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId::NO_ID);
    builder.flags = crate::access::unit_type_flags(UnitType::Constructor);
    builder.grid = Point::new(10, 10);

    // Zero turns: nothing to pay, no turns needed.
    let mut empty = builder.clone();
    empty.build_time = 0;
    empty.build_list.clear();
    let state = crate::game::GameState::new();
    let (turns, consumption) =
        turns_and_consumption(&state, &empty, UnitType::TrainingHall, 1);
    assert_eq!(turns, 0, "the team table has no ATTRIB_TURNS for a new state");
    assert_eq!(consumption, 0);
}
