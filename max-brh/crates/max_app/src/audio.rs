//! Application sound (M6.2): WAV → PCM buffer → playback.
//!
//! The module's job is not a "sound engine" but a minimal non-blocking layer over
//! already decoded sounds:
//!
//! * [`max_assets::wav`] decodes `*.SPW` (PCM16) into [`WavClip`] — this
//!   works **always**, without external dependencies;
//! * [`AudioPlayer`] caches clips, resolves names through the reference resources
//!   `MAX.RES` (`TANK1` → `"tank1.spw"`, `max-original-dasm/formats/spw.md` §1)
//!   and hands them to the backend;
//! * [`AudioBackend`] — the output connection point. There is always a silent
//!   [`NoopBackend`]; behind the `sound` feature — [`RodioBackend`] (rodio/cpal).
//!
//! # Feature `sound`
//!
//! Base builds (`cargo check`/`cargo test`/docker) do not pull ALSA/CPAL:
//! rodio is an optional dependency, and `AudioPlayer::new()` creates a no-op backend.
//! Only `AudioPlayer::with_default_backend()` in a build with `--features sound`
//! tries to open an audio device. If there is no device (headless/CI), the player
//! stays on no-op and writes the reason to [`AudioPlayer::errors`] — there is no crash.
//!
//! Playback does not block: rodio plays samples in its own thread,
//! and the [`AudioPlayer::play`] call only queues the clip.
//!
//! # Names
//!
//! [`AudioPlayer::play`] accepts either a resource tag (`"KBUY0"`, `"TANK1"`),
//! or a file name (`"kbuy0.spw"`). The file lookup is case-insensitive: on Linux
//! the reference from `MAX.RES` is lowercase (`tank1.spw`), while the file in the game directory
//! is uppercase (`TANK1.SPW`), so the directory is indexed by lowercase.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use max_assets::{parse_file_ref, ResArchive, WavClip};

/// Menu click resource tag (`KBUY0` → `kbuy0.spw`).
///
/// `spw.md` §8.3: `KBUY0`/`KCARG0` — the button press sound; used in
/// `ui.rs` on menu clicks.
pub const MENU_CLICK: &str = "KBUY0";

/// How many clips rodio holds at once (protection against "stuck" clicks).
#[cfg(feature = "sound")]
const MAX_CONCURRENT_SINKS: usize = 16;
/// Limit of diagnostic messages (errors must not grow indefinitely).
const MAX_ERRORS: usize = 16;

/// How to play the clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    /// Once.
    Once,
    /// Looped (background/engine).
    Loop,
}

/// Sound output. Implementations: [`NoopBackend`] and (behind the `sound` feature)
/// [`RodioBackend`].
///
/// The trait does not require `Send`: the cpal stream (`OutputStream`) is not
/// `Send`, while the player lives in the UI thread. Clips are passed as `Arc<WavClip>` and cloned
/// only at the moment of queueing.
pub trait AudioBackend: fmt::Debug {
    /// Backend name for logs/reports (`"noop"`, `"rodio/cpal"`).
    fn name(&self) -> &'static str;

    /// Queues the clip. Does not block.
    fn play(&mut self, clip: Arc<WavClip>, mode: PlayMode) -> Result<(), String> {
        self.play_scaled(clip, mode, 1.0)
    }

    /// Queues a short PCM chunk (MVE movie audio) without dropping what is queued.
    ///
    /// The default implementation forwards it to [`AudioBackend::play_scaled`]; a real
    /// backend keeps one streaming voice so that consecutive movie frames are gapless.
    fn play_pcm(&mut self, clip: Arc<WavClip>, gain: f32) -> Result<(), String> {
        self.play_scaled(clip, PlayMode::Once, gain)
    }

    /// Stops the movie audio voice.
    fn stop_pcm(&mut self) {}

    /// Starts a music track (`D2`), replacing the current one.
    ///
    /// The gain is `music_level / 100`; music has its own volume and is not affected by
    /// [`AudioBackend::set_volume`] (effects).
    fn play_music(&mut self, clip: Arc<WavClip>, gain: f32) -> Result<(), String> {
        self.play_scaled(clip, PlayMode::Once, gain)
    }

    /// Stops the music track (the effects keep playing).
    fn stop_music(&mut self) {}

    /// Music volume 0.0..=1.0, independent of the effects volume.
    fn set_music_volume(&mut self, _volume: f32) {}

