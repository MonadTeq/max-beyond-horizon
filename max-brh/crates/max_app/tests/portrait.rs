//! Портрет `WINDOW_CORNER_FLIC`: маппинг тип → FLC, кадр 0, анимация 15 FPS,
//! остановка на `dt = 0` и fallback на спрайт.
//!
//! Оракул — `max-cpp-port/src/game_manager.cpp`:
//!
//! * `GameManager_MenuUnitSelect` → `GameManager_MenuCreateFlic(unit_type)`:
//!   FLC из `Unit::GetFlicsAnimation()` (`F_*`-ресурс MAX.RES → `*.FLC`);
//! * `GameManager_AdvanceFlic`: смена кадра раз в `TIMER_FPS_TO_MS(15)`;
//! * снятие выделения — `GameManager_MenuDeleteFlic` + чёрное окно.
//!
//! Тесты пропускаются, если данных игры рядом нет (как `unit_sprites.rs`).

use std::path::{Path, PathBuf};

use max_app::cargo_unit_data;
use max_app::panels::UiPalette;
use max_app::{App, AppConfig, Assets};
use max_core::enums::{UnitType, UNIT_END};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_render::buffer::RgbaBuffer;
use max_render::layout;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn config() -> Option<AppConfig> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("portrait: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    Some(
        AppConfig::default()
            .with_data_dir(repo_path("max-original"))
            .with_assets_dir(repo_path("assets")),
    )
}

/// `App` с загруженными ассетами.
fn app() -> Option<App> {
    let mut app = App::new(config()?);

    app.init_assets().ok()?;

    Some(app)
}

fn assets(app: &App) -> &Assets {
    app.assets().expect("ассеты загружены")
}

/// Спавнит юнита и выделяет его.
fn select(app: &mut App, unit_type: UnitType) -> max_core::unit::UnitId {
    let id = app
        .state_mut()
        .spawn_unit(unit_type, TeamId::RED, Point::new(20, 20), 0)
        .expect("юнит создан");

    app.select_unit(Some(id));

    id
}

/// Пиксели области портрета из кадра 640×480.
fn portrait_region(frame: &RgbaBuffer) -> Vec<[u8; 4]> {
    let area = layout::CORNER_FLIC;
    let mut out = Vec::with_capacity((area.width * area.height) as usize);

    for y in area.uly..area.uly + area.height {
        for x in area.ulx..area.ulx + area.width {
            let pixel = frame.get(x, y).expect("пиксель в кадре");

            out.push([pixel.r, pixel.g, pixel.b, pixel.a]);
        }
    }

    out
}

/// Рисует кадр с панелями (как `App::render_frame` в окне 1:1).
fn render(app: &App) -> RgbaBuffer {
    let mut frame = RgbaBuffer::new(layout::WINDOW_WIDTH as u32, layout::WINDOW_HEIGHT as u32);

    if let Some(panels) = assets(app).panels.as_ref() {
        panels.draw(&mut frame, app);
    }

    frame
}

#[test]
fn every_unit_type_with_flics_maps_to_a_loaded_128x128_animation() {
    let Some(app) = app() else {
        return;
    };
    let assets = assets(&app);

    let mut with_flic = 0usize;
    let mut tags = std::collections::BTreeSet::new();
    let mut missing = Vec::new();

    for index in 0..UNIT_END {
        let unit_type = UnitType::try_from(index).expect("тип 0..92");
        let Some(tag) = cargo_unit_data::unit_flic(unit_type) else {
            continue;
        };

        assert!(tag.starts_with("F_"), "{unit_type:?}: тег {tag}");

        let Some(flic) = assets.flic(unit_type) else {
            missing.push((unit_type, tag));
            continue;
        };

        assert_eq!((flic.width(), flic.height()), (128, 128), "{tag}: размер");
        assert!(
            flic.is_animated() || flic.frame_count() == 1,
            "{tag}: кадры"
        );
        assert!(
            (1..=30).contains(&flic.frame_count()),
            "{tag}: {}",
            flic.frame_count()
        );

        with_flic += 1;
        tags.insert(tag.to_string());
    }

    assert_eq!(with_flic, 70, "70 типов получают FLC-анимацию портрета");
    assert_eq!(tags.len(), 70, "70 различных F_*-ресурсов");
    // Единственный пробел данных: `MASTER` ссылается на `F_MASTER`, которого
    // нет в MAX.RES (как и спрайта корпуса — `UNIT_TYPES_WITHOUT_SPRITE_DATA`).
    assert_eq!(
        missing,
        vec![(UnitType::MasterBuilder, "F_MASTER")],
        "ожидался только MASTER/F_MASTER"
    );
    assert!(assets.flic(UnitType::MasterBuilder).is_none());
    assert_eq!(assets.flic_count(), 70, "загружены 70 FLC портретов");

    // Остальные 22 типа — без анимации (обломки, взрывы, ракеты, HARVSTER, WALDO).
    let without = (0..UNIT_END)
        .map(|index| UnitType::try_from(index).unwrap())
        .filter(|unit_type| cargo_unit_data::unit_flic(*unit_type).is_none())
        .count();

    assert_eq!(without, 22, "22 типа без F_*");
}

