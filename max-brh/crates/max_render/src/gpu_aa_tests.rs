//! Unit tests of the pure GPU-AA geometry (`gpu-aa-design.md`).
//!
//! The SDL3 GPU calls themselves cannot run in `nextest` (no device, no window),
//! so the layer is split: everything that decides *where* the frame is blitted
//! and *with which* FXAA parameters is a pure function and is checked here. The
//! shader pass itself is covered by the `gpu_aa` example (visual gate).

use super::*;
use crate::presentation::ScreenGeometry;
use crate::render_scale::RenderScale;

/// Content rectangle of a 640×480 frame rendered at `S` in an `output × …`
/// drawable — the same chain the window uses.
fn content(output: (u32, u32), scale: RenderScale) -> PixelRect {
    ScreenGeometry::fit_with_render_scale(output, output, 640, 480, scale).content_rect()
}

#[test]
fn blit_covers_the_whole_swapchain_without_bars() {
    let content = content((1280, 960), RenderScale::Hd2);
    assert_eq!(content, PixelRect::new(0, 0, 1280, 960));

    let regions = blit_regions(content, (1280, 960)).expect("the frame is visible");

    assert_eq!(regions.source, (0, 0, 1280, 960));
    assert_eq!(regions.destination, (0, 0, 1280, 960));
}

#[test]
fn blit_letterboxes_a_centred_frame() {
    // 1280×960 render frame in a 1920×1080 drawable: integer scale 1, centred.
    let content = content((1920, 1080), RenderScale::Hd2);
    assert_eq!(content, PixelRect::new(320, 60, 1280, 960));

    let regions = blit_regions(content, (1920, 1080)).expect("the frame is visible");

    assert_eq!(
        regions.source,
        (0, 0, 1280, 960),
        "the whole render frame is read"
    );
    assert_eq!(
        regions.destination,
        (320, 60, 1280, 960),
        "the frame is written centred, the bars are left to the clear"
    );
}

#[test]
fn blit_clips_a_frame_larger_than_the_swapchain() {
    // The frame is cropped at the centre when the window is smaller than it:
    // a negative content offset must move the *source*, not the destination.
    let regions = blit_regions(PixelRect::new(-160, 0, 1280, 960), (1280, 960))
        .expect("part of the frame is visible");

    assert_eq!(regions.source, (160, 0, 1120, 960));
    assert_eq!(regions.destination, (0, 0, 1120, 960));
}

#[test]
fn blit_clips_a_frame_that_overflows_the_bottom_right() {
    let regions = blit_regions(PixelRect::new(0, 0, 1280, 960), (640, 480))
        .expect("part of the frame is visible");

    assert_eq!(regions.source, (0, 0, 640, 480));
    assert_eq!(regions.destination, (0, 0, 640, 480));
}

#[test]
fn blit_with_nothing_visible_is_none() {
    assert_eq!(
        blit_regions(PixelRect::new(2000, 0, 100, 100), (1280, 960)),
        None,
        "a frame entirely right of the drawable"
    );
    assert_eq!(
        blit_regions(PixelRect::new(0, 1200, 100, 100), (1280, 960)),
        None,
        "a frame entirely below the drawable"
    );
    assert_eq!(
        blit_regions(PixelRect::new(0, 0, 100, 100), (0, 0)),
        None,
        "a zero-sized drawable"
    );
    assert_eq!(
        blit_regions(PixelRect::new(0, 0, 0, 0), (1280, 960)),
        None,
        "an empty frame"
    );
}

#[test]
fn blit_keeps_the_regions_inside_both_textures() {
    for (output, scale) in [
        ((640, 480), RenderScale::PixelPerfect),
        ((1280, 960), RenderScale::Hd2),
        ((1920, 1080), RenderScale::Hd2),
        ((2560, 1920), RenderScale::Hd4),
        ((1000, 700), RenderScale::Hd2),
        ((400, 300), RenderScale::Hd4),
    ] {
        let content = content(output, scale);
        let Some(regions) = blit_regions(content, output) else {
            continue;
        };

        let frame = (content.width.max(0) as u32, content.height.max(0) as u32);
        let (sx, sy, sw, sh) = regions.source;
        let (dx, dy, dw, dh) = regions.destination;

        assert!(sx + sw <= frame.0, "{output:?} {scale:?}: source x");
        assert!(sy + sh <= frame.1, "{output:?} {scale:?}: source y");
        assert!(dx + dw <= output.0, "{output:?} {scale:?}: dest x");
        assert!(dy + dh <= output.1, "{output:?} {scale:?}: dest y");
        assert_eq!((sw, sh), (dw, dh), "{output:?} {scale:?}: 1:1 regions");
    }
}

#[test]
fn fxaa_params_carry_the_flat_threshold() {
    let params = FxaaParams::new();

    assert_eq!(params.threshold, FXAA_LUMA_THRESHOLD);
    assert_eq!(params.padding, [0.0; 3]);
    assert_eq!(FxaaParams::default(), params);
}

#[test]
fn fxaa_params_threshold_separates_flat_from_edges() {
    // A flat 8-bit area has no luma step at all; a black/white edge has 1.0.
    assert!(FXAA_LUMA_THRESHOLD > 0.0);
    assert!(FXAA_LUMA_THRESHOLD < 1.0);
    assert!(
        FXAA_LUMA_THRESHOLD < 0.5,
        "a mid-grey edge must still be smoothed"
    );
}

#[test]
fn uniform_block_matches_the_std140_layout() {
    // `{ vec2 texel; float threshold; float padding; }` is 16 bytes.
    assert_eq!(std::mem::size_of::<FxaaParams>(), 16);
    assert_eq!(std::mem::align_of::<FxaaParams>(), 4);
}
