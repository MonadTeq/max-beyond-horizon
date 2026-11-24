//! Building animation: the unit frame clock (`dt`) and palette cycles.
//!
//! Oracle — C++ + DOS capture
//! (`max-original-dasm/formats/unit-animation.md` §7):
//!
//! * `UnitsManager_ProcessOrderSentry` (`units_manager.cpp:3029-3041`) — for
//!   units with the `ANIMATED` flag (of buildings that is `RADAR`) every tick
//!   `image_index + 1`, after the last frame — `image_index = image_base`;
//! * mines/factories have **no** frame-by-frame animation in C++: there are state frames,
//!   `image_base` (off) and `image_base + 1` (on), which are
//!   set by orders (`UnitInfo::StartBuilding`/`SpawnNewUnit`/
//!   `PowerDown`);
//! * **an idle mine is static** (§7.2: over ~118 s of game time the body
//!   is always frame 0, a powered-on one — `+1`), while the beacons and the drill indicator blink —
//!   palette cycles (`World::InitColorCycles`: ranges 13–16 and 21–24
//!   @6 fps, §7.3);
//! * factories `TRAINHAL`/`LIGHTPLT`/`LANDPLT`/`AIRPLT`/`SHIPYARD` in `MAX.RES`
//!   have exactly **two** body frames (`image_base`, `image_base + 1`) and 8 connector
//!   frames; frame `+1` (`StartBuilding`) uses the slots of palette
//!   cycles 9–12 (9 fps), 13–16 (6), 17–20 (9), 21–24 (6), 25–30 (2) — this is
//!   the "blinking" of a powered-on factory;
//! * for the constructor `UnitsManager_ProcessOrderBuild` (`+8`, when
//!   `image_index + 8 >= 40` — `-16`), the engineer keeps `+16`.
//!
//! The port does not advance building frames by time: `dt` moves only the palette
//! cycles and the visual clock of `ANIMATED`/the builder; `dt = 0` gives the static
//! C++ frame, so golden frames do not drift.
//!
//! Tests are skipped if there is no game data (`max-original/`) nearby.

use std::path::{Path, PathBuf};

use max_app::{App, AppConfig};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::TeamClan;
use max_render::buffer::RgbaBuffer;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn app_with_assets() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("building_animation: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"))
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");
    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().team_units_table_mut()[0] = max_core::TeamUnits::with_clan(TeamClan::Random);
    app.viewport_mut().set_zoom(64);

    Some(app)
}

