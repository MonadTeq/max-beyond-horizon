use super::*;

/// Assembles a cursor: header + frames.
fn build(width: i16, height: i16, hot_x: i16, hot_y: i16, frames: &[&[u8]]) -> Vec<u8> {
    let mut bytes = Vec::new();

    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&hot_x.to_le_bytes());
    bytes.extend_from_slice(&hot_y.to_le_bytes());

    for frame in frames {
        bytes.extend_from_slice(frame);
    }

    bytes
}

#[test]
fn animated_cursor_frames_are_derived_from_height() {
    // C++: width < height -> frame_count = height / width, height /= frames.
    let frame0 = vec![0u8; 2 * 2];
    let frame1 = vec![1u8; 2 * 2];
    let frame2 = vec![2u8; 2 * 2];
    let bytes = build(2, 6, 1, 1, &[&frame0, &frame1, &frame2]);
    let cursor = CursorDescriptor::decode(&bytes).expect("курсор");

    assert_eq!((cursor.width, cursor.height), (2, 2));
    assert_eq!(cursor.frame_count, 3);
    assert!(cursor.is_animated());
    assert_eq!(cursor.hot_x, 1);
    assert_eq!(cursor.transparent, 0);
    assert_eq!(cursor.frame(0), Some(frame0.as_slice()));
    assert_eq!(cursor.frame(2), Some(frame2.as_slice()));
    assert_eq!(cursor.frame(3), None);
    assert_eq!(cursor.pixel(1, 0, 0), Some(1));
}

#[test]
fn static_cursor_keeps_the_full_height() {
    let pixels = vec![7u8; 4 * 3];
    let bytes = build(4, 3, 2, 1, &[&pixels]);
    let cursor = CursorDescriptor::decode(&bytes).expect("курсор");

    assert_eq!((cursor.width, cursor.height), (4, 3));
    assert_eq!(cursor.frame_count, 1);
    assert!(!cursor.is_animated());
    assert_eq!(cursor.transparent, 7);
    assert_eq!(cursor.frame(0).map(<[u8]>::len), Some(12));
}

#[test]
fn truncated_and_invalid_cursors_are_errors() {
    assert!(matches!(
        CursorDescriptor::decode(&[0u8; 4]),
        Err(CursorError::TooShort { .. })
    ));
    assert!(matches!(
        CursorDescriptor::decode(&build(0, 4, 0, 0, &[])),
        Err(CursorError::BadSize { .. })
    ));

    // Height 5 with width 2: C++ yields 2 frames of 2 rows each (integer
    // division), not an error.
    let bytes = build(2, 5, 0, 0, &[&[7u8; 8]]);
    let cursor = CursorDescriptor::decode(&bytes).expect("MINI_PTR-подобный курсор");
    assert_eq!((cursor.frame_count, cursor.height), (2, 2));

    // The header asks for 4×4 pixels, but they are not there.
    assert!(matches!(
        CursorDescriptor::decode(&build(4, 4, 0, 0, &[&[0u8; 3]])),
        Err(CursorError::Truncated { .. })
    ));
}

#[test]
fn real_max_res_cursors_match_the_cpp_descriptors() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../max-original/MAX.RES");

    if !path.is_file() {
        return;
    }

    let archive = ResArchive::open(&path).expect("MAX.RES");
    let cursors = CursorSet::load(&archive);

    assert_eq!(cursors.loaded(), CURSOR_COUNT, "в MAX.RES 30 курсоров");
    assert!(cursors.missing().is_empty());

    // Values from cursor.cpp/MAX.RES (see the table in the module header).
    let hand = cursors.get(1).expect("HANDPTR");
    assert_eq!((hand.width, hand.height), (18, 18));
    assert_eq!((hand.hot_x, hand.hot_y), (0, 1));
    assert_eq!(hand.frame_count, 1);

    let arrow = cursors.get(2).expect("ARROW_N");
    assert_eq!((arrow.width, arrow.height), (16, 16));
    assert_eq!((arrow.hot_x, arrow.hot_y), (7, 1));

    let enemy = cursors.get(18).expect("ENMY_PTR");
    assert_eq!((enemy.width, enemy.height), (37, 33));
    assert_eq!((enemy.hot_x, enemy.hot_y), (18, 13));

    let path_cursor = cursors.get(28).expect("PTR_PATH");
    assert_eq!((path_cursor.width, path_cursor.height), (23, 23));
    assert_eq!(path_cursor.frame_count, 10);
    assert_eq!((path_cursor.hot_x, path_cursor.hot_y), (13, 13));

    let hidden = cursors.get(0).expect("HIDDNPTR");
    assert_eq!((hidden.width, hidden.height), (1, 1));

    // Every frame of every cursor has exactly width*height pixels.
    for index in 0..CURSOR_COUNT {
        let cursor = cursors.get(index).expect(CURSOR_RESOURCES[index]);

        for frame in 0..usize::from(cursor.frame_count) {
            assert_eq!(
                cursor.frame(frame).map(<[u8]>::len),
                Some(cursor.frame_pixels()),
                    "{} кадр {frame}",
                CURSOR_RESOURCES[index]
            );
        }
    }
}
