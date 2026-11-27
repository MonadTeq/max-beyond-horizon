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
use crate::render_scale::RenderScale;

/// Window size in points, drawable size in pixels and the frame layout in pixels.
///
/// `window_points` is what SDL reports for `SDL_GetWindowSize` and what mouse
/// events carry; `output_pixels` is `SDL_GetRenderOutputSize` (the drawable).
/// On a 100% display both are equal.
///
/// `render_scale` (`remaster-hd.md` §2.2) adds one link to the chain:
/// the drawable holds a **render frame** of `640·S × 480·S` (`S = 1` in
/// [`RenderScale::PixelPerfect`]), while every public frame coordinate — the
/// camera, UI hit tests, clicks — stays in the logical 640×480 space. The
/// integer window fit is computed for the render frame, and
/// [`Self::window_to_frame`] divides by `S` after the reverse transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenGeometry {
    window_points: (u32, u32),
    output_pixels: (u32, u32),
    frame_width: u32,
    frame_height: u32,
    render_scale: RenderScale,
    frame_scale: FrameScale,
}

impl ScreenGeometry {
    /// Geometry of a `frame_w × frame_h` frame in a window of `window_points`
    /// points with an `output_pixels` drawable.
    ///
    /// This is the pixel-perfect constructor (`S = 1`); use
    /// [`Self::fit_with_render_scale`] for an HD render target.
    pub fn fit(
        window_points: (u32, u32),
        output_pixels: (u32, u32),
        frame_w: u32,
        frame_h: u32,
    ) -> Self {
        Self::fit_with_render_scale(
            window_points,
            output_pixels,
            frame_w,
            frame_h,
            RenderScale::PixelPerfect,
        )
    }

    /// Geometry of a `frame_w × frame_h` logical frame rendered at `render_scale`
    /// into a window of `window_points` points with an `output_pixels` drawable.
    ///
    /// The frame fit and the content rectangle are computed from the render
    /// frame (`frame_w·S × frame_h·S`); the logical frame size is unchanged.
    pub fn fit_with_render_scale(
        window_points: (u32, u32),
        output_pixels: (u32, u32),
        frame_w: u32,
        frame_h: u32,
        render_scale: RenderScale,
    ) -> Self {
        let frame_width = frame_w.max(1);
        let frame_height = frame_h.max(1);
        let render_width = frame_width * render_scale.scale();
        let render_height = frame_height * render_scale.scale();

        Self {
            window_points,
            output_pixels,
            frame_width,
            frame_height,
            render_scale,
            frame_scale: FrameScale::fit(
                output_pixels.0,
                output_pixels.1,
                render_width,
                render_height,
            ),
        }
    }

    /// Geometry of the 640×480 logical frame (pixel-perfect).
    pub fn native(window_points: (u32, u32), output_pixels: (u32, u32)) -> Self {
        Self::fit(window_points, output_pixels, FRAME_WIDTH, FRAME_HEIGHT)
    }

    /// Geometry of the 640×480 logical frame rendered at `render_scale`.
    pub fn native_with_render_scale(
        window_points: (u32, u32),
        output_pixels: (u32, u32),
        render_scale: RenderScale,
    ) -> Self {
        Self::fit_with_render_scale(
            window_points,
            output_pixels,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            render_scale,
        )
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

    /// Render frame size in pixels (`640·S × 480·S`).
    ///
    /// This is the buffer the renderer draws into in HD mode; in
    /// [`RenderScale::PixelPerfect`] it equals [`Self::frame_size`].
    pub fn render_frame_size(&self) -> (u32, u32) {
        (
            self.frame_width * self.render_scale.scale(),
            self.frame_height * self.render_scale.scale(),
        )
    }

    /// Presentation scale of the render target.
    pub fn render_scale(&self) -> RenderScale {
        self.render_scale
    }

    /// Integer nearest-neighbour scale of the render frame in the drawable.
    ///
    /// This is the window-level factor on top of [`Self::render_scale`]: the
    /// displayed size is `frame_size · S · scale()`.
    pub fn scale(&self) -> u32 {
        self.frame_scale.scale()
    }

    /// The render frame layout in pixels: integer scale, centred, letterbox bars.
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
    ///
    /// The chain is points → output pixels → render frame → `/ S` → logical
    /// frame. In [`RenderScale::PixelPerfect`] the last step is the identity,
    /// so the historical behavior is unchanged.
    pub fn window_to_frame(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        if x < 0 || y < 0 || x >= self.window_points.0 as i32 || y >= self.window_points.1 as i32 {
            return None;
        }

        let (pixel_x, pixel_y) = self.point_to_output(x, y);
        let (render_x, render_y) = self.frame_scale.window_to_frame(pixel_x, pixel_y)?;
        let scale = self.render_scale.scale() as i32;

        Some((render_x / scale, render_y / scale))
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
#[path = "presentation_tests.rs"]
mod tests;
