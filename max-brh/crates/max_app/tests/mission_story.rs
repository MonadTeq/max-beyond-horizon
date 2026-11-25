//! Campaign stage 3b: story screens (intro/victory/defeat) of `menu.cpp:766-864`.
//!
//! Gates:
//! 1. the story screen is **presentation only** — showing it (and closing it) leaves
//!    `snapshot_bytes`/digest of the already started mission byte-identical;
//! 2. a missing story file (the `DEMO2FLC` → `maxmve2.mve` reference is not next to
//!    `MAX.RES`) is a **visible** fallback and the mission still starts/continues;
//! 3. the real descriptors drive the screen: `SC_C0001` intro (text + `ENDGAME*`
//!    background + `DEMO2FLC` video) and `SC_C0009` victory (text + background +
//!    `WINR_MSC` music), while a mission without a block has no screen;
//! 4. rendering with `dt = 0` is deterministic (pixel-for-pixel), like the other
//!    golden screens.
//!
//! The mission fixtures are the stage-1/3a ones; the media are the real
//! `MAX.RES`/`PATCHES.RES` (the tests are skipped when the game data is absent).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use max_app::menu::MenuAssets;
use max_app::qa::{QaConfig, QaMenuClient, QaRunner};
use max_app::story::{StoryKind, StoryPhase, StoryScreen};
use max_app::ui::{InputEvent, Key, Screen, Transition, UiContext};
use max_app::{App, AppConfig};
use max_assets::{MissionRegistry, ResArchive};
use max_core::victory::MissionCategory;
use max_render::buffer::RgbaBuffer;

/// `max-original/SAVE1.CAM` — campaign 1 (`SC_C0001`).
const SAVE1_CAMPAIGN: &[u8] = include_bytes!("data/save1_v70.cam");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn patches_res() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../max_assets/tests/data/PATCHES.RES")
}

/// `MAX.RES` (backgrounds, video references); `None` without the game data.
fn max_res() -> Option<ResArchive> {
    let path = root().join("max-original/MAX.RES");

    path.is_file()
        .then(|| ResArchive::open(path).ok())
        .flatten()
}

fn game_data_dir() -> PathBuf {
    root().join("max-original")
}

/// Temporary mission directory with `SAVE1.CAM` (campaign 1).
fn mission_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let dir = std::env::temp_dir().join(format!(
        "max_app_story_{name}_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    std::fs::create_dir_all(&dir).expect("create temp data dir");
    std::fs::write(dir.join("SAVE1.CAM"), SAVE1_CAMPAIGN).expect("SAVE1.CAM");

    dir
}

fn load_registry(dir: &Path) -> MissionRegistry {
    MissionRegistry::open(patches_res(), dir).expect("PATCHES.RES parses")
}

fn app_in(dir: &Path) -> App {
    App::new(
        AppConfig::default()
            .with_save_dir(dir)
            .with_save_data_dir(dir),
    )
}

fn start_campaign_one(app: &mut App, registry: &MissionRegistry) {
    app.start_mission_from_registry(
        registry,
        MissionCategory::Campaign,
        0,
        &max_app::local::LocalSetup {
            player_name: "Commander".to_string(),
            ..max_app::local::LocalSetup::default()
        },
    )
    .expect("campaign 1 starts");
}

fn campaign_descriptor(registry: &MissionRegistry, index: usize) -> max_assets::MissionDescriptor {
    registry
        .missions(MissionCategory::Campaign)
        .get(index)
        .unwrap_or_else(|| panic!("campaign descriptor {index}"))
        .clone()
}

// ---------------------------------------------------------------------------
// Gate 1: presenting the story screen does not change the mission snapshot
// ---------------------------------------------------------------------------

/// The same mission with the story screen shown/dismissed has the same
/// `snapshot_bytes`/digest as the mission without any story screen — before and
/// after five ticks (owner gate).
#[test]
fn showing_the_story_screen_does_not_change_the_mission_snapshot() {
    let dir = mission_dir("snapshot");
    let registry = load_registry(&dir);
    let max_res = max_res();

    // Baseline: the mission without any story screen.
    let mut baseline = app_in(&dir);
    start_campaign_one(&mut baseline, &registry);
    let snapshot = baseline.snapshot();

    // The same mission with its intro story screen built, rendered and dismissed.
    let mut with_story = app_in(&dir);
    start_campaign_one(&mut with_story, &registry);
    assert_eq!(
        with_story.snapshot(),
        snapshot,
        "the start is deterministic"
    );

    let descriptor = campaign_descriptor(&registry, 0);
    let story = StoryScreen::for_mission(
        Some(&descriptor),
        StoryKind::Intro,
        1,
        max_res.as_ref(),
        // A directory without `maxmve2.mve`: the video fallback is visible, the
        // screen still shows the text (the gate must not depend on the movie).
        &dir,
    )
    .expect("SC_C0001 has an intro story");
    let menu = max_res.as_ref().and_then(MenuAssets::load);
    let mut frame = RgbaBuffer::new(640, 480);
    story.draw(&mut frame, menu.as_ref(), None);
    assert_eq!(
        with_story.snapshot(),
        snapshot,
        "drawing the story screen must not touch the simulation"
    );

    let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
    let mut screen = Screen::MissionStory(Box::new(story));
    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Enter)),
        Transition::StoryFinished,
        "without a playable video the text window finishes the story"
    );
    assert_eq!(
        with_story.snapshot(),
        snapshot,
        "closing the story screen must not touch the simulation"
    );

    // Five ticks stay byte-identical to the no-story run.
    for _ in 0..5 {
        baseline.tick();
        with_story.tick();
    }

    assert_eq!(with_story.snapshot(), baseline.snapshot());
    assert_eq!(with_story.digest(), baseline.digest());

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Gate 2: a missing story file is a visible fallback, the mission starts
// ---------------------------------------------------------------------------

