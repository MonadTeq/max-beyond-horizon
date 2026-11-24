//! Frames of powered-on buildings, their connectors and the mine clan variants.
//!
//! Oracle — C++ `UnitInfo::Render` (`unitinfo.cpp:3138`),
//! `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`),
//! `UnitInfo::PowerDown` (`unitinfo.cpp:5123`) and
//! `UnitInfo::RenderWithConnectors` (`unitinfo.cpp:3151`):
//!
//! * buildings have exactly two state frames in the sprite — `image_base`
//!   (off) and `image_base + 1` (on);
//! * `ORDER_POWER_ON` draws `image_base + 1`, `PowerDown` — `image_base`;
//! * for a mine `image_base = (team_clan - 1) * sizeof(uint16_t)` — eight
//!   clan frame pairs;
//! * the frames after the body are **complex connectors**, not animation:
//!   `D_MINING.connector_image_base = 16 / count = 8`, `D_SMLBLD` (POWGEN)
//!   `base = 2 / count = 4`; they are drawn by the `UnitInfo::connectors` mask
//!   (the DOS capture took them for "8 particles"/"4 smoke puffs",
//!   `max-original-dasm/formats/unit-animation.md` §7.1/§7.3);
//! * C++ has no time-based frame animation for buildings: `UnitInfo::Animate`
//!   only adds draw bounds; an idle mine is static (DOS §7.2).
//!
//! The port does **not** advance building frames by time: for a factory the frame `image_base + 1`
//! is held for the whole `ORDER_BUILD` (`UnitInfo::StartBuilding`), after
//! `SpawnNewUnit`/`CancelBuilding` — `image_base`; the blinking of a powered-on factory
//! comes from palette cycles (`max_render::palette::ColorCycles`).
//!
//! Tests are skipped if there is no game data (`max-original/`) nearby.

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
use max_core::unit::{UnitId, UnitInfo, UnitPresentation};
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
    unit
}

fn render(assets: &Assets, unit: &UnitInfo, base: i16) -> RgbaBuffer {
    let mut frame = RgbaBuffer::new(192, 192);
    // Presentation fields (phase 5.3/B) — a separate `GameState` table, here
    // synthetic: brightness 7, as before in `UnitInfo`.
    let mut presentation = UnitPresentation::new();
    presentation.brightness = 7;

    draw_unit_sprites_with_base(assets, unit, &presentation, base, &mut frame, (96, 96), 64)
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

    // Oracle: while the order does not change, the frame does not change with time — a building
    // has no frame-by-frame animation (`UnitInfo::Animate` draws only draw bounds).
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
    // Clan 7 (SevenKnights): C++ `image_base = (7 - 1) * 2 = 12`.
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
    // `TeamClan::Random` (0) is not resolved to a specific clan in max_core;
    // earlier the mine was drawn with frame 0/1 anyway, we keep this behavior.
    let unit = unit_of(UnitType::MiningStation, UnitOrder::PowerOn, 0);

    assert_eq!(image_base_for(&unit, None), 0);
    assert_eq!(image_base_for(&unit, Some(TeamClan::Random)), 0);
    assert_eq!(image_base_for(&unit, Some(TeamClan::AxisInc)), 14);
}

/// The 16-frame `MININGST` body block is **eight clan variants × two state frames**,
/// not a drill animation: every frame has the same alpha silhouette (nothing moves),
/// each even frame carries a distinct clan emblem and each pair differs only in
/// palette indices (off/on). Frames `16..23` are complex connectors. Therefore
/// `MINING_DRILL_FRAMES_ENABLED` must stay `false`: a "drill" cycle over `1..15`
/// would repaint the mine with another clan's emblem. Oracle:
/// `UnitInfo::UnitInfo` (`unitinfo.cpp:281-283`) sets
/// `image_base = (team_clan - 1) * sizeof(uint16_t)`; DOS capture §7.2 never saw
/// a `0 → 1 → … → 15` transition.
#[test]
fn mining_station_body_frames_are_eight_clan_pairs_not_a_drill_cycle() {
    use max_app::render::MINING_DRILL_FRAMES_ENABLED;
    use max_assets::sprite::SpriteFrame;

    let Some(assets) = game_assets() else {
        return;
    };
    let sprite = assets.sprite(UnitType::MiningStation).expect("MININGST");
    assert_eq!(sprite.sheet.len(), 24, "16 body frames + 8 connectors");
    // Compile-time lock: flipping the switch to `true` must not build (see the
    // constant's documentation for why these frames are clan variants).
    const {
        assert!(
            !MINING_DRILL_FRAMES_ENABLED,
            "enabling requires a separate owner decision (oracle-true, SDL/UI C1/C2 2026-11-24)"
        )
    };

    let body: Vec<&SpriteFrame> = (0..16)
        .map(|index| sprite.sheet.frame(index).expect("body frame"))
        .collect();
    let first = body[0];
    assert_eq!((first.width, first.height), (116, 108), "mine body size");
    let silhouette: Vec<bool> = first.pixels.iter().map(|pixel| pixel.is_some()).collect();

    // Nothing moves: the 16 frames are pixel-identical in alpha (a drill would
    // change the silhouette).
    for (index, frame) in body.iter().enumerate() {
        assert_eq!(
            (frame.width, frame.height),
            (first.width, first.height),
            "frame {index}: body size"
        );
        let mask: Vec<bool> = frame.pixels.iter().map(|pixel| pixel.is_some()).collect();
        assert_eq!(
            mask, silhouette,
            "frame {index}: the silhouette must not move"
        );
    }

    // Eight different clan emblems: all even frames (`image_base`) are distinct.
    for left in (0..16).step_by(2) {
        for right in (left + 2..16).step_by(2) {
            assert_ne!(
                body[left].pixels, body[right].pixels,
                "clan frames {left} and {right} must differ (clan emblem)"
            );
        }
    }

    // Each pair is the off/on state of one clan: palette indices differ.
    for base in (0..16).step_by(2) {
        assert_ne!(
            body[base].pixels,
            body[base + 1].pixels,
            "clan {}: off/on frames differ",
            base / 2 + 1
        );
    }

    // C++ `image_base = (clan - 1) * 2`: clan 1..8 -> 0, 2, ..., 14; powered-on = +1.
    let mut pairs = std::collections::BTreeSet::new();
    for (index, clan) in TeamClan::ALL.iter().skip(1).enumerate() {
        let expected = (index as i16) * 2;
        let powered = unit_of(UnitType::MiningStation, UnitOrder::PowerOn, 0);
        let base = image_base_for(&powered, Some(*clan));
        assert_eq!(base, expected, "{clan:?}: image_base");
        assert!(
            pairs.insert(base),
            "{clan:?}: clan frame pair must be unique"
        );

        // Time never advances a mine body: any frame clock gives the clan's on-frame.
        for animation in [
            FrameAnimation::default(),
            FrameAnimation {
                builder_step: 2,
                sentry_step: 99,
            },
        ] {
            assert_eq!(
                unit_frame_index_with_animation(
                    &powered,
                    base,
                    sprite.sheet.len(),
                    16,
                    0,
                    animation,
                    true
                ),
                base as usize + 1,
                "{clan:?}: a mine never cycles the body"
            );
        }
    }
}

