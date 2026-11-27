//! Window geometry: the map camera is the 448×448 `WINDOW_MAIN_MAP` area, not the
//! whole 640×480 frame.
//!
//! Oracle: `WindowManager_ScaleWindows` (`max-cpp-port/src/window_manager.cpp:202-225`)
//! computes `WindowManager_MapWidth/MapHeight` as the size of `WINDOW_MAIN_MAP`
//! inside the screen (448×448 at (180,18) in the 640×480 window), and
//! `GameManager_CenterMainMapView`/`GameManager_UpdateMapDrawBounds`
//! (`game_manager.cpp:1568-1696`) clamp the camera against that map window: the
//! leftmost `MapWindowDrawBounds.ulx` is 0, i.e. the first cell starts at the very
//! left edge of the map window.
//!
//! The port stored the camera in the 640×480 frame, so the clamp stopped 96 px
//! short (`(640 − 448) / 2`): cell 001-001 never appeared in the map window — it
//! looked like it was under the left panel.

use std::path::{Path, PathBuf};

use max_app::{App, AppConfig};
use max_core::geo::MAP_WIDTH;
use max_render::layout::MAIN_MAP;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn app_with_chrome() -> App {
    let data_dir = repo_path("max-original");
    let config = AppConfig::default()
        .with_data_dir(&data_dir)
        .with_assets_dir(repo_path("tests/fixtures/assets"));
    let mut app = App::new(config);
    app.init_assets().expect("the real game assets are present");
    assert!(
        app.chrome_active(),
        "the default 640×480 frame with FRAMEPIC renders the 1:1 chrome"
    );
    app
}

#[test]
fn the_first_cell_is_visible_at_the_left_top_edge_of_the_map_window() {
    let mut app = app_with_chrome();

    // Scroll as far left/top as the camera goes.
    app.viewport_mut().scroll_by_screen(-100_000, -100_000);

    let map = app.map_viewport();

    assert_eq!(
        map.origin(),
        (0, 0),
        "the camera must reach the map origin: world (0,0) at the map window corner"
    );
    assert_eq!(
        app.viewport().center(),
        (224, 224),
        "the clamp is the 448×448 map window (`half of the view`), not the 640×480 frame"
    );

    let first = map.tile_rect(0, 0);
    assert_eq!(
        (first.ulx, first.uly),
        (0, 0),
        "cell 001-001 starts at the top-left corner of the map buffer"
    );
    assert!(
        MAIN_MAP.contains(MAIN_MAP.ulx + first.ulx, MAIN_MAP.uly + first.uly),
        "the cell's top-left corner is inside WINDOW_MAIN_MAP (not under the left panel)"
    );
    assert!(
        MAIN_MAP.contains(
            MAIN_MAP.ulx + first.lrx() - 1,
            MAIN_MAP.uly + first.lry() - 1
        ),
        "the whole 64×64 cell is inside WINDOW_MAIN_MAP"
    );
    assert_eq!(first.width, 64, "1:1 zoom: the cell is 64 px wide");
    assert_eq!(first.height, 64);
}

#[test]
fn the_last_cell_ends_flush_with_the_map_window() {
    let mut app = app_with_chrome();

    app.viewport_mut().scroll_by_screen(100_000, 100_000);

    let map = app.map_viewport();
    let edge = i32::from(MAP_WIDTH) * 64;

    assert_eq!(
        map.origin(),
        (edge - 448, edge - 448),
        "at the bottom-right the map window shows the last 7 cells"
    );

    let last = map.tile_rect(MAP_WIDTH - 1, MAP_WIDTH - 1);
    assert_eq!(
        (last.lrx(), last.lry()),
        (448, 448),
        "the last cell ends exactly at the right/bottom edge of the map window"
    );
    assert!(MAIN_MAP.contains(MAIN_MAP.ulx + last.lrx() - 1, MAIN_MAP.uly + last.lry() - 1));
}
