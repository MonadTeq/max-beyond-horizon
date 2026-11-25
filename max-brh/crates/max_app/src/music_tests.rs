use std::sync::{Arc, Mutex};

use max_assets::WavClip;
use max_core::enums::WinLossState;
use max_core::game::{TurnEvent, TEAM_SLOTS};
use max_core::team::TeamId;
use max_core::victory::WinLossStatus;

use super::*;
use crate::audio::{AudioBackend, AudioPlayer, PlayMode};

/// Backend without a device that remembers the music queue.
#[derive(Debug, Default)]
struct MusicBackend {
    volumes: Arc<Mutex<Vec<f32>>>,
    playing: Arc<Mutex<bool>>,
    audible: bool,
}

impl AudioBackend for MusicBackend {
    fn name(&self) -> &'static str {
        if self.audible {
                "recording"
        } else {
                "noop"
        }
    }

    fn play(&mut self, _clip: Arc<WavClip>, _mode: PlayMode) -> Result<(), String> {
        Ok(())
    }

    fn play_music(&mut self, _clip: Arc<WavClip>, gain: f32) -> Result<(), String> {
        *self.playing.lock().expect("lock") = true;
        self.volumes.lock().expect("lock").push(gain);

        Ok(())
    }

    fn stop_music(&mut self) {
        *self.playing.lock().expect("lock") = false;
    }

    fn set_music_volume(&mut self, _volume: f32) {}

    fn music_playing(&self) -> bool {
        *self.playing.lock().expect("lock")
    }
}

fn director_player(audible: bool) -> (MusicDirector, AudioPlayer) {
    let backend = MusicBackend {
        volumes: Arc::new(Mutex::new(Vec::new())),
        playing: Arc::new(Mutex::new(false)),
        audible,
    };
    let mut player = AudioPlayer::with_backend(Box::new(backend));

    // The clips are preloaded: the test checks the director's decisions, not the file
    // system (`AudioPlayer::play_music` finds them in the music cache).
    for tag in GAME_PLAYLIST
        .iter()
        .chain(PLANET_TRACKS.iter())
        .chain([TRACK_MENU, TRACK_WIN, TRACK_LOSE].iter())
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

    (MusicDirector::new(), player)
}

fn turn_notice(status: Option<WinLossStatus>) -> GameEvent {
    GameEvent::Turn(TurnNotice {
        event: TurnEvent::Human(TeamId(0)),
        previous_player_team: TeamId(1),
        status,
    })
}

fn status_for(team: usize, state: WinLossState) -> WinLossStatus {
    let mut status = WinLossStatus {
        mission_type: 0,
        team_type: [0; TEAM_SLOTS],
        team_status: [WinLossState::Generic; TEAM_SLOTS],
        team_rank: [0; TEAM_SLOTS],
        teams_total: 2,
        teams_in_play: 2,
        teams_humans_in_play: 2,
        teams_non_computers: 0,
    };

    status.team_status[team] = state;

    status
}

#[test]
fn menu_music_is_requested_once_per_scene() {
    let (mut director, mut audio) = director_player(true);

    director.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    director.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );

    assert_eq!(director.current(), Some(TRACK_MENU));
    assert_eq!(
        director.plays(),
        1,
            "the same scene does not restart the track"
    );
}

