use super::*;

/// Screen 512×384 at scale 64: half the screen is (256, 192) world
/// pixels, so the center of the map (3584, 3584) gives an exact cell corner
/// (3328, 3392) = (52, 53).
fn test_viewport() -> Viewport {
    Viewport::new(512, 384)
}

#[test]
fn default_viewport_shows_the_map_centre_at_native_zoom() {
    let viewport = test_viewport();

    assert_eq!(viewport.screen(), (512, 384));
    assert_eq!(viewport.zoom(), DEFAULT_ZOOM);
    assert!(viewport.is_native_zoom());
    assert_eq!(viewport.tile_pixels(), 64);
    // Center of the map 112*64/2 = 3584.
    assert_eq!(viewport.center(), (3584, 3584));
    assert_eq!(viewport.origin(), (3328, 3392));
    assert_eq!(viewport.grid_to_screen(52, 53), (0, 0));
    assert_eq!(viewport.grid_to_screen(53, 53), (64, 0));

    // Cells 52..=59 in x and 53..=58 in y are visible (8 × 6 = 48 cells).
    let range = viewport.visible_tiles().unwrap();

    assert_eq!((range.x0, range.y0, range.x1, range.y1), (52, 53, 59, 58));
    assert_eq!(range.count(), 48);
    assert!(viewport.is_tile_visible(52, 53));
    assert!(!viewport.is_tile_visible(51, 53));
    assert!(!viewport.is_tile_visible(52, 59));
}

#[test]
fn zoom_scales_tiles_and_changes_the_visible_range() {
    let mut viewport = test_viewport();

    assert_eq!(viewport.zoom_out(), 32);
    assert_eq!(viewport.tile_pixels(), 32);
    // At zoom 32 the frame fits 512/32 = 16 cells in x and 384/32 = 12 in y.
    let range = viewport.visible_tiles().unwrap();

    assert_eq!(range.width(), 16);
    assert_eq!(range.height(), 12);
    assert_eq!((range.x0, range.y0), (48, 50));

    assert_eq!(viewport.zoom_in(), 64);
    assert_eq!(viewport.zoom_in(), 128);
    assert_eq!(viewport.zoom_in(), MAX_ZOOM, "масштаб клампится сверху");
    assert_eq!(viewport.zoom_out(), 64);
    assert_eq!(viewport.set_zoom(1), MIN_ZOOM, "масштаб клампится снизу");
    assert_eq!(viewport.set_zoom(1000), MAX_ZOOM);
}

#[test]
fn tile_rects_and_screen_to_grid_roundtrip() {
    let viewport = test_viewport();

    let rect = viewport.tile_rect(52, 53);

    assert_eq!(rect, PixelRect::new(0, 0, 64, 64));
    assert_eq!(viewport.screen_to_grid(0, 0), (52, 53));
    assert_eq!(viewport.screen_to_grid(63, 63), (52, 53));
    assert_eq!(viewport.screen_to_grid(64, 64), (53, 54));
    assert!(viewport.is_rect_visible(rect));

    // A cell beyond the right edge of the screen is not visible.
    assert_eq!(viewport.tile_rect(60, 53), PixelRect::new(512, 0, 64, 64));
    assert!(!viewport.is_rect_visible(viewport.tile_rect(60, 53)));
    assert!(!viewport.is_rect_visible(PixelRect::new(1000, 1000, 64, 64)));
    assert!(!viewport.is_tile_visible(10, 10));
}

#[test]
fn camera_clamps_to_the_map() {
    let mut viewport = test_viewport();

    viewport.center_on_grid(0, 0);
    // Half the screen is (256, 192) world pixels, no closer to the edge allowed.
    assert_eq!(viewport.origin(), (0, 0));
    assert_eq!(viewport.center(), (256, 192));

    viewport.center_on_grid(MAP_WIDTH - 1, MAP_HEIGHT - 1);
    let range = viewport.visible_tiles().unwrap();

    assert_eq!(range.x1, MAP_WIDTH - 1);
    assert_eq!(range.y1, MAP_HEIGHT - 1);

    // A shift beyond the map is clamped too.
    viewport.scroll_by_screen(-100_000, -100_000);
    assert_eq!(viewport.origin(), (0, 0));
}

#[test]
fn scroll_by_screen_moves_the_origin_by_the_delta() {
    let mut viewport = test_viewport();
    let before = viewport.origin();

    viewport.scroll_by_screen(64, 32);

    assert_eq!(viewport.origin(), (before.0 + 64, before.1 + 32));
    assert_eq!(viewport.center(), (3584 + 64, 3584 + 32));

    // At zoom 32 one screen pixel is two world pixels.
    viewport.set_zoom(32);
    let before = viewport.origin();

    viewport.scroll_by_screen(32, 0);

    assert_eq!(viewport.origin(), (before.0 + 64, before.1));
}

#[test]
fn draw_order_is_by_ground_then_x() {
    let units = [(10, 5), (3, 9), (7, 5), (1, 9), (0, 1)];

    // y = 1: (0,1); y = 5: (7,5) and (10,5); y = 9: (1,9) and (3,9).
    assert_eq!(draw_order(&units), vec![4, 2, 0, 3, 1]);
}

