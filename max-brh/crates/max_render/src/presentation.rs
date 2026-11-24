//! Presentation geometry: window points, output pixels and the logical frame.
//!
//! Task 1.4.2 (rendering on modern systems). SDL3 reports window and mouse
//! coordinates in *screen coordinates* (points), while the renderer draws into
//! *pixels*. On a display with 150% scaling a 640×480-point window has a
//! 960×720-pixel drawable, so the two spaces must not be mixed:
//!
//! * the frame is laid out and drawn in **pixels** — the integer nearest
//!   neighbour scale is computed from [`ScreenGeometry::output_pixels`], so the
//!   picture stays crisp and is not blurred by an OS bilinear stretch;
//! * a mouse point arrives in **points** and is mapped points → pixels first,
//!   then through the same [`FrameScale`] (letterbox bars reject the click).
//!
//! The logical frame stays 640×480 for every window, DPI and refresh rate:
//! scaling is a pure presentation transform and never reaches the simulation
//! (see `max_app::presentation::FixedStepClock` for the clock side of the same
//! invariant).

use crate::buffer::PixelRect;
use crate::frame_scale::{FrameScale, FRAME_HEIGHT, FRAME_WIDTH};

/// Window size in points, drawable size in pixels and the frame layout in pixels.
///
/// `window_points` is what SDL reports for `SDL_GetWindowSize` and what mouse
/// events carry; `output_pixels` is `SDL_GetRenderOutputSize` (the drawable).
/// On a 100% display both are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenGeometry {
    window_points: (u32, u32),
    output_pixels: (u32, u32),
    frame_width: u32,
    frame_height: u32,
    frame_scale: FrameScale,
}

impl ScreenGeometry {
    /// Geometry of a `frame_w × frame_h` frame in a window of `window_points`
    /// points with an `output_pixels` drawable.
    pub fn fit(
        window_points: (u32, u32),
        output_pixels: (u32, u32),
        frame_w: u32,
        frame_h: u32,
    ) -> Self {
        let frame_width = frame_w.max(1);
        let frame_height = frame_h.max(1);

        Self {
            window_points,
            output_pixels,
            frame_width,
            frame_height,
            frame_scale: FrameScale::fit(
                output_pixels.0,
                output_pixels.1,
                frame_width,
                frame_height,
            ),
        }
    }

    /// Geometry of the 640×480 logical frame.
    pub fn native(window_points: (u32, u32), output_pixels: (u32, u32)) -> Self {
        Self::fit(window_points, output_pixels, FRAME_WIDTH, FRAME_HEIGHT)
    }

    /// Window size in points.
    pub fn window_points(&self) -> (u32, u32) {
        self.window_points
    }

    /// Drawable (renderer output) size in pixels.
    pub fn output_pixels(&self) -> (u32, u32) {
        self.output_pixels
    }

    /// Logical frame size (640×480 in the client).
    pub fn frame_size(&self) -> (u32, u32) {
        (self.frame_width, self.frame_height)
    }

    /// Integer nearest-neighbour scale of the frame in the drawable.
    pub fn scale(&self) -> u32 {
        self.frame_scale.scale()
    }

    /// The frame layout in pixels: integer scale, centred, letterbox bars.
    pub fn frame_scale(&self) -> FrameScale {
        self.frame_scale
    }

    /// Frame area in output pixels (may extend beyond the window when cropped).
    pub fn content_rect(&self) -> PixelRect {
        self.frame_scale.content_rect()
    }

    /// Pixels per point (`1.0` at 100% DPI, `1.5` at 150%, `2.0` at 200%).
    ///
    /// Derived from the reported sizes rather than a separate DPI call: this is
    /// exactly the ratio the renderer uses, including display-scale changes
    /// while the window moves between monitors.
    pub fn pixel_ratio(&self) -> f32 {
        let point_width = self.window_points.0.max(1);
        let points = point_width as f32;

        self.output_pixels.0 as f32 / points
    }

    /// Window point (points) → drawable pixel, top-left convention.
    ///
    /// Used only to feed [`Self::window_to_frame`]; a standalone helper is kept
    /// public for tests and for cursor placement that needs pixels.
    pub fn point_to_output(&self, x: i32, y: i32) -> (i32, i32) {
        let point_width = self.window_points.0.max(1);
        let point_height = self.window_points.1.max(1);
        let pixel_x =
            (i64::from(x) * i64::from(self.output_pixels.0) / i64::from(point_width)) as i32;
        let pixel_y =
            (i64::from(y) * i64::from(self.output_pixels.1) / i64::from(point_height)) as i32;

        (pixel_x, pixel_y)
    }

    /// Window point (points) → logical frame point; `None` for a bar or outside.
    pub fn window_to_frame(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        if x < 0 || y < 0 || x >= self.window_points.0 as i32 || y >= self.window_points.1 as i32 {
            return None;
        }

        let (pixel_x, pixel_y) = self.point_to_output(x, y);

        self.frame_scale.window_to_frame(pixel_x, pixel_y)
    }

    /// Whether the frame fills the drawable exactly (no letterbox bars).
    pub fn fills_output(&self) -> bool {
        self.content_rect()
            == PixelRect::new(
                0,
                0,
                self.output_pixels.0 as i32,
                self.output_pixels.1 as i32,
            )
    }
}

/// Presentation-only state of the window: focus, minimize, occlusion.
///
/// Every method changes drawing decisions only. No method reads or writes the
/// simulation, the pause flag or the network state — alt-tab must not reset a
/// match and must not disturb lockstep barriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationState {
    focused: bool,
    minimized: bool,
    occluded: bool,
    /// Focus/minimize transitions seen (diagnostics).
    transitions: u32,
}

impl Default for PresentationState {
    fn default() -> Self {
        Self::new()
    }
}

impl PresentationState {
    /// A focused, visible window.
    pub fn new() -> Self {
        Self {
            focused: true,
            minimized: false,
            occluded: false,
            transitions: 0,
        }
    }

    /// Window lost the input focus (alt-tab, another window).
    pub fn focus_lost(&mut self) {
        if self.focused {
            self.focused = false;
            self.transitions += 1;
        }
    }

    /// Window gained the input focus again.
    pub fn focus_gained(&mut self) {
        if !self.focused {
            self.focused = true;
            self.transitions += 1;
        }
        self.minimized = false;
    }

    /// Window minimized.
    pub fn minimized(&mut self) {
        self.minimized = true;
        self.transitions += 1;
    }

    /// Window restored from the minimized state.
    pub fn restored(&mut self) {
        self.minimized = false;
        self.transitions += 1;
    }

    /// The window is fully occluded by other windows.
    pub fn occluded(&mut self) {
        self.occluded = true;
    }

    /// The window is visible again (or a redraw was requested).
    pub fn exposed(&mut self) {
        self.occluded = false;
    }

    /// Whether the window currently has the input focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether the window is minimized.
    pub fn is_minimized(&self) -> bool {
        self.minimized
    }

    /// Whether the frame should be drawn at all.
    ///
    /// A hidden window still runs its simulation and its network loop; only the
    /// GPU upload is skipped (no change of state, no barrier side effects).
    pub fn should_draw(&self) -> bool {
        !self.minimized && !self.occluded
    }

    /// Number of focus/minimize transitions seen.
    pub fn transitions(&self) -> u32 {
        self.transitions
    }
}

#[cfg(test)]
mod tests {
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
}
