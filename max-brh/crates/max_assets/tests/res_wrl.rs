//! Интеграционные тесты `max_assets` на реальных фикстурах.
//!
//! Фикстуры (`tests/data/`):
//! * `PATCHES.RES` — копия `assets/PATCHES.RES` (84 записи, §6 `formats/res.md`);
//! * `INTROSUB.bin`, `SC_D0002.bin` — ресурсы, выгруженные эталонным
//!   `max-original-dasm/tools/res_parser.py`;
//! * `CRATER_3.WRL` — самая маленькая карта из `max-original/*.WRL`
//!   (669 349 байт, `tile_count = 154`);
//! * `crater_3_surface.txt` — `surface_map` этой карты, выгруженная
//!   `max-original-dasm/tools/wrl_parser.py dump-surface` (bit-в-bit эталон).

use std::fs;
use std::path::{Path, PathBuf};

use max_assets::res::{ENTRY_SIZE, FIRST_DATA_OFFSET, HEADER_SIZE};
use max_assets::{ResArchive, ResError, WrlError, WrlMap, WrlOffsets};
use max_types::enums::Surface;
use max_types::geo::MAP_TILES;
use max_types::map::MapSize;

// ---------------------------------------------------------------------------
// Фикстуры и эталоны
// ---------------------------------------------------------------------------

fn data_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(data_path(name)).unwrap_or_else(|error| panic!("не читается {name}: {error}"))
}

/// Читает `crater_3_surface.txt` эталонного Python-парсера в плоский `W*H`.
fn read_surface_reference() -> Vec<u8> {
    let text = fs::read_to_string(data_path("crater_3_surface.txt")).unwrap();
    let mut flat = Vec::with_capacity(MAP_TILES);

    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        for token in line.split_whitespace() {
            flat.push(
                u8::from_str_radix(token, 16)
                    .unwrap_or_else(|_| panic!("не hex-байт: {token:?}")),
            );
        }
    }

    assert_eq!(flat.len(), MAP_TILES, "эталон surface_map неполный");

    flat
}

// ---------------------------------------------------------------------------
// RES: PATCHES.RES
// ---------------------------------------------------------------------------

#[test]
fn patches_res_lists_84_entries_with_documented_offsets() {
    let bytes = read_fixture("PATCHES.RES");
    let archive = ResArchive::parse(&bytes).unwrap();

    assert_eq!(archive.file_size(), bytes.len());
    assert_eq!(archive.table_offset(), 0x000C_27AF);
    assert_eq!(archive.table_size(), 0x540);
    assert_eq!(archive.table_size() as usize % ENTRY_SIZE, 0);
    assert_eq!(archive.len(), 84);
    assert!(archive.warnings().is_empty(), "{:?}", archive.warnings());

    let expected_prefix: &[(usize, &str, u32, u32)] = &[
        (0, "FONT_1", 0x0000_000C, 31_896),
        (1, "FONT_2", 0x0000_7CA4, 30_040),
        (2, "FONT_5", 0x0000_F1FC, 32_644),
        (3, "ILOGO", 0x0001_7180, 234_467),
        (4, "INTROSUB", 0x0005_0563, 12),
        (5, "SC_ATTRI", 0x0005_056F, 21_114),
    ];

    for &(index, name, offset, size) in expected_prefix {
        let entry = &archive.list()[index];

        assert_eq!(entry.index, index);
        assert_eq!(entry.name, name, "тег записи {index} (NUL-мусор обрезан)");
        assert_eq!(entry.offset, offset, "offset записи {index} ({name})");
        assert_eq!(entry.size, size, "size записи {index} ({name})");
    }

    let last = archive.list().last().unwrap();

    assert_eq!(last.index, 83);
    assert_eq!(last.name, "SC_UNITS");
    assert_eq!(last.offset, 0x000B_37B8);
    assert_eq!(last.size, 61_431);

    // Инварианты §1.2: данные идут вплотную с 0x0C, последняя запись
    // заканчивается ровно на начале таблицы.
    let mut expected_offset = FIRST_DATA_OFFSET;

    for entry in archive.list() {
        assert_eq!(
            entry.offset, expected_offset,
            "разрыв/наложение перед записью {} ({})",
            entry.index, entry.name
        );
        expected_offset = entry.offset + entry.size;
    }

    assert_eq!(expected_offset, archive.table_offset());
    assert_eq!(archive.entry("FONT_1").unwrap().offset, HEADER_SIZE as u32);
}

