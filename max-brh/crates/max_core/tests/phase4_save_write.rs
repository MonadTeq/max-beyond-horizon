//! Phase 4 — V70 **writing** (`save_write.rs`): all sections, byte-exact
//! write → read → write, derelict units (D layer).
//!
//! `tests/save_write.rs` already proves that a real save loaded into
//! `GameUniverse` and written back is byte-identical. This file adds what that
//! scenario does not reach:
//!
//! * a generated game that populates **every** section (all five lists, a
//!   complex, message logs, AI spotted units, heat maps, both `TeamUnits`
//!   tables, header/`SaveMeta` fields) and survives write → read → write;
//! * V70 enum domains (`GameStateType`) and metadata (save/team names, alien
//!   slots, hash sizes) written by `build_save_file`;
//! * object numbering: every object created by the writer is referenced and the
//!   numbers form the contiguous `1..=N` range (`SmartFileWriter`);
//! * derelict-team units: no `TeamUnits` section exists for `PLAYER_TEAM_ALIEN`,
//!   so each such unit's `base_values` is written as its own object;
//! * `SaveMeta::set_name` truncation and raw message-log bytes;
//! * determinism: two writes of the same state are identical, and the write →
//!   read → write cycle is byte-identical (pinned CRC for the rich state).
//!
//! Oracles: `saveload.cpp` (`SaveLoad_Save`), `teamunits.cpp`
//! (`TeamUnits::FileSave`), `unitinfo.cpp` (`UnitInfo::FileSave`),
//! `smartfile.cpp` (object numbering), `save.md`.

use max_core::ai::SpottedUnit;
use max_core::crc;
use max_core::enums::{
    FileType, GameStateType, Opponent, Planet, PlayMode, TeamClan, UnitOrder, UnitType,
};
use max_core::geo::{Point, MAP_TILES};
use max_core::save::{SaveFile, SAVE_VERSION_V70};
use max_core::save_write::{SaveMeta, SAVE_UNIT_HASH_SIZE};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitListKind;
use max_core::unit_values::UNIT_COUNT;
use max_core::universe::GameUniverse;

