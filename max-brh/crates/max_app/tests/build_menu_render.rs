//! Отрисовка `BuildMenu`: иконки `I_*` в строках списка (1:4 у зданий, как
//! `ReportStats_DrawListItemIcon`), портрет выбранного здания — big image
//! `P_*` 300×240 в (11,13) и таблица характеристик `UnitStats_DrawStats` с
//! иконками (оба — `AbstractBuildMenu::Draw`, `buildmenu.cpp:509-536`).
//!
//! Тест грузит настоящий `MAX.RES`/`PATCHES.RES` (если данных нет — пропуск) и
//! сравнивает кадр меню с чистым фоном `CONBUILD`/`FACBUILD` в прямоугольниках
//! C++: иконка строки 32×32 при `LIST_AREA`, окно портрета `PORTRAIT`, таблица
//! `STATS` (11,293,250,174).

use std::path::{Path, PathBuf};

use max_app::assets::Assets;
use max_app::buildmenu::{BuildMenu, LIST_AREA, PORTRAIT};
use max_app::mobilize::{Mobilization, STATS};
use max_app::AppConfig;
use max_core::enums::{TeamClan, UnitType};
use max_core::game::GameState;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_render::buffer::{PixelRect, RgbaBuffer};

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

/// Партия с одним конструктором RED — владельцем `BuildMenu`.
fn build_state() -> (GameState, max_core::unit::UnitId) {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    // Атрибуты из `SC_ATTRI` (как в реальной партии): без них `ATTRIB_TURNS`
    // = 0, и таблица характеристик/ходы пусты.
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    let flags = max_core::access::unit_type_flags(UnitType::Constructor);
    let builder = state
        .spawn_unit(
            UnitType::Constructor,
            TeamId::RED,
            Point::new(30, 20),
            flags,
        )
        .expect("конструктор появился");

    (state, builder)
}

