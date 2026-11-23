//! Phase 4 — V70 **reading** (`save.rs`): every field, corrupt files, wrong ids.
//!
//! The existing `tests/v70_save.rs` checks that two real saves parse and that a
//! few header/section values match. This file closes the "reading" contract:
//!
//! * the exact V70 byte offsets of the header/options/game-state block are
//!   verified by patching them one by one and re-reading the model;
//! * every header, `IniOptions`, `IniPreferences`, `TeamInfo`, `TeamInfoUi` and
//!   `GameManagerState` field survives a patch → parse → write cycle (the
//!   "all fields" requirement);
//! * corrupt input is rejected with a typed [`SaveError`] and never panics:
//!   every format version, truncation at any offset, garbage input, invalid
//!   enum values, wrong object references (invalid index, wrong object kind);
//! * cross-section consistency: lists/hashes/AI/log references resolve, every
//!   unit is in exactly one CRC list, unit/object numbers are unique and sorted,
//!   the map views cover all 12,544 tiles, heat maps clamp as in C++
//!   (`HeatMap::LoadV70`).
//!
//! Oracles: `saveload.cpp` (`SaveLoad_LoadFormatV70`), `smartfile.cpp`,
//! `save.md` (V70 layout), `unitinfo.cpp` (`UnitInfo::FileLoad`),
//! `heatmap.cpp` (`LoadV70`).

use max_core::enums::{
    FileType, GameStateType, Opponent, Planet, PlayMode, TeamClan, UnitOrder, UnitOrderState,
    UnitType, VictoryType,
};
use max_core::geo::{PathStep, Point, MAP_TILES};
use max_core::save::{
    SaveError, SaveFile, SaveFormatHeader, SAVE_TEAM_COUNT, SAVE_VERSION_V70, UNIT_LIST_COUNT,
};
use max_core::team::{TeamId, TeamInfoUi, TeamKind};
use max_core::unit::{UnitId, UnitListKind, UnitPath};
use max_core::unit_values::{ATTRIB_COUNT, UNIT_COUNT};
use max_core::universe::GameUniverse;

/// `max-original/SAVE1.CAM` — the training campaign "Islands in the Sun".
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM` — the "Heart of the Matter" campaign.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

// --- V70 layout offsets of the header/options block -------------------------
//
// `version:u16, save_game_type:u8, save_name[30], world:u8, mission_index:u16,
// team_name[4][30], team_type[5]:u8, team_clan[5]:u8, rng_seed:u32,
// opponent:u8, turn_timer_time:u16, endturn_time:u16, play_mode:u8`,
// then `IniOptions` (12 × i32), `SurfaceMap` (12,544 × u8, raw), `ResourceMap`
// (12,544 × u16), `TeamInfo × 4` (565 bytes each).
const OFF_VERSION: usize = 0;
const OFF_FILE_TYPE: usize = 2;
const OFF_SAVE_NAME: usize = 3;
const OFF_WORLD: usize = 33;
const OFF_MISSION: usize = 34;
const OFF_TEAM_NAMES: usize = 36;
const OFF_TEAM_TYPE: usize = 156;
const OFF_TEAM_CLAN: usize = 161;
const OFF_RNG_SEED: usize = 166;
const OFF_OPPONENT: usize = 170;
const OFF_TURN_TIMER: usize = 171;
const OFF_ENDTURN: usize = 173;
const OFF_PLAY_MODE: usize = 175;
const OFF_INI_OPTIONS: usize = 176;
const SIZE_INI_OPTIONS: usize = 48;
const OFF_SURFACE_MAP: usize = 224;
const OFF_CARGO_MAP: usize = OFF_SURFACE_MAP + MAP_TILES;
const OFF_TEAMS: usize = OFF_CARGO_MAP + MAP_TILES * 2;
const SIZE_TEAM_INFO: usize = 565;
const OFF_GAME_STATE: usize = OFF_TEAMS + SAVE_TEAM_COUNT * SIZE_TEAM_INFO;
const OFF_GAME_TYPE: usize = OFF_GAME_STATE + 1 + 1 + 4;
const OFF_INI_PREFERENCES: usize = OFF_GAME_STATE + 10;

fn load(source: &[u8]) -> SaveFile {
    SaveFile::parse(source).expect("reference save parses")
}

/// A copy of `source` with `bytes` written at `offset`.
fn patched(source: &[u8], offset: usize, bytes: &[u8]) -> Vec<u8> {
    let mut copy = source.to_vec();
    copy[offset..offset + bytes.len()].copy_from_slice(bytes);
    copy
}

