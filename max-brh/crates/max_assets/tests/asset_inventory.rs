//! Distribution inventory on the real game data (`asset_inventory` stage 1).
//!
//! Gate G1 of `max-brh-docs/development/distro-installers.md` §F: the manifest
//! must be exact, deterministic and complete for the retail data set. The
//! integration test stages the oracle directory next to the real `PATCHES.RES`
//! and `maxint.json` from the C++ port's shared assets (`max-cpp-port/assets/`)
//! — the same files the game resolves at runtime through `PATCHES_RES_FALLBACKS`
//! (`max_app/src/assets.rs`) — builds the manifest and pins the documented
//! counts:
//!
//! * 1828 `MAX.RES` records, 721 file references to 567 distinct files
//!   (`max-original-dasm/formats/spw.md` §1);
//! * 71 `*.FLC`, 272 `*.SPW`, 24 `*.WRL`, 3 `*.MVE`, 13 `*.MSC`;
//! * 66 mission descriptors, 61 mission files (2 allowlisted demo gaps) and
//!   61 briefings (`DESCR*` + `WIN9.CAM`);
//! * no required file missing, and the DOS/GOG files excluded.
//!
//! Why an integration test: it goes through the public [`build`] API and the
//! real archives exactly like the `asset_inventory` example. The unit tests of
//! the private helpers live in `src/inventory_tests.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use max_assets::inventory::{
    AssetClass, DataInventory, INVENTORY_FILE, INVENTORY_SCHEMA, MUSIC_TRACKS,
};
use max_assets::ResArchive;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Oracle game data directory (read-only).
fn real_data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original")
}

/// C++ port's shared assets directory (read-only).
///
/// It holds the real `PATCHES.RES` and `maxint.json` a runtime build loads:
/// `PATCHES_RES_FALLBACKS` in `max_app/src/assets.rs` ends with this directory.
fn shared_assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-cpp-port/assets")
}

/// SHA-256 of the real `max-cpp-port/assets/PATCHES.RES` (798,150 bytes).
///
/// Pinned so the gate can never silently fall back to the 797,951-byte
/// `tests/data/PATCHES.RES` copy (the two differ in the `SC_SCHES` record).
const REAL_PATCHES_RES_SHA256: &str =
    "f2b8dc5293cac65e171099ca4dbf43f57e034cccd9c0f2290e70655cc0d71b10";

/// Stages the oracle data directory plus the real `PATCHES.RES`/`maxint.json`
/// into the cargo target temporary directory.
///
/// Files are hard-linked (they are read-only in this test); the two files the
/// checkout keeps outside the oracle directory are copied from the C++ port's
/// shared assets, so the manifest hashes exactly what the game loads. A copy
/// fallback keeps the test working on a filesystem without hard links.
fn stage_real_data(name: &str) -> PathBuf {
    let source = real_data_dir();

    assert!(
        source.join("MAX.RES").is_file(),
        "required test fixture is missing: {:?}",
        source.join("MAX.RES")
    );

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("asset_inventory_{name}"));

    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create staging dir");

    for entry in fs::read_dir(&source).expect("read the oracle data dir") {
        let entry = entry.expect("directory entry");

        if !entry.file_type().expect("file type").is_file() {
            continue;
        }

        let target = dir.join(entry.file_name());

        if fs::hard_link(entry.path(), &target).is_err() {
            fs::copy(entry.path(), &target).expect("copy the oracle data file");
        }
    }

    for name in ["PATCHES.RES", "maxint.json"] {
        let source = shared_assets_dir().join(name);
        let target = dir.join(name);

        assert!(
            source.is_file(),
            "required runtime asset is missing: {source:?}"
        );

        // Never copy over an existing file: it could be a hard link to the
        // oracle and the write would modify the oracle in place.
        if target.exists() {
            fs::remove_file(&target).expect("remove the staged file");
        }

        fs::copy(&source, &target).expect("copy the runtime asset");
    }

    dir
}

/// Minimal valid RES archive: `RES0` header, payloads, record table.
fn build_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"RES0");
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());

    let mut records = Vec::new();

    for (tag, body) in entries {
        let offset = data.len() as u32;
        data.extend_from_slice(body);

        let mut record = [0u8; 16];
        record[..tag.len()].copy_from_slice(tag.as_bytes());
        record[8..12].copy_from_slice(&offset.to_le_bytes());
        record[12..16].copy_from_slice(&(body.len() as u32).to_le_bytes());
        records.push(record);
    }

    let table_offset = data.len() as u32;

    for record in records {
        data.extend_from_slice(&record);
    }

    let table_size = data.len() as u32 - table_offset;
    data[4..8].copy_from_slice(&table_offset.to_le_bytes());
    data[8..12].copy_from_slice(&table_size.to_le_bytes());

    data
}

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).expect("read fixture");
    let digest = Sha256::digest(&bytes);

    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn find<'a>(inventory: &'a DataInventory, path: &str) -> &'a max_assets::InventoryFile {
    inventory
        .files()
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("{path} is not in the manifest"))
}

