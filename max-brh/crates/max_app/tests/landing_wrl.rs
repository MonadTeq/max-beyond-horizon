//! The landing screen draws the **real** `.WRL` tiles, not flat `TileStyle` colors.
//! `TileStyle`.
//!
//! Oracle — `DrawMap_RenderMapTiles`/`Gfx_DecodeMapTile` (`gfx.cpp:125-210`):
//! at zoom 4 each 64×64 tile is reduced to 4×4 pixels sampled at
//! `0, 21, 42, 63`. The test compares the frame with `WrlMap`/`TerrainAtlas` directly
//! (`tile_pixel`), without calling the screen render function.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use max_app::landing::{LandingScreen, LANDING_AREA, LANDING_CELL};
use max_app::menu::MenuAssets;
use max_core::team::TeamId;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::terrain::{cpp_tile_sample_offsets, TerrainAtlas};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn data_dir() -> Option<PathBuf> {
    let dir = repo_path("max-original");

    if dir.join("SNOW_1.WRL").is_file() && dir.join("MAX.RES").is_file() {
        Some(dir)
    } else {
        None
    }
}

#[test]
fn landing_frame_contains_real_wrl_tiles() {
    let Some(data) = data_dir() else {
        eprintln!("landing_wrl: пропуск — нет max-original/SNOW_1.WRL + MAX.RES");
        return;
    };

    let landing = LandingScreen::load(TeamId::RED, 0, &data).expect("SNOW_1.WRL");
    assert!(landing.terrain.is_some(), "тайлы загружены");
    assert!(!landing.terrain.as_ref().unwrap().is_empty());

    let archive = max_assets::ResArchive::open(data.join("MAX.RES")).expect("MAX.RES");
    let menu = MenuAssets::load(&archive).expect("MenuAssets");
    let cursors = max_assets::CursorSet::load(&archive);

    let mut frame = RgbaBuffer::filled(640, 480, Rgba::BLACK);
    let painted = landing.draw(&mut frame, &menu, None, None, "", None);

    assert!(painted > 0, "кадр нарисован");

    // 1. A flat `TileStyle` map would give single-digit color counts; the real tiles —
    //    dozens of `.WRL` palette shades.
    let mut colors = HashSet::new();

    for y in LANDING_AREA.uly..LANDING_AREA.lry() {
        for x in LANDING_AREA.ulx..LANDING_AREA.lrx() {
            if let Some(color) = frame.get(x, y) {
                colors.insert(color.to_rgba());
            }
        }
    }

    assert!(
        colors.len() > 40,
        "в области карты {} цветов — похоже на плоскую заливку TileStyle",
        colors.len()
    );

    // 2. The pixels of specific cells match the `Gfx_DecodeMapTile` sampling
    //    from the raw `.WRL` (palette + tile_id).
    let wrl = max_assets::wrl::WrlMap::open(data.join("SNOW_1.WRL")).expect("WRL");
    let atlas = TerrainAtlas::from_wrl(&wrl);
    let offsets = cpp_tile_sample_offsets(LANDING_CELL);

    // Cells outside the message window/markers/circle.
    for (grid_x, grid_y) in [(100i16, 100i16), (60, 90), (90, 30), (5, 90)] {
        let tile_id = atlas.tile_id_at(grid_x, grid_y).expect("tile id");

        for (local_y, source_y) in offsets.iter().enumerate() {
            for (local_x, source_x) in offsets.iter().enumerate() {
                let expected = atlas
                    .tile_pixel(tile_id, *source_x as u32, *source_y as u32)
                    .expect("пиксель тайла");
                let actual = frame.get(
                    LANDING_AREA.ulx + i32::from(grid_x) * LANDING_CELL + local_x as i32,
                    LANDING_AREA.uly + i32::from(grid_y) * LANDING_CELL + local_y as i32,
                );

                assert_eq!(
                    actual,
                    Some(expected),
                    "клетка ({grid_x},{grid_y}) пиксель ({local_x},{local_y})"
                );
            }
        }
    }

    // 3. The C++ cursor (`Cursor_RenderToBuffer`) gets into the frame: a frame with the mouse
    //    over a valid cell differs from a frame without the cursor.
    let (mouse_x, mouse_y) = (
        LANDING_AREA.ulx + i32::from(landing.cursor.x) * LANDING_CELL + LANDING_CELL / 2,
        LANDING_AREA.uly + i32::from(landing.cursor.y) * LANDING_CELL + LANDING_CELL / 2,
    );
    let mut with_cursor = RgbaBuffer::filled(640, 480, Rgba::BLACK);
    landing.draw(
        &mut with_cursor,
        &menu,
        Some(&cursors),
        None,
        "",
        Some((mouse_x, mouse_y)),
    );

    let mut changed = 0usize;
    for y in (mouse_y - 16)..(mouse_y + 16) {
        for x in (mouse_x - 16)..(mouse_x + 16) {
            if with_cursor.get(x, y) != frame.get(x, y) {
                changed += 1;
            }
        }
    }

    assert!(changed > 0, "CURSOR_UNIT_GO нарисован в кадре");
}

#[test]
fn landing_without_wrl_falls_back_to_flat_map() {
    // Without tiles (headless tests) the documented fallback remains: the frame
    // is still drawn and does not panic.
    let surface = max_core::map::SurfaceMap::from_tiles(
        [max_core::enums::Surface::LAND; max_core::geo::MAP_TILES],
    );
    let landing = LandingScreen::new(TeamId::BLUE, 0, surface);
    let archive = match data_dir() {
        Some(data) => max_assets::ResArchive::open(data.join("MAX.RES")).ok(),
        None => None,
    };
    let Some(menu) = archive.as_ref().and_then(MenuAssets::load) else {
        return;
    };
    let mut frame = RgbaBuffer::filled(640, 480, Rgba::BLACK);

    assert!(landing.draw(&mut frame, &menu, None, None, "", None) > 0);
    assert!(LANDING_AREA
        .intersect(PixelRect::new(0, 0, 640, 480))
        .is_some());
}
