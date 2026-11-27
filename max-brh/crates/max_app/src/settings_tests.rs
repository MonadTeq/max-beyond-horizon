use super::*;

/// A unique temporary path (no `tempfile` dependency in the workspace).
fn temp_path(tag: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    std::env::temp_dir().join(format!(
        "max_brh_settings_{tag}_{}_{stamp}.jsonc",
        std::process::id()
    ))
}

fn parse(text: &str) -> Settings {
    Settings::parse(text).expect("settings.jsonc must parse")
}

#[test]
fn default_path_is_the_documented_file_name() {
    assert_eq!(default_path(), PathBuf::from("settings.jsonc"));
    assert_eq!(SETTINGS_FILE_NAME, "settings.jsonc");
}

#[test]
fn empty_or_comment_only_files_are_the_defaults() {
    for text in ["", "   \n", "// only a comment\n", "/* block */\n"] {
        assert_eq!(parse(text), Settings::default(), "text = {text:?}");
    }

    assert_eq!(parse("{}").render_scale(), RenderScale::PixelPerfect);
}

/// The file shape of the repository template: sections, comments (Russian is
/// allowed in a config file), a commented-out alternative and trailing commas.
#[test]
fn reads_the_documented_sections_with_comments_and_trailing_commas() {
    let text = r#"
{
  // Раздел display: разрешение окна (только презентация).
  "display": {
    "scale": 2, // 1 = 640x480, 2 = 1280x960, 4 = 2560x1920
    // "resolution": "640x480",
  },
  /* Раздел window: масштаб окна и полный экран. */
  "window": {
    "scale": 0, // 0 — подобрать под рабочий стол
    "fullscreen": false,
  },
}
"#;
    let settings = parse(text);

    assert_eq!(settings.display.scale, Some(RenderScale::Hd2));
    assert_eq!(settings.display.resolution, None);
    assert_eq!(settings.window.scale, None, "0 is 'fit the desktop'");
    assert_eq!(settings.window.fullscreen, Some(false));
    assert_eq!(settings.render_scale(), RenderScale::Hd2);
}