// ---------------------------------------------------------------------------
// MAX.RES reference set (documented oracle counts)
// ---------------------------------------------------------------------------

/// `MAX.RES` has 1828 records and 721 file references to 567 distinct files:
/// 607 `*.spw`, 71 `*.flc`, 24 `*.wrl`, 16 `*.msc`, 3 `*.mve`
/// (`max-original-dasm/formats/spw.md` §1).
#[test]
fn real_max_res_has_the_documented_reference_set() {
    let archive = ResArchive::open(real_data_dir().join("MAX.RES")).expect("oracle MAX.RES");

    assert_eq!(archive.len(), 1828, "MAX.RES record count");

    let mut total = 0usize;
    let mut unique: BTreeMap<String, AssetClass> = BTreeMap::new();

    for entry in archive.list() {
        if entry.size == 0 || entry.size > 64 {
            continue;
        }

        let Some(name) = archive.file_reference(&entry.name) else {
            continue;
        };
        let Some((_, extension)) = name.rsplit_once('.') else {
            continue;
        };
        let Some(class) = AssetClass::from_extension(extension) else {
            continue;
        };

        total += 1;
        unique.insert(name.to_owned(), class);
    }

    assert_eq!(total, 721, "MAX.RES file references");
    assert_eq!(unique.len(), 567, "distinct referenced files");

    let mut by_class: BTreeMap<AssetClass, (usize, usize)> = BTreeMap::new();

    for class in unique.values() {
        by_class.entry(*class).or_default().0 += 1;
    }
    // Reference records by extension (a file can be referenced by many tags).
    for entry in archive.list() {
        if entry.size == 0 || entry.size > 64 {
            continue;
        }

        let Some(name) = archive.file_reference(&entry.name) else {
            continue;
        };
        let Some((_, extension)) = name.rsplit_once('.') else {
            continue;
        };
        let Some(class) = AssetClass::from_extension(extension) else {
            continue;
        };

        by_class.entry(class).or_default().1 += 1;
    }

    let expected: [(AssetClass, usize, usize); 5] = [
        (AssetClass::Sound, 454, 607),
        (AssetClass::Flc, 71, 71),
        (AssetClass::Data, 24, 24),
        (AssetClass::Music, 15, 16),
        (AssetClass::Video, 3, 3),
    ];

    for (class, unique_count, reference_count) in expected {
        let (unique_actual, reference_actual) = by_class[&class];

        assert_eq!(unique_actual, unique_count, "{class:?} distinct files");
        assert_eq!(reference_actual, reference_count, "{class:?} references");
    }
}

// ---------------------------------------------------------------------------
// Full manifest gate
// ---------------------------------------------------------------------------

