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
use max_core::enums::{GameStateType, Planet};
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
    // M3.2: `GameManager_InitGame` высаживает стартовые армии обеих команд
    // (шахта, генератор, две плиты и загрузка SC_L0001), иначе условия
    // победы/поражения считают команду выбитой на первом же ходу.
    let per_team = 4 + max_core::DEFAULT_MISSION_LOADOUT.len();
    assert_eq!(report.start_units, 2 * per_team);
    assert_eq!(
        report.units, report.start_units,
        "derelicts=0 не должен создавать alien-юнитов, только стартовые армии"
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
    // Камера центрируется на стартовой базе игрока, поэтому на кадр попадает
    // только часть юнитов (остальные — вне вьюпорта).
    assert!(
        stats.units() > 0 && stats.units() <= report.units,
        "на кадре должны быть юниты: {stats:?} при {} в состоянии",
        report.units
    );
    assert!(frame.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));

    // M3.2: стартовые армии не дают партии завершиться на первом ходу —
    // `menu_check_end_game_conditions` больше не видит команды «выбитыми».
    for turn in 0..3 {
        app.state_mut().game_state = GameStateType::EndTurn;
        assert!(
            !app.state_mut().finish_turn(),
            "партия не должна завершаться на ходу {turn}"
        );
    }
    assert_eq!(
        app.digest().units,
        report.units,
        "ходы не элиминируют команду"
    );

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
        aliens_report.units,
        aliens_report.aliens + aliens_report.start_units,
        "юниты состояния = alien-обломки + стартовые армии"
    );
    assert_eq!(
        aliens_report.start_units, report.start_units,
        "уровень обломков не влияет на стартовые армии"
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
            .filter(|id| {
                state
                    .unit(*id)
                    .is_some_and(|unit| unit.team == TeamId::ALIEN)
            })
            .collect();

        assert_eq!(alien_ids.len(), aliens_report.aliens);
        assert_eq!(
            state.unit_ids_in_order().len(),
            aliens_report.units,
            "кроме alien-обломков в новой партии есть только стартовые армии"
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
        app_aliens.digest().units,
        aliens_report.units,
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
fn visual_units_smoothly_catch_up_and_snap_without_dt() {
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
        .expect("мобильный юнит RED");

    let from = app.state().unit(id).expect("юнит").grid;
    let to = Point::new((from.x + 2).min(MAP_WIDTH - 2), from.y);
    let command = command_move_linear(id, from, to, UnitOrder::Move, UnitOrderState::Init);
    app.apply(command);

    // Без dt — снимок: визуальная позиция равна симуляционной.
    app.update_visual_units(0.0);
    assert_eq!(
        app.visual_position(id),
        Some((f32::from(from.x), f32::from(from.y)))
    );

    for _ in 0..6 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);
    }

    let visual = app.visual_position(id).expect("юнит жив");
    assert!(
        visual.0 > f32::from(from.x),
        "кадр плавно догоняет движение: {visual:?} vs {from:?}"
    );

    app.update_visual_units(0.0);
    let grid = app.state().unit(id).expect("юнит").grid;
    assert_eq!(
        app.visual_position(id),
        Some((f32::from(grid.x), f32::from(grid.y))),
        "dt=0 (headless/QA) — без интерполяции"
    );
}

