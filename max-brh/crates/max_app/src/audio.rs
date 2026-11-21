//! Звук приложения (M6.2): WAV → PCM-буфер → воспроизведение.
//!
//! Задача модуля — не «звуковой движок», а минимальный неблокирующий слой над
//! уже разобранными звуками:
//!
//! * [`max_assets::wav`] декодирует `*.SPW` (PCM16) в [`WavClip`] — это
//!   работает **всегда**, без внешних зависимостей;
//! * [`AudioPlayer`] кэширует клипы, разрешает имена через ресурсы-ссылки
//!   `MAX.RES` (`TANK1` → `"tank1.spw"`, `max-original-dasm/formats/spw.md` §1)
//!   и отдаёт их backend'у;
//! * [`AudioBackend`] — точка подключения вывода. Всегда есть тихий
//!   [`NoopBackend`]; за feature `sound` — [`RodioBackend`] (rodio/cpal).
//!
//! # Feature `sound`
//!
//! Базовые сборки (`cargo check`/`cargo test`/docker) не тянут ALSA/CPAL:
//! rodio — optional-зависимость, а `AudioPlayer::new()` создаёт no-op backend.
//! Только `AudioPlayer::with_default_backend()` в сборке с `--features sound`
//! пытается открыть аудиоустройство. Если устройства нет (headless/CI), игрок
//! остаётся на no-op и пишет причину в [`AudioPlayer::errors`] — падения нет.
//!
//! Воспроизведение не блокирует: rodio проигрывает сэмплы в своём потоке,
//! а вызов [`AudioPlayer::play`] только ставит клип в очередь.
//!
//! # Имена
//!
//! [`AudioPlayer::play`] принимает либо тег ресурса (`"KBUY0"`, `"TANK1"`),
//! либо имя файла (`"kbuy0.spw"`). Поиск файла регистронезависимый: на Linux
//! ссылка из `MAX.RES` — строчная (`tank1.spw`), а файл в каталоге игры —
//! заглавный (`TANK1.SPW`), поэтому каталог индексируется по нижнему регистру.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use max_assets::{parse_file_ref, ResArchive, WavClip};

/// Тег ресурса клика меню (`KBUY0` → `kbuy0.spw`).
///
/// `spw.md` §8.3: `KBUY0`/`KCARG0` — звук нажатия кнопки; используется в
/// `ui.rs` на кликах меню.
pub const MENU_CLICK: &str = "KBUY0";

/// Сколько роликов одновременно держит rodio (защита от «залипания» кликов).
#[cfg(feature = "sound")]
const MAX_CONCURRENT_SINKS: usize = 16;
/// Предел диагностических сообщений (ошибки не должны расти бесконечно).
const MAX_ERRORS: usize = 16;

/// Как проигрывать клип.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    /// Один раз.
    Once,
    /// Зацикленно (фон/двигатель).
    Loop,
}

/// Вывод звука. Реализации: [`NoopBackend`] и (за feature `sound`)
/// [`RodioBackend`].
///
/// Трейт не требует `Send`: cpal-поток (`OutputStream`) не является `Send`, а
/// плеер живёт в UI-потоке. Клипы передаются как `Arc<WavClip>` и клонируются
/// только в момент постановки в очередь.
pub trait AudioBackend: fmt::Debug {
    /// Имя backend'а для логов/отчётов (`"noop"`, `"rodio/cpal"`).
    fn name(&self) -> &'static str;

    /// Ставит клип в очередь. Не блокирует.
    fn play(&mut self, clip: Arc<WavClip>, mode: PlayMode) -> Result<(), String> {
        self.play_scaled(clip, mode, 1.0)
    }

    /// Ставит клип в очередь с дополнительным усилением `gain` (0..1+).
    ///
    /// Нужно для звуков юнитов: `SC_UNITS` задаёт громкость на эффект
    /// (`SoundManager::PlaySfx`: `volume / 100`), а общая громкость `FX`
    /// остаётся в [`AudioPlayer::set_volume`]. Реализация по умолчанию
    /// делегирует в [`AudioBackend::play`] (усиление игнорируется).
    fn play_scaled(
        &mut self,
        clip: Arc<WavClip>,
        mode: PlayMode,
        _gain: f32,
    ) -> Result<(), String> {
        self.play(clip, mode)
    }

    /// Останавливает всё, что играет.
    fn stop_all(&mut self) {}

    /// Громкость 0.0..=1.0 (реализация может игнорировать).
    fn set_volume(&mut self, _volume: f32) {}
}

/// Тихий backend: принимает клипы и ничего не делает.
///
/// Это поведение по умолчанию для сборок без feature `sound` и для тестов:
/// хуки UI остаются на месте, но ALSA/CPAL не нужны.
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

