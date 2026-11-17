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
    upgrade_types, Mobilization, AVAILABLE_AREA, CARGO_BAR, DESCRIPTION, GOLD_BAR, LIST_UP,
    PURCHASE_UNITS, STATS, STAT_ICON_X, TOGGLE_BUTTON, UPGRADE_COST_LIMIT,
};
use max_app::AppConfig;
use max_core::enums::{TeamClan, UnitType};
use max_core::team::TeamId;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::palette::{ColorLut, TeamColors, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
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
fn turret_icons_carry_the_second_frame_and_d2_offset() {
    let Some(assets) = assets() else {
        return;
    };
    let cargo = assets.cargo.as_ref().expect("CargoMenuAssets");

    // `ReportStats_DrawListItemIcon`: кадр 0 — корпус, кадр 1 — башня,
    // сдвиг — `angle_offsets[2]` ресурса `D_*` (см. `units.cpp`).
    let tank = cargo.icon(UnitType::Tank).expect("I_TANK");
    assert!(tank.has_turret(), "танк: отдельный кадр башни");
    assert_eq!(tank.turret_offset, (-8, 0), "D_TANK angle_offsets[2]");
    let turret = tank.turret.as_ref().unwrap();
    assert_eq!((turret.width, turret.height), (38, 24));

    let flak = cargo.icon(UnitType::MobileAntiAir).expect("I_AA");
    assert!(flak.has_turret(), "мобильная зенитка");
    assert_eq!(flak.turret_offset, (11, -1), "D_SP_FLK angle_offsets[2]");

    let escort = cargo.icon(UnitType::FastBoat).expect("I_ESCORT");
    assert!(escort.has_turret(), "эскорт");
    assert_eq!(escort.turret_offset, (4, 0), "D_ESCORT angle_offsets[2]");

    // Все 11 типов с `TURRET_SPRITE`/`SPINNING_TURRET` (`SC_UNITS`): у каждого
    // в `I_*` есть второй кадр, а в `D_*` — сдвиг башни для angle 2.
    for unit in [
        UnitType::GunTurret,
        UnitType::AntiAir,
        UnitType::ArtilleryTurret,
        UnitType::AntiMissile,
        UnitType::Tank,
        UnitType::MobileAntiAir,
        UnitType::FastBoat,
        UnitType::Battleship,
        UnitType::AlienTank,
        UnitType::Scanner,
        UnitType::Awac,
    ] {
        let icon = cargo.icon(unit).unwrap_or_else(|| panic!("{unit:?}: I_*"));
        assert!(
            max_core::access::unit_type_flags(unit)
                & (max_core::enums::unit_flags::TURRET_SPRITE
                    | max_core::enums::unit_flags::SPINNING_TURRET)
                != 0,
            "{unit:?}: флаг турели"
        );
        assert!(icon.has_turret(), "{unit:?}: второй кадр башни");
        let turret = icon.turret.as_ref().unwrap();
        assert!(turret.width > 0 && turret.height > 0, "{unit:?}");
        // Сдвиг `angle_offsets[2]` берётся из `D_*`, а не из спрайта `.SPW`.
        let frame_data = max_app::cargo_unit_data::unit_frame_data(unit)
            .and_then(|data| assets.max_res.read(data).ok())
            .and_then(|bytes| max_assets::sprite::FrameInfo::decode(&bytes))
            .unwrap_or_else(|| panic!("{unit:?}: D_*"));
        assert_eq!(
            frame_data.turret_offset(2),
            icon.turret_offset,
            "{unit:?}: сдвиг angle 2"
        );
    }

    // Без башни: у Constructor в `I_CONTRC` один кадр.
    assert!(!cargo.icon(UnitType::Constructor).unwrap().has_turret());
}

#[test]
fn turret_is_painted_over_the_tank_icon_in_the_list() {
    let Some(assets) = assets() else {
        return;
    };
    let mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 150);

    // Тот же кадр, но у иконки танка убран второй кадр: разница в строке
    // Tank — это ровно башня.
    let mut without_turret = assets.clone();
    if let Some(cargo) = without_turret.cargo.as_mut() {
        for (unit, icon) in cargo.icons.iter_mut() {
            if *unit == UnitType::Tank {
                icon.turret = None;
            }
        }
    }

    let (_, with) = draw(&mobilization, &assets);
    let (_, without) = draw(&mobilization, &without_turret);

    // `CargoMenu::unit_types1`: Tank — 6-я строка (индекс 5).
    let tank_row = PixelRect::new(482, 80 + 5 * 32, 32, 32);
    let turret_pixels = changed(&without, &with, tank_row);
    assert!(turret_pixels > 20, "башня танка в списке: {turret_pixels} px");

    // Соседняя строка не меняется — рисуется только вторая иконка.
    let constructor_row = PixelRect::new(482, 80, 32, 32);
    assert_eq!(changed(&without, &with, constructor_row), 0);
}

