//! Экран `Introduction`: MVE-видео `INTROFLC` → `maxint.mve`.
//!
//! Тест грузит настоящие `MAX.RES`/`MAXINT.MVE` (если данных нет — пропуск):
//! кадр 0 декодируется при входе, масштабируется в 640×480 с letterbox,
//! повторный рендер детерминирован, Esc возвращает в главное меню.

use std::path::{Path, PathBuf};

use max_app::assets::Assets;
use max_app::intro::IntroScreen;
use max_app::qa::QaMenuClient;
use max_app::ui::{Screen, Transition, UiContext};
use max_app::{AppConfig, InputEvent, Key};
use max_render::buffer::{Rgba, RgbaBuffer};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn assets() -> Option<Assets> {
    let root = root();
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        return None;
    }
    if !root.join("max-original/MAXINT.MVE").is_file() {
        return None;
    }

    let config = AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        world_dir: Some(root.join("max-original")),
        ..Default::default()
    };

    Assets::load(&config).ok()
}

#[test]
fn intro_asset_points_to_maxint_mve() {
    let Some(assets) = assets() else {
        return;
    };

    let path = assets.intro_mve.as_ref().expect("INTROFLC → maxint.mve");
    assert!(path.is_file(), "{}", path.display());
    assert_eq!(
        path.file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase()),
        Some("maxint.mve".to_string())
    );
}

#[test]
fn intro_decodes_frame_zero_and_scales_to_letterbox() {
    let Some(assets) = assets() else {
        return;
    };
    let screen = IntroScreen::new(assets.intro_mve.clone());
    let menu = assets.menu.as_ref().expect("MenuAssets");

    assert_eq!(screen.error(), None);
    assert!(screen.is_loaded());
    assert_eq!(screen.frame_index(), 1, "кадр 0 декодирован при входе");

    let mut frame = RgbaBuffer::new(640, 480);
    let painted = screen.draw(&mut frame, menu);

    // Видео 432×320 масштабируется в 640×474, поля сверху/снизу по 3 px.
    assert!(painted > 640 * 400, "видео заполняет кадр: {painted}");
    assert_eq!(frame.get(320, 1), Some(Rgba::rgb(0, 0, 0)), "letterbox");
    assert_ne!(frame.get(320, 240), Some(Rgba::rgb(0, 0, 0)));

    // dt = 0 — стоп-кадр: повторный рендер пиксель-в-пиксель.
    let mut again = RgbaBuffer::new(640, 480);
    screen.draw(&mut again, menu);
    assert_eq!(frame.pixels(), again.pixels());
}

#[test]
fn intro_advances_by_real_time_and_esc_closes() {
    let Some(assets) = assets() else {
        return;
    };
    let mut screen = IntroScreen::new(assets.intro_mve.clone());

    // 15 FPS: одного кадра 1/60 с недостаточно, 0.5 с — несколько кадров.
    assert!(!screen.advance(1.0 / 60.0));
    assert_eq!(screen.frame_index(), 1);
    assert!(!screen.advance(0.5));
    assert!(
        screen.frame_index() > 1,
        "видео идёт: {}",
        screen.frame_index()
    );

    // Esc на экране Introduction — назад в MainMenu (как `mve_cb_ctl`).
    let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
    let mut state = Screen::Introduction(screen);
    let transition = state.handle_input(&mut ctx, InputEvent::Key(Key::Escape));

    assert!(matches!(transition, Transition::None));
    assert!(matches!(state, Screen::MainMenu));
}
