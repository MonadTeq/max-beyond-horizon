//! Unit sprite coverage: all 93 `SC_UNITS` types on the map and in the portrait.
//!
//! Checks what is not visible in regular QA frames: each type from
//! `UNIT_SPRITES` has a non-empty body frame (not a marker), a turret is drawn
//! exactly for types with `TURRET_SPRITE`/`SPINNING_TURRET`, and a full map frame with
//! units of all 93 types does not panic and leaves no markers except
//! the documented `MASTER`.
//!
//! Tests are skipped if there is no game data (`max-original/`) nearby — like
//! `assets_integration_smoke` in `tests/app.rs`.

use std::path::{Path, PathBuf};

use max_app::assets::{UNIT_SPRITES, UNIT_TYPES_WITHOUT_SPRITE_DATA};
use max_app::{draw_unit_sprites, draw_unit_sprites_with_angle, App, AppConfig, Assets};
use max_core::access::unit_type_flags;
use max_core::enums::{unit_flags, UnitType, UNIT_END};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::unit::{UnitId, UnitInfo, UnitPresentation};
use max_render::buffer::RgbaBuffer;

/// Path inside the repository (`max-brh/crates/max_app/../../..`).
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// Game assets; `None` if there is no data nearby.
fn game_assets() -> Option<Assets> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("unit_sprites: пропуск — нет данных игры рядом с репозиторием");
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

/// A synthetic unit of type `unit_type` with the flags from `SC_UNITS`.
fn unit_of(unit_type: UnitType, index: usize) -> UnitInfo {
    let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
    unit.flags = unit_type_flags(unit_type);
    unit.visible_to_team = [true; 5];
    unit.angle = (index % 8) as u8;
    unit.turret_angle = ((index * 3 + 1) % 8) as u8;
    unit
}

/// Presentation for the synthetic unit (phase 5.3/B): brightness 7, as before in
/// `UnitInfo::brightness`.
fn presentation() -> UnitPresentation {
    let mut presentation = UnitPresentation::new();
    presentation.brightness = 7;
    presentation
}

fn has_turret(unit_type: UnitType) -> bool {
    unit_type_flags(unit_type) & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0
}

/// For each of the 93 types (except `MASTER`) the render frame is non-empty, and the
/// turret is drawn exactly where `SC_UNITS` declares it.
#[test]
fn every_unit_type_draws_a_non_empty_frame_and_turret_where_flagged() {
    let Some(assets) = game_assets() else {
        return;
    };

    let mut drawn = 0usize;
    let mut markers = Vec::new();
    let mut turrets = Vec::new();

    for index in 0..UNIT_END as usize {
        let unit_type = UnitType::try_from(index as u16).expect("тип 0..92");
        let unit = unit_of(unit_type, index);

        for zoom in [32u32, 64, 128] {
            // 512×512: the largest frame is 127×127 (`LRGTAPE`/`LRGSLAB`,
            // `S_POWERS` 164×182 for the shadow), at zoom 128 that is 364 px — with a margin.
            let mut frame = RgbaBuffer::new(512, 512);
            let stats = draw_unit_sprites(
                &assets,
                &unit,
                &presentation(),
                &mut frame,
                (256, 256),
                zoom,
            );

            match stats {
                Some(stats) => {
                    assert!(
                        stats.unit > 0,
                        "{unit_type:?} (zoom {zoom}): корпус не нарисован"
                    );
                    if has_turret(unit_type) {
                        assert!(
                            stats.turret > 0,
                            "{unit_type:?} (zoom {zoom}): флаг башни есть, а башня не нарисована"
                        );
                        turrets.push(unit_type);
                    } else {
                        assert_eq!(
                            stats.turret, 0,
                            "{unit_type:?}: башня без флага TURRET_SPRITE"
                        );
                    }
                    drawn += 1;
                }
                None => {
                    assert!(
                        UNIT_TYPES_WITHOUT_SPRITE_DATA.contains(&unit_type),
                        "{unit_type:?}: спрайт не нарисован, но тип не в UNIT_TYPES_WITHOUT_SPRITE_DATA"
                    );
                    markers.push(unit_type);
                }
            }

            // On an empty frame at least one opaque pixel must appear —
            // otherwise it is an "empty" sprite instead of a marker.
            let painted = frame
                .pixels()
                .chunks_exact(4)
                .any(|pixel| pixel[3] != 0);
            assert!(
                painted || stats.is_none(),
                "{unit_type:?} (zoom {zoom}): кадр пуст"
            );
        }
    }

    assert_eq!(
        drawn,
        (UNIT_END as usize - UNIT_TYPES_WITHOUT_SPRITE_DATA.len()) * 3,
        "кадр корпуса ожидался у всех типов, кроме MASTER"
    );
    assert_eq!(
        markers,
        [UnitType::MasterBuilder; 3],
        "единственный тип-маркер — MASTER (нет ресурса корпуса)"
    );

    turrets.sort_unstable_by_key(|unit| *unit as u16);
    turrets.dedup();
    let mut expected: Vec<UnitType> = (0..UNIT_END)
        .map(|index| UnitType::try_from(index).unwrap())
        .filter(|unit_type| has_turret(*unit_type))
        .collect();
    expected.sort_unstable_by_key(|unit| *unit as u16);
    assert_eq!(
        turrets, expected,
        "башня должна рисоваться у всех типов с TURRET_SPRITE/SPINNING_TURRET"
    );
}