#[test]
fn a_turn_notice_starts_the_planet_track_and_the_outcome_track() {
    let (mut director, mut audio) = director_player(true);
    let events = [turn_notice(None)];

    director.sync(
        MusicScene::InGame,
        &events,
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    assert_eq!(director.current(), Some("SNOW_MSC"));

    // Another planet group (`world / 6`): the North Pole group stays for 0..5.
    let (mut other, mut audio2) = director_player(true);
    other.sync(
        MusicScene::InGame,
        &[],
        TeamId(0),
        Planet::Sandspit,
        &mut audio2,
    );
    assert_eq!(other.current(), Some("DSRT_MSC"));

    // Victory of the local team (0): the outcome track replaces the world music.
    let victory = [turn_notice(Some(status_for(0, WinLossState::Won)))];
    director.on_events(&victory, TeamId(0));
    assert_eq!(director.outcome(), Some(MusicOutcome::Win));

    director.sync(
        MusicScene::InGame,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    assert_eq!(director.current(), Some(TRACK_WIN));

    // A loss of another team is not the local outcome.
    let (mut third, mut audio3) = director_player(true);
    let loss = [turn_notice(Some(status_for(2, WinLossState::Lost)))];
    third.on_events(&loss, TeamId(0));
    assert_eq!(third.outcome(), None);

    third.sync(
        MusicScene::InGame,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio3,
    );
    assert_eq!(third.current(), Some("SNOW_MSC"));
}

#[test]
fn the_shuffle_playlist_rotates_after_the_base_track() {
    let (mut director, mut audio) = director_player(true);

    director.sync(
        MusicScene::InGame,
        &[],
        TeamId(0),
        Planet::Frigia,
        &mut audio,
    );
    assert_eq!(director.current(), Some("SNOW_MSC"));
    assert!(audio.music_playing(), "трек идёт");

    // The track has ended (the backend reports it is empty): the next playlist
    // track is queued instead of the base one.
    audio.stop_music();
    assert!(director.advance_if_finished(&mut audio, Planet::Frigia));
    assert_ne!(director.current(), Some("SNOW_MSC"));
    assert!(GAME_PLAYLIST.contains(&director.current().expect("track")));

    // The silent backend cannot tell a finished track from a silent one.
    let (mut silent, mut audio) = director_player(false);
    silent.sync(
        MusicScene::InGame,
        &[],
        TeamId(0),
        Planet::Frigia,
        &mut audio,
    );
    silent.set_enabled(true, &mut audio);
    assert!(!silent.advance_if_finished(&mut audio, Planet::Frigia));
}

#[test]
fn disable_music_stops_and_reenable_restarts() {
    let (mut director, mut audio) = director_player(true);
    director.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    assert_eq!(director.current(), Some(TRACK_MENU));

    director.set_enabled(false, &mut audio);
    assert!(!director.enabled());
    assert_eq!(director.current(), None);
    assert!(director
        .messages()
        .iter()
        .any(|m| m.contains("disable_music")));

    director.set_enabled(true, &mut audio);
    director.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    assert_eq!(director.current(), Some(TRACK_MENU));
}

#[test]
fn a_track_without_data_is_a_visible_diagnostic() {
    let mut director = MusicDirector::new();
    let mut audio = AudioPlayer::new();

    director.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );

    assert_eq!(director.current(), Some(TRACK_MENU));
    assert_eq!(director.plays(), 0);
    assert!(director.last_error().is_some());
    assert!(director
        .messages()
        .iter()
        .any(|message| message.contains(TRACK_MENU)));
}

/// A campaign story track (`menu.cpp:803-805`) suspends the scene cue while the
/// story screen is open; closing the screen resumes the scene track.
#[test]
fn a_story_cue_suspends_and_then_restores_the_scene_track() {
    let (mut director, mut audio) = director_player(true);

    director.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    assert_eq!(director.current(), Some(TRACK_MENU));

    audio.preload_music(
            "STORY_MSC",
        WavClip {
            sample_rate: 22_050,
            channels: 2,
            samples: vec![0, 0],
        },
    );
    assert!(director.play_story("STORY_MSC", &mut audio));
    assert!(director.story_active());
    assert_eq!(director.current(), None, "the scene cue is suspended");

    // The frame loop keeps calling `sync`, but the story track stays in charge.
    director.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    assert_eq!(director.current(), None);

    director.end_story();
    director.sync(
        MusicScene::Menu,
        &[],
        TeamId(0),
        Planet::Snowcrab,
        &mut audio,
    );
    assert!(!director.story_active());
    assert_eq!(
        director.current(),
        Some(TRACK_MENU),
            "the scene cue resumes"
    );
}
