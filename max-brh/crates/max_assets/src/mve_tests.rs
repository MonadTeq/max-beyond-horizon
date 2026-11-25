use super::*;

fn fnv1a(data: &[u8]) -> u32 {
    let mut hash = 2_166_136_261u32;

    for &byte in data {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }

    hash
}

fn real_mve_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAXINT.MVE")
}

/// One opcode: `u16 size`, `u8 opcode`, `u8 version`, body.
fn opcode(opcode: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());

    out.extend_from_slice(&(body.len() as u16).to_le_bytes());
    out.push(opcode);
    out.push(0);
    out.extend_from_slice(body);

    out
}

/// One container record: `u16 size`, `u16 type`, payload.
fn chunk(kind: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());

    out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(payload);

    out
}

/// A synthetic container: the real 26-byte header + the given records + END.
fn container(records: &[Vec<u8>]) -> Vec<u8> {
    let mut file = Vec::new();

    file.extend_from_slice(MVE_SIGNATURE);
    file.push(0);
    file.extend_from_slice(&0x001Au16.to_le_bytes());
    file.extend_from_slice(&0x0100u16.to_le_bytes());
    file.extend_from_slice(&expected_checksum(0x0100).to_le_bytes());
    assert_eq!(file.len(), FILE_HEADER_SIZE);

    for record in records {
        file.extend_from_slice(record);
    }

    file.extend_from_slice(&0u16.to_le_bytes());
    file.extend_from_slice(&CHUNK_END.to_le_bytes());

    file
}

/// `INIT_VIDEO_BUFFERS`: width/height in 8×8 blocks.
fn init_video(width_blocks: u16, height_blocks: u16) -> Vec<u8> {
    let mut body = Vec::new();

    body.extend_from_slice(&width_blocks.to_le_bytes());
    body.extend_from_slice(&height_blocks.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());

    opcode(OP_INIT_VIDEO_BUFFERS, &body)
}

/// A video-data opcode with the 14-byte frame header.
fn video_frame(opcode_id: u8, width_blocks: u16, height_blocks: u16, flags: u16, data: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();

    for _ in 0..2 {
        body.extend_from_slice(&0u16.to_le_bytes());
    }
    body.extend_from_slice(&0u16.to_le_bytes()); // x
    body.extend_from_slice(&0u16.to_le_bytes()); // y
    body.extend_from_slice(&width_blocks.to_le_bytes());
    body.extend_from_slice(&height_blocks.to_le_bytes());
    body.extend_from_slice(&flags.to_le_bytes());
    body.extend_from_slice(data);

    opcode(opcode_id, &body)
}

#[test]
fn decodes_raw_video_data_06() {
    // 2×1 blocks = 16×8 pixels. `nfDecomp`: 64 raw indices per block.
    let mut data = vec![0x11u8; 64];
    data.extend_from_slice(&[0x22u8; 64]);

    let file = container(&[
        chunk(
            CHUNK_INIT_VIDEO,
            &[init_video(2, 1), video_frame(OP_VIDEO_DATA_06, 2, 1, 0, &data)]
                .concat(),
        ),
    ]);

    let mve = Mve::parse(file).expect("0x06 container");
    let info = mve.info();

    assert_eq!((info.width, info.height), (16, 8));
    assert_eq!(info.video_format, OP_VIDEO_DATA_06);
    assert_eq!(info.used_video_formats(), [OP_VIDEO_DATA_06]);
    assert_eq!(info.frame_count, 1);

    let mut decoder = mve.decoder();
    let frame = decoder.next_frame().expect("frame").expect("frame 0");

    assert_eq!(frame.len(), 16 * 8);
    assert!(frame[..8].iter().all(|&pixel| pixel == 0x11));
    assert!(frame[8..16].iter().all(|&pixel| pixel == 0x22));
    assert!(decoder.next_frame().expect("end").is_none());
}

