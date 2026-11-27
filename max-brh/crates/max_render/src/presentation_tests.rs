use super::*;

#[test]
fn hundred_percent_dpi_is_the_identity_geometry() {
    let geometry = ScreenGeometry::native((1280, 960), (1280, 960));

    assert_eq!(geometry.pixel_ratio(), 1.0);
    assert_eq!(geometry.scale(), 2);
    assert_eq!(geometry.content_rect(), PixelRect::new(0, 0, 1280, 960));
    assert!(geometry.fills_output());
    assert_eq!(geometry.window_to_frame(640, 480), Some((320, 240)));
    // A click on the right/bottom edge is still inside the frame.
    assert_eq!(geometry.window_to_frame(1279, 959), Some((639, 479)));
}

#[test]
fn hundred_fifty_percent_dpi_maps_points_through_pixels() {
    // 640×480 points at 150% DPI -> 960×720 pixels. Integer scale of the
    // 640×480 frame is 1, centred: bars 160×120 px.
    let geometry = ScreenGeometry::native((640, 480), (960, 720));

    assert!((geometry.pixel_ratio() - 1.5).abs() < 1e-6);
    assert_eq!(geometry.scale(), 1);
    assert_eq!(geometry.content_rect(), PixelRect::new(160, 120, 640, 480));

    // Frame (0,0) is at point (160/1.5, 120/1.5) = (106.67, 80) -> (106, 80).
    assert_eq!(geometry.point_to_output(106, 80), (159, 120));
    assert_eq!(geometry.window_to_frame(107, 80), Some((0, 0)));
    // Points inside the bar at the left/top return None.
    assert_eq!(geometry.window_to_frame(0, 0), None);
    assert_eq!(geometry.window_to_frame(105, 80), None);
    // The centre of the window is the centre of the frame.
    assert_eq!(geometry.window_to_frame(320, 240), Some((320, 240)));
}

#[test]
fn two_hundred_percent_dpi_uses_the_integer_scale_in_pixels() {
    // 640×480 points at 200% -> 1280×960 px, scale 2 in pixels. A point is
    // two output pixels, so the frame point is computed from pixels.
    let geometry = ScreenGeometry::native((640, 480), (1280, 960));

    assert_eq!(geometry.scale(), 2);
    assert_eq!(geometry.content_rect(), PixelRect::new(0, 0, 1280, 960));
    assert_eq!(geometry.window_to_frame(0, 0), Some((0, 0)));
    assert_eq!(geometry.window_to_frame(319, 239), Some((319, 239)));
    assert_eq!(geometry.window_to_frame(639, 479), Some((639, 479)));
}

#[test]
fn uhd_and_full_hd_keep_the_frame_at_an_integer_scale() {
    for (points, pixels, scale) in [
        ((1920, 1080), (1920, 1080), 2u32),
        ((3840, 2160), (3840, 2160), 4u32),
        ((1366, 768), (1366, 768), 1u32),
        ((2560, 1440), (2560, 1440), 3u32),
    ] {
        let geometry = ScreenGeometry::native(points, pixels);
        assert_eq!(geometry.scale(), scale, "{points:?}");
        assert_eq!(geometry.content_rect().width, 640 * scale as i32);
        assert_eq!(geometry.content_rect().height, 480 * scale as i32);

        // At 100% DPI a window point is an output pixel: any pixel of the
        // 5×7 block maps to frame pixel (5, 7), so letterboxing never
        // shifts the picture.
        let (offset_x, offset_y) = geometry.frame_scale().offset();
        let scale = scale as i32;
        let x = offset_x + 5 * scale + scale - 1;
        let y = offset_y + 7 * scale + scale - 1;

        assert_eq!(geometry.window_to_frame(x, y), Some((5, 7)), "{points:?}");
    }
}

#[test]
fn letterbox_bars_reject_clicks_at_any_scale() {
    let geometry = ScreenGeometry::native((1024, 768), (1024, 768)); // scale 1, bars

    assert_eq!(geometry.scale(), 1);
    assert_eq!(
        geometry.content_rect(),
        PixelRect::new((1024 - 640) / 2, (768 - 480) / 2, 640, 480)
    );
    assert_eq!(geometry.window_to_frame(0, 0), None);
    assert_eq!(geometry.window_to_frame(191, 143), None);
    assert_eq!(geometry.window_to_frame(192, 144), Some((0, 0)));
    assert_eq!(geometry.window_to_frame(831, 623), Some((639, 479)));
    assert_eq!(geometry.window_to_frame(832, 144), None);
}

