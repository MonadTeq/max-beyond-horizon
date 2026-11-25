use super::*;

#[test]
fn empty_state_builds_a_parsable_save() {
    let state = GameState::new();
    let save = state.to_save();
    let bytes = save.to_bytes().expect("writes");
    let reparsed = SaveFile::parse(&bytes).expect("parses");
    assert_eq!(reparsed.header.version, crate::save::SAVE_VERSION_V70);
    assert_eq!(reparsed.units.len(), 0);
    assert_eq!(reparsed.game.turn_counter, 0);
    assert_eq!(reparsed.team_units[0].base_values.len(), UNIT_COUNT);
    // The "model -> file -> model" cycle without units is stable.
    assert_eq!(reparsed.to_bytes().expect("writes"), bytes);
}

#[test]
fn layout_numbers_objects_in_first_appearance_order() {
    let mut layout = Layout::new();
    let first = layout.object(ObjectKey::TeamValues {
        team: 0,
        base: true,
        unit_type: 0,
    });
    let second = layout.object(ObjectKey::TeamComplex { team: 0, id: 1 });
    assert_eq!((first, second), (1, 2));
    assert_eq!(
        layout.object(ObjectKey::TeamValues {
            team: 0,
            base: true,
            unit_type: 0
        }),
        1
    );
    assert_eq!(layout.object(ObjectKey::Unit { id: 7 }), 3);
    assert_eq!(layout.unit(Some(UnitId(7))), 3);
    assert_eq!(layout.unit(Some(UnitId(8))), 0);
    assert_eq!(layout.unit(None), 0);
}

#[test]
fn heat_values_are_clamped_to_int8() {
    assert_eq!(heat_value(0), 0);
    assert_eq!(heat_value(12), 12);
    assert_eq!(heat_value(127), 127);
    assert_eq!(heat_value(1000), 127);
}

/// Writer object order: numbers are consecutive in traversal order.
///
/// Catches the "base and current values mixed up" regression (C++ writes
/// all base `UnitValues` first, then all current ones), which made
/// `SaveFile::to_bytes` fail with `ForwardReference`.
#[test]
fn object_numbers_follow_writer_order() {
    let save = crate::save::SaveFile::parse(include_bytes!("../tests/data/save1_v70.cam"))
        .expect("эталон разбирается");
    let built = GameState::from_save(&save).to_save();

    let mut next = 1u16;
    for (team, units) in built.team_units.iter().enumerate() {
        for (index, slot) in units.base_values.iter().enumerate() {
            assert_eq!(
                slot.object_index, next,
                    "team {team}: базовое значение {index} должно идти подряд"
            );
            next += 1;
        }
        for (index, slot) in units.current_values.iter().enumerate() {
            if slot.object_index >= next {
                assert_eq!(
                    slot.object_index, next,
                        "team {team}: текущее значение {index} должно идти подряд"
                );
                next += 1;
            }
        }
        for (index, slot) in units.complexes.iter().enumerate() {
            assert_eq!(
                slot.object_index, next,
                    "team {team}: комплекс {index} должен идти подряд"
            );
            next += 1;
        }
    }
    // Unit numbers are unique, come after team objects, and together with
    // all nested objects form a continuous range 1..=N
    // (required by `SmartFileWriter`).
    let mut indices: Vec<u16> = Vec::new();
    for units in &built.team_units {
        for slot in units.base_values.iter().chain(units.current_values.iter()) {
            indices.push(slot.object_index);
        }
        for slot in &units.complexes {
            indices.push(slot.object_index);
        }
    }
    for unit in &built.units {
        indices.extend([
            unit.object_index,
            unit.path_object_index,
            unit.base_values_object_index,
            unit.complex_object_index,
            unit.parent_object_index,
            unit.enemy_object_index,
        ]);
    }
    indices.retain(|index| *index != 0);
    indices.sort_unstable();
    indices.dedup();
    assert_eq!(
        indices,
        (1..=indices.len() as u16).collect::<Vec<_>>(),
            "объектные номера должны быть непрерывны"
    );
}

/// `SaveMeta::default` matches the V70 initialization of a new game: four
/// 512-bucket tables, `Custom` file type, no RNG seed, snow planet.
#[test]
fn save_meta_defaults_match_v70() {
    let meta = SaveMeta::default();
    assert_eq!(meta.save_game_type, FileType::Custom);
    assert_eq!(meta.world, Planet::Snowcrab);
    assert_eq!(meta.rng_seed, 0);
    assert_eq!(meta.opponent, Opponent::Average);
    assert_eq!(meta.unit_hash_size, SAVE_UNIT_HASH_SIZE);
    assert_eq!(meta.map_hash_size, SAVE_UNIT_HASH_SIZE);
    assert_eq!(meta.map_hash_x_shift, 2);
    assert_eq!(meta.alien_team_type, TeamKind::None);
    assert_eq!(meta.alien_team_clan, TeamClan::Random);
    assert_eq!(meta.turn_timer, 0);
    assert!(meta.unit_hash_order.is_empty());
    assert!(meta.map_hash_cells.is_empty());
    assert!(meta.save_name().is_empty());
    assert!(meta.team_name(0).is_empty());
    assert!(meta.team_name(9).is_empty());
    assert!(meta.message_logs.iter().all(Vec::is_empty));
}

/// `Layout` numbers objects from 1 and saturates at `u16::MAX` instead of
/// wrapping (a game cannot reach 65535 objects, but the counter must not
/// silently reuse a number).
#[test]
fn layout_numbers_saturate_at_u16_max() {
    let mut layout = Layout::new();
    for id in 0..u16::MAX {
        let index = layout.object(ObjectKey::Unit { id });
        assert_eq!(index, id.wrapping_add(1), "unit {id}");
        // Re-requesting an existing key returns the same number.
        assert_eq!(layout.object(ObjectKey::Unit { id }), index);
    }
    let last = layout.object(ObjectKey::Unit { id: u16::MAX });
    assert_eq!(last, u16::MAX);
    // The counter is saturated: a further object reuses the last number
    // (the writer then rejects the file with `ForwardReference`, it never
    // wraps around to 1).
    assert_eq!(layout.object(ObjectKey::Unit { id: 0 }), 1);
    assert_eq!(
        layout.object(ObjectKey::Unit {
            id: u16::MAX - 1
        }),
        u16::MAX
    );
    assert_eq!(
        layout.object(ObjectKey::Unit {
            id: u16::MAX - 2
        }),
        u16::MAX - 1
    );
}