/// Gate G1: exact counts, classes, digests, exclusions, allowlists and
/// determinism of the complete distribution manifest.
#[test]
fn real_distribution_manifest_matches_the_stage_one_gate() {
    let dir = stage_real_data("gate");
    let inventory = max_assets::inventory::build(&dir).expect("build the inventory");

    assert_eq!(inventory.schema(), INVENTORY_SCHEMA);
    assert!(
        inventory.is_complete(),
        "required files are missing: {:?}",
        inventory.missing()
    );
    assert!(inventory.missing().is_empty());

    let expected: [(AssetClass, usize); 7] = [
        (AssetClass::Data, 27),
        (AssetClass::Mission, 122),
        (AssetClass::Sound, 272),
        (AssetClass::Flc, 71),
        (AssetClass::Video, 3),
        (AssetClass::Music, 13),
        (AssetClass::Subtitles, 1),
    ];

    for (class, count) in expected {
        assert_eq!(inventory.class_count(class), count, "{class:?} files");
    }

    assert_eq!(inventory.totals().files, 509, "manifest entries");
    assert_eq!(
        inventory.totals().bytes,
        385_718_310,
        "the retail distribution is 367.8 MiB of game data"
    );

    // Sorted, unique, well-formed entries with a digest that matches the file.
    let mut previous: Option<&str> = None;
    let mut paths = BTreeSet::new();

    for file in inventory.files() {
        if let Some(previous) = previous {
            assert!(
                previous < file.path.as_str(),
                "manifest is not sorted: {previous} >= {}",
                file.path
            );
        }
        previous = Some(&file.path);

        assert!(paths.insert(file.path.as_str()), "duplicate {}", file.path);
        assert_eq!(file.sha256.len(), 64, "{} digest length", file.path);
        assert!(
            file.sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "{} digest is not lower-case hex: {}",
            file.path,
            file.sha256
        );

        let metadata = fs::metadata(dir.join(&file.path)).expect("manifest file exists");

        assert_eq!(metadata.len(), file.size, "{} size", file.path);
    }

    // The digest matches the bytes; checking a representative sample keeps the
    // test fast (the builder hashed every file above).
    for path in [
        "MAX.RES",
        "PATCHES.RES",
        "COLOR.PAL",
        "maxint.json",
        "SAVE1.CAM",
        "TANK1.SPW",
        "AGT.FLC",
    ] {
        assert_eq!(
            find(&inventory, path).sha256,
            sha256_file(&dir.join(path)),
            "{path} digest"
        );
    }

    // The real archive the game loads at runtime (798,150 bytes), not the older
    // 797,951-byte `tests/data/PATCHES.RES` copy.
    let patches = find(&inventory, "PATCHES.RES");

    assert_eq!(patches.size, 798_150, "real PATCHES.RES size");
    assert_eq!(
        patches.sha256, REAL_PATCHES_RES_SHA256,
        "real PATCHES.RES digest"
    );

    // Every distribution class is represented by the expected kind of file.
    let cases = [
        ("MAX.RES", AssetClass::Data),
        ("PATCHES.RES", AssetClass::Data),
        ("COLOR.PAL", AssetClass::Data),
        ("SNOW_1.WRL", AssetClass::Data),
        ("TANK1.SPW", AssetClass::Sound),
        ("AGT.FLC", AssetClass::Flc),
        ("MAXINT.MVE", AssetClass::Video),
        ("MAIN_MSC.MSC", AssetClass::Music),
        ("maxint.json", AssetClass::Subtitles),
        ("SAVE1.CAM", AssetClass::Mission),
        ("SAVE1.DMO", AssetClass::Mission),
        ("DESCR1.CAM", AssetClass::Mission),
        ("DESCR1.MPS", AssetClass::Mission),
        ("DESCR1.SCE", AssetClass::Mission),
        ("DESCR1.TRA", AssetClass::Mission),
        ("WIN9.CAM", AssetClass::Mission),
    ];

    for (path, class) in cases {
        assert_eq!(find(&inventory, path).class, class, "{path} class");
    }

    // DOS/GOG files and user saves are not part of the distribution (plan §A).
    for excluded in [
        "MAX.INI",
        "MANUAL.PDF",
        "SETUP.EXE",
        "MAXRUN.EXE",
        "DOS4GW.EXE",
        "MAX.gog",
        "MAXRUN.BAT",
        "goggame-1207663773.ico",
        "unins000.exe",
        "preferences.ini",
        "save10.cam",
        "save10.dta",
        "save10.hot",
        "INTRO1.CAM",
    ] {
        assert!(
            inventory
                .files()
                .iter()
                .all(|file| !file.path.eq_ignore_ascii_case(excluded)),
            "{excluded} must not be shipped"
        );
    }

    // Scan counters (oracle documentation).
    let sources = inventory.sources();

    assert_eq!(sources.max_res_entries, 1828);
    assert_eq!(sources.max_res_file_references, 721);
    assert_eq!(sources.max_res_unique_references, 567);
    assert_eq!(
        sources.max_res_unresolved_references, 184,
        "182 unshipped legacy sound tags + 2 allowlisted music files"
    );
    assert_eq!(sources.patches_res_entries, 84);
    assert_eq!(sources.patches_res_file_references, 1);
    assert_eq!(sources.patches_res_unique_references, 1);
    assert_eq!(sources.patches_res_unresolved_references, 0);
    assert_eq!(sources.mission_descriptors, 66);
    assert_eq!(sources.mission_files, 63);
    assert_eq!(sources.mission_files_present, 61);
    assert_eq!(sources.mission_hashes_verified, 61);
    assert_eq!(sources.mission_hashes_mismatched, 0);
    assert_eq!(sources.mission_briefing_files, 61);
    assert_eq!(sources.music_tracks, 16);
    assert_eq!(sources.music_files, 13);
    assert_eq!(sources.music_allowlisted_missing, 3);
    assert_eq!(sources.music_missing, 0);

    // The only gaps are the documented retail ones.
    let gaps: Vec<(String, AssetClass, Vec<String>)> = inventory
        .allowlisted_missing()
        .iter()
        .map(|gap| (gap.path.clone(), gap.class, gap.referenced_by.clone()))
        .collect();
    let expected_gaps: Vec<(String, AssetClass, Vec<String>)> = [
        ("SAVE2.DMO", AssetClass::Mission, vec!["SC_D0002"]),
        ("SAVE3.DMO", AssetClass::Mission, vec!["SC_D0003"]),
        ("bkg7_msc.msc", AssetClass::Music, vec!["BKG7_MSC"]),
        (
            "bkg9_msc.msc",
            AssetClass::Music,
            vec!["BKG8_MSC", "BKG9_MSC"],
        ),
    ]
    .into_iter()
    .map(|(path, class, tags)| {
        (
            path.to_owned(),
            class,
            tags.into_iter().map(str::to_owned).collect(),
        )
    })
    .collect();

    assert_eq!(gaps, expected_gaps);

    // The JSON has no volatile fields and carries the schema.
    let first = inventory.to_json();

    assert!(
        !first.contains(&dir.display().to_string()),
        "the manifest must not embed the absolute data directory"
    );

    let json: serde_json::Value = serde_json::from_str(&first).expect("manifest is JSON");

    assert_eq!(json["schema"].as_u64(), Some(u64::from(INVENTORY_SCHEMA)));
    assert_eq!(json["totals"]["files"].as_u64(), Some(509));
    assert_eq!(json["classes"].as_array().expect("classes array").len(), 7);
    assert_eq!(json["missing"].as_array().expect("missing array").len(), 0);

    // `write` stores exactly the deterministic text.
    let out = dir.join(INVENTORY_FILE);
    inventory.write(&out).expect("write the manifest");
    assert_eq!(fs::read_to_string(&out).expect("read the manifest"), first);

    fs::remove_dir_all(&dir).expect("cleanup");
}