/// Чистый фон меню — то, с чем сравниваются добавленные элементы.
fn background(assets: &Assets, factory: bool) -> RgbaBuffer {
    let mut frame = RgbaBuffer::new(640, 480);
    let menu = assets.build_menu.as_ref().expect("BuildMenuAssets");
    let image = if factory {
        menu.facbuild.as_ref()
    } else {
        menu.conbuild.as_ref()
    }
    .expect("FACBUILD/CONBUILD");

    max_app::menu::draw_big_image(&mut frame, image);

    frame
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

#[test]
fn list_rows_draw_the_i_star_icons_of_every_visible_building() {
    let Some(assets) = assets() else {
        return;
    };
    let (state, builder) = build_state();
    let menu = BuildMenu::new(&state, builder).expect("меню конструктора");
    let background = background(&assets, false);
    let mut frame = RgbaBuffer::new(640, 480);

    let painted = menu.draw(&mut frame, &assets, &state, None);
    assert!(painted > 10_000, "экран BuildMenu нарисован: {painted}");

    // Первая строка выбранная — на ней белые уголки `GameManager_DrawUnitSelector`
    // (они тоже меняют пиксели). Для иконок берём невыделенные строки: в
    // C++ `ReportStats_DrawListItemIcon(..., ulx + 16, uly + 16)` — 32×32.
    for row in 1..11 {
        if row >= menu.unit_types.len() {
            break;
        }
        let y = LIST_AREA.uly + row as i32 * 32;
        let icon_rect = PixelRect::new(LIST_AREA.ulx, y, 32, 32);
        let diff = changed(&background, &frame, icon_rect);

        assert!(
            diff > 20,
            "строка {row} ({}): иконка I_* не нарисована (diff={diff})",
            max_app::buildmenu::unit_name(menu.unit_types[row])
        );
    }

    // Иконка отрисована именно в центре (16,16) строки, а не текстом/именем:
    // в этой точке у зданий есть непрозрачный пиксель (граница/корпус).
    let row_zero = PixelRect::new(LIST_AREA.ulx, LIST_AREA.uly, 32, 32);
    assert!(
        changed(&background, &frame, row_zero) > 20,
        "иконка выбранной строки нарисована (уголки + иконка)"
    );
}

#[test]
fn list_icons_are_recolored_for_the_team_like_cargo_menu() {
    let Some(assets) = assets() else {
        return;
    };
    let (state, builder) = build_state();
    let mut menu = BuildMenu::new(&state, builder).expect("меню конструктора");
    let mut red = RgbaBuffer::new(640, 480);
    let mut green = RgbaBuffer::new(640, 480);

    menu.draw(&mut red, &assets, &state, None);
    menu.team = TeamId::GREEN;
    menu.draw(&mut green, &assets, &state, None);

    // Иконки перекрашиваются LUT команды (`ReportStats_DrawListItemIcon` →
    // `color_index_table`), как списки `CargoMenu`; значит кадры RED/GREEN
    // обязаны отличаться в области иконок.
    let mut icon_diff = 0;
    for row in 0..11.min(menu.unit_types.len()) {
        let y = LIST_AREA.uly + row as i32 * 32;
        icon_diff += changed(&red, &green, PixelRect::new(LIST_AREA.ulx, y, 32, 32));
    }

    assert!(
        icon_diff > 50,
        "командные слоты иконок перекрашиваются (RED/GREEN diff={icon_diff})"
    );
}

#[test]
fn portrait_is_the_selected_buildings_p_star_big_image() {
    let Some(assets) = assets() else {
        return;
    };
    let (state, builder) = build_state();
    let menu = BuildMenu::new(&state, builder).expect("меню конструктора");
    let background = background(&assets, false);
    let mut frame = RgbaBuffer::new(640, 480);

    menu.draw(&mut frame, &assets, &state, None);

    let selected = menu.selected_type().expect("выбранный тип");
    assert_eq!(selected, UnitType::MiningStation, "первая строка списка");

    // `AbstractBuildMenu::Draw`: `WindowManager_LoadBigImage(GetPortrait(), ...)`
    // в `window.buffer[11 + width*13]`, big image 300×240, палитра окна.
    let cargo = assets.cargo.as_ref().expect("CargoMenuAssets");
    let portrait = cargo
        .portrait(&assets.max_res, selected)
        .unwrap_or_else(|| panic!("нет P_* портрета {selected:?}"));
    assert_eq!(
        (portrait.width, portrait.height),
        (300, 240),
        "P_* big image"
    );

    let palette = max_app::panels::UiPalette::from_big_image(
        assets
            .build_menu
            .as_ref()
            .unwrap()
            .conbuild
            .as_ref()
            .unwrap(),
    );

    // Пиксели кадра в окне портрета совпадают с индексами P_*, пропущенными
    // через палитру окна: это именно big image, а не спрайт юнита.
    let mut matches = 0;
    let mut total = 0;
    for y in 0..240u32 {
        for x in 0..300u32 {
            let Some(index) = portrait.pixel(x, y) else {
                continue;
            };
            total += 1;
            let color = palette.rgba(index);
            if frame.get(PORTRAIT.ulx + x as i32, PORTRAIT.uly + y as i32) == Some(color) {
                matches += 1;
            }
        }
    }

    assert!(total > 10_000, "у P_* есть пиксели: {total}");
    assert!(
        matches * 100 / total > 80,
        "портрет в кадре — P_* ({matches}/{total} пикселей совпали)"
    );

    // Портрет отличается от чистого фона окна (то есть он нарисован, а не
    // остался фоном `CONBUILD`).
    let diff = changed(&background, &frame, PORTRAIT);
    assert!(diff > 10_000, "портрет нарисован (diff={diff})");
}

#[test]
fn portrait_follows_the_selected_row_and_the_queue_selection() {
    let Some(assets) = assets() else {
        return;
    };
    let (mut state, builder) = build_state();

    // У завода очередь выбирается кликом; портрет — `selector->GetLast()`
    // активного селектора (список или очередь).
    state.unit_mut(builder).unwrap().unit_type = UnitType::TrainingHall;
    let mut menu = BuildMenu::new(&state, builder).expect("меню завода");
    assert!(menu.factory);

    let mut first = RgbaBuffer::new(640, 480);
    menu.draw(&mut first, &assets, &state, None);

    // Другая строка списка — другой P_* в окне портрета.
    menu.selected = 1;
    let mut second = RgbaBuffer::new(640, 480);
    menu.draw(&mut second, &assets, &state, None);

    assert!(
        changed(&first, &second, PORTRAIT) > 10_000,
        "портрет сменился вместе с выбранной строкой"
    );

    // Очередь: `queue_active` + выбранный элемент → портрет из очереди.
    menu.queue.push(UnitType::Infantry);
    menu.queue_selected = 0;
    menu.queue_active = true;
    let mut queued = RgbaBuffer::new(640, 480);
    menu.draw(&mut queued, &assets, &state, None);

    assert!(
        changed(&first, &queued, PORTRAIT) > 10_000,
        "портрет показывает выбранную строку очереди"
    );
}

#[test]
fn stats_table_shows_the_building_characteristics_with_icons() {
    let Some(assets) = assets() else {
        return;
    };
    let (state, builder) = build_state();
    let mut menu = BuildMenu::new(&state, builder).expect("меню конструктора");
    let training_hall = menu
        .unit_types
        .iter()
        .position(|unit| *unit == UnitType::TrainingHall)
        .expect("TRAINHAL in CONSTRCT list");
    menu.select(training_hall);

    let background = background(&assets, false);
    let mut frame = RgbaBuffer::new(640, 480);
    menu.draw(&mut frame, &assets, &state, None);

    // C++ `AbstractBuildMenu::Draw`: `stats_background->Write(&window)` +
    // `UnitStats_DrawStats(...)` в (11,293,250,174); иконки I_* — те же, что
    // в CargoMenu (pips `I_HITS`, `I_ARMOR`, `I_GOLD`).
    let diff = changed(&background, &frame, STATS);
    assert!(diff > 1_000, "таблица характеристик нарисована (diff={diff})");

    let team_units = &state.team_units[0];
    let rows = Mobilization::stats_rows_for(
        &team_units.base_values,
        &team_units.current_values,
        UnitType::TrainingHall,
    );
    let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();
    for needed in ["Hits", "Armor", "Cost"] {
        assert!(labels.contains(&needed), "нет строки {needed}: {labels:?}");
    }

    // Строка `Cost` (последняя) реально видна в кадре: её полоса отличается от
    // фона, и это ровно `UnitStats_DrawCost` — 2 × 10 ходов = 20.
    let cost = rows.last().expect("cost row");
    assert_eq!(cost.value, Some(20));
    let cost_rect = PixelRect::new(STATS.ulx, cost.y, STATS.width, 19);
    assert!(
        changed(&background, &frame, cost_rect) > 50,
        "строка Cost с иконками I_GOLD нарисована"
    );
}
