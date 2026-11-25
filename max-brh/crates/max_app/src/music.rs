//! Background music (D2) — `SoundManager::PlayMusic` / `UpdateMusic`.
//!
//! What the oracle plays (`max-cpp-port/src/sound_manager.cpp`):
//!
//! * main menu — `PlayMusic(MAIN_MSC, false)` (`main.cpp:43`, `menu.cpp:2558`);
//! * a match — `PlayMusic(world / 6 + SNOW_MSC, true)` (`game_manager.cpp:1967/3422`):
//!   the planet group (`world / 6`) picks the base track, `shuffle` switches
//!   `UpdateMusic` to a random track of the `MAIN_MSC..BKG9_MSC` playlist when the current
//!   one ends (the playlist refills when exhausted);
//! * victory/defeat — `WINR_MSC` / `LOSE_MSC` (`menu.cpp:476/479`);
//! * a campaign story screen — `StoryBlock::music` (`menu.cpp:803-805`):
//!   [`MusicDirector::play_story`] suspends the scene cue until the screen closes;
//! * `disable_music` — [`crate::Preferences::disable_music`]: `PlayMusic` records the
//!   request but starts nothing (`HaltMusicPlayback` stops the current track).
//!
//! The music files are `*.MSC` next to `MAX.RES`: ordinary PCM16 WAV documents
//! (`RIFF/WAVE`, stereo 22050 Hz, ~7 minutes), referenced by `MAX.RES` tags
//! (`MAIN_MSC` → `main_msc.msc`). They are decoded by [`max_assets::wav`] like
//! every other sound; playback goes through [`crate::audio::AudioPlayer`], so a build
//! without the `sound` feature (or without a device) stays silent and the game works.
//!
//! # Layer rules (owner condition)
//!
//! The director **reads the facade event stream** ([`GameEvent`]) and never the
//! `GameState`: the application drains its own cursor ([`crate::App::take_music_events`])
//! and hands the events over. The director cannot reach the universe, mutates nothing in
//! the simulation, and its whole state is presentation-only — it is not part of
//! `snapshot_bytes`/CRC. The "same snapshot with and without music" test lives in
//! `tests/media_presentation.rs`.
//!
//! The track rotation uses a private xorshift instead of the simulation RNG
//! (`Randomizer_Generate` in the oracle): the simulation RNG is part of the state and a
//! presentation layer must not advance it.

use max_core::enums::{Planet, WinLossState};
use max_core::team::TeamId;
use max_core::universe::{GameEvent, TurnNotice};

use crate::audio::AudioPlayer;

/// Menu track (`PlayMusic(MAIN_MSC, false)`).
pub const TRACK_MENU: &str = "MAIN_MSC";
/// Defeat track (`PlayMusic(LOSE_MSC, false)`).
pub const TRACK_LOSE: &str = "LOSE_MSC";
/// Victory track (`PlayMusic(WINR_MSC, false)`).
pub const TRACK_WIN: &str = "WINR_MSC";
/// Planet base tracks (`world / 6 + SNOW_MSC`, `resourcetable.cpp:213`).
pub const PLANET_TRACKS: [&str; 4] = ["SNOW_MSC", "CRTR_MSC", "GREN_MSC", "DSRT_MSC"];
/// The match shuffle playlist (`m_music_playlist`: `MAIN_MSC`..`BKG9_MSC`).
pub const GAME_PLAYLIST: [&str; 10] = [
    "MAIN_MSC", "BKG1_MSC", "BKG2_MSC", "BKG3_MSC", "BKG4_MSC", "BKG5_MSC", "BKG6_MSC", "BKG7_MSC",
    "BKG8_MSC", "BKG9_MSC",
];

/// Where the UI currently is — the screen cue of the music director.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicScene {
    /// Main menu and the dialogs opened from it (`MAIN_MSC`).
    Menu,
    /// The intro MVE (`Movie_PlayIntro`, `main.cpp:39-43`): the oracle starts
    /// `MAIN_MSC` only after the movie, so the menu cue must not overlap the
    /// movie audio.
    Intro,
    /// A match is running (`game_manager.cpp:1967`).
    InGame,
}

/// How the match ended for the local player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicOutcome {
    /// `WinLossState::Won` — `WINR_MSC`.
    Win,
    /// `WinLossState::Lost` — `LOSE_MSC`.
    Lose,
}

impl MusicOutcome {
    /// Resource tag of the outcome track.
    pub fn track(self) -> &'static str {
        match self {
            Self::Win => TRACK_WIN,
            Self::Lose => TRACK_LOSE,
        }
    }
}

