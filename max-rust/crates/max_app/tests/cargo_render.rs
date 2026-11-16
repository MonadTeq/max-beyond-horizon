//! Отрисовка `CargoMenu` (мобилизация): иконки списков, полосы золота/cargo,
//! стрелки апгрейдов, переключатель режима.
//!
//! Тест грузит настоящий `MAX.RES`/`PATCHES.RES` (если данных нет — пропуск) и
//! сверяет изменившиеся пиксели областей кадра с эталонными прямоугольниками
//! C++ (`cargomenu.cpp`): иконка строки 32×32, полоса `VERTGOLD` (361,276),
//! cargo-слайдер (421,301), стрелки `UPGLFT`/`UPGRGT` (283/301), `CRGTOG`
//! (542,446).

use std::path::{Path, PathBuf};

use max_app::assets::Assets;
use max_app::mobilize::{
    Mobilization, CARGO_BAR, GOLD_BAR, LIST_UP, PURCHASE_UNITS, TOGGLE_BUTTON, UPGRADE_COST_LIMIT,
};
use max_app::AppConfig;
use max_core::enums::{TeamClan, UnitType};
use max_core::team::TeamId;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::GnwFontId;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn assets() -> Option<Assets> {
    let root = root();
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        return None;
    }

    let config = AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        world_dir: Some(root.join("max-original")),
        ..Default::default()
    };

    Assets::load(&config).ok()
}

/// Сколько пикселей области различаются между двумя кадрами.
fn changed(before: &RgbaBuffer, after: &RgbaBuffer, rect: PixelRect) -> usize {
    let mut count = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            if before.get(x, y) != after.get(x, y) {
                count += 1;
            }
        }
    }

    count
}

fn draw(mobilization: &Mobilization, assets: &Assets) -> (RgbaBuffer, RgbaBuffer) {
    let mut background = RgbaBuffer::new(640, 480);
    let mut frame = RgbaBuffer::new(640, 480);

    let image = assets
        .cargo
        .as_ref()
        .and_then(|cargo| cargo.background.as_ref())
        .expect("CARGOPIC");

    max_app::menu::draw_big_image(&mut background, image);
    let painted = mobilization.draw(&mut frame, assets, "", None);
    assert!(painted > 10_000, "экран нарисован: {painted} пикселей");

    (background, frame)
}

#[test]
fn cargo_assets_carry_icons_for_every_purchase_type() {
    let Some(assets) = assets() else {
        return;
    };
    let cargo = assets.cargo.as_ref().expect("CargoMenuAssets");

    assert!(cargo.is_ready(), "CARGOPIC + Done/Cancel");
    assert!(cargo.toggle.is_some(), "CRGTOG");
    assert!(cargo.filter_disabled.is_some(), "CRGSELDX");
    for unit in PURCHASE_UNITS {
        assert!(cargo.icon(unit).is_some(), "нет иконки {unit:?}");
    }
    // Иконки зданий и техники из полного списка.
    for unit in [
        UnitType::PowerGenerator,
        UnitType::Tank,
        UnitType::Fighter,
        UnitType::Shipyard,
        UnitType::LandMine,
    ] {
        assert!(cargo.icon(unit).is_some(), "нет иконки {unit:?}");
    }
    // Полосы: VERTRAW/VERTFUEL/VERTGOLD.
    assert!(cargo.bars.iter().all(Option::is_some), "VERT*");
}

#[test]
fn purchase_screen_draws_icons_bars_and_buttons() {
    let Some(assets) = assets() else {
        return;
    };
    let mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 150);
    let (background, frame) = draw(&mobilization, &assets);

    // Иконка строки списка «Available» (первая строка, Constructor).
    let available_icon = PixelRect::new(482, 80, 32, 32);
    assert!(
        changed(&background, &frame, available_icon) > 40,
        "иконка доступного юнита: {}",
        changed(&background, &frame, available_icon)
    );

    // Иконка первой строки «Purchased» (бесплатный Constructor).
    let purchased_icon = PixelRect::new(337, 45, 32, 32);
    assert!(changed(&background, &frame, purchased_icon) > 40);

    // Полоса золота: заливка `VERTGOLD` — нижняя часть (start_gold = credits).
    let gold_fill = PixelRect::new(GOLD_BAR.ulx + 10, GOLD_BAR.lry() - 20, 20, 20);
    assert!(
        changed(&background, &frame, gold_fill) > 100,
        "заливка золота: {}",
        changed(&background, &frame, gold_fill)
    );

    // Cargo-слайдер: у бесплатного конструктора cargo 40/60 — заливка снизу.
    let cargo_fill = PixelRect::new(CARGO_BAR.ulx, CARGO_BAR.lry() - 40, CARGO_BAR.width, 40);
    assert!(changed(&background, &frame, cargo_fill) > 100);

    // Стрелки апгрейдов (Constructor: armor/hits/scan/speed после двух пропусков).
    let arrows = PixelRect::new(283, 331, 36, 17);
    assert!(changed(&background, &frame, arrows) > 20);

    // Переключатель режима.
    assert!(changed(&background, &frame, TOGGLE_BUTTON) > 500);

    // Кнопки Buy/Delete/Cancel/Done.
    for rect in [
        PixelRect::new(590, 386, 41, 22),
        PixelRect::new(412, 240, 53, 23),
        PixelRect::new(357, 452, 62, 23),
        PixelRect::new(447, 452, 54, 23),
    ] {
        assert!(changed(&background, &frame, rect) > 100);
    }

    // Название и цена первой строки (шрифт 5).
    assert!(changed(&background, &frame, PixelRect::new(482, 80, 123, 32)) > 100);
}

