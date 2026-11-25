//! Owner conditions for C3/C4/D2: the media layers are presentation only.
//!
//! * a game runs with and without the movie/music content and reaches byte-identical
//!   `snapshot_bytes`/`state_digest` (no content in the CRC);
//! * the music director reads the facade `EventStream` and mutates nothing;
//! * every missing layer is a visible diagnostic (log/status), never a panic.
//!
//! The real-data parts require `max-original/`; a missing data set fails the test.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use max_app::audio::{AudioBackend, AudioPlayer, PlayMode};
use max_app::intro::IntroScreen;
use max_app::music::{MusicDirector, MusicScene, GAME_PLAYLIST, PLANET_TRACKS, TRACK_MENU};
use max_app::subtitles::SubtitleTrack;
use max_app::{App, AppConfig};
use max_assets::WavClip;
use max_core::enums::{Planet, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn real_config() -> Option<AppConfig> {
    let root = root();
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("tests/fixtures/assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        return None;
    }

    Some(AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        world_dir: Some(root.join("max-original")),
        ..Default::default()
    })
}

fn app(config: AppConfig) -> App {
    let mut app = App::new(config);

    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().teams_mut()[1].team_type = TeamKind::Computer;
    app.universe_mut().team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    app.universe_mut().team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);
    app.universe_mut().set_active_turn_team(TeamId::RED);
    app.universe_mut().set_player_team(TeamId::RED);
    let _ = app.universe_mut().spawn_unit(
        UnitType::Tank,
        TeamId::RED,
        Point::new(10, 10),
        max_core::enums::unit_flags::MOBILE_LAND_UNIT | max_core::enums::unit_flags::SELECTABLE,
    );

    app
}

#[derive(Debug, Default)]
struct SilentBackend {
    started: Arc<Mutex<usize>>,
}

impl AudioBackend for SilentBackend {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn play(&mut self, _clip: Arc<WavClip>, _mode: PlayMode) -> Result<(), String> {
        Ok(())
    }

    fn play_music(&mut self, _clip: Arc<WavClip>, _gain: f32) -> Result<(), String> {
        *self.started.lock().expect("lock") += 1;

        Ok(())
    }
}

/// Loading the real game data must not change the canonical state bytes.
#[test]
fn snapshot_bytes_do_not_depend_on_the_loaded_content() {
    let config = real_config().expect(concat!("test fixture is missing: ", stringify!(real_config())));
    let mut with_content = app(config.clone());
    let mut without_content = app(config);

    assert!(
        with_content.init_assets().is_ok(),
        "данные игры загружаются"
    );
    assert!(with_content.assets().is_some());
    assert!(without_content.assets().is_none());

    for _ in 0..10 {
        with_content.tick();
        without_content.tick();
    }

    assert_eq!(
        with_content.state_ref().snapshot_bytes(),
        without_content.state_ref().snapshot_bytes(),
        "наличие MAX.RES/MAXINT.MVE/*.MSC не входит в snapshot_bytes"
    );
    assert_eq!(
        with_content.state_ref().state_digest(),
        without_content.state_ref().state_digest()
    );
    assert_eq!(
        with_content.digest().state_digest,
        without_content.digest().state_digest
    );
}

/// The intro (video + audio + captions) and the music director run next to the
/// simulation without touching it.
#[test]
fn running_the_media_layers_never_changes_the_state() {
    let config = real_config().unwrap_or_default();
    let mut presented = app(config.clone());
    let mut control = app(config);

    presented.universe_mut().begin_local_round();
    control.universe_mut().begin_local_round();

    // The real movie if present, otherwise the placeholder path: both must be inert.
    let candidate = root().join("max-original/MAXINT.MVE");
    let mve = candidate.is_file().then_some(candidate);
    let caption = SubtitleTrack::parse_json(
        r#"{ "subtitle": { "en-US": [ { "start": 0, "end": 100000, "caption": "media" } ] } }"#,
        "en-US",
    )
    .expect("subtitle track");
    let mut intro = IntroScreen::with_media(mve, Some(caption));
    let started = Arc::new(Mutex::new(0usize));
    let mut audio = AudioPlayer::with_backend(Box::new(SilentBackend {
        started: started.clone(),
    }));

    for tag in GAME_PLAYLIST
        .iter()
        .chain(PLANET_TRACKS.iter())
        .chain([TRACK_MENU].iter())
    {
        audio.preload_music(
            tag,
            WavClip {
                sample_rate: 22_050,
                channels: 2,
                samples: vec![0, 0],
            },
        );
    }

    let mut music = MusicDirector::new();

    for _ in 0..20 {
        presented.tick();
        control.tick();

        let events = presented.take_music_events();
        let team = presented.state_ref().player_team();
        music.sync(
            MusicScene::InGame,
            &events,
            team,
            Planet::Snowcrab,
            &mut audio,
        );
        let _ = intro.advance(0.1);
        let _ = intro.take_audio();
    }

    assert_eq!(
        presented.state_ref().snapshot_bytes(),
        control.state_ref().snapshot_bytes()
    );
    assert_eq!(
        presented.state_ref().state_digest(),
        control.state_ref().state_digest()
    );
    assert_eq!(*started.lock().expect("lock"), 1, "один трек на сцену");
}

/// Fallback condition: no movie, no music files, no subtitles — the game still runs and
/// every layer reports why it is silent.
#[test]
fn missing_media_is_visible_and_does_not_break_the_game() {
    let config = AppConfig {
        max_res: PathBuf::from("no-such-dir/MAX.RES"),
        ..Default::default()
    };
    let mut app = app(config);
    let mut audio = AudioPlayer::new();
    let mut music = MusicDirector::new();
    let mut intro = IntroScreen::new(Some(PathBuf::from("no-such-dir/maxint.mve")));

    // The missing movie is an error string, not a panic; the placeholder still paints.
    assert!(!intro.is_loaded());
    assert!(intro.error().is_some());
    assert!(intro.take_audio().is_empty());

    // A music request without files is a visible diagnostic, plays nothing.
    music.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    assert_eq!(music.plays(), 0);
    assert!(music.last_error().is_some());
    assert!(!music.messages().is_empty());

    // The match keeps ticking.
    for _ in 0..5 {
        app.tick();
        let _ = app.take_music_events();
    }

    assert!(app.ticks() > 0);
}