/// Background music director (presentation only, event-driven).
#[derive(Debug, Clone)]
pub struct MusicDirector {
    /// `disable_music` (`Preferences`).
    enabled: bool,
    /// `music_level / 100`.
    volume: f32,
    /// Currently requested track (resource tag).
    current: Option<&'static str>,
    /// Last track that failed to load (diagnostics).
    last_error: Option<String>,
    /// Visible diagnostics (fallbacks, failed tracks) — the client prints them.
    messages: Vec<String>,
    /// A match has been entered (`PayMusic` of `game_manager.cpp`).
    in_game: bool,
    /// Match outcome seen in the event stream.
    outcome: Option<MusicOutcome>,
    /// A campaign story screen plays its own track (`StoryBlock::music`,
    /// `menu.cpp:803-805`): the scene cue is suspended until
    /// [`MusicDirector::end_story`].
    story_active: bool,
    /// Playlist of the shuffle mode (`m_music_playlist`).
    playlist: [bool; GAME_PLAYLIST.len()],
    /// Presentation-only PRNG (xorshift; never the simulation `Randomizer`).
    random: u32,
    /// How many tracks were queued on the backend.
    plays: u64,
}

impl Default for MusicDirector {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicDirector {
    /// A silent director: nothing is requested until [`Self::sync`] sees a scene.
    pub fn new() -> Self {
        Self {
            enabled: true,
            volume: 1.0,
            current: None,
            last_error: None,
            messages: Vec::new(),
            in_game: false,
            outcome: None,
            story_active: false,
            playlist: [true; GAME_PLAYLIST.len()],
            random: 0x9E37_79B9,
            plays: 0,
        }
    }

    /// `disable_music`: stopping is immediate, re-enabling restarts the scene track.
    pub fn set_enabled(&mut self, enabled: bool, audio: &mut AudioPlayer) {
        if self.enabled == enabled {
            return;
        }

        self.enabled = enabled;

        if !enabled {
            self.current = None;
            audio.stop_music();
            self.log("музыка выключена (disable_music)".to_string());
        }
    }

    /// `music_level` (0..=100 in `Preferences`).
    pub fn set_volume(&mut self, volume: f32, audio: &mut AudioPlayer) {
        self.volume = volume.clamp(0.0, 1.0);
        audio.set_music_volume(self.volume);
    }

    /// Current track tag (for diagnostics/tests).
    pub fn current(&self) -> Option<&'static str> {
        self.current
    }

    /// Whether the director is enabled (`!disable_music`).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// How many tracks were queued.
    pub fn plays(&self) -> u64 {
        self.plays
    }

    /// Visible diagnostics (missing files, disabled music, outcomes).
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// The last track that failed to play.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Where the match stands for the local player (from the event stream).
    pub fn outcome(&self) -> Option<MusicOutcome> {
        self.outcome
    }

    /// Whether a campaign story screen suspended the scene cue.
    pub fn story_active(&self) -> bool {
        self.story_active
    }

    /// Plays a story screen track (`StoryBlock::music`, `menu.cpp:803-805`).
    ///
    /// The scene cue is suspended while the story screen is open; the driver calls
    /// [`Self::end_story`] when it closes and the normal scene track is started
    /// again on the next [`Self::sync`]. `false` — the track could not be queued
    /// (missing `*.MSC`/reference). With `disable_music` nothing is played and the
    /// call reports `true` (the override is a no-op) — music is optional.
    pub fn play_story(&mut self, tag: &str, audio: &mut AudioPlayer) -> bool {
        if !self.enabled {
            return true;
        }

        if audio.play_music(tag, self.volume) {
            self.story_active = true;
            self.current = None;
            self.plays += 1;
            self.last_error = None;

            true
        } else {
            let reason = audio
                .last_error()
                .map(str::to_string)
                .unwrap_or_else(|| "нет файла *.MSC".to_string());

            self.last_error = Some(reason.clone());
            self.log(format!("музыка истории: {tag} не запущена ({reason})"));

            false
        }
    }

    /// Ends the story-screen override; the scene cue resumes on the next
    /// [`Self::sync`].
    pub fn end_story(&mut self) {
        if self.story_active {
            self.story_active = false;
            self.current = None;
        }
    }

    /// Applies the facade events to the music state (no simulation access).
    ///
    /// `Turn` notices are the only events the director cares about: the first one means
    /// the match has started (world music), and a notice with a `WinLossStatus` means the
    /// match is over for someone — the local team's `Won`/`Lost` picks the outcome track.
    pub fn on_events(&mut self, events: &[GameEvent], player_team: TeamId) {
        for event in events {
            let GameEvent::Turn(notice) = event else {
                continue;
            };

            self.in_game = true;

            if let Some(outcome) = outcome_of(notice, player_team) {
                self.outcome = Some(outcome);
                self.log(format!(
                    "музыка: исход матча — {}",
                    match outcome {
                        MusicOutcome::Win => "победа",
                        MusicOutcome::Lose => "поражение",
                    }
                ));
            }
        }
    }

