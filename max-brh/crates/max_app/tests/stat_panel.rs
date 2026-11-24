//! Frame tests of the `WINDOW_STAT_WINDOW` statistics panel (10,173) 153×48.
//!
//! Oracle — `max-cpp-port/src/game_manager.cpp`: `GameManager_UpdateInfoDisplay`
//! + `GameManager_DrawInfoDisplayRow` (`if (base_value && factor)`). Rows with
//! a zero base are not drawn by C++, so for a mine (no `SPEED`, `SHOTS`) only
//! `Hits`, `Cargo` and the complex stock (`Total`) are visible, and for a tank — all four
//! (`Hits/Ammo/Speed/Shots`).
//!
//! The tests use the real `MAX.RES`/`PATCHES.RES` (like `portrait.rs`) and
//! are skipped if there is no game data next to the repository.

use std::path::{Path, PathBuf};

use max_app::panels::UiPalette;
use max_app::{App, AppConfig};
use max_core::enums::{TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamUnits};
use max_core::unit::UnitId;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::layout;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// `App` with loaded assets and the RED team values from `SC_ATTRI`.
fn app() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("stat_panel: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let mut app = App::new(
        AppConfig::default()
            .with_data_dir(repo_path("max-original"))
            .with_assets_dir(repo_path("assets")),
    );

    app.init_assets().ok()?;

    let state = app.universe_mut();
    state.set_player_team(TeamId::RED);
    state.team_units_table_mut()[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Random);

    Some(app)
}

/// A unit in the match: `spawn_unit` + `DeployUnit` (a new unit has `storage = 0`).
fn spawn(app: &mut App, unit_type: UnitType, grid: Point) -> UnitId {
    let id = app
        .universe_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .expect("юнит создан");

    app.universe_mut().unit_mut(id).expect("юнит").storage = 0;

    id
}

/// Text color pixels (`0x01/0x02/0x04/0xA2`) inside the rectangle.
fn text_pixels(frame: &RgbaBuffer, palette: &UiPalette, rect: PixelRect) -> usize {
    let colors = [
        palette.rgba(0x01),
        palette.rgba(0x02),
        palette.rgba(0x04),
        palette.rgba(0xA2),
    ];
    let mut count = 0;

    for y in rect.uly..rect.uly + rect.height {
        for x in rect.ulx..rect.ulx + rect.width {
            if frame.get(x, y).is_some_and(|pixel| colors.contains(&pixel)) {
                count += 1;
            }
        }
    }

    count
}

/// The four `WINDOW_STAT_ROW_1..4` lines of the frame: how much text is in each.
fn row_counts(frame: &RgbaBuffer, palette: &UiPalette) -> [usize; 4] {
    [
        layout::STAT_ROW_1,
        layout::STAT_ROW_2,
        layout::STAT_ROW_3,
        layout::STAT_ROW_4,
    ]
    .map(|rect| text_pixels(frame, palette, rect))
}

