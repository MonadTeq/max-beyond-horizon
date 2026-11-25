//! Construction animation: the builder frame cycle and the building "growth".
//!
//! Oracle:
//! * `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856`) — for the
//!   constructor during `ORDER_BUILD`/`ORDER_STATE_BUILD_IN_PROGRESS` the frame
//!   advances `+8`, and when `image_index + 8 >= 40` it takes `-16`, i.e.
//!   `+16 → +24 → +32 → +16`; the engineer keeps the working pose `+16`
//!   (`UnitInfo::StartBuilding`, `unitinfo.cpp:2789`);
//! * `UnitsManager_ProcessOrderAwaitScaling` (`units_manager.cpp:3150`) —
//!   `scaler_adjust` decreases to 0, and `UnitInfo::RenderSprite`
//!   (`unitinfo.cpp:3085`) draws the unit with `2^-scaler_adjust` (1/16 … 1).
//!
//! In the port the frame "time" is real `dt` (`App::update_visual_units`), not
//! the simulation tick: `dt = 0` (QA/headless) must give a deterministic frame —
//! the working pose `+16` and the full building size.
//!
//! The test requires the game data (`max-original/`); a missing data set fails the test.

use std::path::{Path, PathBuf};

use max_app::render::BUILDER_WORK_FRAME_OFFSET;
use max_app::{App, AppConfig};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
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
    let patches = repo_path("tests/fixtures/assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("construction_animation: нет данных — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        // 640×640 — the full-screen fallback without `FRAMEPIC`; the map is entirely ours.
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");
    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().team_units_table_mut()[0] = max_core::TeamUnits::with_clan(TeamClan::Random);
    app.universe_mut().team_units_table_mut()[0].set_gold(100);
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

/// Two frames with `dt > 0` differ in the builder sprite frame; `dt = 0` gives
/// the working pose `+16` — a deterministic frame.
#[test]
fn builder_sprite_cycles_through_work_frames_with_real_dt() {
    let mut app = app_with_assets().expect(concat!("test fixture is missing: ", stringify!(app_with_assets())));

    let builder = app
        .universe_mut()
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(40, 40), 0)
        .expect("constructor");
    app.viewport_mut().center_on_grid(40, 40);

    {
        let unit = app.universe_mut().unit_mut(builder).expect("constructor");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
    }

    // `dt = 0` (QA/headless): working pose +16, cycle step 0.
    let start = app.render_frame();
    assert_eq!(app.build_animation_step(builder), 0);

    // 0.15 s > BUILDER_ANIM_FRAME_SECONDS (0.1) — the next cycle frame.
    app.update_visual_units(0.15);
    assert_eq!(app.build_animation_step(builder), 1, "цикл сдвинулся на +8");
    let second = app.render_frame();
    assert!(
        changed_pixels(&start, &second) > 0,
        "второй кадр обязан отличаться позой строителя"
    );

    app.update_visual_units(0.15);
    assert_eq!(
        app.build_animation_step(builder),
        2,
        "третий кадр цикла (+8)"
    );
    let third = app.render_frame();
    assert!(
        changed_pixels(&second, &third) > 0,
        "третий кадр обязан отличаться от второго"
    );

    // dt = 0 again: the construction start pose, identical to the first frame.
    app.update_visual_units(0.0);
    assert_eq!(app.build_animation_step(builder), 0);
    let reset = app.render_frame();
    assert_eq!(
        changed_pixels(&start, &reset),
        0,
        "dt = 0 — детерминированная рабочая поза +{BUILDER_WORK_FRAME_OFFSET}"
    );
}

/// Building "growth": in the first frame with `dt > 0` the unit is smaller than full
/// (`2^-scaler_adjust`), after [`max_app::GROWTH_ANIM_SECONDS`] — full
/// size; `dt = 0` immediately gives the full one.
#[test]
fn scaling_unit_grows_to_full_size_over_real_time() {
    let mut app = app_with_assets().expect(concat!("test fixture is missing: ", stringify!(app_with_assets())));

    app.viewport_mut().center_on_grid(40, 40);
    let background = app.render_frame();

    let slab = app
        .universe_mut()
        .spawn_unit(UnitType::LargeSlab, TeamId::RED, Point::new(40, 40), 0)
        .expect("slab");
    {
        let unit = app.universe_mut().unit_mut(slab).expect("slab");
        // `PrepareConstructionSite`: `scaler_adjust = 4` + ScaleUnit(EXPAND).
        unit.scaler_adjust = 4;
        unit.orders = UnitOrder::AwaitScaling;
        unit.state = UnitOrderState::Expand;
    }

    // dt = 0: full size regardless of `scaler_adjust` (QA determinism).
    let full = app.render_frame();
    let full_pixels = changed_pixels(&background, &full);
    assert!(full_pixels > 0, "плита не нарисована");
    assert_eq!(app.unit_growth_scale(slab), 1.0);

    // dt > 0: start from 2^-4 = 1/16 — the slab is noticeably smaller than full.
    app.update_visual_units(1.0 / 60.0);
    let growing = app.render_frame();
    let growing_pixels = changed_pixels(&background, &growing);
    assert!(
        growing_pixels < full_pixels / 2,
        "первый кадр роста обязан быть меньше полного: {growing_pixels} против {full_pixels}"
    );

    // Another ~1 second of real time — full size (like the final C++ step).
    for _ in 0..15 {
        app.update_visual_units(0.1);
    }
    let grown = app.render_frame();
    assert_eq!(app.unit_growth_scale(slab), 1.0);
    assert_eq!(
        changed_pixels(&full, &grown),
        0,
        "после GROWTH_ANIM_SECONDS плита совпадает с полным размером"
    );
}

