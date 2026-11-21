//! Кадры включённых зданий, их коннекторы и клановые варианты шахты.
//!
//! Оракул — C++ `UnitInfo::Render` (`unitinfo.cpp:3138`),
//! `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`),
//! `UnitInfo::PowerDown` (`unitinfo.cpp:5123`) и
//! `UnitInfo::RenderWithConnectors` (`unitinfo.cpp:3151`):
//!
//! * у зданий в спрайте ровно два кадра состояния — `image_base`
//!   (выключено) и `image_base + 1` (включено);
//! * `ORDER_POWER_ON` рисует `image_base + 1`, `PowerDown` — `image_base`;
//! * у шахты `image_base = (team_clan - 1) * sizeof(uint16_t)` — восемь
//!   клановых пар кадров;
//! * кадры после корпуса — **коннекторы комплекса**, а не анимация:
//!   `D_MINING.connector_image_base = 16 / count = 8`, `D_SMLBLD` (POWGEN)
//!   `base = 2 / count = 4`; рисуются по маске `UnitInfo::connectors`
//!   (DOS-съёмка приняла их за «8 частиц»/«4 дымовых клуба»,
//!   `max-original-dasm/formats/unit-animation.md` §7.1/§7.3);
//! * покадровой анимации по времени у зданий в C++ нет: `UnitInfo::Animate`
//!   только добавляет draw bounds; шахта в idle статична (DOS §7.2).
//!
//! Порт **не листает кадры зданий по времени**: у завода кадр `image_base + 1`
//! держится весь `ORDER_BUILD` (`UnitInfo::StartBuilding`), после
//! `SpawnNewUnit`/`CancelBuilding` — `image_base`; мигание включённого завода
//! дают палитровые циклы (`max_render::palette::ColorCycles`).
//!
//! Тесты пропускаются, если данных игры (`max-original/`) рядом нет.

use std::path::{Path, PathBuf};

use max_app::assets::Assets;
use max_app::render::{
    draw_unit_sprites_with_base, factory_on_frame_active, image_base_for, sentry_animation_active,
    unit_frame_index, unit_frame_index_with_animation, FrameAnimation, BUILDER_WORK_FRAME_OFFSET,
};
use max_app::AppConfig;
use max_core::access::unit_type_flags;
use max_core::enums::{TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::team::TeamId;
use max_core::unit::{UnitId, UnitInfo};
use max_render::buffer::RgbaBuffer;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn game_assets() -> Option<Assets> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("building_frames: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        ..Default::default()
    };

    Some(Assets::load(&config).expect("ассеты игры"))
}

fn unit_of(unit_type: UnitType, orders: UnitOrder, angle: u8) -> UnitInfo {
    let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
    unit.flags = unit_type_flags(unit_type);
    unit.visible_to_team = [true; 5];
    unit.orders = orders;
    unit.angle = angle;
    unit.brightness = 7;
    unit
}

fn render(assets: &Assets, unit: &UnitInfo, base: i16) -> RgbaBuffer {
    let mut frame = RgbaBuffer::new(192, 192);

    draw_unit_sprites_with_base(assets, unit, base, &mut frame, (96, 96), 64)
        .expect("спрайт здания");

    frame
}

fn painted(frame: &RgbaBuffer) -> u32 {
    frame
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[3] != 0)
        .count() as u32
}

