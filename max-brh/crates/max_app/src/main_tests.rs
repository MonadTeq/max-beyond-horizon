use super::*;

/// Parsing without `std::env::args` and without checking the `sdl` feature.
fn parse(args: &[&str]) -> Result<Cli> {
    Cli::parse_from(args.iter().map(|arg| (*arg).to_string()))
}

#[test]
fn no_flags_selects_gui_mode() {
    let cli = parse(&[]).expect("пустой список аргументов разбирается");

    assert_eq!(cli.mode, Mode::Ui);
    assert_eq!(cli.config.mode, AppMode::Ui);
}

#[test]
fn ui_flag_is_a_compatible_alias_for_the_default() {
    let cli = parse(&["--ui"]).expect("--ui разбирается");

    assert_eq!(cli.mode, Mode::Ui);
    assert_eq!(cli.config.mode, AppMode::Ui);
}

#[test]
fn fullscreen_and_fps_flags_are_parsed() {
    let cli = parse(&["--ui", "--fullscreen", "--fps", "--scale", "2"])
        .expect("--fullscreen/--fps разбираются");

    assert!(cli.fullscreen);
    assert!(cli.fps_stats);
    assert_eq!(cli.window_scale, Some(2));

    // Defaults: windowed, no presentation statistics.
    let cli = parse(&[]).expect("пустой список аргументов разбирается");
    assert!(!cli.fullscreen);
    assert!(!cli.fps_stats);
}

/// Debug observation mode: `--no-fow` is a flag, off by default, and it does
/// not change any other mode.
#[test]
fn no_fow_flag_is_parsed() {
    let cli = parse(&["--no-fow"]).expect("--no-fow разбирается");

    assert!(cli.no_fow);
    assert_eq!(cli.mode, Mode::Ui, "the flag keeps the interactive mode");

    let cli = parse(&[]).expect("пустой список аргументов разбирается");
    assert!(!cli.no_fow);
}

/// R7 presentation switch: `--hd` / `--hd-scale N` / `--remaster-dir DIR`.
///
/// The default is pixel-perfect, `--hd` alone is the R0-recommended `S = 2`,
/// `--hd-scale` implies `--hd`, and `--hd-scale 1` is the historical pipeline
/// through the same switch. An unsupported multiplier is a launch error: an
/// asset tag exists only for the shipped `×2`/`×4`.
#[test]
fn hd_flags_select_the_presentation_mode() {
    use max_render::render_scale::{PresentationMode, RenderScale};

    let cli = parse(&[]).expect("пустой список аргументов разбирается");
    assert!(!cli.hd);
    assert_eq!(cli.hd_scale, None);
    assert_eq!(
        cli.presentation().expect("the default mode resolves"),
        (PresentationMode::PixelPerfect, RenderScale::PixelPerfect)
    );

    let cli = parse(&["--hd"]).expect("--hd разбирается");
    assert!(cli.hd);
    assert_eq!(
        cli.presentation().expect("--hd resolves"),
        (PresentationMode::Hd, RenderScale::Hd2),
        "the default HD multiplier is the R0 recommendation"
    );

    for (value, expected) in [("2", RenderScale::Hd2), ("4", RenderScale::Hd4)] {
        let cli = parse(&["--hd", "--hd-scale", value]).expect("--hd-scale разбирается");

        assert_eq!(cli.hd_scale, Some(expected.scale()));
        assert_eq!(
            cli.presentation().expect("--hd-scale resolves"),
            (PresentationMode::Hd, expected)
        );
        // `--hd-scale` implies `--hd`.
        let cli = parse(&["--hd-scale", value]).expect("--hd-scale без --hd разбирается");
        assert_eq!(
            cli.presentation().expect("--hd-scale implies --hd"),
            (PresentationMode::Hd, expected)
        );
    }

    // `S = 1` is the historical pipeline via the same switch.
    let cli = parse(&["--hd", "--hd-scale", "1"]).expect("--hd-scale 1 разбирается");
    assert_eq!(
        cli.presentation().expect("S=1 resolves"),
        (PresentationMode::PixelPerfect, RenderScale::PixelPerfect)
    );

    // Any other multiplier is rejected at parse time.
    for value in ["0", "3", "8"] {
        assert!(
            parse(&["--hd", "--hd-scale", value]).is_err(),
            "--hd-scale {value} must be rejected"
        );
    }

    let cli = parse(&["--hd", "--remaster-dir", "D:/hd"]).expect("--remaster-dir разбирается");
    assert_eq!(
        cli.remaster_dir.as_deref(),
        Some(std::path::Path::new("D:/hd"))
    );

    // The switch belongs to the GUI window: headless modes and the direct
    // `--connect --sdl` window reject it instead of silently ignoring it.
    assert!(parse(&["--demo", "save.cam", "--hd"]).is_err());
    assert!(parse(&["--new-game", "--hd"]).is_err());
    assert!(parse(&["--host", "--hd"]).is_err());
    assert!(parse(&["--connect", "127.0.0.1:8082", "--join", "game", "--hd"]).is_err());
}

