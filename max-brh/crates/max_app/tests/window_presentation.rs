//! Gate of the window resolution switch: **CLI**, **`settings.jsonc`** and the
//! game's **`Setup` dialog** (`SETUPPIC`, `max_app::preferences`) must select the
//! same render scale; a dialog choice is presentation only and must not touch
//! the simulation.
//!
//! The 1:1 golden path is not modified by any of this: the window shows the game
//! frame itself (the SDL window menu and its `640×496` chrome are gone), and the
//! render scale never reaches `App::render_frame` / `Screen::render` in
//! `RenderScale::PixelPerfect`.

use std::path::Path;

use max_app::preferences::{
    video_option_rect, Preferences, PreferencesAction, PreferencesDialog, PreferencesInfo,
    PreferencesVariant, VideoRow, WINDOW_ULX, WINDOW_ULY,
};
use max_app::settings::{resolve, CliPresentation, Settings, VideoChoice};
use max_app::ui::InputEvent;
use max_app::{App, AppConfig};
use max_render::presentation::window_points_for;
use max_render::render_scale::{AaMode, PresentationMode, RenderScale};

/// The three shipped resolutions in dialog order.
const RESOLUTIONS: [RenderScale; 3] = [
    RenderScale::PixelPerfect,
    RenderScale::Hd2,
    RenderScale::Hd4,
];

/// A fresh `Setup` dialog showing the given presentation.
fn setup_dialog(video: VideoChoice) -> PreferencesDialog {
    PreferencesDialog::new(
        PreferencesVariant::Setup,
        Preferences::default(),
        PreferencesInfo::default(),
    )
    .with_video(video)
}

fn click_option(dialog: &mut PreferencesDialog, row: VideoRow, index: usize) -> PreferencesAction {
    let rect = video_option_rect(row, index);

    dialog.input(InputEvent::MouseClick(
        WINDOW_ULX + rect.ulx + rect.width / 2,
        WINDOW_ULY + rect.uly + rect.height / 2,
    ))
}

/// `Resolution → <option>` through the real dialog hit tests.
fn choose_resolution_in_the_setup_dialog(scale: RenderScale) -> VideoChoice {
    let index = RESOLUTIONS
        .iter()
        .position(|candidate| *candidate == scale)
        .expect("a shipped resolution");
    // Start from another scale, so the click really changes the running value
    // (choosing the current option is inert by design).
    let current = if scale == RenderScale::PixelPerfect {
        VideoChoice::new(RenderScale::Hd2, AaMode::Off)
    } else {
        VideoChoice::default()
    };
    let mut dialog = setup_dialog(current);
    let action = click_option(&mut dialog, VideoRow::Resolution, index);

    match action {
        PreferencesAction::Video(choice) => {
            assert_eq!(
                dialog.video(),
                choice,
                "the dialog keeps showing the applied value"
            );

            choice
        }
        other => panic!("the dialog must report the video choice, got {other:?}"),
    }
}

/// The four ways of naming a resolution are equivalent: the same `S`, the same
/// render frame, the same window points and the same `PresentationSettings`.
#[test]
fn cli_jsonc_and_the_setup_dialog_select_the_same_resolution() {
    for scale in RESOLUTIONS {
        let cli = resolve(
            CliPresentation {
                hd_scale: Some(scale.scale()),
                ..CliPresentation::default()
            },
            &Settings::default(),
        )
        .expect("the CLI switch resolves");

        let file = Settings::parse(&format!(
            r#"{{ "display": {{ "scale": {} }} }}"#,
            scale.scale()
        ))
        .expect("the settings file parses");
        let from_file = resolve(CliPresentation::default(), &file).expect("the file resolves");

        // The string alias names the very same resolution.
        let (width, height) = scale.render_size();
        let alias = Settings::parse(&format!(
            r#"{{ "display": {{ "resolution": "{width}x{height}" }} }}"#
        ))
        .expect("the resolution alias parses");
        let from_alias = resolve(CliPresentation::default(), &alias).expect("the alias resolves");

        assert_eq!(cli.render_scale, scale, "CLI --hd-scale {scale:?}");
        assert_eq!(from_file.render_scale, scale, "settings.jsonc {scale:?}");
        assert_eq!(from_alias.render_scale, scale, "resolution alias {scale:?}");
        assert_eq!(
            choose_resolution_in_the_setup_dialog(scale).render_scale,
            scale,
            "Setup dialog {scale:?}"
        );

        assert_eq!(
            cli.mode,
            if scale.is_hd() {
                PresentationMode::Hd
            } else {
                PresentationMode::PixelPerfect
            }
        );
        assert_eq!(from_file.mode, cli.mode);
        assert_eq!(
            window_points_for(640, 480, from_file.render_scale, 1),
            window_points_for(640, 480, cli.render_scale, 1),
            "the same render scale must open the same window (no extra chrome)"
        );
    }
}