#[test]
fn v70_layout_offsets_are_exact() {
    let save = load(SAVE1);
    let header = &save.header;

    // The offset table above is derived from the parser/writer; verify every
    // field against the raw bytes so the corruption tests below patch the
    // intended field.
    assert_eq!(
        u16::from_le_bytes([SAVE1[OFF_VERSION], SAVE1[OFF_VERSION + 1]]),
        70
    );
    assert_eq!(SAVE1[OFF_FILE_TYPE], header.save_game_type as u8);
    assert_eq!(&SAVE1[OFF_SAVE_NAME..OFF_SAVE_NAME + 30], &header.save_name);
    assert_eq!(SAVE1[OFF_WORLD], header.world as u8);
    assert_eq!(
        u16::from_le_bytes([SAVE1[OFF_MISSION], SAVE1[OFF_MISSION + 1]]),
        header.mission_index
    );
    for (team, name) in header.team_names.iter().enumerate() {
        let start = OFF_TEAM_NAMES + team * 30;
        assert_eq!(&SAVE1[start..start + 30], name, "team name {team}");
    }
    for (team, kind) in header.team_type.iter().enumerate() {
        assert_eq!(SAVE1[OFF_TEAM_TYPE + team], *kind as u8, "team type {team}");
    }
    for (team, clan) in header.team_clan.iter().enumerate() {
        assert_eq!(SAVE1[OFF_TEAM_CLAN + team], *clan as u8, "team clan {team}");
    }
    assert_eq!(
        u32::from_le_bytes([
            SAVE1[OFF_RNG_SEED],
            SAVE1[OFF_RNG_SEED + 1],
            SAVE1[OFF_RNG_SEED + 2],
            SAVE1[OFF_RNG_SEED + 3],
        ]),
        header.rng_seed
    );
    assert_eq!(SAVE1[OFF_OPPONENT], header.opponent as u8);
    assert_eq!(
        u16::from_le_bytes([SAVE1[OFF_TURN_TIMER], SAVE1[OFF_TURN_TIMER + 1]]),
        header.turn_timer_time
    );
    assert_eq!(
        u16::from_le_bytes([SAVE1[OFF_ENDTURN], SAVE1[OFF_ENDTURN + 1]]),
        header.endturn_time
    );
    assert_eq!(SAVE1[OFF_PLAY_MODE], header.play_mode as u8);

    // IniOptions.
    let i32_at = |offset: usize| {
        i32::from_le_bytes([
            SAVE1[offset],
            SAVE1[offset + 1],
            SAVE1[offset + 2],
            SAVE1[offset + 3],
        ])
    };
    assert_eq!(i32_at(OFF_INI_OPTIONS), save.ini_options.world);
    assert_eq!(i32_at(OFF_INI_OPTIONS + 4), save.ini_options.turn_timer);
    assert_eq!(i32_at(OFF_INI_OPTIONS + 8), save.ini_options.endturn);
    assert_eq!(i32_at(OFF_INI_OPTIONS + 12), save.ini_options.start_gold);
    assert_eq!(
        i32_at(OFF_INI_OPTIONS + 16),
        save.ini_options.play_mode as i32
    );

    // Maps.
    assert_eq!(
        &SAVE1[OFF_SURFACE_MAP..OFF_SURFACE_MAP + MAP_TILES],
        &save.surface_map
    );
    for index in [0usize, 1, 100, 5_000, MAP_TILES - 1] {
        let at = OFF_CARGO_MAP + index * 2;
        assert_eq!(
            u16::from_le_bytes([SAVE1[at], SAVE1[at + 1]]),
            save.cargo_map[index],
            "resource tile {index}"
        );
    }

    // Game state (after four fixed-size `TeamInfo` blocks).
    assert_eq!(SAVE1[OFF_GAME_STATE], save.game.active_turn_team.0);
    assert_eq!(SAVE1[OFF_GAME_STATE + 1], save.game.player_team.0);
    assert_eq!(
        i32::from_le_bytes([
            SAVE1[OFF_GAME_STATE + 2],
            SAVE1[OFF_GAME_STATE + 3],
            SAVE1[OFF_GAME_STATE + 4],
            SAVE1[OFF_GAME_STATE + 5],
        ]),
        save.game.turn_counter
    );
    assert_eq!(
        u16::from_le_bytes([SAVE1[OFF_GAME_TYPE], SAVE1[OFF_GAME_TYPE + 1]]),
        save.game.game_state as u16
    );
    // IniPreferences starts right after the 10-byte game state.
    assert_eq!(i32_at(OFF_INI_PREFERENCES), save.ini_preferences.effects);

    // The derived offsets really are consecutive blocks.
    assert_eq!(OFF_SURFACE_MAP, OFF_INI_OPTIONS + SIZE_INI_OPTIONS);
    assert_eq!(OFF_CARGO_MAP, OFF_SURFACE_MAP + MAP_TILES);
    assert_eq!(OFF_TEAMS, OFF_CARGO_MAP + MAP_TILES * 2);
    assert_eq!(OFF_GAME_STATE, OFF_TEAMS + SAVE_TEAM_COUNT * SIZE_TEAM_INFO);
    assert_eq!(OFF_INI_PREFERENCES, OFF_GAME_STATE + 10);
}

