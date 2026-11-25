use super::*;

/// Builds a sprite body from frames of the form `(width, height, hotx, hoty, rows)`,
/// where `rows` are already encoded rows (bytes together with the terminator).
fn build_sprite(frames: &[(u16, u16, i16, i16, Vec<Vec<u8>>)]) -> Vec<u8> {
    let image_count = frames.len() as u16;
    let mut out = Vec::new();

    out.extend_from_slice(&image_count.to_le_bytes());
    // Frame offset stubs, filled in below.
    out.extend(std::iter::repeat_n(0u8, 4 * frames.len()));

    for (index, (width, height, hotx, hoty, rows)) in frames.iter().enumerate() {
        assert_eq!(rows.len(), *height as usize);

        let frame_offset = out.len() as u32;
        out[HEADER_SIZE + 4 * index..HEADER_SIZE + 4 * index + 4]
            .copy_from_slice(&frame_offset.to_le_bytes());

        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&hotx.to_le_bytes());
        out.extend_from_slice(&hoty.to_le_bytes());

        let row_table_start = out.len();
        out.extend(std::iter::repeat_n(0u8, 4 * rows.len()));

        for (y, row) in rows.iter().enumerate() {
            let row_offset = out.len() as u32;
            out[row_table_start + 4 * y..row_table_start + 4 * y + 4]
                .copy_from_slice(&row_offset.to_le_bytes());
            out.extend_from_slice(row);
        }
    }

    out
}

/// `{skip, count, pixels[]}`.
fn run(skip: u8, pixels: &[u8]) -> Vec<u8> {
    let mut out = vec![skip, pixels.len() as u8];

    out.extend_from_slice(pixels);
    out
}

#[test]
fn decodes_rows_with_transparent_tail_and_hot_spot() {
    // width = 6, row = {skip 1, count 2, [A, B]}, {skip 1, count 2, [C, D]} -> x = 6? no: 1+2+1+2 = 6.
    let mut row = run(1, &[0xAA, 0xBB]);

    row.extend(run(1, &[0xCC, 0xDD]));
    row.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(6, 1, 3, 4, vec![row])]);
    let sheet = decode_sprite(&bytes).unwrap();

    assert_eq!(sheet.image_count, 1);
    assert_eq!(sheet.len(), 1);

    let frame = sheet.frame(0).unwrap();

    assert_eq!(
        (frame.width, frame.height, frame.hotx, frame.hoty),
        (6, 1, 3, 4)
    );
    assert_eq!(
        frame.pixels,
        vec![None, Some(0xAA), Some(0xBB), None, Some(0xCC), Some(0xDD)]
    );
    assert_eq!(frame.opaque_pixels(), 4);
    assert_eq!(frame.rows[0].painted, 6);
    // height = 1, hot = (3, 4), point (10, 20) -> frame (7, 16)..(13, 17).
    assert_eq!(frame.bounds_for_anchor(10, 20), (7, 16, 13, 17));
}

#[test]
fn tail_shorter_than_width_is_transparent() {
    // A real edge case from TANK §3.1: the sum of the counters is less than width.
    let mut row = run(1, &[0x11]);

    row.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(5, 1, 0, 0, vec![row])]);
    let frame = &decode_sprite(&bytes).unwrap().frames[0];

    assert_eq!(frame.pixels, vec![None, Some(0x11), None, None, None]);
    assert_eq!(frame.rows[0].painted, 2);
}

#[test]
fn ff_inside_pixel_payload_is_a_palette_index() {
    // 0xFF is allowed inside the pixel field: the decoder reads exactly count bytes.
    let mut row = run(0, &[ROW_DELIMITER, 0x00, ROW_DELIMITER]);

    row.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(3, 1, 0, 0, vec![row])]);
    let frame = &decode_sprite(&bytes).unwrap().frames[0];

    assert_eq!(frame.pixels, vec![Some(0xFF), Some(0x00), Some(0xFF)]);
}