#[test]
fn visual_angle_stays_on_the_segment_being_animated() {
    use max_core::enums::{unit_flags, UnitType};
    use max_core::geo::PathStep;
    use max_core::unit::{GroundPath, UnitPath};
    use max_core::unit_order::paths_get_angle;

    let mut app = App::new(AppConfig::default());
    app.state_mut().teams[0].team_type = max_core::team::TeamKind::Player;
    app.state_mut().team_units[0] =
        max_core::team::TeamUnits::with_clan(max_core::TeamClan::Random);
    let id = app
        .state_mut()
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("танк");

    // L-маршрут: 2 клетки на восток, затем 2 на север.
    let mut unit = app.state().unit(id).expect("танк").clone();
    unit.angle = 2;
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::InProgress;
    unit.path = Some(UnitPath::Ground(GroundPath {
        end_x: 12,
        end_y: 8,
        step_index: 0,
        steps: vec![
            PathStep::new(1, 0),
            PathStep::new(1, 0),
            PathStep::new(0, -1),
            PathStep::new(0, -1),
        ],
    }));
    app.state_mut().write_unit(id, unit);
    app.update_visual_units(0.0);
    assert_eq!(app.visual_angle(id), Some(2));

    // Первый тик: шаг на восток; следующий шаг тоже на восток — спрайт и
    // симуляция смотрят на восток.
    app.tick();
    assert_eq!(app.state().unit(id).expect("танк").grid, Point::new(11, 10));
    app.update_visual_units(1.0 / 60.0);
    assert_eq!(app.visual_angle(id), Some(2));

    // Второй тик: шаг на (12,10); на границе клетки симуляция уже начала
    // поворот к северу (`GroundPath::UpdateUnitAngle`: E → NE), но спрайт
    // ещё идёт по восточному отрезку и обязан смотреть на восток.
    app.tick();
    assert_eq!(app.state().unit(id).expect("танк").grid, Point::new(12, 10));
    assert_eq!(app.state().unit(id).expect("танк").angle, 1);
    app.update_visual_units(1.0 / 60.0);
    assert_eq!(
        app.visual_angle(id),
        Some(2),
        "спрайт движется на восток — кадр обязан смотреть на восток, а не NE"
    );

    // Сим и кадр идут вместе: пока спрайт смещается по оси, его угол равен
    // направлению смещения (осевые отрезки этого маршрута).
    let mut previous = app.visual_position(id).expect("танк");
    for frame in 0..40 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);
        let now = app.visual_position(id).expect("танк");
        let dx = now.0 - previous.0;
        let dy = now.1 - previous.1;
        let step_x = i32::from(dx > 1e-4) - i32::from(dx < -1e-4);
        let step_y = i32::from(dy > 1e-4) - i32::from(dy < -1e-4);

        // Кадр не может пересечь угол маршрута; если это случилось (диагональ
        // интерполяции/переход), направление смещения неоднозначно — пропуск.
        if step_x != 0 && step_y != 0 {
            previous = now;
            continue;
        }
        if step_x != 0 || step_y != 0 {
            let angle = paths_get_angle(step_x, step_y) as u8;
            assert_eq!(
                app.visual_angle(id),
                Some(angle),
                "кадр {frame} смотрит не по ходу: смещение ({dx}, {dy}), позиция {now:?}"
            );
        }
        previous = now;
    }

    // Доигрываем кадр без тиков симуляции: спрайт доходит до последней клетки,
    // после чего угол снова берётся из симуляции (последнее направление).
    for _ in 0..300 {
        app.update_visual_units(1.0 / 60.0);
    }

    let unit = app.state().unit(id).expect("танк");
    assert_eq!(unit.grid, Point::new(12, 8));
    assert_eq!(unit.angle, 0);
    assert_eq!(
        app.visual_position(id),
        Some((12.0, 8.0)),
        "по завершении маршрута визуальная позиция совпадает с клеткой"
    );
    assert_eq!(app.visual_angle(id), Some(0));
}

#[test]
fn movement_is_visible_between_frames_and_finishes_on_the_target_cell() {
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
        .expect("мобильный юнит RED");

    let from = app.state().unit(id).expect("юнит").grid;
    let to = Point::new((from.x + 2).min(MAP_WIDTH - 2), from.y);
    app.apply(command_move_linear(
        id,
        from,
        to,
        UnitOrder::Move,
        UnitOrderState::Init,
    ));

    // `dt = 0` — стартовый снимок (ровно симуляционная клетка).
    app.update_visual_units(0.0);
    let start = app.visual_position(id).expect("юнит жив");
    assert_eq!(start, (f32::from(from.x), f32::from(from.y)));

    // Ждём тик, на котором симуляция делает первый шаг маршрута: приказ
    // `Init` может сначала запросить путь (`NEW_ORDER`/`ISSUING_PATH`).
    let mut ticks = 0;
    while app.state().unit(id).map(|unit| unit.grid) == Some(from) && ticks < 120 {
        app.tick();
        ticks += 1;
    }
    assert_ne!(
        app.state().unit(id).map(|unit| unit.grid),
        Some(from),
        "юнит не сдвинулся за {ticks} тиков"
    );

    // Кадр с реальным `dt` идёт по маршруту: два последовательных кадра
    // различаются позицией спрайта (видимое движение, а не телепорт).
    app.update_visual_units(1.0 / 60.0);
    let first = app.visual_position(id).expect("юнит жив");
    assert_ne!(first, start, "после шага кадр обязан сдвинуться: {first:?}");

    // Пиксельная проверка: два кадра подряд различаются (маркер/спрайт
    // сдвинулся), без тика симуляции между ними.
    app.viewport_mut().center_on_grid(from.x, from.y);
    let first_frame = app.render_frame();

    app.update_visual_units(1.0 / 60.0);
    let second = app.visual_position(id).expect("юнит жив");
    let second_frame = app.render_frame();
    assert_ne!(second, first, "два кадра с dt>0 должны различаться позицией");
    assert!(second.0 > first.0, "спрайт плавно идёт вправо: {second:?}");
    assert_ne!(
        first_frame.pixels(),
        second_frame.pixels(),
        "кадр карты должен измениться: спрайт юнита сместился"
    );

    // Визуальное состояние — только кадр: CRC/снапшот не меняются.
    let snapshot = app.snapshot();
    app.update_visual_units(1.0 / 60.0);
    assert_eq!(
        app.snapshot(),
        snapshot,
        "update_visual_units не должен менять симуляцию"
    );

    // Доводим маршрут: 2 клетки при 4 клетках/с — ~30 кадров, берём запас.
    for _ in 0..300 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);
    }

    let target = app.state().unit(id).expect("юнит жив").grid;
    assert_eq!(
        app.visual_position(id),
        Some((f32::from(target.x), f32::from(target.y))),
        "по завершении маршрута визуальная позиция совпадает с целевой клеткой"
    );
}