/// Shot flash: the `firing_image_base` frames from `D_*` (and the special
/// `COMMANDO`/`INFANTRY` queue) while `firing_recoil_frames > 0`.
///
/// Oracle: `UnitInfo::PrepareFire` (`unitinfo.cpp:4709-4747`) — the body
/// `DrawSpriteFrame(unit_angle + firing_image_base)`, the turret
/// `DrawSpriteTurretFrame(turret_angle + firing_image_base)`, for
/// `ANTIAIR`/`SP_FLAK`/`FASTBOAT` — a `±8` toggle on each `ProgressFire`
/// (`:4824-4833`); infantry — `104 + angle`, then `+8` per tick.
#[test]
fn firing_frames_come_from_the_d_star_firing_block() {
    use max_app::render::{
        firing_body_frame_index, firing_frame_active, firing_turret_frame_index,
    };

    let Some(assets) = game_assets() else {
        return;
    };

    // Tank: the body does not change, the turret — `firing_image_base + turret_angle`.
    let sprite = assets.sprite(UnitType::Tank).expect("TANK");
    let info = sprite.frame_info.expect("D_TANK");
    let mut tank = unit_of(UnitType::Tank, 0);
    tank.turret_angle = 3;
    tank.firing_recoil_frames = 3;

    assert!(firing_frame_active(&tank));
    assert_eq!(firing_body_frame_index(&info, &tank, 0), None);
    assert_eq!(
        firing_turret_frame_index(&info, &tank),
        Some(usize::from(info.firing_image_base) + 3),
        "туррель: `DrawSpriteTurretFrame(turret_angle + firing_image_base)`"
    );

    // Artillery (no turret): the body frame `firing_image_base + angle`.
    let sprite = assets.sprite(UnitType::AssaultGun).expect("ARTILLRY");
    let info = sprite.frame_info.expect("D_ARTILLRY");
    let mut artillery = unit_of(UnitType::AssaultGun, 0);
    artillery.angle = 5;
    artillery.firing_recoil_frames = 3;
    assert_eq!(
        firing_body_frame_index(&info, &artillery, 5),
        Some(usize::from(info.firing_image_base) + 5)
    );

    // ANTIAIR: 15 recoil frames; the first tick — the firing frame, then ±8.
    let sprite = assets.sprite(UnitType::AntiAir).expect("ANTIAIR");
    let info = sprite.frame_info.expect("D_ANTIAI");
    let mut aa = unit_of(UnitType::AntiAir, 0);
    aa.turret_angle = 0;
    let base = usize::from(info.firing_image_base);
    for (recoil, shift) in [(15, 0), (14, 8), (13, 0), (12, 8)] {
        aa.firing_recoil_frames = recoil;
        assert_eq!(
            firing_turret_frame_index(&info, &aa),
            Some(base + shift),
            "ANTIAIR, отдача {recoil}"
        );
    }

    // SP_FLAK: 6 frames; the first tick — firing, then alternating with the normal one.
    let sprite = assets.sprite(UnitType::MobileAntiAir).expect("SP_FLAK");
    let info = sprite.frame_info.expect("D_SP_FLAK");
    let mut flak = unit_of(UnitType::MobileAntiAir, 0);
    flak.turret_angle = 0;
    let base = usize::from(info.firing_image_base);
    for (recoil, expected) in [(6, base), (5, base - 8), (4, base), (3, base - 8)] {
        flak.firing_recoil_frames = recoil;
        assert_eq!(
            firing_turret_frame_index(&info, &flak),
            Some(expected),
            "SP_FLAK, отдача {recoil}"
        );
    }

    // Infantry: `104 + angle`, then +8 for each recoil tick.
    let sprite = assets.sprite(UnitType::Infantry).expect("INFANTRY");
    let info = sprite.frame_info.expect("D_INFANT");
    let mut infantry = unit_of(UnitType::Infantry, 0);
    for (recoil, expected) in [(8, 104), (7, 112), (1, 160)] {
        infantry.firing_recoil_frames = recoil;
        assert_eq!(
            firing_body_frame_index(&info, &infantry, 0),
            Some(expected),
            "INFANTRY, отдача {recoil}"
        );
    }

    // After recoil (`firing_recoil_frames == 0`) the frame is normal.
    infantry.firing_recoil_frames = 0;
    assert!(!firing_frame_active(&infantry));
    assert_eq!(firing_body_frame_index(&info, &infantry, 0), None);

    // Drawing: the firing frame differs from the normal one.
    let mut normal = unit_of(UnitType::AssaultGun, 0);
    normal.angle = 2;
    let mut firing = normal.clone();
    firing.firing_recoil_frames = 3;
    let mut normal_frame = RgbaBuffer::new(256, 256);
    let mut firing_frame = RgbaBuffer::new(256, 256);
    draw_unit_sprites_with_angle(
        &assets,
        &normal,
        &presentation(),
        0,
        2,
        &mut normal_frame,
        (128, 128),
        64,
    );
    draw_unit_sprites_with_angle(
        &assets,
        &firing,
        &presentation(),
        0,
        2,
        &mut firing_frame,
        (128, 128),
        64,
    );
    assert_ne!(
        normal_frame.pixels(),
        firing_frame.pixels(),
        "вспышка обязана менять кадр спрайта"
    );
}