#[test]
fn upgrade_screen_shows_more_of_the_frame() {
    let Some(assets) = assets() else {
        return;
    };
    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 500);
    mobilization.select_type(5); // Tank — 4 военных апгрейда
    let (background, buy_frame) = draw(&mobilization, &assets);
    let buy_changed = changed(&background, &buy_frame, PixelRect::new(0, 0, 640, 480));

    mobilization.toggle_mode();
    assert!(mobilization.types.len() > PURCHASE_UNITS.len());
    let (_, upgrade_frame) = draw(&mobilization, &assets);
    let upgrade_changed = changed(&background, &upgrade_frame, PixelRect::new(0, 0, 640, 480));

    assert!(
        upgrade_changed > buy_changed,
        "список модернизации плотнее: {upgrade_changed} vs {buy_changed}"
    );

    // Фильтры домена в режиме модернизации рисуются активными (`_D`).
    let filter = PixelRect::new(467, 411, 32, 31);
    assert!(changed(&background, &upgrade_frame, filter) > 50);

    // Стрелки всех строк Tank: attack/rounds/range/ammo/armor/hits/scan/speed.
    assert_eq!(mobilization.controls.len(), 8, "Tank: 8 строк");
    let (left, right) = mobilization.control_arrows(7).expect("speed");
    assert!(changed(&background, &upgrade_frame, left) > 5);
    assert!(changed(&background, &upgrade_frame, right) > 5);
}

#[test]
fn upgrade_costs_are_shown_within_the_limit() {
    let Some(assets) = assets() else {
        return;
    };
    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 500);
    mobilization.select_type(5); // TANK

    for row in 0..mobilization.controls.len() {
        let quote = mobilization.quote(row).expect("строка");
        let cost = quote.cost();
        assert!(cost > 0, "Tank строка {row}: цена {cost}");
        assert!(
            cost < UPGRADE_COST_LIMIT,
            "Tank строка {row}: цена {cost} должна показываться"
        );
    }

    // Числа цен и иконка золота в колонке (321..361).
    let (background, frame) = draw(&mobilization, &assets);
    let column = PixelRect::new(321, 331, 40, 100);
    assert!(changed(&background, &frame, column) > 100);

    // Заголовки Cost/Credit/Cargo и значения над ними.
    let font = GnwFontId::Font5.font();
    assert_eq!(font.height(), 10);
    let values = PixelRect::new(320, 267, 137, 30);
    assert!(changed(&background, &frame, values) > 100);
}

#[test]
fn full_purchase_list_fits_nine_rows_with_scroll() {
    let Some(assets) = assets() else {
        return;
    };
    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 1000);

    // Список покупки — 15 типов, видно 9; последняя строка достигается
    // прокруткой (стрелка `BLDDWN`).
    assert_eq!(mobilization.types.len(), 15);
    assert!(mobilization.types.len() > 9);

    let (background, first) = draw(&mobilization, &assets);
    let top_row = PixelRect::new(482, 80, 123, 32);
    assert!(changed(&background, &first, top_row) > 100);

    mobilization.scroll(max_app::mobilize::ActiveList::Types, 1);
    assert_eq!(mobilization.type_page, 1);
    let (_, scrolled) = draw(&mobilization, &assets);
    assert!(
        changed(&first, &scrolled, PixelRect::new(482, 80, 123, 288)) > 100,
        "прокрутка списка меняет кадр"
    );

    // Стрелка вверх появляется, когда страница не первая.
    let up = LIST_UP;
    let (background, scrolled) = (background, scrolled);
    assert!(changed(&background, &scrolled, up) > 5);
}
