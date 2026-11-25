use super::*;
use crate::enums::TeamClan;
use crate::unit_values::base_unit_values;

#[test]
fn team_info_reset_matches_cpp() {
    let info = TeamInfo::new();
    assert_eq!(info.team_type, TeamKind::None);
    assert_eq!(info.team_clan, TeamClan::Random);
    assert!(info.unit_counters.iter().all(|&value| value == 1));
    assert!(info.casualties.iter().all(|&value| value == 0));
    assert_eq!(info.unit_counter(0), 1);
}

#[test]
fn research_turns_uses_ceiling() {
    let mut topic = ResearchTopicInfo {
        research_level: 0,
        turns_to_complete: 7,
        allocation: 1,
    };
    assert_eq!(topic.turns_remaining(), 7);
    topic.allocation = 2;
    assert_eq!(topic.turns_remaining(), 4);
    topic.allocation = 7;
    assert_eq!(topic.turns_remaining(), 1);
    topic.allocation = 0;
    assert_eq!(topic.turns_remaining(), 0);
    assert!(topic.is_started());
    topic.turns_to_complete = 0;
    assert!(!topic.is_started());
}

#[test]
fn complexes_are_created_in_id_order() {
    let mut units = TeamUnits::with_clan(TeamClan::TheChosen);
    assert_eq!(units.create_complex(), ComplexId(1));
    assert_eq!(units.create_complex(), ComplexId(2));
    assert_eq!(units.create_complex(), ComplexId(3));
    units.remove_complex(ComplexId(2));
    // C++ reuses the lowest free id.
    assert_eq!(units.create_complex(), ComplexId(2));
    let ids: Vec<i16> = units.complexes().iter().map(|complex| complex.id).collect();
    assert_eq!(ids, vec![1, 2, 3]);
    assert!(units.get_complex(ComplexId(3)).is_some());
    units.clear_complexes();
    assert!(units.complexes().is_empty());
}

#[test]
fn complex_grow_shrink() {
    let mut complex = Complex::new(1);
    complex.grow();
    complex.grow();
    assert_eq!(complex.buildings, 2);
    assert!(!complex.shrink());
    assert!(complex.shrink());
    assert_eq!(complex.buildings, 0);
}

#[test]
fn team_units_initialized_from_clan_table() {
    let units = TeamUnits::with_clan(TeamClan::Musashi);
    // Musashi: Tank +4 attack, +2 armor, +4 hits, +1 speed.
    let tank = units.base_unit_values(crate::enums::UnitType::Tank as u16);
    let plain = base_unit_values(crate::enums::UnitType::Tank);
    assert_eq!(tank.attack, plain.attack + 4);
    assert_eq!(tank.armor, plain.armor + 2);
    assert_eq!(units.current_values, units.base_values);
    // Musashi: Gun Turret +4 attack, +2 armor, +4 hits.
    let turret = units.base_unit_values(crate::enums::UnitType::GunTurret as u16);
    let plain_turret = base_unit_values(crate::enums::UnitType::GunTurret);
    assert_eq!(turret.attack, plain_turret.attack + 4);
    assert_eq!(turret.armor, plain_turret.armor + 2);
    // A unit the clan does not touch stays as in the table.
    assert_eq!(
        units.base_unit_values(crate::enums::UnitType::Radar as u16),
        &base_unit_values(crate::enums::UnitType::Radar)
    );
    // For an empty clan the table matches the base one.
    let random = TeamUnits::with_clan(TeamClan::Random);
    assert_eq!(random.base_values, crate::unit_values::BASE_UNIT_VALUES);
}

/// `ResourceManager_InitClanUnitValues` rebuilds the base from `SC_ATTRI`
/// first, so a repeated call (or a clan change) never stacks deltas and
/// `ClearComplexes()` drops the team complexes.
#[test]
fn clan_initialization_is_idempotent_and_clears_complexes() {
    let mut units = TeamUnits::with_clan(TeamClan::Musashi);
    units.create_complex();
    assert!(!units.complexes().is_empty());

    units.init_clan_unit_values(TeamClan::VonGriffin);
    assert!(units.complexes().is_empty(), "ClearComplexes");
    let expected = crate::unit_values::team_base_values(TeamClan::VonGriffin);
    assert_eq!(units.base_values, expected);
    assert_eq!(units.current_values, expected);

    // A second identical call does not add the trade-offs twice.
    units.init_clan_unit_values(TeamClan::VonGriffin);
    assert_eq!(units.base_values, expected);
    assert_eq!(units.current_values, expected);

    // Switching back to a plain clan removes the previous modifiers.
    units.init_clan_unit_values(TeamClan::Random);
    assert_eq!(units.base_values, crate::unit_values::BASE_UNIT_VALUES);
    assert_eq!(units.current_values, crate::unit_values::BASE_UNIT_VALUES);
}

#[test]
fn ui_defaults_match_v70_initialization() {
    let ui = TeamInfoUi::default();
    for marker in ui.markers {
        assert_eq!(marker, Point::new(-1, -1));
    }
    for location in ui.screen_locations {
        assert_eq!((location.x, location.y), (-1, -1));
    }
    assert_eq!(ui.selected_unit, None);
    assert_eq!(ui.display_buttons, [0; DISPLAY_BUTTON_COUNT]);
}

/// `TeamUnits::SetGold` stores the full `u32` (V70 narrows it on disk).
#[test]
fn gold_boundaries() {
    let mut units = TeamUnits::new();
    assert_eq!(units.gold(), 0);
    units.set_gold(u32::MAX);
    assert_eq!(units.gold(), u32::MAX);
    units.set_gold(0);
    assert_eq!(units.gold(), 0);
}

/// `TeamInfo` per-type counters and casualty accessors cover all 93 slots.
#[test]
fn team_info_type_indexed_accessors() {
    let mut info = TeamInfo::new();
    let last = crate::unit_values::UNIT_COUNT as u16 - 1;
    assert_eq!(info.unit_counter(0), 1);
    assert_eq!(info.unit_counter(last), 1);
    assert_eq!(info.casualties_for(0), 0);
    assert_eq!(info.casualties_for(last), 0);
    info.unit_counters[last as usize] = 77;
    info.casualties[0] = 5;
    assert_eq!(info.unit_counter(last), 77);
    assert_eq!(info.casualties_for(0), 5);
}

/// Complex lookup/removal semantics of `TeamUnits`.
#[test]
fn complex_lookup_and_remove_semantics() {
    let mut units = TeamUnits::new();
    let id = units.create_complex();
    assert!(units.get_complex(id).is_some());
    units
        .get_complex_mut(id)
        .expect("complex")
        .buildings = 3;
    assert_eq!(units.get_complex(id).map(|complex| complex.buildings), Some(3));
    assert!(units.remove_complex(id));
    assert!(!units.remove_complex(id));
    assert!(units.get_complex(id).is_none());
    assert!(units.get_complex_mut(id).is_none());
}
