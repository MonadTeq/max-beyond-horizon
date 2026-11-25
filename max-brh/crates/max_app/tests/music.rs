//! D2: background music — the event stream, the track choice and the fallbacks.
//!
//! The director is driven exactly like in the client: [`App::take_music_events`] drains
//! the facade event stream, [`MusicDirector::sync`] picks the track and
//! [`crate::max_app::audio::AudioPlayer`] plays it. The tests use a recording backend, so
//! no audio device is needed; the real `MAX.RES`/`*.MSC` checks require the game
//! data and fail without it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use max_app::audio::{AudioBackend, AudioPlayer, PlayMode};
use max_app::music::{
    MusicDirector, MusicScene, GAME_PLAYLIST, PLANET_TRACKS, TRACK_MENU, TRACK_WIN,
};
use max_app::{App, AppConfig};
use max_assets::{ResArchive, WavClip};
use max_core::enums::{Planet, TeamClan};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::GameEvent;

/// Backend without a device: remembers the music starts, reports "playing".
#[derive(Debug, Default)]
struct RecordingBackend {
    started: Arc<Mutex<usize>>,
    playing: Arc<Mutex<bool>>,
}

impl AudioBackend for RecordingBackend {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn play(&mut self, _clip: Arc<WavClip>, _mode: PlayMode) -> Result<(), String> {
        Ok(())
    }

    fn play_music(&mut self, _clip: Arc<WavClip>, _gain: f32) -> Result<(), String> {
        *self.started.lock().expect("lock") += 1;
        *self.playing.lock().expect("lock") = true;

        Ok(())
    }

    fn stop_music(&mut self) {
        *self.playing.lock().expect("lock") = false;
    }

    fn music_playing(&self) -> bool {
        *self.playing.lock().expect("lock")
    }
}

fn player_with_tracks() -> (AudioPlayer, Arc<Mutex<usize>>) {
    let started = Arc::new(Mutex::new(0));
    let mut player = AudioPlayer::with_backend(Box::new(RecordingBackend {
        started: started.clone(),
        playing: Arc::new(Mutex::new(false)),
    }));

    for tag in GAME_PLAYLIST
        .iter()
        .chain(PLANET_TRACKS.iter())
        .chain([TRACK_MENU, TRACK_WIN, max_app::music::TRACK_LOSE].iter())
    {
        player.preload_music(
            tag,
            WavClip {
                sample_rate: 22_050,
                channels: 2,
                samples: vec![0, 0],
            },
        );
    }

    (player, started)
}

fn new_app() -> App {
    let mut app = App::new(AppConfig::default());

    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().teams_mut()[1].team_type = TeamKind::Computer;
    app.universe_mut().team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    app.universe_mut().team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);
    app
}

#[test]
fn the_director_reads_turn_notices_from_the_facade_stream() {
    let mut app = new_app();
    let (mut audio, started) = player_with_tracks();
    let mut music = MusicDirector::new();

    app.universe_mut().begin_local_round();

    let events = app.take_music_events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, GameEvent::Turn(_))),
        "начало партии публикуется как GameEvent::Turn"
    );
    // The cursor is consumed: the same event is not re-read next frame.
    assert!(app.take_music_events().is_empty());

    music.sync(
        MusicScene::InGame,
        &events,
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );

    assert_eq!(music.current(), Some("SNOW_MSC"));
    assert_eq!(*started.lock().expect("lock"), 1);
    assert_eq!(audio.music_name(), Some("snow_msc.msc"));
}

#[test]
fn draining_the_music_cursor_does_not_touch_the_snapshot() {
    let mut app = new_app();
    let before = app.state_ref().snapshot_bytes();
    let digest_before = app.state_ref().state_digest();

    app.universe_mut().begin_local_round();
    for _ in 0..5 {
        app.tick();
        let _events = app.take_music_events();
    }

    // The state advanced (ticks), but the event cursor itself is not state: a second
    // application that never drains the music cursor reaches the same bytes.
    let mut control = new_app();
    control.universe_mut().begin_local_round();
    for _ in 0..5 {
        control.tick();
    }

    assert_ne!(
        before,
        app.state_ref().snapshot_bytes(),
        "тики двигают состояние"
    );
    assert_eq!(
        app.state_ref().snapshot_bytes(),
        control.state_ref().snapshot_bytes(),
        "курсор событий музыки не входит в snapshot_bytes"
    );
    assert_eq!(
        app.state_ref().state_digest(),
        control.state_ref().state_digest()
    );
    let _ = digest_before;
}

