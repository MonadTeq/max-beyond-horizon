use super::*;

/// A `side × side` tile whose texel `(x, y)` encodes its coordinates:
/// `R = x`, `G = y`, `B = x + y`, `A = 255`.
fn coordinate_tile(side: u32) -> HdTile {
    let mut rgba = Vec::with_capacity(side as usize * side as usize * 4);

    for y in 0..side {
        for x in 0..side {
            rgba.extend_from_slice(&[x as u8, y as u8, (x + y) as u8, 255]);
        }
    }

    HdTile::from_rgba(side, side, rgba).expect("coordinate tile")
}

#[test]
fn hd_tile_rejects_degenerate_textures() {
    assert!(HdTile::from_rgba(0, 0, Vec::new()).is_none(), "empty");
    assert!(
        HdTile::from_rgba(2, 3, vec![0; 2 * 3 * 4]).is_none(),
        "terrain tiles are square"
    );
    assert!(
        HdTile::from_rgba(2, 2, vec![0; 2 * 2 * 4 - 1]).is_none(),
        "truncated RGBA"
    );
    assert!(HdTile::from_rgba(2, 2, vec![0; 2 * 2 * 4]).is_some());
}

#[test]
fn hd_tile_reads_texels_with_alpha() {
    let tile = HdTile::from_rgba(
        2,
        2,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    )
    .unwrap();

    assert_eq!(tile.side(), 2);
    assert_eq!(tile.texel(0, 0), Some(Rgba::from_rgba([1, 2, 3, 4])));
    assert_eq!(tile.texel(1, 1), Some(Rgba::from_rgba([13, 14, 15, 16])));
    assert_eq!(tile.texel(2, 0), None);
    assert_eq!(tile.texel(0, 2), None);
}

#[test]
fn equal_block_sampling_uses_the_hd_source_dimensions() {
    let tile = coordinate_tile(4);
    let mut dst = RgbaBuffer::new(8, 8);

    assert_eq!(
        render_hd_tile_into(&mut dst, &tile, PixelRect::new(0, 0, 8, 8)),
        64,
        "every destination pixel is painted"
    );

    // local * 4 / 8: 0,0,1,1,2,2,3,3 on both axes.
    for y in 0..8usize {
        for x in 0..8usize {
            let expected = Rgba::rgb((x / 2) as u8, (y / 2) as u8, (x / 2 + y / 2) as u8);
            assert_eq!(dst.get(x as i32, y as i32), Some(expected), "({x}, {y})");
        }
    }
}

#[test]
fn equal_block_sampling_scales_between_the_zooms() {
    let tile = coordinate_tile(16);

    // minification 16 -> 8: every other texel.
    let mut small = RgbaBuffer::new(8, 8);
    assert_eq!(
        render_hd_tile_into(&mut small, &tile, PixelRect::new(0, 0, 8, 8)),
        64
    );
    assert_eq!(small.get(0, 0), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(small.get(7, 7), Some(Rgba::rgb(14, 14, 28)));

    // magnification 16 -> 32: every texel becomes a 2x2 block.
    let mut large = RgbaBuffer::new(32, 32);
    assert_eq!(
        render_hd_tile_into(&mut large, &tile, PixelRect::new(0, 0, 32, 32)),
        32 * 32
    );
    assert_eq!(large.get(0, 0), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(large.get(1, 1), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(large.get(2, 2), Some(Rgba::rgb(1, 1, 2)));
    assert_eq!(large.get(31, 31), Some(Rgba::rgb(15, 15, 30)));
}

#[test]
fn cpp_sampling_of_the_hd_source_reaches_the_corners() {
    let tile = coordinate_tile(8);

    let mut identity = RgbaBuffer::new(8, 8);
    assert_eq!(
        render_hd_tile_cpp_into(&mut identity, &tile, PixelRect::new(0, 0, 8, 8)),
        64
    );
    for y in 0..8i32 {
        for x in 0..8i32 {
            assert_eq!(
                identity.get(x, y),
                tile.texel(x as u32, y as u32),
                "1:1 HD sampling must be the identity ({x}, {y})"
            );
        }
    }

    let mut corners = RgbaBuffer::new(4, 4);
    assert_eq!(
        render_hd_tile_cpp_into(&mut corners, &tile, PixelRect::new(0, 0, 4, 4)),
        16
    );
    // `((8-1)<<16)/(4-1)+8` -> source offsets 0, 2, 4, 7.
    assert_eq!(corners.get(0, 0), tile.texel(0, 0));
    assert_eq!(corners.get(3, 0), tile.texel(7, 0));
    assert_eq!(corners.get(0, 3), tile.texel(0, 7));
    assert_eq!(corners.get(3, 3), tile.texel(7, 7));
}

#[test]
fn sampling_clips_to_the_destination_but_counts_painted_pixels() {
    let tile = coordinate_tile(4);
    let mut dst = RgbaBuffer::new(4, 4);

    // The rectangle starts at (2, 2) and extends past the buffer: only 4x4
    // pixels are visible (2..4 on both axes), the rest is clipped.
    let painted = render_hd_tile_into(&mut dst, &tile, PixelRect::new(2, 2, 6, 6));

    assert_eq!(painted, 4);
    assert_eq!(dst.get(2, 2), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(dst.get(3, 3), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(dst.get(0, 0), Some(Rgba::TRANSPARENT), "nothing above-left");
}
