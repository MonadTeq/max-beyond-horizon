use super::*;
use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::unit_values::{base_unit_values, ATTRIB_ATTACK};

/// A real V70 save from `max-original/SAVE1.CAM` (a copy is in `tests/data/`).
const SAVE_FIXTURE: &[u8] = include_bytes!("../tests/data/save1_v70.cam");

/// The parser resolves the object indices of the real V70 save: the first unit
/// of the ground-cover list is a GREEN `WTRPLTFM` awaiting an order and carries
/// a base-values object. The fixture contract itself (header values, all
/// sections, round-trip) is pinned in `tests/v70_save.rs`.
#[test]
fn parses_real_v70_save_unit_objects() {
    let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");

    // The first unit of the ground cover list: type WTRPLTFM (33), team GREEN,
    // order state `AWAIT/EXECUTING_ORDER`.
    let first = save.unit_by_object_index(save.unit_lists[0][0]).unwrap();
    assert_eq!(first.info.unit_type, UnitType::WaterPlatform);
    assert_eq!(first.info.team, TeamId::GREEN);
    assert_eq!(first.info.orders, UnitOrder::Await);
    assert_eq!(first.info.state, UnitOrderState::ExecutingOrder);
    // The unit has a base_values object (a reference into the shared object table).
    assert_ne!(first.base_values_object_index, 0);
    assert_ne!(first.info.hits, 0);
}

#[test]
fn base_values_match_real_v70_save() {
    let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
    // The team base in the file = `BASE_UNIT_VALUES` + clan modifiers
    // (`TeamUnits::Init` + `ResourceManager_SetClanUpgradesFromClans`).
    // Exceptions are fields the original changes during the game:
    //   * `units_built` — the counter/"slot occupied" flag (`in_use`), grows during play;
    //   * `LANDPAD.storage` — 1 in the real save vs 0 in the table (a mutation
    //     during play; the original ATTRIBS also gives 0).
    let mut mismatches: Vec<String> = Vec::new();
    for (team_index, saved) in save.team_units.iter().enumerate() {
        // Empty slots (team_type == NONE) contain values from
        // a previous game: the clan in the header does not apply to them, so
        // only playing teams are compared.
        if save.teams[team_index].team_type == TeamKind::None {
            continue;
        }
        let clan = save.teams[team_index].team_clan;
        let expected = crate::unit_values::team_base_values(clan);
        for (unit_index, slot) in saved.base_values.iter().enumerate() {
            let want = expected[unit_index];
            let got = slot.values;
            for attribute in 0..crate::unit_values::ATTRIB_COUNT {
                if attribute == crate::unit_values::ATTRIB_FUEL {
                    continue;
                }
                if want.get_attribute(attribute) != got.get_attribute(attribute)
                    && !(unit_index == UnitType::LandingPad as usize
                        && attribute == crate::unit_values::ATTRIB_STORAGE
                        && got.get_attribute(attribute) == 1)
                {
                    mismatches.push(format!(
                            "team {team_index} unit {unit_index} attr {attribute}: save={} table={}",
                        got.get_attribute(attribute),
                        want.get_attribute(attribute)
                    ));
                }
            }
            assert_eq!(got.version, want.version, "unit {unit_index} version");
        }
    }
    assert!(mismatches.is_empty(), "mismatches: {mismatches:?}");
    // Check that the comparison actually checked something: playing teams have a clan
    // other than Random and the table differs from the pure base.
    assert_ne!(save.teams[0].team_clan, TeamClan::Random);
    assert_ne!(save.teams[1].team_clan, TeamClan::Random);
    assert_ne!(
        crate::unit_values::team_base_values(save.teams[0].team_clan),
        crate::unit_values::BASE_UNIT_VALUES
    );
    assert_ne!(
        crate::unit_values::team_base_values(save.teams[1].team_clan),
        crate::unit_values::BASE_UNIT_VALUES
    );
}