#[test]
fn portrait_draws_the_flc_first_frame_and_dt_zero_keeps_it_still() {
    let Some(mut app) = app() else {
        return;
    };

    let id = select(&mut app, UnitType::Tank);
    let portrait = app.portrait().expect("FLC-анимация танка");

    assert_eq!(portrait.unit(), id);
    assert_eq!(portrait.unit_type(), UnitType::Tank);
    assert_eq!(portrait.frame_index(), 0, "выделили — кадр 0");
    assert!(portrait.is_animated(), "у TANK.FLC 30 кадров");
    // Кадр не пустой: индекс 0 — чёрный фон FLC, но танк занимает сотни пикселей.
    assert!(
        portrait
            .frame()
            .pixels
            .iter()
            .filter(|pixel| **pixel != 0)
            .count()
            > 1000,
        "кадр 0 пустой"
    );

    // QA-кадры: `dt = 0` не двигает анимацию (детерминизм golden).
    for _ in 0..600 {
        app.update_portrait(0.0);
    }

    assert_eq!(app.portrait().unwrap().frame_index(), 0);
    assert_eq!(app.portrait().unwrap().elapsed(), 0.0);

    // Кадр панели совпадает с кадром FLC, покрашенным палитрой окна
    // (C++: `win_draw_rect` переводит индексы системной палитрой).
    let frame = render(&app);
    let region = portrait_region(&frame);
    let palette = UiPalette::from_assets(assets(&app)).unwrap();
    let flc = app.portrait().unwrap().frame();
    let area = layout::CORNER_FLIC;

    // Строки 0..19 занимает имя/статус (рисуются поверх кадра) — сверяем ниже.
    for y in 20..area.height {
        for x in 0..area.width {
            let index = flc.pixels[y as usize * flc.width + x as usize];
            let expected = palette.rgba(index);
            let actual = region[y as usize * area.width as usize + x as usize];

            assert_eq!(
                actual,
                [expected.r, expected.g, expected.b, expected.a],
                "портрет {x},{y} не совпал с кадром FLC"
            );
        }
    }
}

#[test]
fn portrait_advances_at_fifteen_fps_and_loops() {
    let Some(mut app) = app() else {
        return;
    };

    select(&mut app, UnitType::Tank);

    // Один кадр FLC = 1/15 с (`GameManager_AdvanceFlic`).
    app.update_portrait(1.0 / 15.0);
    assert_eq!(app.portrait().unwrap().frame_index(), 1);

    app.update_portrait(1.0 / 15.0);
    assert_eq!(app.portrait().unwrap().frame_index(), 2);

    // Зацикливание: 30 кадров — снова 0 (`animate = 2`).
    for _ in 0..28 {
        app.update_portrait(1.0 / 15.0);
    }
    assert_eq!(app.portrait().unwrap().frame_index(), 0);

    // Второй кадр действительно отличается (анимация, а не статика).
    let first = app.portrait().unwrap().frame().pixels.clone();

    app.update_portrait(1.0 / 15.0);
    assert_ne!(app.portrait().unwrap().frame().pixels, first);
}

#[test]
fn clearing_the_selection_paints_the_portrait_black() {
    let Some(mut app) = app() else {
        return;
    };

    select(&mut app, UnitType::Tank);
    assert!(app.portrait().is_some());

    app.select_unit(None);
    assert!(app.portrait().is_none(), "анимация освобождена");

    let region = portrait_region(&render(&app));

    assert!(
        region.iter().all(|pixel| *pixel == [0, 0, 0, 255]),
        "без выделения окно портрета должно быть чёрным (FillOrRestoreWindow)"
    );
}

#[test]
fn type_without_flic_falls_back_to_the_sprite() {
    let Some(mut app) = app() else {
        return;
    };

    // У обломков (LRGRUBLE) нет `F_*`, но есть спрайт — рисуется ×2.
    select(&mut app, UnitType::LargeRubble);

    assert!(app.portrait().is_none(), "FLC у обломков нет");

    let region = portrait_region(&render(&app));
    let painted = region
        .iter()
        .filter(|pixel| **pixel != [0, 0, 0, 255])
        .count();

    assert!(
        painted > 100,
        "fallback-спрайт не нарисован ({painted} пикселей)"
    );
}

#[test]
fn switching_units_rebuilds_the_animation_from_frame_zero() {
    let Some(mut app) = app() else {
        return;
    };

    select(&mut app, UnitType::Tank);
    app.update_portrait(0.5);
    assert_ne!(app.portrait().unwrap().frame_index(), 0);

    let id = select(&mut app, UnitType::Infantry);

    assert_eq!(app.portrait().unwrap().unit(), id);
    assert_eq!(app.portrait().unwrap().unit_type(), UnitType::Infantry);
    assert_eq!(
        app.portrait().unwrap().frame_index(),
        0,
        "новый юнит — кадр 0"
    );
}