#[test]
fn zero_count_run_only_skips() {
    let mut row = run(2, &[]);

    row.extend(run(1, &[0x42]));
    row.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(4, 1, 0, 0, vec![row])]);
    let frame = &decode_sprite(&bytes).unwrap().frames[0];

    assert_eq!(frame.pixels, vec![None, None, None, Some(0x42)]);
    assert_eq!(frame.rows[0].runs.len(), 2);
    assert_eq!(frame.rows[0].runs[0].span(), 2);
}

#[test]
fn shadow_rows_are_pairs_of_counters() {
    // S_TANK §3.2: {4, 5}, {25, 5}, 0xFF at width = 49.
    let mut row = vec![4, 5, 25, 5];

    row.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(49, 1, 20, 23, vec![row])]);
    let sheet = decode_shadow(&bytes).unwrap();
    let frame = sheet.frame(0).unwrap();

    assert_eq!(frame.width, 49);
    assert_eq!((frame.hotx, frame.hoty), (20, 23));
    assert_eq!(frame.darkened_pixels(), 10);
    // {4, 5}: x = 4..=8; {25, 5}: x = 4 + 5 + 25 = 34..=38.
    assert!(frame.darkened(4, 0));
    assert!(frame.darkened(8, 0));
    assert!(frame.darkened(34, 0));
    assert!(frame.darkened(38, 0));
    assert!(!frame.darkened(3, 0));
    assert!(!frame.darkened(9, 0));
    assert!(!frame.darkened(33, 0));
    assert!(!frame.darkened(39, 0));
    assert!(!frame.darkened(48, 0));
    assert!(frame.rows[0].runs.iter().all(SpriteRun::is_shadow));
    assert!(frame.rows[0].runs.iter().all(|run| run.pixels.is_empty()));
}

#[test]
fn multi_frame_offsets_are_absolute() {
    let mut first = run(0, &[1, 2]);

    first.push(ROW_DELIMITER);

    let mut second = run(0, &[3]);

    second.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(2, 1, 0, 0, vec![first]), (1, 1, 7, 8, vec![second])]);
    let sheet = decode_sprite(&bytes).unwrap();

    assert_eq!(sheet.len(), 2);
    assert_eq!(sheet.frames[0].pixels, vec![Some(1), Some(2)]);
    assert_eq!(sheet.frames[1].pixels, vec![Some(3)]);
    assert_eq!((sheet.frames[1].hotx, sheet.frames[1].hoty), (7, 8));

    // Unit frames are laid out as "phase × 8 + angle" (§5.3).
    assert!(sheet.directional_frame(0, 0, 0).is_some());
    assert!(sheet.directional_frame(0, 1, 0).is_none());
}

#[test]
fn decode_returns_the_requested_mode() {
    let mut row = run(0, &[9, 9]);

    row.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(2, 1, 0, 0, vec![row])]);

    assert!(decode(&bytes, false).unwrap().as_sprite().is_some());
    assert!(!decode(&bytes, false).unwrap().is_shadow());
    assert_eq!(
        decode(&bytes, false)
            .unwrap()
            .as_sprite()
            .unwrap()
            .image_count,
        1
    );

    // The same width, but a shadow row: {0, 2} pairs without pixel bytes.
    let mut shadow_row = vec![0, 2];

    shadow_row.push(ROW_DELIMITER);

    let shadow_bytes = build_sprite(&[(2, 1, 0, 0, vec![shadow_row])]);
    let shadow = decode(&shadow_bytes, true).unwrap();

    assert!(shadow.is_shadow());
    assert!(shadow.as_sprite().is_none());
    assert_eq!(
        shadow
            .as_shadow()
            .unwrap()
            .frame(0)
            .unwrap()
            .darkened_pixels(),
        2
    );
}

