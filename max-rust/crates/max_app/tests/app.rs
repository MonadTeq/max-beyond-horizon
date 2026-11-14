//! Интеграционные тесты `max_app`: demo-прогон, локальная партия, команды.
//!
//! Фикстура `tests/data/save1_v70.cam` — копия
//! `max-rust/crates/max_core/tests/data/save1_v70.cam` (кампания 1, 312 юнитов).
//!
//! Тесты не требуют сети и SDL: demo-режим и `apply` работают на чистом
//! `max_core`, новый кадр — на чистых частях `max_render`. Единственный
//! «живой» тест (`assets_integration_smoke`) сам пропускается, если данных игры
//! (`max-original/MAX.RES`) нет рядом с репозиторием.

use std::path::{Path, PathBuf};

use max_app::{App, AppConfig, AppDigest, NewGameConfig};
use max_core::crc;
use max_core::enums::Planet;
use max_core::events::command_move_linear;
use max_core::events::ApplyOutcome;
use max_core::geo::{Point, MAP_WIDTH};
use max_core::setup::derive_surface_map;
use max_core::setup::ResourceLevel;
use max_core::{TeamId, UnitListKind, UnitOrder, UnitOrderState};

const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");

/// Эталон `--demo tests/data/save1_v70.cam --turns 5`, получен прогоном.
///
/// Фиксируется в тесте, чтобы любое изменение симуляции/загрузки сейва
/// (регрессия паритета) падало явно, а не «тихо» меняло дайджест.
const SAVE1_FIVE_TICK_STATE_DIGEST: u16 = 0xC354;
const SAVE1_FIVE_TICK_STATE_CHECKSUM: u16 = 0x408A;
const SAVE1_FIVE_TICK_REPLAY_CRC: u16 = 0x5561;
const SAVE1_FIVE_TICK_UNITS: usize = 312;
const SAVE1_FIVE_TICK_TURN: i32 = 1;
const SAVE1_FIVE_TICK_SNAPSHOT_LEN: usize = 788_026;

fn fixture_save_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/save1_v70.cam")
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

// ---------------------------------------------------------------------------
// 1. Demo на SAVE1.CAM: load → 5 тиков → эталонный digest
// ---------------------------------------------------------------------------

#[test]
fn demo_save1_five_ticks_matches_reference_digest() {
    let mut app = App::new(AppConfig::default());
    let digest = app
        .run_demo(fixture_save_path(), 5)
        .expect("demo на SAVE1.CAM");

    assert_eq!(digest.ticks, 5);
    assert_eq!(
        digest.units, SAVE1_FIVE_TICK_UNITS,
        "число юнитов не совпало с эталоном: {digest}"
    );
    assert_eq!(digest.turn_counter, SAVE1_FIVE_TICK_TURN);
    assert_eq!(
        digest.state_digest, SAVE1_FIVE_TICK_STATE_DIGEST,
        "state_digest разошёлся с эталоном: {digest}"
    );
    assert_eq!(
        digest.state_checksum, SAVE1_FIVE_TICK_STATE_CHECKSUM,
        "state_checksum (пакет 45) разошёлся: {digest}"
    );
    assert_eq!(
        digest.replay_crc, SAVE1_FIVE_TICK_REPLAY_CRC,
        "replay_crc разошёлся: {digest}"
    );
    assert_eq!(
        digest.snapshot_len, SAVE1_FIVE_TICK_SNAPSHOT_LEN,
        "длина снапшота разошлась: {digest}"
    );

    // Digest снимается в любой момент и не зависит от предыдущих вызовов.
    let again = app.digest();
    assert_eq!(again, digest);
}

#[test]
fn demo_is_bit_identical_across_two_runs() {
    let first = App::new(AppConfig::default())
        .run_demo(fixture_save_path(), 5)
        .expect("первый demo-прогон");
    let second = App::new(AppConfig::default())
        .run_demo(fixture_save_path(), 5)
        .expect("второй demo-прогон");

    assert_eq!(first, second, "demo недетерминирован: {first} != {second}");

    // Полный снапшот тоже обязан совпасть (тест паритета, а не только digest).
    let mut left = App::new(AppConfig::default());
    left.load_save_bytes(SAVE1).expect("загрузка SAVE1.CAM");
    let mut right = App::new(AppConfig::default());
    right.load_save_bytes(SAVE1).expect("загрузка SAVE1.CAM");

    for _ in 0..5 {
        left.tick();
        right.tick();
    }

    assert_eq!(
        left.state().snapshot_bytes(),
        right.state().snapshot_bytes()
    );
    assert_eq!(left.digest(), right.digest());
}

