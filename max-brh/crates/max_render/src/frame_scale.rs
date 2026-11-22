//! Scaling of the fixed 640×480 frame to a window of any size
//! (item F of `max-brh-docs/visual-1to1-status.md`).
//!
//! Plan variant 1: the application always draws a frame of [`crate::layout::WINDOW_WIDTH`]×
//! [`crate::layout::WINDOW_HEIGHT`] (640×480), and it is output to the window with an
//! **integer nearest-neighbour** upscale. At scale `N` every
//! source pixel becomes an `N×N` block, so the picture stays
//! pixel-for-pixel on a monitor with any DPI (the edges do not blur as with
//! bilinear stretching).
//!
//! If the window is not a multiple of 640×480, the frame is centered and black
//! bars remain around it (letterbox). The inverse mouse coordinate transform accounts for
//! both the scale and the bars:
//!
//! ```text
//! frame_x = (window_x - offset_x) / scale
//! ```
//!
//! The C++ port does it differently — `WindowManager_GetScale` and per-element
//! scaling (`WindowManager_ScaleWindow`, `WindowManager_RescaleSimpleImage`)
//! in `max-cpp-port/src/window_manager.cpp`. Our variant was chosen because it gives
//! a strict "pixel-for-pixel" criterion: the source frame is not interpolated, and all
//! UI coordinates stay 640×480 constants.

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};

/// Width of the original logical frame.
pub const FRAME_WIDTH: u32 = 640;
/// Height of the original logical frame.
pub const FRAME_HEIGHT: u32 = 480;
/// Maximum integer window scale (protection against gigantic buffers).
pub const MAX_SCALE: u32 = 8;

/// How the `frame_w × frame_h` frame is laid out in the `window_w × window_h` window.
///
/// `scale == 1` — 1:1 frame; `scale == 2` — every pixel 2×2, etc.
/// `offset_x`/`offset_y` — position of the top-left corner of the frame in the window
/// (non-negative when the window is larger than the frame, negative when it is smaller — then
/// the frame is cropped at the center).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameScale {
    window_width: u32,
    window_height: u32,
    frame_width: u32,
    frame_height: u32,
    scale: u32,
    offset_x: i32,
    offset_y: i32,
}

impl FrameScale {
    /// Layout of the `frame_w × frame_h` frame in the `window_w × window_h` window.
    ///
    /// Integer scale — `min(window_w / frame_w, window_h / frame_h)`,
    /// but not less than 1: a window smaller than the frame crops it rather than shrinking it
    /// (shrinking breaks "pixel-for-pixel"). The frame is centered.
    pub fn fit(window_w: u32, window_h: u32, frame_w: u32, frame_h: u32) -> Self {
        let frame_width = frame_w.max(1);
        let frame_height = frame_h.max(1);
        let scale = (window_w / frame_width)
            .min(window_h / frame_height)
            .clamp(1, MAX_SCALE);
        let content_w = frame_width as i64 * i64::from(scale);
        let content_h = frame_height as i64 * i64::from(scale);
        let offset_x = ((i64::from(window_w) - content_w) / 2) as i32;
        let offset_y = ((i64::from(window_h) - content_h) / 2) as i32;

        Self {
            window_width: window_w,
            window_height: window_h,
            frame_width,
            frame_height,
            scale,
            offset_x,
            offset_y,
        }
    }

    /// Layout of the 640×480 frame (the original window) in a window.
    pub fn for_frame(window_w: u32, window_h: u32, frame_w: u32, frame_h: u32) -> Self {
        Self::fit(window_w, window_h, frame_w, frame_h)
    }

    /// Screen 640×480 (the values [`FRAME_WIDTH`]/[`FRAME_HEIGHT`]).
    pub fn native(window_w: u32, window_h: u32) -> Self {
        Self::fit(window_w, window_h, FRAME_WIDTH, FRAME_HEIGHT)
    }

    /// Integer factor (1..=[`MAX_SCALE`]).
    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// Window size.
    pub fn window_size(&self) -> (u32, u32) {
        (self.window_width, self.window_height)
    }

    /// Size of the logical frame.
    pub fn frame_size(&self) -> (u32, u32) {
        (self.frame_width, self.frame_height)
    }

    /// Offset of the frame in the window.
    pub fn offset(&self) -> (i32, i32) {
        (self.offset_x, self.offset_y)
    }

    /// Frame area in window coordinates (may extend beyond the window).
    pub fn content_rect(&self) -> PixelRect {
        PixelRect::new(
            self.offset_x,
            self.offset_y,
            (self.frame_width * self.scale) as i32,
            (self.frame_height * self.scale) as i32,
        )
    }

    /// Logical frame point -> the top-left pixel of its block in the window.
    pub fn frame_to_window(&self, frame_x: i32, frame_y: i32) -> (i32, i32) {
        (
            self.offset_x + frame_x * self.scale as i32,
            self.offset_y + frame_y * self.scale as i32,
        )
    }

    /// Window point -> logical frame point; `None` — the point is in a black bar
    /// (letterbox) or outside the frame.
    pub fn window_to_frame(&self, window_x: i32, window_y: i32) -> Option<(i32, i32)> {
        if window_x < 0
            || window_y < 0
            || window_x >= self.window_width as i32
            || window_y >= self.window_height as i32
        {
            return None;
        }

        let frame_x = (window_x - self.offset_x).div_euclid(self.scale as i32);
        let frame_y = (window_y - self.offset_y).div_euclid(self.scale as i32);

        if frame_x < 0
            || frame_y < 0
            || frame_x >= self.frame_width as i32
            || frame_y >= self.frame_height as i32
        {
            return None;
        }

        Some((frame_x, frame_y))
    }

    /// Expands the frame into a buffer of the window size: `scale × scale` blocks, the bars
    /// are black (alpha 255).
    ///
    /// This is the reference implementation for tests and headless frames; the SDL layer
    /// uses the same [`FrameScale`] but leaves scaling to the GPU
    /// (`copy` into the [`Self::content_rect`] rectangle), without copying pixels on
    /// CPU.
    pub fn upscale(&self, frame: &RgbaBuffer) -> RgbaBuffer {
        let mut window = RgbaBuffer::filled(self.window_width, self.window_height, Rgba::BLACK);
        let scale = self.scale as i32;

        for frame_y in 0..frame.height() as i32 {
            for frame_x in 0..frame.width() as i32 {
                let Some(color) = frame.get(frame_x, frame_y) else {
                    continue;
                };
                let (window_x, window_y) = self.frame_to_window(frame_x, frame_y);

                for dy in 0..scale {
                    for dx in 0..scale {
                        window.set(window_x + dx, window_y + dy, color);
                    }
                }
            }
        }

        window
    }
}

/// Nearest-neighbour upscale of the whole frame to an arbitrary size (no bars).
///
/// Used when the aspect ratio already matches (e.g. 640×480 →
/// 1280×960): every source pixel lands in exactly an `N×N` block. For an
/// arbitrary aspect ratio the caller usually takes [`FrameScale`],
/// so as not to blur or distort the picture.
pub fn upscale_nearest(source: &RgbaBuffer, width: u32, height: u32) -> RgbaBuffer {
    let mut output = RgbaBuffer::new(width, height);
    let source_width = source.width().max(1);
    let source_height = source.height().max(1);

    for y in 0..height {
        let source_y = (y * source_height / height).min(source_height - 1) as i32;

        for x in 0..width {
            let source_x = (x * source_width / width).min(source_width - 1) as i32;

            if let Some(color) = source.get(source_x, source_y) {
                output.set(x as i32, y as i32, color);
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
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
}
