use super::*;

/// Minimal FLC from ready frames: `frames` frames of `chunks`.
fn build_flic(width: u16, height: u16, frames: &[&[(u16, Vec<u8>)]], speed: u32) -> Vec<u8> {
    let mut out = vec![0u8; FLIC_HEADER_SIZE];
    let mut frame_offsets = Vec::new();

    for chunks in frames {
        frame_offsets.push(out.len() as u32);

        let mut body = Vec::new();

        for (kind, payload) in *chunks {
            body.extend_from_slice(
                &(payload.len() as u32 + FLIC_CHUNK_HEADER_SIZE as u32).to_le_bytes(),
            );
            body.extend_from_slice(&kind.to_le_bytes());
            body.extend_from_slice(payload);
        }

        let size = body.len() as u32 + FLIC_FRAME_HEADER_SIZE as u32;
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
        out.extend_from_slice(&(chunks.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&body);
    }

    let total = out.len() as u32;

    out[0..4].copy_from_slice(&total.to_le_bytes());
    out[4..6].copy_from_slice(&FLC_MAGIC.to_le_bytes());
    out[6..8].copy_from_slice(&(frames.len() as u16).to_le_bytes());
    out[8..10].copy_from_slice(&width.to_le_bytes());
    out[10..12].copy_from_slice(&height.to_le_bytes());
    out[12..14].copy_from_slice(&8u16.to_le_bytes());
    out[14..16].copy_from_slice(&3u16.to_le_bytes());
    out[16..20].copy_from_slice(&speed.to_le_bytes());
    out[80..84].copy_from_slice(&frame_offsets[0].to_le_bytes());
    out[84..88].copy_from_slice(&frame_offsets.last().copied().unwrap_or(0).to_le_bytes());

    out
}

/// `BYTE_RUN` frame: each row is a packet byte and literal pixels.
fn byte_run(width: usize, height: usize, value: u8) -> Vec<u8> {
    let mut body = Vec::new();

    for _ in 0..height {
        body.push(1u8);
        body.push(width as u8); // count, < 128 — repeat
        body.push(value);
    }

    body
}

#[test]
fn rejects_short_fli_and_bad_geometry() {
    assert!(matches!(
        Flic::parse(&[0u8; 4]),
        Err(FlicError::TooShort { .. })
    ));

    let mut fli = build_flic(4, 4, &[&[(15, byte_run(4, 4, 7))]], 71);
    fli[4..6].copy_from_slice(&FLI_MAGIC.to_le_bytes());
    assert!(matches!(
        Flic::parse(&fli),
        Err(FlicError::BadMagic { found: FLI_MAGIC })
    ));

    let mut zero = build_flic(0, 4, &[&[(15, Vec::new())]], 71);
    zero[6..8].copy_from_slice(&0u16.to_le_bytes());
    assert!(matches!(Flic::parse(&zero), Err(FlicError::NoFrames)));

    let zero = build_flic(0, 4, &[&[(15, byte_run(1, 4, 1))]], 71);
    assert!(matches!(
        Flic::parse(&zero),
        Err(FlicError::BadGeometry { .. })
    ));
}

#[test]
fn decodes_byte_run_and_literal_frames() {
    let literal: Vec<u8> = (0..16u8).collect();
    let bytes = build_flic(
        4,
        4,
        &[
            &[(15, byte_run(4, 4, 5))],
            &[(16, literal)],
            &[(15, byte_run(4, 4, 0))],
        ],
        71,
    );
    let flic = Flic::parse(&bytes).unwrap();

    assert_eq!((flic.width(), flic.height()), (4, 4));
    assert_eq!(flic.frame_count(), 3);
    assert_eq!(flic.speed_ms(), 71);
    assert_eq!(flic.declared_size(), bytes.len() as u32);
    assert!(flic.is_animated());

    assert_eq!(flic.decode_frame(0).unwrap().pixels, vec![5; 16]);
    assert_eq!(
        flic.decode_frame(1).unwrap().pixels,
        (0..16u8).collect::<Vec<_>>()
    );
    assert_eq!(flic.decode_frame(2).unwrap().pixels, vec![0; 16]);

    let frame = flic.decode_frame(1).unwrap();
    let palette = [[1u8, 2, 3, 4]; 256];
    let rgba = frame.rgba(&palette);
    assert_eq!(rgba.len(), 16 * 4);
    assert_eq!(&rgba[0..4], &[1, 2, 3, 4]);
}

#[test]
fn decodes_color_chunks_into_palette() {
    // Palette packet: skip 0, count 2, two colors (8-bit).
    let mut color = Vec::new();
    color.extend_from_slice(&1u16.to_le_bytes());
    color.extend_from_slice(&[0, 2, 255, 0, 0, 0, 255, 0]);
    // COLOR_64: 6-bit values without shift, skip 1 → index 1.
    let mut color64 = Vec::new();
    color64.extend_from_slice(&1u16.to_le_bytes());
    color64.extend_from_slice(&[1, 1, 63, 0, 0, 0, 63, 0]);

    let bytes = build_flic(
        2,
        2,
        &[
            &[(4, color), (15, byte_run(2, 2, 0))],
            &[(11, color64), (15, byte_run(2, 2, 1))],
        ],
        71,
    );
    let flic = Flic::parse(&bytes).unwrap();
    let first = flic.decode_frame(0).unwrap();
    let second = flic.decode_frame(1).unwrap();

    // COLOR_256 shifts by 2 bits — like C++ (`>> 2` into the 6-bit palette).
    assert_eq!(first.palette[0], [63, 0, 0]);
    assert_eq!(first.palette[1], [0, 63, 0]);
    // Second frame: COLOR_64 without shift overwrote index 1.
    assert_eq!(second.palette[0], [63, 0, 0]);
    assert_eq!(second.palette[1], [63, 0, 0]);
    assert_eq!(second.palette[2], [0, 0, 0]);
    assert_eq!(first.pixels, vec![0; 4]);
    assert_eq!(second.pixels, vec![1; 4]);
}

#[test]
fn decodes_delta_flc_skip_last_byte_and_runs() {
    // 4×4, frame 0 — all 9s, frame 1 — a delta:
    // 2 rows: the first row starts with a skip of 1 row, then a packet
    // "skip 1, repeat 1 word (0x0A0A)", in the second row —
    // an optional word with the last byte 0x0B.
    let mut delta = Vec::new();
    delta.extend_from_slice(&2u16.to_le_bytes()); // 2 rows
                                                  // optional word -1 = 0xFFFF → skip one row
    delta.extend_from_slice(&0xFFFFu16.to_le_bytes());
    delta.extend_from_slice(&1u16.to_le_bytes()); // 1 packet
    delta.push(1); // skip
    delta.push(0xFF); // -1 word
    delta.extend_from_slice(&0x0A0Au16.to_le_bytes());
    // second row: optional word 0x800B — last byte 0x0B
    delta.extend_from_slice(&0x800Bu16.to_le_bytes());
    delta.extend_from_slice(&0u16.to_le_bytes()); // 0 packets

    let bytes = build_flic(4, 4, &[&[(15, byte_run(4, 4, 9))], &[(7, delta)]], 71);
    let flic = Flic::parse(&bytes).unwrap();
    let first = flic.decode_frame(0).unwrap();
    let second = flic.decode_frame(1).unwrap();

    assert_eq!(&second.pixels[0..4], &[9, 9, 9, 9]);
    assert_eq!(&second.pixels[4..8], &[9, 0x0A, 0x0A, 9]);
    assert_eq!(&second.pixels[8..12], &[9, 9, 9, 0x0B]);
    assert_eq!(first.pixels, vec![9; 16]);
    // The frames are independent: re-decoding gives the same pixels.
    assert_eq!(flic.decode_frame(0).unwrap(), first);
}

#[test]
fn player_matches_random_access_decoding() {
    // Delta of frame 1: in the first row a packet "skip 0, repeat word 0x0202".
    let mut delta = Vec::new();
    delta.extend_from_slice(&1u16.to_le_bytes()); // 1 row
    delta.extend_from_slice(&1u16.to_le_bytes()); // 1 packet
    delta.push(0); // skip
    delta.push(0xFF); // -1 word
    delta.extend_from_slice(&0x0202u16.to_le_bytes());

    let bytes = build_flic(
        4,
        4,
        &[
            &[(15, byte_run(4, 4, 1))],
            &[(7, delta)],
            &[(13, Vec::new())],
        ],
        71,
    );
    let flic = Flic::parse(&bytes).unwrap();
    let mut player = FlicPlayer::new(&flic).unwrap();

    assert_eq!(player.frame_index(), 0);
    assert_eq!(player.frame(), flic.decode_frame(0).unwrap());
    assert_eq!(player.pixels(), vec![1; 16].as_slice());

    assert_eq!(player.advance().unwrap(), 1);
    assert_eq!(player.frame(), flic.decode_frame(1).unwrap());
    assert_eq!(&player.pixels()[0..4], &[2, 2, 1, 1]);

    assert_eq!(player.advance().unwrap(), 2);
    assert_eq!(player.frame(), flic.decode_frame(2).unwrap());
    assert_eq!(player.pixels(), vec![0; 16].as_slice());

    // Past the last frame — frame 1 again (like `animate == 2` in C++:
    // the delta is applied to the current buffer, not to frame 0).
    assert_eq!(player.advance().unwrap(), 1);
    let mut expected = vec![0u8; 16];
    expected[0] = 2;
    expected[1] = 2;
    assert_eq!(player.pixels(), expected.as_slice());
    // And that is not the same as decoding from scratch: that is how C++ works.
    assert_ne!(player.frame().pixels, flic.decode_frame(1).unwrap().pixels);

    // Static FLC: advance does not move the frame.
    let single = build_flic(2, 2, &[&[(15, byte_run(2, 2, 3))]], 71);
    let single = Flic::parse(&single).unwrap();
    let mut player = FlicPlayer::new(&single).unwrap();

    assert_eq!(player.advance().unwrap(), 0);
    assert_eq!(player.pixels(), vec![3; 4].as_slice());
}

#[test]
fn rejects_truncated_and_broken_chunks() {
    let good = build_flic(4, 4, &[&[(15, byte_run(4, 4, 1))]], 71);

    // Truncated file.
    assert!(matches!(
        Flic::parse(&good[..good.len() - 2]),
        Err(FlicError::Frame { .. })
    ));

    // A zero-size chunk: the frame parses but does not decode.
    let mut broken = good.clone();
    let chunk_offset = FLIC_HEADER_SIZE + FLIC_FRAME_HEADER_SIZE;
    broken[chunk_offset..chunk_offset + 4].copy_from_slice(&0u32.to_le_bytes());
    let flic = Flic::parse(&broken).unwrap();
    assert!(matches!(
        flic.decode_frame(0),
        Err(FlicError::Frame {
            reason: FrameError::BadChunk { .. },
            ..
        })
    ));

    // Unknown chunk.
    let mut broken = good.clone();
    broken[chunk_offset + 4..chunk_offset + 6].copy_from_slice(&77u16.to_le_bytes());
    let flic = Flic::parse(&broken).unwrap();
    assert!(matches!(
        flic.decode_frame(0),
        Err(FlicError::Frame {
            reason: FrameError::UnknownChunk { kind: 77, .. },
            ..
        })
    ));

    // Zero-length BYTE_RUN series — do not loop forever.
    let mut zero_run = Vec::new();
    zero_run.push(0u8);
    zero_run.extend_from_slice(&[0, 0, 0, 0, 0]);
    let bytes = build_flic(4, 1, &[&[(15, zero_run)]], 71);
    let flic = Flic::parse(&bytes).unwrap();
    assert!(matches!(
        flic.decode_frame(0),
        Err(FlicError::Format { .. })
    ));

    // Frame index out of range.
    let flic = Flic::parse(&good).unwrap();
    assert!(matches!(
        flic.decode_frame(5),
        Err(FlicError::FrameIndex {
            index: 5,
            frames: 1
        })
    ));
}

#[test]
fn palette_and_black_chunks_reset_canvas() {
    let bytes = build_flic(
        2,
        1,
        &[
            &[(15, byte_run(2, 1, 4))],
            &[(13, Vec::new())],
            &[(16, vec![1, 2])],
        ],
        71,
    );
    let flic = Flic::parse(&bytes).unwrap();

    assert_eq!(flic.decode_frame(1).unwrap().pixels, vec![0, 0]);
    assert_eq!(flic.decode_frame(2).unwrap().pixels, vec![1, 2]);
    // PTSAMP/unknown frames do not break `palette()`.
    assert_eq!(flic.palette().unwrap(), [[0; 3]; 256]);
}