#[test]
fn decodes_change_map_video_data_10() {
    // Frame 0 (`0x11`): both blocks solid, codes 0xE (no stream), colors from data.
    // Frame 1 (`0x10`, flags 0x01): only block 1 changed, block 0 keeps frame 0.
    let mut init = init_video(2, 1);
    let mut map = opcode(OP_SET_DECODING_MAP2, &[0xEE]);
    let mut change = opcode(OP_SET_DECODING_MAP1, &[0b10]);

    init.append(&mut map);
    init.append(&mut change);

    let frame0 = video_frame(OP_VIDEO_DATA_11, 2, 1, 0, &[0x11, 0x22]);
    let frame1 = video_frame(OP_VIDEO_DATA_10, 2, 1, 0x01, &[0x44]);

    let file = container(&[chunk(CHUNK_INIT_VIDEO, &[init].concat()), chunk(CHUNK_VIDEO, &[frame0].concat()), chunk(CHUNK_VIDEO, &[frame1].concat())]);

    let mve = Mve::parse(file).expect("0x10 container");
    let info = mve.info();

    assert_eq!(info.frame_count, 2);
    assert_eq!(info.video_format, OP_VIDEO_DATA_11);
    assert_eq!(
        info.used_video_formats(),
        [OP_VIDEO_DATA_11, OP_VIDEO_DATA_10]
    );

    let mut decoder = mve.decoder();
    let frame = decoder.next_frame().expect("frame 0").expect("frame 0");
    assert!(frame[..8].iter().all(|&pixel| pixel == 0x11));
    assert!(frame[8..16].iter().all(|&pixel| pixel == 0x22));

    let frame = decoder.next_frame().expect("frame 1").expect("frame 1");
    // Block 0: the change bit is clear — the previous frame is kept.
    assert!(frame[..8].iter().all(|&pixel| pixel == 0x11));
    // Block 1: the change bit is set — decoded from the block code map.
    assert!(frame[8..16].iter().all(|&pixel| pixel == 0x44));
}

#[test]
fn video_data_10_without_change_map_is_an_error() {
    let mut init = init_video(2, 1);
    let mut map = opcode(OP_SET_DECODING_MAP2, &[0xEE]);

    init.append(&mut map);

    let file = container(&[chunk(
        CHUNK_INIT_VIDEO,
        &[init, video_frame(OP_VIDEO_DATA_10, 2, 1, 0, &[0x11])].concat(),
    )]);

    assert!(matches!(
        Mve::parse(file),
        Err(MveError::MissingChangeMap { frame: 0 })
    ));
}

#[test]
fn video_data_10_with_a_short_change_map_is_an_error() {
    let mut init = init_video(2, 1);
    let mut map = opcode(OP_SET_DECODING_MAP2, &[0xEE]);
    let mut change = opcode(OP_SET_DECODING_MAP1, &[]);

    init.append(&mut map);
    init.append(&mut change);

    let file = container(&[chunk(
        CHUNK_INIT_VIDEO,
        &[init, video_frame(OP_VIDEO_DATA_10, 2, 1, 0x01, &[0x11])].concat(),
    )]);
    let mve = Mve::parse(file).expect("container");

    assert!(matches!(
        mve.decoder().next_frame(),
        Err(MveError::BadChangeMap { frame: 0, .. })
    ));
}

#[test]
fn truncated_raw_video_is_an_error_without_a_panic() {
    // A 2×1-block frame needs 128 bytes of raw indices; give it 64.
    let file = container(&[chunk(
        CHUNK_INIT_VIDEO,
        &[
            init_video(2, 1),
            video_frame(OP_VIDEO_DATA_06, 2, 1, 0, &[0x33u8; 64]),
        ]
        .concat(),
    )]);
    let mve = Mve::parse(file).expect("container");

    assert!(matches!(
        mve.decoder().next_frame(),
        Err(MveError::TruncatedVideo { frame: 0 })
    ));
}

/// `ALLOC_AUDIO_BUFFERS` (version 1): unused u16, flags u16, rate u16, min size u32.
fn alloc_audio(flags: u16, sample_rate: u16) -> Vec<u8> {
    let mut body = Vec::new();

    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&flags.to_le_bytes());
    body.extend_from_slice(&sample_rate.to_le_bytes());
    body.extend_from_slice(&88200u32.to_le_bytes());

    opcode(OP_ALLOC_AUDIO_BUFFERS, &body)
}

fn audio_frame(mask: u16, decoded_len: u16, data: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();

    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&mask.to_le_bytes());
    body.extend_from_slice(&decoded_len.to_le_bytes());
    body.extend_from_slice(data);

    opcode(OP_AUDIO_FRAME, &body)
}

fn silence_frame(mask: u16, decoded_len: u16) -> Vec<u8> {
    let mut body = Vec::new();

    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&mask.to_le_bytes());
    body.extend_from_slice(&decoded_len.to_le_bytes());

    opcode(OP_SILENCE_FRAME, &body)
}

