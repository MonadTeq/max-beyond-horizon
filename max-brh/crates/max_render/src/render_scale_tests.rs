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
// AaMode (GPU anti-aliasing, gpu-aa-design.md)
// ---------------------------------------------------------------------------

#[test]
fn antialiasing_is_off_by_default() {
    let aa = AaMode::default();

    assert_eq!(aa, AaMode::Off);
    assert!(aa.is_off());
    assert!(!aa.is_on());
    assert_eq!(aa.label(), "off");
    assert_eq!(aa.menu_label(), "Off");
}

#[test]
fn the_owner_decision_modes_are_off_fxaa_smaa() {
    // Owner decision 2026-11-27: SSAA is replaced by a GPU AA mode, and the
    // flag is a clean break (`off`/`fxaa`/`smaa`).
    assert_eq!(AaMode::ALL, [AaMode::Off, AaMode::Fxaa, AaMode::Smaa]);
    assert!(AaMode::Fxaa.is_on());
    assert!(AaMode::Smaa.is_on());
    assert_eq!(AaMode::Fxaa.label(), "fxaa");
    assert_eq!(AaMode::Smaa.label(), "smaa");
    assert_eq!(AaMode::Fxaa.menu_label(), "FXAA");
    assert_eq!(AaMode::Smaa.menu_label(), "SMAA");
}

#[test]
fn the_cli_flag_accepts_the_three_names_case_insensitively() {
    for (value, expected) in [
        ("off", AaMode::Off),
        ("OFF", AaMode::Off),
        (" Off ", AaMode::Off),
        ("fxaa", AaMode::Fxaa),
        ("FXAA", AaMode::Fxaa),
        ("Fxaa", AaMode::Fxaa),
        ("smaa", AaMode::Smaa),
        ("SMAA", AaMode::Smaa),
    ] {
        assert_eq!(AaMode::parse_flag(value), Ok(expected), "{value:?}");
    }
}

#[test]
fn the_cli_flag_rejects_the_retired_multipliers() {
    // The SSAA multipliers are a clean break, not a silent alias.
    for value in ["0", "1", "2", "4", "3", "8", "16"] {
        let error = AaMode::parse_flag(value).expect_err("a retired multiplier");

        assert_eq!(
            error,
            AaModeError::InvalidFlag(value.to_string()),
            "{value}"
        );
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn the_cli_flag_rejects_unknown_words() {
    for value in ["", " ", "two", "aa-2x", "x2", "-2", "2x", "fxaa2"] {
        let error = AaMode::parse_flag(value).expect_err("not a mode");

        assert!(matches!(error, AaModeError::InvalidFlag(_)), "{value:?}");
    }
}

#[test]
fn the_flag_error_names_the_three_modes() {
    let message = AaMode::parse_flag("2")
        .expect_err("a retired multiplier")
        .to_string();

    assert!(message.contains("off"), "{message}");
    assert!(message.contains("fxaa"), "{message}");
    assert!(message.contains("smaa"), "{message}");
}