/// A dialog choice is the same `PresentationSettings` pair the launch resolver
/// produces for the same scale and mode (the window size/fullscreen are not part
/// of the dialog).
#[test]
fn the_dialog_choice_matches_the_resolved_presentation() {
    for scale in RESOLUTIONS {
        let resolved = resolve(
            CliPresentation {
                hd_scale: Some(scale.scale()),
                ..CliPresentation::default()
            },
            &Settings::default(),
        )
        .expect("resolve");

        let choice = choose_resolution_in_the_setup_dialog(scale);
        let presentation = choice.presentation();

        assert_eq!(presentation.render_scale, resolved.render_scale);
        assert_eq!(presentation.mode, resolved.mode);
        // `S = 1` forces anti-aliasing off in both paths.
        assert_eq!(presentation.aa, resolved.aa);
        assert_eq!(presentation.aa, AaMode::Off);
    }
}

/// The anti-aliasing row follows the output-layer rule: it is inert at `S = 1`
/// and reports `Off`/`FXAA`/`SMAA` in HD.
#[test]
fn the_antialiasing_row_needs_hd_and_reports_the_choice() {
    let mut pixel_perfect = setup_dialog(VideoChoice::new(RenderScale::PixelPerfect, AaMode::Off));
    assert_eq!(
        click_option(&mut pixel_perfect, VideoRow::AntiAliasing, 1),
        PreferencesAction::None,
        "S = 1 keeps aa = Off (golden 1:1 byte-for-byte)"
    );

    let mut hd = setup_dialog(VideoChoice::new(RenderScale::Hd2, AaMode::Off));
    assert_eq!(
        click_option(&mut hd, VideoRow::AntiAliasing, 1),
        PreferencesAction::Video(VideoChoice::new(RenderScale::Hd2, AaMode::Fxaa))
    );
    assert_eq!(hd.video().aa, AaMode::Fxaa);
    assert_eq!(
        click_option(&mut hd, VideoRow::AntiAliasing, 2),
        PreferencesAction::Video(VideoChoice::new(RenderScale::Hd2, AaMode::Smaa))
    );

    // Back to 1:1: the mode is dropped with the HD geometry.
    assert_eq!(
        click_option(&mut hd, VideoRow::Resolution, 0),
        PreferencesAction::Video(VideoChoice::new(RenderScale::PixelPerfect, AaMode::Off))
    );
    assert_eq!(hd.video().aa, AaMode::Off);
}

