use super::*;

fn sample(width: u32, height: u32) -> Image {
    let mut image = Image::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let value = ((x * 7 + y * 13) % 251) as u8;
            image.set_pixel(
                x,
                y,
                [value, value.wrapping_mul(3), value.wrapping_add(y as u8)],
            );
        }
    }

    image
}

#[test]
fn png_roundtrip_preserves_every_pixel() {
    let image = sample(160, 120);
    let bytes = encode(&image);

    assert_eq!(&bytes[..8], &SIGNATURE);
    assert_eq!(&bytes[12..16], b"IHDR");

    let decoded = decode(&bytes).expect("PNG разбирается");

    assert_eq!(decoded, image);
}

#[test]
fn palette_png_is_used_for_few_colors_and_roundtrips() {
    // 8-bit graphics: 4 colors → indexed PNG (color type 3).
    let palette = [[10u8, 20, 30], [40, 50, 60], [255, 255, 255], [0, 0, 0]];
    let mut image = Image::new(64, 48);

    for y in 0..48 {
        for x in 0..64 {
            image.set_pixel(x, y, palette[((x / 8 + y / 8) % 4) as usize]);
        }
    }

    let bytes = encode(&image);

    assert_eq!(bytes[25], 3, "color type 3 (индексированный)");
    assert!(
        bytes.windows(4).any(|window| window == b"PLTE"),
            "есть палитра"
    );
    assert_eq!(decode(&bytes).expect("разбирается"), image);
    assert!(
        bytes.len() < 2_000,
            "четырёхцветный кадр 64×48: {} байт",
        bytes.len()
    );
}

#[test]
fn many_colors_fall_back_to_truecolor() {
    let image = sample(64, 64);
    let bytes = encode(&image);

    assert_eq!(bytes[25], 2, "color type 2 (truecolor)");
    assert_eq!(decode(&bytes).expect("разбирается"), image);
}

#[test]
fn flat_frame_compresses_well() {
    let mut image = Image::new(640, 480);
    image.rgb.fill(0x40);

    let bytes = encode(&image);

    assert!(
        bytes.len() < 8_000,
            "однотонный кадр 640×480: {} байт",
        bytes.len()
    );
    assert_eq!(decode(&bytes).expect("разбирается"), image);
}

#[test]
fn crc_and_adler_catch_corruption() {
    let image = sample(32, 24);
    let mut bytes = encode(&image);

    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;

    assert!(decode(&bytes).is_err(), "порча IEND ловится CRC");
}

#[test]
fn compare_counts_only_pixels_above_the_threshold() {
    let expected = Image::new(10, 10);
    let mut actual = expected.clone();

    actual.set_pixel(0, 0, [30, 0, 0]); // exactly the threshold — not a change
    actual.set_pixel(1, 0, [31, 0, 0]); // above the threshold

    let diff = compare(&expected, &actual, 30).expect("сверка");

    assert_eq!(diff.total, 100);
    assert_eq!(diff.changed, 1);
    assert_eq!(diff.max_delta, 31);
    assert_eq!(diff.image.pixel(1, 0), Some([0xC0, 0x20, 0x20]));
    assert_eq!(diff.image.pixel(0, 0), Some([0xFF, 0xFF, 0xFF]));
}

#[test]
fn compare_rejects_different_sizes() {
    let left = Image::new(4, 4);
    let right = Image::new(5, 4);

    assert!(compare(&left, &right, 30).is_err());
}

#[test]
fn rgba_conversion_drops_alpha() {
    let rgba = [1u8, 2, 3, 255, 4, 5, 6, 0];
    let image = Image::from_rgba(2, 1, &rgba);

    assert_eq!(image.rgb, vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn adler32_matches_the_reference_value() {
    assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
}

#[test]
fn png_files_roundtrip_through_the_filesystem() {
    let dir = std::env::temp_dir().join(format!("max_png_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp");
    let path = dir.join("frame.png");

    let image = sample(64, 48);
    write(&path, &image).expect("запись");
    assert_eq!(read(&path).expect("чтение"), image);

    std::fs::remove_dir_all(dir).ok();
}
