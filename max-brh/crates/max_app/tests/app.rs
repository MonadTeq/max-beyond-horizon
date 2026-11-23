//! `max_app` integration tests: demo run, local match, commands.
//!
//! The fixture `tests/data/save1_v70.cam` — a copy of
//! `max-brh/crates/max_core/tests/data/save1_v70.cam` (campaign 1, 312 units).
//!
//! The tests require no network and no SDL: demo mode and `apply` work on pure
//! `max_core`, a new frame — on the pure parts of `max_render`. The only
//! "live" test (`assets_integration_smoke`) skips itself if there is no game data
//! (`max-original/MAX.RES`) next to the repository.

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

/// Reference `--demo tests/data/save1_v70.cam --turns 5`, obtained by a run.
///
/// Fixed in the test so that any change of the simulation/save loading
/// (a parity regression) fails explicitly instead of "quietly" changing the digest.
const SAVE1_FIVE_TICK_STATE_DIGEST: u16 = 0xDBD0;
const SAVE1_FIVE_TICK_STATE_CHECKSUM: u16 = 0x408A;
const SAVE1_FIVE_TICK_REPLAY_CRC: u16 = 0x5561;
const SAVE1_FIVE_TICK_UNITS: usize = 312;
const SAVE1_FIVE_TICK_TURN: i32 = 1;
const SAVE1_FIVE_TICK_SNAPSHOT_LEN: usize = 788_740;

fn fixture_save_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/save1_v70.cam")
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

// ---------------------------------------------------------------------------
// 1. Demo on SAVE1.CAM: load → 5 ticks → the reference digest
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

    // The digest is taken at any moment and does not depend on previous calls.
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

    // The full snapshot must match too (a parity test, not only the digest).
    let mut left = App::new(AppConfig::default());
    left.load_save_bytes(SAVE1).expect("загрузка SAVE1.CAM");
    let mut right = App::new(AppConfig::default());
    right.load_save_bytes(SAVE1).expect("загрузка SAVE1.CAM");

    for _ in 0..5 {
        left.tick();
        right.tick();
    }

    assert_eq!(
        left.state_ref().snapshot_bytes(),
        right.state_ref().snapshot_bytes()
    );
    assert_eq!(left.digest(), right.digest());
}

// ---------------------------------------------------------------------------
// 2. new_game by the P2 vector parameters `p00_l111_d0_s0`
// ---------------------------------------------------------------------------

/// The first field of the `case p00_l111_d0_s0` record from `tools/map_vectors/vectors.txt`.
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
    // The P2 vector input: the same surface the vector generator synthesizes
    // (`build_world_input`: tile_ids + pass_table for planet 0).
    let pass_table = hex_bytes(&p2_reference_field("pass_table"));
    let tile_ids = hex_u16_le(&p2_reference_field("tile_ids"));
    let expected_surface = hex_bytes(&p2_reference_field("surface"));
    let surface = derive_surface_map(&tile_ids, &pass_table);

    assert_eq!(
        surface.to_bytes().as_slice(),
        expected_surface.as_slice(),
        "вход поверхности разошёлся с P2-вектором"
    );

    // The resource phase goes before the alien phase and does not depend on the debris
    // cost table, so the resource map must match the vector bit for bit.
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
    // M3.2: `GameManager_InitGame` lands the starting armies of both teams
    // (mine, generator, two slabs and the SC_L0001 loadout), otherwise the victory/defeat
    // conditions consider the team eliminated on the very first turn.
    let per_team = 4 + max_core::DEFAULT_MISSION_LOADOUT.len();
    assert_eq!(report.start_units, 2 * per_team);
    assert_eq!(
        report.units, report.start_units,
        "derelicts=0 не должен создавать alien-юнитов, только стартовые армии"
    );
    // The P2 vector `p00_l111_d0_s0` has debris level 0 — no placements
    // (the vector itself has 0 `placement` rows too).
    assert_eq!(
        report.aliens, 0,
        "derelicts=0 не должен разворачивать обломки"
    );

    // The same input in another App — the same report and the same digest.
    let mut replay = App::new(AppConfig::default().with_screen(320, 240));
    let replay_report = replay.new_game_on_surface(&config, &surface);
    assert_eq!(report, replay_report);
    assert_eq!(app.digest(), replay.digest());

    // render_frame: the buffer size and the absence of panics (map + markers/sprites).
    let (frame, stats) = app.render_frame_with_stats();
    assert_eq!(frame.width(), 320);
    assert_eq!(frame.height(), 240);
    assert_eq!(frame.pixels().len(), 320 * 240 * 4);
    assert_eq!(frame.stride(), 320 * 4);
    assert!(stats.map_tiles > 0);
    // The camera is centered on the player's start base, so only part of the units
    // gets into the frame (the rest are outside the viewport).
    assert!(
        stats.units() > 0 && stats.units() <= report.units,
        "на кадре должны быть юниты: {stats:?} при {} в состоянии",
        report.units
    );
    assert!(frame.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));

    // M3.2: the starting armies do not let the match end on the first turn —
    // `menu_check_end_game_conditions` no longer sees the teams as "eliminated".
    for turn in 0..3 {
        app.universe.set_game_state(GameStateType::EndTurn);
        assert!(
            !app.universe.finish_turn(),
            "партия не должна завершаться на ходу {turn}"
        );
    }
    assert_eq!(
        app.digest().units,
        report.units,
        "ходы не элиминируют команду"
    );

    // Debris level 2 gives a non-empty setup placement, and all placements
    // are deployed into GameState on behalf of ALIEN (C++ `DeployUnit`).
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

    // Units — in the lists, the map hash and with a correct team; buildings occupy
    // all four 2×2 cells (`Hash_MapHash.Add`).
    {
        let state = app_aliens.state_ref();
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

    // The tick does not panic, the digest is stable between identical runs.
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
    // Only part of the map gets into the 320×240 camera, so no more units are drawn
    // than there are in the state (debris is usually outside the start view).
    assert!(aliens_stats.units() <= aliens_report.units);
}