    /// Whether a music track is still sounding (the director rotates tracks by this).
    fn music_playing(&self) -> bool {
        false
    }

    /// Queues the clip with additional gain `gain` (0..1+).
    ///
    /// Needed for unit sounds: `SC_UNITS` sets the volume per effect
    /// (`SoundManager::PlaySfx`: `volume / 100`), while the overall `FX` volume
    /// stays in [`AudioPlayer::set_volume`]. The default implementation
    /// delegates to [`AudioBackend::play`] (the gain is ignored).
    fn play_scaled(
        &mut self,
        clip: Arc<WavClip>,
        mode: PlayMode,
        _gain: f32,
    ) -> Result<(), String> {
        self.play(clip, mode)
    }

    /// Stops everything that is playing.
    fn stop_all(&mut self) {}

    /// Volume 0.0..=1.0 (the implementation may ignore it).
    fn set_volume(&mut self, _volume: f32) {}
}

/// Silent backend: accepts clips and does nothing.
///
/// This is the default behavior for builds without the `sound` feature and for tests:
/// the UI hooks remain in place, but ALSA/CPAL is not needed.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopBackend;

impl AudioBackend for NoopBackend {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn play(&mut self, _clip: Arc<WavClip>, _mode: PlayMode) -> Result<(), String> {
        Ok(())
    }
}

/// Player: clip cache, `MAX.RES` references and backend.
pub struct AudioPlayer {
    backend: Box<dyn AudioBackend>,
    sound_dir: Option<PathBuf>,
    /// lowercase file name → real path (FS case does not matter).
    files: HashMap<String, PathBuf>,
    /// uppercase resource tag → lowercase `.spw`/`.wav`/`.msc` name.
    references: BTreeMap<String, String>,
    /// lowercase file name → decoded clip.
    cache: HashMap<String, Arc<WavClip>>,
    /// Music clips by file name (preloaded or decoded once; the `*.MSC` files are tens
    /// of megabytes, so they are not kept in [`Self::cache`] together with the effects).
    music_cache: HashMap<String, Arc<WavClip>>,
    /// Currently requested music file (for diagnostics/tests).
    music_name: Option<String>,
    /// A music track was queued (the backend decides whether it is still sounding).
    music_started: bool,
    /// Music volume (`music_level / 100`), independent of the effects volume.
    music_volume: f32,
    errors: Vec<String>,
    last_error: Option<String>,
    enabled: bool,
    plays: u64,
    volume: f32,
}

impl AudioPlayer {
    /// Player with a silent backend and no data.
    pub fn new() -> Self {
        Self::with_backend(Box::new(NoopBackend))
    }

    /// Player with an arbitrary backend (tests, another sound engine).
    pub fn with_backend(backend: Box<dyn AudioBackend>) -> Self {
        Self {
            backend,
            sound_dir: None,
            files: HashMap::new(),
            references: BTreeMap::new(),
            cache: HashMap::new(),
            music_cache: HashMap::new(),
            music_name: None,
            music_started: false,
            music_volume: 1.0,
            errors: Vec::new(),
            last_error: None,
            enabled: true,
            plays: 0,
            volume: 1.0,
        }
    }

    /// Player for a real launch: rodio behind the `sound` feature, otherwise no-op.
    ///
    /// If the audio device did not open, a no-op player is returned with the reason
    /// in [`Self::errors`] — the client keeps working without sound.
    pub fn with_default_backend() -> Self {
        #[cfg(feature = "sound")]
        {
            match rodio_backend::RodioBackend::new() {
                Ok(backend) => Self::with_backend(Box::new(backend)),
                Err(error) => {
                    let mut player = Self::new();
                    player.push_error(format!("звук недоступен ({error}); включён no-op"));
                    player
                }
            }
        }

        #[cfg(not(feature = "sound"))]
        {
            Self::new()
        }
    }

    /// Name of the active backend.
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// Whether there is a real output (not [`NoopBackend`]).
    pub fn is_audible(&self) -> bool {
        self.backend.name() != "noop"
    }

    /// Whether sound is enabled (UI hooks are still called).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Enables/disables sound; disabling stops the queue.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;