#[test]
fn team_units_domain_model_is_consistent() {
    let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
    let teams = save.team_units();
    let red = &teams[0];
    assert_eq!(red.gold, 52);
    assert_eq!(red.complexes.len(), 2);
    // Base values = the clan table.
    let expected = crate::unit_values::team_base_values(TeamClan::VonGriffin);
    for unit_index in 0..UNIT_COUNT {
        if unit_index == UnitType::LandingPad as usize {
            continue;
        }
        if expected[unit_index].hits != red.base_values[unit_index].hits {
            panic!("unit {unit_index} hits differ");
        }
    }
    // Object identity is preserved: base and current share objects where
    // there were no upgrades (C++ writes a reference to the same `UnitValues`).
    let shared = save.team_units[0]
        .base_values
        .iter()
        .zip(save.team_units[0].current_values.iter())
        .filter(|(base, current)| base.object_index == current.object_index)
        .count();
    assert!(shared > 80, "only {shared} shared base/current objects");
    let upgraded = UNIT_COUNT - shared;
    assert!(upgraded > 0 && upgraded < 20, "upgraded {upgraded}");
}

#[test]
fn unit_references_resolve() {
    let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
    // All parent/enemy references point to existing units.
    for unit in &save.units {
        if let Some(parent) = unit.info.parent_unit {
            assert!(save.unit_by_id(parent).is_some(), "parent {parent:?}");
        }
        if let Some(enemy) = unit.info.enemy_unit {
            assert!(save.unit_by_id(enemy).is_some(), "enemy {enemy:?}");
        }
        if let Some(complex) = unit.info.complex {
            assert!(
                save.team_units.iter().any(|team| team
                    .complexes
                    .iter()
                    .any(|slot| slot.complex.id == complex.0)),
                    "complex {complex:?}"
            );
        }
    }
    // The unit hash contains exactly the units from the lists (by objects).
    let hashed: usize = save.unit_hash.iter_values().count();
    assert_eq!(hashed, save.units.len());
}

#[test]
fn resource_and_surface_views() {
    let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
    let surface = save.surface();
    assert_eq!(surface.to_bytes(), save.surface_map);
    let resources = save.resource_map();
    assert_eq!(resources.bits_at(0), save.cargo_map[0]);
    // Full byte-exact verification via the map's u16 bitfields.
    for index in [1usize, 100, 5_000, MAP_TILES - 1] {
        assert_eq!(resources.bits_at(index), save.cargo_map[index]);
    }
    let counts = resources.counts();
    assert!(counts.raw_tiles > 0 || counts.fuel_tiles > 0 || counts.gold_tiles > 0);
}

/// `MapHash::FileLoad` ignores the saved `x_shift` (C++ rehashes with the
/// shift derived from the saved size). A foreign value is informational:
/// parsing succeeds and the table keeps its size-derived shift.
#[test]
fn map_hash_saved_x_shift_is_informational() {
    let size = crate::hash::HASH_MAP_HASH_SIZE_MIN;
    let mut data = Vec::new();
    data.extend_from_slice(&size.to_le_bytes());
    data.extend_from_slice(&7u16.to_le_bytes()); // foreign saved shift
    for _ in 0..size {
        data.extend_from_slice(&0u16.to_le_bytes()); // empty buckets
    }

    let mut reader = Reader::new(&data);
    let hash = reader
        .read_map_hash()
        .expect("a saved x_shift mismatch must not abort the load");
    assert_eq!(reader.remaining(), 0);
    assert_eq!(hash.size(), size);
    assert_eq!(
        hash.x_shift(),
        MapHash::<u16>::new(size).x_shift(),
            "the live shift comes from the saved size, not from the header"
    );
    assert_eq!(hash.x_shift(), 2);
    assert_eq!(hash.iter_objects().count(), 0);
}

#[test]
fn rejects_other_versions_and_trailing_bytes() {
    let mut bytes = SAVE_FIXTURE.to_vec();
    bytes[0] = 71;
    assert_eq!(SaveFile::parse(&bytes), Err(SaveError::UnknownVersion(71)));
    let mut bytes = SAVE_FIXTURE.to_vec();
    bytes.push(0);
    assert_eq!(SaveFile::parse(&bytes), Err(SaveError::TrailingBytes(1)));
    assert!(SaveFile::parse(&SAVE_FIXTURE[..100]).is_err());
}

#[test]
fn empty_parser_state_round_trips() {
    // Minimal structure: only the header + an empty map without objects.
    // Check that the writer does not fail and the parser reads what was written.
    let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
    let bytes = save.to_bytes().expect("serializes");
    let again = SaveFile::parse(&bytes).expect("re-parses");
    assert_eq!(save, again);
}

