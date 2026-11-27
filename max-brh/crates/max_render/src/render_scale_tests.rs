use super::*;

#[test]
fn pixel_perfect_is_scale_one_and_unchanged() {
    let scale = RenderScale::default();

    assert_eq!(scale, RenderScale::PixelPerfect);
    assert_eq!(scale.scale(), 1);
    assert!(!scale.is_hd());
    assert_eq!(scale.hd_scale(), None);
    assert_eq!(scale.render_size(), (640, 480));
    assert_eq!(scale.frame_size(), (640, 480));
    assert_eq!(scale.tile_pixels(), 64);
    assert_eq!(scale.filter(), FilterMode::Nearest);
}

#[test]
fn hd_scales_match_the_owner_decision() {
    for (scale, multiplier) in [(RenderScale::Hd2, 2), (RenderScale::Hd4, 4)] {
        assert_eq!(scale.scale(), multiplier);
        assert!(scale.is_hd());
        assert_eq!(scale.hd_scale(), Some(multiplier));
        assert_eq!(scale.render_size(), (640 * multiplier, 480 * multiplier));
        assert_eq!(
            scale.frame_size(),
            (640, 480),
            "the logical frame stays 640x480"
        );
        assert_eq!(scale.tile_pixels(), 64 * multiplier);
        assert_eq!(scale.filter(), FilterMode::Linear);
    }
}

#[test]
fn unsupported_scales_are_rejected() {
    assert_eq!(RenderScale::from_scale(1), Some(RenderScale::PixelPerfect));
    assert_eq!(RenderScale::from_scale(2), Some(RenderScale::Hd2));
    assert_eq!(RenderScale::from_scale(4), Some(RenderScale::Hd4));
    // The remaster ships x2/x4 only (owner decision); x3 and x0 are invalid.
    for scale in [0, 3, 5, 8, u32::MAX] {
        assert_eq!(RenderScale::from_scale(scale), None, "S={scale}");
    }
}

#[test]
fn filtering_is_global_per_mode() {
    // Guardrail 5: one filter for the whole frame, not per asset.
    assert_eq!(RenderScale::PixelPerfect.filter(), FilterMode::Nearest);
    assert_eq!(RenderScale::Hd2.filter(), FilterMode::Linear);
    assert_eq!(RenderScale::Hd4.filter(), FilterMode::Linear);
    assert_ne!(FilterMode::default(), FilterMode::Linear);
}

#[test]
fn labels_are_distinct() {
    let labels = [
        RenderScale::PixelPerfect.label(),
        RenderScale::Hd2.label(),
        RenderScale::Hd4.label(),
    ];

    assert_eq!(labels, ["pixel-perfect", "hd-2x", "hd-4x"]);
}
