//! Integration tests of the R3 HD map tiles through the public API
//! (`max-brh-docs/development/test-organization.md`: unit → `src/`,
//! integration → `tests/`).
//!
//! The terrain bank comes from a real `.WRL` fixture (`max_assets/tests/data`),
//! the HD source is a synthetic exact `×S` nearest upscale of a bank tile, and
//! the loader test resolves a real `.ktx2` through R1's
//! [`RemasterCache`](max_assets::remaster::RemasterCache) with the R3 tag
//! convention.
//!
//! Properties checked here:
//!
//! 1. with no HD assets the atlas is **byte-identical** to the original
//!    [`TerrainAtlas`] at every zoom (`8..=128`) — the per-asset fallback;
//! 2. an HD tile is sampled with exactly the documented coordinate mapping
//!    (equal-block and `Gfx_DecodeMapTile`), at every zoom;
//! 3. substitution is per tile: one cell comes from HD, its neighbour from
//!    `MAX.RES`/`.WRL`.

use std::path::{Path, PathBuf};

use max_assets::remaster::{terrain_tile_tag, RemasterCache, RemasterManifest};
use max_assets::WrlMap;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::hd_terrain::{HdTerrainAtlas, HdTile};
use max_render::render_scale::RenderScale;
use max_render::terrain::{cpp_tile_sample_offsets_for, TerrainAtlas};
use max_render::viewport::Viewport;

/// Zooms the gate requires (`Gfx_ZoomLevel` 8..128).
const ZOOMS: [u32; 6] = [8, 16, 32, 64, 96, 128];

fn assets_data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("max_assets")
        .join("tests")
        .join("data")
        .join(name)
}

fn remaster_root() -> PathBuf {
    assets_data("remaster")
}

fn terrain_atlas() -> TerrainAtlas {
    let map = WrlMap::open(assets_data("CRATER_3.WRL")).expect("WRL fixture");
    TerrainAtlas::from_wrl(&map)
}

/// The first two distinct tile ids used by the map, with one cell each.
fn two_used_tiles(atlas: &TerrainAtlas) -> ((u16, (i16, i16)), (u16, (i16, i16))) {
    let (width, height) = atlas.size();
    let mut first: Option<(u16, (i16, i16))> = None;

    for grid_y in 0..height {
        for grid_x in 0..width {
            let Some(tile_id) = atlas.tile_id_at(grid_x, grid_y) else {
                continue;
            };

            match first {
                None => first = Some((tile_id, (grid_x, grid_y))),
                Some((first_id, _)) if tile_id != first_id => {
                    return (first.expect("first cell"), (tile_id, (grid_x, grid_y)));
                }
                Some(_) => {}
            }
        }
    }

    panic!("the WRL fixture must use at least two distinct tiles");
}

/// Exact `factor ×` nearest upscale of a bank tile (the shape of an R3 asset
/// before the R2 encoder).
fn upscale_tile(atlas: &TerrainAtlas, tile_id: u16, factor: u32) -> HdTile {
    let side = 64 * factor;
    let mut rgba = Vec::with_capacity(side as usize * side as usize * 4);

    for y in 0..side {
        for x in 0..side {
            let color = atlas
                .tile_pixel(tile_id, x / factor, y / factor)
                .expect("tile texel");
            rgba.extend_from_slice(&color.to_rgba());
        }
    }

    HdTile::from_rgba(side, side, rgba).expect("square HD tile")
}

/// Reference equal-block sampler: `local · source / rect`.
fn reference_equal_block(tile: &HdTile, width: i32, height: i32) -> RgbaBuffer {
    let mut buffer = RgbaBuffer::new(width as u32, height as u32);

    for local_y in 0..height {
        let source_y = (local_y as u32 * tile.height() / height as u32) as u32;

        for local_x in 0..width {
            let source_x = (local_x as u32 * tile.width() / width as u32) as u32;
            let color = tile.texel(source_x, source_y).expect("source texel");
            buffer.set(local_x, local_y, color);
        }
    }

    buffer
}

/// Reference `Gfx_DecodeMapTile` sampler (the C++ edge-stretch formula).
fn reference_cpp(tile: &HdTile, width: i32, height: i32) -> RgbaBuffer {
    let mut buffer = RgbaBuffer::new(width as u32, height as u32);
    let columns = cpp_tile_sample_offsets_for(tile.width() as usize, width);
    let rows = cpp_tile_sample_offsets_for(tile.height() as usize, height);

    for (local_y, source_y) in rows.iter().enumerate() {
        for (local_x, source_x) in columns.iter().enumerate() {
            let color = tile
                .texel(*source_x as u32, *source_y as u32)
                .expect("source texel");
            buffer.set(local_x as i32, local_y as i32, color);
        }
    }

    buffer
}

fn atlas_with(source: TerrainAtlas, scale: RenderScale, tile: (u16, HdTile)) -> HdTerrainAtlas {
    let mut tiles = vec![None; source.tile_count() as usize];
    tiles[tile.0 as usize] = Some(tile.1);

    HdTerrainAtlas::with_tiles(source, scale, tiles)
}