fn changed_pixels(left: &RgbaBuffer, right: &RgbaBuffer) -> usize {
    left.pixels()
        .chunks_exact(4)
        .zip(right.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}

/// An idle mine **does not advance the body** (DOS capture §7.2): a powered-on one stands on
/// frame `image_base + 1`, a powered-off one — on `image_base`, and `dt` does not change this.
/// Only beacon/indicator pixels change — they are rotated by the palette cycle
/// (§7.3): 4 phases of 166.7 ms (6 fps), a full cycle of 666.7 ms.
#[test]
fn mining_station_body_is_static_and_only_the_palette_cycles_with_dt() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let mine = app
        .universe_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(40, 40), 0)
        .expect("mining station");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.universe_mut().unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    // `dt = 0` — the static C++ frame (`image_base + 1`), the palette original.
    let statically_powered = app.render_frame();
    assert!(
        !max_app::render::factory_on_frame_active(app.state_ref().unit(mine).expect("mine")),
        "шахта не завод — кадр состояния по `POWER_ON`"
    );
    assert!(
        app.animated_palette().is_none(),
        "dt = 0 не вращает палитру"
    );
    // One step of 6 fps — the beacons (slots 13..16) get the next color.
    app.update_visual_units(1.0 / 6.0 + 0.001);
    assert_eq!(app.color_cycles().step_counts()[1], 1);
    assert!(
        app.animated_palette().is_some(),
        "dt > 0 даёт снимок палитры с циклами"
    );
    let blink_1 = app.render_frame();
    assert!(
        changed_pixels(&statically_powered, &blink_1) > 0,
        "маяки шахты обязаны менять цвет от палитрового цикла"
    );

    // The second step — another phase. The body is not advanced.
    app.update_visual_units(1.0 / 6.0);
    assert_eq!(app.color_cycles().step_counts()[1], 2);
    let blink_2 = app.render_frame();
    assert!(
        changed_pixels(&blink_1, &blink_2) > 0,
        "вторая фаза маяков"
    );
    // Four steps (666.7 ms) — a full cycle, the beacon color is original again.
    app.update_visual_units(2.0 / 6.0);
    assert_eq!(app.color_cycles().step_counts()[1], 4);
    let full_cycle = app.render_frame();
    assert_eq!(
        changed_pixels(&statically_powered, &full_cycle),
        0,
        "4 шага по 6 fps возвращают цвета маяков"
    );

    // The mine body is not advanced at any dt: the building frame clock is empty.
    for _ in 0..10 {
        app.update_visual_units(1.0 / 30.0 + 0.0005);
        assert_eq!(
            app.unit_animation(mine),
            max_app::render::FrameAnimation::default(),
            "у здания нет кадровых часов по времени"
        );
    }

    // `dt = 0` again — exactly the original static frame.
    app.update_visual_units(0.0);
    assert!(app.animated_palette().is_none());
    assert_eq!(
        changed_pixels(&statically_powered, &app.render_frame()),
        0,
        "dt = 0 — детерминированный статичный кадр C++"
    );

    // A powered-off mine is not animated even at a large dt (frame 0).
    {
        let unit = app.universe_mut().unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOff;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    app.update_visual_units(0.0);
    let powered_off = app.render_frame();
    assert_ne!(
        changed_pixels(&powered_off, &statically_powered),
        0,
        "выключенная шахта рисует другой кадр состояния (0 вместо 1)"
    );

    for _ in 0..3 {
        for _ in 0..7 {
            app.update_visual_units(0.1);
        }
        assert_eq!(
            app.unit_animation(mine),
            max_app::render::FrameAnimation::default()
        );
        assert_eq!(
            changed_pixels(&powered_off, &app.render_frame()),
            0,
            "выключенная шахта статична"
        );
    }
}

/// A factory with a non-empty queue (`ORDER_BUILD`) **does not advance body frames**:
/// C++ `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`) draws `image_base + 1`
/// and holds it until `SpawnNewUnit`/`CancelBuilding`. The blinking of a powered-on
/// factory — palette cycles `World::InitColorCycles` (`world.cpp:542`):
/// frame `+1` uses slots 9–12 (9 fps), 13–16 (6), 17–20 (9), 21–24 (6),
/// 25–30 (2) — exactly those ranges that `ColorCycles` rotates.
#[test]
fn factory_with_a_queue_holds_its_on_frame_and_blinks_only_via_the_palette() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let factory = app
        .universe_mut()
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(40, 40), 0)
        .expect("training hall");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.universe_mut().unit_mut(factory).expect("factory");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.build_list.push(UnitType::Infantry);
    }
    assert!(max_app::render::factory_on_frame_active(
        app.state_ref().unit(factory).expect("factory")
    ));

    // `dt = 0` — the C++ frame `image_base + 1` (working factory), the palette original.
    let working_static = app.render_frame();

    // One original tick (1/30 s) in C++ does **not** change the body frame: the minimum
    // palette period is 100 ms (10 fps), so the pixels are the same. Before
    // the fix the port advanced factory frames per 1/30 s — that was "too fast".
    app.update_visual_units(1.0 / 30.0 + 0.0005);
    assert_eq!(
        changed_pixels(&working_static, &app.render_frame()),
        0,
        "за 1/30 с корпус и палитра завода не меняются"
    );

    // After 1/6 s the 6 fps palette cycles take a step — the factory blinks.
    app.update_visual_units(1.0 / 6.0 + 0.0005);
    assert!(
        app.color_cycles().step_counts()[1] >= 1,
        "цикл 13–16 (6 fps) шагнул"
    );
    let blink = app.render_frame();
    assert!(
        changed_pixels(&working_static, &blink) > 0,
        "включённый завод мигает палитровыми циклами"
    );

    // The body remains frame `image_base + 1`: there is no factory frame clock
    // at all, so any dt gives `FrameAnimation::default()`.
    for _ in 0..30 {
        app.update_visual_units(1.0 / 30.0);
        assert_eq!(
            app.unit_animation(factory),
            max_app::render::FrameAnimation::default()
        );
    }

    // Palette cycle periods per 1 s — exactly C++ `TIMER_FPS_TO_MS`
    // (`World::InitColorCycles`): 9/6/9/6/2/6/8/8/10/6/6 steps. The frame clock
    // moves in 0.1 s steps (a real frame ~16 ms), so we count 10
    // steps; tolerance ±1 — rounding of the accumulated dt.
    app.update_visual_units(0.0);
    for _ in 0..10 {
        app.update_visual_units(0.1);
    }
    let expected = [9, 6, 9, 6, 2, 6, 8, 8, 10, 6, 6];
    for (index, (actual, expected)) in app
        .color_cycles()
        .step_counts()
        .iter()
        .zip(expected.iter())
        .enumerate()
    {
        assert!(
            actual.abs_diff(*expected as u64) <= 1,
            "цикл {index}: за 1 с {actual} шагов, ожидалось ~{expected}"
        );
    }

    // The queue is empty + `UNIT_READY` — C++ `SpawnNewUnit` draws `image_base`
    // (frame 0), the factory is static.
    {
        let unit = app.universe_mut().unit_mut(factory).expect("factory");
        unit.build_list.clear();
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
    }
    assert!(!max_app::render::factory_on_frame_active(
        app.state_ref().unit(factory).expect("factory")
    ));
    app.update_visual_units(0.0);
    let ready = app.render_frame();
    assert_ne!(
        changed_pixels(&ready, &working_static),
        0,
        "готовый завод рисует кадр `image_base`, а не работающий `+1`"
    );
}

