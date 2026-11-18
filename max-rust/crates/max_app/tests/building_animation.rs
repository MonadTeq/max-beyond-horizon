//! Покадровая анимация работающих зданий по реальному `dt`.
//!
//! Оракул — C++:
//! * `UnitsManager_ProcessOrderSentry` (`units_manager.cpp:3029-3041`) — у
//!   юнитов с флагом `ANIMATED` (из зданий это `RADAR`) каждый тик
//!   `image_index + 1`, после последнего кадра — `image_index = image_base`;
//! * у шахт/заводов покадровой анимации в C++ **нет**: есть только два кадра
//!   состояния, `image_base` (выключено) и `image_base + 1` (включено),
//!   которые выставляются при `POWER_ON`/`POWER_DOWN`;
//! * у конструктора `UnitsManager_ProcessOrderBuild` (`+8`, при
//!   `image_index + 8 >= 40` — `-16`), инженер держит `+16`
//!   (`UnitInfo::StartBuilding`).
//!
//! Порт оживляет включённые шахты/заводы **только на кадре**: пока здание
//! работает (`POWER_ON` у шахты, непустая очередь у завода), два кадра
//! состояния чередуются по `dt`; `dt = 0` даёт статичный кадр C++, поэтому
//! golden-кадры не дрейфуют. Симуляция и CRC не меняются.
//!
//! Тесты пропускаются, если данных игры (`max-original/`) рядом нет.

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
    app.state_mut().teams[0].team_type = TeamKind::Player;
    app.state_mut().team_units[0] = max_core::TeamUnits::with_clan(TeamClan::Random);
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

/// Включённая шахта (`ORDER_POWER_ON`) меняет кадр с реальным `dt`; выключенная
/// (`ORDER_POWER_OFF`) — нет. `dt = 0` возвращает ровно статичный кадр C++.
#[test]
fn powered_mining_station_animates_and_powered_off_mine_does_not() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let mine = app
        .state_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(40, 40), 0)
        .expect("mining station");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.state_mut().unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    // `dt = 0` — статичный кадр C++ (`image_base + 1`), как в golden.
    let statically_powered = app.render_frame();
    assert_eq!(app.unit_animation(mine).activity_step, 0);

    // > 0.5 с (WORKING_BUILDING_ANIM_FRAME_SECONDS) — второй кадр пары.
    // `update_visual_units` клампит шаг до 0.1 с, поэтому идём кадрами по 0.1.
    for _ in 0..7 {
        app.update_visual_units(0.1);
    }
    assert_eq!(app.unit_animation(mine).activity_step, 1);
    let animated = app.render_frame();
    assert!(
        changed_pixels(&statically_powered, &animated) > 0,
        "включённая шахта обязана менять кадр по dt"
    );

    // `dt = 0` снова — в точности исходный статичный кадр.
    app.update_visual_units(0.0);
    let reset = app.render_frame();
    assert_eq!(
        changed_pixels(&statically_powered, &reset),
        0,
        "dt = 0 — детерминированный статичный кадр C++"
    );

    // Выключенная шахта не анимируется даже при большом dt.
    {
        let unit = app.state_mut().unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOff;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    app.update_visual_units(0.0);
    let powered_off = app.render_frame();

    for _ in 0..3 {
        for _ in 0..7 {
            app.update_visual_units(0.1);
        }
        assert_eq!(app.unit_animation(mine).activity_step, 0);
        assert_eq!(
            changed_pixels(&powered_off, &app.render_frame()),
            0,
            "выключенная шахта статична"
        );
    }
}

/// Завод с непустой очередью (`ORDER_BUILD`, `build_list`) анимирован; тот же
/// завод без очереди — статичен.
#[test]
fn factory_with_a_production_queue_animates_and_without_it_is_static() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let factory = app
        .state_mut()
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(40, 40), 0)
        .expect("training hall");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.state_mut().unit_mut(factory).expect("factory");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
        unit.build_list.push(UnitType::Infantry);
    }

    let static_queue = app.render_frame();
    assert_eq!(app.unit_animation(factory).activity_step, 0);

    for _ in 0..7 {
        app.update_visual_units(0.1);
    }
    assert_eq!(app.unit_animation(factory).activity_step, 1);
    let animated = app.render_frame();
    assert!(
        changed_pixels(&static_queue, &animated) > 0,
        "завод с очередью обязан менять кадр по dt"
    );

    // Очередь опустела — индикатор работы выключается.
    {
        let unit = app.state_mut().unit_mut(factory).expect("factory");
        unit.build_list.clear();
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    app.update_visual_units(0.0);
    let idle = app.render_frame();

    for _ in 0..3 {
        for _ in 0..7 {
            app.update_visual_units(0.1);
        }
        assert_eq!(app.unit_animation(factory).activity_step, 0);
        assert_eq!(
            changed_pixels(&idle, &app.render_frame()),
            0,
            "завод без очереди статичен"
        );
    }
}

/// `RADAR` — единственное здание с флагом `ANIMATED`; C++ листает его кадры в
/// `ProcessOrderSentry` (16 кадров из `D_RADAR`, не 20 кадров спрайта).
#[test]
fn radar_sentry_animation_advances_with_real_dt() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let radar = app
        .state_mut()
        .spawn_unit(UnitType::Radar, TeamId::RED, Point::new(40, 40), 0)
        .expect("radar");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.state_mut().unit_mut(radar).expect("radar");
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    let static_frame = app.render_frame();
    assert_eq!(app.unit_animation(radar).sentry_step, 0);

    // Один тик C++ (~1/30 с) = следующий кадр блока.
    app.update_visual_units(1.0 / 30.0 + 0.001);
    assert_eq!(app.unit_animation(radar).sentry_step, 1);
    let next = app.render_frame();
    assert!(
        changed_pixels(&static_frame, &next) > 0,
        "ANIMATED-здание обязано менять кадр"
    );

    // Полный оборот 16 кадров — снова кадр `image_base`.
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

/// Визуальные кадровые часы не попадают в снапшот/CRC: dt двигает только
/// кадры, а состояние партии остаётся байт-в-байт тем же.
#[test]
fn animation_clock_does_not_change_the_snapshot() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let mine = app
        .state_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(40, 40), 0)
        .expect("mining station");
    {
        let unit = app.state_mut().unit_mut(mine).expect("mine");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    let before = app.state().snapshot_bytes();
    for _ in 0..12 {
        app.update_visual_units(0.1);
    }
    let after = app.state().snapshot_bytes();

    assert_eq!(before, after, "визуальная анимация не меняет состояние");
    assert_eq!(
        app.unit_animation(mine).activity_step,
        0,
        "0.6 + 0.6 = 1.2 с → чётный шаг"
    );
}