// ---------------------------------------------------------------------------
// 2. new_game по параметрам P2-вектора `p00_l111_d0_s0`
// ---------------------------------------------------------------------------

/// Первое поле записи `case p00_l111_d0_s0` из `tools/map_vectors/vectors.txt`.
fn p2_reference_field(field: &str) -> String {
    const VECTORS: &str = include_str!("../../../tools/map_vectors/vectors.txt");
    const CASE: &str = "p00_l111_d0_s0";
    let mut inside = false;

    for line in VECTORS.lines() {
        let line = line.trim();

        if let Some(name) = line.strip_prefix("case ") {
            inside = name.trim() == CASE;
            continue;
        }
        if inside {
            if let Some(value) = line.strip_prefix(&format!("{field} ")) {
                return value.trim().to_string();
            }
        }
    }

    panic!("в vectors.txt нет case {CASE} с полем {field}");
}

fn hex_bytes(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0, "нечётная длина hex-строки");

    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("hex-байт"))
        .collect()
}

fn hex_u16_le(text: &str) -> Vec<u16> {
    let bytes = hex_bytes(text);

    bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

#[test]
fn new_game_with_p2_parameters_is_deterministic_and_renders() {
    // Вход P2-вектора: та же поверхность, что синтезирует генератор векторов
    // (`build_world_input`: tile_ids + pass_table для planet 0).
    let pass_table = hex_bytes(&p2_reference_field("pass_table"));
    let tile_ids = hex_u16_le(&p2_reference_field("tile_ids"));
    let expected_surface = hex_bytes(&p2_reference_field("surface"));
    let surface = derive_surface_map(&tile_ids, &pass_table);

    assert_eq!(
        surface.to_bytes().as_slice(),
        expected_surface.as_slice(),
        "вход поверхности разошёлся с P2-вектором"
    );

    // Ресурсная фаза идёт до alien-фазы и не зависит от таблицы стоимости
    // обломков, поэтому карта ресурсов обязана совпасть с вектором бит-в-бит.
    let expected_resources = hex_u16_le(&p2_reference_field("resources"));
    let expected_resources_bytes: Vec<u8> = expected_resources
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let expected_resources_crc = crc::crc16(&expected_resources_bytes);

    let config = NewGameConfig::p2_reference();
    let mut app = App::new(AppConfig::default().with_screen(320, 240));
    let report = app.new_game_on_surface(&config, &surface);

    assert_eq!(report.planet, config.planet);
    assert_eq!(report.seed, config.seed);
    assert_eq!(report.teams, 2);
    assert_eq!(
        report.resources_crc, expected_resources_crc,
        "карта ресурсов разошлась с P2-вектором"
    );
    assert_eq!(
        report.surface_crc,
        crc::crc16(&expected_surface),
        "surface_crc должен совпадать с P2-вектором"
    );
    assert_eq!(
        report.units, 0,
        "derelicts=0 не должен создавать юнитов в GameState"
    );
    // У P2-вектора `p00_l111_d0_s0` уровень обломков 0 — размещений нет
    // (в самом векторе тоже 0 строк `placement`).
    assert_eq!(
        report.aliens, 0,
        "derelicts=0 не должен разворачивать обломки"
    );

    // Тот же вход в другом App — тот же отчёт и тот же digest.
    let mut replay = App::new(AppConfig::default().with_screen(320, 240));
    let replay_report = replay.new_game_on_surface(&config, &surface);
    assert_eq!(report, replay_report);
    assert_eq!(app.digest(), replay.digest());

    // render_frame: размеры буфера и отсутствие паник (карта + маркеры/спрайты).
    let (frame, stats) = app.render_frame_with_stats();
    assert_eq!(frame.width(), 320);
    assert_eq!(frame.height(), 240);
    assert_eq!(frame.pixels().len(), 320 * 240 * 4);
    assert_eq!(frame.stride(), 320 * 4);
    assert!(stats.map_tiles > 0);
    assert_eq!(stats.units(), report.units);
    assert!(frame.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));

    // Уровень обломков 2 даёт непустую расстановку из setup, и все размещения
    // разворачиваются в GameState от имени ALIEN (C++ `DeployUnit`).
    let mut with_aliens = config.clone();
    with_aliens.derelicts = ResourceLevel::HIGH;

    let mut app_aliens = App::new(AppConfig::default().with_screen(320, 240));
    let aliens_report = app_aliens.new_game_on_surface(&with_aliens, &surface);

    assert!(
        aliens_report.aliens > 0,
        "derelicts=2 должен дать размещения: {aliens_report:?}"
    );
    assert_eq!(
        aliens_report.units, aliens_report.aliens,
        "каждое размещение alien-обломков должно стать юнитом GameState"
    );
    assert_eq!(
        aliens_report.resources_crc, expected_resources_crc,
        "уровень обломков не влияет на карту ресурсов"
    );
    assert_eq!(app_aliens.digest().units, aliens_report.units);

    // Юниты — в списках, хэше карты и с корректной командой; у зданий заняты
    // все четыре клетки 2×2 (`Hash_MapHash.Add`).
    {
        let state = app_aliens.state();
        let alien_ids: Vec<_> = state
            .unit_ids_in_order()
            .into_iter()
            .filter(|id| state.unit(*id).is_some_and(|unit| unit.team == TeamId::ALIEN))
            .collect();

        assert_eq!(alien_ids.len(), aliens_report.units);
        assert_eq!(
            state.unit_ids_in_order().len(),
            aliens_report.units,
            "кроме alien-обломков в новой партии юнитов нет"
        );

        for id in &alien_ids {
            let unit = state.unit(*id).expect("юнит из unit_ids_in_order");
            assert!(
                state.unit_list(unit.unit_list).contains(id),
                "юнит {id:?} должен быть в списке {:?}",
                unit.unit_list
            );
            assert!(
                state.units_at(unit.grid).contains(id),
                "юнит {id:?} должен быть в map hash клетки {:?}",
                unit.grid
            );

            if unit.flags & max_core::enums::unit_flags::BUILDING != 0 {
                let right = Point::new(unit.grid.x + 1, unit.grid.y);
                let down = Point::new(unit.grid.x, unit.grid.y + 1);
                let corner = Point::new(unit.grid.x + 1, unit.grid.y + 1);
                for cell in [right, down, corner] {
                    assert!(
                        state.units_at(cell).contains(id),
                        "здание {id:?} должно занимать клетку {cell:?} (2×2)"
                    );
                }
            }
        }
    }

    // Тик не паникует, дайджест стабилен между одинаковыми прогонами.
    let mut replay_aliens = App::new(AppConfig::default().with_screen(320, 240));
    let replay_aliens_report = replay_aliens.new_game_on_surface(&with_aliens, &surface);
    assert_eq!(aliens_report, replay_aliens_report);
    assert_eq!(app_aliens.digest(), replay_aliens.digest());

    for _ in 0..3 {
        app_aliens.tick();
        replay_aliens.tick();
    }
    assert_eq!(app_aliens.digest(), replay_aliens.digest());
    assert_eq!(
        app_aliens.digest().units, aliens_report.units,
        "alien-обломки не исчезают за тики"
    );

    let (aliens_frame, aliens_stats) = app_aliens.render_frame_with_stats();
    assert_eq!((aliens_frame.width(), aliens_frame.height()), (320, 240));
    // В камеру 320×240 попадает лишь часть карты, поэтому рисуется не больше
    // юнитов, чем есть в состоянии (обломки обычно вне стартового вида).
    assert!(aliens_stats.units() <= aliens_report.units);
}