/// `DEMO2FLC` points to `maxmve2.mve`; when the file is not next to `MAX.RES` the
/// screen names it in its visible diagnostics and closes straight into the match.
#[test]
fn a_missing_story_file_is_visible_and_does_not_block_the_mission() {
    let Some(max_res) = max_res() else {
        return;
    };
    let dir = mission_dir("missing");
    let registry = load_registry(&dir);
    let descriptor = campaign_descriptor(&registry, 0);

    let mut app = app_in(&dir);
    start_campaign_one(&mut app, &registry);
    let snapshot = app.snapshot();

    let story =
        StoryScreen::for_mission(Some(&descriptor), StoryKind::Intro, 1, Some(&max_res), &dir)
            .expect("the intro text is shown without the movie");

    assert!(!story.text().is_empty());
    assert!(story.has_background(), "ENDGAME* are in MAX.RES");
    assert!(
        story.videos().is_empty(),
        "maxmve2.mve is not in the temp data directory"
    );
    assert!(
        story
            .diagnostics()
            .iter()
            .any(|line| line.contains("maxmve2.mve")),
        "the missing file must be named: {:?}",
        story.diagnostics()
    );
    assert!(
        !story.status_line().is_empty(),
        "the driver has a visible status line"
    );

    // The drawing itself must not be an empty frame.
    let menu = MenuAssets::load(&max_res);
    let mut frame = RgbaBuffer::new(640, 480);
    assert!(story.draw(&mut frame, menu.as_ref(), None) > 0);

    // The driver action after the screen: the already started mission continues.
    let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
    let mut screen = Screen::MissionStory(Box::new(story));
    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape)),
        Transition::StoryFinished
    );
    assert!(
        app.local_active(),
        "the mission starts without the story file"
    );
    assert_eq!(app.snapshot(), snapshot);

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Gate 3: the real story blocks
// ---------------------------------------------------------------------------

/// `SC_C0001` intro: text, the `ENDGAME*` background list and the `DEMO2FLC`
/// (→ `maxmve2.mve`) video; the deterministic seed picks a background from the
/// oracle's list instead of advancing the simulation RNG.
#[test]
fn the_campaign_intro_uses_the_real_story_block() {
    let Some(max_res) = max_res() else {
        return;
    };
    let dir = mission_dir("intro");
    let registry = load_registry(&dir);
    let descriptor = campaign_descriptor(&registry, 0);

    let story = StoryScreen::for_mission(
        Some(&descriptor),
        StoryKind::Intro,
        1,
        Some(&max_res),
        &game_data_dir(),
    )
    .expect("SC_C0001 intro");

    assert!(story
        .text()
        .starts_with("We are the Secret Council of Freedom"));
    assert_eq!(story.background_tag(), Some("ENDGAME2"), "seed 1");
    assert!(story.diagnostics().is_empty(), "{:?}", story.diagnostics());
    assert_eq!(story.music(), None, "SC_C0001 has no story music");

    let videos = story.videos();
    assert_eq!(videos.len(), 1);
    assert_eq!(
        videos[0]
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase()),
        Some("maxmve2.mve".to_string())
    );
    assert!(videos[0].is_file(), "{}", videos[0].display());

    std::fs::remove_dir_all(&dir).ok();
}

