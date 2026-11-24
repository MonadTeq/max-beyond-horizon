//! `Introduction` screen: MVE video `INTROFLC` → `maxint.mve`.
//!
//! The test loads the real `MAX.RES`/`MAXINT.MVE` (skipped if there is no data):
//! frame 0 is decoded on entry, scaled to 640×480 with letterbox,
//! a repeated render is deterministic, Esc returns to the main menu.

use std::path::{Path, PathBuf};

use max_app::assets::Assets;
use max_app::intro::IntroScreen;
use max_app::qa::QaMenuClient;
use max_app::subtitles::SubtitleTrack;
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

    // The 432×320 video is scaled to 640×474, 3 px bars at the top/bottom.
    assert!(painted > 640 * 400, "видео заполняет кадр: {painted}");
    assert_eq!(frame.get(320, 1), Some(Rgba::rgb(0, 0, 0)), "letterbox");
    assert_ne!(frame.get(320, 240), Some(Rgba::rgb(0, 0, 0)));

    // dt = 0 — freeze-frame: a repeated render is pixel-for-pixel.
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

    // 15 FPS: one 1/60 s frame is not enough, 0.5 s — several frames.
    assert!(!screen.advance(1.0 / 60.0));
    assert_eq!(screen.frame_index(), 1);
    assert!(!screen.advance(0.5));
    assert!(
        screen.frame_index() > 1,
        "видео идёт: {}",
        screen.frame_index()
    );

    // Esc on the Introduction screen — back to MainMenu (like `mve_cb_ctl`).
    let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
    let mut state = Screen::Introduction(screen);
    let transition = state.handle_input(&mut ctx, InputEvent::Key(Key::Escape));

    assert!(matches!(transition, Transition::None));
    assert!(matches!(state, Screen::MainMenu));
}

/// C3: the real track (stereo 22050 Hz 16-bit delta) is decoded with the frames.
#[test]
fn intro_audio_track_is_decoded_into_chunks() {
    let Some(assets) = assets() else {
        return;
    };
    let mut screen = IntroScreen::new(assets.intro_mve.clone());
    let format = screen.audio_format().expect("ALLOC_AUDIO_BUFFERS");

    assert_eq!(format.channels, 2);
    assert_eq!(format.sample_rate, 22_050);
    assert_eq!(format.bits, 16);

    // Frame 0 was decoded on entry; the container leads the first scene with 14
    // audio-only records, so the first drain returns the lead-in plus frame 0.
    let chunks = screen.take_audio();
    assert_eq!(chunks.len(), 15);
    assert!(chunks.iter().all(|chunk| chunk.channels == 2));
    assert!(chunks.iter().all(|chunk| chunk.sample_rate == 22_050));
    // `sndDecompS16`: the initial samples of the first chunk come from the stream.
    assert_eq!(&chunks[0].samples[..4], &[-2, 1, 10, 4]);

    // Advancing yields exactly one chunk per video frame.
    let before = screen.frame_index();
    assert!(!screen.advance(0.5));
    let decoded = screen.frame_index() - before;
    let chunks = screen.take_audio();

    assert!(decoded > 1);
    assert_eq!(chunks.len(), decoded);
}

/// C3: captions are drawn over the video area for the frame that is shown.
#[test]
fn intro_draws_the_caption_of_the_current_frame() {
    let Some(assets) = assets() else {
        return;
    };
    let menu = assets.menu.as_ref().expect("MenuAssets");
    let caption = "They were the unwanted, cults, religious factions, even entire governments.";
    let json = format!(
        r#"{{ "subtitle": {{ "en-US": [ {{ "start": 0, "end": 60, "caption": "{caption}" }} ] }} }}"#
    );
    let track = SubtitleTrack::parse_json(&json, "en-US").expect("subtitle track");

    let plain = IntroScreen::new(assets.intro_mve.clone());
    let with_captions =
        IntroScreen::with_media(assets.intro_mve.clone(), Some(track.clone()));
    let mut without_frame = RgbaBuffer::new(640, 480);
    let mut with_frame = RgbaBuffer::new(640, 480);

    plain.draw(&mut without_frame, menu);
    with_captions.draw(&mut with_frame, menu);

    // The 432×320 video occupies rows 3..477; the caption sits at 90 % of its height.
    let mut changed_in_band = 0usize;
    let mut changed_outside = 0usize;

    for y in 0..480 {
        for x in 0..640 {
            if without_frame.get(x, y) != with_frame.get(x, y) {
                if (400..470).contains(&y) {
                    changed_in_band += 1;
                } else {
                    changed_outside += 1;
                }
            }
        }
    }

    assert!(
        changed_in_band > 100,
        "подпись рисуется в нижней полосе: {changed_in_band}"
    );
    assert_eq!(changed_outside, 0, "вне полосы подписи кадр не меняется");
    assert_eq!(with_captions.subtitles().map(SubtitleTrack::len), Some(1));

    // After `end` (frame 60) the caption disappears: the frames are identical again.
    let mut plain = IntroScreen::new(assets.intro_mve.clone());
    let mut captioned = IntroScreen::with_media(assets.intro_mve.clone(), Some(track));

    for _ in 0..20 {
        plain.advance(0.5);
        captioned.advance(0.5);
    }

    assert!(plain.frame_index() > 60);
    assert_eq!(plain.frame_index(), captioned.frame_index());

    let mut plain_frame = RgbaBuffer::new(640, 480);
    let mut captioned_frame = RgbaBuffer::new(640, 480);

    plain.draw(&mut plain_frame, menu);
    captioned.draw(&mut captioned_frame, menu);
    assert_eq!(
        plain_frame.pixels(),
        captioned_frame.pixels(),
        "после окна подписи кадры совпадают"
    );
}

/// Fallback: no movie and no captions — the placeholder is drawn, nothing panics.
#[test]
fn a_missing_intro_and_missing_subtitles_are_a_placeholder() {
    let Some(assets) = assets() else {
        return;
    };
    let menu = assets.menu.as_ref().expect("MenuAssets");
    let mut screen = IntroScreen::new(None);

    assert!(!screen.is_loaded());
    assert_eq!(screen.error(), None);
    assert!(screen.take_audio().is_empty());
    assert_eq!(screen.audio_format(), None);

    let mut frame = RgbaBuffer::new(640, 480);
    let painted = screen.draw(&mut frame, menu);

    assert!(painted > 0, "заглушка рисуется");
    assert_eq!(screen.subtitles(), None);
}