// ---------------------------------------------------------------------------
// 3. Move command of max_core through App (packet 38)
// ---------------------------------------------------------------------------

#[test]
fn visual_units_smoothly_catch_up_and_snap_without_dt() {
    let mut app = App::new(AppConfig::default());
    app.load_save_bytes(SAVE1).expect("загрузка SAVE1.CAM");

    let id = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref().unit(*id).is_some_and(|unit| {
                unit.team == TeamId::RED && unit.unit_list == UnitListKind::MobileLandSea
            })
        })
        .expect("мобильный юнит RED");

    let from = app.state_ref().unit(id).expect("юнит").grid;
    let to = Point::new((from.x + 2).min(MAP_WIDTH - 2), from.y);
    let command = command_move_linear(id, from, to, UnitOrder::Move, UnitOrderState::Init);
    app.universe.apply_now(command);

    // Without dt — a snapshot: the visual position equals the simulation one.
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
    let grid = app.state_ref().unit(id).expect("юнит").grid;
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
    app.universe.teams_mut()[0].team_type = max_core::team::TeamKind::Player;
    app.universe.team_units_table_mut()[0] =
        max_core::team::TeamUnits::with_clan(max_core::TeamClan::Random);
    let id = app
        .universe
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("танк");

    // L route: 2 cells east, then 2 north.
    let mut unit = app.state_ref().unit(id).expect("танк").clone();
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
    app.universe.write_unit(id, unit);
    app.update_visual_units(0.0);
    assert_eq!(app.visual_angle(id), Some(2));

    // The first tick: a step east; the next step is east too — the sprite and
    // the simulation face east.
    app.tick();
    assert_eq!(app.state_ref().unit(id).expect("танк").grid, Point::new(11, 10));
    app.update_visual_units(1.0 / 60.0);
    assert_eq!(app.visual_angle(id), Some(2));

    // The second tick: a step to (12,10); at the cell boundary the simulation has already started
    // turning north (`GroundPath::UpdateUnitAngle`: E → NE), but the sprite
    // still walks the east segment and must face east.
    app.tick();
    assert_eq!(app.state_ref().unit(id).expect("танк").grid, Point::new(12, 10));
    assert_eq!(app.state_ref().unit(id).expect("танк").angle, 1);
    app.update_visual_units(1.0 / 60.0);
    assert_eq!(
        app.visual_angle(id),
        Some(2),
        "спрайт движется на восток — кадр обязан смотреть на восток, а не NE"
    );

    // The simulation and the frame move together: while the sprite shifts along an axis, its angle equals
    // the shift direction (axial segments of this route).
    let mut previous = app.visual_position(id).expect("танк");
    for frame in 0..40 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);
        let now = app.visual_position(id).expect("танк");
        let dx = now.0 - previous.0;
        let dy = now.1 - previous.1;
        let step_x = i32::from(dx > 1e-4) - i32::from(dx < -1e-4);
        let step_y = i32::from(dy > 1e-4) - i32::from(dy < -1e-4);

        // The frame cannot cross a route corner; if it did (interpolation/transition
        // diagonal), the shift direction is ambiguous — skip.
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

    // We finish the frame without simulation ticks: the sprite reaches the last cell,
    // after which the angle is taken from the simulation again (the last direction).
    for _ in 0..300 {
        app.update_visual_units(1.0 / 60.0);
    }

    let unit = app.state_ref().unit(id).expect("танк");
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
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref().unit(*id).is_some_and(|unit| {
                unit.team == TeamId::RED && unit.unit_list == UnitListKind::MobileLandSea
            })
        })
        .expect("мобильный юнит RED");

    let from = app.state_ref().unit(id).expect("юнит").grid;
    let to = Point::new((from.x + 2).min(MAP_WIDTH - 2), from.y);
    app.universe.apply_now(command_move_linear(
        id,
        from,
        to,
        UnitOrder::Move,
        UnitOrderState::Init,
    ));

    // `dt = 0` — the starting snapshot (exactly the simulation cell).
    app.update_visual_units(0.0);
    let start = app.visual_position(id).expect("юнит жив");
    assert_eq!(start, (f32::from(from.x), f32::from(from.y)));

    // We wait for the tick on which the simulation takes the first route step: the `Init`
    // order may first request a path (`NEW_ORDER`/`ISSUING_PATH`).
    let mut ticks = 0;
    while app.state_ref().unit(id).map(|unit| unit.grid) == Some(from) && ticks < 120 {
        app.tick();
        ticks += 1;
    }
    assert_ne!(
        app.state_ref().unit(id).map(|unit| unit.grid),
        Some(from),
        "юнит не сдвинулся за {ticks} тиков"
    );

    // A frame with real `dt` walks the route: two consecutive frames
    // differ in the sprite position (visible movement, not a teleport).
    app.update_visual_units(1.0 / 60.0);
    let first = app.visual_position(id).expect("юнит жив");
    assert_ne!(first, start, "после шага кадр обязан сдвинуться: {first:?}");

    // Pixel check: two consecutive frames differ (the marker/sprite
    // moved) without a simulation tick between them.
    app.viewport_mut().center_on_grid(from.x, from.y);
    let first_frame = app.render_frame();

    app.update_visual_units(1.0 / 60.0);
    let second = app.visual_position(id).expect("юнит жив");
    let second_frame = app.render_frame();
    assert_ne!(
        second, first,
        "два кадра с dt>0 должны различаться позицией"
    );
    assert!(second.0 > first.0, "спрайт плавно идёт вправо: {second:?}");
    assert_ne!(
        first_frame.pixels(),
        second_frame.pixels(),
        "кадр карты должен измениться: спрайт юнита сместился"
    );

    // The visual state is frame-only: the CRC/snapshot do not change.
    let snapshot = app.snapshot();
    app.update_visual_units(1.0 / 60.0);
    assert_eq!(
        app.snapshot(),
        snapshot,
        "update_visual_units не должен менять симуляцию"
    );

    // We finish the route: 2 cells at 4 cells/s — ~30 frames, we take a margin.
    for _ in 0..300 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);
    }

    let target = app.state_ref().unit(id).expect("юнит жив").grid;
    assert_eq!(
        app.visual_position(id),
        Some((f32::from(target.x), f32::from(target.y))),
        "по завершении маршрута визуальная позиция совпадает с целевой клеткой"
    );
}

