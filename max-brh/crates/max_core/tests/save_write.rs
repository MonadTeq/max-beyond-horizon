//! V70 writing from `GameUniverse` (P3): round-trip and byte-exact comparison with references.
//!
//! The strongest check of the layer — "load a real save and write it back
//! from live state": `SAVE1.CAM`/`SAVE2.CAM` → [`GameUniverse::from_save`] →
//! [`GameUniverse::to_save`] → the bytes must match **byte for byte**. Any
//! discrepancy is printed with the field it refers to (see
//! `first_difference`), so the test also works as diagnostics.

use std::collections::BTreeMap;

use max_core::enums::{GameStateType, UnitType};
use max_core::geo::Point;
use max_core::save::{SaveFile, SaveUnit};
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

/// `max-original/SAVE1.CAM` — the training campaign "Islands in the Sun".
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM` — the "Heart of the Matter" campaign.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

/// The first difference between two buffers with context.
fn first_difference(actual: &[u8], expected: &[u8]) -> String {
    if actual == expected {
        return "идентичны".to_string();
    }
    if actual.len() != expected.len() {
        return format!(
            "длины различаются: записано {} байт, в эталоне {}",
            actual.len(),
            expected.len()
        );
    }
    let position = actual
        .iter()
        .zip(expected.iter())
        .position(|(left, right)| left != right)
        .unwrap_or(actual.len());
    let from = position.saturating_sub(16);
    let to = (position + 16).min(actual.len());

    format!(
        "первое расхождение по смещению {position} (0x{position:X}): записано {:02X?}, эталон {:02X?}",
        &actual[from..to],
        &expected[from..to]
    )
}

fn round_trip(source: &[u8], tag: &str) -> Vec<u8> {
    let save = SaveFile::parse(source).unwrap_or_else(|error| panic!("{tag}: разбор: {error}"));
    let state = GameUniverse::from_save(&save);
    let written = state
        .state()
        .to_save()
        .to_bytes()
        .unwrap_or_else(|error| panic!("{tag}: запись: {error}"));

    assert_eq!(
        written.len(),
        source.len(),
        "{tag}: размер файла должен совпасть ({} против {})",
        written.len(),
        source.len()
    );
    written
}

#[test]
fn save1_load_to_save_is_byte_identical() {
    let written = round_trip(SAVE1, "SAVE1.CAM");
    assert_eq!(
        written,
        SAVE1,
        "SAVE1.CAM: {}",
        first_difference(&written, SAVE1)
    );
}

#[test]
fn save2_load_to_save_is_byte_identical() {
    let written = round_trip(SAVE2, "SAVE2.CAM");
    assert_eq!(
        written,
        SAVE2,
        "SAVE2.CAM: {}",
        first_difference(&written, SAVE2)
    );
}

/// Key fields survive the `SaveFile -> GameUniverse -> SaveFile` cycle.
#[test]
fn round_trip_preserves_key_fields() {
    for (source, tag) in [(SAVE1, "SAVE1"), (SAVE2, "SAVE2")] {
        let original = SaveFile::parse(source).expect("parses");
        let state = GameUniverse::from_save(&original);
        let rebuilt = state.state().to_save();

        // Header/settings.
        assert_eq!(rebuilt.header, original.header, "{tag}: заголовок");
        assert_eq!(
            rebuilt.ini_options, original.ini_options,
            "{tag}: IniOptions"
        );
        assert_eq!(
            rebuilt.ini_preferences, original.ini_preferences,
            "{tag}: IniPreferences"
        );
        assert_eq!(
            rebuilt.surface_map, original.surface_map,
            "{tag}: SurfaceMap"
        );
        assert_eq!(rebuilt.cargo_map, original.cargo_map, "{tag}: ResourceMap");

        // Teams, economy, complexes.
        for team in 0..4 {
            assert_eq!(
                rebuilt.teams[team], original.teams[team],
                "{tag}: TeamInfo {team}"
            );
            assert_eq!(
                rebuilt.team_ui[team], original.team_ui[team],
                "{tag}: TeamInfoUi {team}"
            );
            assert_eq!(
                rebuilt.team_units[team].gold, original.team_units[team].gold,
                "{tag}: золото {team}"
            );
            assert_eq!(
                rebuilt.team_units[team].complexes, original.team_units[team].complexes,
                "{tag}: комплексы {team}"
            );
            for unit_type in 0..max_core::UNIT_COUNT {
                assert_eq!(
                    rebuilt.team_units[team].base_values[unit_type].values,
                    original.team_units[team].base_values[unit_type].values,
                    "{tag}: base values {team}/{unit_type}"
                );
                assert_eq!(
                    rebuilt.team_units[team].current_values[unit_type].values,
                    original.team_units[team].current_values[unit_type].values,
                    "{tag}: current values {team}/{unit_type}"
                );
            }
        }

        // Game state.
        assert_eq!(rebuilt.game, original.game, "{tag}: GameManagerState");

        // Units: the same ids, types, cells, orders, paths, references.
        assert_eq!(rebuilt.units.len(), original.units.len(), "{tag}: юнитов");
        let mut rebuilt_by_id: BTreeMap<UnitId, &SaveUnit> = BTreeMap::new();
        for unit in &rebuilt.units {
            rebuilt_by_id.insert(unit.info.id, unit);
        }
        for unit in &original.units {
            let other = rebuilt_by_id
                .get(&unit.info.id)
                .unwrap_or_else(|| panic!("{tag}: юнит {:?} потерян", unit.info.id));
            assert_eq!(other.info.unit_type, unit.info.unit_type, "{tag}: тип");
            assert_eq!(other.info.team, unit.info.team, "{tag}: команда");
            assert_eq!(other.info.grid, unit.info.grid, "{tag}: клетка");
            assert_eq!(other.info.flags, unit.info.flags, "{tag}: флаги");
            assert_eq!(other.info.orders, unit.info.orders, "{tag}: приказ");
            assert_eq!(other.info.state, unit.info.state, "{tag}: состояние");
            assert_eq!(other.info.path, unit.info.path, "{tag}: путь");
            assert_eq!(
                other.info.parent_unit, unit.info.parent_unit,
                "{tag}: parent_unit"
            );
            assert_eq!(
                other.info.enemy_unit, unit.info.enemy_unit,
                "{tag}: enemy_unit"
            );
            assert_eq!(other.info.complex, unit.info.complex, "{tag}: комплекс");
            assert_eq!(
                other.info.build_list, unit.info.build_list,
                "{tag}: build_list"
            );
            assert_eq!(
                other.info.ai_state_bits, unit.info.ai_state_bits,
                "{tag}: ai_state_bits"
            );
            assert_eq!(other.info.hits, unit.info.hits, "{tag}: hits");
            assert_eq!(
                other.info.build_rate, unit.info.build_rate,
                "{tag}: build_rate"
            );
        }

        // Five lists: the same units in the same order.
        for kind in max_core::UnitListKind::ALL {
            let original_ids: Vec<UnitId> = original
                .unit_list(kind)
                .iter()
                .filter_map(|object_index| {
                    original
                        .unit_by_object_index(*object_index)
                        .map(|unit| unit.info.id)
                })
                .collect();
            let rebuilt_ids: Vec<UnitId> = rebuilt
                .unit_list(kind)
                .iter()
                .filter_map(|object_index| {
                    rebuilt
                        .unit_by_object_index(*object_index)
                        .map(|unit| unit.info.id)
                })
                .collect();
            assert_eq!(rebuilt_ids, original_ids, "{tag}: список {kind:?}");
        }

        // Hashes: the same ids by bucket (order within a bucket — file order).
        let object_to_id = |save: &SaveFile, object_index: u16| {
            save.unit_by_object_index(object_index)
                .map(|unit| unit.info.id)
        };
        let ids_of = |save: &SaveFile, hash: &max_core::UnitHash<u16>| {
            hash.buckets()
                .iter()
                .map(|bucket| {
                    bucket
                        .iter()
                        .filter_map(|object_index| object_to_id(save, *object_index))
                        .collect::<Vec<UnitId>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids_of(&rebuilt, &rebuilt.unit_hash),
            ids_of(&original, &original.unit_hash),
            "{tag}: UnitHash"
        );
        assert_eq!(
            rebuilt.unit_hash.size(),
            original.unit_hash.size(),
            "{tag}: размер UnitHash"
        );
        let map_cells = |save: &SaveFile| {
            save.map_hash
                .buckets()
                .iter()
                .map(|bucket| {
                    bucket
                        .iter()
                        .map(|object| {
                            (
                                object.x,
                                object.y,
                                object
                                    .units
                                    .iter()
                                    .filter_map(|object_index| object_to_id(save, *object_index))
                                    .collect::<Vec<UnitId>>(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(map_cells(&rebuilt), map_cells(&original), "{tag}: MapHash");

        // Heat maps: the same (the references have no negative values).
        for team in 0..4 {
            match (&rebuilt.heat_maps[team], &original.heat_maps[team]) {
                (None, None) => {}
                (Some(left), Some(right)) => {
                    assert_eq!(left.complete, right.complete, "{tag}: heat complete {team}");
                    assert_eq!(
                        left.stealth_sea, right.stealth_sea,
                        "{tag}: heat stealth_sea {team}"
                    );
                    assert_eq!(
                        left.stealth_land, right.stealth_land,
                        "{tag}: heat stealth_land {team}"
                    );
                }
                _ => panic!("{tag}: наличие тепловой карты {team} изменилось"),
            }
        }

        // Message logs.
        let logs = |save: &SaveFile| -> Vec<Vec<(Vec<u8>, Point, bool, u16)>> {
            save.message_logs
                .iter()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|entry| {
                            (
                                entry.text.clone(),
                                entry.point,
                                entry.is_alert_message,
                                entry.id,
                            )
                        })
                        .collect()
                })
                .collect()
        };
        assert_eq!(logs(&rebuilt), logs(&original), "{tag}: журналы");

        // AI: player states.
        for team in 0..4 {
            match (&rebuilt.ai_players[team], &original.ai_players[team]) {
                (None, None) => {}
                (Some(left), Some(right)) => {
                    assert_eq!(left.player_team, right.player_team, "{tag}: ai team {team}");
                    assert_eq!(left.strategy, right.strategy, "{tag}: ai strategy {team}");
                    assert_eq!(
                        left.greenhouse_ratio, right.greenhouse_ratio,
                        "{tag}: greenhouse {team}"
                    );
                    assert_eq!(
                        left.minefield_density, right.minefield_density,
                        "{tag}: minefield {team}"
                    );
                    assert_eq!(
                        left.target_location, right.target_location,
                        "{tag}: ai target {team}"
                    );
                    assert_eq!(
                        left.unused_v70, right.unused_v70,
                        "{tag}: unused_v70 {team}"
                    );
                    assert_eq!(
                        left.target_team_v70, right.target_team_v70,
                        "{tag}: target_team_v70 {team}"
                    );
                    assert_eq!(
                        left.info_map.is_some(),
                        right.info_map.is_some(),
                        "{tag}: info_map presence {team}"
                    );
                    assert_eq!(
                        left.mine_map.is_some(),
                        right.mine_map.is_some(),
                        "{tag}: mine_map presence {team}"
                    );
                    assert_eq!(
                        left.spotted_units.len(),
                        right.spotted_units.len(),
                        "{tag}: spotted {team}"
                    );
                }
                _ => panic!("{tag}: наличие AiPlayer {team} изменилось"),
            }
        }

        // Our file is parsed without errors and is stable on repeated writing.
        let bytes = rebuilt.to_bytes().expect("writes");
        let reparsed = SaveFile::parse(&bytes).expect("повторный разбор нашего сейва");
        assert_eq!(
            reparsed.to_bytes().expect("writes"),
            bytes,
            "{tag}: стабильность"
        );
    }
}

#[test]
fn save_of_a_fresh_game_parses_and_reloads() {
    let mut state = GameUniverse::default();
    for index in 0..4 {
        state.state_mut().teams[index].team_type = if index == 0 {
            TeamKind::Player
        } else if index == 1 {
            TeamKind::Computer
        } else {
            TeamKind::None
        };
        if index < 2 {
            state.state_mut().teams[index].team_clan = max_core::TeamClan::VonGriffin;
            state.state_mut().team_units[index] =
                max_core::TeamUnits::with_clan(max_core::TeamClan::VonGriffin);
            state.state_mut().team_units[index].gold = 150;
        }
    }
    state.state_mut().player_team = TeamId::RED;
    state.state_mut().play_mode = max_core::PlayMode::TurnBased;
    state.state_mut().turn_counter = 3;
    state.state_mut().save_meta.save_name = [0; 30];
    max_core::save_write::SaveMeta::set_name(
        &mut state.state_mut().save_meta.save_name,
        "Test Save",
    );
    state.state_mut().save_meta.team_names[0] = [0; 30];
    max_core::save_write::SaveMeta::set_name(
        &mut state.state_mut().save_meta.team_names[0],
        "Player 1",
    );

    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("tank spawns");
    let harvester = state
        .spawn_unit(UnitType::Harvester, TeamId::GREEN, Point::new(20, 20), 0)
        .expect("harvester spawns");
    state.state_mut().add_to_draw_list(harvester);
    state.state_mut().add_to_draw_list(tank);
    state
        .state_mut()
        .push_message(0, "Hello", Point::new(10, 10), Some(tank));

    let bytes = state.state().to_save_bytes().expect("our save writes");
    let save = SaveFile::parse(&bytes).expect("our save parses");

    assert_eq!(save.header.save_name(), "Test Save");
    assert_eq!(save.header.team_name(0), "Player 1");
    assert_eq!(save.game.turn_counter, 3);
    assert_eq!(save.game.game_state, GameStateType::InGame);
    assert_eq!(save.units.len(), 2);
    assert_eq!(save.message_logs[0].len(), 1);
    assert!(
        save.heat_maps[0].is_some(),
        "у играющей команды есть heat map"
    );
    assert!(save.heat_maps[2].is_none());
    assert!(
        save.ai_players[1].is_some(),
        "у компьютерной команды есть AiPlayer"
    );
    assert!(save.ai_players[0].is_none());

    // Loading the saved game returns the same key values.
    let reloaded = GameUniverse::from_save(&save);
    assert_eq!(reloaded.state().turn_counter, 3);
    assert_eq!(reloaded.snapshot().unit_count(), 2);
    assert_eq!(
        reloaded
            .snapshot()
            .unit(tank)
            .map(|unit| (unit.unit_type, unit.grid))
            .unwrap_or((UnitType::Tank, Point::ZERO)),
        (UnitType::Tank, Point::new(10, 10))
    );
    assert!(reloaded.snapshot().has_unit(harvester));
    assert_eq!(reloaded.state().team_units[0].gold, 150);

    // Writing again after loading gives the same bytes.
    let again = reloaded.state().to_save_bytes().expect("writes again");
    assert_eq!(again, bytes, "save -> load -> save стабилен");
}

/// `map_hash` and `unit_hash` of the live `GameUniverse` correspond to the save: after
/// loading the reference, the state has exactly the same cells and buckets.
#[test]
fn hashes_of_loaded_state_match_the_file() {
    let original = SaveFile::parse(SAVE1).expect("parses");
    let state = GameUniverse::from_save(&original);
    let rebuilt = state.state().to_save();

    assert_eq!(
        rebuilt.map_hash.buckets().len(),
        original.map_hash.buckets().len()
    );
    assert_eq!(rebuilt.unit_hash.size(), original.unit_hash.size());
}

#[test]
fn all_units_are_reachable_through_lists_and_hash() {
    let state = GameUniverse::from_save(&SaveFile::parse(SAVE1).expect("parses"));
    let save = state.state().to_save();

    let mut in_lists: BTreeMap<UnitId, ()> = BTreeMap::new();
    for kind in max_core::UnitListKind::ALL {
        for object_index in save.unit_list(kind) {
            let unit = save
                .unit_by_object_index(*object_index)
                .unwrap_or_else(|| panic!("список ссылается на неизвестный объект {object_index}"));
            in_lists.insert(unit.info.id, ());
        }
    }
    for unit in &save.units {
        assert!(
            in_lists.contains_key(&unit.info.id),
            "юнит {:?} не попал ни в один список",
            unit.info.id
        );
    }
    let hashed = save.unit_hash.iter_values().count();
    assert_eq!(hashed, save.units.len(), "UnitHash содержит всех юнитов");
}

/// The derelict team (`PLAYER_TEAM_ALIEN`) has no `TeamUnits` section in the
/// save, so its units cannot share a `TeamValues` object with the live team
/// table: the values are written as an own object, as C++ does
/// (`UnitInfo::base_values` points at the derelict `TeamUnits`, whose objects
/// are not part of the four saved teams).
#[test]
fn derelict_units_save_and_reload() {
    let mut state = GameUniverse::with_setup(
        max_core::map::Map::new(),
        [
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );
    state.state_mut().player_team = TeamId::RED;
    let alien = state
        .spawn_unit(UnitType::AlienTank, TeamId::ALIEN, Point::new(10, 10), 0)
        .expect("alien tank spawns");
    assert_eq!(
        state.snapshot().unit(alien).map(|unit| unit.base_values),
        Some(max_core::unit_values::base_unit_values(UnitType::AlienTank)),
        "The Chosen does not modify alien debris types"
    );

    let bytes = state
        .state()
        .to_save_bytes()
        .expect("save with a derelict unit writes");
    let save = SaveFile::parse(&bytes).expect("save with a derelict unit parses");
    assert!(save.units.iter().any(|unit| unit.info.id == alien));

    let reloaded = GameUniverse::from_save(&save);
    assert!(reloaded.snapshot().has_unit(alien));
    assert_eq!(
        reloaded.state().to_save_bytes().expect("writes again"),
        bytes,
        "save -> load -> save стабилен с derelict-юнитом"
    );
}
