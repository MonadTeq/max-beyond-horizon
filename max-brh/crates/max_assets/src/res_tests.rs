use super::*;

/// Builds a minimal valid archive from (tag, data) pairs.
fn build_archive(items: &[(&str, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut table = Vec::new();
    let mut offset = FIRST_DATA_OFFSET;

    for (name, payload) in items {
        let mut tag = [0u8; TAG_LEN];
        let bytes = name.as_bytes();

        tag[..bytes.len().min(TAG_LEN)].copy_from_slice(&bytes[..bytes.len().min(TAG_LEN)]);
        table.extend_from_slice(&tag);
        table.extend_from_slice(&offset.to_le_bytes());
        table.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        offset += payload.len() as u32;
        data.extend_from_slice(payload);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&RES_SIGNATURE);
    out.extend_from_slice(&(FIRST_DATA_OFFSET + data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(table.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out.extend_from_slice(&table);
    out
}

#[test]
fn duplicate_tags_keep_the_first_entry() {
    let bytes = build_archive(&[("DUP", b"first"), ("DUP", b"second"), ("END", b"z")]);
    let archive = ResArchive::parse(&bytes).unwrap();

    assert_eq!(archive.len(), 3);
    assert_eq!(archive.read("DUP").unwrap(), b"first");
    assert_eq!(archive.read("END").unwrap(), b"z");

    let all = archive.extract_all().unwrap();

    assert_eq!(all.len(), 2);
    assert_eq!(all["DUP"], b"first");
    assert_eq!(all["END"], b"z");
}

#[test]
fn eight_byte_and_nul_padded_tags_decode_as_c_strings() {
    let bytes = build_archive(&[("ABCDEFGH", b"x")]);
    // Second archive: a tag with a NUL and garbage after it.
    let mut with_garbage = build_archive(&[("ILOGO", b"y")]);
    let table_start = u32::from_le_bytes([
        with_garbage[4],
        with_garbage[5],
        with_garbage[6],
        with_garbage[7],
    ]) as usize;

    with_garbage[table_start + 5] = 0;
    with_garbage[table_start + 6] = b'T';
    with_garbage[table_start + 7] = b'M';

    assert_eq!(
        ResArchive::parse(&bytes).unwrap().read("ABCDEFGH").unwrap(),
            b"x"
    );
    assert_eq!(
        ResArchive::parse(&with_garbage)
            .unwrap()
            .read("ILOGO")
            .unwrap(),
            b"y"
    );
}

#[test]
fn non_multiple_table_size_warns_but_parses() {
    let mut bytes = build_archive(&[("A", b"1")]);
    let table_size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

    // Append 3 "extra" bytes to the table: count = size / 16.
    bytes[8..12].copy_from_slice(&(table_size + 3).to_le_bytes());
    bytes.extend_from_slice(&[0u8; 3]);

    let archive = ResArchive::parse(&bytes).unwrap();

    assert_eq!(archive.len(), 1);
    assert_eq!(archive.warnings().len(), 1);
}

#[test]
fn structural_errors_do_not_panic() {
    assert!(matches!(
        ResArchive::parse(b""),
        Err(ResError::TooShort { .. })
    ));
    assert!(matches!(
        ResArchive::parse(b"RES0\x00\x00\x00"),
        Err(ResError::TooShort { .. })
    ));
    assert!(matches!(
        ResArchive::parse(b"NOPE\x0C\x00\x00\x00\x00\x00\x00\x00"),
        Err(ResError::BadSignature { .. })
    ));

    // The table is beyond the end of the file.
    let mut bytes = build_archive(&[("A", b"1")]);
    bytes[4..8].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
    assert!(matches!(
        ResArchive::parse(&bytes),
        Err(ResError::TableOutOfBounds { .. })
    ));
}

#[test]
fn out_of_bounds_entry_is_a_warning_and_read_error() {
    let mut bytes = build_archive(&[("A", b"1")]);
    let table_start = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;

    // data_size = u32::MAX for the single record.
    bytes[table_start + 12..table_start + 16].copy_from_slice(&u32::MAX.to_le_bytes());

    let archive = ResArchive::parse(&bytes).unwrap();

    assert_eq!(archive.warnings().len(), 1);
    assert!(matches!(
        archive.read("A"),
        Err(ResError::ResourceOutOfBounds { .. })
    ));
    assert!(archive.extract_all().is_err());
}

#[test]
fn file_references_are_ascii_names_without_paths() {
    let bytes = build_archive(&[
        ("TANK1", b"tank1.spw\0"),
        ("SNOW_1", b"snow_1.w"),
        ("NO_NUL", b"menu38.spw"),
        ("BINARY", &[0x18, 0x00, 0x62, 0x00]),
        ("TRAVERSAL", b"../secret\0"),
        ("ABSOLUTE", b"C:\\game\\x.spw\0"),
        ("EMPTY", b"\0"),
    ]);
    let archive = ResArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_reference("TANK1"), Some("tank1.spw"));
    assert_eq!(archive.file_reference("SNOW_1"), Some("snow_1.w"));
    assert_eq!(
        archive.file_reference("NO_NUL"),
        Some("menu38.spw"),
            "NUL в конце не обязателен"
    );
    assert_eq!(archive.file_reference("BINARY"), None);
    assert_eq!(archive.file_reference("TRAVERSAL"), None);
    assert_eq!(archive.file_reference("ABSOLUTE"), None);
    assert_eq!(archive.file_reference("EMPTY"), None);
    assert_eq!(
        archive.file_reference("MISSING"),
        None,
            "нет тега — нет ссылки"
    );

    assert_eq!(parse_file_ref(b"tank1.spw\0"), Some("tank1.spw"));
    assert_eq!(parse_file_ref(b""), None);
    assert_eq!(parse_file_ref(b"snow_1.w\0garbage"), Some("snow_1.w"));
}

/// Real `MAX.RES`: hundreds of ASCIIZ references to `*.spw` and other files.
///
/// The check catches a discrepancy with the format (`spw.md` §1: 607 references to
/// `.spw`, 721 in total). If the game data is absent, the test is skipped.
#[test]
fn real_max_res_exposes_ascii_file_references() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAX.RES");

    if !path.is_file() {
        return;
    }

    let archive = ResArchive::open(&path).expect("MAX.RES");
    assert_eq!(archive.file_reference("TANK1"), Some("tank1.spw"));
    assert_eq!(archive.file_reference("KBUY0"), Some("kbuy0.spw"));
    assert_eq!(
        archive.file_reference("TANK"),
        None,
            "спрайт TANK — не ссылка на файл"
    );

    let mut spw = 0usize;
    let mut other = 0usize;

    for entry in archive.list() {
        // The references are tiny; sprites/maps are discarded before reading.
        if entry.size == 0 || entry.size > 64 {
            continue;
        }
        if let Some(name) = archive.file_reference(&entry.name) {
            if name.ends_with(".spw") {
                spw += 1;
            } else {
                other += 1;
            }
        }
    }

    assert!(spw >= 600, "ожидались сотни ссылок .spw, найдено {spw}");
    assert!(
        other >= 20,
            "ссылки .wrl/.flc/.msc/.mve тоже читаются: {other}"
    );
}