#[test]
fn modified_state_is_written_and_reparsed() {
    // Check of the reverse direction: not only "file -> model -> file",
    // but also "model with changes -> file -> model".
    let mut save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
    save.team_units[0].gold = 777;
    save.teams[0].team_points = 42;
    save.game.turn_counter = 9;
    save.team_ui[0].zoom_level = 64;
    let bytes = save.to_bytes().expect("serializes");
    let reparsed = SaveFile::parse(&bytes).expect("re-parses");
    assert_eq!(reparsed.team_units[0].gold, 777);
    assert_eq!(reparsed.teams[0].team_points, 42);
    assert_eq!(reparsed.game.turn_counter, 9);
    assert_eq!(reparsed.team_ui[0].zoom_level, 64);
    // Everything else is unchanged.
    assert_eq!(reparsed.units.len(), save.units.len());
    assert_eq!(reparsed.unit_lists, save.unit_lists);
    // The file differs from the original exactly in the changed fields.
    assert_ne!(bytes, SAVE_FIXTURE);
    assert_eq!(bytes.len(), SAVE_FIXTURE.len());
}

#[test]
fn unit_values_attribute_from_save() {
    let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
    let slot = &save.team_units[0].base_values[UnitType::Tank as usize];
    let plain = base_unit_values(UnitType::Tank);
    // Von Griffin does not change the tank: the values match the table.
    assert_eq!(slot.values, plain);
    // The attribute is read through the common interface.
    assert_eq!(
        slot.values.get_attribute(ATTRIB_ATTACK),
        plain.get_attribute(ATTRIB_ATTACK)
    );
}

/// `SaveFormatHeader::save_name`/`team_name` stop at the first NUL and
/// tolerate non-UTF-8 bytes (raw V70 names are not guaranteed to be UTF-8).
#[test]
fn header_name_accessors_boundaries() {
    let mut header = SaveFormatHeader {
        version: SAVE_VERSION_V70,
        save_game_type: FileType::Custom,
        save_name: [0; 30],
        world: Planet::Snowcrab,
        mission_index: 0,
        team_names: [[0; 30]; 4],
        team_type: [TeamKind::None; 5],
        team_clan: [TeamClan::Random; 5],
        rng_seed: 0,
        opponent: Opponent::Average,
        turn_timer_time: 0,
        endturn_time: 0,
        play_mode: PlayMode::TurnBased,
    };
    assert_eq!(header.save_name(), "");
    assert_eq!(header.team_name(0), "");
    // Out-of-range team index: empty, no panic.
    assert_eq!(header.team_name(4), "");
    assert_eq!(header.team_name(usize::MAX), "");

    // Exactly 30 non-NUL bytes are a full name.
    header.save_name = [b'N'; 30];
    assert_eq!(header.save_name(), "N".repeat(30));
    // An embedded NUL terminates.
    header.save_name = [0; 30];
    header.save_name[..5].copy_from_slice(b"ABCDE");
    header.save_name[5] = 0;
    header.save_name[6] = b'X';
    assert_eq!(header.save_name(), "ABCDE");
    // Invalid UTF-8 is replaced, not an error.
    header.team_names[2][0] = 0xFF;
    header.team_names[2][1] = b'A';
    assert_eq!(header.team_name(2), "\u{FFFD}A");
}

/// `HeatMap::complete_clamped` matches `HeatMap::LoadV70` for the whole
/// `int8_t` range.
#[test]
fn heat_map_clamp_boundaries() {
    let mut heat_map = HeatMap {
        complete: [0; MAP_TILES],
        stealth_sea: [0; MAP_TILES],
        stealth_land: [0; MAP_TILES],
    };
    for (value, expected) in [
        (i8::MIN, 0u8),
        (-1, 0),
        (0, 0),
        (1, 1),
        (127, 127),
    ] {
        heat_map.complete[0] = value;
        assert_eq!(heat_map.complete_clamped(0), expected, "value {value}");
    }
    // The raw value is preserved (the byte-exact round-trip needs it).
    heat_map.complete[0] = -42;
    assert_eq!(heat_map.complete[0], -42);
}