#[test]
fn move_command_replays_through_app() {
    fn run() -> AppDigest {
        let mut app = App::new(AppConfig::default());
        app.load_save_bytes(SAVE1).expect("загрузка SAVE1.CAM");

        let id = app
            .state_ref()
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                app.state_ref().unit(*id).is_some_and(|unit| {
                    unit.team == TeamId::RED && unit.unit_list == UnitListKind::MobileLandSea
                })
            })
            .expect("в SAVE1.CAM есть мобильный юнит красных");

        let from = app.state_ref().unit(id).expect("юнит существует").grid;
        let to = Point::new((from.x + 3).min(MAP_WIDTH - 2), from.y);

        // Exactly the same helper as in the max_core tests (`packet_38_builds_ground_path`).
        let command = command_move_linear(id, from, to, UnitOrder::Move, UnitOrderState::Init);

        assert_eq!(app.universe.apply_now(command), ApplyOutcome::Applied);
        assert!(
            app.state_ref().unit(id).expect("юнит").path.is_some(),
            "пакет 38 обязан выставить путь"
        );
        assert_eq!(app.state_ref().order_data(id).move_to, to);

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
// 4. Live smoke on real game data (if present nearby)
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

    // We center the camera on a local player unit whose sprite
    // is really loaded from MAX.RES: otherwise not a single drawn sprite may get into
    // the small window (all depends on where the first unit stands).
    let mut player_types: Vec<u16> = Vec::new();
    for id in app.state_ref().unit_ids_in_order() {
        let Some(unit) = app.state_ref().unit(id) else {
            continue;
        };
        if unit.team == app.state_ref().player_team() {
            player_types.push(unit.unit_type as u16);
        }
    }
    player_types.sort_unstable();
    player_types.dedup();

    let target = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref().unit(*id).is_some_and(|unit| {
                unit.team == app.state_ref().player_team()
                    && app
                        .assets()
                        .is_some_and(|assets| assets.sprite(unit.unit_type).is_some())
            })
        })
        .unwrap_or_else(|| {
            panic!("нет спрайта ни для одного типа красных; типы: {player_types:?}")
        });

    let grid = app.state_ref().unit(target).expect("юнит").grid;
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

    // `App::new_game` (the path with real assets): the planet map from .WRL.
    // NewLuzon (index 12) — that is GREEN_1.WRL.
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
// 4. 1:1 chrome: mouse coordinates and panel clicks
// ---------------------------------------------------------------------------

/// Loads the application with game assets; `None` if there is no data nearby.
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
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .find(|unit| unit.team == app.player_team())
        .expect("стартовый юнит игрока");
    let grid = unit.grid;

    app.viewport_mut().center_on_grid(grid.x, grid.y);

    // A click at the cell center on the screen: the map starts at (180,18), and without
    // coordinate conversion the click would hit (0,0) of the map area (risk 4).
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
        app.state_ref().unit(selected).expect("юнит").grid,
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

// ---------------------------------------------------------------------------
// 4b. A2/A3: turn timer and End Turn button on a real frame
// ---------------------------------------------------------------------------

/// Ink color inside the rectangle (pixels of exactly this RGBA).
fn count_color(
    frame: &max_render::buffer::RgbaBuffer,
    rect: max_render::buffer::PixelRect,
    color: max_render::buffer::Rgba,
) -> usize {
    let mut count = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            if frame.get(x, y) == Some(color) {
                count += 1;
            }
        }
    }

    count
}

/// End Turn button rectangle from `windows[]` (`WINDOW_ENDTURN_BUTTON`).
const END_TURN_RECT: max_render::buffer::PixelRect =
    max_render::buffer::PixelRect::new(390, 2, 72, 21);