/// Проигрыватель: кэш клипов, ссылки `MAX.RES` и backend.
pub struct AudioPlayer {
    backend: Box<dyn AudioBackend>,
    sound_dir: Option<PathBuf>,
    /// имя файла в нижнем регистре → реальный путь (регистр ФС не важен).
    files: HashMap<String, PathBuf>,
    /// тег ресурса в верхнем регистре → имя `.spw` в нижнем регистре.
    references: BTreeMap<String, String>,
    /// имя файла в нижнем регистре → разобранный клип.
    cache: HashMap<String, Arc<WavClip>>,
    errors: Vec<String>,
    last_error: Option<String>,
    enabled: bool,
    plays: u64,
    volume: f32,
}

impl AudioPlayer {
    /// Плеер с тихим backend'ом и без данных.
    pub fn new() -> Self {
        Self::with_backend(Box::new(NoopBackend))
    }

    /// Плеер с произвольным backend'ом (тесты, другой звуковой движок).
    pub fn with_backend(backend: Box<dyn AudioBackend>) -> Self {
        Self {
            backend,
            sound_dir: None,
            files: HashMap::new(),
            references: BTreeMap::new(),
            cache: HashMap::new(),
            errors: Vec::new(),
            last_error: None,
            enabled: true,
            plays: 0,
            volume: 1.0,
        }
    }

    /// Плеер для реального запуска: rodio за feature `sound`, иначе no-op.
    ///
    /// Если аудиоустройство не открылось, возвращается no-op плеер с причиной
    /// в [`Self::errors`] — клиент продолжает работать без звука.
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

    /// Имя активного backend'а.
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// Есть ли реальный вывод (не [`NoopBackend`]).
    pub fn is_audible(&self) -> bool {
        self.backend.name() != "noop"
    }

    /// Включён ли звук (хуки UI всё равно вызываются).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Включает/выключает звук; при выключении останавливает очередь.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;

        if !enabled {
            self.backend.stop_all();
        }
    }

    /// Громкость 0.0..=1.0.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.backend.set_volume(self.volume);
    }

    /// Текущая громкость.
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Каталог звуков (рядом с `MAX.RES`).
    pub fn sound_dir(&self) -> Option<&Path> {
        self.sound_dir.as_deref()
    }

    /// Индексирует каталог звуков (`*.SPW`/`*.WAV`, регистр не важен).
    ///
    /// Возвращает `true`, если найден хотя бы один файл. Отсутствие каталога —
    /// не ошибка: клиент просто останется без звука.
    pub fn set_sound_dir(&mut self, dir: impl Into<PathBuf>) -> bool {
        let dir = dir.into();
        let mut files = HashMap::new();

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_sound = path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("spw") || extension.eq_ignore_ascii_case("wav")
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

    /// Собирает ссылки `тег → файл` из `MAX.RES` (только `*.spw`).
    ///
    /// Тело ресурса-ссылки — ASCIIZ-имя файла (`TANK1` → `"tank1.spw"`); обычные
    /// ресурсы (спрайты/карты) отбрасываются [`parse_file_ref`].
    pub fn attach_res(&mut self, res: &ResArchive) -> usize {
        let mut added = 0;

        for entry in res.list() {
            // Ссылки крошечные (10–16 байт); 256 — с запасом, чтобы не читать
            // тела спрайтов/карт.
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

            if !file_name.ends_with(".spw") {
                continue;
            }

            self.references
                .insert(entry.name.to_ascii_uppercase(), file_name);
            added += 1;
        }

        added
    }

    /// Привязывает `MAX.RES` и каталог звуков одной операцией.
    pub fn load_from(&mut self, res: &ResArchive, sound_dir: impl Into<PathBuf>) -> &mut Self {
        self.attach_res(res);
        self.set_sound_dir(sound_dir);
        self
    }

    /// Сколько тегов-ссылок собрано из `MAX.RES`.
    pub fn reference_count(&self) -> usize {
        self.references.len()
    }

    /// Сколько звуковых файлов проиндексировано.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Сколько клипов разобрано и закэшировано.
    pub fn cached_count(&self) -> usize {
        self.cache.len()
    }

    /// Сколько клипов успешно отправлено backend'у.
    pub fn plays(&self) -> u64 {
        self.plays
    }

    /// Диагностика (битые WAV, нет файла, нет устройства).
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Последняя ошибка.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Кладёт клип в кэш заранее (встроенные звуки, тесты).
    pub fn preload(&mut self, name: impl AsRef<str>, clip: WavClip) {
        let key = Self::file_key(name.as_ref());
        self.cache.insert(key, Arc::new(clip));
    }

    /// Есть ли клип для имени (без чтения файла).
    pub fn has_clip(&self, name: &str) -> bool {
        self.resolve(name).is_some()
    }

    /// Проигрывает звук по тегу/имени; `true` — клип поставлен в очередь.
    ///
    /// Не блокирует и не возвращает ошибку наружу: причина попадает в
    /// [`Self::errors`], а UI продолжает работать (звук — не блокер).
    pub fn play(&mut self, name: &str, mode: PlayMode) -> bool {
        self.play_gain(name, mode, 1.0)
    }

    /// [`AudioPlayer::play`] с усилением (звуки юнитов: `volume / 100`).
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

    /// Один раз.
    pub fn play_once(&mut self, name: &str) -> bool {
        self.play(name, PlayMode::Once)
    }

    /// Зацикленно.
    pub fn play_loop(&mut self, name: &str) -> bool {
        self.play(name, PlayMode::Loop)
    }

    /// Останавливает всё, что играет.
    pub fn stop_all(&mut self) {
        self.backend.stop_all();
    }

    /// `тег/имя` → имя файла в нижнем регистре, если он есть в кэше/каталоге.
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

    /// Имя файла в нижнем регистре: короткое имя получает расширение `.spw`.
    fn file_key(name: &str) -> String {
        let lower = name.to_ascii_lowercase();

        if lower.ends_with(".spw") || lower.ends_with(".wav") {
            lower
        } else {
            format!("{lower}.spw")
        }
    }

    /// Достаёт клип из кэша или читает файл (с кэшированием).
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
            .field("enabled", &self.enabled)
            .field("plays", &self.plays)
            .field("errors", &self.errors.len())
            .finish()
    }
}

