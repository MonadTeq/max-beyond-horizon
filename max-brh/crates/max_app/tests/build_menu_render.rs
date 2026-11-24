//! `BuildMenu` rendering: `I_*` icons in the list rows (1:4 for buildings, like
//! `ReportStats_DrawListItemIcon`), the selected building portrait — big image
//! `P_*` 300×240 at (11,13) and the `UnitStats_DrawStats` characteristics table with
//! icons (both — `AbstractBuildMenu::Draw`, `buildmenu.cpp:509-536`).
//!
//! The test loads the real `MAX.RES`/`PATCHES.RES` (skipped if there is no data) and
//! compares the menu frame with the clean `CONBUILD`/`FACBUILD` background in the C++
//! rectangles: the 32×32 row icon at `LIST_AREA`, the `PORTRAIT` window, the
//! `STATS` (11,293,250,174).

use std::path::{Path, PathBuf};

use max_app::assets::Assets;
use max_app::buildmenu::{BuildMenu, LIST_AREA, PORTRAIT};
use max_app::mobilize::{Mobilization, STATS};
use max_app::AppConfig;
use max_core::enums::{TeamClan, UnitType};
use max_core::geo::Point;
use max_core::universe::StateRef;
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

/// A match with one RED constructor — the owner of `BuildMenu`.
fn build_state() -> (
    max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    max_core::unit::UnitId,
) {
    let mut state = max_core::universe::GameUniverse::default();
    state
        .team_mut(max_core::TeamId::RED)
        .expect("RED slot")
        .team_type = TeamKind::Player;
    // Attributes from `SC_ATTRI` (as in a real match): without them `ATTRIB_TURNS`
    // = 0, and the characteristics table/turns are empty.
    *state.team_units_mut(max_core::TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(TeamClan::Random);
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

/// The clean menu background — what the added elements are compared against.
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

/// How many pixels of the area differ between two frames.
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
    let menu = BuildMenu::new(StateRef::from(&state), builder).expect("меню конструктора");
    let background = background(&assets, false);
    let mut frame = RgbaBuffer::new(640, 480);

    let painted = menu.draw(&mut frame, &assets, StateRef::from(&state), None);
    assert!(painted > 10_000, "экран BuildMenu нарисован: {painted}");

    // The first row is selected — it has the white corners of `GameManager_DrawUnitSelector`
    // (they change pixels too). For the icons we take non-selected rows: in
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

    // The icon is drawn exactly at the center (16,16) of the row, not as text/name:
    // at this point buildings have an opaque pixel (border/body).
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
    let mut menu = BuildMenu::new(StateRef::from(&state), builder).expect("меню конструктора");
    let mut red = RgbaBuffer::new(640, 480);
    let mut green = RgbaBuffer::new(640, 480);

    menu.draw(&mut red, &assets, StateRef::from(&state), None);
    menu.team = TeamId::GREEN;
    menu.draw(&mut green, &assets, StateRef::from(&state), None);

    // The icons are recolored by the team LUT (`ReportStats_DrawListItemIcon` →
    // `color_index_table`), like the `CargoMenu` lists; so the RED/GREEN frames
    // must differ in the icon area.
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
    let menu = BuildMenu::new(StateRef::from(&state), builder).expect("меню конструктора");
    let background = background(&assets, false);
    let mut frame = RgbaBuffer::new(640, 480);

    menu.draw(&mut frame, &assets, StateRef::from(&state), None);

    let selected = menu.selected_type().expect("выбранный тип");
    assert_eq!(selected, UnitType::MiningStation, "первая строка списка");

    // `AbstractBuildMenu::Draw`: `WindowManager_LoadBigImage(GetPortrait(), ...)`
    // in `window.buffer[11 + width*13]`, big image 300×240, window palette.
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

    // The frame pixels in the portrait window match the P_* indices passed
    // through the window palette: this is exactly the big image, not the unit sprite.
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

    // The portrait differs from the clean window background (i.e. it is drawn, not
    // left as the `CONBUILD` background).
    let diff = changed(&background, &frame, PORTRAIT);
    assert!(diff > 10_000, "портрет нарисован (diff={diff})");
}

#[test]
fn portrait_follows_the_selected_row_and_the_queue_selection() {
    let Some(assets) = assets() else {
        return;
    };
    let (mut state, builder) = build_state();

    // For a factory the queue is selected by a click; the portrait — `selector->GetLast()`
    // of the active selector (list or queue).
    state.unit_mut(builder).unwrap().unit_type = UnitType::TrainingHall;
    let mut menu = BuildMenu::new(StateRef::from(&state), builder).expect("меню завода");
    assert!(menu.factory);

    let mut first = RgbaBuffer::new(640, 480);
    menu.draw(&mut first, &assets, StateRef::from(&state), None);

    // Another list row — another P_* in the portrait window.
    menu.selected = 1;
    let mut second = RgbaBuffer::new(640, 480);
    menu.draw(&mut second, &assets, StateRef::from(&state), None);

    assert!(
        changed(&first, &second, PORTRAIT) > 10_000,
        "портрет сменился вместе с выбранной строкой"
    );

    // Queue: `queue_active` + the selected element → the portrait from the queue.
    menu.queue.push(UnitType::Infantry);
    menu.queue_selected = 0;
    menu.queue_active = true;
    let mut queued = RgbaBuffer::new(640, 480);
    menu.draw(&mut queued, &assets, StateRef::from(&state), None);

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
    let mut menu = BuildMenu::new(StateRef::from(&state), builder).expect("меню конструктора");
    let training_hall = menu
        .unit_types
        .iter()
        .position(|unit| *unit == UnitType::TrainingHall)
        .expect("TRAINHAL in CONSTRCT list");
    menu.select(training_hall);

    let background = background(&assets, false);
    let mut frame = RgbaBuffer::new(640, 480);
    menu.draw(&mut frame, &assets, StateRef::from(&state), None);

    // C++ `AbstractBuildMenu::Draw`: `stats_background->Write(&window)` +
    // `UnitStats_DrawStats(...)` in (11,293,250,174); the I_* icons — the same as
    // in CargoMenu (pips `I_HITS`, `I_ARMOR`), but the `Cost` row is `I_RAW`:
    // production is paid in raw materials, not in credits.
    let diff = changed(&background, &frame, STATS);
    assert!(diff > 1_000, "таблица характеристик нарисована (diff={diff})");

    let team_units = &StateRef::from(&state).team_units()[0];
    let rows = Mobilization::stats_rows_for(
        &team_units.base_values,
        &team_units.current_values,
        UnitType::TrainingHall,
        "I_RAW",
        "I_RAWE",
    );
    let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();
    for needed in ["Hits", "Armor", "Cost"] {
        assert!(labels.contains(&needed), "нет строки {needed}: {labels:?}");
    }

    // The `Cost` row (the last) is really visible in the frame: its bar differs from
    // the background, and it is exactly `UnitStats_DrawCost` — 2 × 10 turns = 20.
    let cost = rows.last().expect("cost row");
    assert_eq!(cost.value, Some(20));
    assert_eq!(cost.icons[0].resource, "I_RAW");
    assert_eq!(cost.icons[0].empty_resource, "I_RAWE");
    let cost_rect = PixelRect::new(STATS.ulx, cost.y, STATS.width, 19);
    assert!(
        changed(&background, &frame, cost_rect) > 50,
        "строка Cost с иконками I_RAW нарисована"
    );
}
