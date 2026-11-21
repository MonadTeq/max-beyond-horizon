//! Интеграционные тесты V70 на **реальных** сейвах оригинальной игры.
//!
//! Фикстуры — копии файлов из `max-original/` (не сгенерированы):
//!
//! * `tests/data/save1_v70.cam` — копия `max-original/SAVE1.CAM` (183 047 байт);
//! * `tests/data/save2_v70.cam` — копия `max-original/SAVE2.CAM` (216 410 байт).
//!
//! Тесты проверяют, что парсер проходит файл до последнего байта (все секции:
//! заголовок, `IniOptions`/`IniPreferences`, карты, `TeamInfo`×4, состояние
//! партии, `TeamUnits`×4, пять списков `UnitInfo`, `UnitHash`, `MapHash`,
//! тепловые карты, журналы сообщений, `AiPlayer`), что значения заголовка
//! совпадают с реальными, и что запись возвращает **побайтово** тот же файл.

use max_core::enums::{FileType, GameStateType, Opponent, Planet, PlayMode, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::save::{SaveError, SaveFile};
use max_core::team::{TeamId, TeamKind};

/// `max-original/SAVE1.CAM`: обучающая кампания «Islands in the Sun».
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM`: кампания «Heart of the Matter».
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

#[test]
fn save1_header_matches_original_file() {
    let save = SaveFile::parse(SAVE1).expect("SAVE1.CAM разбирается целиком");
    let header = &save.header;

    // Реальные значения заголовка SAVE1.CAM:
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

    // Состояние партии: ход 1, фаза IN_GAME, активные красные.
    assert_eq!(save.game.active_turn_team, TeamId::RED);
    assert_eq!(save.game.player_team, TeamId::RED);
    assert_eq!(save.game.turn_counter, 1);
    assert_eq!(save.game.game_state, GameStateType::InGame);
    assert_eq!(save.game.turn_timer, 0);

    // TeamInfo: 87 и 288 созданных объектов, 52 золота у красных, 2 комплекса.
    assert_eq!(save.teams[0].number_of_objects_created, 87);
    assert_eq!(save.teams[1].number_of_objects_created, 288);
    assert_eq!(save.team_units[0].gold, 52);
    assert_eq!(save.team_units[0].complexes.len(), 2);
    assert_eq!(save.team_units[1].complexes.len(), 3);
    assert_eq!(save.teams[0].unit_counters[UnitType::CommTower as usize], 2);
    // UI-поля: камера красных (85, 52), зум 35, выбран юнит 0x0057.
    assert_eq!(save.team_ui[0].camera_position, Point::new(85, 52));
    assert_eq!(save.team_ui[0].zoom_level, 35);
    assert_eq!(save.team_ui[0].selected_unit, Some(0x0057));
    assert_eq!(save.team_ui[1].camera_position, Point::new(30, 38));

    // Пять списков: ground cover 102, mobile land/sea 60, stationary 132,
    // mobile air 18, particles 0 — всего 312 юнитов.
    let counts: Vec<usize> = save.unit_lists.iter().map(Vec::len).collect();
    assert_eq!(counts, vec![102, 60, 132, 18, 0]);
    assert_eq!(save.units.len(), 312);

    // Хэши: 512 ведёрок у обоих, x_shift = 2; в хэше юнитов все 312 объектов.
    assert_eq!(save.unit_hash.size(), 512);
    assert_eq!(save.map_hash.size(), 512);
    assert_eq!(save.map_hash.x_shift(), 2);
    assert_eq!(
        save.unit_hash.iter_values().count(),
        save.units.len(),
        "UnitHash должен содержать все прочитанные юниты"
    );

    // Тепловые карты — только у двух играющих команд.
    assert!(save.heat_maps[0].is_some());
    assert!(save.heat_maps[1].is_some());
    assert!(save.heat_maps[2].is_none());
    assert!(save.heat_maps[3].is_none());

    // Журналы пусты, состояние ИИ есть только у компьютерной команды 1.
    assert!(save.message_logs.iter().all(Vec::is_empty));
    assert!(save.ai_players[0].is_none());
    let ai = save.ai_players[1].as_deref().expect("AI team 1");
    assert_eq!(ai.player_team, 1);
    assert_eq!(ai.strategy, 0);
    assert_eq!(ai.greenhouse_ratio, -1);
    assert_eq!(ai.target_location, Point::new(1, 1));

    // Карта: 12 544 клетки поверхности и столько же записей ресурсов.
    assert_eq!(save.surface_map.len(), 12_544);
    assert_eq!(save.cargo_map.len(), 12_544);
    assert!(save.surface_map.iter().all(|byte| *byte != 0));
}

#[test]
fn save1_round_trip_is_byte_identical() {
    let save = SaveFile::parse(SAVE1).expect("SAVE1.CAM разбирается целиком");
    let bytes = save.to_bytes().expect("SAVE1.CAM записывается");
    assert_eq!(bytes.len(), SAVE1.len());
    assert_eq!(bytes, SAVE1, "запись V70 должна совпадать побайтово");
}

#[test]
fn save2_header_matches_original_file() {
    let save = SaveFile::parse(SAVE2).expect("SAVE2.CAM разбирается целиком");
    let header = &save.header;

    // Реальные значения заголовка SAVE2.CAM:
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

    // Списки юнитов: 63 + 145 + 76 + 1 + 0 = 285.
    let counts: Vec<usize> = save.unit_lists.iter().map(Vec::len).collect();
    assert_eq!(counts, vec![63, 145, 76, 1, 0]);
    assert_eq!(save.units.len(), 285);

    // Три команды в игре → три тепловые карты.
    assert!(save.heat_maps[0].is_some());
    assert!(save.heat_maps[1].is_some());
    assert!(save.heat_maps[2].is_some());
    assert!(save.heat_maps[3].is_none());

    // Журналы сообщений: по одной записи у красных, зелёных и синих.
    let log_counts: Vec<usize> = save.message_logs.iter().map(Vec::len).collect();
    assert_eq!(log_counts, vec![1, 1, 1, 0]);
    for entry in &save.message_logs[0] {
        // V70 пишет `strlen + 1`, то есть текст с завершающим нулём.
        assert!(entry.text.last() == Some(&0), "текст с NUL на конце");
        assert!(!entry.text.is_empty());
        assert!(!entry.is_alert_message || entry.point != Point::ZERO || entry.unit.is_some());
    }

    // Два компьютерных игрока — два блока `AiPlayer`.
    assert!(save.ai_players[0].is_none());
    assert_eq!(save.ai_players[1].as_deref().map(|ai| ai.player_team), Some(1));
    assert_eq!(save.ai_players[2].as_deref().map(|ai| ai.player_team), Some(2));

    // Кланы влияют на базовые значения: Musashi усиливает танк (+4 атаки).
    let tank = save.team_units[0].base_values[UnitType::Tank as usize].values;
    let plain = max_core::base_unit_values(UnitType::Tank);
    assert_eq!(tank.attack, plain.attack + 4);
}

#[test]
fn save2_round_trip_is_byte_identical() {
    let save = SaveFile::parse(SAVE2).expect("SAVE2.CAM разбирается целиком");
    let bytes = save.to_bytes().expect("SAVE2.CAM записывается");
    assert_eq!(bytes, SAVE2, "запись V70 должна совпадать побайтово");
}

#[test]
fn truncated_files_are_rejected() {
    // Обрезанный файл не должен «успешно» разбираться.
    for length in [2, 100, 4_000, SAVE1.len() / 2, SAVE1.len() - 1] {
        assert!(
            SaveFile::parse(&SAVE1[..length]).is_err(),
            "обрезка до {length} байт должна давать ошибку"
        );
    }
    // Лишний байт в конце — тоже ошибка (парсер обязан пройти файл целиком).
    let mut extended = SAVE1.to_vec();
    extended.push(0);
    assert_eq!(
        SaveFile::parse(&extended),
        Err(SaveError::TrailingBytes(1))
    );
}