#[test]
fn move_command_replays_through_app() {    fn run() -> AppDigest {
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
    assert_eq!(
        report.start_units,
        2 * (4 + max_core::DEFAULT_MISSION_LOADOUT.len()),
        "на реальной карте стартовые армии тоже высаживаются"
    );
    assert_eq!(report.units, report.start_units);
    assert!(report.resources_crc != 0);
}

// ---------------------------------------------------------------------------
// 4. Хром 1:1: координаты мыши и клики по панелям
// ---------------------------------------------------------------------------

/// Загружает приложение с ассетами игры; `None`, если данных нет рядом.
fn chrome_app() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        return None;
    }

    let config = AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        ..Default::default()
    };
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");
    app.new_game(&NewGameConfig::p2_reference())
        .expect("партия");

    Some(app)
}

#[test]
fn chrome_click_on_the_map_centre_selects_the_unit_under_it() {
    let Some(mut app) = chrome_app() else {
        return;
    };

    assert!(app.chrome_active(), "640×480 + FRAMEPIC = хром 1:1");

    let unit = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .find(|unit| unit.team == app.player_team())
        .expect("стартовый юнит игрока");
    let grid = unit.grid;

    app.viewport_mut().center_on_grid(grid.x, grid.y);

    // Клик в центр клетки на экране: карта начинается с (180,18), и без
    // пересчёта координат клик попадал бы в (0,0) области карты (риск 4).
    let map_view = app.map_viewport();
    let tile = map_view.tile_rect(grid.x, grid.y);
    let x = max_render::layout::MAIN_MAP.ulx + tile.ulx + tile.width / 2;
    let y = max_render::layout::MAIN_MAP.uly + tile.uly + tile.height / 2;

    assert_eq!(
        map_view.screen_to_grid(tile.ulx + tile.width / 2, tile.uly + tile.height / 2),
        (grid.x, grid.y)
    );

    let transition = max_app::ui::handle_session_input(
        &mut app,
        max_app::ui::InputEvent::MouseClick(x, y),
        false,
    );

    assert_eq!(transition, max_app::ui::Transition::None);
    let selected = app.hud().selected.expect("клик по карте выбирает юнита");
    assert_eq!(
        app.state().unit(selected).expect("юнит").grid,
        grid,
        "клик в центр окна карты выбирает юнита в ожидаемой клетке"
    );
}

#[test]
fn chrome_end_turn_click_is_a_button_not_a_map_click() {
    let Some(mut app) = chrome_app() else {
        return;
    };
    let rect = max_render::layout::END_TURN_BUTTON;
    let x = rect.ulx + rect.width / 2;
    let y = rect.uly + rect.height / 2;

    assert_eq!(
        max_app::panels::panel_button_at(x, y),
        Some(max_app::PanelButton::EndTurn)
    );
    assert_eq!(
        max_app::panels::screen_to_map(x, y),
        None,
        "End Turn заходит на карту, но клик принадлежит кнопке"
    );

    max_app::ui::handle_session_input(&mut app, max_app::ui::InputEvent::MouseClick(x, y), true);

    assert!(
        app.hud().end_turn_requested,
        "клик по End Turn ставит запрос закрытия хода"
    );
    assert_eq!(app.hud().selected, None, "клик не ушёл в карту");
}

#[test]
fn chrome_toggle_and_zoom_clicks_work_on_the_panels() {
    let Some(mut app) = chrome_app() else {
        return;
    };
    let click = |app: &mut App, rect: max_render::buffer::PixelRect| {
        max_app::ui::handle_session_input(
            app,
            max_app::ui::InputEvent::MouseClick(
                rect.ulx + rect.width / 2,
                rect.uly + rect.height / 2,
            ),
            false,
        );
    };

    click(&mut app, max_render::layout::SURVEY_BUTTON);
    assert!(app.hud().panels.survey, "Survey — локальный флаг HudState");
    click(&mut app, max_render::layout::SURVEY_BUTTON);
    assert!(!app.hud().panels.survey);

    assert_eq!(app.viewport().zoom(), 64);
    click(&mut app, max_render::layout::ZOOM_MINUS);
    assert_eq!(app.viewport().zoom(), 56, "кнопка − уменьшает зум");
    click(&mut app, max_render::layout::ZOOM_PLUS);
    assert_eq!(app.viewport().zoom(), 64, "кнопка + возвращает 1:1");
}

