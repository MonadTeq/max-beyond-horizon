//! Анимация зданий: кадровые часы юнитов (`dt`) и палитровые циклы.
//!
//! Оракул — C++ + DOS-съёмка
//! (`max-original-dasm/formats/unit-animation.md` §7):
//!
//! * `UnitsManager_ProcessOrderSentry` (`units_manager.cpp:3029-3041`) — у
//!   юнитов с флагом `ANIMATED` (из зданий это `RADAR`) каждый тик
//!   `image_index + 1`, после последнего кадра — `image_index = image_base`;
//! * у шахт/заводов покадровой анимации в C++ **нет**: есть кадры состояния,
//!   `image_base` (выключено) и `image_base + 1` (включено), которые
//!   выставляются приказами (`UnitInfo::StartBuilding`/`SpawnNewUnit`/
//!   `PowerDown`);
//! * **шахта в idle статична** (§7.2: за ~118 с игрового времени корпус
//!   всегда кадр 0, включённая — `+1`), а мигают маяки и индикатор дрели —
//!   палитровые циклы (`World::InitColorCycles`: диапазоны 13–16 и 21–24
//!   @6 fps, §7.3);
//! * у заводов `TRAINHAL`/`LIGHTPLT`/`LANDPLT`/`AIRPLT`/`SHIPYARD` в `MAX.RES`
//!   ровно **два** кадра корпуса (`image_base`, `image_base + 1`) и 8 кадров
//!   коннекторов; кадр `+1` (`StartBuilding`) использует слоты палитровых
//!   циклов 9–12 (9 fps), 13–16 (6), 17–20 (9), 21–24 (6), 25–30 (2) — это и
//!   есть «мигание» включённого завода;
//! * у конструктора `UnitsManager_ProcessOrderBuild` (`+8`, при
//!   `image_index + 8 >= 40` — `-16`), инженер держит `+16`.
//!
//! Порт не листает кадры зданий по времени: `dt` двигает только палитровые
//! циклы и визуальные часы `ANIMATED`/строителя; `dt = 0` даёт статичный кадр
//! C++, поэтому golden-кадры не дрейфуют.
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