/// A2: the timer shows `GameManager_TurnTimerValue` in the
/// `WINDOW_TURN_TIMER_DISPLAY` (534,2…594,22), `FONT5`, `"%2.2i:%2.2i"`,
/// color `COLOR_RED` at `<= 20` and `0xA2` otherwise (`GameManager_DrawTurnTimer`).
#[test]
fn chrome_timer_shows_the_turn_timer_value_with_the_cpp_colors() {
    let Some(mut app) = chrome_app() else {
        return;
    };

    let palette = max_app::panels::UiPalette::from_assets(app.assets().expect("ассеты"))
        .expect("палитра FRAMEPIC");

    // The inner part of the timer window, without the bevel: only digits in it.
    let inner = max_render::buffer::PixelRect::new(536, 5, 54, 14);
    let tan_color = palette.rgba(max_app::panels::TEXT);
    let red_color = palette.rgba(max_app::panels::RED);

    // 01:41 — exactly as in the DOS original frame.
    app.hud_mut().panels.timer_seconds = 101;
    let (frame, _) = app.render_frame_with_stats();
    let tan_101 = count_color(&frame, inner, tan_color);
    let red_101 = count_color(&frame, inner, red_color);

    assert!(
        tan_101 > 50,
        "текст таймера рисуется цветом 0xA2: {tan_101}"
    );
    assert_eq!(red_101, 0, "101 с — не красный");

    // 15 s — red (`turn_time <= 20`).
    app.hud_mut().panels.timer_seconds = 15;
    let (frame, _) = app.render_frame_with_stats();
    let tan_15 = count_color(&frame, inner, tan_color);
    let red_15 = count_color(&frame, inner, red_color);

    assert!(red_15 > 50, "15 с — COLOR_RED: {red_15}");
    assert_eq!(tan_15, 0, "меньше 20 с — только красный");

    // The timer is off: C++ draws «00:00» **in red** (`DrawTurnTimer(0)`).
    app.hud_mut().panels.timer_seconds = 0;
    let (frame, _) = app.render_frame_with_stats();

    assert!(
        count_color(&frame, inner, red_color) > 50,
        "0 с (таймер выключен) — тоже красный"
    );
}

/// A3: on the computer turn the End Turn button is pressed and labeled in the active
/// team color; on the human turn — released with the usual label
/// (`GameManager_UpdateGui` → `MenuClickEndTurnButton`).
#[test]
fn chrome_end_turn_is_pressed_with_the_team_color_on_an_ai_turn() {
    let Some(mut app) = chrome_app() else {
        return;
    };

    let palette = max_app::panels::UiPalette::from_assets(app.assets().expect("ассеты"))
        .expect("палитра FRAMEPIC");
    let tan = palette.rgba(max_app::panels::END_TURN_TEXT);

    // My turn (RED, human): the button is released, the usual label.
    app.universe.set_active_turn_team(max_core::TeamId::RED);
    app.universe.teams_mut()[0].team_type = max_core::team::TeamKind::Player;
    let (human, _) = app.render_frame_with_stats();

    assert!(
        count_color(&human, END_TURN_RECT, tan) > 0,
        "на ходу человека подпись обычная (0xA0)"
    );

    // Computer GREEN turn: the button is pressed, the label — the team color (index 2).
    app.universe.set_active_turn_team(max_core::TeamId::GREEN);
    app.universe.teams_mut()[1].team_type = max_core::team::TeamKind::Computer;
    let (ai, _) = app.render_frame_with_stats();
    let green = palette.rgba(2);

    assert!(
        count_color(&ai, END_TURN_RECT, green) > 0,
        "на ходу компьютера подпись цветом команды (FontColor(team+1, …))"
    );
    assert_eq!(
        count_color(&ai, END_TURN_RECT, tan),
        0,
        "обычной подписи на нажатой кнопке нет"
    );

    // The bevel of the pressed button — the `R_ENDT_D + team` resource, not `ENDTRN_U`:
    // the frames differ inside the button.
    let mut changed = 0;

    for y in END_TURN_RECT.uly..END_TURN_RECT.lry() {
        for x in END_TURN_RECT.ulx..END_TURN_RECT.lrx() {
            if human.get(x, y) != ai.get(x, y) {
                changed += 1;
            }
        }
    }

    assert!(
        changed > 100,
        "нажатая кнопка отличается от отжатой (пикселей: {changed})"
    );

    // HOT-SEAT: the second human moves — the button is released again.
    app.universe.set_active_turn_team(max_core::TeamId::GREEN);
    app.universe.teams_mut()[1].team_type = max_core::team::TeamKind::Player;
    let (hotseat, _) = app.render_frame_with_stats();

    assert!(
        count_color(&hotseat, END_TURN_RECT, tan) > 0,
        "hot-seat: ход второго человека — кнопка отжата"
    );
    assert_eq!(
        count_color(&hotseat, END_TURN_RECT, green),
        0,
        "hot-seat: цветного текста на отжатой кнопке нет"
    );
}