/// `SC_C0009` victory: text, background and the `WINR_MSC` music cue.
#[test]
fn the_last_campaign_mission_has_a_victory_story_with_music() {
    let Some(max_res) = max_res() else {
        return;
    };
    let dir = mission_dir("victory");
    let registry = load_registry(&dir);
    let descriptor = campaign_descriptor(&registry, 8);

    let story = StoryScreen::for_mission(
        Some(&descriptor),
        StoryKind::Victory,
        9,
        Some(&max_res),
        &game_data_dir(),
    )
    .expect("SC_C0009 victory story");

    assert!(story
        .text()
        .starts_with("The last threat to the Great Ships"));
    assert_eq!(story.background_tag(), Some("ENDGAME1"), "seed 9 % 9");
    assert_eq!(story.music(), Some("WINR_MSC"));
    assert!(story.videos().is_empty());
    assert!(story.diagnostics().is_empty(), "{:?}", story.diagnostics());

    std::fs::remove_dir_all(&dir).ok();
}

/// A mission without the block has no story screen — the oracle's
/// `HasIntroInfo`/`HasVictoryInfo` guards (`menu.cpp:499/506/2065`).
#[test]
fn a_mission_without_a_story_block_has_no_screen() {
    let dir = mission_dir("no_story");
    let registry = load_registry(&dir);
    let second = campaign_descriptor(&registry, 1);

    for kind in [StoryKind::Intro, StoryKind::Victory, StoryKind::Defeat] {
        assert!(
            StoryScreen::for_mission(Some(&second), kind, 2, None, &dir).is_none(),
            "{kind:?}"
        );
    }

    assert!(StoryScreen::for_mission(None, StoryKind::Intro, 1, None, &dir).is_none());

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Gate 4: deterministic frames and the text → video sequence
// ---------------------------------------------------------------------------

/// The text window is a deterministic frame (`dt = 0`); the real video is decoded
/// only when the player advances, and any key then finishes the single movie.
#[test]
fn the_story_frame_is_deterministic_and_the_video_sequence_finishes() {
    let Some(max_res) = max_res() else {
        return;
    };
    let dir = mission_dir("frames");
    let registry = load_registry(&dir);
    let descriptor = campaign_descriptor(&registry, 0);
    let menu = MenuAssets::load(&max_res).expect("MenuAssets");

    let story = StoryScreen::for_mission(
        Some(&descriptor),
        StoryKind::Intro,
        1,
        Some(&max_res),
        &game_data_dir(),
    )
    .expect("SC_C0001 intro");

    let mut first = RgbaBuffer::new(640, 480);
    let mut second = RgbaBuffer::new(640, 480);
    story.draw(&mut first, Some(&menu), None);
    story.draw(&mut second, Some(&menu), None);
    assert_eq!(first.pixels(), second.pixels(), "dt = 0 frames match");

    let mut screen = Screen::MissionStory(Box::new(story));
    let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);

    // Enter closes the text window and starts `maxmve2.mve`.
    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Enter)),
        Transition::None
    );
    match &screen {
        Screen::MissionStory(story) => {
            assert_eq!(story.phase(), StoryPhase::Video);
        }
        other => panic!("expected MissionStory, got {other:?}"),
    }

    // Any key aborts the movie; the single video is the end of the story.
    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape)),
        Transition::StoryFinished
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Gate 1 (driver level): the scripted UI driver enters the same mission
// ---------------------------------------------------------------------------

/// The real driver (`QaRunner`: `main.rs` loop) shows the intro story and then
/// enters the session; its mission state is byte-identical to a direct V70 load
/// of the same file (`menu.cpp:2065-2098`).
#[test]
fn the_qa_driver_shows_the_intro_story_and_starts_the_same_mission() {
    let Some(_max_res) = max_res() else {
        return;
    };
    let dir = mission_dir("qa_driver");
    let script = dir.join("story.qa");

    std::fs::write(
        &script,
        "\
click 485 189
click 485 329
key ENTER
assert screen=MissionStory
key ENTER
assert screen=MissionStory
key ESC
assert screen=InGame
",
    )
    .expect("write the QA script");

    let config = QaConfig::new(&dir)
        .with_data_dir(game_data_dir())
        .with_assets_dir(root().join("assets"));
    let mut runner = QaRunner::new(config).expect("QA runner with the real assets");
    let report = runner.run_file(&script);

    assert!(report.passed, "{:?}", report.failures);
    assert!(
        report.log.contains("screen=MissionStory"),
        "the scripted driver must show the intro story"
    );
    assert!(runner.app().local_active(), "the mission is loaded");

    // The same bytes through the public V70 loader; the story screen is pure
    // presentation, so the state after it must match.
    let setup = max_app::local::LocalSetup {
        player_name: "Commander".to_string(),
        ..max_app::local::LocalSetup::default()
    };
    let bytes = std::fs::read(game_data_dir().join("SAVE1.CAM")).expect("SAVE1.CAM");
    let mut direct = app_in(&dir);
    direct
        .start_local_game_from_save(&setup, &bytes)
        .expect("direct load of campaign 1");

    assert_eq!(
        runner.app().snapshot(),
        direct.snapshot(),
        "the intro story must not change the mission snapshot"
    );

    std::fs::remove_dir_all(&dir).ok();
}