#[test]
fn resolution_is_an_alias_of_scale() {
    let settings = parse(r#"{ "display": { "resolution": "2560x1920" } }"#);

    assert_eq!(settings.render_scale(), RenderScale::Hd4);
    assert_eq!(settings.display.scale, None);
    assert_eq!(settings.display.resolution, Some(RenderScale::Hd4));

    // Surrounding whitespace and the uppercase separator are accepted.
    assert_eq!(
        parse(r#"{ "display": { "resolution": " 1280X960 " } }"#).render_scale(),
        RenderScale::Hd2
    );
}

#[test]
fn both_aliases_with_the_same_value_are_accepted() {
    let settings = parse(r#"{ "display": { "scale": 4, "resolution": "2560x1920" } }"#);

    assert_eq!(settings.render_scale(), RenderScale::Hd4);
}

#[test]
fn unsupported_scales_are_rejected() {
    for value in [0, 3, 5, 8] {
        let error = Settings::parse(&format!(r#"{{ "display": {{ "scale": {value} }} }}"#))
            .expect_err("an unsupported display.scale is an error");

        assert!(
            matches!(error, SettingsError::UnsupportedScale { value: got } if got == value),
            "{error}"
        );
        assert!(error.to_string().contains("640x480"), "{error}");
    }
}

#[test]
fn unsupported_resolutions_are_rejected() {
    for value in ["1024x768", "1280x1024", "640", "abc", "1280x"] {
        let error = Settings::parse(&format!(
            r#"{{ "display": {{ "resolution": "{value}" }} }}"#
        ))
        .expect_err("an unsupported display.resolution is an error");

        assert!(
            matches!(&error, SettingsError::UnsupportedResolution { value: got } if got == value),
            "{error}"
        );
    }
}

#[test]
fn conflicting_display_aliases_are_rejected() {
    let error = Settings::parse(r#"{ "display": { "scale": 2, "resolution": "2560x1920" } }"#)
        .expect_err("different aliases must not select two resolutions");

    assert!(
        matches!(error, SettingsError::ConflictingDisplay { scale: 2, .. }),
        "{error}"
    );
    assert!(error.to_string().contains("2560x1920"), "{error}");
}

#[test]
fn window_scale_is_bounded_and_zero_means_auto() {
    assert_eq!(parse(r#"{ "window": { "scale": 0 } }"#).window.scale, None);
    assert_eq!(
        parse(r#"{ "window": { "scale": 8 } }"#).window.scale,
        Some(8)
    );

    let error = parse_result(r#"{ "window": { "scale": 9 } }"#).expect_err("9 is out of range");

    assert!(
        matches!(error, SettingsError::WindowScale { value: 9, max: 8 }),
        "{error}"
    );
}

fn parse_result(text: &str) -> Result<Settings, SettingsError> {
    Settings::parse(text)
}

#[test]
fn unknown_keys_are_rejected_instead_of_silently_ignored() {
    for text in [
        r#"{ "display": { "scael": 2 } }"#,
        r#"{ "displays": { "scale": 2 } }"#,
        r#"{ "window": { "fullscren": true } }"#,
    ] {
        let error = Settings::parse(text).expect_err("a typo must not fall back to a default");

        assert!(matches!(error, SettingsError::Syntax { .. }), "{error}");
    }
}

/// Comments are replaced by spaces, so the `serde_json` position points into
/// the original file (the comment line is preserved).
#[test]
fn syntax_errors_report_the_original_line() {
    let error = Settings::parse(
        "{\n  // a comment on line 2\n  \"display\": { \"scale\": 2 },\n  broken\n}\n",
    )
    .expect_err("malformed JSON must be reported");

    match error {
        SettingsError::Syntax { line, .. } => assert_eq!(line, 4),
        other => panic!("expected a syntax error, got {other}"),
    }
}

#[test]
fn comment_markers_inside_strings_are_values_not_comments() {
    let text = r#"{ "future": "http://example.test/a//b" }"#;
    let stripped = strip_jsonc(text);

    assert!(stripped.contains("http://example.test/a//b"), "{stripped}");
    // Unknown top-level sections are rejected, but the string survived intact.
    assert!(serde_json::from_str::<serde_json::Value>(&stripped).is_ok());
}

#[test]
fn a_utf8_bom_is_accepted() {
    let settings = parse("\u{feff}{ \"display\": { \"scale\": 4 } }");

    assert_eq!(settings.render_scale(), RenderScale::Hd4);
}

#[test]
fn load_missing_file_is_none() {
    let path = temp_path("missing");

    assert_eq!(
        Settings::load(&path).expect("missing file is not an error"),
        None
    );
    assert_eq!(
        Settings::load_or_default(&path).expect("defaults"),
        Settings::default()
    );
}

#[test]
fn load_reads_a_file_from_disk() {
    let path = temp_path("disk");
    std::fs::write(&path, "{\n  // 4x\n  \"display\": { \"scale\": 4 },\n}\n")
        .expect("write the temporary settings file");

    let settings = Settings::load(&path)
        .expect("the file parses")
        .expect("the file exists");

    assert_eq!(settings.render_scale(), RenderScale::Hd4);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn cli_presentation_overrides_the_file() {
    let file = parse(r#"{ "display": { "scale": 2 } }"#);

    // No CLI presentation flag: the file applies.
    let resolved = resolve(CliPresentation::default(), &file).expect("resolve");
    assert_eq!(resolved.render_scale, RenderScale::Hd2);
    assert_eq!(resolved.mode, PresentationMode::Hd);

    // `--hd-scale 4` wins over `display.scale = 2`.
    let resolved = resolve(
        CliPresentation {
            hd_scale: Some(4),
            ..CliPresentation::default()
        },
        &file,
    )
    .expect("resolve");
    assert_eq!(resolved.render_scale, RenderScale::Hd4);

    // `--hd` alone is the R0 default (`S = 2`), not the file's value.
    let file4 = parse(r#"{ "display": { "scale": 4 } }"#);
    let resolved = resolve(
        CliPresentation {
            hd: true,
            ..CliPresentation::default()
        },
        &file4,
    )
    .expect("resolve");
    assert_eq!(resolved.render_scale, RenderScale::Hd2);

    // `--hd-scale 1` is pixel-perfect through the same switch.
    let resolved = resolve(
        CliPresentation {
            hd_scale: Some(1),
            ..CliPresentation::default()
        },
        &file4,
    )
    .expect("resolve");
    assert_eq!(resolved.render_scale, RenderScale::PixelPerfect);
    assert_eq!(resolved.mode, PresentationMode::PixelPerfect);
}

#[test]
fn defaults_are_pixel_perfect_when_neither_cli_nor_file_names_a_resolution() {
    let resolved = resolve(CliPresentation::default(), &Settings::default()).expect("resolve");

    assert_eq!(resolved.render_scale, RenderScale::PixelPerfect);
    assert_eq!(resolved.mode, PresentationMode::PixelPerfect);
    assert_eq!(resolved.window_scale, None);
    assert!(!resolved.fullscreen);
}

#[test]
fn window_scale_and_fullscreen_follow_the_same_priority() {
    let file = parse(r#"{ "window": { "scale": 3, "fullscreen": true } }"#);

    let resolved = resolve(CliPresentation::default(), &file).expect("resolve");
    assert_eq!(resolved.window_scale, Some(3));
    assert!(resolved.fullscreen);

    // `--scale` wins; `--scale 0` means "fit the desktop" and overrides the file.
    let resolved = resolve(
        CliPresentation {
            window_scale: Some(2),
            ..CliPresentation::default()
        },
        &file,
    )
    .expect("resolve");
    assert_eq!(resolved.window_scale, Some(2));

    let resolved = resolve(
        CliPresentation {
            window_scale: Some(0),
            ..CliPresentation::default()
        },
        &file,
    )
    .expect("resolve");
    assert_eq!(resolved.window_scale, None);

    // A bare `--fullscreen` can only enable it; the file cannot turn it off.
    let file_off = parse(r#"{ "window": { "fullscreen": false } }"#);
    let resolved = resolve(
        CliPresentation {
            fullscreen: true,
            ..CliPresentation::default()
        },
        &file_off,
    )
    .expect("resolve");
    assert!(resolved.fullscreen);
}

#[test]
fn invalid_cli_scale_is_reported_by_the_presentation_switch() {
    let error = resolve(
        CliPresentation {
            hd_scale: Some(3),
            ..CliPresentation::default()
        },
        &Settings::default(),
    )
    .expect_err("3 is not a shipped scale");

    assert!(matches!(
        error,
        SettingsError::Presentation(PresentationModeError::UnsupportedScale(3))
    ));
}

// ---------------------------------------------------------------------------
// display.aa — GPU anti-aliasing (gpu-aa-design.md)
// ---------------------------------------------------------------------------

#[test]
fn the_display_aa_value_is_parsed_and_defaults_to_off() {
    use max_render::render_scale::AaMode;

    assert_eq!(Settings::default().display.aa, None);
    assert_eq!(
        parse("{}").display.aa,
        None,
        "a file without aa keeps the built-in default"
    );

    for (value, expected) in [
        ("off", AaMode::Off),
        ("fxaa", AaMode::Fxaa),
        ("smaa", AaMode::Smaa),
        ("FXAA", AaMode::Fxaa),
    ] {
        let settings = parse(&format!(r#"{{ "display": {{ "aa": "{value}" }} }}"#));

        assert_eq!(settings.display.aa, Some(expected), "display.aa = {value}");
    }
}

#[test]
fn unsupported_display_aa_values_are_rejected() {
    // The retired SSAA multipliers and unknown words are launch errors: `--aa`
    // is a clean break, so a number must not silently mean something else.
    for value in ["2", "4", "0", "1", "fast", "aa-2x"] {
        let text = if value.chars().all(|c| c.is_ascii_digit()) {
            format!(r#"{{ "display": {{ "aa": {value} }} }}"#)
        } else {
            format!(r#"{{ "display": {{ "aa": "{value}" }} }}"#)
        };
        let error = Settings::parse(&text).expect_err("an unsupported aa mode is an error");

        assert!(
            matches!(&error, SettingsError::UnsupportedAa { value: got } if *got == value),
            "{value}: {error}"
        );
        assert!(error.to_string().contains("fxaa"), "{error}");
    }
}

#[test]
fn antialiasing_follows_cli_then_the_file() {
    use max_render::render_scale::{AaMode, RenderScale};

    // The file at S=2 with aa=fxaa.
    let file = parse(r#"{ "display": { "scale": 2, "aa": "fxaa" } }"#);
    let resolved = resolve(CliPresentation::default(), &file).expect("resolve");
    assert_eq!(resolved.render_scale, RenderScale::Hd2);
    assert_eq!(resolved.aa, AaMode::Fxaa);

    // `--aa smaa` wins over the file, `--aa off` disables it.
    let resolved = resolve(
        CliPresentation {
            aa: Some(AaMode::Smaa),
            ..CliPresentation::default()
        },
        &file,
    )
    .expect("resolve");
    assert_eq!(resolved.aa, AaMode::Smaa);

    let resolved = resolve(
        CliPresentation {
            aa: Some(AaMode::Off),
            ..CliPresentation::default()
        },
        &file,
    )
    .expect("resolve");
    assert_eq!(resolved.aa, AaMode::Off);

    // A file without aa and no flag is off.
    let resolved = resolve(
        CliPresentation::default(),
        &parse(r#"{ "display": { "scale": 4 } }"#),
    )
    .expect("resolve");
    assert_eq!(resolved.aa, AaMode::Off);
}

/// `S = 1` forces the anti-aliasing off, whatever the CLI or the file says: the
/// pixel-perfect frame and its golden stay byte-for-byte.
#[test]
fn pixel_perfect_forces_antialiasing_off() {
    use max_render::render_scale::AaMode;

    let file = parse(r#"{ "display": { "scale": 1, "aa": "fxaa" } }"#);
    let resolved = resolve(CliPresentation::default(), &file).expect("resolve");

    assert_eq!(resolved.render_scale, RenderScale::PixelPerfect);
    assert_eq!(resolved.aa, AaMode::Off, "S=1 must ignore display.aa");
    assert_eq!(resolved.render_scale.scale(), 1);

    // A CLI `--aa fxaa` cannot turn it on either.
    let resolved = resolve(
        CliPresentation {
            aa: Some(AaMode::Fxaa),
            ..CliPresentation::default()
        },
        &Settings::default(),
    )
    .expect("resolve");
    assert_eq!(resolved.aa, AaMode::Off);
}

/// Anti-aliasing is an output-layer parameter: it never multiplies the render
/// frame, so the frame size of a presentation setting is only `S`.
#[test]
fn antialiasing_never_changes_the_render_scale() {
    use max_render::render_scale::AaMode;

    for (aa, expected) in [
        (AaMode::Off, (1280, 960)),
        (AaMode::Fxaa, (1280, 960)),
        (AaMode::Smaa, (1280, 960)),
    ] {
        let resolved = resolve(
            CliPresentation {
                hd: true,
                aa: Some(aa),
                ..CliPresentation::default()
            },
            &Settings::default(),
        )
        .expect("resolve");

        assert_eq!(resolved.render_scale.render_size(), expected, "{aa:?}");
    }
}