/// `max-original/SAVE1.CAM`.
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM`.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

/// A game with every save section populated.
fn rich_state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.state_mut().network_game = false;
    state.state_mut().game_state = GameStateType::InGame;
    state.state_mut().play_mode = PlayMode::TurnBased;
    state.state_mut().turn_counter = 7;
    state.state_mut().active_turn_team = TeamId::RED;
    state.state_mut().player_team = TeamId::RED;
    state.state_mut().mission_index = 3;

    let kinds = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    let clans = [
        TeamClan::Musashi,
        TeamClan::AxisInc,
        TeamClan::Random,
        TeamClan::Random,
    ];
    for team in 0..4 {
        state.state_mut().teams[team].team_type = kinds[team];
        state.state_mut().teams[team].team_clan = clans[team];
        state.state_mut().teams[team].team_points = 10 + team as u32;
        state.state_mut().teams[team].number_of_objects_created = 100 + team as u16;
        state.state_mut().teams[team].finished_turn = team % 2 == 1;
        state.state_mut().team_units[team] = TeamUnits::with_clan(clans[team]);
        state.state_mut().team_units[team].set_gold(300 - team as u32);
        state.state_mut().team_units[team].create_complex();
    }
    // The derivation pre-seeds a second complex for the player, so the
    // `TeamUnits::FileSave` list (not the complex of the buildings) is covered.
    state.state_mut().team_units[0].create_complex();

    // Header/options metadata that does not belong to the simulation.
    state.state_mut().save_meta.save_game_type = FileType::Scenario;
    state.state_mut().save_meta.world = Planet::Frigia;
    state.state_mut().save_meta.opponent = Opponent::Master;
    state.state_mut().save_meta.rng_seed = 0x0BAD_F00D;
    state.state_mut().save_meta.turn_timer_time = 120;
    state.state_mut().save_meta.endturn_time = 30;
    state.state_mut().save_meta.alien_team_type = TeamKind::Computer;
    state.state_mut().save_meta.alien_team_clan = TeamClan::TheChosen;
    SaveMeta::set_name(
        &mut state.state_mut().save_meta.save_name,
        "Phase 4 all sections",
    );
    for team in 0..4 {
        SaveMeta::set_name(
            &mut state.state_mut().save_meta.team_names[team],
            &format!("Team {team}"),
        );
    }
    state.state_mut().save_meta.ini_options.world = 1;
    state.state_mut().save_meta.ini_options.start_gold = 250;
    state.state_mut().save_meta.ini_options.victory_type = max_core::enums::VictoryType::Score;
    state.state_mut().save_meta.ini_options.victory_limit = 50;
    state.state_mut().save_meta.ini_preferences.quick_scroll = 33;
    state.state_mut().save_meta.team_ui[0].zoom_level = 35;
    state.state_mut().save_meta.team_ui[0].camera_position = Point::new(51, 49);
    state.state_mut().save_meta.team_ui[1].selected_unit = Some(0x0002);
    state.state_mut().save_meta.ai_unused_v70[1] = -7;
    state.state_mut().save_meta.ai_target_team_v70[1] = 2;

    // One unit per list: GroundCover, MobileLandSea, Stationary, MobileAir,
    // Particle. The barracks creates a complex (a building immediately gets
    // its own one).
    let road = state
        .spawn_unit(UnitType::Road, TeamId::RED, Point::new(5, 5), 0)
        .expect("road");
    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(6, 5), 0)
        .expect("tank");
    let barracks = state
        .spawn_unit(UnitType::Barracks, TeamId::RED, Point::new(10, 10), 0)
        .expect("barracks");
    let fighter = state
        .spawn_unit(UnitType::Fighter, TeamId::GREEN, Point::new(20, 5), 0)
        .expect("fighter");
    let rocket = state
        .spawn_unit(UnitType::Rocket, TeamId::GREEN, Point::new(21, 5), 0)
        .expect("rocket");

    // Simulation values that the writer must keep. `ai_state_bits` avoids the
    // reminder bits (0x100/0x200) that `GameUniverse::load_state` clears, as C++
    // `UnitInfo::FileLoad` does.
    let mut unit = state.snapshot().unit(tank).expect("tank").clone();
    unit.hits = 17;
    unit.ammo = 9;
    unit.orders = UnitOrder::Sentry;
    unit.ai_state_bits = 0x00A5_5400;
    unit.build_list = vec![UnitType::Scout, UnitType::Tank];
    state.write_unit(tank, unit);
    state.unit_mut(rocket).expect("rocket").flags |= max_core::enums::unit_flags::EXPLODING;

    // Message logs for two teams, with and without a unit reference.
    state
        .state_mut()
        .push_message(0, "Ready", Point::new(6, 5), Some(tank));
    state
        .state_mut()
        .push_message(0, "Second", Point::new(1, 2), None);
    state
        .state_mut()
        .push_message(1, "Green here", Point::new(20, 5), Some(fighter));

    // AI spotted units for the computer team.
    let spotted = SpottedUnit::new(state.snapshot().unit(tank).expect("tank"), TeamId::GREEN);
    state.state_mut().ai.players[1].spotted_units.push(spotted);

    let _ = (road, barracks);
    state
}

#[test]
fn rich_state_sections_survive_write_read_write() {
    let state = rich_state();
    let bytes = state.state().to_save_bytes().expect("writes");
    let save = SaveFile::parse(&bytes).expect("parses");

    // Five lists with one unit each (the rocket is a particle).
    let counts: Vec<usize> = save.unit_lists.iter().map(Vec::len).collect();
    assert_eq!(counts, vec![1, 1, 1, 1, 1], "list counts: {counts:?}");
    assert_eq!(save.units.len(), 5);

    // Header and metadata.
    assert_eq!(save.header.version, SAVE_VERSION_V70);
    assert_eq!(save.header.save_game_type, FileType::Scenario);
    assert_eq!(save.header.world, Planet::Frigia);
    assert_eq!(save.header.mission_index, 3);
    assert_eq!(save.header.opponent, Opponent::Master);
    assert_eq!(save.header.rng_seed, 0x0BAD_F00D);
    assert_eq!(save.header.turn_timer_time, 120);
    assert_eq!(save.header.endturn_time, 30);
    assert_eq!(save.header.play_mode, PlayMode::TurnBased);
    assert_eq!(save.header.save_name(), "Phase 4 all sections");
    assert_eq!(save.header.team_name(2), "Team 2");
    assert_eq!(save.header.team_type[4], TeamKind::Computer);
    assert_eq!(save.header.team_clan[4], TeamClan::TheChosen);
    assert_eq!(save.ini_options.start_gold, 250);
    assert_eq!(save.ini_options.victory_limit, 50);
    assert_eq!(save.ini_preferences.quick_scroll, 33);
    assert_eq!(save.team_ui[0].zoom_level, 35);
    assert_eq!(save.team_ui[0].camera_position, Point::new(51, 49));
    assert_eq!(save.team_ui[1].selected_unit, Some(0x0002));
    assert_eq!(save.game.turn_counter, 7);
    assert_eq!(save.game.active_turn_team, TeamId::RED);

    // Teams, economy and complexes.
    for team in 0..4 {
        assert_eq!(
            save.teams[team].team_type,
            state.state().teams[team].team_type
        );
        assert_eq!(
            save.teams[team].team_clan,
            state.state().teams[team].team_clan
        );
        assert_eq!(save.teams[team].team_points, 10 + team as u32);
        assert_eq!(u32::from(save.team_units[team].gold), 300 - team as u32);
        assert_eq!(save.team_units[team].base_values.len(), UNIT_COUNT);
        assert_eq!(save.team_units[team].current_values.len(), UNIT_COUNT);
    }
    assert_eq!(
        save.team_units[0].complexes.len(),
        3,
        "two seeded + the barracks"
    );
    assert_eq!(save.team_units[1].complexes.len(), 1);

    // Complex membership of the barracks resolves to the team's complex (the
    // complex object is shared with the team list, so the unit stores a
    // reference, not a body).
    let barracks = save
        .units
        .iter()
        .find(|unit| unit.info.unit_type == UnitType::Barracks)
        .expect("barracks");
    let complex = barracks.info.complex.expect("the barracks has a complex");
    let slot = save.team_units[0]
        .complexes
        .iter()
        .find(|slot| slot.complex.id == complex.0)
        .expect("unit complex is in the team list");
    assert_eq!(barracks.complex_object_index, slot.object_index);
    assert_eq!(barracks.complex_value, slot.complex);

    // Unit fields.
    let tank = save
        .units
        .iter()
        .find(|unit| unit.info.unit_type == UnitType::Tank)
        .expect("tank");
    assert_eq!(tank.info.hits, 17);
    assert_eq!(tank.info.ammo, 9);
    assert_eq!(tank.info.orders, UnitOrder::Sentry);
    assert_eq!(tank.info.ai_state_bits, 0x00A5_5400);
    assert_eq!(tank.info.build_list, vec![UnitType::Scout, UnitType::Tank]);
    let rocket = save
        .units
        .iter()
        .find(|unit| unit.info.unit_type == UnitType::Rocket)
        .expect("rocket");
    assert_eq!(rocket.info.unit_list, UnitListKind::Particle);
    assert_ne!(
        rocket.info.flags & max_core::enums::unit_flags::EXPLODING,
        0
    );

    // Message logs: raw text with the terminating NUL, unit references resolve.
    assert_eq!(save.message_logs[0].len(), 2);
    assert_eq!(save.message_logs[1].len(), 1);
    assert!(save.message_logs[2].is_empty());
    assert_eq!(save.message_logs[0][0].text, b"Ready\0");
    assert_eq!(save.message_logs[0][0].unit, Some(tank.info.id));
    assert_eq!(save.message_logs[0][0].point, Point::new(6, 5));
    assert_eq!(save.message_logs[0][1].unit, None);

    // AI spotted units.
    let ai = save.ai_players[1]
        .as_deref()
        .expect("AI for the computer team");
    assert_eq!(ai.player_team, 1);
    assert_eq!(ai.unused_v70, -7);
    assert_eq!(ai.target_team_v70, 2);
    assert_eq!(ai.spotted_units.len(), 1);
    assert_eq!(ai.spotted_units[0].unit, Some(tank.info.id));
    assert_eq!(ai.spotted_units[0].team, u16::from(TeamId::GREEN.0));

    // Heat maps only for playing teams.
    assert!(save.heat_maps[0].is_some());
    assert!(save.heat_maps[1].is_some());
    assert!(save.heat_maps[2].is_none());
    assert!(save.heat_maps[3].is_none());

    // Table sizes follow `SaveMeta` defaults (512).
    assert_eq!(save.unit_hash.size(), SAVE_UNIT_HASH_SIZE);
    assert_eq!(save.map_hash.size(), 512);
    assert_eq!(save.map_hash.x_shift(), 2);

    // Determinism and the D-layer contract: two writes identical, and the
    // write -> load -> write cycle is byte-identical.
    assert_eq!(state.state().to_save_bytes().expect("writes"), bytes);
    let reloaded = GameUniverse::from_save(&save);
    assert_eq!(reloaded.state().to_save_bytes().expect("writes"), bytes);
    let third = GameUniverse::from_save(&SaveFile::parse(&bytes).expect("parses"));
    assert_eq!(third.state().to_save_bytes().expect("writes"), bytes);
    // Pinned CRC of the rich save — a change in any section moves it.
    assert_eq!(crc::crc16(&bytes), RICH_STATE_CRC);
}

/// CRC16 of [`rich_state`]'s V70 file — reference lock for the writer.
const RICH_STATE_CRC: u16 = 0x7E34;

#[test]
fn reference_saves_write_read_write_identically() {
    for (source, tag) in [(SAVE1, "SAVE1"), (SAVE2, "SAVE2")] {
        let save = SaveFile::parse(source).expect("parses");
        let state = GameUniverse::from_save(&save);
        let first = state.state().to_save_bytes().expect("writes");
        assert_eq!(first, source, "{tag}: load -> save is byte-identical");

        let reparsed = SaveFile::parse(&first).expect("parses the written file");
        let second = GameUniverse::from_save(&reparsed)
            .state()
            .to_save_bytes()
            .expect("writes");
        assert_eq!(second, first, "{tag}: write -> read -> write");
        let third = GameUniverse::from_save(&SaveFile::parse(&second).expect("parses"))
            .state()
            .to_save_bytes()
            .expect("writes");
        assert_eq!(third, first, "{tag}: a third cycle");
    }
}

#[test]
fn modified_game_state_save_is_stable() {
    let save = SaveFile::parse(SAVE1).expect("parses");
    let mut state = GameUniverse::from_save(&save);

    state.state_mut().team_units[0].set_gold(777);
    state.state_mut().teams[0].team_points = 42;
    state.state_mut().turn_counter = 9;
    state.state_mut().save_meta.save_name = [0; 30];
    SaveMeta::set_name(&mut state.state_mut().save_meta.save_name, "Modified");
    state.state_mut().save_meta.team_ui[0].zoom_level = 64;
    let id = state.snapshot().unit_ids_in_order()[0];
    let mut unit = state.snapshot().unit(id).expect("unit").clone();
    unit.hits = unit.hits.saturating_sub(1);
    unit.grid = Point::new(unit.grid.x.wrapping_add(1), unit.grid.y);
    let new_grid = unit.grid;
    state.write_unit(id, unit);

    let bytes = state.state().to_save_bytes().expect("writes");
    assert_ne!(bytes, SAVE1, "the file must change");
    let parsed = SaveFile::parse(&bytes).expect("parses");
    assert_eq!(parsed.team_units[0].gold, 777);
    assert_eq!(parsed.teams[0].team_points, 42);
    assert_eq!(parsed.game.turn_counter, 9);
    assert_eq!(parsed.header.save_name(), "Modified");
    assert_eq!(parsed.team_ui[0].zoom_level, 64);
    let reloaded = GameUniverse::from_save(&parsed);
    let unit = reloaded.snapshot().unit(id).expect("unit");
    assert_eq!(unit.grid, new_grid);
    let hits = unit.hits;
    assert_eq!(hits, state.snapshot().unit(id).expect("unit").hits);
    // Moving the unit updated the cell table: it is at the new cell only.
    assert!(reloaded.snapshot().units_at(new_grid).contains(&id));
    // The resulting file is stable on a second cycle.
    assert_eq!(reloaded.state().to_save_bytes().expect("writes"), bytes);
}

#[test]
fn object_numbers_are_contiguous() {
    let state = rich_state();
    let save = SaveFile::parse(&state.state().to_save_bytes().expect("writes")).expect("parses");

    let mut indices: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    for team in &save.team_units {
        for slot in team.base_values.iter().chain(team.current_values.iter()) {
            if slot.object_index != 0 {
                indices.insert(slot.object_index);
            }
        }
        for slot in &team.complexes {
            if slot.object_index != 0 {
                indices.insert(slot.object_index);
            }
        }
    }
    for unit in &save.units {
        for index in [
            unit.object_index,
            unit.path_object_index,
            unit.base_values_object_index,
            unit.complex_object_index,
            unit.parent_object_index,
            unit.enemy_object_index,
        ] {
            if index != 0 {
                indices.insert(index);
            }
        }
    }
    for logs in &save.message_logs {
        for entry in logs {
            if entry.unit_object_index != 0 {
                indices.insert(entry.unit_object_index);
            }
        }
    }
    for ai in save.ai_players.iter().flatten() {
        for spotted in &ai.spotted_units {
            if spotted.unit_object_index != 0 {
                indices.insert(spotted.unit_object_index);
            }
        }
    }

    let max = *indices.iter().next_back().expect("objects");
    assert_eq!(
        indices.len(),
        max as usize,
        "object numbers must be the contiguous range 1..={max}"
    );
    assert_eq!(*indices.iter().next().expect("objects"), 1);
}

#[test]
fn derelict_units_get_their_own_values_object() {
    // V70 has four `TeamUnits` sections; the derelict (`PLAYER_TEAM_ALIEN`) has
    // none. Sharing a derelict unit's values with the live table would leave a
    // dangling object number, so the writer emits an own `UnitValues` object.
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    // `GameManager_InitUnitsAndGameState` initializes the derelict team with
    // `TEAM_CLAN_THE_CHOSEN` before the game starts.
    state.state_mut().team_units[4] = TeamUnits::with_clan(TeamClan::TheChosen);
    state.state_mut().player_team = TeamId::RED;
    let first = state
        .spawn_unit(UnitType::AlienTank, TeamId::ALIEN, Point::new(10, 10), 0)
        .expect("alien tank");
    let second = state
        .spawn_unit(UnitType::AlienTank, TeamId::ALIEN, Point::new(12, 10), 0)
        .expect("alien tank");

    let bytes = state.state().to_save_bytes().expect("writes");
    let save = SaveFile::parse(&bytes).expect("parses");
    let units: Vec<_> = save
        .units
        .iter()
        .filter(|unit| unit.info.team == TeamId::ALIEN)
        .collect();
    assert_eq!(units.len(), 2);
    for unit in &units {
        assert_ne!(
            unit.base_values_object_index, 0,
            "the derelict unit must have its own values object"
        );
        assert_eq!(
            unit.info.base_values,
            max_core::unit_values::base_unit_values(UnitType::AlienTank)
        );
    }
    assert_ne!(
        units[0].base_values_object_index, units[1].base_values_object_index,
        "each derelict unit writes its own object"
    );
    // The four `TeamUnits` sections still have consecutive numbers: no alien
    // section was inserted.
    assert_eq!(save.team_units.len(), 4);

    let reloaded = GameUniverse::from_save(&save);
    assert!(reloaded.snapshot().has_unit(first));
    assert!(reloaded.snapshot().has_unit(second));
    assert_eq!(reloaded.state().to_save_bytes().expect("writes"), bytes);
}

#[test]
fn game_state_variants_round_trip() {
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.state_mut().player_team = TeamId::RED;

    for variant in 0..=0x0Fu16 {
        state.state_mut().game_state =
            GameStateType::try_from(variant).expect("all 16 values are defined");
        let bytes = state.state().to_save_bytes().expect("writes");
        let save = SaveFile::parse(&bytes).expect("parses");
        assert_eq!(
            save.game.game_state,
            state.state().game_state,
            "variant {variant}"
        );
        assert_eq!(
            GameUniverse::from_save(&save).state().game_state,
            state.state().game_state,
            "variant {variant} after load"
        );
    }

    for mode in [PlayMode::TurnBased, PlayMode::SimultaneousMoves] {
        state.state_mut().play_mode = mode;
        let save =
            SaveFile::parse(&state.state().to_save_bytes().expect("writes")).expect("parses");
        assert_eq!(save.header.play_mode, mode);
    }
}

#[test]
fn metadata_hash_sizes_are_applied() {
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.state_mut().player_team = TeamId::RED;
    state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("tank");

    // The file sizes come from `SaveMeta`; `x_shift` is derived from the size by
    // `MapHash::Init` (the stored `map_hash_x_shift` field is informational only).
    state.state_mut().save_meta.unit_hash_size = 1024;
    state.state_mut().save_meta.map_hash_size = 1024;
    state.state_mut().save_meta.map_hash_x_shift = 0;
    let save = SaveFile::parse(&state.state().to_save_bytes().expect("writes")).expect("parses");
    assert_eq!(save.unit_hash.size(), 1024);
    assert_eq!(save.map_hash.size(), 1024);
    assert_eq!(save.map_hash.x_shift(), 3, "1024 buckets -> shift 3");
    assert_eq!(save.unit_hash.iter_values().count(), 1);

    // A save loaded from this file keeps the sizes when written again.
    let reloaded = GameUniverse::from_save(&save);
    let save2 =
        SaveFile::parse(&reloaded.state().to_save_bytes().expect("writes")).expect("parses");
    assert_eq!(save2.unit_hash.size(), 1024);
    assert_eq!(save2.map_hash.size(), 1024);
}

#[test]
fn empty_state_has_empty_sections() {
    let state = GameUniverse::default();
    let bytes = state.state().to_save_bytes().expect("writes");
    let save = SaveFile::parse(&bytes).expect("parses");

    assert_eq!(save.header.version, SAVE_VERSION_V70);
    assert_eq!(save.header.save_game_type, FileType::Custom);
    assert!(save.units.is_empty());
    assert!(save.unit_lists.iter().all(Vec::is_empty));
    assert_eq!(save.unit_hash.size(), SAVE_UNIT_HASH_SIZE);
    assert_eq!(save.map_hash.size(), 512);
    assert!(save.heat_maps.iter().all(Option::is_none));
    assert!(save.team_units.iter().all(|team| team.complexes.is_empty()));
    for team in &save.team_units {
        assert_eq!(team.base_values.len(), UNIT_COUNT);
        assert_eq!(team.current_values.len(), UNIT_COUNT);
    }
    assert!(save.ai_players.iter().all(Option::is_none));
    assert!(save.message_logs.iter().all(Vec::is_empty));
    assert_eq!(
        GameUniverse::from_save(&save)
            .state()
            .to_save_bytes()
            .expect("writes"),
        bytes
    );
}

#[test]
fn save_meta_and_names_round_trip() {
    let save = SaveFile::parse(SAVE1).expect("parses");
    let meta = SaveMeta::from_save(&save);

    // `set_name`: 30 bytes max, no NUL terminator when exactly 30 are used.
    let mut field = [0u8; 30];
    SaveMeta::set_name(&mut field, "012345678901234567890123456789XXXX");
    assert_eq!(&field, b"012345678901234567890123456789");
    SaveMeta::set_name(&mut field, "abc");
    assert_eq!(field[..4], *b"abc\0");
    assert!(field[4..].iter().all(|byte| *byte == 0));
    // Embedded NULs terminate the name.
    SaveMeta::set_name(&mut field, "ab\0cd");
    assert_eq!(meta_name(&field), "ab");

    // The metadata is derived from the file and restored through the state.
    let state = GameUniverse::from_save(&save);
    assert_eq!(state.state().save_meta, meta);
    assert_eq!(meta.save_name(), "Islands in the Sun");
    assert_eq!(meta.team_name(1), "Green Team");
    assert_eq!(meta.unit_hash_size, 512);
    assert_eq!(meta.map_hash_size, 512);
    let reparsed =
        SaveFile::parse(&state.state().to_save_bytes().expect("writes")).expect("parses");
    assert_eq!(SaveMeta::from_save(&reparsed), meta);
}

/// Name of a fixed 30-byte field up to the first NUL.
fn meta_name(bytes: &[u8; 30]) -> String {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(30);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[test]
fn message_logs_preserve_raw_bytes_and_clamp_the_team() {
    let mut state = GameUniverse::default();
    state.state_mut().teams[0].team_type = TeamKind::Player;
    state.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);

    // UTF-8 text, an empty message and a long message; out-of-range team index
    // is clamped to the last log (C++ `PlayerTeamCount`).
    state
        .state_mut()
        .push_message(0, "Привет, мир", Point::new(1, 2), None);
    state.state_mut().push_message(0, "", Point::ZERO, None);
    let long = "x".repeat(1000);
    state
        .state_mut()
        .push_message(9, &long, Point::new(3, 4), None);

    let save = SaveFile::parse(&state.state().to_save_bytes().expect("writes")).expect("parses");
    assert_eq!(save.message_logs[0].len(), 2);
    assert_eq!(save.message_logs[3].len(), 1);

    let first = &save.message_logs[0][0];
    let mut expected = "Привет, мир".as_bytes().to_vec();
    expected.push(0);
    assert_eq!(first.text, expected);
    assert_eq!(first.point, Point::new(1, 2));
    assert_eq!(first.unit, None);

    // An empty message is stored as a single NUL byte.
    assert_eq!(save.message_logs[0][1].text, vec![0]);
    assert_eq!(save.message_logs[0][1].point, Point::ZERO);

    let long_entry = &save.message_logs[3][0];
    assert_eq!(long_entry.text.len(), 1001);
    assert_eq!(long_entry.text.last(), Some(&0));
    assert_eq!(long_entry.text[..1000], *long.as_bytes());
    assert_eq!(long_entry.point, Point::new(3, 4));

    // The raw bytes survive a state round-trip unchanged.
    let reloaded = GameUniverse::from_save(&save);
    let again =
        SaveFile::parse(&reloaded.state().to_save_bytes().expect("writes")).expect("parses");
    assert_eq!(again.message_logs[0][0].text, expected);
    assert_eq!(again.message_logs[3][0].text, long_entry.text);
}

#[test]
fn surface_and_resource_maps_cover_all_tiles() {
    let mut state = rich_state();
    // Resources: three kinds with different amounts and visibility.
    for (index, kind) in [
        max_core::map::ResourceKind::Raw,
        max_core::map::ResourceKind::Fuel,
        max_core::map::ResourceKind::Gold,
    ]
    .into_iter()
    .enumerate()
    {
        let entry = max_core::map::ResourceMapEntry::new(10 + index as u8, Some(kind), 0b0101);
        assert!(state.state_mut().map.resources.set(index as i16, 0, entry));
    }
    state
        .state_mut()
        .map
        .surface
        .set(0, 1, max_core::enums::Surface::WATER);
    state
        .state_mut()
        .map
        .surface
        .set(1, 1, max_core::enums::Surface::COAST);

    let save = SaveFile::parse(&state.state().to_save_bytes().expect("writes")).expect("parses");
    assert_eq!(save.surface_map.len(), MAP_TILES);
    assert_eq!(save.cargo_map.len(), MAP_TILES);
    assert_eq!(save.surface().to_bytes(), save.surface_map);
    assert_eq!(save.resource_map().bits_at(0), save.cargo_map[0]);
    assert!(save.resource_map().counts().raw_tiles >= 1);
    // (0, 1) is flat index `y * 112 + x = 112`.
    assert_eq!(
        state.state().map.surface.get(0, 1),
        max_core::enums::Surface::from_bits(save.surface_map[112]).expect("surface")
    );
}
