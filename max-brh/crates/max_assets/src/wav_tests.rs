use super::*;

/// Builds a PCM16 WAV from samples (mono/stereo).
fn build_wav(channels: u16, sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() * SAMPLE_BYTES;
    let block_align = channels * 2;
    let mut out = Vec::with_capacity(44 + data_len);

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());

    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }

    out
}

#[test]
fn parses_pcm16_mono_and_stereo() {
    let mono = WavClip::parse(&build_wav(1, 22050, &[1, -2, 3])).expect("mono");
    assert_eq!(mono.sample_rate, 22050);
    assert_eq!(mono.channels, 1);
    assert_eq!(mono.samples, vec![1, -2, 3]);
    assert_eq!(mono.frames(), 3);

    let stereo = WavClip::parse(&build_wav(2, 44100, &[10, 20, 30, 40])).expect("stereo");
    assert_eq!(stereo.channels, 2);
    assert_eq!(stereo.samples, vec![10, 20, 30, 40]);
    assert_eq!(stereo.frames(), 2);
}

#[test]
fn skips_unknown_odd_and_trailing_chunks() {
    let mut wav = build_wav(1, 22050, &[7, 8]);
    // Insert "junk" (3 bytes + padding) after WAVE and "smpl" at the end:
    // the walk must account for the odd size and not be picky about foreign chunks.
    let mut junk = Vec::new();
    junk.extend_from_slice(b"junk");
    junk.extend_from_slice(&3u32.to_le_bytes());
    junk.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0x00]);

    wav.splice(12..12, junk);
    // We do not recompute the RIFF size: the parser follows the chunks, not the header
    // (in real files the size is sometimes inaccurate).
    wav.extend_from_slice(b"smpl");
    wav.extend_from_slice(&60u32.to_le_bytes());
    wav.extend_from_slice(&[0u8; 60]);

    let clip = WavClip::parse(&wav).expect("чанки пропущены");
    assert_eq!(clip.samples, vec![7, 8]);
    assert_eq!(clip.sample_rate, 22050);
}

#[test]
fn truncates_data_to_whole_frames() {
    // Stereo with a "half" frame: 3 samples instead of 4.
    let mut wav = build_wav(2, 22050, &[1, 2, 3, 4]);
    // data starts at 44: 6 bytes = 3 samples, the last frame is incomplete.
    wav.truncate(wav.len() - 2);
    wav[40..44].copy_from_slice(&6u32.to_le_bytes());

    let clip = WavClip::parse(&wav).expect("хвост обрезан");
    assert_eq!(clip.channels, 2);
    assert_eq!(clip.samples, vec![1, 2]);
    assert_eq!(clip.frames(), 1);
}

#[test]
fn rejects_malformed_files() {
    assert!(matches!(
        WavClip::parse(b""),
        Err(WavError::TooShort { .. })
    ));
    assert!(matches!(
        WavClip::parse(b"NOPE\0\0\0\0WAVE"),
        Err(WavError::BadRiff { .. })
    ));
    assert!(matches!(
        WavClip::parse(b"RIFF\0\0\0\0NOPE"),
        Err(WavError::BadWave { .. })
    ));

    let wav = build_wav(1, 22050, &[1]);
    // No data.
    let mut no_data = wav.clone();
    no_data[36..40].copy_from_slice(b"junk");
    assert!(matches!(
        WavClip::parse(&no_data),
        Err(WavError::MissingData)
    ));

    // No fmt.
    let mut no_fmt = wav.clone();
    no_fmt[12..16].copy_from_slice(b"junk");
    assert!(matches!(WavClip::parse(&no_fmt), Err(WavError::MissingFmt)));

    // Truncated data chunk.
    let mut truncated = wav.clone();
    truncated.truncate(truncated.len() - 1);
    assert!(matches!(
        WavClip::parse(&truncated),
        Err(WavError::TruncatedChunk { .. })
    ));

    // 8 bits.
    let mut eight_bit = wav.clone();
    eight_bit[34..36].copy_from_slice(&8u16.to_le_bytes());
    assert!(matches!(
        WavClip::parse(&eight_bit),
        Err(WavError::UnsupportedBits { bits: 8 })
    ));

    // 3 channels and a wrong block_align.
    let mut three_channels = wav.clone();
    three_channels[22..24].copy_from_slice(&3u16.to_le_bytes());
    assert!(matches!(
        WavClip::parse(&three_channels),
        Err(WavError::UnsupportedChannels { channels: 3 })
    ));

    let mut bad_align = wav.clone();
    bad_align[32..34].copy_from_slice(&6u16.to_le_bytes());
    assert!(matches!(
        WavClip::parse(&bad_align),
        Err(WavError::BadBlockAlign { .. })
    ));

    // Non-PCM (IEEE float).
    let mut float = wav.clone();
    float[20..22].copy_from_slice(&3u16.to_le_bytes());
    assert!(matches!(
        WavClip::parse(&float),
        Err(WavError::UnsupportedFormat { format: 3 })
    ));

    // Zero sample rate.
    let mut zero_rate = wav;
    zero_rate[24..28].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        WavClip::parse(&zero_rate),
        Err(WavError::BadSampleRate { sample_rate: 0 })
    ));
}