#[test]
fn visible_units_filters_and_orders() {
    let viewport = test_viewport();

    // Cells 52..=59 x 53..=58 are visible.
    let units = [
        (55, 55), // visible
        (10, 10), // not visible
        (52, 57), // visible, below
        (56, 54), // visible, above
        (52, 53), // visible (corner)
        (60, 55), // beyond the right edge
    ];

    let visible = viewport.visible_units(&units, 0);

    assert_eq!(visible, vec![4, 3, 0, 2]);
    // With a one-cell margin the unit at x = 60 appears (same y, further along x).
    assert_eq!(viewport.visible_units(&units, 1), vec![4, 3, 0, 5, 2]);
}

#[test]
fn tile_range_iterates_row_major() {
    let range = TileRange::new(2, 3, 3, 4).unwrap();
    let cells: Vec<(i16, i16)> = range.cells().collect();

    assert_eq!(cells, vec![(2, 3), (3, 3), (2, 4), (3, 4)]);
    assert_eq!(range.width(), 2);
    assert_eq!(range.height(), 2);
    assert!(TileRange::new(3, 3, 2, 4).is_none());
}

#[test]
fn grid_lines_close_the_playable_area() {
    // `WINDOW_MAIN_MAP` is 448×448 — exactly 7×7 cells; the next cell boundary
    // (448) is one pixel past the buffer. The camera is cell-aligned here.
    let viewport = Viewport::with_center(448, 448, (3616, 3616), 64);
    let lines = viewport.grid_lines(448, 448);

    assert!(!lines.is_empty());
    assert_eq!(lines.xs.first(), Some(&0), "left edge of the map window");
    assert_eq!(lines.xs.last(), Some(&447), "the last column is closed");
    assert_eq!(lines.ys.last(), Some(&447), "the last row is closed");
    assert_eq!(
        &lines.xs[..lines.xs.len() - 1],
        &[0, 64, 128, 192, 256, 320, 384],
            "7 cell boundaries of the 7×7 map window"
    );
    assert_eq!(
        &lines.ys[..lines.ys.len() - 1],
        &[0, 64, 128, 192, 256, 320, 384]
    );
    assert_eq!(lines.xs.len(), 8, "7 cell boundaries + the closing edge");
    assert_eq!(lines.ys.len(), 8);
    assert!(
        (447 - lines.xs[lines.xs.len() - 2]).abs() < 64,
            "the closing edge belongs to the last cell"
    );
}

#[test]
fn grid_lines_follow_the_viewport_edges_at_every_zoom() {
    for (width, height, zoom) in [
        (448i32, 448i32, 64u32),
        (448, 448, 128),
        (448, 448, 32),
        (512, 384, 48),
    ] {
        let viewport =
            Viewport::with_center(width as u32, height as u32, (3584, 3584), zoom);
        let lines = viewport.grid_lines(width, height);

        assert_eq!(
            lines.xs.first(),
            Some(&0),
                "zoom {zoom}: left edge is closed"
        );
        assert_eq!(
            lines.ys.first(),
            Some(&0),
                "zoom {zoom}: top edge is closed"
        );
        assert!(
            lines.xs.last() == Some(&(width - 1)),
                "zoom {zoom}: right edge is closed"
        );
        assert!(
            lines.ys.last() == Some(&(height - 1)),
                "zoom {zoom}: bottom edge is closed"
        );
        assert!(
            lines.xs.iter().all(|x| (0..width).contains(x)),
                "zoom {zoom}: columns stay inside the buffer: {:?}",
            lines.xs
        );
        assert!(
            lines.ys.iter().all(|y| (0..height).contains(y)),
                "zoom {zoom}: rows stay inside the buffer: {:?}",
            lines.ys
        );
        assert!(lines.xs.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(lines.ys.windows(2).all(|pair| pair[0] < pair[1]));
    }
}

/// Defect 7, second pass: when the camera is **not** cell-aligned, the first
/// cell boundary lies inside the map window. The previous fix closed only the
/// right/bottom edges, so the outer strip of the left/top border cells had no
/// line against the abutting UI panel and looked cut off. All four window
/// edges must be closed (`grid_lines`), with the interior boundaries kept.
#[test]
fn grid_lines_close_all_four_edges_for_a_non_aligned_camera() {
    // Map pixel 3520 is the first cell boundary at screen x = 12:
    // origin = 3732 - 224 = 3508, 3508 / 64 = 54.8125.
    let viewport = Viewport::with_center(448, 448, (3732, 3732), 64);
    let lines = viewport.grid_lines(448, 448);

    assert_eq!(lines.xs.first(), Some(&0), "left window edge is closed");
    assert_eq!(lines.ys.first(), Some(&0), "top window edge is closed");
    assert_eq!(lines.xs.last(), Some(&447), "right window edge is closed");
    assert_eq!(lines.ys.last(), Some(&447), "bottom window edge is closed");
    assert_eq!(
        &lines.xs[1..4],
        &[12, 76, 140],
            "interior cell boundaries are preserved after the closing edge"
    );
    assert_eq!(&lines.ys[1..4], &[12, 76, 140]);
    assert_eq!(
        lines.xs.len(),
        9,
            "closing edge + 7 boundaries + closing edge"
    );
}

#[test]
fn grid_lines_of_an_empty_viewport_are_empty() {
    let viewport = test_viewport();

    assert!(viewport.grid_lines(0, 480).is_empty());
    assert!(viewport.grid_lines(640, 0).is_empty());
    assert!(viewport.grid_lines(-1, -1).is_empty());
}