#[test]
fn cropped_window_keeps_the_centre_of_the_frame_visible() {
    // A window smaller than the frame: scale 1, negative offset, centre kept.
    let geometry = ScreenGeometry::native((320, 240), (320, 240));

    assert_eq!(geometry.scale(), 1);
    assert_eq!(geometry.frame_scale().offset(), (-160, -120));
    assert_eq!(geometry.window_to_frame(0, 0), Some((160, 120)));
    assert_eq!(geometry.window_to_frame(319, 239), Some((479, 359)));
}

#[test]
fn zero_sized_window_is_handled_without_panicking() {
    let geometry = ScreenGeometry::native((0, 0), (0, 0));

    assert_eq!(geometry.pixel_ratio(), 0.0);
    assert_eq!(geometry.scale(), 1);
    assert_eq!(geometry.window_to_frame(0, 0), None);
}

#[test]
fn hd2_renders_a_1280x960_frame_but_keeps_the_logical_frame() {
    // A 1280x960 point window with a 1280x960 drawable: the render frame is
    // exactly 1280x960, so the window-level integer scale is 1 and no letterbox
    // remains.
    let geometry =
        ScreenGeometry::native_with_render_scale((1280, 960), (1280, 960), RenderScale::Hd2);

    assert_eq!(geometry.render_scale(), RenderScale::Hd2);
    assert_eq!(geometry.frame_size(), (640, 480), "UI stays logical");
    assert_eq!(geometry.render_frame_size(), (1280, 960));
    assert_eq!(geometry.scale(), 1);
    assert_eq!(geometry.content_rect(), PixelRect::new(0, 0, 1280, 960));
    assert!(geometry.fills_output());

    // A point maps through the render frame and is divided by S.
    assert_eq!(geometry.window_to_frame(0, 0), Some((0, 0)));
    assert_eq!(geometry.window_to_frame(1279, 959), Some((639, 479)));
    // Every 2x2 render block belongs to the same logical pixel.
    assert_eq!(geometry.window_to_frame(11, 11), Some((5, 5)));
    assert_eq!(geometry.window_to_frame(13, 13), Some((6, 6)));
}

#[test]
fn hd4_renders_a_2560x1920_frame() {
    let geometry =
        ScreenGeometry::native_with_render_scale((2560, 1920), (2560, 1920), RenderScale::Hd4);

    assert_eq!(geometry.render_frame_size(), (2560, 1920));
    assert_eq!(geometry.scale(), 1);
    assert_eq!(geometry.content_rect(), PixelRect::new(0, 0, 2560, 1920));
    assert_eq!(geometry.window_to_frame(0, 0), Some((0, 0)));
    assert_eq!(geometry.window_to_frame(2559, 1919), Some((639, 479)));
    assert_eq!(
        geometry.window_to_frame(4095, 4095),
        None,
        "outside the frame"
    );
}

#[test]
fn hd_letterbox_still_rejects_clicks() {
    // A 1920x1080 drawable fits the 1280x960 render frame at scale 1, centred:
    // 320 px bars left/right and 60 px top/bottom.
    let geometry =
        ScreenGeometry::native_with_render_scale((1920, 1080), (1920, 1080), RenderScale::Hd2);

    assert_eq!(geometry.scale(), 1);
    assert_eq!(geometry.content_rect(), PixelRect::new(320, 60, 1280, 960));
    assert!(!geometry.fills_output());
    assert_eq!(geometry.window_to_frame(319, 60), None, "left bar");
    assert_eq!(geometry.window_to_frame(320, 60), Some((0, 0)));
    assert_eq!(geometry.window_to_frame(1599, 1019), Some((639, 479)));
    assert_eq!(geometry.window_to_frame(1600, 60), None, "right bar");
}