/// A1: construction markers `BLDMRK1..5` are drawn around a completed site while
/// the builder waits for a player order (`DrawMap_RenderBuildMarker`).
#[test]
fn chrome_build_markers_are_drawn_around_the_finished_site() {
    use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
    use max_core::geo::Point;

    let Some(mut app) = chrome_app() else {
        return;
    };

    let site = Point::new(30, 20);
    let team = app.player_team();
    let builder = app
        .universe
        .spawn_unit(UnitType::Constructor, team, site, 0)
        .expect("constructor");
    let building = app
        .universe
        .spawn_unit(UnitType::TrainingHall, team, site, 0)
        .expect("training hall");

    app.universe.deploy_construction_site(
        builder,
        UnitType::TrainingHall,
        site,
    )
    .expect("site deployed");

    let mut builder_unit = app.state_ref().unit(builder).expect("builder").clone();
    builder_unit.grid = site;
    builder_unit.parent_unit = Some(building);
    builder_unit.orders = UnitOrder::Build;
    builder_unit.state = UnitOrderState::UnitReady;
    app.universe.write_unit(builder, builder_unit);

    let mut building_unit = app.state_ref().unit(building).expect("building").clone();
    building_unit.parent_unit = Some(builder);
    building_unit.orders = UnitOrder::Idle;
    building_unit.state = UnitOrderState::BuildingReady;
    app.universe.write_unit(building, building_unit);

    app.hud_mut().select(Some(builder));
    app.viewport_mut().center_on_grid(site.x, site.y);

    let cells =
        max_app::render::build_marker_cells(app.state_ref(), app.state_ref().unit(builder).unwrap());
    assert!(
        cells.len() >= 8,
        "вокруг площадки 2×2 обязаны быть проходимые клетки: {cells:?}"
    );

    let (frame, stats) = app.render_frame_with_stats();
    assert!(
        stats.build_markers >= cells.len(),
        "маркеры нарисованы: {} (ожидалось {})",
        stats.build_markers,
        cells.len()
    );

    // We clear the selection — the markers disappear.
    app.hud_mut().select(None);
    let (_, stats) = app.render_frame_with_stats();
    assert_eq!(stats.build_markers, 0, "без выделения маркеров нет");

    let _ = frame;
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
// 6. TC-1112: repeated launch/exit (`App::new` twice) — without sticking
// ---------------------------------------------------------------------------

/// TC-1112: the application is created, plays, writes a save, exits — and all this
/// twice in a row in one process. Checks that there is no stuck state,
/// save file locks and "the second `App` sees the first".
///
/// This is exactly the "player closed the game and started again" scenario without restarting
/// the process: `App::new` does not touch global resources, and `leave_local_game`
/// frees the match. The save file is deliberately the same — on Windows
/// an open descriptor would give a write error in the second round.
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
        assert_eq!(app.state_ref().turn_counter(), 1, "новый App — новый ход");
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

    // Both rounds start from the same state (default seed):
    // the second App does not "remember" the first.
    assert_eq!(digests[0].state_digest, digests[1].state_digest);
    assert_eq!(digests[0].snapshot_len, digests[1].snapshot_len);

    let bytes = std::fs::read(&save).expect("сейв читается после выхода");
    assert!(!bytes.is_empty());

    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// 7. TC-1111: non-ASCII in rendering — the fallback '?' glyph (without panic)
// ---------------------------------------------------------------------------

/// HUD frame of a local match with the given player name (the name is visible in the log).
///
/// We draw exactly the HUD (`render_hud`), not `render_frame`: the message log and
/// the line «Local game: <name> vs …» are part of the HUD layer.
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
            state: app.state_ref(),
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

    // The name really gets into the frame (otherwise the comparison below means nothing).
    assert_ne!(
        question, ascii,
        "имя игрока должно рисоваться в HUD (журнал локальной партии)"
    );
    // The non-ASCII character is replaced with the same '?', so the frames match bitwise.
    assert_eq!(
        question, cyrillic,
        "не-ASCII имя должно рисоваться fallback-глифом '?' без паники"
    );
}

// ---------------------------------------------------------------------------
// A4–A9: tape center, construction markers, resource knowledge, selector, radar
// ---------------------------------------------------------------------------

/// Empty land without resources and units: RED — a human with gold.
///
/// Prepares a controlled map for frame tests: without foreign units, resource
/// knowledge and palette "beacons" that could change pixels.
fn empty_land_state(
) -> max_core::universe::GameUniverse<max_core::universe::LocalDispatcher> {
    let mut state = max_core::universe::GameUniverse::default();

    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            let _ = state
                .map_mut()
                .surface
                .set(x, y, max_core::enums::Surface::LAND);
        }
    }

    state
        .team_mut(max_core::TeamId::RED)
        .expect("RED slot")
        .team_type = max_core::team::TeamKind::Player;
    let team_units = state
        .team_units_mut(max_core::TeamId::RED)
        .expect("RED economy");
    *team_units = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    team_units.set_gold(2000);
    state
}

/// `App` with game assets and empty land (without `new_game`).
fn assets_app_with_land() -> Option<App> {
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
    app.universe = empty_land_state();
    app.viewport_mut().center_on_grid(40, 40);

    Some(app)
}

/// How many pixels of cell `cell` changed between frames (in windowed mode
/// the map is shifted by `MAIN_MAP.ulx/uly`).
fn changed_pixels_in_cell(
    before: &max_render::buffer::RgbaBuffer,
    after: &max_render::buffer::RgbaBuffer,
    app: &App,
    cell: Point,
) -> usize {
    let area = max_render::layout::MAIN_MAP;
    let viewport = app.map_viewport();
    let rect = viewport.tile_rect(cell.x, cell.y);
    let mut changed = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            let (sx, sy) = (area.ulx + x, area.uly + y);

            if before.get(sx, sy) != after.get(sx, sy) {
                changed += 1;
            }
        }
    }

    changed
}