#[test]
fn structural_errors_do_not_panic() {
    assert!(matches!(
        decode_sprite(b""),
        Err(SpriteError::TooShort { .. })
    ));
    assert!(matches!(
        decode_sprite(&0u16.to_le_bytes()),
        Err(SpriteError::ZeroImages)
    ));

    // image_count = 5, but the table is truncated.
    let mut truncated = vec![5, 0, 0, 0];

    assert!(matches!(
        decode_sprite(&truncated),
        Err(SpriteError::OffsetTableTruncated { .. })
    ));

    // A frame beyond the end of the body.
    truncated = vec![1, 0, 0xFF, 0x00, 0x00, 0x00];

    assert!(matches!(
        decode_sprite(&truncated),
        Err(SpriteError::FrameOutOfBounds { .. })
    ));

    // Zero frame width.
    let zero_width = build_sprite(&[(0, 1, 0, 0, vec![vec![ROW_DELIMITER]])]);

    assert!(matches!(
        decode_sprite(&zero_width),
        Err(SpriteError::BadDimensions { .. })
    ));

    // A row without a terminator: it reaches the end of the body.
    // height = 1, run {skip 0, count 1, 0xAA} and no 0xFF at the end.
    let broken = build_sprite(&[(1, 1, 0, 0, vec![vec![0u8, 1, 0xAA]])]);

    assert!(matches!(
        decode_sprite(&broken),
        Err(SpriteError::RowUnterminated { .. })
    ));

    // The row goes beyond width.
    let mut overrun = run(3, &[1, 2, 3]);

    overrun.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(5, 1, 0, 0, vec![overrun])]);

    assert!(matches!(
        decode_sprite(&bytes),
        Err(SpriteError::RowOverrun { .. })
    ));

    // A truncated pixel field.
    let mut clipped = vec![0u8, 5, 1, 2]; // count = 5, but only 2 bytes

    clipped.push(ROW_DELIMITER);

    let bytes = build_sprite(&[(5, 1, 0, 0, vec![clipped])]);

    assert!(matches!(
        decode_sprite(&bytes),
        Err(SpriteError::PixelPayloadTruncated { .. })
    ));
}

/// `D_TANK` from `MAX.RES` — 24 bytes: hull 0..7, turret 8..15,
/// firing 16..23 and `angle_offsets` (the same numbers C++ reads).
#[test]
fn frame_info_decodes_tank_data() {
    let bytes = [
        0u8, 8, 8, 8, 16, 8, 0, 0, // counters
        0, 7, 0xFF, 7, 0xF8, 0, 0xF8, 0xF9, // (0,7) (-7,7) (-8,0) (-8,-7)
        0, 0xF8, 6, 0xF8, 7, 0, 7, 6, // (0,-8) (6,-8) (7,0) (7,6)
    ];
    let info = FrameInfo::decode(&bytes).expect("D_TANK");

    assert_eq!(info.image_base, 0);
    assert_eq!(info.image_count, 8);
    assert_eq!(info.turret_image_base, 8);
    assert_eq!(info.turret_image_count, 8);
    assert_eq!(info.firing_image_base, 16);
    assert_eq!(info.firing_image_count, 8);
    assert_eq!(info.turret_frame_index(0), 8);
    assert_eq!(info.turret_frame_index(7), 15);
    assert_eq!(info.turret_offset(0), (0, 7));
    assert_eq!(info.turret_offset(2), (-8, 0));
    assert_eq!(info.turret_offset(7), (7, 6));
    // `angle` is masked to 0..7, as in C++ (`image_index & 0x07`).
    assert_eq!(info.turret_offset(10), (-8, 0));
}

/// 8-byte resource (`D_FIRING`) — without turret/offsets (`angle_offsets` = 0).
#[test]
fn frame_info_without_offsets_is_valid() {
    let info = FrameInfo::decode(&[0, 16, 0, 0, 16, 16, 0, 0]).expect("D_FIRING");

    assert_eq!(info.image_count, 16);
    assert_eq!(info.turret_image_count, 0);
    assert_eq!(info.angle_offsets, [(0, 0); 8]);
    assert_eq!(info.turret_offset(2), (0, 0));
    // Infantry: `image_count` = 200 (0xC8), C++ reads int8 → -56.
    assert_eq!(FrameInfo::decode(&[0, 200, 0, 0, 0, 0, 0, 0]).unwrap().image_count, 200);
    assert_eq!(FrameInfo::decode(&[0, 1, 2]), None, "короче заголовка");
}
