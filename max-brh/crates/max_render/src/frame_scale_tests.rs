use super::*;
use crate::layout::MAIN_MAP;
use crate::viewport::Viewport;

/// 640×480 frame with a unique color per pixel: `r`/`g` encode
/// the coordinates, so any shift/interpolation is visible.
fn coded_frame() -> RgbaBuffer {
    let mut frame = RgbaBuffer::new(FRAME_WIDTH, FRAME_HEIGHT);

    for y in 0..FRAME_HEIGHT {
        for x in 0..FRAME_WIDTH {
            frame.set(
                x as i32,
                y as i32,
                Rgba::rgb((x % 256) as u8, (y % 256) as u8, 7),
            );
        }
    }

    frame
}

#[test]
fn double_upscale_replicates_every_pixel_as_a_two_by_two_block() {
    let frame = coded_frame();
    let scale = FrameScale::native(1280, 960);

    assert_eq!(scale.scale(), 2);
    assert_eq!(scale.offset(), (0, 0));

    let window = scale.upscale(&frame);

    assert_eq!((window.width(), window.height()), (1280, 960));

    // Every source pixel is exactly a 2×2 block, without blending.
    for (x, y) in [(0, 0), (1, 0), (255, 127), (639, 479), (320, 240)] {
        let expected = frame.get(x, y).expect("пиксель кадра");
        let block = scale.frame_to_window(x, y);

        for dy in 0..2 {
            for dx in 0..2 {
                assert_eq!(
                    window.get(block.0 + dx, block.1 + dy),
                    Some(expected),
                        "пиксель ({x},{y}) должен быть блоком 2×2 в {block:?}"
                );
            }
        }
    }

    // Neighboring blocks differ — so this is not a fill with one color.
    assert_ne!(window.get(0, 0), window.get(2, 0));
    assert_ne!(window.get(0, 0), window.get(0, 2));
}

#[test]
fn generic_upscale_is_pure_nearest_neighbour() {
    let mut frame = RgbaBuffer::new(2, 2);
    let red = Rgba::rgb(255, 0, 0);
    let green = Rgba::rgb(0, 255, 0);
    let blue = Rgba::rgb(0, 0, 255);
    let white = Rgba::rgb(255, 255, 255);

    frame.set(0, 0, red);
    frame.set(1, 0, green);
    frame.set(0, 1, blue);
    frame.set(1, 1, white);

    let doubled = upscale_nearest(&frame, 4, 4);

    assert_eq!(doubled.get(0, 0), Some(red));
    assert_eq!(doubled.get(1, 1), Some(red));
    assert_eq!(doubled.get(2, 0), Some(green));
    assert_eq!(doubled.get(3, 1), Some(green));
    assert_eq!(doubled.get(0, 2), Some(blue));
    assert_eq!(doubled.get(1, 3), Some(blue));
    assert_eq!(doubled.get(2, 2), Some(white));
    assert_eq!(doubled.get(3, 3), Some(white));
    // No intermediate colors.
    for pixel in doubled.pixels().chunks_exact(4) {
        assert!(
            [red, green, blue, white]
                .iter()
                .any(|color| pixel == color.to_rgba()),
                "появился интерполированный цвет {pixel:?}"
        );
    }
}

#[test]
fn odd_window_letterboxes_and_centres_the_frame() {
    // 1000×700: integer scale 1, bars at (180,110).
    let scale = FrameScale::native(1000, 700);

    assert_eq!(scale.scale(), 1);
    assert_eq!(scale.offset(), (180, 110));
    assert_eq!(scale.content_rect(), PixelRect::new(180, 110, 640, 480));

    // A frame point is translated to the window and back.
    for (x, y) in [(0, 0), (639, 479), (320, 240)] {
        let window = scale.frame_to_window(x, y);
        assert_eq!(scale.window_to_frame(window.0, window.1), Some((x, y)));
    }

    // Bars do not accept clicks: (10, 10) is outside the frame.
    assert_eq!(scale.window_to_frame(10, 10), None);
    assert_eq!(scale.window_to_frame(999, 699), None);
    assert_eq!(scale.window_to_frame(180, 110), Some((0, 0)));
}

#[test]
fn window_smaller_than_frame_crops_instead_of_shrinking() {
    let scale = FrameScale::native(320, 240);

    assert_eq!(scale.scale(), 1);
    assert_eq!(scale.offset(), (-160, -120));
    // The center of the frame is visible, the edges are cropped.
    assert_eq!(scale.window_to_frame(0, 0), Some((160, 120)));
    assert_eq!(scale.window_to_frame(-1, 0), None);
    assert_eq!(scale.window_to_frame(320, 240), None);
    assert_eq!(scale.window_to_frame(159, 119), Some((319, 239)));
    assert_eq!(scale.window_to_frame(639, 479), None);
}

#[test]
fn clicks_in_a_1280x960_window_map_to_the_same_map_cell() {
    // The 448×448 map in a 640×480 window starts at (180,18).
    let viewport = Viewport::with_center(
        MAIN_MAP.width as u32,
        MAIN_MAP.height as u32,
        (32 * 64 + 32, 32 * 64 + 32),
        64,
    );
    let scale = FrameScale::native(1280, 960);

    assert_eq!(scale.scale(), 2);

    // Local map point (224,160) is cell (32,31).
    let frame_click = (MAIN_MAP.ulx + 224, MAIN_MAP.uly + 160);
    let expected_grid = viewport.screen_to_grid(224, 160);

    assert_eq!(expected_grid, (32, 31));

    // A click in a 1280×960 window inside the corresponding 2×2 block.
    let window_click = scale.frame_to_window(frame_click.0, frame_click.1);
    let frame_point = scale
        .window_to_frame(window_click.0 + 1, window_click.1 + 1)
        .expect("клик внутри кадра");

    assert_eq!(frame_point, frame_click);

    let map_point = (frame_point.0 - MAIN_MAP.ulx, frame_point.1 - MAIN_MAP.uly);

    assert_eq!(
        viewport.screen_to_grid(map_point.0, map_point.1),
        expected_grid
    );

    // A corner click of the same block gives the same cell — the scale does not "eat"
    // a pixel at the boundary.
    let corner = scale
        .window_to_frame(window_click.0, window_click.1)
        .expect("левый верхний пиксель блока");
    assert_eq!(corner, frame_click);
}

#[test]
fn scale_is_clamped_and_never_zero() {
    assert_eq!(FrameScale::native(0, 0).scale(), 1);
    assert_eq!(FrameScale::native(639, 479).scale(), 1);
    assert_eq!(FrameScale::native(100_000, 100_000).scale(), MAX_SCALE);
}

/// Task 1.4.2 / golden gate: upscaling is a pure presentation transform.
///
/// The 640×480 frame stays the reference (the golden is rendered into it,
/// never into a scaled buffer), and every window/DPI profile produces a
/// derived image without touching a single source byte.
#[test]
fn upscale_does_not_touch_the_source_frame() {
    let frame = coded_frame();
    let before = frame.pixels().to_vec();
    let native = FrameScale::native(FRAME_WIDTH, FRAME_HEIGHT);

    assert_eq!(native.scale(), 1);
    assert_eq!(native.upscale(&frame).pixels(), before.as_slice());

    for (width, height) in [(1280, 960), (1920, 1080), (2560, 1440), (3840, 2160)] {
        let scale = FrameScale::native(width, height);
        let window = scale.upscale(&frame);

        assert_eq!((window.width(), window.height()), (width, height));
        assert_eq!(
            frame.pixels(),
            before.as_slice(),
                "the source frame changed at {width}x{height}"
        );
    }
}