/// An idle mine **does not advance frames** — DOS capture §7.2
/// (`max-original-dasm/formats/unit-animation.md`): a powered-on one stands on
/// `image_base + 1`, a powered-off one — on `image_base`. The eight frames `16..23` —
/// not "drilling particles" but connectors
/// (`D_MINING.connector_image_base = 16`, `connector_image_count = 8`):
/// drawn by the `connectors` mask, in a dense base — all eight at once
/// (this is what the DOS capture recorded, §7.1).
#[test]
fn mining_station_body_is_static_and_connectors_are_mask_driven() {
    use max_app::render::connector_frame_indices;

    let mut mine = unit_of(UnitType::MiningStation, UnitOrder::PowerOn, 0);

    assert!(
        !factory_on_frame_active(&mine),
        "шахта в idle статична (DOS-съёмка §7.2)"
    );

    // The body frame does not depend on time: a building has no frame clock.
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

    // Powered off — frame 0.
    mine.orders = UnitOrder::PowerOff;
    assert_eq!(
        unit_frame_index_with_animation(&mine, 0, 24, 16, 0, FrameAnimation::default(), true),
        0,
        "выключенная шахта статична"
    );
    // The clan shift is preserved (clan state frames: 12/13, 14/15).
    mine.orders = UnitOrder::PowerOn;
    assert_eq!(
        unit_frame_index_with_animation(&mine, 12, 24, 16, 0, FrameAnimation::default(), true),
        13,
        "клан 7: включённый кадр = 12 + 1"
    );

    // Connectors: all eight bits — frames 16..23 in C++ order.
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

/// `POWGEN` (`D_SMLBLD`: base 2, 4 connectors) — four "smoke puffs" from
/// the DOS capture (§7.3), actually N/E/S/W pipes; the body is static (0/1).
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
    // The body has no frame-by-frame animation by time — not a single extra frame.
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

/// A working factory holds frame `image_base + 1` (`UnitInfo::StartBuilding`,
/// `unitinfo.cpp:2789`), a ready/stopped one — `image_base`
/// (`SpawnNewUnit`/`CancelBuilding`). Frames are not advanced by time: a
/// `TRAINHAL` has 10 frames (2 bodies + 8 connectors), a repeated
/// `unit_frame_index` call at any dt gives the same frame.
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

    // No advancing at all: the frame is the same at any number of "ticks".
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

    // A ready factory (`UNIT_READY`) — C++ `DrawSpriteFrame(image_base)`.
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

    // Stopped (`CancelBuilding` → HALT_BUILDING) — also frame 0.
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

    // The `+1` and `0` frames really differ (two state frames).
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

/// `RADAR` — the only building with the `ANIMATED` flag: C++
/// `UnitsManager_ProcessOrderSentry` does `image_index + 1` every tick and after
/// the last frame returns to `image_base`. `D_RADAR.image_count = 16`,
/// although the sprite has 20 frames (4 connectors), so the cycle is exactly 16.
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

    // A non-ANIMATED building gets no sentry cycle.
    let gen = unit_of(UnitType::PowerGenerator, UnitOrder::Sentry, 0);
    assert!(!sentry_animation_active(&gen));
}

/// During construction C++ draws the builder working pose — `image_index + 16`
/// (`unitinfo.cpp:2789` for `ENGINEER`, `4451` for `CONSTRCT`). The frame must
/// return to the base after `ORDER_AWAIT` (C++ `SpawnNewUnit`:
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

/// Construction site decals — the real C++ sprites (`LRGTAPE` 127×127,
/// `LRGSLAB` 127×127, `LRGCONES` 117×117): each is drawn on the map, not as
/// a marker. `SMLTAPE`/`SMLCONES`/`SMLSLAB` — the small 1×1 versions.
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