#[test]
fn patches_res_read_matches_python_parser_dumps() {
    let archive = ResArchive::open(data_path("PATCHES.RES")).unwrap();

    let introsub = read_fixture("INTROSUB.bin");
    let sc_d0002 = read_fixture("SC_D0002.bin");

    assert_eq!(introsub.len(), 12);
    assert_eq!(archive.read("INTROSUB").unwrap(), introsub.as_slice());
    assert_eq!(archive.read("INTROSUB").unwrap(), b"maxint.json\0");

    assert_eq!(sc_d0002.len(), 345);
    assert_eq!(archive.read("SC_D0002").unwrap(), sc_d0002.as_slice());

    // FONT_1 — TrueType: sfnt-версия 0x00010000 + numTables 0x000E.
    let font = archive.read("FONT_1").unwrap();

    assert_eq!(font.len(), 31_896);
    assert_eq!(&font[..16], &[
        0x00, 0x01, 0x00, 0x00, 0x00, 0x0E, 0x00, 0x80, 0x00, 0x03, 0x00, 0x60, 0x46, 0x46, 0x54,
        0x4D,
    ]);

    assert!(matches!(
        archive.read("NOPE"),
        Err(ResError::NotFound { .. })
    ));
    assert!(
        matches!(archive.read("font_1"), Err(ResError::NotFound { .. })),
        "теги сравниваются с учётом регистра, как в эталонном парсере"
    );
}

#[test]
fn patches_res_extract_all_returns_every_resource() {
    let archive = ResArchive::parse(&read_fixture("PATCHES.RES")).unwrap();
    let extracted = archive.extract_all().unwrap();

    assert_eq!(extracted.len(), 84);
    assert_eq!(extracted.keys().next().map(String::as_str), Some("FONT_1"));
    assert_eq!(
        extracted.keys().last().map(String::as_str),
        Some("SC_UNITS")
    );
    assert_eq!(extracted["INTROSUB"], b"maxint.json\0");
    assert_eq!(extracted["SC_UNITS"].len(), 61_431);

    let mut total = 0usize;

    for entry in archive.list() {
        let payload = &extracted[&entry.name];

        assert_eq!(payload.len(), entry.size as usize, "размер {}", entry.name);
        total += payload.len();
    }

    assert_eq!(total, 796_579);
}

