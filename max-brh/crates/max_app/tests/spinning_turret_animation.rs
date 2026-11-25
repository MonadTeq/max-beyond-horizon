//! DEFECT 1: the rotating radar of the `AWAC` and the mobile `SCANNER`.
//!
//! Oracle — `max-cpp-port/src`:
//!
//! * `UnitsManager_ProcessOrder` (`units_manager.cpp:3263-3266`) calls
//!   `UnitInfo::SpinningTurretAdvanceAnimation` for every `SPINNING_TURRET` unit
//!   on each order tick, for any order except `ORDER_DISABLE`, only with
//!   `settings["effects"]`;
//! * `UnitInfo::SpinningTurretAdvanceAnimation` (`unitinfo.cpp:3891-3898`):
//!   `turret_image_index + 1`, after `image_index_max` — `turret_image_base`;
//! * `UnitsManager_DeployUnit` (`units_manager.cpp:2446-2449`) sets that
//!   `image_index_max = turret_image_base + turret_image_count - 1`, so the dish
//!   walks exactly the `D_*` turret frames: `D_AWAC` — 30, `D_SCANNER` — 16;
//! * `UnitInfo::Render`/`RenderWithConnectors` (`unitinfo.cpp:3143/3199`) draw the
//!   turret as the separate frame `turret_image_index` with the hull-angle offset.
//!
//! The port never advanced the turret frame, so both radars stood still. The
//! animation is frame-only: the simulation keeps the C++ `turret_image_index`, the
//! presentation clock [`max_app::render::FrameAnimation::turret_step`] rotates the
//! drawn frame, so the snapshot/CRC do not change.
//!
//! Tests are skipped if there is no game data (`max-original/`) nearby.

use std::path::{Path, PathBuf};

