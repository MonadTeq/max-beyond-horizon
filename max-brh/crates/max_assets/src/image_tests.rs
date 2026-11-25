use super::*;

#[test]
fn simple_image_header_and_pixels() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3i16.to_le_bytes());
    bytes.extend_from_slice(&2i16.to_le_bytes());
    bytes.extend_from_slice(&7i16.to_le_bytes());
    bytes.extend_from_slice(&9i16.to_le_bytes());
    // The pixels start right after the 8-byte header: in C++ the ninth byte
    // of the structure (`transparent_color`) is the first pixel.
    bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6]);

    let image = SimpleImage::decode(&bytes).expect("должна разобраться");

    assert_eq!((image.width, image.height), (3, 2));
    assert_eq!((image.ulx, image.uly), (7, 9));
    assert_eq!(image.pixel(0, 0), Some(1));
    assert_eq!(image.pixel(2, 1), Some(6));
    assert_eq!(image.pixel(3, 1), None);
    assert_eq!(
        image.transparent, 1,
            "прозрачный индекс — первый пиксель (C++ читает его с offset 8)"
    );
    assert_eq!(
        bytes.len(),
        SIMPLE_HEADER + 6,
            "8-байтный заголовок + пиксели"
    );
}

#[test]
fn simple_image_rejects_truncated() {
    // The header asks for 3×2, but there are four pixels.
    let bytes = [3u8, 0, 2, 0, 0, 0, 0, 0, 0, 1, 2, 3];

    assert!(matches!(
        SimpleImage::decode(&bytes),
        Err(ImageError::Truncated { .. })
    ));
}

/// Big-image header + RLE rows.
fn big_image_bytes(rows: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0i16.to_le_bytes());
    bytes.extend_from_slice(&0i16.to_le_bytes());
    bytes.extend_from_slice(&2i16.to_le_bytes()); // width
    bytes.extend_from_slice(&(rows.len() as i16).to_le_bytes());
    // palette: index 1 = (10,20,30), index 2 = (40,50,60)
    let mut palette = [0u8; PALETTE_BYTES];
    palette[3..6].copy_from_slice(&[10, 20, 30]);
    palette[6..9].copy_from_slice(&[40, 50, 60]);
    bytes.extend_from_slice(&palette);
    for row in rows {
        bytes.extend_from_slice(row);
    }
    bytes
}

#[test]
fn big_image_rle_literal_and_repeat() {
    // Row 1: a literal of 2 bytes; row 2: a repeat of -2 of value 2.
    let mut row1 = Vec::new();
    row1.extend_from_slice(&2i16.to_le_bytes());
    row1.extend_from_slice(&[1, 2]);
    let mut row2 = Vec::new();
    row2.extend_from_slice(&(-2i16).to_le_bytes());
    row2.push(2);

    let image = BigImage::decode(&big_image_bytes(&[row1, row2])).expect("RLE разбирается");

    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(image.pixel(0, 0), Some(1));
    assert_eq!(image.pixel(1, 0), Some(2));
    assert_eq!(image.pixel(0, 1), Some(2));
    assert_eq!(image.pixel(1, 1), Some(2));
    assert_eq!(image.rgb(0, 0), Some([10, 20, 30]));
    assert_eq!(image.rgb(1, 1), Some([40, 50, 60]));
}

#[test]
fn big_image_allows_chunk_crossing_row_boundary() {
    // As in FRAMEPIC: the chunk is wider than the row. C++ writes sequentially and moves to
    // the next row; the tail is overwritten by the start of the next row.
    let mut rows = Vec::new();
    // row 0: a literal of 3 bytes at a width of 2 (spills into row 1)
    let mut row0 = Vec::new();
    row0.extend_from_slice(&3i16.to_le_bytes());
    row0.extend_from_slice(&[1, 2, 1]);
    rows.push(row0);
    // row 1: a repeat of -2 of value 2
    let mut row1 = Vec::new();
    row1.extend_from_slice(&(-2i16).to_le_bytes());
    row1.push(2);
    rows.push(row1);

    let image = BigImage::decode(&big_image_bytes(&rows)).expect("пересечение строк допустимо");

    assert_eq!(image.pixel(0, 0), Some(1));
    assert_eq!(image.pixel(1, 0), Some(2));
    assert_eq!(
        image.pixel(0, 1),
        Some(2),
            "хвост перекрыт началом строки 1"
    );
    assert_eq!(image.pixel(1, 1), Some(2));
}

#[test]
fn big_image_tolerates_short_stream() {
    // The first literal is read, the second is cut off: the rest of the frame is zeros.
    let mut row = Vec::new();
    row.extend_from_slice(&1i16.to_le_bytes());
    row.push(1);
    row.extend_from_slice(&9i16.to_le_bytes()); // there is no more data for it
    row.extend_from_slice(&[2, 3]);

    let image = BigImage::decode(&big_image_bytes(&[row])).expect("короткий поток допустим");

    assert_eq!(image.pixel(0, 0), Some(1));
    assert_eq!(image.pixel(1, 0), Some(0), "недостающее — нули");
}

/// `SNOW_PIC` — decoder regression: its RLE contains a literal that
/// runs past the record (a service chunk). If **all** rows are cut off there,
/// the planet picture stays almost black (visible in the 8×3 grid).
#[test]
fn snow_pic_decodes_all_rows() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let path = root.join("max-original/MAX.RES");

    if !path.is_file() {
        return;
    }

    let archive = crate::res::ResArchive::open(&path).expect("MAX.RES");
    let bytes = archive.read("SNOW_PIC").expect("SNOW_PIC");
    let image = BigImage::decode(bytes).expect("SNOW_PIC");

    assert_eq!((image.width, image.height), (128, 128));

    let nonzero = image.pixels().iter().filter(|&&value| value != 0).count();

    assert!(
        nonzero > 10_000,
            "SNOW_PIC почти пустая: {nonzero} ненулевых"
    );
    assert!(
        (0..128).any(|x| image.pixel(x, 100) != Some(0)),
            "нижние строки планеты должны декодироваться"
    );
}