#[test]
fn disable_music_is_kept_by_the_director() {
    let mut app = new_app();
    let (mut audio, _started) = player_with_tracks();
    let mut music = MusicDirector::new();

    app.set_preferences({
        let mut preferences = app.preferences().clone();
        preferences.disable_music = true;
        preferences.music_level = 40;
        preferences
    });

    music.set_enabled(app.preferences().music_enabled(), &mut audio);
    music.set_volume(app.preferences().music_volume(), &mut audio);
    music.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );

    assert!(!music.enabled());
    assert_eq!(
        music.current(),
        None,
        "disable_music: трек не запрашивается"
    );
    assert!((audio.music_volume() - 0.4).abs() < 1e-6);
}

#[test]
fn real_max_res_references_the_music_tracks() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original");
    let max_res = dir.join("MAX.RES");

    assert!(max_res.is_file(), concat!("required test fixture is missing: ", stringify!(!max_res.is_file())));

    let res = ResArchive::open(&max_res).expect("MAX.RES");
    let mut audio = AudioPlayer::new();
    audio.attach_res(&res);
    assert!(audio.set_sound_dir(&dir), "*.MSC files next to MAX.RES");

    // `resourcetable.cpp:213-215`: the tags are file references. The shipped data has
    // `MAIN_MSC` + `BKG1..BKG6` (the oracle's playlist also lists `BKG7..BKG9`, which the
    // retail data does not ship — a missing track must simply stay silent).
    let present = [
        "MAIN_MSC", "BKG1_MSC", "BKG2_MSC", "BKG3_MSC", "BKG4_MSC", "BKG5_MSC", "BKG6_MSC",
        "SNOW_MSC", "CRTR_MSC", "GREN_MSC", "DSRT_MSC", "WINR_MSC", "LOSE_MSC",
    ];

    for tag in present {
        assert!(audio.has_music(tag), "{tag} → *.msc");
    }

    // A playlist entry without a file is a fallback case, not a panic.
    for tag in ["BKG7_MSC", "BKG8_MSC", "BKG9_MSC"] {
        let _ = audio.has_music(tag);
    }
}

/// The music path is lazy: `play_music` opens the `*.MSC` headers and the PCM is
/// decoded at the first playback, then handed to the backend once.
#[test]
fn real_music_plays_through_the_lazy_wav_source() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original");
    let max_res = dir.join("MAX.RES");

    assert!(
        max_res.is_file(),
        "required test fixture is missing: {}",
        max_res.display()
    );

    let res = ResArchive::open(&max_res).expect("MAX.RES");
    let started = Arc::new(Mutex::new(0));
    let mut audio = AudioPlayer::with_backend(Box::new(RecordingBackend {
        started: started.clone(),
        playing: Arc::new(Mutex::new(false)),
    }));

    audio.attach_res(&res);
    assert!(audio.set_sound_dir(&dir), "*.MSC files next to MAX.RES");
    assert!(audio.play_music("MAIN_MSC", 0.5));

    assert_eq!(audio.music_name(), Some("main_msc.msc"));
    assert_eq!(*started.lock().expect("lock"), 1);
    assert_eq!(audio.plays(), 1);
    assert!(audio.errors().is_empty(), "{:?}", audio.errors());
}

/// The music documents are ordinary stereo PCM16 WAVs (the oracle feeds them to
/// miniaudio as files): 22050 Hz, 2 channels.
#[test]
fn real_msc_tracks_are_stereo_pcm16_wav() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original");
    let path: PathBuf = dir.join("MAIN_MSC.MSC");

    assert!(path.is_file(), concat!("required test fixture is missing: ", stringify!(!path.is_file())));

    let clip = WavClip::open(&path).expect("MAIN_MSC.MSC");

    assert_eq!(clip.sample_rate, 22_050);
    assert_eq!(clip.channels, 2);
    assert!(clip.frames() > 22_050, "трек длиннее секунды");
}