/// A ready unit without `scaler_adjust` does not "grow": scale 1 and the frame does not
/// depend on `dt`.
#[test]
fn unit_without_scaler_is_never_scaled() {
    let mut app = app_with_assets().expect(concat!("test fixture is missing: ", stringify!(app_with_assets())));

    let unit: UnitId = app
        .universe_mut()
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(40, 40), 0)
        .expect("tank");
    app.viewport_mut().center_on_grid(40, 40);

    assert_eq!(app.unit_growth_scale(unit), 1.0);
    let frame = app.render_frame();
    app.update_visual_units(0.2);
    assert_eq!(app.unit_growth_scale(unit), 1.0);
    assert_eq!(app.unit_render_zoom(unit, 64), 64);
    assert_eq!(changed_pixels(&frame, &app.render_frame()), 0);
}

/// The "growth" takes [`max_app::GROWTH_ANIM_STEPS`] C++ order ticks.
///
/// Oracle: `UnitsManager_ProcessOrderAwaitScaling` (`units_manager.cpp:3159-3173`)
/// drops `scaler_adjust` by one per tick (`4 → 3 → 2 → 1 → 0`); a tick is gated by
/// `GameManager_IsTickDue` (`game_manager.cpp:5103`) with
/// `TIMER_FPS_TO_MS(24) = 1000 / 24 → 42 ms` (`timer.h:27`). Full size therefore
/// comes at `4 × 42 ms = 168 ms` — not at the earlier 1 s (≈ 6× too slow).
#[test]
fn growth_lasts_four_order_ticks() {
    assert_eq!(max_app::GROWTH_ANIM_STEPS, 4);
    assert!(
        (max_app::GROWTH_ANIM_SECONDS - 4.0 * max_app::ORDER_TICK_SECONDS).abs() < f32::EPSILON,
        "рост = четыре тика обработки приказов"
    );
    assert!(
        (max_app::GROWTH_ANIM_SECONDS - 0.168).abs() < 1e-6,
        "C++ TIMER_FPS_TO_MS(24) = 42 мс, четыре тика = 168 мс, получено {}",
        max_app::GROWTH_ANIM_SECONDS
    );

    let mut app = app_with_assets().expect(concat!("test fixture is missing: ", stringify!(app_with_assets())));

    app.viewport_mut().center_on_grid(40, 40);
    let background = app.render_frame();
    let slab = app
        .universe_mut()
        .spawn_unit(UnitType::LargeSlab, TeamId::RED, Point::new(40, 40), 0)
        .expect("slab");
    {
        let unit = app.universe_mut().unit_mut(slab).expect("slab");
        unit.scaler_adjust = 4;
        unit.orders = UnitOrder::AwaitScaling;
        unit.state = UnitOrderState::Expand;
    }

    let full = app.render_frame();
    assert!(
        changed_pixels(&background, &full) > 0,
        "плита не нарисована"
    );
    assert_eq!(app.unit_growth_scale(slab), 1.0, "dt = 0 — полный размер");

    // 8 × 20 ms = 160 ms — still inside the fourth C++ tick.
    for _ in 0..8 {
        app.update_visual_units(0.02);
    }
    let almost = app.unit_growth_scale(slab);
    assert!(
        almost < 1.0,
        "через 160 мс (3.8 тика) рост ещё не закончен: {almost}"
    );

    // One more 20 ms step — the fourth tick (168 ms): full size.
    app.update_visual_units(0.02);
    assert_eq!(
        app.unit_growth_scale(slab),
        1.0,
        "через 4 тика обработки приказов плита полного размера"
    );
    assert_eq!(
        changed_pixels(&full, &app.render_frame()),
        0,
        "кадр полного размера совпадает с эталонным"
    );
}