#[test]
fn mine_panel_frame_skips_speed_and_empty_rows_tank_keeps_them() {
    let Some(mut app) = app() else {
        return;
    };
    let palette = UiPalette::from_assets(app.assets().expect("ассеты")).expect("палитра FRAMEPIC");

    // Without a selection the statistics lines are the window background — the "empty line" reference.
    app.select_unit(None);
    let baseline = row_counts(&app.render_frame(), &palette);

    // Mine: C++ draws Hits, Cargo and the complex stock (row 3), row 4 is empty.
    let mine = spawn(&mut app, UnitType::MiningStation, Point::new(22, 22));
    app.select_unit(Some(mine));
    let mine_rows = row_counts(&app.render_frame(), &palette);

    assert!(
        mine_rows[0] > baseline[0],
        "Hits у шахты должен быть нарисован: {mine_rows:?} vs {baseline:?}"
    );
    assert!(
        mine_rows[1] > baseline[1],
        "Cargo у шахты должен быть нарисован: {mine_rows:?}"
    );
    assert!(
        mine_rows[2] > baseline[2],
        "строка 3 у шахты — Total запаса, а не Speed: {mine_rows:?}"
    );
    assert_eq!(
        mine_rows[3], baseline[3],
        "строка 4 (Shots) у шахты пуста: SPEED/SHOTS не рисуются: {mine_rows:?}"
    );

    // Tank: all four rows (Hits/Ammo/Speed/Shots).
    let tank = spawn(&mut app, UnitType::Tank, Point::new(24, 22));
    app.select_unit(Some(tank));
    let tank_rows = row_counts(&app.render_frame(), &palette);

    for (index, count) in tank_rows.iter().enumerate() {
        assert!(
            *count > baseline[index],
            "ряд {} у танка должен быть нарисован: {tank_rows:?}",
            index + 1
        );
    }

    // The panels differ: the mine has 3 rows, the tank 4.
    let filled = |rows: &[usize; 4]| {
        rows.iter()
            .zip(baseline.iter())
            .filter(|(count, empty)| count > empty)
            .count()
    };

    assert_eq!(filled(&mine_rows), 3, "шахта: Hits/Cargo/Total");
    assert_eq!(filled(&tank_rows), 4, "танк: Hits/Ammo/Speed/Shots");
}

/// Pixels that differ between two frames inside `rect`.
fn changed_pixels(before: &RgbaBuffer, after: &RgbaBuffer, rect: PixelRect) -> usize {
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

/// DEFECT 2 regression: the statistics rows draw the `SI_*`/`EI_*` pip icons after
/// the 75 px label area (`GameManager_DrawInfoDisplayRowIcons`,
/// `game_manager.cpp:7290-7367`). The port used to draw only the `current/base` value
/// and the label — the icon strip stayed the window background.
#[test]
fn stat_rows_draw_the_pip_icons() {
    let Some(mut app) = app() else {
        return;
    };
    let palette = UiPalette::from_assets(app.assets().expect("ассеты")).expect("палитра FRAMEPIC");

    app.select_unit(None);
    let empty = app.render_frame();
    let tank = spawn(&mut app, UnitType::Tank, Point::new(24, 22));

    app.select_unit(Some(tank));
    let frame = app.render_frame();

    // `GameManager_DrawInfoDisplayRow` puts the icons at `buffer[75]`.
    for rect in [
        layout::STAT_ROW_1,
        layout::STAT_ROW_2,
        layout::STAT_ROW_3,
        layout::STAT_ROW_4,
    ] {
        let strip = PixelRect::new(rect.ulx + 75, rect.uly, rect.width - 75, rect.height);

        assert!(
            changed_pixels(&empty, &frame, strip) > 0,
            "ряд {:?}: полоса иконок пуста",
            rect
        );
    }

    // The label area (0…45 px) still holds the `0xA2` text.
    assert!(
        text_pixels(
            &frame,
            &palette,
            PixelRect::new(
                layout::STAT_ROW_1.ulx,
                layout::STAT_ROW_1.uly,
                75,
                layout::STAT_ROW_1.height
            )
        ) > 0
    );
}

#[test]
fn training_hall_panel_frame_replaces_speed_with_usage_and_total() {
    let Some(mut app) = app() else {
        return;
    };
    let palette = UiPalette::from_assets(app.assets().expect("ассеты")).expect("палитра FRAMEPIC");

    app.select_unit(None);
    let baseline = row_counts(&app.render_frame(), &palette);

    // Factory: Type1 (`life = 1`) — Hits/Usage/Total, row 4 is empty.
    let hall = spawn(&mut app, UnitType::TrainingHall, Point::new(26, 22));
    app.select_unit(Some(hall));
    let rows = row_counts(&app.render_frame(), &palette);

    assert!(rows[0] > baseline[0], "Hits: {rows:?}");
    assert!(rows[1] > baseline[1], "Usage: {rows:?}");
    assert!(rows[2] > baseline[2], "Total: {rows:?}");
    assert_eq!(rows[3], baseline[3], "Shots у завода пуст: {rows:?}");
}