#[test]
fn res_rejects_bad_signature_and_truncated_files() {
    assert!(matches!(
        ResArchive::parse(b""),
        Err(ResError::TooShort { .. })
    ));
    assert!(matches!(
        ResArchive::parse(b"RES0"),
        Err(ResError::TooShort { .. })
    ));

    let bytes = read_fixture("PATCHES.RES");

    let mut bad_signature = bytes.clone();
    bad_signature[0..4].copy_from_slice(b"MAX0");

    match ResArchive::parse(&bad_signature) {
        Err(ResError::BadSignature { found }) => assert_eq!(found, *b"MAX0"),
        other => panic!("ожидалась BadSignature, получено {other:?}"),
    }

    // Половина файла: таблица (offset = 0xC27AF) уже не помещается.
    match ResArchive::parse(&bytes[..bytes.len() / 2]) {
        Err(ResError::TableOutOfBounds { .. }) => {}
        other => panic!("ожидалась TableOutOfBounds, получено {other:?}"),
    }

    // Обрезан ровно перед таблицей — заголовок цел, таблица на 0 байт.
    let mut zero_table = bytes[..HEADER_SIZE].to_vec();
    zero_table[4..8].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    zero_table[8..12].copy_from_slice(&0u32.to_le_bytes());

    assert!(ResArchive::parse(&zero_table).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// WRL: CRATER_3.WRL
// ---------------------------------------------------------------------------

#[test]
fn crater3_header_sections_and_raw_data() {
    let map = WrlMap::open(data_path("CRATER_3.WRL")).unwrap();

    assert_eq!(map.file_size(), 669_349);
    assert_eq!(map.signature(), *b"WRL");
    assert_eq!(map.signature_text(), "WRL");
    assert_eq!(map.layer_count(), 1);
    assert_eq!(
        map.layer_dimensions(),
        &[MapSize {
            width: 112,
            height: 112
        }]
    );
    assert_eq!((map.width(), map.height()), (112, 112));
    assert_eq!(map.cells(), 12_544);
    assert!(map.is_standard_size());
    assert_eq!(map.tile_count(), 154);
    assert!(map.warnings().is_empty(), "{:?}", map.warnings());
    assert_eq!(map.trailing_bytes(), 0);

    let expected_offsets = WrlOffsets {
        signature: 0x000000,
        layer_count: 0x000003,
        dims: 0x000005,
        minimap: 0x000009,
        tile_ids: 0x003109,
        tile_count: 0x009309,
        tiles: 0x00930B,
        palette: 0x0A330B,
        pass_table: 0x0A360B,
        end: 0x0A36A5,
    };

    assert_eq!(*map.offsets(), expected_offsets);
    assert_eq!(map.offsets().end, map.file_size());

    // §1: size = 38 411 + 4 097 * tile_count для 112×112.
    assert_eq!(map.file_size(), 38_411 + 4_097 * map.tile_count() as usize);

    assert_eq!(
        map.minimap().len(),
        map.cells(),
        "миникарта — ровно один слой W*H"
    );
    assert_eq!(
        &map.minimap()[..16],
        &[98, 98, 98, 99, 99, 98, 99, 99, 98, 99, 99, 98, 99, 99, 98, 98]
    );

    assert_eq!(map.tile_map().len(), map.cells());
    assert_eq!(
        &map.tile_map()[..16],
        &[0, 1, 2, 3, 4, 5, 3, 4, 5, 3, 4, 5, 3, 4, 0, 1]
    );

    assert_eq!(map.pass_table().len(), map.tile_count() as usize);
    assert_eq!(&map.pass_table()[..8], &[1, 1, 1, 1, 1, 1, 1, 1]);
    assert_eq!(
        &map.pass_table()[map.pass_table().len() - 8..],
        &[3, 3, 3, 0, 0, 3, 3, 3]
    );

    let mut pass_histogram = [0usize; 4];

    for &value in map.pass_table() {
        pass_histogram[value as usize] += 1;
    }

    assert_eq!(pass_histogram, [57, 9, 39, 49]);

    assert_eq!(
        &map.palette()[..16],
        &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 255]
    );
    assert_eq!(&map.palette()[96 * 3..96 * 3 + 3], &[183, 103, 0]);

    assert_eq!(map.tiles().len(), 154 * 4_096);
    assert_eq!(
        &map.tile_pixels(0).unwrap()[..32],
        &[
            97, 109, 103, 108, 104, 108, 103, 107, 104, 106, 104, 105, 109, 107, 107, 104, 106,
            103, 107, 102, 108, 102, 109, 101, 98, 101, 98, 101, 99, 100, 99, 99,
        ]
    );
    assert_eq!(map.tile_pixels(153).unwrap().len(), 4_096);
    assert!(map.tile_pixels(154).is_none());
}

#[test]
fn crater3_surface_map_matches_python_reference_bit_for_bit() {
    let map = WrlMap::parse(&read_fixture("CRATER_3.WRL")).unwrap();
    let expected = read_surface_reference();
    let surface = map.surface_map();
    let got = surface.to_bytes();

    assert_eq!(got.len(), expected.len());

    for (index, (&got, &want)) in got.iter().zip(expected.iter()).enumerate() {
        assert_eq!(
            got,
            want,
            "surface_map расходится с эталоном Python в клетке (x={}, y={}, index={index}): 0x{got:02X} != 0x{want:02X}",
            index % 112,
            index / 112
        );
    }

    // Гистограмма §3.1 для CRATER_3: LAND=2292, WATER=9429, COAST=753, AIR=70.
    let mut histogram = [0usize; 4];

    for tile in surface.tiles() {
        let slot = match tile.bits() {
            0x1 => 0,
            0x2 => 1,
            0x4 => 2,
            0x8 => 3,
            other => panic!("неожиданная поверхность 0x{other:02X}"),
        };

        histogram[slot] += 1;
    }

    assert_eq!(histogram, [2_292, 9_429, 753, 70]);

    // Паритет с формулой C++ (`world.cpp:382-409`), записанной здесь независимо
    // от крейтов: pass-таблица разворачивается в поверхность, tile-id клетки
    // индексируют её. Раньше это сверялось с `max_core::setup::derive_surface_map`;
    // после Ф5.2 у max_assets нет зависимости от max_core, поэтому эталонная
    // формула продублирована в тесте (плюс остаётся сверка с Python-дампом выше).
    const PASS_DATA: [u8; 4] = [0x01, 0x02, 0x04, 0x08]; // LAND, WATER, COAST, AIR
    let mut derived = Vec::with_capacity(MAP_TILES);

    for &tile_id in map.tile_map() {
        let pass_index = usize::from(map.pass_table()[usize::from(tile_id)]);
        let surface = PASS_DATA.get(pass_index).copied().unwrap_or(0);
        derived.push(surface);
    }

    assert_eq!(
        derived, got,
        "WrlMap::surface_map расходится с формулой world.cpp:382-409"
    );

    assert_eq!(map.pass_surfaces().len(), map.tile_count() as usize);
    assert_eq!(map.pass_surfaces()[0], Surface::WATER);
}

#[test]
fn wrl_open_and_parse_are_identical() {
    let path = data_path("CRATER_3.WRL");
    let opened = WrlMap::open(&path).unwrap();
    let parsed = WrlMap::parse(&read_fixture("CRATER_3.WRL")).unwrap();

    assert_eq!(opened.data(), parsed.data());
    assert_eq!(
        opened.surface_map().to_bytes(),
        parsed.surface_map().to_bytes()
    );
    assert_eq!(opened.tile_map(), parsed.tile_map());
    assert_eq!(opened.pass_table(), parsed.pass_table());
    assert_eq!(opened.palette(), parsed.palette());
}

#[test]
fn wrl_accepts_dmo_signature_and_rejects_broken_files() {
    let bytes = read_fixture("CRATER_3.WRL");

    // SNOW_1.WRL — известное исключение "DMO" (§1.1): файл валиден.
    let mut dmo = bytes.clone();
    dmo[0..3].copy_from_slice(b"DMO");

    let map = WrlMap::parse(&dmo).unwrap();

    assert_eq!(map.signature(), *b"DMO");
    assert!(map
        .warnings()
        .iter()
        .any(|warning| warning.contains("DMO")));
    assert_eq!(
        map.surface_map().to_bytes(),
        WrlMap::parse(&bytes).unwrap().surface_map().to_bytes()
    );

    let mut bad_signature = bytes.clone();
    bad_signature[0..3].copy_from_slice(b"XYZ");

    match WrlMap::parse(&bad_signature) {
        Err(WrlError::BadSignature { found }) => assert_eq!(found, *b"XYZ"),
        other => panic!("ожидалась BadSignature, получено {other:?}"),
    }

    // Обрезанный файл: нет даже заголовка / нет секций / нет pass-таблицы.
    assert!(matches!(
        WrlMap::parse(&bytes[..2]),
        Err(WrlError::TooShort { .. })
    ));
    assert!(matches!(
        WrlMap::parse(&bytes[..100]),
        Err(WrlError::Truncated { .. })
    ));
    assert!(matches!(
        WrlMap::parse(&bytes[..0x930B]),
        Err(WrlError::Truncated { .. })
    ));
    assert!(matches!(
        WrlMap::parse(&bytes[..bytes.len() - 1]),
        Err(WrlError::Truncated { .. })
    ));

    let mut zero_layers = bytes.clone();
    zero_layers[3..5].copy_from_slice(&0u16.to_le_bytes());

    assert!(matches!(
        WrlMap::parse(&zero_layers),
        Err(WrlError::ZeroLayers)
    ));
}

#[test]
fn wrl_never_panics_on_truncated_prefixes() {
    let bytes = read_fixture("CRATER_3.WRL");

    for len in 0..64 {
        assert!(
            WrlMap::parse(&bytes[..len]).is_err(),
            "префикс {len} байт должен быть ошибкой, а не валидной картой"
        );
    }

    // Случайные короткие срезы в середине данных тоже не паникуют.
    for len in [100usize, 1_000, 12_553, 37_642, 669_000] {
        let _ = WrlMap::parse(&bytes[..len]);
    }
}