// ---------------------------------------------------------------------------
// 3. Move-команда max_core через App (пакет 38)
// ---------------------------------------------------------------------------

#[test]
fn move_command_replays_through_app() {
    fn run() -> AppDigest {
        let mut app = App::new(AppConfig::default());
        app.load_save_bytes(SAVE1).expect("загрузка SAVE1.CAM");

        let id = app
            .state()
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                app.state().unit(*id).is_some_and(|unit| {
                    unit.team == TeamId::RED && unit.unit_list == UnitListKind::MobileLandSea
                })
            })
            .expect("в SAVE1.CAM есть мобильный юнит красных");

        let from = app.state().unit(id).expect("юнит существует").grid;
        let to = Point::new((from.x + 3).min(MAP_WIDTH - 2), from.y);

        // Ровно тот же помощник, что в тестах max_core (`packet_38_builds_ground_path`).
        let command = command_move_linear(id, from, to, UnitOrder::Move, UnitOrderState::Init);

        assert_eq!(app.apply(command), ApplyOutcome::Applied);
        assert_eq!(app.applied_commands(), 1);
        assert!(
            app.state().unit(id).expect("юнит").path.is_some(),
            "пакет 38 обязан выставить путь"
        );
        assert_eq!(app.state().order_data(id).move_to, to);

        for _ in 0..3 {
            app.tick();
        }

        app.digest()
    }

    let first = run();
    let second = run();

    assert_eq!(first, second, "move-реплей недетерминирован");
    assert_eq!(first.ticks, 3);
}

