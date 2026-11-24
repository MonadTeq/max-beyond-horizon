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
//! Tests are skipped if there is no game data (`max-original/`) nearby.

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
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("construction_animation: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"))
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
    let Some(mut app) = app_with_assets() else {
        return;
    };

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
    let Some(mut app) = app_with_assets() else {
        return;
    };

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
    let Some(mut app) = app_with_assets() else {
        return;
    };

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