use max_app::render::{draw_unit_sprites_with_animation, FrameAnimation};
use max_app::{App, AppConfig, SPINNING_TURRET_ANIM_FRAME_SECONDS};
use max_core::enums::{UnitOrder, UnitType};
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
    // `PATCHES.RES` lives either in the delivery `assets/` directory or in the
    // C++ port's shared one (`PATCHES_RES_FALLBACKS` in `assets.rs`).
    let patches = [
        repo_path("assets/PATCHES.RES"),
        repo_path("max-cpp-port/assets/PATCHES.RES"),
        repo_path("bin/linux-server-template/PATCHES.RES"),
    ]
    .into_iter()
    .find(|path| path.is_file());

    if !max_res.is_file() || !color_pal.is_file() {
        eprintln!("spinning_turret_animation: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let Some(patches) = patches else {
        eprintln!("spinning_turret_animation: пропуск — нет PATCHES.RES");
        return None;
    };

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(patches.parent().expect("каталог PATCHES.RES"))
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

/// Renders one unit at 1:1 with an explicit frame clock (static palette, so the
/// only differences come from the turret frame).
fn render_unit(app: &App, id: max_core::unit::UnitId, animation: FrameAnimation) -> RgbaBuffer {
    let unit = app.state_ref().unit(id).expect("юнит").clone();
    let presentation = app
        .state_ref()
        .presentation(id)
        .cloned()
        .unwrap_or_default();
    let assets = app.assets().expect("ассеты");
    let mut frame = RgbaBuffer::new(256, 256);
    let stats = draw_unit_sprites_with_animation(
        assets,
        &assets.system_palette,
        &unit,
        &presentation,
        unit.image_base,
        unit.angle,
        animation,
        true,
        unit.connectors,
        &mut frame,
        (128, 128),
        64,
    )
    .expect("спрайт юнита");

    assert!(stats.turret > 0, "радар рисуется отдельным кадром башни");

    frame
}

/// Every turn step of the `turret_image_count` cycle is a distinct picture, the
/// frame after the last one is the first again — exactly C++'s
/// `turret_image_index + 1 > image_index_max → turret_image_base`.
#[test]
fn awac_radar_turns_through_thirty_frames_and_wraps() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let awac = app
        .universe_mut()
        .spawn_unit(UnitType::Awac, TeamId::RED, Point::new(40, 40), 0)
        .expect("AWAC");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.universe_mut().unit_mut(awac).expect("AWAC");
        unit.orders = UnitOrder::Await;
    }

    let sprite = app
        .assets()
        .and_then(|assets| assets.sprite(UnitType::Awac))
        .expect("D_AWAC");
    let frames = usize::from(sprite.frame_info.expect("frame info").turret_image_count);
    assert_eq!(frames, 30, "D_AWAC: 30 кадров радара");

    let base = render_unit(&app, awac, FrameAnimation::default());
    let mut distinct = std::collections::BTreeSet::new();
    let mut first_different = None;

    for step in 0..frames as u32 {
        let picture = render_unit(
            &app,
            awac,
            FrameAnimation {
                turret_step: step,
                ..FrameAnimation::default()
            },
        );
        let hash: u64 = picture
            .pixels()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });

        if step > 0 && changed_pixels(&base, &picture) > 0 && first_different.is_none() {
            first_different = Some(step);
        }
        distinct.insert(hash);
    }

    // The original `D_AWAC` art holds each radar pose twice (30 records, 15
    // distinct pictures, cf. `z`-pairs in the archive), so the cycle is checked by
    // the period and by "the picture changes", not by 30 unique bitmaps.
    assert!(
        distinct.len() >= 8,
        "радар AWAC рисуется недостаточно разными позами: {}",
        distinct.len()
    );
    assert_eq!(
        first_different,
        Some(2),
        "радар AWAC обязан сменить позу к третьему тику (арт повторяет кадры парами)"
    );

    // One full revolution returns the first picture.
    let full_turn = render_unit(
        &app,
        awac,
        FrameAnimation {
            turret_step: frames as u32,
            ..FrameAnimation::default()
        },
    );
    assert_eq!(
        changed_pixels(&base, &full_turn),
        0,
        "после полного оборота — turret_image_base"
    );
    for step in [1u32, 7, 29] {
        let one = render_unit(
            &app,
            awac,
            FrameAnimation {
                turret_step: step,
                ..FrameAnimation::default()
            },
        );
        let next_turn = render_unit(
            &app,
            awac,
            FrameAnimation {
                turret_step: step + frames as u32,
                ..FrameAnimation::default()
            },
        );
        assert_eq!(
            changed_pixels(&one, &next_turn),
            0,
            "оборот радара AWAC периодичен с шагом {frames}"
        );
    }

    // `settings["effects"] = false` disables the frame-by-frame animation in C++
    // (`UnitsManager_ProcessOrder`, `units_manager.cpp:3264`) — the frame stays.
    let unit = app.state_ref().unit(awac).expect("AWAC").clone();
    let presentation = app
        .state_ref()
        .presentation(awac)
        .cloned()
        .unwrap_or_default();
    let assets = app.assets().expect("ассеты");
    let mut still = RgbaBuffer::new(256, 256);
    draw_unit_sprites_with_animation(
        assets,
        &assets.system_palette,
        &unit,
        &presentation,
        unit.image_base,
        unit.angle,
        FrameAnimation {
            turret_step: 7,
            ..FrameAnimation::default()
        },
        false,
        unit.connectors,
        &mut still,
        (128, 128),
        64,
    );
    assert_eq!(
        changed_pixels(&base, &still),
        0,
        "effects = false — статичный кадр C++"
    );
}