/// A4: the builder stays in the **tape center** after completion too
/// (`ORDER_BUILD`/`ORDER_STATE_UNIT_READY`) until the player orders it to leave.
///
/// C++ `SpawnNewUnit` (`unitinfo.cpp:3440-3592`) does not move the builder's `x`/`y`:
/// the pixel stays where `PositionInTape` brought it (32 px to the 2×2 center), and
/// the shift is removed only in `TAPE_POSITIONING_DEINIT`/`LEAVE`. The port earlier
/// zeroed the shift at `UNIT_READY` and "pushed" the sprite into the site corner.
#[test]
fn the_builder_stays_in_the_tape_centre_after_completion() {
    use max_core::enums::{UnitOrder, UnitOrderState, UnitType};

    let mut app = App::new(AppConfig::default());
    app.universe = empty_land_state();

    let site = Point::new(40, 40);
    let team = app.player_team();
    let building = app
        .universe
        .spawn_unit(UnitType::TrainingHall, team, site, 0)
        .expect("training hall");
    let builder = app
        .universe
        .spawn_unit(UnitType::Constructor, team, Point::new(41, 41), 0)
        .expect("constructor");

    let tape = app
        .universe
        .spawn_unit(UnitType::LargeTape, team, site, 0)
        .expect("large tape");
    {
        // Only the tape (without cones/slab): the frame must have exactly one
        // marker — the builder, otherwise the centroid would mix with the site decals.
        let mut t = app.state_ref().unit(tape).expect("tape").clone();
        t.parent_unit = Some(builder);
        t.orders = UnitOrder::Await;
        t.state = UnitOrderState::ExecutingOrder;
        app.universe.write_unit(tape, t);
    }

    {
        let mut unit = app.state_ref().unit(builder).expect("builder").clone();
        unit.grid = Point::new(41, 41);
        unit.parent_unit = Some(building);
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
        app.universe.write_unit(builder, unit);
    }
    {
        let mut unit = app.state_ref().unit(building).expect("building").clone();
        unit.parent_unit = Some(builder);
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::BuildingReady;
        app.universe.write_unit(building, unit);
    }

    app.viewport_mut().center_on_grid(site.x, site.y);

    // Without assets the builder is drawn with a 6×6 team marker at its anchor.
    let (frame, stats) = app.render_frame_with_stats();
    assert_eq!(stats.units_marker, 2, "строитель и лента — маркеры");

    let colour = max_app::render::MARKER_COLORS[0];
    let pixels_near = |cx: i32, cy: i32| {
        let mut count = 0;
        for y in (cy - 4)..=(cy + 4) {
            for x in (cx - 4)..=(cx + 4) {
                if frame.get(x, y) == Some(colour) {
                    count += 1;
                }
            }
        }
        count
    };

    // The center of the 2×2 tape — `grid*64 + 64` (C++ `OffsetDrawZones` ×4 of 8 px).
    let (ex, ey) = app
        .viewport()
        .world_to_screen(site.x as i32 * 64 + 64, site.y as i32 * 64 + 64);
    // The tape corner where the builder would stand **without** the shift (the old behavior):
    // the center of its cell (41,41) — `grid*64 + 32`.
    let (corner_x, corner_y) = app.viewport().world_to_screen(41 * 64 + 32, 41 * 64 + 32);

    assert!(
        pixels_near(ex, ey) > 0,
        "маркер строителя в центре ленты ({ex},{ey})"
    );
    assert_eq!(
        pixels_near(corner_x, corner_y),
        0,
        "строитель не «вытолкнут» в угол площадки ({corner_x},{corner_y})"
    );

    // The player's order to leave — the only thing that removes the centering: after
    // `TAPE_POSITIONING_DEINIT`/`LEAVE` the shift includes the exit steps.
    let mut unit = app.state_ref().unit(builder).expect("builder").clone();
    unit.orders = UnitOrder::AwaitTapePositioning;
    unit.state = UnitOrderState::TapePositioningLeave;
    unit.moved = 2;

    let offset = max_app::buildsite::tape_center_offset(app.state_ref(), &unit);
    assert_ne!(
        offset,
        (-32, -32),
        "на съезде сдвиг меняется (8 px × moved), а не остаётся центром"
    );
}

/// A5: construction marker cells — exactly the `DrawMap_RedrawUnit` ring
/// (`drawmap.cpp:304-337`): 12 cells around a 2×2 site, **not** its cells.
#[test]
fn build_marker_cells_are_the_cpp_ring_around_a_two_by_two_site() {
    use max_core::enums::{UnitOrder, UnitOrderState, UnitType};

    let Some(mut app) = chrome_app() else {
        return;
    };

    let site = Point::new(30, 20);
    let team = app.player_team();
    let builder = app
        .universe
        .spawn_unit(UnitType::Constructor, team, site, 0)
        .expect("constructor");
    let building = app
        .universe
        .spawn_unit(UnitType::TrainingHall, team, site, 0)
        .expect("training hall");
    app.universe.deploy_construction_site(
        builder,
        UnitType::TrainingHall,
        site,
    )
    .expect("site");

    let mut builder_unit = app.state_ref().unit(builder).expect("builder").clone();
    builder_unit.grid = site;
    builder_unit.parent_unit = Some(building);
    builder_unit.orders = UnitOrder::Build;
    builder_unit.state = UnitOrderState::UnitReady;
    app.universe.write_unit(builder, builder_unit);

    let cells =
        max_app::render::build_marker_cells(app.state_ref(), app.state_ref().unit(builder).unwrap());

    // C++: start `(parent.x - 1, parent.y + 1)`, for a building `+1` in y, then
    // N/E/S/W for three steps cumulatively; the `Access_IsAccessible` filter.
    let expected: Vec<Point> = [
        (29, 21),
        (29, 20),
        (29, 19),
        (30, 19),
        (31, 19),
        (32, 19),
        (32, 20),
        (32, 21),
        (32, 22),
        (31, 22),
        (30, 22),
        (29, 22),
    ]
    .into_iter()
    .map(|(x, y)| Point::new(x, y))
    .collect();

    assert!(!cells.is_empty(), "маркеры обязаны найтись");
    for cell in &cells {
        assert!(
            expected.contains(cell),
            "клетка {cell:?} не из кольца C++: {cells:?}"
        );
    }
    for site_cell in [
        Point::new(30, 20),
        Point::new(31, 20),
        Point::new(30, 21),
        Point::new(31, 21),
    ] {
        assert!(
            !cells.contains(&site_cell),
            "маркер не ставится на клетку самой площадки {site_cell:?}"
        );
    }

    // Frame: each marker — a 62×62 picture at the center of its cell
    // (`DrawMap_Callback2`: `ulx * 64 + 32`), so the number of markers equals
    // the number of ring cells.
    app.hud_mut().select(Some(builder));
    app.viewport_mut().center_on_grid(site.x, site.y);
    let (_, stats) = app.render_frame_with_stats();
    assert_eq!(
        stats.build_markers,
        cells.len(),
        "нарисован маркер на каждой клетке кольца"
    );
}