#[test]
fn decodes_compressed_stereo_audio_and_the_track_mask() {
    // flags: stereo | 16 bit | delta compression.
    let flags = 0x07;
    // 6 samples (3 stereo frames): initial L=-2, R=1, then deltas 1,2,3,4.
    let mut data = Vec::new();
    data.extend_from_slice(&(-2i16).to_le_bytes());
    data.extend_from_slice(&1i16.to_le_bytes());
    data.extend_from_slice(&[1u8, 2, 3, 4]);

    let audio = chunk(
        CHUNK_AUDIO_ONLY,
        &[
            alloc_audio(flags, 22_050),
            audio_frame(1, 12, &data),
            // Another track of the same movie: `mask` bit 1, ignored by track 0.
            audio_frame(2, 4, &[7, 0, 9, 0]),
            silence_frame(1, 4),
        ]
        .concat(),
    );
    let video = chunk(
        CHUNK_VIDEO,
        &[init_video(1, 1), video_frame(OP_VIDEO_DATA_06, 1, 1, 0, &[0u8; 64])].concat(),
    );
    let file = container(&[audio, video]);
    let mve = Mve::parse(file).expect("audio container");
    let info = mve.info();

    assert_eq!(
        info.audio,
        Some(MveAudioFormat {
            channels: 2,
            sample_rate: 22_050,
            bits: 16,
            compressed: true,
        })
    );

    let mut decoder = mve.decoder();
    assert!(decoder.next_frame().expect("frame").is_some());

    let chunks = decoder.take_audio();
    assert_eq!(chunks.len(), 2, "track 0: audio + silence");
    assert_eq!(chunks[0].samples, vec![-2, 1, -1, 3, 2, 7]);
    assert_eq!(chunks[0].channels, 2);
    assert_eq!(chunks[0].frames(), 3);
    assert!((chunks[0].seconds() - 3.0 / 22_050.0).abs() < 1e-9);
    assert!(!chunks[0].silent);
    assert_eq!(chunks[1].samples, vec![0, 0]);
    assert!(chunks[1].silent);
    assert!(decoder.take_audio().is_empty());

    // Track 1 decodes the second stream and skips the first one.
    let mut other = mve.decoder_with_track(1);
    assert!(other.next_frame().expect("frame").is_some());

    let chunks = other.take_audio();
    assert_eq!(chunks.len(), 1);
    // The same compressed stereo format: two absolute samples, no deltas.
    assert_eq!(chunks[0].samples, vec![7, 9]);
}

#[test]
fn audio_frame_before_the_format_is_skipped_without_a_panic() {
    let audio = chunk(CHUNK_AUDIO_ONLY, &audio_frame(1, 4, &[1, 2, 3, 4]));
    let video = chunk(
        CHUNK_VIDEO,
        &[init_video(1, 1), video_frame(OP_VIDEO_DATA_06, 1, 1, 0, &[0u8; 64])].concat(),
    );
    let mve = Mve::parse(container(&[audio, video])).expect("container");
    let mut decoder = mve.decoder();

    assert!(decoder.next_frame().expect("frame").is_some());
    assert!(decoder.take_audio().is_empty());
    assert_eq!(decoder.audio_events_without_format(), 1);
}

#[test]
fn real_maxint_audio_track_matches_the_oracle_decoder() {
    let path = real_mve_path();

    if !path.is_file() {
        return;
    }

    let mve = Mve::open(&path).expect("MAXINT.MVE");
    let info = mve.info();
    let format = info.audio.expect("ALLOC_AUDIO_BUFFERS: the intro has a track");

    assert_eq!(format.channels, 2);
    assert_eq!(format.sample_rate, 22_050);
    assert_eq!(format.bits, 16);
    assert!(format.compressed);

    let mut decoder = mve.decoder_with_track(0);
    assert!(decoder.next_frame().expect("frame 0").is_some());

    // The container starts with 14 audio-only records (the lead-in of the first
    // scene) and then one audio record per video frame: the oracle feeds all of
    // them before the first `SHOW_VIDEO_FRAME`, and so does the decoder.
    let chunks = decoder.take_audio();
    assert_eq!(chunks.len(), 15, "lead-in + the chunk of frame 0");

    let chunk = &chunks[0];
    // The first frame: `length` = 5876 bytes → 2938 PCM16 samples → 1469 stereo frames.
    assert_eq!(chunk.samples.len(), 2938);
    assert_eq!(chunk.frames(), 1469);
    assert!((chunk.seconds() - 1469.0 / 22_050.0).abs() < 1e-6);
    // `sndDecompS16`: initial samples from the stream, then `snd_8to16` deltas.
    assert_eq!(&chunk.samples[..4], &[-2, 1, 10, 4]);

    // The whole track: one audio chunk per video frame, drained frame by frame.
    let mut decoder = mve.decoder();
    let mut decoded = 0usize;
    let mut chunks_total = 0usize;
    let mut total_frames = 0usize;

    while let Some(_frame) = decoder.next_frame().expect("frame") {
        let chunks = decoder.take_audio();

        assert!(!chunks.is_empty(), "frame {decoded} has no audio chunk");
        chunks_total += chunks.len();

        for chunk in chunks {
            total_frames += chunk.frames();
        }

        decoded += 1;

        if decoded == 50 {
            break;
        }
    }

    assert_eq!(decoded, 50);
    // Lead-in (14 chunks) + one per frame: 64 chunks of ~1469 stereo frames.
    assert_eq!(chunks_total, 64);
    assert!(
        (63 * 1469..=65 * 1490).contains(&total_frames),
            "audio frames {total_frames}"
    );
    assert_eq!(decoder.dropped_audio_chunks(), 0);
}

