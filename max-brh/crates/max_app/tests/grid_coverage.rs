//! GRID button regression (defect 7 of the manual run 1.2, second pass).
//!
//! Oracle: `DrawMap_RenderMapTiles` → `DrawMap_RenderSimpleGridOverlay`
//! (`drawmap.cpp:922-954`) draws a line at the top-left corner of every cell of
//! the redrawn rectangle. The port additionally closes the map window edges
//! (see `Viewport::grid_lines`) so the grid covers the whole playable area even
//! when the camera origin is not cell-aligned — the UI panels abut the map on
//! every side, and the border strip of the outer cells otherwise looked cut off.
//!
//! The test draws through the real `max_app::overlays::draw_grid` path;
//! it fails when the game data (`max-original/`) is not available.

use std::path::{Path, PathBuf};

use max_app::overlays::{draw_grid, grid_color};
use max_app::panels::UiPalette;
use max_app::{App, AppConfig};
use max_core::TeamClan;
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::viewport::Viewport;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn app_with_assets() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    // The checkout keeps `PATCHES.RES` in the C++ oracle's shared assets
    // directory; the app's fallback finds it from the repository root too.
    let patches = repo_path("tests/fixtures/assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("grid_coverage: нет данных — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");
    for index in 0..4 {
        app.universe_mut().team_units_table_mut()[index] =
            max_core::TeamUnits::with_clan(TeamClan::Random);
    }
    Some(app)
}

fn painted_column(frame: &RgbaBuffer, x: i32, color: Rgba) -> usize {
    (0..frame.height() as i32)
        .filter(|y| frame.get(x, *y) == Some(color))
        .count()
}

fn painted_row(frame: &RgbaBuffer, y: i32, color: Rgba) -> usize {
    (0..frame.width() as i32)
        .filter(|x| frame.get(*x, y) == Some(color))
        .count()
}

#[test]
fn grid_covers_all_four_edges_of_the_map_window() {
    let app = app_with_assets().expect(concat!("test fixture is missing: ", stringify!(app_with_assets())));
    let assets = app.assets().expect("ассеты");
    let palette = UiPalette::from_assets(assets).expect("палитра окна");
    let land = grid_color(&palette, max_core::enums::Surface::LAND);

    // Non-cell-aligned camera: origin = 3732 - 224 = 3508 (54.8125 cells), the
    // first cell boundary is at screen 12. Before the fix the left/top closing
    // lines were missing, so columns/rows 0 had no grid pixels.
    let viewport = Viewport::with_center(448, 448, (3732, 3732), 64);
    let mut frame = RgbaBuffer::filled(448, 448, Rgba::BLACK);

    let painted = draw_grid(&mut frame, &app, &viewport, &palette, false);
    assert!(painted > 0, "the grid is drawn");

    assert_eq!(
        painted_column(&frame, 0, land),
        448,
        "the left window edge is part of the grid"
    );
    assert_eq!(
        painted_row(&frame, 0, land),
        448,
        "the top window edge is part of the grid"
    );
    assert_eq!(
        painted_column(&frame, 447, land),
        448,
        "the right window edge is closed"
    );
    assert_eq!(
        painted_row(&frame, 447, land),
        448,
        "the bottom window edge is closed"
    );

    // The interior boundaries are still there (12 = 3520 - 3508 ...).
    assert_eq!(
        painted_column(&frame, 12, land),
        448,
        "interior cell boundary"
    );
    assert_eq!(painted_row(&frame, 12, land), 448, "interior cell boundary");
}