/// `RADAR` — the only building with the `ANIMATED` flag; C++ advances its frames in
/// `ProcessOrderSentry` (16 frames from `D_RADAR`, not 20 sprite frames).
#[test]
fn radar_sentry_animation_advances_with_real_dt() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let radar = app
        .universe_mut()
        .spawn_unit(UnitType::Radar, TeamId::RED, Point::new(40, 40), 0)
        .expect("radar");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.universe_mut().unit_mut(radar).expect("radar");
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    let static_frame = app.render_frame();
    assert_eq!(app.unit_animation(radar).sentry_step, 0);

    // One C++ tick (~1/30 s) = the next block frame.
    app.update_visual_units(1.0 / 30.0 + 0.001);
    assert_eq!(app.unit_animation(radar).sentry_step, 1);
    let next = app.render_frame();
    assert!(
        changed_pixels(&static_frame, &next) > 0,
        "ANIMATED-здание обязано менять кадр"
    );

    // A full revolution of 16 frames — `image_base` again.
    for _ in 0..15 {
        app.update_visual_units(1.0 / 30.0 + 0.0001);
    }
    assert_eq!(app.unit_animation(radar).sentry_step, 16);
    app.update_visual_units(0.0);
    assert_eq!(app.unit_animation(radar).sentry_step, 0);
    assert_eq!(
        changed_pixels(&static_frame, &app.render_frame()),
        0,
        "dt = 0 возвращает кадр C++"
    );
}

/// The visual frame clock is not part of the snapshot/CRC: dt moves only
/// frames (and palette cycles), while the match state remains byte-for-byte the same.
/// Both animated-looking cases are covered: a powered-on mine (static body, palette
/// beacons) and a working factory (`image_base + 1`, palette blinking).
#[test]
fn animation_clock_does_not_change_the_snapshot() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let mine = app
        .universe_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(40, 40), 0)
        .expect("mining station");
    {
        let unit = app.universe_mut().unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    let factory = app
        .universe_mut()
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(44, 40), 0)
        .expect("training hall");
    {
        let unit = app.universe_mut().unit_mut(factory).expect("factory");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.build_list.push(UnitType::Infantry);
    }

    let before = app.state_ref().snapshot_bytes();
    for _ in 0..12 {
        app.update_visual_units(0.1);
    }
    let after = app.state_ref().snapshot_bytes();

    assert_eq!(before, after, "визуальная анимация не меняет состояние");
    // 12 × 0.1 s = 1.2 s → the palette cycles stepped (the beacons blink), the mine
    // body is static, and the state is byte-for-byte the same.
    assert_eq!(
        app.unit_animation(mine),
        max_app::render::FrameAnimation::default(),
        "корпус шахты статичен"
    );
    assert_eq!(
        app.unit_animation(factory),
        max_app::render::FrameAnimation::default(),
        "у работающего завода нет кадровых часов — только палитра"
    );
    assert!(
        app.color_cycles().step_counts().iter().any(|&steps| steps > 0),
        "часы палитровых циклов идут, но в снапшот не входят"
    );
    assert!(app.animated_palette().is_some(), "палитра кадра повёрнута");
}