    /// Plays what the current scene and the event stream ask for.
    ///
    /// Called once per frame by the client. A missing file is a diagnostic, never an
    /// error of the game: the frame keeps running without music.
    pub fn sync(
        &mut self,
        scene: MusicScene,
        events: &[GameEvent],
        player_team: TeamId,
        planet: Planet,
        audio: &mut AudioPlayer,
    ) {
        self.on_events(events, player_team);

        if !self.enabled || self.story_active {
            return;
        }

        if let Some(track) = self.desired(scene, planet) {
            if self.current != Some(track) {
                self.start(track, audio);
            }
        } else if self.current.is_some() {
            // Silence cue (`Intro`): stop the menu/match track before the movie
            // audio starts, and let the next scene restart its own track.
            self.current = None;
            audio.stop_music();
        }
    }

    /// The track the scene asks for (`None` — silence).
    fn desired(&mut self, scene: MusicScene, planet: Planet) -> Option<&'static str> {
        if let Some(outcome) = self.outcome {
            return Some(outcome.track());
        }

        match scene {
            MusicScene::Menu => Some(TRACK_MENU),
            MusicScene::Intro => None,
            MusicScene::InGame => {
                if !self.in_game {
                    // The scene says "match", the stream has not confirmed it yet: the
                    // base planet track is the safe cue (`PlayMusic(world/6 + SNOW_MSC)`).
                    self.in_game = true;
                }

                Some(self.planet_track(planet))
            }
        }
    }

    /// Base track of the planet group (`world / 6`, `game_manager.cpp:1967`).
    pub fn planet_track(&self, planet: Planet) -> &'static str {
        let index = usize::from(planet as u8 / 6).min(PLANET_TRACKS.len() - 1);

        PLANET_TRACKS[index]
    }

    /// Queues a track on the audio layer.
    fn start(&mut self, track: &'static str, audio: &mut AudioPlayer) {
        self.current = Some(track);

        if audio.play_music(track, self.volume) {
            self.plays += 1;
            self.last_error = None;
        } else {
            let reason = audio
                .last_error()
                .map(str::to_string)
                .unwrap_or_else(|| "нет файла *.MSC".to_string());

            self.last_error = Some(reason.clone());
            self.log(format!("музыка: {track} не запущена ({reason})"));
        }
    }

    /// Chooses the next shuffle track when the current one has ended.
    ///
    /// `true` — a new track was queued. With the silent backend (no device/build without
    /// the `sound` feature) the rotation is not attempted: `music_playing` cannot tell a
    /// finished track from a silent one.
    pub fn advance_if_finished(&mut self, audio: &mut AudioPlayer, planet: Planet) -> bool {
        if !self.enabled || self.outcome.is_some() || self.story_active || self.current.is_none() {
            return false;
        }

        if !audio.is_audible() || audio.music_playing() {
            return false;
        }

        let track = self.next_shuffle_track(planet);

        self.start(track, audio);

        true
    }

    /// Next track of the shuffle playlist (`UpdateMusic`); the planet base track first.
    fn next_shuffle_track(&mut self, planet: Planet) -> &'static str {
        let base = self.planet_track(planet);

        if self.current != Some(base) {
            return base;
        }

        let mut tries = 0usize;

        loop {
            let index = self.random_next() as usize % GAME_PLAYLIST.len();

            if self.playlist[index] {
                self.playlist[index] = false;
                let track = GAME_PLAYLIST[index];

                if Some(track) != self.current {
                    return track;
                }
            }

            tries += 1;

            if tries > GAME_PLAYLIST.len() {
                self.playlist = [true; GAME_PLAYLIST.len()];
            }
        }
    }

    /// xorshift32 — deterministic, seeded presentation randomness.
    fn random_next(&mut self) -> u32 {
        let mut value = self.random | 1;

        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        self.random = value;

        value
    }

    fn log(&mut self, message: String) {
        const MAX_MESSAGES: usize = 8;

        if !self.messages.contains(&message) {
            if self.messages.len() >= MAX_MESSAGES {
                self.messages.remove(0);
            }

            self.messages.push(message);
        }
    }
}

/// The local team's outcome of a `Turn` notice, if the match is over.
fn outcome_of(notice: &TurnNotice, player_team: TeamId) -> Option<MusicOutcome> {
    let status = notice.status?;
    let team = usize::from(player_team.0);

    if team >= status.team_status.len() {
        return None;
    }

    match status.team_status[team] {
        WinLossState::Won => Some(MusicOutcome::Win),
        WinLossState::Lost => Some(MusicOutcome::Lose),
        _ => None,
    }
}

#[cfg(test)]
#[path = "music_tests.rs"]
mod tests;