#[test]
fn powered_generator_uses_the_on_frame_and_power_off_uses_the_off_frame() {
    let Some(assets) = game_assets() else {
        return;
    };

    let on = unit_of(UnitType::PowerGenerator, UnitOrder::PowerOn, 0);
    let off = unit_of(UnitType::PowerGenerator, UnitOrder::PowerOff, 0);
    let sprite = assets.sprite(UnitType::PowerGenerator).expect("POWGEN");
    let base = image_base_for(&on, None);

    assert_eq!(base, 0, "POWGEN: база кадров из D_POWGEN");
    assert_eq!(
        unit_frame_index(&on, base, sprite.sheet.len()),
        1,
        "C++ ProcessOrderPowerOn: DrawSpriteFrame(image_base + 1)"
    );
    assert_eq!(
        unit_frame_index(&off, base, sprite.sheet.len()),
        0,
        "C++ PowerDown: DrawSpriteFrame(image_base)"
    );

    let on_frame = render(&assets, &on, base);
    let off_frame = render(&assets, &off, base);
    let differ = on_frame
        .pixels()
        .chunks_exact(4)
        .zip(off_frame.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();

    assert!(painted(&on_frame) > 0, "включённый генератор не нарисован");
    assert!(
        differ > 0,
        "кадр включённого генератора обязан отличаться от выключенного"
    );

    // Оракул: пока приказ не меняется, кадр не меняется от времени — у здания
    // нет покадровой анимации (`UnitInfo::Animate` рисует только draw bounds).
    for _ in 0..120 {
        assert_eq!(
            unit_frame_index(&on, base, sprite.sheet.len()),
            1,
            "в C++ включённое здание не перелистывает кадры по времени"
        );
    }
}

#[test]
fn powered_mining_station_uses_its_clan_variant_and_on_frame() {
    let Some(assets) = game_assets() else {
        return;
    };

    let mut unit = unit_of(UnitType::MiningStation, UnitOrder::PowerOn, 0);
    // Клан 7 (SevenKnights): C++ `image_base = (7 - 1) * 2 = 12`.
    let base = image_base_for(&unit, Some(TeamClan::SevenKnights));

    assert_eq!(base, 12, "MININGST: image_base = (team_clan - 1) * 2");

    let sprite = assets.sprite(UnitType::MiningStation).expect("MININGST");

    assert_eq!(
        unit_frame_index(&unit, base, sprite.sheet.len()),
        base as usize + 1,
        "включённая шахта — кадр клана + 1"
    );
    assert!(
        sprite.sheet.frame(base as usize + 1).is_some(),
        "у шахты есть клановый кадр «включено»"
    );

    let frame = render(&assets, &unit, base);
    assert!(painted(&frame) > 0, "включённая шахта не нарисована");

    unit.orders = UnitOrder::PowerOff;
    assert_eq!(
        unit_frame_index(&unit, base, sprite.sheet.len()),
        base as usize,
        "выключенная шахта — базовый клановый кадр"
    );
}

#[test]
fn random_clan_falls_back_to_clan_one_frame_pair() {
    // `TeamClan::Random` (0) в max_core не разрешается в конкретный клан;
    // раньше шахта и так рисовалась кадром 0/1, это поведение сохраняем.
    let unit = unit_of(UnitType::MiningStation, UnitOrder::PowerOn, 0);

    assert_eq!(image_base_for(&unit, None), 0);
    assert_eq!(image_base_for(&unit, Some(TeamClan::Random)), 0);
    assert_eq!(image_base_for(&unit, Some(TeamClan::AxisInc)), 14);
}

/// Шахта в idle **не листает кадры** — DOS-съёмка §7.2
/// (`max-original-dasm/formats/unit-animation.md`): включённая стоит на
/// `image_base + 1`, выключенная — на `image_base`. Восемь кадров `16..23` —
/// не «частицы бурения», а коннекторы
/// (`D_MINING.connector_image_base = 16`, `connector_image_count = 8`):
/// рисуются по маске `connectors`, в плотной базе — все восемь сразу
/// (это и засняла DOS-съёмка, §7.1).
#[test]
fn mining_station_body_is_static_and_connectors_are_mask_driven() {
    use max_app::render::connector_frame_indices;

    let mut mine = unit_of(UnitType::MiningStation, UnitOrder::PowerOn, 0);

    assert!(
        !factory_on_frame_active(&mine),
        "шахта в idle статична (DOS-съёмка §7.2)"
    );

    // Кадр корпуса не зависит от времени: у здания нет кадровых часов.
    for _ in 0..16 {
        assert_eq!(
            unit_frame_index_with_animation(
                &mine,
                0,
                24,
                16,
                0,
                FrameAnimation::default(),
                true
            ),
            1,
            "корпус включённой шахты статичен"
        );
    }

    // Выключенная — кадр 0.
    mine.orders = UnitOrder::PowerOff;
    assert_eq!(
        unit_frame_index_with_animation(&mine, 0, 24, 16, 0, FrameAnimation::default(), true),
        0,
        "выключенная шахта статична"
    );
    // Клановый сдвиг сохраняется (кадры состояния клана: 12/13, 14/15).
    mine.orders = UnitOrder::PowerOn;
    assert_eq!(
        unit_frame_index_with_animation(&mine, 12, 24, 16, 0, FrameAnimation::default(), true),
        13,
        "клан 7: включённый кадр = 12 + 1"
    );

    // Коннекторы: все восемь битов — кадры 16..23 в порядке C++.
    let Some(assets) = game_assets() else {
        return;
    };
    let sprite = assets.sprite(UnitType::MiningStation).expect("MININGST");
    let full = max_core::enums::CONNECTOR_NORTH_LEFT
        | max_core::enums::CONNECTOR_NORTH_RIGHT
        | max_core::enums::CONNECTOR_EAST_TOP
        | max_core::enums::CONNECTOR_EAST_BOTTOM
        | max_core::enums::CONNECTOR_SOUTH_LEFT
        | max_core::enums::CONNECTOR_SOUTH_RIGHT
        | max_core::enums::CONNECTOR_WEST_TOP
        | max_core::enums::CONNECTOR_WEST_BOTTOM;

    assert_eq!(
        connector_frame_indices(sprite, full),
        vec![16, 20, 18, 22, 17, 21, 19, 23],
        "D_MINING: N_L, N_R, S_L, S_R, E_T, E_B, W_T, W_B"
    );
    assert!(
        connector_frame_indices(sprite, 0).is_empty(),
        "нет соседей — нет кадров"
    );
    assert_eq!(
        connector_frame_indices(sprite, max_core::enums::CONNECTOR_NORTH_LEFT),
        vec![16],
        "один северный коннектор"
    );
}

/// `POWGEN` (`D_SMLBLD`: база 2, 4 коннектора) — четыре «дымовых клуба» из
/// DOS-съёмки (§7.3), на самом деле трубы N/E/S/W; корпус статичен (0/1).
#[test]
fn power_generator_body_is_static_and_its_four_frames_are_pipes() {
    use max_app::render::connector_frame_indices;

    let Some(assets) = game_assets() else {
        return;
    };
    let sprite = assets.sprite(UnitType::PowerGenerator).expect("POWGEN");
    let base = image_base_for(
        &unit_of(UnitType::PowerGenerator, UnitOrder::PowerOn, 0),
        None,
    );

    assert_eq!(base, 0);
    assert_eq!(
        unit_frame_index(&unit_of(UnitType::PowerGenerator, UnitOrder::PowerOn, 0), base, 6),
        1,
        "включённый POWGEN — кадр 1"
    );
    assert_eq!(
        unit_frame_index(&unit_of(UnitType::PowerGenerator, UnitOrder::PowerOff, 0), base, 6),
        0,
        "выключенный POWGEN — кадр 0"
    );
    // Покадровой анимации по времени у корпуса нет — ни одного лишнего кадра.
    assert_eq!(sprite.sheet.len(), 6, "2 корпуса + 4 трубы");

    let full = max_core::enums::CONNECTOR_NORTH_LEFT
        | max_core::enums::CONNECTOR_EAST_TOP
        | max_core::enums::CONNECTOR_SOUTH_LEFT
        | max_core::enums::CONNECTOR_WEST_TOP;

    assert_eq!(
        connector_frame_indices(sprite, full),
        vec![2, 4, 3, 5],
        "кадры 2..5 = трубы N/S/E/W (3 и 5 — одна и та же картинка зеркально)"
    );
}

/// Работающий завод держит кадр `image_base + 1` (`UnitInfo::StartBuilding`,
/// `unitinfo.cpp:2789`), готовый/остановленный — `image_base`
/// (`SpawnNewUnit`/`CancelBuilding`). Кадры по времени не листаются: у
/// `TRAINHAL` 10 кадров (2 корпуса + 8 коннекторов), повторный проход
/// `unit_frame_index` при любом dt даёт тот же кадр.
#[test]
fn working_factory_holds_the_on_frame_and_an_idle_factory_is_off() {
    let Some(assets) = game_assets() else {
        return;
    };

    let mut factory = unit_of(UnitType::TrainingHall, UnitOrder::Build, 0);
    factory.state = UnitOrderState::BuildInProgress;
    factory.build_list.push(UnitType::Infantry);
    let sprite = assets.sprite(UnitType::TrainingHall).expect("TRAINHAL");
    assert_eq!(sprite.sheet.len(), 10, "2 кадра корпуса + 8 коннекторов");

    assert!(factory_on_frame_active(&factory));
    assert_eq!(
        unit_frame_index_with_animation(
            &factory,
            0,
            10,
            2,
            0,
            FrameAnimation::default(),
            true
        ),
        1,
        "C++ StartBuilding: DrawSpriteFrame(image_base + 1)"
    );

    // Никакого листания: кадр тот же при любом числе «тиков».
    for _ in 0..30 {
        assert_eq!(
            unit_frame_index_with_animation(
                &factory,
                0,
                10,
                2,
                0,
                FrameAnimation::default(),
                true
            ),
            1,
            "работающий завод статичен (мигает только палитра)"
        );
    }

    // Готовый завод (`UNIT_READY`) — C++ `DrawSpriteFrame(image_base)`.
    factory.state = UnitOrderState::UnitReady;
    assert!(!factory_on_frame_active(&factory));
    assert_eq!(
        unit_frame_index_with_animation(
            &factory,
            0,
            10,
            2,
            0,
            FrameAnimation::default(),
            true
        ),
        0,
        "SpawnNewUnit: DrawSpriteFrame(image_base)"
    );

    // Остановленный (`CancelBuilding` → HALT_BUILDING) — тоже кадр 0.
    factory.orders = UnitOrder::HaltBuilding;
    factory.state = UnitOrderState::ExecutingOrder;
    assert_eq!(
        unit_frame_index_with_animation(
            &factory,
            0,
            10,
            2,
            0,
            FrameAnimation::default(),
            true
        ),
        0,
        "CancelBuilding: DrawSpriteFrame(image_base)"
    );

    // Кадры `+1` и `0` действительно разные (два кадра состояния).
    let on = render(&assets, &{
        let mut unit = factory.clone();
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit
    }, 0);
    let off = render(&assets, &factory, 0);
    assert!(painted(&on) > 0 && painted(&off) > 0);
    assert!(
        on.pixels() != off.pixels(),
        "включённый и выключенный кадры завода различаются"
    );
}

/// `RADAR` — единственное здание с флагом `ANIMATED`: C++
/// `UnitsManager_ProcessOrderSentry` каждый тик делает `image_index + 1` и после
/// последнего кадра возвращается к `image_base`. `D_RADAR.image_count = 16`,
/// хотя в спрайте 20 кадров (4 коннектора), поэтому цикл — ровно 16.
#[test]
fn radar_sentry_animation_cycles_its_cpp_frame_block() {
    let radar = unit_of(UnitType::Radar, UnitOrder::Sentry, 0);

    assert!(sentry_animation_active(&radar));

    let frame = |step: u32| {
        unit_frame_index_with_animation(
            &radar,
            0,
            20,
            16,
            0,
            FrameAnimation {
                sentry_step: step,
                ..FrameAnimation::default()
            },
            true,
        )
    };

    assert_eq!(frame(0), 0);
    assert_eq!(frame(1), 1);
    assert_eq!(frame(15), 15);
    assert_eq!(frame(16), 0, "после последнего кадра — image_base");
    assert_eq!(frame(20), 4);

    // Не-ANIMATED здание не получает sentry-цикл.
    let gen = unit_of(UnitType::PowerGenerator, UnitOrder::Sentry, 0);
    assert!(!sentry_animation_active(&gen));
}

/// Во время стройки C++ рисует рабочую позу строителя — `image_index + 16`
/// (`unitinfo.cpp:2789` для `ENGINEER`, `4451` для `CONSTRCT`). Кадр обязан
/// возвращаться к базовому после `ORDER_AWAIT` (C++ `SpawnNewUnit`:
/// `DrawSpriteFrame(image_index - 16)`).
#[test]
fn working_builder_uses_the_plus_sixteen_frame() {
    let Some(assets) = game_assets() else {
        return;
    };

    for (unit_type, expected_frames) in [
        (UnitType::Engineer, 32usize),
        (UnitType::Constructor, 40usize),
    ] {
        let sprite = assets.sprite(unit_type).expect("спрайт строителя");
        assert_eq!(sprite.sheet.len(), expected_frames, "{unit_type:?}: кадры");

        let mut unit = unit_of(unit_type, UnitOrder::Build, 3);
        unit.state = UnitOrderState::BuildInProgress;
        let base = image_base_for(&unit, None);

        assert_eq!(
            unit_frame_index(&unit, base, sprite.sheet.len()),
            3 + BUILDER_WORK_FRAME_OFFSET,
            "{unit_type:?}: C++ DrawSpriteFrame(image_index + 16)"
        );
        let work_frame = render(&assets, &unit, base);

        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
        assert_eq!(
            unit_frame_index(&unit, base, sprite.sheet.len()),
            3,
            "{unit_type:?}: после стройки — базовый кадр"
        );
        let idle_frame = render(&assets, &unit, base);

        assert!(painted(&work_frame) > 0, "{unit_type:?}: рабочая поза не нарисована");
        assert!(
            work_frame.pixels() != idle_frame.pixels(),
            "{unit_type:?}: рабочая поза обязана отличаться от обычной"
        );
    }
}

/// Декали стройплощадки — настоящие спрайты C++ (`LRGTAPE` 127×127,
/// `LRGSLAB` 127×127, `LRGCONES` 117×117): каждая рисуется на карте, а не
/// маркером. `SMLTAPE`/`SMLCONES`/`SMLSLAB` — малые версии 1×1.
#[test]
fn construction_site_decals_have_sprites() {
    let Some(assets) = game_assets() else {
        return;
    };

    for unit_type in [
        UnitType::LargeTape,
        UnitType::LargeSlab,
        UnitType::LargeCones,
        UnitType::SmallTape,
        UnitType::SmallSlab,
        UnitType::SmallCones,
    ] {
        let sprite = assets.sprite(unit_type).expect("спрайт площадки");
        assert!(!sprite.sheet.is_empty(), "{unit_type:?}: нет кадров");

        let mut unit = unit_of(unit_type, UnitOrder::Await, 0);
        unit.state = UnitOrderState::ExecutingOrder;
        let frame = render(&assets, &unit, 0);

        assert!(painted(&frame) > 0, "{unit_type:?}: пустой кадр площадки");
    }
}