#[test]
fn pixel_perfect_geometry_is_the_same_through_either_constructor() {
    for (points, pixels) in [
        ((640, 480), (640, 480)),
        ((1920, 1080), (1920, 1080)),
        ((640, 480), (960, 720)),
        ((2560, 1440), (2560, 1440)),
    ] {
        let legacy = ScreenGeometry::native(points, pixels);
        let explicit =
            ScreenGeometry::native_with_render_scale(points, pixels, RenderScale::PixelPerfect);

        assert_eq!(legacy, explicit, "{points:?} / {pixels:?}");
        assert_eq!(explicit.render_frame_size(), (640, 480));
        assert_eq!(explicit.scale(), legacy.scale());
    }
}

#[test]
fn hd_window_opens_at_the_render_frame_so_the_interface_fits() {
    for scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let factor = scale.scale();
        let render = (FRAME_WIDTH * factor, FRAME_HEIGHT * factor);
        let points = window_points_for(FRAME_WIDTH, FRAME_HEIGHT, scale, 1);

        // The window is exactly the render frame (1280×960 at S = 2), never the
        // 640·S² frame that results from applying S twice.
        assert_eq!(points, render, "[{}]", scale.label());

        let geometry = ScreenGeometry::native_with_render_scale(points, points, scale);

        // The whole interface is in the window: the content rectangle is the
        // drawable itself, not a larger frame cropped to its centre.
        assert_eq!(geometry.render_frame_size(), render, "[{}]", scale.label());
        assert_eq!(
            geometry.content_rect(),
            PixelRect::new(0, 0, render.0 as i32, render.1 as i32),
            "[{}] the interface is cropped by the window",
            scale.label()
        );
        assert!(geometry.fills_output(), "[{}]", scale.label());

        // Hit test: the window point arrives at the UI as the logical 640×480
        // point (`/S` exactly once, no half-screen shift).
        assert_eq!(geometry.window_to_frame(0, 0), Some((0, 0)));
        assert_eq!(
            geometry.window_to_frame(render.0 as i32 - 1, render.1 as i32 - 1),
            Some((FRAME_WIDTH as i32 - 1, FRAME_HEIGHT as i32 - 1)),
            "[{}] the bottom-right window point",
            scale.label()
        );
        assert_eq!(
            geometry.window_to_frame((render.0 / 2) as i32, (render.1 / 2) as i32),
            Some(((FRAME_WIDTH / 2) as i32, (FRAME_HEIGHT / 2) as i32)),
            "[{}] the window centre",
            scale.label()
        );
    }
}

#[test]
fn hd_window_scale_multiplies_the_render_frame() {
    // `--scale 2` on the HD window: twice the render frame, every render pixel
    // an exact 2×2 output block — the logical frame is still 640×480.
    let points = window_points_for(FRAME_WIDTH, FRAME_HEIGHT, RenderScale::Hd2, 2);

    assert_eq!(points, (2560, 1920));

    let geometry = ScreenGeometry::native_with_render_scale(points, points, RenderScale::Hd2);

    assert_eq!(geometry.scale(), 2);
    assert_eq!(geometry.content_rect(), PixelRect::new(0, 0, 2560, 1920));
    assert_eq!(geometry.window_to_frame(2559, 1919), Some((639, 479)));
    assert_eq!(geometry.window_to_frame(1280, 960), Some((320, 240)));
}

#[test]
fn pixel_perfect_window_points_are_the_historical_sizes() {
    // `S = 1` keeps the historical `new_scaled` rule: the window is the logical
    // frame times the integer window scale.
    for (scale, expected) in [(1, (640, 480)), (2, (1280, 960)), (4, (2560, 1920))] {
        assert_eq!(
            window_points_for(FRAME_WIDTH, FRAME_HEIGHT, RenderScale::PixelPerfect, scale),
            expected
        );
    }

    // A zero window scale is the logical frame itself (defensive clamp).
    assert_eq!(
        window_points_for(FRAME_WIDTH, FRAME_HEIGHT, RenderScale::PixelPerfect, 0),
        (640, 480)
    );
    assert_eq!(
        window_points_for(FRAME_WIDTH, FRAME_HEIGHT, RenderScale::Hd2, 0),
        (1280, 960)
    );
}