#[test]
fn demo_flag_selects_headless_demo() {
    let cli =
        parse(&["--demo", "max-original/SAVE1.CAM", "--turns", "7"]).expect("--demo разбирается");

    assert_eq!(cli.mode, Mode::Demo);
    assert_eq!(cli.config.mode, AppMode::Demo);
    assert_eq!(cli.turns, Some(7));
    assert_eq!(
        cli.save.as_deref(),
        Some(std::path::Path::new("max-original/SAVE1.CAM"))
    );
}

#[test]
fn new_game_flag_selects_headless_new_game() {
    let cli = parse(&["--new-game", "--world", "12"]).expect("--new-game разбирается");

    assert_eq!(cli.mode, Mode::NewGame);
    assert_eq!(cli.config.mode, AppMode::NewGame);
}

#[test]
fn connect_flag_selects_headless_connect() {
    let cli =
        parse(&["--connect", "127.0.0.1:8082", "--join", "demo"]).expect("--connect разбирается");

    assert_eq!(cli.mode, Mode::Connect);
    assert_eq!(cli.config.mode, AppMode::Connect);
    assert_eq!(cli.config.horizon_host, "127.0.0.1");
    assert_eq!(cli.config.horizon_port, 8082);
    assert_eq!(cli.game_id.as_deref(), Some("demo"));
    assert!(cli.endpoint.is_none(), "с --join это режим Horizon");
    assert!(!cli.sdl, "без --sdl сессия остаётся headless");
}

#[test]
fn connect_without_join_is_a_direct_host_endpoint() {
    let cli = parse(&["--connect", "127.0.0.1:31554"]).expect("прямой --connect");

    assert_eq!(cli.mode, Mode::Connect);
    assert_eq!(cli.game_id, None);
    assert!(!cli.sdl);
    let endpoint = cli.endpoint.as_ref().expect("прямой endpoint");
    assert_eq!(endpoint.host, "127.0.0.1");
    assert_eq!(endpoint.port, 31554);
}

#[test]
fn direct_connect_defaults_to_the_host_port() {
    let cli = parse(&["--connect", "127.0.0.1"]).expect("прямой --connect без порта");

    assert_eq!(cli.endpoint.expect("endpoint").port, DEFAULT_HOST_PORT);
}

#[test]
fn host_flag_selects_the_server_lobby() {
    let cli = parse(&[
        "--host",
        "--listen",
        "127.0.0.1",
        "--host-port",
        "31554",
        "--min-players",
        "3",
        "--host-policy",
        "eliminate",
    ])
    .expect("--host разбирается");

    assert_eq!(cli.mode, Mode::Host);
    assert_eq!(cli.config.mode, AppMode::Host);
    assert_eq!(cli.listen, Some(Ipv4Addr::LOCALHOST));
    assert_eq!(cli.host_port, Some(31554));
    assert_eq!(cli.min_players, Some(3));
    assert_eq!(
        cli.host_policy,
        Some(SessionPolicy::EliminateAfter {
            ticks: max_app::host::DEFAULT_ELIMINATION_TICKS
        })
    );
    assert!(cli.endpoint.is_none());
}

#[test]
fn host_flags_require_the_host_mode() {
    let error = parse(&["--demo", "save.cam", "--min-players", "3"])
        .expect_err("--min-players без --host отклоняется");

    assert!(error.to_string().contains("--min-players"), "{error:#}");
}

#[test]
fn host_and_connect_are_mutually_exclusive() {
    let error = parse(&["--host", "--connect", "127.0.0.1:31554"])
        .expect_err("--host + --connect отклоняется");

    assert!(error.to_string().contains("--host"), "{error:#}");
}

#[test]
fn host_policy_rejects_unknown_values() {
    let error = parse(&["--host", "--host-policy", "banish"])
        .expect_err("неизвестная политика отклоняется");

    assert!(error.to_string().contains("pause"), "{error:#}");
}

#[test]
fn direct_connect_rejects_sdl() {
    let error = parse(&["--connect", "127.0.0.1:31554", "--sdl"])
        .expect_err("--sdl для прямого --connect отклоняется");

    assert!(error.to_string().contains("--sdl"), "{error:#}");
}

#[test]
fn record_is_accepted_in_the_default_gui_mode() {
    let cli = parse(&["--record", "run.qa"]).expect("--record в GUI разбирается");

    assert_eq!(cli.mode, Mode::Ui);
    assert_eq!(cli.record.as_deref(), Some(std::path::Path::new("run.qa")));
}

/// `--settings PATH` names the `settings.jsonc` read at launch; the default is
/// the file in the working/repository directory (`None` here).
#[test]
fn settings_flag_selects_the_configuration_file() {
    let cli = parse(&["--settings", "D:/cfg/settings.jsonc"]).expect("--settings разбирается");

    assert_eq!(
        cli.settings.as_deref(),
        Some(std::path::Path::new("D:/cfg/settings.jsonc"))
    );

    let cli = parse(&[]).expect("пустой список аргументов разбирается");
    assert_eq!(cli.settings, None);
}

