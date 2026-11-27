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

// ---------------------------------------------------------------------------
// PresentationMode (§R7 switch)
// ---------------------------------------------------------------------------

#[test]
fn the_default_presentation_mode_is_pixel_perfect() {
    let mode = PresentationMode::default();

    assert_eq!(mode, PresentationMode::PixelPerfect);
    assert!(!mode.is_hd());
    assert_eq!(mode.label(), "pixel-perfect");
    // Even with an HD scale in hand the pixel-perfect mode never uses it.
    assert_eq!(
        mode.render_scale(RenderScale::Hd4),
        RenderScale::PixelPerfect
    );
}

#[test]
fn hd_mode_resolves_to_the_chosen_hd_scale() {
    let mode = PresentationMode::Hd;

    assert!(mode.is_hd());
    assert_eq!(mode.label(), "hd");
    assert_eq!(mode.render_scale(RenderScale::Hd2), RenderScale::Hd2);
    assert_eq!(mode.render_scale(RenderScale::Hd4), RenderScale::Hd4);
    // The default is the multiplier R0 measured as affordable (×2).
    assert_eq!(PresentationMode::DEFAULT_HD_SCALE, RenderScale::Hd2);
}

#[test]
fn cli_flags_select_the_mode_and_scale() {
    // No flag: the historical pipeline.
    assert_eq!(
        PresentationMode::from_flags(false, None),
        Ok((PresentationMode::PixelPerfect, RenderScale::PixelPerfect))
    );
    // `--hd` alone: the default HD scale.
    assert_eq!(
        PresentationMode::from_flags(true, None),
        Ok((PresentationMode::Hd, RenderScale::Hd2))
    );
    // `--hd --hd-scale N`.
    for (scale, expected) in [(2u32, RenderScale::Hd2), (4, RenderScale::Hd4)] {
        assert_eq!(
            PresentationMode::from_flags(true, Some(scale)),
            Ok((PresentationMode::Hd, expected)),
            "--hd-scale {scale}"
        );
        // `--hd-scale` implies HD: naming a multiplier asks for the HD mode.
        assert_eq!(
            PresentationMode::from_flags(false, Some(scale)),
            Ok((PresentationMode::Hd, expected)),
            "--hd-scale {scale} without --hd"
        );
    }
    // `--hd-scale 1` is the pixel-perfect mode through the same switch.
    assert_eq!(
        PresentationMode::from_flags(true, Some(1)),
        Ok((PresentationMode::PixelPerfect, RenderScale::PixelPerfect))
    );
}

#[test]
fn unsupported_cli_scales_are_rejected() {
    for scale in [0u32, 3, 5, 8, u32::MAX] {
        let error = PresentationMode::from_flags(true, Some(scale))
            .expect_err(&format!("S={scale} must be rejected"));

        assert_eq!(error, PresentationModeError::UnsupportedScale(scale));
        assert_eq!(error.scale(), scale);
        assert!(!error.to_string().is_empty());
    }
}

// ---------------------------------------------------------------------------
// AaMode (SSAA, ssaa-design.md)
// ---------------------------------------------------------------------------

#[test]
fn antialiasing_is_off_by_default() {
    let aa = AaMode::default();

    assert_eq!(aa, AaMode::Off);
    assert!(aa.is_off());
    assert!(!aa.is_on());
    assert_eq!(aa.factor(), 1);
    assert_eq!(aa.box_size(), 1);
    assert_eq!(aa.label(), "off");
    assert_eq!(aa.menu_label(), "Off");
}

#[test]
fn shipped_antialiasing_multipliers_are_the_owner_decision() {
    // Owner decision 2026-11-27: the SSAA box is ×2 or ×4 only.
    assert_eq!(AA_FACTORS, [2, 4]);
    assert_eq!(AaMode::X2.factor(), 2);
    assert_eq!(AaMode::X4.factor(), 4);
    assert!(AaMode::X2.is_on());
    assert!(AaMode::X4.is_on());
    assert_eq!(AaMode::X2.box_size(), 2);
    assert_eq!(AaMode::X4.box_size(), 4);
    assert_eq!(AaMode::X2.label(), "aa-2x");
    assert_eq!(AaMode::X4.label(), "aa-4x");
    // The menu order is Off → 2x → 4x.
    assert_eq!(AaMode::ALL, [AaMode::Off, AaMode::X2, AaMode::X4]);
}

#[test]
fn zero_and_one_mean_off_and_an_unsupported_factor_is_rejected() {
    assert_eq!(AaMode::from_factor(0), Some(AaMode::Off));
    assert_eq!(AaMode::from_factor(1), Some(AaMode::Off));
    assert_eq!(AaMode::from_factor(2), Some(AaMode::X2));
    assert_eq!(AaMode::from_factor(4), Some(AaMode::X4));

    // A non-integer box is not an anti-aliasing filter we ship (3/8/…).
    for factor in [3, 5, 6, 8, u32::MAX] {
        assert_eq!(AaMode::from_factor(factor), None, "aa={factor}");
    }
}

#[test]
fn the_cli_flag_accepts_off_and_the_four_numbers() {
    for value in ["off", "OFF", " Off "] {
        assert_eq!(AaMode::parse_flag(value), Ok(AaMode::Off), "{value:?}");
    }

    for (value, expected) in [
        ("0", AaMode::Off),
        ("1", AaMode::Off),
        ("2", AaMode::X2),
        ("4", AaMode::X4),
    ] {
        assert_eq!(AaMode::parse_flag(value), Ok(expected), "{value:?}");
    }
}

#[test]
fn the_cli_flag_rejects_unsupported_multipliers_and_words() {
    for value in ["3", "5", "8", "16"] {
        let error = AaMode::parse_flag(value).expect_err("unsupported box");

        assert_eq!(
            error,
            AaModeError::UnsupportedFactor(value.parse().expect("a number")),
            "{value}"
        );
        assert!(!error.to_string().is_empty());
    }

    for value in ["", "two", "x2", "-2", "2x"] {
        let error = AaMode::parse_flag(value).expect_err("not a value");

        assert!(matches!(error, AaModeError::InvalidFlag(_)), "{value:?}");
    }
}