// ---------------------------------------------------------------------------
// 4. Живой smoke на настоящих данных игры (если они есть рядом)
// ---------------------------------------------------------------------------

#[test]
fn assets_integration_smoke() {
    let data_dir = repo_path("max-original");
    let max_res = data_dir.join("MAX.RES");
    let color_pal = data_dir.join("COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!(
            "assets_integration_smoke: пропуск — нет {} / {} / {}",
            max_res.display(),
            color_pal.display(),
            patches.display()
        );
        return;
    }

    let config = AppConfig::default()
        .with_data_dir(&data_dir)
        .with_assets_dir(repo_path("assets"))
        .with_screen(256, 192);
    let mut app = App::new(config);
    let report = app.init_assets().expect("данные игры разбираются");

    assert!(report.max_res_entries > 1000, "MAX.RES: {report:?}");
    assert!(report.patches_res_entries > 0, "PATCHES.RES: {report:?}");
    assert_eq!(report.world, "SNOW_1");
    assert_eq!(report.world_size, (112, 112));
    assert!(
        report.sprite_count() >= 10,
        "ожидались спрайты из UNIT_SPRITES: {report:?}"
    );

    app.load_save(fixture_save_path()).expect("SAVE1.CAM");

    // Центрируем камеру на юните локального игрока, для типа которого спрайт
    // реально загружен из MAX.RES: иначе в маленькое окно может не попасть ни
    // один рисуемый спрайт (всё зависит от того, где стоит первый юнит).
    let mut player_types: Vec<u16> = Vec::new();
    for id in app.state().unit_ids_in_order() {
        let Some(unit) = app.state().unit(id) else {
            continue;
        };
        if unit.team == app.state().player_team {
            player_types.push(unit.unit_type as u16);
        }
    }
    player_types.sort_unstable();
    player_types.dedup();

    let target = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state().unit(*id).is_some_and(|unit| {
                unit.team == app.state().player_team
                    && app
                        .assets()
                        .is_some_and(|assets| assets.sprite(unit.unit_type).is_some())
            })
        })
        .unwrap_or_else(|| {
            panic!("нет спрайта ни для одного типа красных; типы: {player_types:?}")
        });

    let grid = app.state().unit(target).expect("юнит").grid;
    app.viewport_mut().center_on_grid(grid.x, grid.y);

    let (frame, stats) = app.render_frame_with_stats();

    assert_eq!((frame.width(), frame.height()), (256, 192));
    assert!(stats.map_tiles > 0);
    assert!(
        stats.units_drawn > 0,
        "на кадре должен быть хотя бы один спрайт (drawn={}, marker={}, типы красных={player_types:?})",
        stats.units_drawn,
        stats.units_marker
    );

    // `App::new_game` (путь с настоящими ассетами): карта планеты из .WRL.
    // NewLuzon (индекс 12) — это GREEN_1.WRL.
    let report = app
        .new_game(&NewGameConfig {
            planet: Planet::NewLuzon,
            seed: 7,
            ..NewGameConfig::p2_reference()
        })
        .expect("new_game на настоящих ассетах");
    assert_eq!(report.planet, Planet::NewLuzon);
    assert_eq!(report.seed, 7);
    assert_eq!(report.teams, 2);
    assert_eq!(report.units, 0);
    assert!(report.resources_crc != 0);
}