/// A full map frame with units of all 93 types: 92 sprites and 1 marker
/// (`MASTER`), without panics on any type.
#[test]
fn map_render_loop_covers_all_93_unit_types() {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        return;
    }

    let config = AppConfig::default()
        .with_data_dir(&repo_path("max-original"))
        .with_assets_dir(repo_path("assets"))
        // 10×10 cells at zoom 64 — exactly under 93 units in one frame.
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    // A dense 10×10 block in the map center: all units get into the frame.
    let mut ids = Vec::with_capacity(UNIT_END as usize);
    for index in 0..UNIT_END as usize {
        let unit_type = UnitType::try_from(index as u16).expect("тип 0..92");
        let grid = Point::new(40 + (index as i16 % 10), 40 + (index as i16 / 10));
        let id = app
            .universe_mut()
            .spawn_unit(unit_type, TeamId::RED, grid, 0)
            .expect("спавн юнита");
        ids.push(id);
    }

    // Mines/commandos/submarine are hidden from all teams in the original
    // (`InitStealthStatus`); for the render test we make them visible.
    for id in &ids {
        if let Some(unit) = app.universe_mut().unit_mut(*id) {
            unit.visible_to_team = [true; 5];
        }
    }

    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(45, 45);

    let (frame, stats) = app.render_frame_with_stats();

    assert_eq!((frame.width(), frame.height()), (640, 640));
    assert!(stats.map_tiles > 0);
    assert_eq!(
        stats.units_drawn,
        UNIT_SPRITES.len() - UNIT_TYPES_WITHOUT_SPRITE_DATA.len(),
        "спрайтами должны рисоваться все 92 типа с данными (markers={})",
        stats.units_marker
    );
    assert_eq!(
        stats.units_marker,
        UNIT_TYPES_WITHOUT_SPRITE_DATA.len(),
        "маркер остаётся только у MASTER"
    );
    assert_eq!(
        stats.units(),
        UNIT_END as usize,
        "в кадре должны участвовать все 93 типа"
    );
}