/// A6/A7: resource icons — only on surveyed cells, only with
/// SURVEY/a selected scout and **over** buildings.
#[test]
fn resource_icons_are_survey_knowledge_only_and_above_buildings() {
    use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
    use max_core::map::{ResourceKind, ResourceMapEntry};

    let Some(mut app) = assets_app_with_land() else {
        return;
    };

    let known_cell = Point::new(40, 40);
    let unknown_cell = Point::new(42, 40);

    // RED surveyed gold in `known_cell`, the second cell is not surveyed.
    app.universe.map_mut().resources.set(
        known_cell.x,
        known_cell.y,
        ResourceMapEntry::new(9, Some(ResourceKind::Gold), 0b1000),
    );
    app.universe.map_mut().resources.set(
        unknown_cell.x,
        unknown_cell.y,
        ResourceMapEntry::new(9, Some(ResourceKind::Gold), 0),
    );

    let building = app
        .universe
        .spawn_unit(UnitType::PowerGenerator, TeamId::RED, known_cell, 0)
        .expect("power generator");
    {
        let mut unit = app.state_ref().unit(building).expect("generator").clone();
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        app.universe.write_unit(building, unit);
    }

    let surveyor = app
        .universe
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(30, 30), 0)
        .expect("surveyor");

    app.viewport_mut().center_on_grid(40, 40);

    // 1. Neither SURVEY nor a scout is selected — there are no icons at all.
    app.hud_mut().select(None);
    let (base, stats) = app.render_frame_with_stats();
    assert!(!app.survey_overlay_active(), "оверлей SURVEY выключен");
    assert_eq!(stats.resource_icon_cells, 0, "без оверлея иконок нет");

    // 2. A scout is selected (the order is not IDLE) → the icon only on the surveyed one.
    app.hud_mut().select(Some(surveyor));
    let (surveyor_frame, stats) = app.render_frame_with_stats();
    assert!(app.survey_overlay_active(), "разведчик включает оверлей");
    assert_eq!(
        stats.resource_icon_cells, 1,
        "иконка только на клетке с битом RED"
    );
    assert!(
        changed_pixels_in_cell(&base, &surveyor_frame, &app, known_cell) > 0,
        "иконка нарисована поверх здания (пиксели клетки изменились)"
    );
    assert_eq!(
        changed_pixels_in_cell(&base, &surveyor_frame, &app, unknown_cell),
        0,
        "неразведанная клетка остаётся без иконки"
    );

    // 3. The SURVEY button — the same knowledge mask, not "the whole world".
    app.hud_mut().select(None);
    app.hud_mut().panels.survey = true;
    let (button_frame, stats) = app.render_frame_with_stats();
    assert_eq!(
        stats.resource_icon_cells, 1,
        "SURVEY показывает только разведанные клетки"
    );
    assert_eq!(
        changed_pixels_in_cell(&surveyor_frame, &button_frame, &app, known_cell),
        0,
        "кадр кнопки SURVEY совпадает с кадром выбранного разведчика"
    );
}

/// A8: `GameManager_DrawUnitSelector` (`game_manager.cpp:744`) — three nested
/// corner frames around the selected unit with the blinking `GameManager_MarkerColor`.
#[test]
fn the_selected_unit_gets_the_cpp_corner_selector_and_it_blinks() {
    use max_core::enums::UnitType;

    let mut app = App::new(AppConfig::default());
    app.universe = empty_land_state();

    let cell = Point::new(40, 40);
    let tank = app
        .universe
        .spawn_unit(UnitType::Tank, TeamId::RED, cell, 0)
        .expect("tank");
    app.viewport_mut().center_on_grid(cell.x, cell.y);

    // Without a selection there is no frame.
    let (base, stats) = app.render_frame_with_stats();
    assert_eq!(stats.selector_units, 0, "без выделения селектора нет");

    app.hud_mut().select(Some(tank));
    let (selected, stats) = app.render_frame_with_stats();
    assert_eq!(stats.selector_units, 1, "у выделенного юнита есть селектор");

    // `DrawUnitSelector` geometry at 1:1 and `double_marker = true`:
    // `size1 = 16`, `size2 = 64*64/64 - 2 = 62`, half-size 32.
    let (ax, ay) = app
        .viewport()
        .world_to_screen(cell.x as i32 * 64 + 32, cell.y as i32 * 64 + 32);
    let (ox, oy) = (ax - 32 + 1, ay - 32 + 1);
    let white = max_render::buffer::Rgba::rgb(255, 255, 255);
    let black = max_render::buffer::Rgba::rgb(0, 0, 0);

    assert_eq!(
        selected.get(ox, oy),
        Some(white),
        "внешняя рамка — цвет GameManager_MarkerColor (0xFF)"
    );
    assert_eq!(
        selected.get(ox + 1, oy + 1),
        Some(black),
        "средняя рамка — цвет ^ 0xFF (0x00)"
    );
    assert_ne!(
        base.get(ox, oy),
        Some(white),
        "до выделения в этом пикселе рамки не было"
    );

    // Blinking: `GameManager_MarkerColor ^= 0xFF` every 210 ms. The frame clock
    // clamps one step to 0.1 s (a real frame ~16 ms), so we take three.
    for _ in 0..3 {
        app.update_visual_units(0.07);
    }
    assert_eq!(app.marker_color_index(), 0x00, "цвет переключился");
    let (blinked, stats) = app.render_frame_with_stats();
    assert_eq!(stats.selector_units, 1);
    assert_eq!(
        blinked.get(ox, oy),
        Some(black),
        "после 210 мс внешняя рамка чёрная"
    );
    assert_eq!(
        blinked.get(ox + 1, oy + 1),
        Some(white),
        "средняя рамка стала белой (цвет ^ 0xFF)"
    );

    // `dt = 0` (QA/headless) — the white frame again, the frames are deterministic.
    app.update_visual_units(0.0);
    assert_eq!(app.marker_color_index(), 0xFF);

    // Clearing the selection removes the frame.
    app.hud_mut().select(None);
    let (_, stats) = app.render_frame_with_stats();
    assert_eq!(stats.selector_units, 0);
}