/// A real game sound: the `max-original/KBUY0.SPW` fixture (2 604 bytes).
///
/// The path is baked in at compile time: the file lies in the repository, so the
/// test does not depend on the current directory and is not silently skipped.
#[test]
fn parses_real_kbuy0_spw() {
    const KBUY0: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
            "/../../../max-original/KBUY0.SPW"
    ));

    let clip = WavClip::parse(KBUY0).expect("KBUY0.SPW — валидный PCM16 WAV");

    assert_eq!(clip.sample_rate, 22050);
    assert_eq!(clip.channels, 1);
    assert_eq!(clip.samples.len(), 1280);
    assert_eq!(clip.frames(), 1280);
    assert!(!clip.is_empty());
    assert!((clip.duration().as_secs_f64() - 1280.0 / 22050.0).abs() < 1e-9);
}

/// All 272 `*.SPW` of the game directory are PCM16 22050 Hz mono (`spw.md` §8.5).
///
/// The test reads the directory rather than a single file: this catches any discrepancy
/// of the real data with the parser's assumptions. If the directory is absent (a build without
/// game data), the test fails.
#[test]
fn all_real_spw_files_are_pcm16_22050_mono() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original");
    let entries = fs::read_dir(&dir).expect("max-original directory with the SPW files");
    let mut checked = 0usize;

    for entry in entries.flatten() {
        let path = entry.path();
        let is_spw = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("spw"));
        if !is_spw {
            continue;
        }

        let clip =
            WavClip::open(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(clip.sample_rate, 22050, "{}", path.display());
        assert_eq!(clip.channels, 1, "{}", path.display());
        assert!(!clip.is_empty(), "{}", path.display());
        checked += 1;
    }

    assert!(checked >= 200, "ожидались сотни .SPW, найдено {checked}");
}

// ---------------------------------------------------------------------------
// Lazy `*.MSC` source (`WavSource`).
// ---------------------------------------------------------------------------

/// Writes `bytes` to a unique temporary file (the lazy source needs a path).
fn temp_wav(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "max_assets_wav_source_{}_{name}.wav",
        std::process::id()
    ));

    fs::write(&path, bytes).expect("temp wav");

    path
}

#[test]
fn lazy_source_opens_headers_and_decodes_like_parse() {
    let path = temp_wav("stereo", &build_wav(2, 44100, &[10, 20, 30, 40]));
    let source = WavSource::open(&path).expect("PCM16 headers");

    assert_eq!(source.sample_rate(), 44100);
    assert_eq!(source.channels(), 2);
    assert_eq!(source.frames(), 2);
    assert!(!source.is_empty());
    assert!((source.duration().as_secs_f64() - 2.0 / 44100.0).abs() < 1e-9);
    assert_eq!(source.path(), path.as_path());

    let clip = source.decode().expect("lazy decode");
    assert_eq!(clip.sample_rate, 44100);
    assert_eq!(clip.channels, 2);
    assert_eq!(clip.samples, vec![10, 20, 30, 40]);
    assert_eq!(clip.frames(), 2);

    let _ = fs::remove_file(&path);
}

