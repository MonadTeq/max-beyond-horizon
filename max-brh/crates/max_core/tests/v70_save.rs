//! V70 integration tests on **real** saves of the original game.
//!
//! Fixtures — copies of files from `max-original/` (not generated):
//!
//! * `tests/data/save1_v70.cam` — a copy of `max-original/SAVE1.CAM` (183,047 bytes);
//! * `tests/data/save2_v70.cam` — a copy of `max-original/SAVE2.CAM` (216,410 bytes).
//!
//! The tests check that the parser reads the file to the last byte (all sections:
//! header, `IniOptions`/`IniPreferences`, maps, `TeamInfo`×4, game
//! state, `TeamUnits`×4, five `UnitInfo` lists, `UnitHash`, `MapHash`,
//! heat maps, message logs, `AiPlayer`), that the header values
//! match the real ones, and that writing returns the same file **byte for byte**.

use max_core::enums::{FileType, GameStateType, Opponent, Planet, PlayMode, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::save::{SaveError, SaveFile};
use max_core::team::{TeamId, TeamKind};

/// `max-original/SAVE1.CAM`: the training campaign "Islands in the Sun".
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM`: the "Heart of the Matter" campaign.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

#[test]
fn save1_header_matches_original_file() {
    let save = SaveFile::parse(SAVE1).expect("SAVE1.CAM разбирается целиком");
    let header = &save.header;

    // Real header values of SAVE1.CAM:
    //   version = 70             (V70)
    //   save_game_type = 2       (FileType::Campaign)
    //   save_name = "Islands in the Sun"
    //   world = 16               (Planet::Islandia)
    //   mission_index = 1
    //   team_type = [Player, Computer, None, None, None]
    //   team_clan = [VonGriffin (3), SevenKnights (7), SevenKnights, AxisInc (8), Random]
    //   rng_seed = 0x32A61291
    //   opponent = 3             (Opponent::Expert)
    //   turn_timer_time = 0, endturn_time = 0
    //   play_mode = 1            (PlayMode::SimultaneousMoves)
    assert_eq!(header.version, 70);
    assert_eq!(header.save_game_type, FileType::Campaign);
    assert_eq!(header.save_name(), "Islands in the Sun");
    assert_eq!(header.world, Planet::Islandia);
    assert_eq!(header.mission_index, 1);
    assert_eq!(
        header.team_type,
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ]
    );
    assert_eq!(
        header.team_clan,
        [
            TeamClan::VonGriffin,
            TeamClan::SevenKnights,
            TeamClan::SevenKnights,
            TeamClan::AxisInc,
            TeamClan::Random,
        ]
    );
    assert_eq!(header.rng_seed, 0x32A6_1291);
    assert_eq!(header.opponent, Opponent::Expert);
    assert_eq!(header.turn_timer_time, 0);
    assert_eq!(header.endturn_time, 0);
    assert_eq!(header.play_mode, PlayMode::SimultaneousMoves);
    assert_eq!(header.team_name(0), "Player 1");
    assert_eq!(header.team_name(1), "Green Team");
    assert_eq!(header.team_name(2), "");

    // IniOptions: [16, 0, 0, 200, 1, 0, 30, 3, 1, 1, 1, 0].
    assert_eq!(save.ini_options.world, 16);
    assert_eq!(save.ini_options.turn_timer, 0);
    assert_eq!(save.ini_options.endturn, 0);
    assert_eq!(save.ini_options.start_gold, 200);
    assert_eq!(save.ini_options.play_mode, PlayMode::SimultaneousMoves);
    assert_eq!(save.ini_options.victory_limit, 30);
    assert_eq!(save.ini_options.raw_resource, 1);
    assert_eq!(save.ini_options.fuel_resource, 1);
    assert_eq!(save.ini_options.gold_resource, 1);
    assert_eq!(save.ini_options.alien_derelicts, 0);

    // IniPreferences: [1, 0, 40, 1, 0, 0, 1].
    assert_eq!(save.ini_preferences.effects, 1);
    assert_eq!(save.ini_preferences.click_scroll, 0);
    assert_eq!(save.ini_preferences.quick_scroll, 40);
    assert_eq!(save.ini_preferences.fast_movement, 1);
    assert_eq!(save.ini_preferences.follow_unit, 0);
}

#[test]
fn save1_all_sections_are_present() {
    let save = SaveFile::parse(SAVE1).expect("SAVE1.CAM разбирается целиком");

    // Game state: turn 1, phase IN_GAME, red active.
    assert_eq!(save.game.active_turn_team, TeamId::RED);
    assert_eq!(save.game.player_team, TeamId::RED);
    assert_eq!(save.game.turn_counter, 1);
    assert_eq!(save.game.game_state, GameStateType::InGame);
    assert_eq!(save.game.turn_timer, 0);

    // TeamInfo: 87 and 288 created objects, 52 gold for red, 2 complexes.
    assert_eq!(save.teams[0].number_of_objects_created, 87);
    assert_eq!(save.teams[1].number_of_objects_created, 288);
    assert_eq!(save.teams[0].team_points, 0);
    assert_eq!(save.teams[1].team_points, 4);
    assert_eq!(save.teams[0].team_clan, TeamClan::VonGriffin);
    assert_eq!(save.team_units[0].gold, 52);
    assert_eq!(save.team_units[0].complexes.len(), 2);
    assert_eq!(save.team_units[1].complexes.len(), 3);
    assert_eq!(
        save.teams[0].team_unit_counters[UnitType::GoldRefinery as usize],
        2
    );
    // UI fields: red camera (85, 52), zoom 35, unit 0x0057 selected.
    assert_eq!(save.team_ui[0].camera_position, Point::new(85, 52));
    assert_eq!(save.team_ui[0].zoom_level, 35);
    assert_eq!(save.team_ui[0].selected_unit, Some(0x0057));
    assert_eq!(save.team_ui[1].camera_position, Point::new(30, 38));

    // Five lists: ground cover 102, mobile land/sea 60, stationary 132,
    // mobile air 18, particles 0 — 312 units in total.
    let counts: Vec<usize> = save.unit_lists.iter().map(Vec::len).collect();
    assert_eq!(counts, vec![102, 60, 132, 18, 0]);
    assert_eq!(save.units.len(), 312);

    // Hashes: 512 buckets in both, x_shift = 2; all 312 objects in the unit hash.
    assert_eq!(save.unit_hash.size(), 512);
    assert_eq!(save.map_hash.size(), 512);
    assert_eq!(save.map_hash.x_shift(), 2);
    assert_eq!(
        save.unit_hash.iter_values().count(),
        save.units.len(),
        "UnitHash должен содержать все прочитанные юниты"
    );

    // Heat maps — only for the two playing teams.
    assert!(save.heat_maps[0].is_some());
    assert!(save.heat_maps[1].is_some());
    assert!(save.heat_maps[2].is_none());
    assert!(save.heat_maps[3].is_none());

    // The logs are empty, AI state exists only for computer team 1.
    assert!(save.message_logs.iter().all(Vec::is_empty));
    assert!(save.ai_players[0].is_none());
    let ai = save.ai_players[1].as_deref().expect("AI team 1");
    assert_eq!(ai.player_team, 1);
    assert_eq!(ai.strategy, 0);
    assert_eq!(ai.greenhouse_ratio, -1);
    assert_eq!(ai.target_location, Point::new(1, 1));

    // Map: 12,544 surface cells and as many resource entries.
    assert_eq!(save.surface_map.len(), 12_544);
    assert_eq!(save.cargo_map.len(), 12_544);
    assert!(save.surface_map.iter().all(|byte| *byte != 0));
}

#[test]
fn save1_round_trip_is_byte_identical() {
    let save = SaveFile::parse(SAVE1).expect("SAVE1.CAM разбирается целиком");
    let bytes = save.to_bytes().expect("SAVE1.CAM записывается");
    assert_eq!(bytes.len(), SAVE1.len());
    assert_eq!(
        bytes, SAVE1,
        "запись V70 должна совпадать побайтово"
    );
}

#[test]
fn save2_header_matches_original_file() {
    let save = SaveFile::parse(SAVE2).expect("SAVE2.CAM разбирается целиком");
    let header = &save.header;

    // Real header values of SAVE2.CAM:
    //   version = 70, category = 2 (Campaign), name = "Heart of the Matter",
    //   world = 9 (Planet::ValentinesPlanet), mission_index = 2,
    //   team_type = [Player, Computer, Computer, None, None],
    //   team_clan = [Musashi (5), AxisInc (8), AyersHand (4), CrimsonPath (2), Random],
    //   rng_seed = 0x329B8D8E, opponent = 3, play_mode = 1.
    assert_eq!(header.version, 70);
    assert_eq!(header.save_name(), "Heart of the Matter");
    assert_eq!(header.world, Planet::ValentinesPlanet);
    assert_eq!(header.mission_index, 2);
    assert_eq!(
        header.team_type,
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ]
    );
    assert_eq!(
        header.team_clan,
        [
            TeamClan::Musashi,
            TeamClan::AxisInc,
            TeamClan::AyersHand,
            TeamClan::CrimsonPath,
            TeamClan::Random,
        ]
    );
    assert_eq!(header.rng_seed, 0x329B_8D8E);
    assert_eq!(header.opponent, Opponent::Expert);
    assert_eq!(header.play_mode, PlayMode::SimultaneousMoves);
    assert_eq!(header.team_name(2), "blue");
    assert_eq!(save.ini_options.start_gold, 250);
    assert_eq!(save.ini_options.raw_resource, 2);
}

#[test]
fn save2_sections_logs_and_two_ai_players() {
    let save = SaveFile::parse(SAVE2).expect("SAVE2.CAM разбирается целиком");

    // Unit lists: 63 + 145 + 76 + 1 + 0 = 285.
    let counts: Vec<usize> = save.unit_lists.iter().map(Vec::len).collect();
    assert_eq!(counts, vec![63, 145, 76, 1, 0]);
    assert_eq!(save.units.len(), 285);

    // Three teams in play → three heat maps.
    assert!(save.heat_maps[0].is_some());
    assert!(save.heat_maps[1].is_some());
    assert!(save.heat_maps[2].is_some());
    assert!(save.heat_maps[3].is_none());

    // Message logs: one entry each for red, green and blue.
    let log_counts: Vec<usize> = save.message_logs.iter().map(Vec::len).collect();
    assert_eq!(log_counts, vec![1, 1, 1, 0]);
    for entry in &save.message_logs[0] {
        // V70 writes `strlen + 1`, i.e. text with a terminating zero.
        assert!(
            entry.text.last() == Some(&0),
            "текст с NUL на конце"
        );
        assert!(!entry.text.is_empty());
        assert!(!entry.is_alert_message || entry.point != Point::ZERO || entry.unit.is_some());
    }

    // Two computer players — two `AiPlayer` blocks.
    assert!(save.ai_players[0].is_none());
    assert_eq!(
        save.ai_players[1].as_deref().map(|ai| ai.player_team),
        Some(1)
    );
    assert_eq!(
        save.ai_players[2].as_deref().map(|ai| ai.player_team),
        Some(2)
    );

    // Clans affect base values: Musashi strengthens the tank (+4 attack).
    let tank = save.team_units[0].base_values[UnitType::Tank as usize].values;
    let plain = max_core::base_unit_values(UnitType::Tank);
    assert_eq!(tank.attack, plain.attack + 4);
}

#[test]
fn save2_round_trip_is_byte_identical() {
    let save = SaveFile::parse(SAVE2).expect("SAVE2.CAM разбирается целиком");
    let bytes = save.to_bytes().expect("SAVE2.CAM записывается");
    assert_eq!(
        bytes, SAVE2,
        "запись V70 должна совпадать побайтово"
    );
}

#[test]
fn truncated_files_are_rejected() {
    // A truncated file must not parse "successfully".
    for length in [2, 100, 4_000, SAVE1.len() / 2, SAVE1.len() - 1] {
        assert!(
            SaveFile::parse(&SAVE1[..length]).is_err(),
            "обрезка до {length} байт должна давать ошибку"
        );
    }
    // An extra byte at the end is also an error (the parser must read the whole file).
    let mut extended = SAVE1.to_vec();
    extended.push(0);
    assert_eq!(SaveFile::parse(&extended), Err(SaveError::TrailingBytes(1)));
}