#[test]
fn rejects_bad_header() {
    assert!(matches!(
        Mve::parse(vec![0u8; 4]),
        Err(MveError::TooShort { .. })
    ));
    assert!(matches!(
        Mve::parse(vec![0u8; FILE_HEADER_SIZE]),
        Err(MveError::BadSignature)
    ));

    let mut bytes = vec![0u8; FILE_HEADER_SIZE];
    bytes[..MVE_SIGNATURE.len()].copy_from_slice(MVE_SIGNATURE);
    bytes[20..22].copy_from_slice(&0x001Au16.to_le_bytes());
    bytes[22..24].copy_from_slice(&0x0100u16.to_le_bytes());
    bytes[24..26].copy_from_slice(&0u16.to_le_bytes());

    assert!(matches!(Mve::parse(bytes), Err(MveError::BadHeader)));
}

#[test]
fn real_maxint_metadata_and_reference_frames() {
    let path = real_mve_path();

    if !path.is_file() {
        return;
    }

    let mve = Mve::open(&path).expect("MAXINT.MVE");
    let info = mve.info();

    assert_eq!(info.width, 432);
    assert_eq!(info.height, 320);
    assert_eq!(info.frame_count, 4391);
    assert_eq!(info.frame_delay_us, 66_664);
    assert_eq!(info.video_format, OP_VIDEO_DATA_11);
    assert!((info.frames_per_second() - 15.0).abs() < 0.01);

    let mut decoder = mve.decoder();
    let mut hashes = Vec::new();

    for index in 0..=100 {
        let frame = decoder
            .next_frame()
            .unwrap_or_else(|error| panic!("кадр {index}: {error}"))
            .unwrap_or_else(|| panic!("кадр {index} отсутствует"));

        assert_eq!(frame.len(), 432 * 320);

        if index == 0 {
            // The first frame is a dark blue background (index 4), not black.
            assert!(frame.iter().any(|&pixel| pixel != 0));
            assert!(frame.iter().all(|&pixel| pixel == 4));
        }

        if matches!(index, 0 | 50 | 100) {
            hashes.push((index, fnv1a(frame)));
        }
    }

    assert_eq!(
        hashes,
        vec![(0, 0x74CD_BDC5), (50, 0x22EB_1E36), (100, 0x158D_8E0D)],
            "кадры 0/50/100 должны совпасть с эталоном nfPkDecomp"
    );

    // The first palette (`SET_PALETTE` in the init record): index 4 — (4,0,4);
    // 255 stays 50/50/50 (`movie_init_palette`). Palettes change over
    // the course of the movie (13 `SET_PALETTE` in different places), so here
    // the palette of the first frames is what is checked.
    assert_eq!(decoder.palette()[4], [4, 0, 4]);
    assert_eq!(decoder.palette()[255], [50, 50, 50]);
    assert_eq!(decoder.frame_index(), 101);
}

#[test]
fn real_maxint_reaches_the_end() {
    let path = real_mve_path();

    if !path.is_file() {
        return;
    }

    // A full pass over 4391 frames — a quick check of indexing/finale
    // (without checking the pixels of each frame): the decoder does not crash and yields
    // exactly `frame_count` frames.
    let mve = Mve::open(&path).unwrap();
    let mut decoder = mve.decoder();
    let mut count = 0usize;

    while decoder.next_frame().unwrap().is_some() {
        count += 1;
    }

    assert_eq!(count, mve.info().frame_count);
    assert!(decoder.is_finished());
    // By the end of the movie all 13 `SET_PALETTE` are applied: index 4 — (0,4,44).
    assert_eq!(decoder.palette()[4], [0, 4, 44]);
}