        if !enabled {
            self.backend.stop_all();
        }
    }

    /// Volume 0.0..=1.0.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.backend.set_volume(self.volume);
    }

    /// Current volume.
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Sound directory (next to `MAX.RES`).
    pub fn sound_dir(&self) -> Option<&Path> {
        self.sound_dir.as_deref()
    }

    /// Indexes the sound directory (`*.SPW`/`*.WAV`/`*.MSC`, case does not matter).
    ///
    /// Returns `true` if at least one file was found. A missing directory is
    /// not an error: the client will simply be left without sound.
    pub fn set_sound_dir(&mut self, dir: impl Into<PathBuf>) -> bool {
        let dir = dir.into();
        let mut files = HashMap::new();

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_sound = path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("spw")
                        || extension.eq_ignore_ascii_case("wav")
                        || extension.eq_ignore_ascii_case("msc")
                });

                if !is_sound || !path.is_file() {
                    continue;
                }

                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    files.insert(name.to_ascii_lowercase(), path);
                }
            }
        }

        let found = !files.is_empty();
        self.files = files;
        self.sound_dir = Some(dir);

        found
    }

    /// Collects `tag → file` references from `MAX.RES` (`*.spw`, `*.wav`, `*.msc`).
    ///
    /// The body of a reference resource is an ASCIIZ file name (`TANK1` → `"tank1.spw"`,
    /// `MAIN_MSC` → `"main_msc.msc"`); regular resources (sprites/maps) are discarded by
    /// [`parse_file_ref`].
    pub fn attach_res(&mut self, res: &ResArchive) -> usize {
        let mut added = 0;

        for entry in res.list() {
            // References are tiny (10–16 bytes); 256 is a margin so as not to read
            // the bodies of sprites/maps.
            if entry.size == 0 || entry.size > 256 {
                continue;
            }

            let Ok(bytes) = res.read(&entry.name) else {
                continue;
            };
            let Some(name) = parse_file_ref(bytes) else {
                continue;
            };
            let file_name = name.to_ascii_lowercase();

            if !(file_name.ends_with(".spw")
                || file_name.ends_with(".wav")
                || file_name.ends_with(".msc"))
            {
                continue;
            }

            self.references
                .insert(entry.name.to_ascii_uppercase(), file_name);
            added += 1;
        }

        added
    }

    /// Binds `MAX.RES` and the sound directory in one operation.
    pub fn load_from(&mut self, res: &ResArchive, sound_dir: impl Into<PathBuf>) -> &mut Self {
        self.attach_res(res);
        self.set_sound_dir(sound_dir);
        self
    }

    /// How many tag references were collected from `MAX.RES`.
    pub fn reference_count(&self) -> usize {
        self.references.len()
    }

    /// How many sound files were indexed.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// How many clips were decoded and cached.
    pub fn cached_count(&self) -> usize {
        self.cache.len()
    }

    /// How many clips were successfully sent to the backend.
    pub fn plays(&self) -> u64 {
        self.plays
    }

    /// Diagnostics (broken WAV, missing file, no device).
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// The last error.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Puts a clip into the cache in advance (built-in sounds, tests).
    pub fn preload(&mut self, name: impl AsRef<str>, clip: WavClip) {
        let key = Self::file_key(name.as_ref());
        self.cache.insert(key, Arc::new(clip));
    }

    /// Whether there is a clip for the name (without reading the file).
    pub fn has_clip(&self, name: &str) -> bool {
        self.resolve(name).is_some()
    }

    /// Plays a sound by tag/name; `true` — the clip was queued.
    ///
    /// Does not block and does not return an error to the caller: the reason goes to
    /// [`Self::errors`], and the UI keeps working (sound is not a blocker).
    pub fn play(&mut self, name: &str, mode: PlayMode) -> bool {
        self.play_gain(name, mode, 1.0)
    }

    /// [`AudioPlayer::play`] with gain (unit sounds: `volume / 100`).
    pub fn play_gain(&mut self, name: &str, mode: PlayMode, gain: f32) -> bool {
        if !self.enabled {
            return false;
        }

        let Some(file_name) = self.resolve(name) else {
            self.push_error(format!("звук {name:?} не найден (нет файла или ссылки)"));

            return false;
        };
        let Some(clip) = self.clip_for(&file_name) else {
            return false;
        };

        match self.backend.play_scaled(clip, mode, gain.clamp(0.0, 4.0)) {
            Ok(()) => {
                self.plays += 1;
                true
            }
            Err(error) => {
                self.push_error(format!("{}: {error}", self.backend.name()));

                false
            }
        }
    }

    /// Once.
    pub fn play_once(&mut self, name: &str) -> bool {
        self.play(name, PlayMode::Once)
    }

    /// Looped.
    pub fn play_loop(&mut self, name: &str) -> bool {
        self.play(name, PlayMode::Loop)
    }

    /// Stops everything that is playing.
    pub fn stop_all(&mut self) {
        self.backend.stop_all();
    }

    // -----------------------------------------------------------------------
    // Movie audio (C3)
    // -----------------------------------------------------------------------

    /// Queues a decoded MVE audio chunk (PCM16, interleaved).
    ///
    /// The movie layer hands the chunks over frame by frame; a backend with a streaming
    /// voice keeps them gapless. The switch/volume is the caller's (`disable_movie`,
    /// `movie_level`) — the effects flag does not mute the movie (`movie_run` uses its
    /// own `MVE_sndVolume`). `false` — no chunk was queued (empty) or the backend refused
    /// (the reason goes to [`Self::errors`]).
    pub fn play_pcm(
        &mut self,
        samples: &[i16],
        channels: u8,
        sample_rate: u32,
        gain: f32,
    ) -> bool {
        if samples.is_empty() || channels == 0 || sample_rate == 0 {
            return false;
        }

        let clip = Arc::new(WavClip {
            sample_rate,
            channels: u16::from(channels),
            samples: samples.to_vec(),
        });

        match self.backend.play_pcm(clip, gain.clamp(0.0, 1.0)) {
            Ok(()) => {
                self.plays += 1;

                true
            }
            Err(error) => {
                self.push_error(format!("{}: {error}", self.backend.name()));

                false
            }
        }
    }

    /// Stops the movie audio voice (the movie ended / was interrupted).
    pub fn stop_movie_audio(&mut self) {
        self.backend.stop_pcm();
    }

    // -----------------------------------------------------------------------
    // Music (D2)
    // -----------------------------------------------------------------------

    /// Puts a music clip into the cache in advance (tests, built-in tracks).
    pub fn preload_music(&mut self, name: impl AsRef<str>, clip: WavClip) {
        let key = self.music_key(name.as_ref());

        self.music_cache.insert(key, Arc::new(clip));
    }

    /// Whether there is a music clip/file for the name.
    pub fn has_music(&self, name: &str) -> bool {
        self.resolve_music(name).is_some()
    }

    /// Currently requested music file name.
    pub fn music_name(&self) -> Option<&str> {
        self.music_name.as_deref()
    }

    /// Whether the backend still has the music track sounding.
    ///
    /// The silent backend cannot answer (it has no clock), so the music director only
    /// rotates tracks when [`Self::is_audible`] is true.
    pub fn music_playing(&self) -> bool {
        self.music_started && self.backend.music_playing()
    }

    /// Plays a music track by tag/name with gain (`music_level / 100`).
    ///
    /// Music has its own volume: [`Self::set_volume`] (effects) does not change it. A
    /// missing file is a diagnostic, never a blocking error.
    pub fn play_music(&mut self, name: &str, gain: f32) -> bool {
        self.stop_music();

        let Some(file_name) = self.resolve_music(name) else {
            self.push_error(format!("музыка {name:?}: нет файла или ссылки"));

            return false;
        };
        let Some(clip) = self.music_clip_for(&file_name) else {
            return false;
        };

        match self
            .backend
            .play_music(clip, gain.clamp(0.0, 1.0) * self.music_volume)
        {
            Ok(()) => {
                self.music_name = Some(file_name);
                self.music_started = true;
                self.plays += 1;

                true
            }
            Err(error) => {
                self.push_error(format!("{}: {error}", self.backend.name()));

                false
            }
        }
    }

    /// Stops the music track (the effects keep playing).
    pub fn stop_music(&mut self) {
        self.backend.stop_music();
        self.music_started = false;
        self.music_name = None;
    }

    /// Music volume 0.0..=1.0 (`music_level / 100`).
    pub fn set_music_volume(&mut self, volume: f32) {
        self.music_volume = volume.clamp(0.0, 1.0);
        self.backend.set_music_volume(self.music_volume);
    }

    /// Music volume.
    pub fn music_volume(&self) -> f32 {
        self.music_volume
    }

    /// `tag/name` → lowercase music file name, if a clip/file exists.
    fn resolve_music(&self, name: &str) -> Option<String> {
        let key = self.music_key(name);

        if self.music_cache.contains_key(&key) || self.files.contains_key(&key) {
            Some(key)
        } else {
            None
        }
    }

    /// Lowercase music file name: a tag with a resource reference resolves to its file,
    /// otherwise a short name gets the `.msc` extension.
    fn music_key(&self, name: &str) -> String {
        let lower = name.to_ascii_lowercase();

        if let Some(reference) = self.references.get(&name.to_ascii_uppercase()) {
            return reference.clone();
        }

        if lower.ends_with(".msc") || lower.ends_with(".wav") {
            lower
        } else {
            format!("{lower}.msc")
        }
    }

    /// Gets the music clip from the cache or decodes the file.
    ///
    /// The `*.MSC` tracks are 14–18 MB WAV files; only preloaded/converted clips stay in
    /// the cache, the file is read on every track change (one track at a time).
    fn music_clip_for(&mut self, file_name: &str) -> Option<Arc<WavClip>> {
        if let Some(clip) = self.music_cache.get(file_name) {
            return Some(clip.clone());
        }

        let path = self.files.get(file_name)?.clone();

        match WavClip::open(&path) {
            Ok(clip) => Some(Arc::new(clip)),
            Err(error) => {
                self.push_error(format!("{}: {error}", path.display()));

                None
            }
        }
    }

    /// `tag/name` → lowercase file name, if it is in the cache/directory.
    fn resolve(&self, name: &str) -> Option<String> {
        let lower = name.to_ascii_lowercase();
        let tag = name.to_ascii_uppercase();

        let file_name = self
            .references
            .get(&tag)
            .cloned()
            .unwrap_or_else(|| Self::file_key(&lower));

        if self.cache.contains_key(&file_name) || self.files.contains_key(&file_name) {
            Some(file_name)
        } else {
            None
        }
    }

    /// Lowercase file name: a short name gets the `.spw` extension.
    fn file_key(name: &str) -> String {
        let lower = name.to_ascii_lowercase();

        if lower.ends_with(".spw") || lower.ends_with(".wav") {
            lower
        } else {
            format!("{lower}.spw")
        }
    }

    /// Gets the clip from the cache or reads the file (with caching).
    fn clip_for(&mut self, file_name: &str) -> Option<Arc<WavClip>> {
        if let Some(clip) = self.cache.get(file_name) {
            return Some(clip.clone());
        }

        let path = self.files.get(file_name)?.clone();

        match WavClip::open(&path) {
            Ok(clip) => {
                let clip = Arc::new(clip);
                self.cache.insert(file_name.to_owned(), clip.clone());

                Some(clip)
            }
            Err(error) => {
                self.push_error(format!("{}: {error}", path.display()));

                None
            }
        }
    }

    fn push_error(&mut self, error: String) {
        self.last_error = Some(error.clone());

        if self.errors.len() < MAX_ERRORS && !self.errors.contains(&error) {
            self.errors.push(error);
        }
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for AudioPlayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioPlayer")
            .field("backend", &self.backend.name())
            .field("sound_dir", &self.sound_dir)
            .field("files", &self.files.len())
            .field("references", &self.references.len())
            .field("cache", &self.cache.len())
            .field("music_cache", &self.music_cache.len())
            .field("music_name", &self.music_name)
            .field("music_volume", &self.music_volume)
            .field("enabled", &self.enabled)
            .field("plays", &self.plays)
            .field("errors", &self.errors.len())
            .finish()
    }
}