/// Шахта в idle **не листает корпус** (DOS-съёмка §7.2): включённая стоит на
/// кадре `image_base + 1`, выключенная — на `image_base`, и `dt` это не меняет.
/// Меняются только пиксели маяков/индикатора — их вращает палитровый цикл
/// (§7.3): 4 фазы по 166.7 мс (6 fps), полный цикл 666.7 мс.
#[test]
fn mining_station_body_is_static_and_only_the_palette_cycles_with_dt() {
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

    // `dt = 0` — статичный кадр C++ (`image_base + 1`), палитра исходная.
    let statically_powered = app.render_frame();
    assert!(
        !max_app::render::factory_on_frame_active(app.state().unit(mine).expect("mine")),
        "шахта не завод — кадр состояния по `POWER_ON`"
    );
    assert!(
        app.animated_palette().is_none(),
        "dt = 0 не вращает палитру"
    );
    // Один шаг 6 fps — маяки (слоты 13..16) получают следующий цвет.
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

    // Второй шаг — ещё одна фаза. Корпус при этом не листается.
    app.update_visual_units(1.0 / 6.0);
    assert_eq!(app.color_cycles().step_counts()[1], 2);
    let blink_2 = app.render_frame();
    assert!(
        changed_pixels(&blink_1, &blink_2) > 0,
        "вторая фаза маяков"
    );
    // Четыре шага (666.7 мс) — полный цикл, цвет маяков снова исходный.
    app.update_visual_units(2.0 / 6.0);
    assert_eq!(app.color_cycles().step_counts()[1], 4);
    let full_cycle = app.render_frame();
    assert_eq!(
        changed_pixels(&statically_powered, &full_cycle),
        0,
        "4 шага по 6 fps возвращают цвета маяков"
    );

    // Корпус шахты не листается ни на одном dt: кадровые часы здания пусты.
    for _ in 0..10 {
        app.update_visual_units(1.0 / 30.0 + 0.0005);
        assert_eq!(
            app.unit_animation(mine),
            max_app::render::FrameAnimation::default(),
            "у здания нет кадровых часов по времени"
        );
    }

    // `dt = 0` снова — в точности исходный статичный кадр.
    app.update_visual_units(0.0);
    assert!(app.animated_palette().is_none());
    assert_eq!(
        changed_pixels(&statically_powered, &app.render_frame()),
        0,
        "dt = 0 — детерминированный статичный кадр C++"
    );

    // Выключенная шахта не анимируется даже при большом dt (кадр 0).
    {
        let unit = app.state_mut().unit_mut(mine).expect("mine");
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

/// Завод с непустой очередью (`ORDER_BUILD`) **не листает кадры корпуса**:
/// C++ `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`) рисует `image_base + 1`
/// и держит его до `SpawnNewUnit`/`CancelBuilding`. Мигание включённого
/// завода — палитровые циклы `World::InitColorCycles` (`world.cpp:542`):
/// кадр `+1` использует слоты 9–12 (9 fps), 13–16 (6), 17–20 (9), 21–24 (6),
/// 25–30 (2) — это ровно те диапазоны, которые вращает `ColorCycles`.
#[test]
fn factory_with_a_queue_holds_its_on_frame_and_blinks_only_via_the_palette() {
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
        unit.state = UnitOrderState::ExecutingOrder;
        unit.build_list.push(UnitType::Infantry);
    }
    assert!(max_app::render::factory_on_frame_active(
        app.state().unit(factory).expect("factory")
    ));

    // `dt = 0` — кадр C++ `image_base + 1` (работающий завод), палитра исходная.
    let working_static = app.render_frame();

    // Один тик оригинала (1/30 с) в C++ **не** меняет кадр корпуса: минимум
    // палитрового периода — 100 мс (10 fps), поэтому и пиксели те же. До
    // правки порт листал кадры завода за 1/30 с — это и было «слишком быстро».
    app.update_visual_units(1.0 / 30.0 + 0.0005);
    assert_eq!(
        changed_pixels(&working_static, &app.render_frame()),
        0,
        "за 1/30 с корпус и палитра завода не меняются"
    );

    // Через 1/6 с палитровые циклы 6 fps делают шаг — завод мигает.
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

    // Корпус при этом остаётся кадром `image_base + 1`: кадровых часов завода
    // нет вообще, поэтому любые dt дают `FrameAnimation::default()`.
    for _ in 0..30 {
        app.update_visual_units(1.0 / 30.0);
        assert_eq!(
            app.unit_animation(factory),
            max_app::render::FrameAnimation::default()
        );
    }

    // Периоды палитровых циклов за 1 с — ровно `TIMER_FPS_TO_MS` C++
    // (`World::InitColorCycles`): 9/6/9/6/2/6/8/8/10/6/6 шагов. Часы кадра
    // двигаются шагами по 0.1 с (реальный кадр ~16 мс), поэтому считаем 10
    // шагов; допуск ±1 — округление накопленного dt.
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

    // Очередь опустела + `UNIT_READY` — C++ `SpawnNewUnit` рисует `image_base`
    // (кадр 0), завод статичен.
    {
        let unit = app.state_mut().unit_mut(factory).expect("factory");
        unit.build_list.clear();
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
    }
    assert!(!max_app::render::factory_on_frame_active(
        app.state().unit(factory).expect("factory")
    ));
    app.update_visual_units(0.0);
    let ready = app.render_frame();
    assert_ne!(
        changed_pixels(&ready, &working_static),
        0,
        "готовый завод рисует кадр `image_base`, а не работающий `+1`"
    );
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
/// кадры (и палитровые циклы), а состояние партии остаётся байт-в-байт тем же.
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
    // 12 × 0.1 с = 1.2 с → палитровые циклы шагнули (маяки мигают), тело
    // шахты при этом статично, а состояние — байт-в-байт то же.
    assert_eq!(
        app.unit_animation(mine),
        max_app::render::FrameAnimation::default(),
        "корпус шахты статичен"
    );
    assert!(
        app.color_cycles().step_counts().iter().any(|&steps| steps > 0),
        "часы палитровых циклов идут, но в снапшот не входят"
    );
    assert!(app.animated_palette().is_some(), "палитра кадра повёрнута");
}

/// Коннекторы зданий — `UnitInfo::RenderWithConnectors`: кадры
/// `connector_image_base + n` по маске `connectors`. У шахты (`D_MINING`:
/// база 16, 8 кадров) в плотной базе DOS-съёмка видела все восемь
/// одновременно (§7.1 — «8 частиц»), у `POWGEN` (`D_SMLBLD`: база 2, 4 кадра) —
/// четыре «дымовых клуба», на самом деле трубы N/E/S/W.
#[test]
fn adjacent_buildings_get_connector_frames_and_isolated_ones_do_not() {
    use max_app::render::{connector_frame_indices, connector_mask_for};

    let Some(mut app) = app_with_assets() else {
        return;
    };

    let mine = app
        .state_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(40, 40), 0)
        .expect("mining station");
    let powgen = app
        .state_mut()
        .spawn_unit(UnitType::PowerGenerator, TeamId::RED, Point::new(42, 40), 0)
        .expect("power generator");
    app.viewport_mut().center_on_grid(40, 40);

    for id in [mine, powgen] {
        let unit = app.state_mut().unit_mut(id).expect("здание");
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    // Шахта 2×2 в (40,40) и POWGEN 1×1 в (42,40): у шахты восточный коннектор
    // (`EAST_TOP`), у POWGEN — западный (`WEST_TOP`, он же у 1×1-здания).
    let mine_mask = connector_mask_for(app.state(), app.state().unit(mine).unwrap());
    assert_eq!(
        mine_mask,
        max_core::enums::CONNECTOR_EAST_TOP,
        "шахта: сосед справа"
    );
    let powgen_mask = connector_mask_for(app.state(), app.state().unit(powgen).unwrap());
    assert_eq!(
        powgen_mask,
        max_core::enums::CONNECTOR_WEST_TOP,
        "POWGEN: сосед слева"
    );

    // Одиночная шахта коннекторов не имеет.
    let isolated = max_app::render::connector_mask_for(app.state(), &{
        let mut unit = app.state().unit(mine).unwrap().clone();
        unit.grid = Point::new(80, 80);
        unit
    });
    assert_eq!(isolated, 0, "одиночное здание без соседей");

    // В окружении POWGEN рисует все 4 трубы (N/E/S/W = 2/3/4/5).
    for (x, y) in [(42, 39), (43, 40), (42, 41)] {
        let id = app
            .state_mut()
            .spawn_unit(UnitType::PowerGenerator, TeamId::RED, Point::new(x, y), 0)
            .expect("сосед");
        let unit = app.state_mut().unit_mut(id).expect("сосед");
        unit.orders = UnitOrder::Await;
    }
    let full = connector_mask_for(app.state(), app.state().unit(powgen).unwrap());
    assert_eq!(
        full,
        max_core::enums::CONNECTOR_NORTH_LEFT
            | max_core::enums::CONNECTOR_EAST_TOP
            | max_core::enums::CONNECTOR_SOUTH_LEFT
            | max_core::enums::CONNECTOR_WEST_TOP,
        "POWGEN: все четыре стороны"
    );

    // Кадры — из настоящего спрайта (`D_*`): последний шаг, после всех
    // заимствований состояния.
    let assets = app.assets().expect("ассеты");
    let mine_frames = connector_frame_indices(
        assets.sprite(UnitType::MiningStation).unwrap(),
        mine_mask,
    );
    assert_eq!(mine_frames, vec![17], "D_MINING: EAST_TOP = база 16 + 1");
    let frames = connector_frame_indices(assets.sprite(UnitType::PowerGenerator).unwrap(), full);
    assert_eq!(frames, vec![2, 4, 3, 5], "D_SMLBLD: порядок C++ N,S,E,W");
}