/// Реализация вывода через rodio/cpal (только с feature `sound`).
#[cfg(feature = "sound")]
mod rodio_backend {
    use std::fmt;
    use std::sync::Arc;

    use max_assets::WavClip;
    use rodio::{buffer::SamplesBuffer, OutputStream, OutputStreamHandle, Sink, Source};

    use super::{AudioBackend, PlayMode, MAX_CONCURRENT_SINKS};

    /// Неблокирующий backend rodio: один `Sink` на клип.
    pub struct RodioBackend {
        /// Держит cpal-поток живым; при `Drop` воспроизведение прекращается.
        _stream: OutputStream,
        handle: OutputStreamHandle,
        sinks: Vec<Sink>,
        volume: f32,
    }

    impl fmt::Debug for RodioBackend {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("RodioBackend")
                .field("sinks", &self.sinks.len())
                .field("volume", &self.volume)
                .finish_non_exhaustive()
        }
    }

    impl RodioBackend {
        /// Открывает устройство вывода по умолчанию.
        pub fn new() -> Result<Self, String> {
            let (stream, handle) =
                OutputStream::try_default().map_err(|error| error.to_string())?;

            Ok(Self {
                _stream: stream,
                handle,
                sinks: Vec::new(),
                volume: 1.0,
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
            // Завершившиеся клипы не держим; очередь ограничена, чтобы частые
            // клики не копили потоки.
            self.sinks.retain(|sink| !sink.empty());

            if self.sinks.len() >= MAX_CONCURRENT_SINKS {
                self.sinks.remove(0).stop();
            }

            let sink = Sink::try_new(&self.handle).map_err(|error| error.to_string())?;
            // rodio принимает `Vec`; клип в `Arc`, поэтому клонируем PCM-буфер
            // (клипы M.A.X. короткие — единицы–сотни КБ).
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
    }
}

#[cfg(feature = "sound")]
pub use rodio_backend::RodioBackend;

#[cfg(test)]
mod tests {
    use super::*;
    use max_assets::ResArchive;
    use std::sync::Mutex;

    /// Backend, запоминающий вызовы (без аудиоустройства).
    #[derive(Debug)]
    struct RecordingBackend {
        calls: Arc<Mutex<Vec<PlayMode>>>,
        gains: Arc<Mutex<Vec<f32>>>,
    }

    impl AudioBackend for RecordingBackend {
        fn name(&self) -> &'static str {
            "recording"
        }

        fn play(&mut self, _clip: Arc<WavClip>, mode: PlayMode) -> Result<(), String> {
            self.calls.lock().expect("lock").push(mode);

            Ok(())
        }

        fn play_scaled(
            &mut self,
            _clip: Arc<WavClip>,
            mode: PlayMode,
            gain: f32,
        ) -> Result<(), String> {
            self.calls.lock().expect("lock").push(mode);
            self.gains.lock().expect("lock").push(gain);

            Ok(())
        }
    }

    fn recording_player() -> (AudioPlayer, Arc<Mutex<Vec<PlayMode>>>, Arc<Mutex<Vec<f32>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let gains = Arc::new(Mutex::new(Vec::new()));
        let player = AudioPlayer::with_backend(Box::new(RecordingBackend {
            calls: calls.clone(),
            gains: gains.clone(),
        }));

        (player, calls, gains)
    }

    fn clip() -> WavClip {
        WavClip {
            sample_rate: 22050,
            channels: 1,
            samples: vec![1, 2, 3],
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "max_app_audio_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");

        dir
    }

    /// Минимальный WAV PCM16 (моно).
    fn wav_bytes(samples: &[i16]) -> Vec<u8> {
        let data_len = samples.len() * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&22050u32.to_le_bytes());
        out.extend_from_slice(&44100u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    /// Минимальный RES-архив с одной ссылкой (`tag` → `file`).
    fn res_with_ref(tag: &str, file: &str) -> ResArchive {
        let mut tag_field = [0u8; 8];
        tag_field[..tag.len()].copy_from_slice(tag.as_bytes());
        let mut payload = file.as_bytes().to_vec();
        payload.push(0);

        let table_offset = 12 + payload.len() as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RES0");
        bytes.extend_from_slice(&table_offset.to_le_bytes());
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&tag_field);
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());

        ResArchive::parse(&bytes).expect("RES-архив")
    }

    #[test]
    fn plays_preloaded_clip_once_and_looping() {
        let (mut player, calls, gains) = recording_player();
        player.preload("KBUY0", clip());

        assert!(player.has_clip("KBUY0"));
        assert!(player.play_once(MENU_CLICK));
        assert!(player.play_loop("kbuy0.spw"));
        assert_eq!(player.plays(), 2);
        assert_eq!(
            *calls.lock().expect("lock"),
            vec![PlayMode::Once, PlayMode::Loop]
        );
        // Обычные звуки играют с усилением 1.0 (`play` → `play_scaled`).
        assert_eq!(*gains.lock().expect("lock"), vec![1.0, 1.0]);
    }

    #[test]
    fn play_gain_forwards_the_unit_volume_to_the_backend() {
        let (mut player, _calls, gains) = recording_player();
        player.preload("TANK14", clip());

        // `SoundManager::PlaySfx`: `volume / 100` из таблицы юнита.
        assert!(player.play_gain("TANK14", PlayMode::Once, 0.6));
        assert!(player.play_gain("TANK14", PlayMode::Once, 0.8));
        assert_eq!(player.plays(), 2);
        assert_eq!(*gains.lock().expect("lock"), vec![0.6, 0.8]);
    }

    #[test]
    fn resolves_res_reference_to_case_insensitive_file() {
        let dir = temp_dir("ref");
        // Ссылка строчная, файл — заглавный, как в оригинальном каталоге.
        std::fs::write(dir.join("TANK1.SPW"), wav_bytes(&[1, 2, 3, 4])).expect("wav");
        let res = res_with_ref("TANK1", "tank1.spw");
        let (mut player, calls, _gains) = recording_player();

        player.load_from(&res, &dir);

        assert_eq!(player.reference_count(), 1);
        assert_eq!(player.file_count(), 1);
        assert!(player.play_once("TANK1"));
        assert_eq!(player.cached_count(), 1);
        assert_eq!(player.plays(), 1);
        assert_eq!(calls.lock().expect("lock").len(), 1);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_sound_is_reported_but_does_not_panic() {
        let mut player = AudioPlayer::new();

        assert!(!player.play_once("NOPE"));
        assert!(player.errors().iter().any(|error| error.contains("NOPE")));
        assert!(player.last_error().is_some());

        // Выключенный звук не трогает backend и не считается ошибкой.
        player.preload("KBUY0", clip());
        player.set_enabled(false);
        assert!(!player.play_once(MENU_CLICK));
        assert_eq!(player.plays(), 0);
        assert_eq!(player.backend_name(), "noop");
    }

    #[test]
    fn default_backend_without_sound_feature_is_silent() {
        let player = AudioPlayer::with_default_backend();

        #[cfg(not(feature = "sound"))]
        {
            assert_eq!(player.backend_name(), "noop");
            assert!(!player.is_audible());
        }

        // С feature sound backend может быть и noop (нет устройства в CI), и
        // rodio; оба варианта корректны, паники быть не должно.
        let _ = player.is_audible();
    }

    /// Реальные `MAX.RES` + каталог игры: ссылки и регистр файлов сходятся.
    #[test]
    fn real_game_data_resolves_menu_click_and_unit_sounds() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original");
        let max_res = dir.join("MAX.RES");

        if !max_res.is_file() {
            return;
        }

        let res = ResArchive::open(&max_res).expect("MAX.RES");
        let mut player = AudioPlayer::new();
        let refs = player.attach_res(&res);

        assert!(refs >= 600, "ожидались сотни .spw-ссылок, найдено {refs}");
        assert!(player.set_sound_dir(&dir), "в каталоге игры есть *.SPW");
        assert!(player.has_clip(MENU_CLICK), "KBUY0 → kbuy0.spw");
        assert!(player.has_clip("TANK1"), "TANK1 → tank1.spw");
        assert!(!player.has_clip("NO_SUCH_SOUND"));
    }
}