/// Building connectors — `UnitInfo::RenderWithConnectors`: frames
/// `connector_image_base + n` by the `connectors` mask. For a mine (`D_MINING`:
/// base 16, 8 frames) in a dense base the DOS capture saw all eight
/// at once (§7.1 — "8 particles"), for `POWGEN` (`D_SMLBLD`: base 2, 4 frames) —
/// four "smoke puffs", actually N/E/S/W pipes.
#[test]
fn adjacent_buildings_get_connector_frames_and_isolated_ones_do_not() {
    use max_app::render::{connector_frame_indices, connector_mask_for};

    let Some(mut app) = app_with_assets() else {
        return;
    };

    let mine = app
        .universe_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(40, 40), 0)
        .expect("mining station");
    let powgen = app
        .universe_mut()
        .spawn_unit(UnitType::PowerGenerator, TeamId::RED, Point::new(42, 40), 0)
        .expect("power generator");
    app.viewport_mut().center_on_grid(40, 40);

    for id in [mine, powgen] {
        let unit = app.universe_mut().unit_mut(id).expect("здание");
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    // Mine 2×2 at (40,40) and POWGEN 1×1 at (42,40): the mine has the east connector
    // (`EAST_TOP`), POWGEN — the west one (`WEST_TOP`, also for a 1×1 building).
    let mine_mask = connector_mask_for(app.state_ref(), app.state_ref().unit(mine).unwrap());
    assert_eq!(
        mine_mask,
        max_core::enums::CONNECTOR_EAST_TOP,
        "шахта: сосед справа"
    );
    let powgen_mask = connector_mask_for(app.state_ref(), app.state_ref().unit(powgen).unwrap());
    assert_eq!(
        powgen_mask,
        max_core::enums::CONNECTOR_WEST_TOP,
        "POWGEN: сосед слева"
    );

    // A lone mine has no connectors.
    let isolated = max_app::render::connector_mask_for(app.state_ref(), &{
        let mut unit = app.state_ref().unit(mine).unwrap().clone();
        unit.grid = Point::new(80, 80);
        unit
    });
    assert_eq!(isolated, 0, "одиночное здание без соседей");

    // Surrounded, POWGEN draws all 4 pipes (N/E/S/W = 2/3/4/5).
    for (x, y) in [(42, 39), (43, 40), (42, 41)] {
        let id = app
            .universe_mut()
            .spawn_unit(UnitType::PowerGenerator, TeamId::RED, Point::new(x, y), 0)
            .expect("сосед");
        let unit = app.universe_mut().unit_mut(id).expect("сосед");
        unit.orders = UnitOrder::Await;
    }
    let full = connector_mask_for(app.state_ref(), app.state_ref().unit(powgen).unwrap());
    assert_eq!(
        full,
        max_core::enums::CONNECTOR_NORTH_LEFT
            | max_core::enums::CONNECTOR_EAST_TOP
            | max_core::enums::CONNECTOR_SOUTH_LEFT
            | max_core::enums::CONNECTOR_WEST_TOP,
        "POWGEN: все четыре стороны"
    );

    // The frames are from a real sprite (`D_*`): the last step, after all
    // state borrowings.
    let assets = app.assets().expect("ассеты");
    let mine_frames = connector_frame_indices(
        assets.sprite(UnitType::MiningStation).unwrap(),
        mine_mask,
    );
    assert_eq!(mine_frames, vec![17], "D_MINING: EAST_TOP = база 16 + 1");
    let frames = connector_frame_indices(assets.sprite(UnitType::PowerGenerator).unwrap(), full);
    assert_eq!(frames, vec![2, 4, 3, 5], "D_SMLBLD: порядок C++ N,S,E,W");
}
