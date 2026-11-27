//! Pure geometry/parameters of the GPU anti-aliasing layer
//! (`max-brh-docs/development/gpu-aa-design.md`).
//!
//! The SDL3 GPU presentation (`crate::gpu_present`, feature `sdl`) needs two
//! values that are plain integer/`f32` arithmetic and must be correct without a
//! window, a device or a driver:
//!
//! * [`blit_regions`] — which part of the render frame is blitted to which part
//!   of the swapchain. The frame is centred with black bars and may be **cropped**
//!   when the window is smaller than the render frame, so the source and the
//!   destination rectangles are not the same size; an out-of-bounds blit region
//!   is a Vulkan validation error, hence the clipping here.
//! * [`fxaa_params`] — the uniform block of the FXAA fragment shader (one texel
//!   step in UV space and the luma threshold below which a neighbourhood counts
//!   as flat).
//!
//! Keeping them pure is what makes the layer testable in `nextest`
//! (`--no-default-features` has no SDL): `max_render::gpu_present` only feeds
//! these values to the API.

use crate::buffer::PixelRect;

/// Luma contrast below which the FXAA pass leaves a pixel untouched (default).
///
/// A `640·S × 480·S` frame is quantised to 8 bits per channel, so the luma step
/// of adjacent flat pixels is exactly `0`; `1/32` is far above that noise floor
/// and far below the contrast of any real edge (sprite outlines, tile borders,
/// glyph stems), so flat interiors are copied verbatim while edges are smoothed.
pub const FXAA_LUMA_THRESHOLD: f32 = 1.0 / 32.0;

/// Uniform block of `shaders/fxaa.frag` (std140-layout compatible).
///
/// The block carries **only** the flat-area threshold. The texel size — the
/// other thing the pass needs — is derived in the shader from the source
/// texture (`textureSize`), so a lost/zero uniform can never collapse the taps
/// into a copy: the worst case of a missing uniform is a more aggressive
/// anti-aliaser (threshold `0`), not a silent no-op.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxaaParams {
    /// Luma contrast below which the neighbourhood is flat; see
    /// [`FXAA_LUMA_THRESHOLD`].
    pub threshold: f32,
    /// Padding to the 16-byte std140 block size.
    pub padding: [f32; 3],
}

impl FxaaParams {
    /// The default parameters of the pass.
    pub const fn new() -> Self {
        Self {
            threshold: FXAA_LUMA_THRESHOLD,
            padding: [0.0; 3],
        }
    }
}

impl Default for FxaaParams {
    fn default() -> Self {
        Self::new()
    }
}

/// Source and destination rectangles of the swapchain blit, in pixels.
///
/// Each tuple is `(x, y, width, height)`. Both are always inside their texture:
/// the destination inside the swapchain, the source inside the render frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlitRegions {
    /// Region of the render frame to read.
    pub source: (u32, u32, u32, u32),
    /// Region of the swapchain to write.
    pub destination: (u32, u32, u32, u32),
}

/// Clips the frame layout `content` to the `swapchain` pixels.
///
/// `content` is [`crate::presentation::ScreenGeometry::content_rect`]: the
/// render frame in output pixels, centred, letterboxed and possibly negative
/// (cropped) when the window is smaller than the frame. The result is the
/// largest whole-pixel overlap of the two, with the source offset adjusted by
/// the cropped part; `None` when nothing of the frame is visible.
///
/// Blitting exactly `content` without this clip would be out of bounds on the
/// swapchain (Vulkan rejects a region that leaves the image) and would shift the
/// picture when the frame is cropped.
pub fn blit_regions(content: PixelRect, swapchain: (u32, u32)) -> Option<BlitRegions> {
    let swapchain_width = swapchain.0 as i64;
    let swapchain_height = swapchain.1 as i64;

    if swapchain_width <= 0 || swapchain_height <= 0 || content.is_empty() {
        return None;
    }

    let frame_right = i64::from(content.ulx) + i64::from(content.width);
    let frame_bottom = i64::from(content.uly) + i64::from(content.height);
    let left = i64::from(content.ulx).max(0);
    let top = i64::from(content.uly).max(0);
    let right = frame_right.min(swapchain_width);
    let bottom = frame_bottom.min(swapchain_height);

    if right <= left || bottom <= top {
        return None;
    }

    let width = (right - left) as u32;
    let height = (bottom - top) as u32;

    Some(BlitRegions {
        source: (
            (left - i64::from(content.ulx)) as u32,
            (top - i64::from(content.uly)) as u32,
            width,
            height,
        ),
        destination: (left as u32, top as u32, width, height),
    })
}

#[cfg(test)]
#[path = "gpu_aa_tests.rs"]
mod tests;