// ---------------------------------------------------------------------------
// 1. Per-asset fallback: no HD asset -> the original sampler, byte for byte
// ---------------------------------------------------------------------------

#[test]
fn fallback_without_hd_assets_is_byte_identical_at_every_zoom() {
    let source = terrain_atlas();

    for render_scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let atlas = HdTerrainAtlas::new(source.clone(), render_scale);
        assert_eq!(atlas.hd_tile_count(), 0);

        let (tile_id, grid) = two_used_tiles(&source).0;

        for zoom in ZOOMS {
            let side = zoom as i32 * render_scale.scale() as i32;
            let rect = PixelRect::new(0, 0, side, side);
            let mut hd_frame = RgbaBuffer::filled(side as u32, side as u32, Rgba::BLACK);
            let mut original_frame = RgbaBuffer::filled(side as u32, side as u32, Rgba::BLACK);

            let hd_painted = atlas.render_tile_into(&mut hd_frame, grid, rect);
            let original_painted = source.render_tile_into(&mut original_frame, grid, rect);

            assert_eq!(hd_painted, original_painted, "tile {tile_id}, zoom {zoom}");
            assert_eq!(
                hd_frame, original_frame,
                "the fallback must reproduce the original tile exactly (tile {tile_id}, zoom {zoom}, {render_scale:?})"
            );

            // The C++ landing-screen sampler follows the same fallback.
            let mut hd_cpp = RgbaBuffer::filled(side as u32, side as u32, Rgba::BLACK);
            let mut original_cpp = RgbaBuffer::filled(side as u32, side as u32, Rgba::BLACK);
            atlas.render_tile_cpp_into(&mut hd_cpp, grid, rect);
            source.render_tile_cpp_into(&mut original_cpp, grid, rect);

            assert_eq!(
                hd_cpp, original_cpp,
                "the C++ sampler fallback must be identical (tile {tile_id}, zoom {zoom})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. HD sampling: the same mapping, the HD source dimensions
// ---------------------------------------------------------------------------

#[test]
fn hd_sampling_matches_the_reference_at_every_zoom() {
    let source = terrain_atlas();
    let (tile_id, grid) = two_used_tiles(&source).0;

    for render_scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let factor = render_scale.scale();
        let hd_tile = upscale_tile(&source, tile_id, factor);
        let atlas = atlas_with(source.clone(), render_scale, (tile_id, hd_tile.clone()));

        for zoom in ZOOMS {
            let side = zoom as i32 * factor as i32;
            let rect = PixelRect::new(0, 0, side, side);
            let mut frame = RgbaBuffer::filled(side as u32, side as u32, Rgba::BLACK);

            assert_eq!(
                atlas.render_tile_into(&mut frame, grid, rect),
                (side * side) as u32,
                "every pixel of the HD rect must be painted (zoom {zoom})"
            );
            assert_eq!(
                frame,
                reference_equal_block(&hd_tile, side, side),
                "HD equal-block sampling must match the documented formula (zoom {zoom}, {render_scale:?})"
            );

            let mut cpp_frame = RgbaBuffer::filled(side as u32, side as u32, Rgba::BLACK);
            assert_eq!(
                atlas.render_tile_cpp_into(&mut cpp_frame, grid, rect),
                (side * side) as u32
            );
            assert_eq!(
                cpp_frame,
                reference_cpp(&hd_tile, side, side),
                "HD Gfx_DecodeMapTile sampling must match the oracle formula (zoom {zoom})"
            );
        }
    }
}

#[test]
fn one_to_one_hd_sampling_is_the_identity() {
    // At zoom 64 with S = 2 the tile covers 128x128 render pixels and the HD
    // source is 128x128: every texel is drawn exactly once.
    let source = terrain_atlas();
    let (tile_id, grid) = two_used_tiles(&source).0;
    let hd_tile = upscale_tile(&source, tile_id, 2);
    let atlas = atlas_with(source, RenderScale::Hd2, (tile_id, hd_tile.clone()));
    let mut frame = RgbaBuffer::new(128, 128);

    atlas.render_tile_into(&mut frame, grid, PixelRect::new(0, 0, 128, 128));

    for y in 0..128u32 {
        for x in 0..128u32 {
            assert_eq!(
                frame.get(x as i32, y as i32),
                hd_tile.texel(x, y),
                "({x}, {y})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Substitution is per asset, not all-or-nothing
// ---------------------------------------------------------------------------

#[test]
fn substitution_is_per_asset() {
    let source = terrain_atlas();
    let ((hd_id, hd_cell), (plain_id, plain_cell)) = two_used_tiles(&source);
    let hd_tile = upscale_tile(&source, hd_id, 2);
    let atlas = atlas_with(source.clone(), RenderScale::Hd2, (hd_id, hd_tile.clone()));

    assert_eq!(atlas.hd_tile_count(), 1);
    assert!(atlas.hd_tile(hd_id).is_some());
    assert!(atlas.hd_tile(plain_id).is_none());

    let rect = PixelRect::new(0, 0, 128, 128);
    let mut hd_frame = RgbaBuffer::new(128, 128);
    let mut fallback_frame = RgbaBuffer::new(128, 128);

    atlas.render_tile_into(&mut hd_frame, hd_cell, rect);
    atlas.render_tile_into(&mut fallback_frame, plain_cell, rect);

    assert_eq!(
        hd_frame,
        reference_equal_block(&hd_tile, 128, 128),
        "the tile with an asset comes from HD"
    );

    let mut original = RgbaBuffer::new(128, 128);
    source.render_tile_into(&mut original, plain_cell, rect);
    assert_eq!(
        fallback_frame, original,
        "the tile without an asset keeps the original pixels"
    );
    assert_ne!(hd_frame, fallback_frame);
}

#[test]
fn render_into_draws_the_visible_map_at_hd_resolution() {
    let source = terrain_atlas();
    let ((hd_id, _), _) = two_used_tiles(&source);
    let atlas = atlas_with(
        source.clone(),
        RenderScale::Hd2,
        (hd_id, upscale_tile(&source, hd_id, 2)),
    );

    // 1280x960 render frame, zoom 128: the same world area as a 640x480 frame
    // at zoom 64, but with the HD tile source.
    let viewport = Viewport::with_center(1280, 960, (0, 0), 128);
    let mut frame = RgbaBuffer::filled(1280, 960, Rgba::BLACK);

    let cells = atlas.render_into(&mut frame, &viewport);

    assert!(cells > 0, "the visible map must be drawn");
    assert!(
        frame
            .pixels()
            .chunks_exact(4)
            .any(|pixel| pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0),
        "the HD atlas must paint pixels, not only count cells"
    );
}

// ---------------------------------------------------------------------------
// 4. Loading through R1: manifest tag -> KTX2 -> RGBA8, per-asset fallback
// ---------------------------------------------------------------------------

fn cache_for(tiles: &[(u16, &str)]) -> RemasterCache {
    let mut assets = String::new();

    for (index, (tile_id, file)) in tiles.iter().enumerate() {
        if index > 0 {
            assets.push(',');
        }

        assets.push_str(&format!(
            r#""{}":{{"path":"{file}","scale":2}}"#,
            terrain_tile_tag("GREEN_1", *tile_id, 2)
        ));
    }

    let manifest = RemasterManifest::parse(&format!(r#"{{"version":1,"assets":{{{assets}}}}}"#))
        .expect("test manifest");

    RemasterCache::with_manifest(remaster_root(), manifest)
}

#[test]
fn load_resolves_r1_assets_and_falls_back_per_asset() {
    let source = terrain_atlas();
    let ((hd_id, hd_cell), (plain_id, plain_cell)) = two_used_tiles(&source);
    // The fixture `.ktx2` is a real 64x64 tile; the second tag is listed but
    // points at a missing file, so it must fall back too.
    let cache = cache_for(&[
        (hd_id, "terrain_tile_ETC1S.ktx2"),
        (plain_id, "does_not_exist.ktx2"),
    ]);
    let atlas = HdTerrainAtlas::load(&cache, "GREEN_1", source.clone(), RenderScale::Hd2);

    assert_eq!(atlas.hd_tile_count(), 1, "only the existing .ktx2 resolves");
    assert_eq!(atlas.hd_tile(hd_id).map(HdTile::side), Some(64));
    assert!(
        atlas.hd_tile(plain_id).is_none(),
        "missing file must fall back"
    );

    let rect = PixelRect::new(0, 0, 128, 128);
    let mut hd_frame = RgbaBuffer::new(128, 128);
    let mut fallback_frame = RgbaBuffer::new(128, 128);

    atlas.render_tile_into(&mut hd_frame, hd_cell, rect);
    atlas.render_tile_into(&mut fallback_frame, plain_cell, rect);

    let loaded = atlas.hd_tile(hd_id).expect("resolved HD tile");
    assert_eq!(hd_frame, reference_equal_block(loaded, 128, 128));

    let mut original = RgbaBuffer::new(128, 128);
    source.render_tile_into(&mut original, plain_cell, rect);
    assert_eq!(fallback_frame, original);
}

#[test]
fn load_without_a_manifest_is_a_full_fallback() {
    let source = terrain_atlas();
    let cache = RemasterCache::open("__no_such_remaster_root__");
    let atlas = HdTerrainAtlas::load(&cache, "GREEN_1", source.clone(), RenderScale::Hd4);

    assert_eq!(atlas.hd_tile_count(), 0);
    assert_eq!(atlas.tile_count(), source.tile_count());

    let (_, grid) = two_used_tiles(&source).0;
    let rect = PixelRect::new(0, 0, 256, 256);
    let mut hd_frame = RgbaBuffer::new(256, 256);
    let mut original = RgbaBuffer::new(256, 256);
    atlas.render_tile_into(&mut hd_frame, grid, rect);
    source.render_tile_into(&mut original, grid, rect);

    assert_eq!(hd_frame, original);
}