#[test]
fn lazy_source_skips_unknown_odd_and_trailing_chunks() {
    let mut wav = build_wav(1, 22050, &[7, 8]);
    let mut junk = Vec::new();
    junk.extend_from_slice(b"junk");
    junk.extend_from_slice(&3u32.to_le_bytes());
    junk.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0x00]);

    wav.splice(12..12, junk);
    wav.extend_from_slice(b"smpl");
    wav.extend_from_slice(&60u32.to_le_bytes());
    wav.extend_from_slice(&[0u8; 60]);

    let path = temp_wav("chunks", &wav);
    let source = WavSource::open(&path).expect("foreign chunks are skipped");

    assert_eq!(source.frames(), 2);

    let clip = source.decode().expect("lazy decode");
    assert_eq!(clip.samples, vec![7, 8]);
    assert_eq!(clip.sample_rate, 22050);

    let _ = fs::remove_file(&path);
}

#[test]
fn lazy_source_truncates_data_to_whole_frames() {
    // Stereo with a "half" frame: 3 samples instead of 4.
    let mut wav = build_wav(2, 22050, &[1, 2, 3, 4]);
    wav.truncate(wav.len() - 2);
    wav[40..44].copy_from_slice(&6u32.to_le_bytes());

    let path = temp_wav("half_frame", &wav);
    let source = WavSource::open(&path).expect("half frame is not an error");

    assert_eq!(source.frames(), 1);

    let clip = source.decode().expect("lazy decode");
    assert_eq!(clip.channels, 2);
    assert_eq!(clip.samples, vec![1, 2]);

    let _ = fs::remove_file(&path);
}

#[test]
fn lazy_source_rejects_malformed_files_at_open() {
    // The whole chunk layout is validated by `open` (the audio body is not read).
    let truncated_path = {
        let mut wav = build_wav(1, 22050, &[1]);
        wav.truncate(wav.len() - 1);
        temp_wav("truncated", &wav)
    };
    assert!(matches!(
        WavSource::open(&truncated_path),
        Err(WavError::TruncatedChunk { .. })
    ));
    let _ = fs::remove_file(&truncated_path);

    let no_data_path = {
        let mut wav = build_wav(1, 22050, &[1]);
        wav[36..40].copy_from_slice(b"junk");
        temp_wav("no_data", &wav)
    };
    assert!(matches!(
        WavSource::open(&no_data_path),
        Err(WavError::MissingData)
    ));
    let _ = fs::remove_file(&no_data_path);

    let no_fmt_path = {
        let mut wav = build_wav(1, 22050, &[1]);
        wav[12..16].copy_from_slice(b"junk");
        temp_wav("no_fmt", &wav)
    };
    assert!(matches!(
        WavSource::open(&no_fmt_path),
        Err(WavError::MissingFmt)
    ));
    let _ = fs::remove_file(&no_fmt_path);

    let bad_riff_path = temp_wav("bad_riff", b"NOPE\0\0\0\0WAVE");
    assert!(matches!(
        WavSource::open(&bad_riff_path),
        Err(WavError::BadRiff { .. })
    ));
    let _ = fs::remove_file(&bad_riff_path);

    let empty_path = temp_wav("empty", b"");
    assert!(matches!(
        WavSource::open(&empty_path),
        Err(WavError::TooShort { .. })
    ));
    let _ = fs::remove_file(&empty_path);

    let missing = std::env::temp_dir().join("max_assets_wav_source_missing_12345.wav");
    assert!(matches!(
        WavSource::open(&missing),
        Err(WavError::Io { .. })
    ));
}

/// The real music track: the lazy path decodes exactly what the eager parser does.
#[test]
fn lazy_source_decodes_real_main_msc_like_the_eager_parser() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAIN_MSC.MSC");

    assert!(
        path.is_file(),
        "required test fixture is missing: {}",
        path.display()
    );

    let source = WavSource::open(&path).expect("MAIN_MSC.MSC headers");
    assert_eq!(source.sample_rate(), 22_050);
    assert_eq!(source.channels(), 2);
    assert!(source.frames() > 22_050, "трек длиннее секунды");

    let frames = source.frames();
    let clip = source.decode().expect("MAIN_MSC.MSC decodes lazily");
    assert_eq!(clip.sample_rate, 22_050);
    assert_eq!(clip.channels, 2);
    assert_eq!(clip.frames(), frames);
    assert_eq!(clip.samples.len(), frames * 2);
    assert!(clip.samples.iter().any(|&sample| sample != 0));

    let eager = WavClip::open(&path).expect("MAIN_MSC.MSC eager");
    assert_eq!(clip.samples, eager.samples);
}