/// `SCANNER` — the mobile scanner's radar: `D_SCANNER` declares 16 turret frames.
#[test]
fn mobile_scanner_radar_turns_through_sixteen_frames() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let scanner = app
        .universe_mut()
        .spawn_unit(UnitType::Scanner, TeamId::RED, Point::new(40, 40), 0)
        .expect("SCANNER");
    app.viewport_mut().center_on_grid(40, 40);

    let sprite = app
        .assets()
        .and_then(|assets| assets.sprite(UnitType::Scanner))
        .expect("D_SCANNER");
    let frames = usize::from(sprite.frame_info.expect("frame info").turret_image_count);
    assert_eq!(frames, 16, "D_SCANNER: 16 кадров радара");

    let base = render_unit(&app, scanner, FrameAnimation::default());
    let next = render_unit(
        &app,
        scanner,
        FrameAnimation {
            turret_step: 2,
            ..FrameAnimation::default()
        },
    );
    assert!(
        changed_pixels(&base, &next) > 0,
        "радар сканера не вращается"
    );

    let full_turn = render_unit(
        &app,
        scanner,
        FrameAnimation {
            turret_step: frames as u32,
            ..FrameAnimation::default()
        },
    );
    assert_eq!(
        changed_pixels(&base, &full_turn),
        0,
        "16 кадров — полный оборот"
    );
}

/// The animation clock: one `UnitsManager_ProcessOrder` tick advances the drawn
/// frame, `ORDER_DISABLE` and `effects = false` stop it, `dt = 0` resets it, and
/// the simulation (`turret_image_index`/snapshot bytes) is untouched.
#[test]
fn turret_clock_is_frame_only_and_follows_the_order_tick() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let awac = app
        .universe_mut()
        .spawn_unit(UnitType::Awac, TeamId::RED, Point::new(40, 40), 0)
        .expect("AWAC");
    let scanner = app
        .universe_mut()
        .spawn_unit(UnitType::Scanner, TeamId::RED, Point::new(42, 40), 0)
        .expect("SCANNER");
    app.viewport_mut().center_on_grid(40, 40);

    assert_eq!(app.unit_animation(awac).turret_step, 0, "dt = 0 — статика");
    let stored = app.state_ref().unit(awac).expect("AWAC").turret_image_index;
    let before = app.state_ref().snapshot_bytes();

    app.update_visual_units(SPINNING_TURRET_ANIM_FRAME_SECONDS + 0.001);
    assert_eq!(app.unit_animation(awac).turret_step, 1);
    assert_eq!(app.unit_animation(scanner).turret_step, 1);

    for _ in 0..29 {
        app.update_visual_units(SPINNING_TURRET_ANIM_FRAME_SECONDS + 0.0001);
    }
    assert_eq!(
        app.unit_animation(awac).turret_step,
        30,
        "30 тиков — полный оборот радара AWAC"
    );

    assert_eq!(
        app.state_ref().unit(awac).expect("AWAC").turret_image_index,
        stored,
        "визуальный оборот не пишет turret_image_index"
    );
    // The palette cycles rotate with dt, but they are frame-only as well: the
    // canonical state bytes stay identical.
    assert_eq!(
        app.state_ref().snapshot_bytes(),
        before,
        "анимация радара не меняет снапшот/CRC"
    );

    // `ORDER_DISABLE` — C++ skips `SpinningTurretAdvanceAnimation`.
    {
        let unit = app.universe_mut().unit_mut(awac).expect("AWAC");
        unit.orders = UnitOrder::Disable;
    }
    app.update_visual_units(SPINNING_TURRET_ANIM_FRAME_SECONDS + 0.001);
    assert_eq!(
        app.unit_animation(awac).turret_step,
        0,
        "ORDER_DISABLE останавливает радар"
    );

    // Effects off — the clock does not run at all.
    let mut preferences = app.preferences().clone();
    preferences.effects = false;
    app.set_preferences(preferences);
    app.update_visual_units(SPINNING_TURRET_ANIM_FRAME_SECONDS + 0.001);
    assert_eq!(app.unit_animation(scanner).turret_step, 0);

    // `dt = 0` (QA/headless) — the static C++ frame.
    let mut preferences = app.preferences().clone();
    preferences.effects = true;
    app.set_preferences(preferences);
    app.update_visual_units(0.0);
    assert_eq!(app.unit_animation(awac), FrameAnimation::default());
    assert_eq!(app.unit_animation(scanner), FrameAnimation::default());
}