/// Output implementation via rodio/cpal (only with the `sound` feature).
#[cfg(feature = "sound")]
mod rodio_backend {
    use std::fmt;
    use std::sync::Arc;

    use max_assets::WavClip;
    use rodio::{buffer::SamplesBuffer, OutputStream, OutputStreamHandle, Sink, Source};

    use super::{AudioBackend, PlayMode, MAX_CONCURRENT_SINKS};

    /// Non-blocking rodio backend: one `Sink` per clip; the movie voice and the music
    /// track keep their own sinks (gapless chunks, independent volumes).
    pub struct RodioBackend {
        /// Keeps the cpal stream alive; on `Drop` playback stops.
        _stream: OutputStream,
        handle: OutputStreamHandle,
        sinks: Vec<Sink>,
        /// Streaming voice of the MVE audio track (one sink, chunks are appended).
        movie_sink: Option<Sink>,
        /// Current music track (`D2`).
        music_sink: Option<Sink>,
        volume: f32,
        music_volume: f32,
    }

    impl fmt::Debug for RodioBackend {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("RodioBackend")
                .field("sinks", &self.sinks.len())
                .field("movie", &self.movie_sink.is_some())
                .field("music", &self.music_sink.is_some())
                .field("volume", &self.volume)
                .field("music_volume", &self.music_volume)
                .finish_non_exhaustive()
        }
    }

    impl RodioBackend {
        /// Opens the default output device.
        pub fn new() -> Result<Self, String> {
            let (stream, handle) =
                OutputStream::try_default().map_err(|error| error.to_string())?;

            Ok(Self {
                _stream: stream,
                handle,
                sinks: Vec::new(),
                movie_sink: None,
                music_sink: None,
                volume: 1.0,
                music_volume: 1.0,
            })
        }
    }

    impl AudioBackend for RodioBackend {
        fn name(&self) -> &'static str {
            "rodio/cpal"
        }

        fn play(&mut self, clip: Arc<WavClip>, mode: PlayMode) -> Result<(), String> {
            self.play_scaled(clip, mode, 1.0)
        }

        fn play_scaled(
            &mut self,
            clip: Arc<WavClip>,
            mode: PlayMode,
            gain: f32,
        ) -> Result<(), String> {
            // Do not keep finished clips; the queue is limited so that frequent
            // clicks do not accumulate threads.
            self.sinks.retain(|sink| !sink.empty());

            if self.sinks.len() >= MAX_CONCURRENT_SINKS {
                self.sinks.remove(0).stop();
            }

            let sink = Sink::try_new(&self.handle).map_err(|error| error.to_string())?;
            // rodio accepts a `Vec`; the clip is in an `Arc`, so we clone the PCM buffer
            // (M.A.X. clips are short — units to hundreds of KB).
            let source = SamplesBuffer::new(clip.channels, clip.sample_rate, clip.samples.clone());

            if mode == PlayMode::Loop {
                sink.append(source.repeat_infinite());
            } else {
                sink.append(source);
            }

            sink.set_volume(self.volume * gain.clamp(0.0, 4.0));
            self.sinks.push(sink);

            Ok(())
        }

        fn stop_all(&mut self) {
            for sink in self.sinks.drain(..) {
                sink.stop();
            }
        }

        fn set_volume(&mut self, volume: f32) {
            self.volume = volume.clamp(0.0, 1.0);

            for sink in &self.sinks {
                sink.set_volume(self.volume);
            }
        }

        fn play_pcm(&mut self, clip: Arc<WavClip>, gain: f32) -> Result<(), String> {
            // The movie audio is a stream of ~66 ms chunks: one sink, appended in order,
            // so consecutive frames do not click and do not fight the effect limit.
            if self.movie_sink.as_ref().is_none_or(Sink::empty) {
                let sink = Sink::try_new(&self.handle).map_err(|error| error.to_string())?;

                self.movie_sink = Some(sink);
            }

            let sink = self.movie_sink.as_mut().expect("movie sink");

            sink.set_volume(gain.clamp(0.0, 1.0));
            sink.append(SamplesBuffer::new(
                clip.channels,
                clip.sample_rate,
                clip.samples.clone(),
            ));

            Ok(())
        }

        fn stop_pcm(&mut self) {
            if let Some(sink) = self.movie_sink.take() {
                sink.stop();
            }
        }

        fn play_music(&mut self, clip: Arc<WavClip>, gain: f32) -> Result<(), String> {
            self.stop_music();

            let sink = Sink::try_new(&self.handle).map_err(|error| error.to_string())?;

            sink.append(SamplesBuffer::new(
                clip.channels,
                clip.sample_rate,
                clip.samples.clone(),
            ));
            sink.set_volume(self.music_volume * gain.clamp(0.0, 1.0));
            self.music_sink = Some(sink);

            Ok(())
        }

        fn stop_music(&mut self) {
            if let Some(sink) = self.music_sink.take() {
                sink.stop();
            }
        }

        fn set_music_volume(&mut self, volume: f32) {
            self.music_volume = volume.clamp(0.0, 1.0);

            if let Some(sink) = &self.music_sink {
                sink.set_volume(self.music_volume);
            }
        }

        fn music_playing(&self) -> bool {
            self.music_sink
                .as_ref()
                .is_some_and(|sink| !sink.empty())
        }
    }
}

#[cfg(feature = "sound")]
pub use rodio_backend::RodioBackend;

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;
