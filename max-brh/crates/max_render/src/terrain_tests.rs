use super::*;

fn atlas_with_one_tile(tiles: Vec<u8>, palette: PaletteRgba) -> TerrainAtlas {
    TerrainAtlas {
        palette,
        tiles,
        tile_ids: vec![0],
        tile_count: 1,
        width: 1,
        height: 1,
    }
}

#[test]
fn palette_scales_six_bit_channels() {
    let mut raw = vec![0u8; PALETTE_COLORS * PALETTE_STRIDE];
    raw[0] = 63;
    raw[1] = 32;

    let palette = palette_from_wrl(&raw);

    assert_eq!(palette[0][0], 255, "63 → 255");
    assert_eq!(palette[0][1], 130, "32 → 32*255/63 ≈ 130");
    assert_eq!(palette[0][3], 255, "альфа всегда 255");
}

#[test]
fn palette_keeps_eight_bit_values() {
    let mut raw = vec![0u8; PALETTE_COLORS * PALETTE_STRIDE];
    raw[0] = 200;
    raw[1] = 120;
    raw[2] = 7;

    let palette = palette_from_wrl(&raw);

    assert_eq!(&palette[0][..3], &[200, 120, 7]);
}

#[test]
fn tile_pixels_use_palette() {
    let mut palette = [[0u8, 0, 0, 255]; PALETTE_COLORS];
    palette[3] = [10, 20, 30, 255];

    let mut tiles = vec![0u8; TILE_PIXELS];
    tiles[0] = 3; // (0,0) of the tile
    tiles[TILE_SIZE + 5] = 3; // (5,1)

    let atlas = atlas_with_one_tile(tiles, palette);

    assert_eq!(atlas.tile_pixel(0, 0, 0), Some(Rgba::rgb(10, 20, 30)));
    assert_eq!(atlas.tile_pixel(0, 5, 1), Some(Rgba::rgb(10, 20, 30)));
    assert_eq!(atlas.tile_pixel(0, 1, 0), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(atlas.tile_pixel(1, 0, 0), None, "нет такого тайла");
    assert_eq!(atlas.tile_pixel(0, 64, 0), None, "за пределами тайла");
}

#[test]
fn cpp_sampling_matches_gfx_decode_map_tile() {
    // `map_tile_zoom_factor = ((64-1)<<16)/(size-1) + 8`;
    // source = `(i*factor)>>16` (`gfx.cpp:128,196`).
    assert_eq!(cpp_tile_sample_offsets(4), vec![0, 21, 42, 63]);
    assert_eq!(
        cpp_tile_sample_offsets(8),
        vec![0, 9, 18, 27, 36, 45, 54, 63]
    );
    assert_eq!(
        cpp_tile_sample_offsets(64),
        (0..64).collect::<Vec<_>>(),
            "зум 1:1 — тождественная выборка"
    );
    assert_eq!(cpp_tile_sample_offsets(1), vec![0]);
}

#[test]
fn cpp_tile_render_samples_tile_corners() {
    let mut palette = [[0u8, 0, 0, 255]; PALETTE_COLORS];
    palette[1] = [10, 20, 30, 255];
    palette[2] = [40, 50, 60, 255];

    let mut tiles = vec![0u8; TILE_PIXELS];
    tiles[0] = 1; // (0,0)
    tiles[TILE_SIZE * TILE_SIZE - 1] = 2; // (63,63)

    let atlas = atlas_with_one_tile(tiles, palette);
    let mut frame = RgbaBuffer::new(4, 4);

    assert_eq!(
        atlas.render_tile_cpp_into(
            &mut frame,
            (0, 0),
            crate::buffer::PixelRect::new(0, 0, 4, 4)
        ),
        16
    );
    assert_eq!(frame.get(0, 0), Some(Rgba::rgb(10, 20, 30)));
    assert_eq!(frame.get(3, 3), Some(Rgba::rgb(40, 50, 60)));
}

#[test]
fn empty_atlas_draws_nothing() {
    let atlas = TerrainAtlas {
        palette: [[0u8, 0, 0, 255]; PALETTE_COLORS],
        tiles: Vec::new(),
        tile_ids: Vec::new(),
        tile_count: 0,
        width: 0,
        height: 0,
    };
    let mut frame = RgbaBuffer::new(8, 8);
    let viewport = Viewport::new(8, 8);

    assert!(atlas.is_empty());
    assert_eq!(atlas.render_into(&mut frame, &viewport), 0);
}
