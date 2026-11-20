//! Покрытие спрайтов юнитов: все 93 типа `SC_UNITS` на карте и в портрете.
//!
//! Проверяет то, чего не видно в обычных QA-кадрах: у каждого типа из
//! `UNIT_SPRITES` есть непустой кадр корпуса (не маркер), башня рисуется
//! ровно у типов с `TURRET_SPRITE`/`SPINNING_TURRET`, а полный кадр карты с
//! юнитами всех 93 типов не падает и не оставляет маркеров, кроме
//! задокументированного `MASTER`.
//!
//! Тесты пропускаются, если данных игры (`max-original/`) рядом нет — как
//! `assets_integration_smoke` в `tests/app.rs`.

use std::path::{Path, PathBuf};

use max_app::assets::{UNIT_SPRITES, UNIT_TYPES_WITHOUT_SPRITE_DATA};
use max_app::{draw_unit_sprites, draw_unit_sprites_with_angle, App, AppConfig, Assets};
use max_core::access::unit_type_flags;
use max_core::enums::{unit_flags, UnitType, UNIT_END};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::unit::{UnitId, UnitInfo};
use max_render::buffer::RgbaBuffer;

/// Путь внутри репозитория (`max-rust/crates/max_app/../../..`).
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// Ассеты игры; `None`, если данных рядом нет.
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

/// Синтетический юнит типа `unit_type` с флагами из `SC_UNITS`.
fn unit_of(unit_type: UnitType, index: usize) -> UnitInfo {
    let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
    unit.flags = unit_type_flags(unit_type);
    unit.visible_to_team = [true; 5];
    unit.angle = (index % 8) as u8;
    unit.turret_angle = ((index * 3 + 1) % 8) as u8;
    unit.brightness = 7;
    unit
}

fn has_turret(unit_type: UnitType) -> bool {
    unit_type_flags(unit_type) & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0
}

/// Для каждого из 93 типов (кроме `MASTER`) кадр отрисовки не пуст, а башня
/// рисуется ровно там, где её объявляет `SC_UNITS`.
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
            // 512×512: самый большой кадр — 127×127 (`LRGTAPE`/`LRGSLAB`,
            // `S_POWERS` 164×182 у тени), на zoom 128 это 364 px — с запасом.
            let mut frame = RgbaBuffer::new(512, 512);
            let stats = draw_unit_sprites(&assets, &unit, &mut frame, (256, 256), zoom);

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

            // На пустом кадре должен появиться хотя бы один непрозрачный
            // пиксель — иначе это «пустой» спрайт вместо маркера.
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

/// Полный кадр карты с юнитами всех 93 типов: 92 спрайта и 1 маркер
/// (`MASTER`), без паники на любом типе.
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
        // 10×10 клеток при zoom 64 — ровно под 93 юнита в одном кадре.
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    // Плотный блок 10×10 в центре карты: все юниты попадают в кадр.
    let mut ids = Vec::with_capacity(UNIT_END as usize);
    for index in 0..UNIT_END as usize {
        let unit_type = UnitType::try_from(index as u16).expect("тип 0..92");
        let grid = Point::new(40 + (index as i16 % 10), 40 + (index as i16 / 10));
        let id = app
            .state_mut()
            .spawn_unit(unit_type, TeamId::RED, grid, 0)
            .expect("спавн юнита");
        ids.push(id);
    }

    // Мины/коммандос/подлодка в оригинале скрыты от всех команд
    // (`InitStealthStatus`); для теста отрисовки делаем их видимыми.
    for id in &ids {
        if let Some(unit) = app.state_mut().unit_mut(*id) {
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

/// HUD-портрет берёт ту же запись `UNIT_SPRITES` и рисует непустой кадр для
/// всех 93 типов (турели, флот, авиация, инженеры — в том числе те, которых не
/// было в старой таблице из 43): `panels::draw_portrait` вызывает тот же
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

        if let Some(stats) = draw_unit_sprites(&assets, &unit, &mut portrait, (160, 100), 64) {
            assert!(stats.unit > 0, "{unit_type:?}: пустой портрет");
        } else {
            assert!(
                UNIT_TYPES_WITHOUT_SPRITE_DATA.contains(&unit_type),
                "{unit_type:?}: нет портрета"
            );
        }
    }
}

/// Кадр корпуса выбирается по переданному углу (`image_base + angle`): явный
/// угол даёт ровно тот же кадр, что и `unit.angle`, а разные углы — разные
/// кадры. Это контракт `App::visual_angle` → `draw_unit_sprites_with_angle`.
#[test]
fn explicit_body_angle_selects_the_corresponding_sprite_frame() {
    let Some(assets) = game_assets() else {
        return;
    };

    // Scout — без башни: сравнивается только кадр корпуса.
    let mut unit = unit_of(UnitType::Scout, 0);
    unit.angle = 0;

    let render = |body_angle: u8, unit: &UnitInfo| {
        let mut frame = RgbaBuffer::new(256, 256);
        let stats = draw_unit_sprites_with_angle(
            &assets,
            unit,
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

    // Явный угол эквивалентен `unit.angle` — рендер карты и HUD используют
    // один и тот же селектор кадра.
    let mut turned = unit.clone();
    turned.angle = 3;
    let via_unit_angle = {
        let mut frame = RgbaBuffer::new(256, 256);
        draw_unit_sprites(&assets, &turned, &mut frame, (128, 128), 64).expect("спрайт Scout");
        frame.pixels().to_vec()
    };
    assert_eq!(
        angle_three, via_unit_angle,
        "draw_unit_sprites_with_angle(3) = draw_unit_sprites(unit.angle=3)"
    );
}

// ---------------------------------------------------------------------------
// Кадровые данные симуляции (`Unit::GetFrameInfo` → `image_index_max`)
// ---------------------------------------------------------------------------

/// Таблица `max_core::frames` — те же байты `D_*`, что читает C++
/// (`Unit::FileLoad`, `unit.cpp:51-64`). Значения не подбирались: сверяем все
/// загруженные типы с декодером реальных ресурсов. `MASTER` пропускается —
/// у него в архивах нет спрайта корпуса (`D_MOBILE` читается отдельно и
/// зафиксирован в `frames.rs`).
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