#[test]
fn sprite_renderer_draws_the_tank_turret_as_a_separate_frame() {
    let Some(assets) = assets() else {
        return;
    };
    let sprite = assets.sprite(UnitType::Tank).expect("TANK");
    let body = sprite.frame_for(0, 0).expect("корпус");
    let turret = sprite.turret_frame(0).expect("башня");
    let colors = TeamColors::ALL[TeamId::RED.0 as usize];
    let renderer = SpriteRenderer::new(
        &assets.palette,
        &assets.brightness,
        ColorLut::for_team(colors),
        FULL_BRIGHTNESS_LEVEL,
    );

    let mut with = RgbaBuffer::new(160, 160);
    let mut without = RgbaBuffer::new(160, 160);
    renderer.draw_unit(&mut with, body, None, (80, 80), (0, 0));
    renderer.draw_unit(&mut without, body, None, (80, 80), (0, 0));
    renderer.draw_turret_scaled(
        &mut with,
        turret,
        None,
        (80, 80),
        (0, 0),
        sprite.turret_offset(0),
        64,
    );

    // `angle_offsets[0]` у `D_TANK` = (0, 7): башня смещается вниз и добавляет
    // непрозрачные пиксели поверх корпуса.
    assert_eq!(sprite.turret_offset(0), (0, 7));
    assert!(
        changed(&without, &with, PixelRect::new(0, 0, 160, 160)) > 100,
        "кадр башни рисуется отдельно"
    );
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

    mobilization.toggle_mode();
    assert!(mobilization.types.len() > PURCHASE_UNITS.len());
    let (_, upgrade_frame) = draw(&mobilization, &assets);

    // Список модернизации — другой набор строк (62 типа против 15), а не
    // просто «больше пикселей»: иконки обеих страниц теперь с башнями.
    assert!(
        changed(&buy_frame, &upgrade_frame, AVAILABLE_AREA) > 1_000,
        "список модернизации отличается от списка покупки"
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

#[test]
fn portrait_is_the_p_star_big_image_for_every_selectable_type() {
    let Some(assets) = assets() else {
        return;
    };
    let cargo = assets.cargo.as_ref().expect("CargoMenuAssets");

    let mut all = PURCHASE_UNITS.to_vec();
    all.extend(upgrade_types(&[true, true, true, true, false]));
    all.sort_by_key(|unit| *unit as u16);
    all.dedup();

    assert!(all.len() >= 60, "типов: {}", all.len());

    // `CargoMenuAssets::portrait` читает `P_*` из `MAX.RES` — ресурс есть у
    // каждого открываемого типа (`Unit::GetPortrait()`).
    for unit in all {
        let portrait = cargo.portrait(&assets.max_res, unit);
        assert!(portrait.is_some(), "{unit:?}: P_* портрет");
        let portrait = portrait.unwrap();
        assert_eq!((portrait.width, portrait.height), (300, 240), "{unit:?}");
    }

    // Не-юниты без портрета (`INVALID_ID`) — пусто.
    for unit in [UnitType::Rocket, UnitType::Harvester, UnitType::LargeRubble] {
        assert!(cargo.portrait(&assets.max_res, unit).is_none(), "{unit:?}");
    }
}

#[test]
fn purchase_screen_draws_portrait_description_and_stat_pips() {
    let Some(assets) = assets() else {
        return;
    };
    let mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 150);
    let (background, frame) = draw(&mobilization, &assets);

    // Портрет `P_CONTRC` 300×240 в (11,13): окно 280×240 почти всё занято
    // картинкой (фон `CARGOPIC` тёмный, портрет — сцена).
    let portrait = PixelRect::new(11, 13, 280, 240);
    let portrait_pixels = changed(&background, &frame, portrait);
    assert!(
        portrait_pixels > 280 * 240 / 2,
        "портрет нарисован: {portrait_pixels} px"
    );

    // Описание рисуется поверх портрета (`GetDescription()` + `BLDDES`):
    // с выключенным флагом область описания отличается только текстом.
    let mut without_description = mobilization.clone();
    without_description.description = false;
    let (_, no_description) = draw(&without_description, &assets);
    let description_pixels = changed(&no_description, &frame, DESCRIPTION);
    assert!(
        description_pixels > 300,
        "описание видно: {description_pixels} px"
    );
    // Вне области описания (левее кнопки `BLDDES` 292,264) кадр тот же —
    // переключается только текст.
    let below = PixelRect::new(DESCRIPTION.ulx, DESCRIPTION.lry(), 260, 10);
    assert_eq!(changed(&no_description, &frame, below), 0);

    // Иконки статистики (`UnitStats_DrawIcons`) — колонка от x=76:
    // у Constructor заполнены Cargo/Uses/Armor/Hits/Scan/Speed/Cost.
    let pips = PixelRect::new(
        STATS.ulx + STAT_ICON_X,
        STATS.uly,
        STATS.width - STAT_ICON_X,
        STATS.height - 4,
    );
    let pip_pixels = changed(&background, &frame, pips);
    assert!(pip_pixels > 500, "pips нарисованы: {pip_pixels} px");

    // Числа и подписи слева от pips (x=24/28).
    let labels = PixelRect::new(STATS.ulx, STATS.uly, STAT_ICON_X, 140);
    assert!(changed(&background, &frame, labels) > 300);

    // Красные линии-разделители под строками (`draw_line(2,18 — 248,18)`):
    // строка y = 293 + 18 = 311 отличается от фона на всю ширину блока.
    let separator = PixelRect::new(STATS.ulx + 2, STATS.uly + 18, STATS.width - 4, 1);
    let separator_pixels = changed(&background, &frame, separator);
    assert!(
        separator_pixels > 200,
        "линия строки нарисована: {separator_pixels} px"
    );
}

#[test]
fn stat_rows_change_with_the_selected_unit() {
    let Some(assets) = assets() else {
        return;
    };
    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 500);
    mobilization.select_loadout(0); // Constructor: Cargo/Uses/Armor/Hits/Scan/Speed/Cost

    let (_, constructor) = draw(&mobilization, &assets);
    assert_eq!(mobilization.stats_rows().len(), 7);

    mobilization.info = Some(UnitType::Tank);
    assert_eq!(mobilization.stats_rows().len(), 9);
    let (_, tank) = draw(&mobilization, &assets);

    let stats = PixelRect::new(
        STATS.ulx + STAT_ICON_X,
        STATS.uly,
        STATS.width - STAT_ICON_X,
        9 * 19,
    );
    assert!(
        changed(&constructor, &tank, stats) > 500,
        "сетка статистики зависит от типа"
    );

    // Портрет тоже другой (`P_TANK` против `P_CONTRC`).
    assert!(changed(&constructor, &tank, PixelRect::new(11, 13, 280, 240)) > 10_000);
}