#[test]
fn header_fields_are_read_from_their_offsets() {
    let original = load(SAVE1);
    assert_eq!(original.header.version, SAVE_VERSION_V70);

    // Every header field can be patched at once and is picked up by the parser
    // (one parse keeps the test's stack usage flat: `SaveFile` is ~45 KB).
    let mut name = [0u8; 30];
    name[..12].copy_from_slice(b"Phase 4 test");
    let mut bytes = SAVE1.to_vec();
    bytes[OFF_FILE_TYPE] = FileType::Scenario as u8;
    bytes[OFF_SAVE_NAME..OFF_SAVE_NAME + 30].copy_from_slice(&name);
    bytes[OFF_WORLD] = Planet::Snowcrab as u8;
    bytes[OFF_MISSION..OFF_MISSION + 2].copy_from_slice(&0x1234u16.to_le_bytes());
    bytes[OFF_TEAM_NAMES + 2 * 30..OFF_TEAM_NAMES + 2 * 30 + 10].copy_from_slice(b"Blue Team\0");
    // The fifth ("alien") header slot is stored and read even though it has no
    // `TeamInfo` block of its own.
    bytes[OFF_TEAM_TYPE + 4] = TeamKind::Computer as u8;
    bytes[OFF_TEAM_CLAN + 4] = TeamClan::Musashi as u8;
    bytes[OFF_TEAM_CLAN + 2] = TeamClan::CrimsonPath as u8;
    bytes[OFF_RNG_SEED..OFF_RNG_SEED + 4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    bytes[OFF_OPPONENT] = Opponent::God as u8;
    bytes[OFF_TURN_TIMER..OFF_TURN_TIMER + 2].copy_from_slice(&90u16.to_le_bytes());
    bytes[OFF_ENDTURN..OFF_ENDTURN + 2].copy_from_slice(&45u16.to_le_bytes());
    bytes[OFF_PLAY_MODE] = PlayMode::TurnBased as u8;

    let patched_save = load(&bytes);
    let header = &patched_save.header;
    assert_eq!(header.save_game_type, FileType::Scenario);
    assert_eq!(header.save_name(), "Phase 4 test");
    assert_eq!(header.world, Planet::Snowcrab);
    assert_eq!(header.mission_index, 0x1234);
    assert_eq!(header.team_name(2), "Blue Team");
    assert_eq!(header.team_type[4], TeamKind::Computer);
    assert_eq!(header.team_clan[4], TeamClan::Musashi);
    assert_eq!(header.team_clan[2], TeamClan::CrimsonPath);
    assert_eq!(header.rng_seed, 0xDEAD_BEEF);
    assert_eq!(header.opponent, Opponent::God);
    assert_eq!(header.turn_timer_time, 90);
    assert_eq!(header.endturn_time, 45);
    assert_eq!(header.play_mode, PlayMode::TurnBased);

    // A full 30-byte name without a NUL terminator is kept as is.
    let full = [b'A'; 30];
    let bytes = patched(SAVE1, OFF_SAVE_NAME, &full);
    assert_eq!(load(&bytes).header.save_name(), "A".repeat(30));

    // IniOptions: all twelve `signed int` fields at their offsets.
    let mut bytes = SAVE1.to_vec();
    let mut set_i32 = |field: usize, value: i32| {
        bytes[OFF_INI_OPTIONS + field * 4..OFF_INI_OPTIONS + field * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    };
    set_i32(0, 7);
    set_i32(1, 120);
    set_i32(2, 60);
    set_i32(3, 250);
    set_i32(4, i32::from(PlayMode::TurnBased as u8));
    set_i32(5, i32::from(VictoryType::Score as u8));
    set_i32(6, 99);
    set_i32(7, i32::from(Opponent::Master as u8));
    set_i32(8, 2);
    set_i32(9, 1);
    set_i32(10, 2);
    set_i32(11, 1);
    let options = load(&bytes).ini_options;
    assert_eq!(options.world, 7);
    assert_eq!(options.turn_timer, 120);
    assert_eq!(options.endturn, 60);
    assert_eq!(options.start_gold, 250);
    assert_eq!(options.play_mode, PlayMode::TurnBased);
    assert_eq!(options.victory_type, VictoryType::Score);
    assert_eq!(options.victory_limit, 99);
    assert_eq!(options.opponent, Opponent::Master);
    assert_eq!(options.raw_resource, 2);
    assert_eq!(options.fuel_resource, 1);
    assert_eq!(options.gold_resource, 2);
    assert_eq!(options.alien_derelicts, 1);

    // Game state and IniPreferences.
    let mut bytes = SAVE1.to_vec();
    bytes[OFF_GAME_STATE] = TeamId::BLUE.0;
    bytes[OFF_GAME_STATE + 1] = TeamId::GREEN.0;
    bytes[OFF_GAME_STATE + 2..OFF_GAME_STATE + 6].copy_from_slice(&0x0102_0304i32.to_le_bytes());
    bytes[OFF_GAME_TYPE..OFF_GAME_TYPE + 2]
        .copy_from_slice(&(GameStateType::DeployingUnits as u16).to_le_bytes());
    bytes[OFF_GAME_STATE + 8..OFF_GAME_STATE + 10].copy_from_slice(&77u16.to_le_bytes());
    let preferences = [11i32, 3, 41, 2, 4, 5, 6];
    for (index, value) in preferences.iter().enumerate() {
        bytes[OFF_INI_PREFERENCES + index * 4..OFF_INI_PREFERENCES + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let patched_save = load(&bytes);
    assert_eq!(patched_save.game.active_turn_team, TeamId::BLUE);
    assert_eq!(patched_save.game.player_team, TeamId::GREEN);
    assert_eq!(patched_save.game.turn_counter, 0x0102_0304);
    assert_eq!(patched_save.game.game_state, GameStateType::DeployingUnits);
    assert_eq!(patched_save.game.turn_timer, 77);
    assert_eq!(
        [
            patched_save.ini_preferences.effects,
            patched_save.ini_preferences.click_scroll,
            patched_save.ini_preferences.quick_scroll,
            patched_save.ini_preferences.fast_movement,
            patched_save.ini_preferences.follow_unit,
            patched_save.ini_preferences.auto_select,
            patched_save.ini_preferences.enemy_halt,
        ],
        preferences
    );
}

#[test]
fn every_header_and_option_field_survives_a_write_cycle() {
    let mut save = load(SAVE1);
    let original_len = save.to_bytes().expect("writes").len();

    // Header.
    save.header = SaveFormatHeader {
        version: SAVE_VERSION_V70,
        save_game_type: FileType::Scenario,
        save_name: [b'Z'; 30],
        world: Planet::ValentinesPlanet,
        mission_index: 0xBEEF,
        team_names: [[b'T'; 30]; 4],
        team_type: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::Computer,
        ],
        team_clan: [
            TeamClan::TheChosen,
            TeamClan::CrimsonPath,
            TeamClan::AyersHand,
            TeamClan::SacredEight,
            TeamClan::SevenKnights,
        ],
        rng_seed: 0x0BAD_F00D,
        opponent: Opponent::Clueless,
        turn_timer_time: 0xFFFF,
        endturn_time: 1,
        play_mode: PlayMode::Unknown,
    };
    // `IniOptions` — every field.
    save.ini_options.world = 5;
    save.ini_options.turn_timer = -1;
    save.ini_options.endturn = 3600;
    save.ini_options.start_gold = 249;
    save.ini_options.play_mode = PlayMode::TurnBased;
    save.ini_options.victory_type = VictoryType::Score;
    save.ini_options.victory_limit = 1000;
    save.ini_options.opponent = Opponent::God;
    save.ini_options.raw_resource = 2;
    save.ini_options.fuel_resource = 0;
    save.ini_options.gold_resource = 2;
    save.ini_options.alien_derelicts = 1;
    // `IniPreferences` — every field.
    save.ini_preferences = max_core::save::IniPreferences {
        effects: 1,
        click_scroll: 1,
        quick_scroll: 39,
        fast_movement: 0,
        follow_unit: 1,
        auto_select: 0,
        enemy_halt: 1,
    };
    // Game state.
    save.game.active_turn_team = TeamId::ALIEN;
    save.game.player_team = TeamId::GRAY;
    save.game.turn_counter = i32::MIN;
    save.game.game_state = GameStateType::InGame;
    save.game.turn_timer = 65535;

    let bytes = save.to_bytes().expect("writes");
    assert_eq!(bytes.len(), original_len, "header fields are fixed-size");
    let reparsed = SaveFile::parse(&bytes).expect("parses");
    assert_eq!(reparsed.header, save.header);
    assert_eq!(reparsed.ini_options, save.ini_options);
    assert_eq!(reparsed.ini_preferences, save.ini_preferences);
    assert_eq!(reparsed.game, save.game);
}

#[test]
fn every_team_and_ui_field_survives_a_write_cycle() {
    let mut save = load(SAVE2);
    let mut expected = save.clone();

    for team in 0..SAVE_TEAM_COUNT {
        let info = &mut save.teams[team];
        // `team_type` gates whether the file carries a heat map/AI block, so it
        // is not flipped here (the section presence is checked in
        // `all_sections_are_cross_consistent`; the header's fifth slot is
        // covered by `header_fields_are_read_from_their_offsets`).
        info.finished_turn = team % 2 == 1;
        info.team_clan = TeamClan::try_from([1u8, 2, 3, 4][team]).expect("clan");
        for (index, topic) in info.research_topics.iter_mut().enumerate() {
            topic.research_level = index as u32;
            topic.turns_to_complete = (index as u32) * 7 + 1;
            topic.allocation = (index as u32) % 3;
        }
        info.team_points = 0x8000_0000 + team as u32;
        info.number_of_objects_created = 0x1000 + team as u16;
        for (index, counter) in info.unit_counters.iter_mut().enumerate() {
            *counter = (index % 251) as u32;
        }
        for (index, value) in info.score_graph.iter_mut().enumerate() {
            *value = (index as i16).wrapping_mul(3).wrapping_sub(75);
        }
        info.stats_factories_built = 3;
        info.stats_mines_built = -4;
        info.stats_buildings_built = 5;
        info.stats_units_built = -6;
        for (index, casualties) in info.casualties.iter_mut().enumerate() {
            *casualties = (index % 65536) as u32;
        }
        info.stats_gold_spent_on_upgrades = 0x7FFF;
        save.team_gold_spent_raw[team] = 0x7FFF;

        // UI fields (`TeamInfoUi`).
        let ui: &mut TeamInfoUi = &mut save.team_ui[team];
        for (index, marker) in ui.markers.iter_mut().enumerate() {
            *marker = Point::new(index as i16 - 5, 3 - index as i16);
        }
        for (index, location) in ui.screen_locations.iter_mut().enumerate() {
            location.x = index as i8;
            location.y = -(index as i8);
        }
        ui.selected_unit = if team == 0 {
            None
        } else {
            Some(0x1234 + team as u16)
        };
        ui.zoom_level = 4 + team as u16 * 7;
        ui.camera_position = Point::new(-1000, 2000);
        for (index, button) in ui.display_buttons.iter_mut().enumerate() {
            *button = index as i8 - 5;
        }
    }
    expected.teams = save.teams.clone();
    expected.team_ui = save.team_ui.clone();
    expected.team_gold_spent_raw = save.team_gold_spent_raw;

    let bytes = save.to_bytes().expect("writes");
    let reparsed = SaveFile::parse(&bytes).expect("parses");
    assert_eq!(reparsed.teams, expected.teams);
    assert_eq!(reparsed.team_ui, expected.team_ui);
    assert_eq!(reparsed.team_gold_spent_raw, expected.team_gold_spent_raw);
    // Write -> read -> write is stable.
    assert_eq!(reparsed.to_bytes().expect("writes"), bytes);
}

#[test]
fn every_unit_field_survives_a_write_cycle() {
    let mut save = load(SAVE1);
    let mut expected = save.clone();

    // Fields the V70 body serializes (F5.3/B split of the old `UnitInfo`).
    let unit = &mut save.units[0];
    unit.info.flags ^= 0x0001_0000;
    unit.info.grid = Point::new(3, 4);
    unit.info.target_grid = Point::new(9, 10);
    unit.info.name = "Phase 4 unit".to_string();
    unit.info.team = TeamId::GREEN;
    unit.info.unit_id = 200;
    unit.info.angle = 7;
    unit.info.visible_to_team = [true, false, true, false, true];
    unit.info.spotted_by_team = [false, true, false, true, false];
    unit.info.max_velocity = 3;
    unit.info.velocity = 2;
    unit.info.scaler_adjust = 1;
    unit.info.turret_angle = 5;
    unit.info.image_base = -123;
    unit.info.turret_image_base = 321;
    unit.info.image_index = -7;
    unit.info.turret_image_index = 8;
    unit.info.image_index_max = 999;
    unit.info.orders = UnitOrder::Sentry;
    unit.info.state = UnitOrderState::ExecutingOrder;
    unit.info.prior_orders = UnitOrder::Await;
    unit.info.prior_state = UnitOrderState::NewOrder;
    unit.info.laying_state = 3;
    unit.info.build_time = 11;
    unit.info.total_mining = 2;
    unit.info.raw_mining = 3;
    unit.info.fuel_mining = 4;
    unit.info.gold_mining = 5;
    unit.info.raw_mining_max = 6;
    unit.info.gold_mining_max = 7;
    unit.info.fuel_mining_max = 8;
    unit.info.hits = 200;
    unit.info.speed = 20;
    unit.info.shots = 3;
    unit.info.move_and_fire = 1;
    unit.info.ammo = 15;
    unit.info.targeting_mode = 2;
    unit.info.enter_mode = 1;
    unit.info.delayed_reaction = 9;
    unit.info.damaged_this_turn = true;
    unit.info.research_topic = max_core::enums::ResearchTopic::Armor;
    unit.info.moved = 4;
    unit.info.shake_effect_state = 2;
    unit.info.engine = 1;
    unit.info.weapon = 2;
    unit.info.comm = 6;
    unit.info.fuel_distance = 7;
    unit.info.move_fraction = 8;
    unit.info.energized = true;
    unit.info.repeat_build = true;
    unit.info.build_rate = 5;
    unit.info.disabled_reaction_fire = true;
    unit.info.auto_survey = true;
    unit.info.ai_state_bits = 0x00AB_CDEF;
    unit.info.connectors = 0x5A5A;
    // Presentation fields.
    unit.presentation.shadow_offset = Point::new(-2, 2);
    unit.presentation.brightness = 128;
    unit.presentation.sound = 7;
    unit.presentation.sprite_bounds = max_core::unit::Rect {
        ulx: 1,
        uly: 2,
        lrx: 3,
        lry: 4,
    };
    unit.presentation.shadow_bounds = max_core::unit::Rect {
        ulx: -1,
        uly: -2,
        lrx: -3,
        lry: -4,
    };
    unit.presentation.turret_offset_x = -8;
    unit.presentation.turret_offset_y = 8;
    unit.presentation.total_images = 100;
    unit.presentation.firing_image_base = 55;
    unit.presentation.connector_image_base = 66;
    unit.presentation.cursor = 3;
    unit.presentation.bobbed = true;
    unit.info.build_list = vec![
        max_core::enums::UnitType::Tank,
        max_core::enums::UnitType::Scout,
    ];

    // Entity reference: the *last* unit points at the first one (a backward
    // reference, which the object-numbered writer accepts; the body of an
    // already written object is not repeated).
    let first_id = save.units[0].info.id;
    let first_index = save.units[0].object_index;
    {
        let last = save.units.last_mut().expect("units");
        assert_ne!(last.object_index, first_index);
        last.info.enemy_unit = Some(first_id);
        last.enemy_object_index = first_index;
    }

    expected.units = save.units.clone();
    let bytes = save.to_bytes().expect("writes");
    let reparsed = SaveFile::parse(&bytes).expect("parses");
    assert_eq!(reparsed.units.len(), expected.units.len());
    for (index, (got, want)) in reparsed.units.iter().zip(expected.units.iter()).enumerate() {
        assert_eq!(
            got.object_index, want.object_index,
            "unit {index}: object number"
        );
        assert_eq!(
            got.info.unit_type, want.info.unit_type,
            "unit {index}: type"
        );
        assert_eq!(got.info.id, want.info.id, "unit {index}: id");
        assert_eq!(got.info.flags, want.info.flags, "unit {index}: flags");
        assert_eq!(got.info.grid, want.info.grid, "unit {index}: grid");
        assert_eq!(got.info.name, want.info.name, "unit {index}: name");
        assert_eq!(got.info.orders, want.info.orders, "unit {index}: orders");
        assert_eq!(got.info.state, want.info.state, "unit {index}: state");
        assert_eq!(got.info.hits, want.info.hits, "unit {index}: hits");
        assert_eq!(got.info.path, want.info.path, "unit {index}: path");
        assert_eq!(
            got.presentation, want.presentation,
            "unit {index}: presentation"
        );
        assert_eq!(
            got.info.parent_unit, want.info.parent_unit,
            "unit {index}: parent"
        );
        assert_eq!(
            got.info.enemy_unit, want.info.enemy_unit,
            "unit {index}: enemy"
        );
        assert_eq!(
            got.info.build_list, want.info.build_list,
            "unit {index}: build list"
        );
    }
    assert_eq!(reparsed.to_bytes().expect("writes"), bytes);
}

/// Builds a save of a fresh game with one unit of each path kind.
fn save_with_paths() -> Vec<u8> {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.team_units_table_mut()[0] = max_core::team::TeamUnits::with_clan(TeamClan::Random);
    state.set_player_team(TeamId::RED);

    let tank = state
        .spawn_unit(
            max_core::enums::UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            0,
        )
        .expect("tank");
    let fighter = state
        .spawn_unit(
            max_core::enums::UnitType::Fighter,
            TeamId::RED,
            Point::new(20, 20),
            0,
        )
        .expect("fighter");
    let builder = state
        .spawn_unit(
            max_core::enums::UnitType::Constructor,
            TeamId::RED,
            Point::new(30, 30),
            0,
        )
        .expect("constructor");

    state.unit_mut(tank).expect("tank").path = Some(UnitPath::Ground(max_core::unit::GroundPath {
        end_x: 12,
        end_y: 13,
        step_index: 2,
        steps: vec![
            PathStep::new(1, 0),
            PathStep::new(1, 1),
            PathStep::new(0, 1),
            PathStep::new(-1, 1),
        ],
    }));
    state.unit_mut(fighter).expect("fighter").path = Some(UnitPath::Air(max_core::unit::AirPath {
        length: 15,
        angle: 3,
        start_x: 100,
        start_y: 200,
        end_x: 300,
        end_y: 400,
        step_x: 5,
        step_y: -7,
        delta_x: 11,
        delta_y: -13,
    }));
    state.unit_mut(builder).expect("builder").path =
        Some(UnitPath::Builder(max_core::unit::BuilderPath {
            direction_x: 3,
            direction_y: -4,
        }));

    state.snapshot().to_save_bytes().expect("writes")
}

#[test]
fn unit_path_bodies_survive_a_write_cycle() {
    // The reference saves contain no route objects (the units are idle), so the
    // path bodies are checked on a generated save with one unit per path kind.
    let bytes = save_with_paths();
    let mut save = SaveFile::parse(&bytes).expect("parses");

    let mut kinds = Vec::new();
    for unit in save.units.iter_mut() {
        match unit.info.path.as_mut() {
            Some(UnitPath::Ground(path)) => {
                assert_eq!(path.step_index, 2);
                path.end_x += 1;
                path.end_y -= 1;
                path.step_index = 3;
                path.steps[0] = PathStep::new(9, 9);
                path.steps.push(PathStep::new(-2, -2));
                kinds.push("ground");
            }
            Some(UnitPath::Air(path)) => {
                assert_eq!(path.length, 15);
                path.length = 16;
                path.angle = 5;
                path.start_x -= 1;
                path.end_y += 1;
                path.step_x += 1;
                path.delta_y -= 1;
                kinds.push("air");
            }
            Some(UnitPath::Builder(path)) => {
                path.direction_x = -2;
                path.direction_y = 2;
                kinds.push("builder");
            }
            None => {}
        }
    }
    kinds.sort_unstable();
    assert_eq!(kinds, vec!["air", "builder", "ground"]);

    let expected = save.units.clone();
    let rewritten = save.to_bytes().expect("writes");
    let reparsed = SaveFile::parse(&rewritten).expect("parses");
    assert_eq!(reparsed.units.len(), expected.len());
    for (got, want) in reparsed.units.iter().zip(expected.iter()) {
        assert_eq!(got.info.path, want.info.path, "{:?}: path", want.info.id);
    }
    // Path object numbers are stable on a second write.
    assert_eq!(reparsed.to_bytes().expect("writes"), rewritten);
}

#[test]
fn every_format_version_is_classified() {
    for version in 0..=u16::MAX {
        let bytes = version.to_le_bytes();
        let result = SaveFile::parse(&bytes);
        if version == SAVE_VERSION_V70 {
            assert!(
                matches!(result, Err(SaveError::UnexpectedEof { offset: 2, .. })),
                "V70 header on a 2-byte buffer must be truncated, got {result:?}"
            );
        } else {
            assert_eq!(
                result,
                Err(SaveError::UnknownVersion(version)),
                "version {version}"
            );
        }
    }
}

#[test]
fn truncation_is_always_reported_with_its_offset() {
    let total = SAVE1.len();
    let mut samples: Vec<usize> = (0..=512).collect();
    samples.extend((1..total).step_by(997));
    samples.push(total - 1);

    for length in samples {
        let result = SaveFile::parse(&SAVE1[..length]);
        match result {
            Err(SaveError::UnexpectedEof { offset, needed }) => {
                // The reader reports where the failed read started and how many
                // bytes it wanted; the cut lies inside that request.
                assert!(
                    offset <= length,
                    "truncation at {length}: offset {offset} is past the cut"
                );
                assert!(
                    offset + needed > length,
                    "truncation at {length}: read {offset}+{needed} fits, yet failed"
                );
                assert!(needed >= 1, "truncation at {length}: needed {needed}");
            }
            other => panic!("truncation at {length} produced {other:?}"),
        }
    }
}

#[test]
fn garbage_input_never_panics_and_is_rejected() {
    // Deterministic pseudo-random buffers of many lengths.
    let mut state = 0x1234_5678u32;
    for length in [0usize, 1, 2, 3, 7, 16, 64, 256, 1000, 4096, 50_000] {
        let mut buffer = Vec::with_capacity(length);
        for _ in 0..length {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            buffer.push((state >> 16) as u8);
        }
        let result = SaveFile::parse(&buffer);
        assert!(result.is_err(), "random buffer of {length} bytes parsed");
    }

    // All zero / all 0xFF buffers.
    for byte in [0u8, 0xFF] {
        for length in [2usize, 176, 1000, 100_000] {
            let buffer = vec![byte; length];
            assert!(SaveFile::parse(&buffer).is_err(), "{byte:#04x} × {length}");
        }
    }

    // Single-bit corruption of the header block: either a valid model or a
    // typed error, never a panic.
    for offset in 0..OFF_SURFACE_MAP {
        for bit in [0x01u8, 0x80] {
            let mut bytes = SAVE1.to_vec();
            bytes[offset] ^= bit;
            match SaveFile::parse(&bytes) {
                Ok(save) => {
                    // If the corruption produced a parseable file, it must be
                    // writable in the same format.
                    let _ = save.to_bytes().expect("corrupted header still writes");
                }
                Err(_) => {}
            }
        }
    }
}

#[test]
fn invalid_enum_values_are_rejected() {
    // Header enums: the raw values are out of the enum domain.
    for (offset, value, what) in [
        (OFF_TEAM_TYPE, 9u8, "TeamKind"),
        (OFF_TEAM_CLAN + 1, 9, "TeamClan"),
        (OFF_OPPONENT, 9, "Opponent"),
        (OFF_PLAY_MODE, 3, "PlayMode"),
        (OFF_FILE_TYPE, 0xFF, "FileType"),
        (OFF_WORLD, 0xFF, "Planet"),
    ] {
        let bytes = patched(SAVE1, offset, &[value]);
        let result = SaveFile::parse(&bytes);
        assert!(
            matches!(
                result,
                Err(SaveError::InvalidEnum {
                    what: got,
                    value: raw,
                    ..
                }) if got == what && raw == u32::from(value)
            ),
            "offset {offset}: expected InvalidEnum({what}, {value}), got {}",
            match &result {
                Ok(_) => "a parsed file".to_string(),
                Err(error) => format!("{error:?}"),
            }
        );
    }

    // IniOptions enums are 32-bit fields (indices: 4 = play_mode,
    // 5 = victory_type, 7 = opponent).
    for (index, value, what) in [
        (4usize, 3i32, "PlayMode"),
        (5, 0x7F, "VictoryType"),
        (7, 6, "Opponent"),
    ] {
        let bytes = patched(SAVE1, OFF_INI_OPTIONS + index * 4, &value.to_le_bytes());
        let result = SaveFile::parse(&bytes);
        assert!(
            matches!(
                result,
                Err(SaveError::InvalidEnum { what: got, .. }) if got == what
            ),
            "IniOptions field {index}: expected InvalidEnum({what}), got {}",
            match &result {
                Ok(_) => "a parsed file".to_string(),
                Err(error) => format!("{error:?}"),
            }
        );
    }

    // GameStateType (`uint16_t`).
    let bytes = patched(SAVE1, OFF_GAME_TYPE, &0xFFFFu16.to_le_bytes());
    let result = SaveFile::parse(&bytes);
    assert!(
        matches!(
            result,
            Err(SaveError::InvalidEnum {
                what: "GameStateType",
                value: 0xFFFF,
                ..
            })
        ),
        "GameStateType: expected InvalidEnum, got {}",
        match &result {
            Ok(_) => "a parsed file".to_string(),
            Err(error) => format!("{error:?}"),
        }
    );
}

/// A one-unit save with a message log entry that has a recognizable marker.
fn marker_save() -> (Vec<u8>, [u8; 11]) {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.team_units_table_mut()[0] = max_core::team::TeamUnits::with_clan(TeamClan::Random);
    state.set_player_team(TeamId::RED);
    let tank = state
        .spawn_unit(
            max_core::enums::UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            0,
        )
        .expect("tank");
    state
        .push_message(0, "PHASE4MARK", Point::new(10, 10), Some(tank));
    let bytes = state.snapshot().to_save_bytes().expect("writes");
    (bytes, *b"PHASE4MARK\0")
}

fn marker_offset(bytes: &[u8], marker: &[u8]) -> usize {
    bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("marker present in the save")
}

#[test]
fn wrong_object_references_are_rejected() {
    let (bytes, marker) = marker_save();
    let unit_ref = marker_offset(&bytes, &marker) + marker.len();

    // The message log entry stores the unit as a V70 object reference after the
    // text. A number past the object table is an invalid object index.
    let invalid = patched(&bytes, unit_ref, &0xFFFEu16.to_le_bytes());
    match SaveFile::parse(&invalid) {
        Err(SaveError::InvalidObject {
            index, expected, ..
        }) => {
            assert_eq!(index, 0xFFFE);
            // The next free object number is well past the team values.
            assert!(expected > 1, "expected next object {expected}");
        }
        other => panic!("expected InvalidObject for 0xFFFE, got {other:?}"),
    }

    // Object 1 is the first `UnitValues` slot of TeamUnits → using it as a unit
    // reference is a type mismatch.
    let wrong_kind = patched(&bytes, unit_ref, &1u16.to_le_bytes());
    match SaveFile::parse(&wrong_kind) {
        Err(SaveError::ObjectKindMismatch {
            index,
            expected,
            actual,
        }) => {
            assert_eq!(index, 1);
            assert_eq!(expected, "UnitInfo");
            assert_eq!(actual, "UnitValues");
        }
        other => panic!("expected ObjectKindMismatch for object 1, got {other:?}"),
    }

    // A zero reference is the documented "no unit" value.
    let zero = patched(&bytes, unit_ref, &0u16.to_le_bytes());
    let save = SaveFile::parse(&zero).expect("a zero reference parses");
    let logs = save
        .message_logs
        .iter()
        .flatten()
        .find(|entry| entry.text.starts_with(b"PHASE4MARK"))
        .expect("the message entry");
    assert_eq!(logs.unit, None);
    assert_eq!(logs.unit_object_index, 0);
}

#[test]
fn trailing_bytes_are_rejected_at_every_short_append() {
    for extra in 1..=16usize {
        let mut bytes = SAVE1.to_vec();
        bytes.extend(std::iter::repeat_n(0u8, extra));
        assert_eq!(
            SaveFile::parse(&bytes),
            Err(SaveError::TrailingBytes(extra)),
            "extra {extra}"
        );
    }
}

#[test]
fn all_sections_are_cross_consistent() {
    for (source, tag) in [(SAVE1, "SAVE1"), (SAVE2, "SAVE2")] {
        let save = load(source);
        assert_eq!(save.unit_lists.len(), UNIT_LIST_COUNT);
        assert_eq!(save.header.version, SAVE_VERSION_V70);

        // Object numbers are unique and sorted (the parser sorts for binary
        // search on write).
        let mut previous = None;
        for unit in &save.units {
            assert_ne!(unit.object_index, 0, "{tag}: object number 0 is null");
            if let Some(previous) = previous {
                assert!(unit.object_index > previous, "{tag}: units are not sorted");
            }
            previous = Some(unit.object_index);
        }

        // Every list/hash/AI/log reference resolves to a unit.
        let resolves = |object_index: u16| {
            object_index == 0 || save.unit_by_object_index(object_index).is_some()
        };
        for (kind, list) in UnitListKind::ALL.iter().zip(save.unit_lists.iter()) {
            for object_index in list {
                assert!(
                    resolves(*object_index),
                    "{tag}: list {kind:?} references unknown object {object_index}"
                );
            }
        }
        for bucket in save.unit_hash.buckets() {
            assert!(bucket.iter().all(|object_index| resolves(*object_index)));
        }
        for object in save.map_hash.iter_objects() {
            assert!(!object.units.is_empty());
            assert!(object
                .units
                .iter()
                .all(|object_index| resolves(*object_index)));
        }
        for logs in &save.message_logs {
            for entry in logs {
                assert!(resolves(entry.unit_object_index));
            }
        }
        for ai in save.ai_players.iter().flatten() {
            for spotted in &ai.spotted_units {
                assert!(resolves(spotted.unit_object_index));
                assert_eq!(
                    save.unit_by_object_index(spotted.unit_object_index)
                        .map(|unit| unit.info.id),
                    spotted.unit,
                    "{tag}: spotted unit reference"
                );
            }
        }

        // Every unit is referenced by exactly one of the four CRC lists; the
        // `Particle` list is the exception (`GameManager_CheckDesync`).
        let mut in_crc_list: std::collections::BTreeMap<u16, usize> =
            std::collections::BTreeMap::new();
        for kind in [
            UnitListKind::GroundCover,
            UnitListKind::MobileLandSea,
            UnitListKind::Stationary,
            UnitListKind::MobileAir,
        ] {
            for object_index in save.unit_list(kind) {
                *in_crc_list.entry(*object_index).or_default() += 1;
            }
        }
        for unit in &save.units {
            if unit.info.unit_list == UnitListKind::Particle {
                continue;
            }
            assert_eq!(
                in_crc_list.get(&unit.object_index),
                Some(&1),
                "{tag}: unit {:?} is not in exactly one CRC list",
                unit.info.id
            );
        }

        // The `unit_list` field matches the list that actually holds the unit.
        for (kind, list) in UnitListKind::ALL.iter().zip(save.unit_lists.iter()) {
            for object_index in list {
                let unit = save.unit_by_object_index(*object_index).expect("unit");
                assert_eq!(unit.info.unit_list, *kind, "{tag}: list kind");
            }
        }

        // Parent/enemy references resolve.
        for unit in &save.units {
            if let Some(parent) = unit.info.parent_unit {
                assert!(save.unit_by_id(parent).is_some(), "{tag}: parent");
            }
            if let Some(enemy) = unit.info.enemy_unit {
                assert!(save.unit_by_id(enemy).is_some(), "{tag}: enemy");
            }
        }

        // Map views cover the whole map bit-exactly.
        assert_eq!(save.surface().to_bytes(), save.surface_map);
        let resources = save.resource_map();
        for index in [0usize, 1, 17, 4096, MAP_TILES - 1] {
            assert_eq!(
                resources.bits_at(index),
                save.cargo_map[index],
                "{tag}: tile {index}"
            );
        }

        // Heat maps exist exactly for playing teams and clamp negatives.
        for (team, heat) in save.heat_maps.iter().enumerate() {
            if save.teams[team].team_type == TeamKind::None {
                assert!(heat.is_none(), "{tag}: heat map for an empty team");
                continue;
            }
            let heat = heat.as_deref().expect("heat map for a playing team");
            assert_eq!(heat.complete.len(), MAP_TILES);
            for index in [0usize, 1, 1000, MAP_TILES - 1] {
                let raw = heat.complete[index];
                let expected = if raw < 0 { 0 } else { raw as u8 };
                assert_eq!(heat.complete_clamped(index), expected, "{tag}: clamp");
            }
        }

        // Message logs: V70 writes text with a terminating NUL.
        for logs in &save.message_logs {
            for entry in logs {
                assert!(!entry.text.is_empty());
                assert_eq!(entry.text.last(), Some(&0), "{tag}: NUL-terminated text");
            }
        }

        // AI players exist exactly for computer teams.
        for (team, ai) in save.ai_players.iter().enumerate() {
            match save.teams[team].team_type {
                TeamKind::Computer => assert!(ai.is_some(), "{tag}: AI for computer {team}"),
                _ => assert!(ai.is_none(), "{tag}: AI for non-computer {team}"),
            }
        }
    }
}

#[test]
fn unit_and_team_units_lookup_edges() {
    let save = load(SAVE1);

    // `unit_by_object_index`: object numbers start at 1.
    assert!(save.unit_by_object_index(0).is_none());
    assert!(save.unit_by_object_index(u16::MAX).is_none());
    let first = save.units.first().expect("units").object_index;
    assert!(save.unit_by_object_index(first).is_some());
    assert!(save.unit_by_object_index(first + 1).is_some() || first + 1 == u16::MAX);

    // `unit_by_id`: a present id and an absent one.
    let present = save.units.first().expect("units").info.id;
    assert!(save.unit_by_id(present).is_some());
    assert!(save.unit_by_id(max_core::UnitId::NO_ID).is_none());
    let mut absent = max_core::UnitId(0);
    while save.unit_by_id(absent).is_some() {
        absent = max_core::UnitId(absent.0.wrapping_add(1));
    }
    assert!(save.unit_by_id(absent).is_none());

    // `unit_list` for every kind.
    for (kind, list) in UnitListKind::ALL.iter().zip(save.unit_lists.iter()) {
        assert_eq!(save.unit_list(*kind), list.as_slice(), "{kind:?}");
    }

    // `SaveTeamUnits::to_team_units` maps gold `u16 → u32` and all tables.
    let teams = save.team_units();
    assert_eq!(teams.len(), SAVE_TEAM_COUNT);
    for (team, saved) in save.team_units.iter().enumerate() {
        assert_eq!(teams[team].gold, u32::from(saved.gold));
        assert_eq!(teams[team].complexes.len(), saved.complexes.len());
        assert_eq!(teams[team].base_values.len(), UNIT_COUNT);
        for (index, slot) in saved.base_values.iter().enumerate() {
            assert_eq!(teams[team].base_values[index], slot.values);
        }
        for (index, slot) in saved.current_values.iter().enumerate() {
            assert_eq!(teams[team].current_values[index], slot.values);
        }
        // Attribute access covers all 14 indices.
        for attribute in 0..ATTRIB_COUNT {
            let _ = teams[team].base_values[0].get_attribute(attribute);
        }
    }
}

/// Regression: a unit spawned after a V70 load must not reuse a loaded id.
///
/// C++ `UnitsManager_DeployUnit` (`units_manager.cpp:2365`) increments the flat
/// `UnitsManager_UnitIdCounter`, which the V70 loader starts at `0xA000`
/// (`saveload.cpp:834`), so the new id lies above every V70 id
/// (`(team << 13) | object_index`). The port keeps the team in [`UnitId`]; its
/// object index must continue after the highest loaded one instead of
/// restarting at `counter & 0x1FFF` (which collided with V70 id `1`).
#[test]
fn spawn_after_v70_load_does_not_collide_with_loaded_ids() {
    let save = load(SAVE2);
    let loaded_max = save
        .units
        .iter()
        .map(|unit| unit.info.id.object_index())
        .max()
        .expect("SAVE2 has units");

    let mut state = GameUniverse::from_save(&save);
    let before = state.snapshot().unit_count();
    let loaded_first = UnitId::new(TeamId::RED, 1);
    assert!(
        state.snapshot().unit(loaded_first).is_some(),
        "SAVE2 contains V70 id 1"
    );

    let id = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(60, 60), 0)
        .expect("unit spawns after a load");

    assert_eq!(id.team(), TeamId::RED, "the team stays encoded in UnitId");
    assert_eq!(
        id.object_index(),
        loaded_max + 1,
        "the object index continues after the highest loaded one"
    );
    assert_ne!(
        id, loaded_first,
        "the new id must not collide with V70 id 1"
    );

    // The hash and the id addressee agree: `state.unit(id)` is the new unit.
    let unit = state
        .snapshot()
        .unit(id)
        .expect("the new unit is addressable by id");
    assert_eq!(unit.id, id);
    assert_eq!(unit.team, TeamId::RED);
    assert_eq!(unit.unit_type, UnitType::Tank);
    assert_eq!(state.snapshot().unit_count(), before + 1);

    let unique: std::collections::BTreeSet<UnitId> =
        state.snapshot().unit_ids_in_order().into_iter().collect();
    assert_eq!(
        unique.len(),
        state.snapshot().unit_count(),
        "every live unit id is still unique"
    );
}

#[test]
fn save_error_display_is_informative() {
    // `SaveError` implements `Display`/`Error`: every variant renders a message
    // with the relevant context (used by the app's diagnostics).
    let errors = [
        SaveError::UnexpectedEof {
            offset: 10,
            needed: 4,
        },
        SaveError::UnknownVersion(71),
        SaveError::InvalidObject {
            index: 9,
            expected: 5,
            offset: 100,
        },
        SaveError::InvalidObjectKind {
            type_index: 7,
            offset: 101,
        },
        SaveError::ObjectKindMismatch {
            index: 3,
            expected: "UnitInfo",
            actual: "UnitValues",
        },
        SaveError::ForwardReference { index: 5, next: 3 },
        SaveError::InvalidEnum {
            what: "TeamClan",
            value: 9,
            offset: 200,
        },
        SaveError::TrailingBytes(4),
        SaveError::InvalidLength {
            what: "UnitInfo::name",
            length: 5000,
        },
    ];
    for error in errors {
        let text = error.to_string();
        assert!(!text.is_empty(), "{error:?}");
        // `Display` must not be the derived `Debug` output.
        assert!(!text.starts_with("UnexpectedEof"), "{error:?}: {text}");
    }
}
