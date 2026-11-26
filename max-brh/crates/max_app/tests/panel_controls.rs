//! Chrome panel controls that are **not** `GNW` buttons (manual-run defect):
//! the zoom `+`/`−` pair and the zoom slider.
//!
//! Oracle: `GameManager_ProcessInput`, `WINDOW_ZOOM_PLUS_BUTTON`/
//! `WINDOW_ZOOM_MINUS_BUTTON`/`WINDOW_ZOOM_SLIDER_BUTTON`
//! (`game_manager.cpp:6645-6679`):
//!
//! * `+` — on `MOUSE_RELEASE_LEFT`: `Gfx_ZoomLevel + 10`;
//! * `−` — on `MOUSE_RELEASE_LEFT`: `Gfx_ZoomLevel - 10`;
//! * slider — on `MOUSE_PRESS_LEFT`: `64 - ((x - ulx - 14) * 60) / 99`.
//!
//! The pressed `*_OF`/`*_ON` picture feedback of the ordinary panel buttons is
//! covered by `button_press.rs`; here the event timing (press vs release) is the
//! subject — the port used to act on the press with a step of 8.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::ui::{handle_session_input, InputEvent};
use max_app::{App, AppConfig};
use max_core::team::TeamKind;
use max_render::layout;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn fixture_app() -> Option<App> {
    let root = repo_path("max-original");
    let patches = repo_path("tests/fixtures/assets/PATCHES.RES");

    if !root.join("MAX.RES").is_file() || !root.join("COLOR.PAL").is_file() || !patches.is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&root)
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        .with_mode(max_app::AppMode::Ui);
    let mut app = App::new(config);

    app.init_assets().ok()?;

    let mut setup = LocalSetup::default();

    setup.world = 0;
    setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    app.start_local_game(&setup).ok()?;

    Some(app)
}

/// The centre of a panel rectangle (window coordinates of the 640×480 frame).
fn center(rect: max_render::buffer::PixelRect) -> (i32, i32) {
    (rect.ulx + rect.width / 2, rect.uly + rect.height / 2)
}

/// `+`/`−` act on the release with a step of 10 (`game_manager.cpp:6645-6679`),
/// the press itself changes nothing.
#[test]
fn the_zoom_pair_steps_by_ten_on_the_release() {
    let Some(mut app) = fixture_app() else {
        eprintln!("test fixture is missing: max-original/MAX.RES");

        return;
    };

    let start = app.viewport().zoom();
    let (plus_x, plus_y) = center(layout::ZOOM_PLUS);

    // The press only arms the visual; the zoom step is the release.
    handle_session_input(&mut app, InputEvent::MouseClick(plus_x, plus_y));
    assert_eq!(
        app.viewport().zoom(),
        start,
        "the press of `+` must not zoom (`MOUSE_RELEASE_LEFT`)"
    );
    handle_session_input(&mut app, InputEvent::MouseRelease(plus_x, plus_y));
    assert_eq!(
        app.viewport().zoom(),
        (start + 10).min(64),
        "the release of `+` zooms by 10"
    );

    let after_plus = app.viewport().zoom();
    let (minus_x, minus_y) = center(layout::ZOOM_MINUS);

    handle_session_input(&mut app, InputEvent::MouseClick(minus_x, minus_y));
    assert_eq!(
        app.viewport().zoom(),
        after_plus,
        "the press of `−` is silent"
    );
    handle_session_input(&mut app, InputEvent::MouseRelease(minus_x, minus_y));
    assert_eq!(
        app.viewport().zoom(),
        after_plus.saturating_sub(10),
        "the release of `−` zooms out by 10"
    );
}

/// The slider is a press control (`game_manager.cpp:6651-6672`): the leftmost
/// clickable point (window x + 14) is the 1:1 end of the original 60-step scale.
#[test]
fn the_zoom_slider_reacts_to_the_press() {
    let Some(mut app) = fixture_app() else {
        eprintln!("test fixture is missing: max-original/MAX.RES");

        return;
    };

    let x = layout::ZOOM_SLIDER.ulx + max_app::panels::ZOOM_SLIDER_KNOB_OFFSET;
    let y = layout::ZOOM_SLIDER.uly + layout::ZOOM_SLIDER.height / 2;

    handle_session_input(&mut app, InputEvent::MouseClick(x, y));
    assert_eq!(app.viewport().zoom(), 64, "left end of the slider — 1:1");

    let right = layout::ZOOM_SLIDER.ulx + max_app::panels::ZOOM_SLIDER_KNOB_OFFSET + 99;

    handle_session_input(&mut app, InputEvent::MouseClick(right, y));
    assert_eq!(
        app.viewport().zoom(),
        max_render::viewport::MIN_ZOOM,
        "right end — the smallest zoom of the port's viewport"
    );
}
