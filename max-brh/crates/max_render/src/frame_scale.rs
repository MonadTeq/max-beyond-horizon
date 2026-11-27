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
//!
//! # SSAA resolve
//!
//! [`downscale_box`] is the other direction: the super-sampled presentation frame
//! (`640·S·aa × 480·S·aa`, `max-brh-docs/development/ssaa-design.md`) is resolved
//! back to `640·S × 480·S` by an **integer box filter** — every output pixel is
//! the truncated mean of its `aa × aa` input block. The filter is a pure integer
//! function, so the resolved picture is a deterministic function of the render
//! frame and nothing else (no SDL linear filtering, no float rounding, no
//! window/DPI influence).

use crate::buffer::{PixelRect, Rgba, RgbaBuffer, BYTES_PER_PIXEL};

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

/// Box downsample of the whole frame by an integer factor (SSAA resolve).
///
/// Every output pixel is the **truncated mean** of its `factor × factor` block of
/// input pixels, per RGBA channel:
///
/// ```text
/// out[x, y] = sum(source[x·f + dx, y·f + dy]) / (f · f)
/// ```
///
/// The filter is an integer box on purpose (the owner decision of 2026-11-27,
/// `ssaa-design.md`):
///
/// * deterministic — the same input bytes always give the same output bytes, with
///   no float rounding and no SDL/graphics-driver filter;
/// * exact on flat areas — a block of one colour resolves to that colour, so the
///   interiors of sprites, tiles and UI chrome do not change;
/// * a true anti-aliaser on edges — a block that straddles two colours yields
///   their mean, so sub-pixel geometry drawn at `S·aa` comes out with
///   intermediate colours.
///
/// `factor <= 1` is the identity (a copy): [`AaMode::Off`](crate::render_scale::AaMode::Off)
/// must not touch the frame. The output size is `source / factor` (integer
/// division; a source whose size is not a multiple of `factor` is cropped at the
/// right/bottom edge — the SSAA frame is always an exact multiple).
pub fn downscale_box(source: &RgbaBuffer, factor: u32) -> RgbaBuffer {
    if factor <= 1 {
        return source.clone();
    }

    let width = source.width() / factor;
    let height = source.height() / factor;
    let mut output = RgbaBuffer::new(width, height);

    downscale_box_into(source, factor, &mut output);

    output
}

/// [`downscale_box`] into a caller-owned buffer (the per-frame presentation path).
///
/// `target` must be exactly `source / factor`; a differently sized target is
/// resized by the same rule ([`downscale_box`] allocates a correctly sized one
/// for one-shot callers). Returns the number of resolved pixels written.
pub fn downscale_box_into(source: &RgbaBuffer, factor: u32, target: &mut RgbaBuffer) -> u32 {
    if factor <= 1 {
        *target = source.clone();

        return target.len() as u32;
    }

    let width = source.width() / factor;
    let height = source.height() / factor;

    if target.width() != width || target.height() != height {
        *target = RgbaBuffer::new(width, height);
    }

    let box_pixels = factor * factor;
    let source_width = source.width() as usize;
    let source_pixels = source.pixels();
    let target_pixels = target.pixels_mut();
    let mut written = 0u32;

    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut sum = [0u32; 4];

            for dy in 0..factor as usize {
                let row = (y * factor as usize + dy) * source_width;

                for dx in 0..factor as usize {
                    let offset = (row + x * factor as usize + dx) * BYTES_PER_PIXEL;

                    for (channel, value) in sum.iter_mut().enumerate() {
                        *value += u32::from(source_pixels[offset + channel]);
                    }
                }
            }

            let offset = (y * width as usize + x) * BYTES_PER_PIXEL;

            for (channel, value) in sum.iter().enumerate() {
                target_pixels[offset + channel] = (value / box_pixels) as u8;
            }

            written += 1;
        }
    }

    written
}

#[cfg(test)]
#[path = "frame_scale_tests.rs"]
mod tests;