/// The HUD portrait takes the same `UNIT_SPRITES` record and draws a non-empty frame for
/// all 93 types (turrets, navy, aviation, engineers — including those that were not
/// in the old table of 43): `panels::draw_portrait` calls the same
/// `draw_unit_sprites`.
#[test]
fn hud_portrait_draws_every_unit_type() {
    let Some(assets) = game_assets() else {
        return;
    };

    for index in 0..UNIT_END as usize {
        let unit_type = UnitType::try_from(index as u16).expect("тип");
        let unit = unit_of(unit_type, index);
        let mut portrait = RgbaBuffer::new(320, 200);

        if let Some(stats) = draw_unit_sprites(
            &assets,
            &unit,
            &presentation(),
            &mut portrait,
            (160, 100),
            64,
        ) {
            assert!(stats.unit > 0, "{unit_type:?}: пустой портрет");
        } else {
            assert!(
                UNIT_TYPES_WITHOUT_SPRITE_DATA.contains(&unit_type),
                "{unit_type:?}: нет портрета"
            );
        }
    }
}

/// The body frame is chosen by the passed angle (`image_base + angle`): an explicit
/// angle gives exactly the same frame as `unit.angle`, and different angles — different
/// frames. This is the contract `App::visual_angle` → `draw_unit_sprites_with_angle`.
#[test]
fn explicit_body_angle_selects_the_corresponding_sprite_frame() {
    let Some(assets) = game_assets() else {
        return;
    };

    // Scout — without a turret: only the body frame is compared.
    let mut unit = unit_of(UnitType::Scout, 0);
    unit.angle = 0;

    let render = |body_angle: u8, unit: &UnitInfo| {
        let mut frame = RgbaBuffer::new(256, 256);
        let stats = draw_unit_sprites_with_angle(
            &assets,
            unit,
            &presentation(),
            unit.image_base,
            body_angle,
            &mut frame,
            (128, 128),
            64,
        )
        .expect("спрайт Scout");
        assert!(stats.unit > 0);
        frame.pixels().to_vec()
    };

    let angle_zero = render(0, &unit);
    let angle_three = render(3, &unit);
    assert_ne!(
        angle_zero, angle_three,
        "кадры 0 и 3 (восток) должны различаться"
    );

    // The explicit angle is equivalent to `unit.angle` — the map renderer and HUD use
    // the same frame selector.
    let mut turned = unit.clone();
    turned.angle = 3;
    let via_unit_angle = {
        let mut frame = RgbaBuffer::new(256, 256);
        draw_unit_sprites(
            &assets,
            &turned,
            &presentation(),
            &mut frame,
            (128, 128),
            64,
        )
        .expect("спрайт Scout");
        frame.pixels().to_vec()
    };
    assert_eq!(
        angle_three, via_unit_angle,
        "draw_unit_sprites_with_angle(3) = draw_unit_sprites(unit.angle=3)"
    );
}

// ---------------------------------------------------------------------------
// Simulation frame data (`Unit::GetFrameInfo` → `image_index_max`)
// ---------------------------------------------------------------------------

/// The `max_core::frames` table — the same `D_*` bytes that C++ reads
/// (`Unit::FileLoad`, `unit.cpp:51-64`). The values were not picked: we compare all
/// loaded types with the real resource decoder. `MASTER` is skipped —
/// it has no body sprite in the archives (`D_MOBILE` is read separately and
/// fixed in `frames.rs`).
#[test]
fn frame_info_table_matches_game_data() {
    let Some(assets) = game_assets() else {
        return;
    };

    for index in 0..UNIT_END {
        let unit_type = UnitType::try_from(index).expect("UnitType 0..92");

        if UNIT_TYPES_WITHOUT_SPRITE_DATA.contains(&unit_type) {
            continue;
        }

        let sprite = assets
            .sprite(unit_type)
            .unwrap_or_else(|| panic!("{unit_type:?}: нет загруженного спрайта"));
        let info = sprite
            .frame_info
            .unwrap_or_else(|| panic!("{unit_type:?}: нет D_* (FrameInfo)"));
        let table = max_core::frames::frame_info(unit_type);

        assert_eq!(
            table.image_base,
            i16::from(info.image_base),
            "{unit_type:?}: image_base"
        );
        assert_eq!(
            table.image_count,
            i16::from(info.image_count),
            "{unit_type:?}: image_count"
        );
        assert_eq!(
            table.turret_image_base,
            i16::from(info.turret_image_base),
            "{unit_type:?}: turret_image_base"
        );
        assert_eq!(
            table.turret_image_count,
            i16::from(info.turret_image_count),
            "{unit_type:?}: turret_image_count"
        );
    }
}

