use super::*;
use max_assets::ResArchive;
use std::sync::Mutex;

/// Backend that remembers calls (without an audio device).
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

/// Minimal WAV PCM16 (mono).
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

/// Minimal RES archive with one reference (`tag` → `file`).
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
    // Regular sounds play with gain 1.0 (`play` → `play_scaled`).
    assert_eq!(*gains.lock().expect("lock"), vec![1.0, 1.0]);
}

#[test]
fn play_gain_forwards_the_unit_volume_to_the_backend() {
    let (mut player, _calls, gains) = recording_player();
    player.preload("TANK14", clip());

    // `SoundManager::PlaySfx`: `volume / 100` from the unit table.
    assert!(player.play_gain("TANK14", PlayMode::Once, 0.6));
    assert!(player.play_gain("TANK14", PlayMode::Once, 0.8));
    assert_eq!(player.plays(), 2);
    assert_eq!(*gains.lock().expect("lock"), vec![0.6, 0.8]);
}

#[test]
fn resolves_res_reference_to_case_insensitive_file() {
    let dir = temp_dir("ref");
    // The reference is lowercase, the file is uppercase, as in the original directory.
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

    // Disabled sound does not touch the backend and is not considered an error.
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

    // With the sound feature the backend can be either noop (no device in CI) or
    // rodio; both variants are valid, no panic must occur.
    let _ = player.is_audible();
}

/// Real `MAX.RES` + game directory: references and file case agree.
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