/// `settings.jsonc` supplies the presentation the CLI did not name; the CLI
/// flags replace the same parameter (`CLI > file > default`).
#[test]
fn settings_file_fills_in_the_presentation_unless_the_cli_overrides_it() {
    use max_app::settings::Settings;
    use max_render::render_scale::{PresentationMode, RenderScale};

    let file = Settings::parse(r#"{ "display": { "scale": 4 } }"#).expect("settings parse");

    let cli = parse(&[]).expect("пустой список аргументов разбирается");
    let resolved = cli.presentation_settings(&file).expect("the file resolves");

    assert_eq!(resolved.render_scale, RenderScale::Hd4);
    assert_eq!(resolved.mode, PresentationMode::Hd);

    let cli = parse(&["--hd-scale", "2"]).expect("--hd-scale разбирается");
    let resolved = cli
        .presentation_settings(&file)
        .expect("the CLI overrides the file");

    assert_eq!(resolved.render_scale, RenderScale::Hd2);

    // No file and no CLI flags: the byte-for-byte default.
    let cli = parse(&[]).expect("пустой список аргументов разбирается");
    let resolved = cli
        .presentation_settings(&Settings::default())
        .expect("the default resolves");

    assert_eq!(resolved.render_scale, RenderScale::PixelPerfect);
    assert_eq!(resolved.mode, PresentationMode::PixelPerfect);
}

/// SSAA switch (`ssaa-design.md`): `--aa 0|off|1|2|4`; the multiplier is
/// presentation-only and belongs to the GUI window, like `--hd`.
#[test]
fn aa_flag_accepts_off_and_the_shipped_multipliers() {
    use max_render::render_scale::AaMode;

    let cli = parse(&[]).expect("пустой список аргументов разбирается");
    assert_eq!(cli.aa, None);

    for (value, expected) in [
        ("0", AaMode::Off),
        ("off", AaMode::Off),
        ("OFF", AaMode::Off),
        ("1", AaMode::Off),
        ("2", AaMode::X2),
        ("4", AaMode::X4),
    ] {
        let cli = parse(&["--aa", value]).expect("--aa разбирается");

        assert_eq!(cli.aa, Some(expected), "--aa {value}");
        assert_eq!(cli.mode, Mode::Ui, "the flag keeps the interactive mode");
    }

    // The CLI value replaces the file value (`CLI > file`).
    let file = max_app::settings::Settings::parse(r#"{ "display": { "scale": 2, "aa": 2 } }"#)
        .expect("settings parse");
    let cli = parse(&["--aa", "4"]).expect("--aa разбирается");
    let resolved = cli.presentation_settings(&file).expect("resolves");

    assert_eq!(resolved.aa, AaMode::X4);
    assert_eq!(resolved.render_factor(), 8, "S=2 x aa=4");
}

#[test]
fn aa_flag_rejects_unsupported_multipliers_and_headless_modes() {
    for value in ["3", "8", "16", "x2", "two"] {
        let error = parse(&["--aa", value]).expect_err("неподдерживаемое значение");

        assert!(error.to_string().contains("--aa"), "{error:#}");
    }

    assert!(parse(&["--aa"]).is_err(), "--aa без значения");

    // Like `--hd`, the switch belongs to the GUI window: the headless modes and
    // the direct `--connect --sdl` window would silently ignore it.
    assert!(parse(&["--demo", "save.cam", "--aa", "2"]).is_err());
    assert!(parse(&["--new-game", "--aa", "2"]).is_err());
    assert!(parse(&["--host", "--aa", "2"]).is_err());
    assert!(parse(&["--connect", "127.0.0.1:8082", "--join", "g", "--aa", "2"]).is_err());
}

#[test]
fn record_is_rejected_in_headless_modes() {
    let error = parse(&["--demo", "save.cam", "--record", "run.qa"])
        .expect_err("--record с --demo должен отклоняться");

    assert!(error.to_string().contains("--record"), "{error:#}");
}

#[test]
fn sdl_still_requires_connect() {
    let error = parse(&["--sdl"]).expect_err("--sdl без --connect отклоняется");

    assert!(error.to_string().contains("--connect"), "{error:#}");
}

#[test]
fn unknown_flag_is_rejected() {
    let error = parse(&["--bogus"]).expect_err("неизвестный флаг отклоняется");

    assert!(error.to_string().contains("--bogus"), "{error:#}");
}

#[cfg(not(feature = "sdl"))]
#[test]
fn gui_without_sdl_feature_reports_a_build_hint() {
    let cli = parse(&[]).expect("аргументы разбираются без SDL");

    let error = cli
        .ensure_gui_available()
        .expect_err("без feature sdl GUI недоступен");
    let text = format!("{error:#}");

    assert!(text.contains("feature sdl"), "{text}");
    assert!(text.contains("--demo"), "{text}");
    assert!(text.contains("--new-game"), "{text}");
    assert!(text.contains("--connect"), "{text}");
}

#[cfg(feature = "sdl")]
#[test]
fn gui_is_available_with_sdl_feature() {
    parse(&[])
        .expect("аргументы разбираются")
        .ensure_gui_available()
        .expect("с feature sdl GUI доступен");
}
