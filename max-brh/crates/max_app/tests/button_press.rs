//! Visual press feedback of buttons (manual-run defect 4).
//!
//! Oracle: `button.c` draws the `*D` picture while the pointer is held over the
//! button (`:479-531`), and `Button::SetCaption` (`button.cpp:412-449`) paints the
//! pressed label with `Fonts_DarkOrageColor (5,56,199)`. The port ignored the
//! `down_image`/`pressed` arguments of `draw_image_button`, so a click never
//! pressed the picture.

use std::path::{Path, PathBuf};

use max_app::assets::Assets;
use max_app::local::LocalSetup;
use max_app::menu;
use max_app::press;
use max_app::{App, AppConfig};
use max_core::team::{TeamId, TeamKind};
use max_render::buffer::{PixelRect, RgbaBuffer};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn assets() -> Option<Assets> {
    let root = repo_path("max-original");
    let patches = repo_path("tests/fixtures/assets/PATCHES.RES");

    if !root.join("MAX.RES").is_file() || !root.join("COLOR.PAL").is_file() || !patches.is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&root)
        .with_assets_dir(repo_path("tests/fixtures/assets"));

    Assets::load(&config).ok()
}

/// Ends the press pulse after a release.
fn drain_press() {
    press::release();

    for _ in 0..press::RELEASE_FRAMES {
        press::advance();
    }
}

fn changed(before: &RgbaBuffer, after: &RgbaBuffer, rect: PixelRect) -> usize {
    let mut count = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            if before.get(x, y) != after.get(x, y) {
                count += 1;
            }
        }
    }

    count
}

/// The main menu's `New Game` button switches to `BLANK_DN` while pressed and
/// returns to `BLANK_UP` after the release pulse.
#[test]
fn a_main_menu_button_draws_its_pressed_picture() {
    let assets = assets().expect(concat!("test fixture is missing: ", stringify!(assets())));
    let menu = assets.menu.as_ref().expect("MenuAssets");

    let mut rest = RgbaBuffer::new(640, 480);
    menu::draw_main_menu(&mut rest, menu, "", None);

    let button = menu::MAIN_MENU_BUTTONS[0].rect;
    press::arm(button.ulx + button.width / 2, button.uly + button.height / 2);

    let mut pressed = RgbaBuffer::new(640, 480);
    menu::draw_main_menu(&mut pressed, menu, "", None);

    assert!(
        changed(&rest, &pressed, button) > 200,
        "the button picture and label change while pressed"
    );

    // This is a frame of the pressed look, not a global repaint.
    assert!(
        changed(&rest, &pressed, PixelRect::new(0, 0, 640, 480)) < 20_000,
        "only the pressed button changes"
    );

    drain_press();

    let mut released = RgbaBuffer::new(640, 480);
    menu::draw_main_menu(&mut released, menu, "", None);
    assert_eq!(
        changed(&rest, &released, button),
        0,
        "after the release pulse the button is back to rest"
    );
}

/// The in-game panel button (`End Turn`) also shows its pressed picture.
#[test]
fn the_end_turn_panel_button_draws_its_pressed_picture() {
    let app = {
        let config = AppConfig::default()
            .with_data_dir(repo_path("max-original"))
            .with_assets_dir(repo_path("tests/fixtures/assets"))
            .with_mode(max_app::AppMode::Ui);
        let mut app = App::new(config);
        app.init_assets().expect("game assets");

        let mut setup = LocalSetup::default();
        setup.world = 0;
        setup.team_types = [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ];
        app.start_local_game(&setup).expect("local match");
        app
    };

    assert_eq!(app.state_ref().player_team(), TeamId::RED);
    let rect = max_render::layout::END_TURN_BUTTON;
    let rest = app.render_frame();

    press::arm(rect.ulx + rect.width / 2, rect.uly + rect.height / 2);
    let pressed = app.render_frame();

    assert!(
        changed(&rest, &pressed, rect) > 200,
        "the End Turn picture changes while pressed"
    );
    assert!(app.hud().pending.is_empty(), "rendering gives no orders");

    drain_press();
    let released = app.render_frame();
    assert_eq!(changed(&rest, &released, rect), 0, "release restores the rest picture");
}