// ---------------------------------------------------------------------------
// 6. TC-1112: повторный запуск/выход (`App::new` дважды) — без залипаний
// ---------------------------------------------------------------------------

/// TC-1112: приложение создаётся, играет, пишет сейв, выходит — и всё это
/// дважды подряд в одном процессе. Проверяет, что нет залипшего состояния,
/// блокировок файлов сейва и «второй `App` видит первый».
///
/// Это ровно сценарий «игрок закрыл игру и запустил снова» без перезапуска
/// процесса: `App::new` не трогает глобальные ресурсы, а `leave_local_game`
/// освобождает партию. Файл сейва намеренно один и тот же — на Windows
/// открытый дескриптор дал бы ошибку записи во втором раунде.
#[test]
fn app_can_be_created_twice_and_start_exit_twice() {
    let config = || AppConfig {
        max_res: repo_path("max-original/MAX.RES"),
        color_pal: repo_path("max-original/COLOR.PAL"),
        patches_res: repo_path("assets/PATCHES.RES"),
        ..Default::default()
    };

    if !config().max_res.is_file() {
        return;
    }

    let dir = std::env::temp_dir().join(format!(
        "max_app_restart_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let save = dir.join("restart.mxs");
    let mut digests = Vec::new();

    for _ in 0..2 {
        let mut app = App::new(config());
        app.init_assets().expect("ассеты игры");

        let setup = max_app::LocalSetup::default();
        app.start_local_game(&setup).expect("локальная партия");

        assert!(app.local_active(), "партия поднялась");
        assert_eq!(app.state().turn_counter, 1, "новый App — новый ход");
        assert_eq!(app.ticks(), 0, "счётчик тиков нового App сброшен");

        app.local_update(0.0);
        app.write_local_save(&save).expect("сейв записан");
        assert!(save.is_file(), "{}", save.display());
        digests.push(app.digest());

        app.leave_local_game();
        assert!(!app.local_active(), "выход освобождает партию");
        assert!(app.local().is_none());
        assert!(app.build_menu().is_none());
    }

    // Оба раунда начинаются с одного и того же состояния (seed по умолчанию):
    // второй App не «помнит» первый.
    assert_eq!(digests[0].state_digest, digests[1].state_digest);
    assert_eq!(digests[0].snapshot_len, digests[1].snapshot_len);

    let bytes = std::fs::read(&save).expect("сейв читается после выхода");
    assert!(!bytes.is_empty());

    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// 7. TC-1111: не-ASCII в отрисовке — fallback-глиф '?' (без паники)
// ---------------------------------------------------------------------------

/// Кадр HUD локальной партии с заданным именем игрока (имя видно в журнале).
///
/// Рисуем именно HUD (`render_hud`), а не `render_frame`: журнал сообщений и
/// строка «Local game: <имя> vs …» — часть HUD-слоя.
fn frame_for_player_name(name: &str) -> Option<Vec<u8>> {
    let config = AppConfig {
        max_res: repo_path("max-original/MAX.RES"),
        color_pal: repo_path("max-original/COLOR.PAL"),
        patches_res: repo_path("assets/PATCHES.RES"),
        ..Default::default()
    };

    if !config.max_res.is_file() {
        return None;
    }

    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    let mut setup = max_app::LocalSetup::default();
    setup.player_name = name.to_string();
    app.start_local_game(&setup).expect("локальная партия");

    let mut buffer = max_render::buffer::RgbaBuffer::new(640, 480);
    max_app::render_hud(
        &mut buffer,
        &max_app::HudView {
            state: app.state(),
            hud: app.hud(),
            viewport: app.viewport(),
            team: app.player_team(),
            status: "Local game",
        },
    );

    Some(buffer.pixels().to_vec())
}

#[test]
fn non_ascii_player_name_renders_with_the_fallback_glyph() {
    let Some(question) = frame_for_player_name("?") else {
        return;
    };
    let ascii = frame_for_player_name("A").expect("кадр с ASCII-именем");
    let cyrillic = frame_for_player_name("Ж").expect("кадр с не-ASCII именем");

    // Имя действительно попадает в кадр (иначе сравнение ниже ничего не значит).
    assert_ne!(
        question, ascii,
        "имя игрока должно рисоваться в HUD (журнал локальной партии)"
    );
    // Не-ASCII символ заменяется тем же '?', поэтому кадры совпадают побитово.
    assert_eq!(
        question, cyrillic,
        "не-ASCII имя должно рисоваться fallback-глифом '?' без паники"
    );
}