/// A9: a built `RADAR` returns to `ORDER_SENTRY` and its frames are advanced again
/// by `ProcessOrderSentry` (16 `D_RADAR` frames).
///
/// Earlier `SpawnNewUnit` lost `prior_orders`: the radar remained in `ORDER_AWAIT`,
/// and the `ANIMATED` cycle did not start — "radar animation broken".
#[test]
fn a_built_radar_keeps_rotating_its_sixteen_frames() {
    use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
    

    let Some(mut app) = assets_app_with_land() else {
        return;
    };

    let mut site = Point::new(40, 40);
    let engineer = app
        .universe
        .spawn_unit(UnitType::Engineer, TeamId::RED, Point::new(30, 30), 0)
        .expect("engineer");
    // `UnitsManager_DeployUnit` zeroes the new unit's storage; landing gives 40.
    app.universe
        .unit_mut(engineer)
        .expect("engineer")
        .storage = 40;

    {
        app.universe
            .enqueue_build(TeamId::RED, engineer, UnitType::Radar)
            .expect("queue");
        let report = app
            .universe
            .commit_build(TeamId::RED, engineer)
            .expect("site");

        // `commit` chooses the site anchor itself (`PathBuild`): then we look for
        // the building exactly in it, not in the engineer's original cell.
        site = report.site;

        let mut unit = app.state_ref().unit(engineer).expect("engineer").clone();
        unit.grid = report.site;
        app.universe.write_unit(engineer, unit);
    }

    let mut radar = None;
    for _ in 0..80 {
        app.universe.manage_economy(TeamId::RED);
        app.universe.progress_build_state(TeamId::RED);

        if let Some(unit) = app
            .state_ref()
            .units_at(site)
            .iter()
            .filter_map(|id| app.state_ref().unit(*id))
            .find(|unit| unit.unit_type == UnitType::Radar)
        {
            radar = Some(unit.id);
            break;
        }
    }

    let radar = radar.unwrap_or_else(|| {
        let state = app.state_ref();
        let e = state.unit(engineer).expect("engineer");
        panic!(
            "RADAR построен: engineer={:?}/{:?} build_time={} build_list={:?} site={site:?}",
            e.orders, e.state, e.build_time, e.build_list
        )
    });

    {
        let unit = app.state_ref().unit(radar).expect("radar");
        assert_eq!(
            (unit.orders, unit.state),
            (UnitOrder::Idle, UnitOrderState::BuildingReady),
            "здание ждёт ухода строителя"
        );
        assert_eq!(
            unit.prior_orders,
            UnitOrder::Sentry,
            "SetNewOrderInt сохранил ORDER_SENTRY в prior"
        );
    }

    // The builder leaves by order (A1) → `BuildingReady`/`RestoreOrders`.
    assert!(app.universe.issue_builder_exit(engineer));
    for _ in 0..60 {
        app.universe.tick();
        if app
            .state_ref()
            .unit(radar)
            .is_some_and(|unit| unit.orders == UnitOrder::Sentry)
        {
            break;
        }
    }
    assert_eq!(
        app.state_ref().unit(radar).expect("radar").orders,
        UnitOrder::Sentry,
        "RADAR снова в ORDER_SENTRY"
    );

    app.viewport_mut().center_on_grid(site.x, site.y);
    app.update_visual_units(0.0);
    assert_eq!(app.unit_animation(radar).sentry_step, 0);
    let static_frame = app.render_frame();

    // One C++ tick (~1/30 s) — the next block frame; 16 frames — a revolution.
    app.update_visual_units(max_app::SENTRY_ANIM_FRAME_SECONDS + 0.0005);
    assert_eq!(app.unit_animation(radar).sentry_step, 1);
    let rotated = app.render_frame();
    assert!(
        changed_pixels(&static_frame, &rotated) > 0,
        "ANIMATED-радар обязан менять кадр после достройки"
    );

    for _ in 0..15 {
        app.update_visual_units(max_app::SENTRY_ANIM_FRAME_SECONDS + 0.0001);
    }
    assert_eq!(
        app.unit_animation(radar).sentry_step,
        16,
        "16-кадровый цикл ProcessOrderSentry"
    );
    app.update_visual_units(0.0);
    assert_eq!(
        changed_pixels(&static_frame, &app.render_frame()),
        0,
        "dt = 0 возвращает кадр image_base — golden детерминированы"
    );
}

/// The number of differing pixels of two frames.
fn changed_pixels(
    left: &max_render::buffer::RgbaBuffer,
    right: &max_render::buffer::RgbaBuffer,
) -> usize {
    left.pixels()
        .chunks_exact(4)
        .zip(right.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}
