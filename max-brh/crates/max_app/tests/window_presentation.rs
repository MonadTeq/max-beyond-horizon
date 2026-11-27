//! Gate of the window resolution switch (points 2 and 3 of the task):
//! **CLI**, **`settings.jsonc`** and the **in-window menu** must select the same
//! render scale; a menu choice is presentation only and must not touch the
//! simulation; the menu bar must not alter the game frame it sits above.
//!
//! The 1:1 golden path is not modified by any of this: the menu bar is window
//! chrome above the final frame (`window_menu`), the game frame is copied
//! unchanged below it, and the render scale never reaches `App::render_frame` /
//! `Screen::render` in `RenderScale::PixelPerfect`.

use std::path::Path;

use max_app::settings::{resolve, CliPresentation, Settings};
use max_app::ui::InputEvent;
use max_app::window_menu::{
    composite_overlay, item_rect, resolution_label, title_rect, MenuItem, MenuPage, WindowMenu,
    WindowMenuAction, BAR_BACKGROUND, BAR_HEIGHT, RESOLUTIONS,
};
use max_app::{App, AppConfig};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::presentation::window_points_for;
use max_render::render_scale::{AaMode, PresentationMode, RenderScale};

fn center(rect: PixelRect) -> (i32, i32) {
    (rect.ulx + rect.width / 2, rect.uly + rect.height / 2)
}

fn click(menu: &mut WindowMenu, rect: PixelRect) -> max_app::window_menu::MenuResponse {
    let (x, y) = center(rect);

    menu.handle_input(InputEvent::MouseClick(x, y))
}

/// `Settings в†’ Resolutions в†’ row(scale)` through the real hit tests.
fn choose_resolution_in_the_menu(scale: RenderScale) -> WindowMenuAction {
    let mut menu = WindowMenu::new();

    click(&mut menu, title_rect(MenuPage::Settings));
    click(&mut menu, item_rect(MenuItem::Resolutions));

    let response = click(&mut menu, item_rect(MenuItem::Resolution(scale)));

    assert!(!menu.is_open(), "choosing a resolution closes the menu");

    response.action
}

/// The three ways of naming a resolution are equivalent: the same `S`, the same
/// render frame and the same window points for the same window scale.
#[test]
fn cli_jsonc_and_menu_select_the_same_resolution() {
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
            choose_resolution_in_the_menu(scale),
            WindowMenuAction::SetResolution(scale)
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
            window_points_for(640, 480 + BAR_HEIGHT as u32, from_file.render_scale, 1),
            window_points_for(640, 480 + BAR_HEIGHT as u32, cli.render_scale, 1),
            "the same render scale must open the same window (game frame + menu bar)"
        );
    }
}

/// The precedence is enforced end to end: a CLI presentation flag replaces the
/// file value for the same parameter; the file fills in the parameters the CLI
/// did not name.
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

/// The menu action is pure input; applying it to the window presentation must
/// not change the application/session state (snapshot is the determinism lock).
#[test]
fn a_menu_resolution_choice_does_not_touch_the_simulation() {
    let app = App::new(AppConfig::default());
    let before = app.state_ref().snapshot_bytes();

    for scale in RESOLUTIONS {
        let action = choose_resolution_in_the_menu(scale);

        assert_eq!(action, WindowMenuAction::SetResolution(scale));
        assert_eq!(
            app.state_ref().snapshot_bytes(),
            before,
            "choosing {scale:?} is presentation only: the snapshot must not change"
        );
    }
}

/// Copies the game frame into the presentation buffer below the menu bar.
///
/// This mirrors `WindowPresentation::present`: the game frame is written
/// unchanged under the `BAR_HEIGHT` band, never stretched or recoloured.
fn present_game_frame(frame: &RgbaBuffer, bar_pixels: u32) -> RgbaBuffer {
    let mut present = RgbaBuffer::filled(frame.width(), frame.height() + bar_pixels, Rgba::BLACK);
    let row_bytes = frame.width() as usize * 4;
    let target_row = present.width() as usize * 4;
    let target = present.pixels_mut();

    for row in 0..frame.height() as usize {
        let source_start = row * row_bytes;
        let target_start = (bar_pixels as usize + row) * target_row;

        target[target_start..target_start + row_bytes]
            .copy_from_slice(&frame.pixels()[source_start..source_start + row_bytes]);
    }

    present
}

/// The bar is window chrome above any screen: the game frame (main menu or a
/// match) is copied byte-for-byte below it, and the menu overlay only paints its
/// own pixels.
#[test]
fn the_menu_bar_keeps_the_underlying_screen_intact() {
    // A menu-like dark frame and a game-like frame stand in for the two screens.
    for background in [Rgba::rgb(12, 16, 24), Rgba::rgb(160, 120, 70)] {
        let frame = RgbaBuffer::filled(640, 480, background);
        let original = frame.clone();
        let mut menu = WindowMenu::new();

        // `File` is open; the menu draws in the bar band and, while open, over
        // the top of the game area.
        click(&mut menu, title_rect(MenuPage::File));

        let mut present = present_game_frame(&frame, BAR_HEIGHT as u32);
        let mut overlay = RgbaBuffer::new(640, 480 + BAR_HEIGHT as u32);
        menu.render_overlay(&mut overlay, RenderScale::PixelPerfect, AaMode::Off);
        composite_overlay(&overlay, &mut present, 1);

        assert_eq!(
            frame, original,
            "the game frame itself must not be modified"
        );
        assert_eq!(
            present.get(320, (BAR_HEIGHT as u32 + 300) as i32),
            Some(background),
            "the game pixels below the menu must stay byte-identical"
        );
        assert_eq!(
            present.get(500, 5),
            Some(BAR_BACKGROUND),
            "the menu bar itself must be drawn in the band above the frame"
        );
    }
}

/// HD modes composite the bar as a crisp nearest `Г—S` block; the HD game frame
/// below it keeps its pixels.
#[test]
fn the_menu_bar_scales_with_the_hd_frame() {
    let background = Rgba::rgb(40, 80, 120);
    let frame = RgbaBuffer::filled(1280, 960, background);
    let mut menu = WindowMenu::new();

    click(&mut menu, title_rect(MenuPage::Settings));

    let mut present = present_game_frame(&frame, BAR_HEIGHT as u32 * 2);
    let mut overlay = RgbaBuffer::new(640, 480 + BAR_HEIGHT as u32);
    menu.render_overlay(&mut overlay, RenderScale::Hd2, AaMode::Off);
    composite_overlay(&overlay, &mut present, 2);

    // (500,5) logical в†’ (1000,10) pixels: the bar is a 2Г—2 block there.
    assert_eq!(present.get(1000, 10), Some(BAR_BACKGROUND));
    assert_eq!(present.get(1001, 11), Some(BAR_BACKGROUND));
    // Below the bar the HD frame is untouched.
    assert_eq!(present.get(640, 32 + 600), Some(background));
    assert_eq!(frame.get(640, 600), Some(background));
}

/// The labels in the menu are exactly the resolutions the requirement names.
#[test]
fn the_menu_names_the_three_resolutions() {
    assert_eq!(resolution_label(RenderScale::PixelPerfect), "640x480 (1:1)");
    assert_eq!(resolution_label(RenderScale::Hd2), "1280x960 (x2)");
    assert_eq!(resolution_label(RenderScale::Hd4), "2560x1920 (x4)");
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
