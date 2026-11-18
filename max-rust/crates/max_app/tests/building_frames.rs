//! Кадры включённых зданий и клановые варианты шахты.
//!
//! Оракул — C++ `UnitInfo::Render` (`unitinfo.cpp:3138`),
//! `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`) и
//! `UnitInfo::PowerDown` (`unitinfo.cpp:5123`):
//!
//! * у зданий в спрайте ровно два кадра состояния — `image_base`
//!   (выключено) и `image_base + 1` (включено);
//! * `ORDER_POWER_ON` рисует `image_base + 1`, `PowerDown` — `image_base`;
//! * у шахты `image_base = (team_clan - 1) * sizeof(uint16_t)` — восемь
//!   клановых пар кадров;
//! * покадровой анимации по времени у зданий в C++ нет: `UnitInfo::Animate`
//!   только добавляет draw bounds, а `ProcessOrderPowerOn` вызывает
//!   `DrawSpriteFrame` один раз при включении.
//!
//! Тесты пропускаются, если данных игры (`max-original/`) рядом нет.

use std::path::{Path, PathBuf};

use max_app::assets::Assets;
use max_app::render::{
    draw_unit_sprites_with_base, image_base_for, unit_frame_index, BUILDER_WORK_FRAME_OFFSET,
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