// ---------------------------------------------------------------------------
// Synthetic data set: missing files and determinism
// ---------------------------------------------------------------------------

/// Small data directory with the core files and the music playlist, but no
/// `maxint.json` and no mission files.
fn stage_synthetic_data(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("asset_inventory_{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create data dir");

    // MAX.RES: a sound reference and every music tag (otherwise the playlist
    // check would correctly report 13 missing tracks on this synthetic set).
    let bodies: Vec<Vec<u8>> = MUSIC_TRACKS
        .iter()
        .map(|tag| format!("{}.msc\0", tag.to_ascii_lowercase()).into_bytes())
        .collect();
    let mut entries: Vec<(&str, &[u8])> = MUSIC_TRACKS
        .iter()
        .zip(&bodies)
        .map(|(tag, body)| (*tag, body.as_slice()))
        .collect();
    entries.push(("TANK1", b"tank1.spw\0"));

    fs::write(dir.join("MAX.RES"), build_archive(&entries)).expect("write MAX.RES");
    fs::write(dir.join("PATCHES.RES"), build_archive(&[])).expect("write PATCHES.RES");
    fs::write(dir.join("COLOR.PAL"), b"palette").expect("write COLOR.PAL");
    fs::write(dir.join("tank1.spw"), b"sound").expect("write sound");

    for body in &bodies {
        let name = std::str::from_utf8(body)
            .expect("ASCII name")
            .trim_end_matches('\0');

        fs::write(dir.join(name), b"music").expect("write music");
    }

    dir
}

/// A data directory with the core files but no `maxint.json` and no mission
/// files reports both as required gaps and still writes a manifest.
#[test]
fn missing_required_files_are_reported() {
    let dir = stage_synthetic_data("missing");
    let inventory = max_assets::inventory::build(&dir).expect("build the inventory");

    assert!(!inventory.is_complete());
    assert_eq!(inventory.missing_count(AssetClass::Mission), 1, "WIN9.CAM");
    assert_eq!(
        inventory.missing_count(AssetClass::Subtitles),
        1,
        "maxint.json"
    );
    assert!(inventory.allowlisted_missing().is_empty());
    assert_eq!(inventory.class_count(AssetClass::Sound), 1);
    assert_eq!(inventory.class_count(AssetClass::Music), 16);
    assert_eq!(inventory.class_count(AssetClass::Data), 3);

    let paths: Vec<&str> = inventory
        .missing()
        .iter()
        .map(|missing| missing.path.as_str())
        .collect();

    assert_eq!(paths, ["WIN9.CAM", "maxint.json"]);

    fs::remove_dir_all(&dir).expect("cleanup");
}

/// Two builds over the same data directory produce byte-identical JSON: the
/// manifest has no timestamps, absolute paths or directory-order dependence.
#[test]
fn manifest_is_deterministic_for_the_same_data() {
    let dir = stage_synthetic_data("deterministic");
    let first = max_assets::inventory::build(&dir)
        .expect("first build")
        .to_json();
    let second = max_assets::inventory::build(&dir)
        .expect("second build")
        .to_json();

    assert_eq!(first, second, "the manifest must be deterministic");
    assert!(first.contains("\"missing\""));

    fs::remove_dir_all(&dir).expect("cleanup");
}