/// The precedence is enforced end to end: a CLI presentation flag replaces the
/// file value for the same parameter; the file fills in the parameters the CLI
/// did not name. A dialog choice is a *runtime* override, not a source for the
/// next launch — it never rewrites `settings.jsonc`.
#[test]
fn cli_overrides_the_file_and_the_file_overrides_the_default() {
    let file = Settings::parse(r#"{ "display": { "scale": 4 } }"#).expect("parse");

    let default = resolve(CliPresentation::default(), &Settings::default()).expect("resolve");
    assert_eq!(default.render_scale, RenderScale::PixelPerfect);

    let from_file = resolve(CliPresentation::default(), &file).expect("resolve");
    assert_eq!(from_file.render_scale, RenderScale::Hd4);

    let overridden = resolve(
        CliPresentation {
            hd_scale: Some(2),
            ..CliPresentation::default()
        },
        &file,
    )
    .expect("resolve");
    assert_eq!(overridden.render_scale, RenderScale::Hd2);

    // `--hd-scale 1` is the pixel-perfect mode through the same switch.
    let pixel_perfect = resolve(
        CliPresentation {
            hd_scale: Some(1),
            ..CliPresentation::default()
        },
        &file,
    )
    .expect("resolve");
    assert_eq!(pixel_perfect.render_scale, RenderScale::PixelPerfect);
}

/// The dialog action is pure presentation state: applying it must not change the
/// application/session state (the snapshot is the determinism lock).
#[test]
fn a_setup_video_choice_does_not_touch_the_simulation() {
    let app = App::new(AppConfig::default());
    let before = app.state_ref().snapshot_bytes();

    for scale in RESOLUTIONS {
        let choice = choose_resolution_in_the_setup_dialog(scale);

        assert_eq!(choice.render_scale, scale);
        assert_eq!(
            app.state_ref().snapshot_bytes(),
            before,
            "choosing {scale:?} is presentation only: the snapshot must not change"
        );
    }
}

/// The rows name exactly the resolutions and the anti-aliasing modes the
/// requirement names.
#[test]
fn the_setup_dialog_names_the_five_settings() {
    let captions: Vec<String> = (0..3)
        .map(|index| VideoRow::Resolution.option_caption(index))
        .collect();
    assert_eq!(captions, ["640x480", "1280x960", "2560x1920"]);

    let aa: Vec<String> = (0..3)
        .map(|index| VideoRow::AntiAliasing.option_caption(index))
        .collect();
    assert_eq!(aa, ["Off", "FXAA", "SMAA"]);
}

/// The `SETUPPIC` block is pinned inside the panel: the option cells never leave
/// the free area, and `PREFSPIC` (no room) has no video rows at all.
#[test]
fn the_video_rows_live_in_the_free_setuppic_block() {
    let setup = setup_dialog(VideoChoice::new(RenderScale::Hd4, AaMode::Smaa));
    assert!(setup.has_video_rows());

    for row in VideoRow::ALL {
        for index in 0..row.option_count() {
            let rect = video_option_rect(row, index);

            assert!(rect.ulx >= 25, "left margin: {rect:?}");
            assert!(
                rect.ulx + rect.width <= 375,
                "right margin: {rect:?} on {row:?}"
            );
            assert!(rect.uly >= 233, "below the last C++ row: {rect:?}");
            assert!(rect.uly + rect.height <= 383, "above the buttons: {rect:?}");
        }
    }

    let in_game = PreferencesDialog::new(
        PreferencesVariant::InGame,
        Preferences::default(),
        PreferencesInfo::default(),
    );
    assert!(
        !in_game.has_video_rows(),
        "PREFSPIC fills the block with its own rows"
    );
    assert_eq!(
        in_game.video_option_screen_rect(VideoRow::Resolution, 0),
        None
    );
}

/// The repository ships a valid, documented template; it must stay readable by
/// the parser (comments/trailing-comma rules of this task).
#[test]
fn the_repository_settings_template_parses() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../settings.jsonc");

    assert!(
        path.is_file(),
        "settings.jsonc must exist at the repository root"
    );

    let settings = Settings::load(&path)
        .expect("settings.jsonc parses")
        .expect("settings.jsonc exists");

    assert!(
        [1, 2, 4].contains(&settings.render_scale().scale()),
        "the template selects a shipped resolution"
    );
    // The template documents the GPU anti-aliasing mode and ships it off
    // (the default, the byte-for-byte path).
    assert_eq!(
        settings.display.aa,
        Some(AaMode::Off),
        "the template must keep display.aa = off"
    );
}
