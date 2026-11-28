//! Unit tests for the distribution inventory (`inventory.rs`).
//!
//! The private helpers (reference collection, allowlist predicates, the
//! case-insensitive directory map) are tested here; the end-to-end manifest
//! against the real game data is `tests/asset_inventory.rs`.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use super::{
    hex_digest, music_is_allowlisted, ArchiveReferences, AssetClass, DiskFiles,
    ALLOWLISTED_MISSING_MISSIONS, ALLOWLISTED_MISSING_MUSIC, MUSIC_PLAYLIST, MUSIC_TRACKS,
    OUTCOME_TRACKS, PLANET_TRACKS,
};
use crate::res::ResArchive;
use sha2::Digest;

/// Builds a minimal valid RES archive: `RES0` + header + payloads + table.
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

#[test]
fn asset_classes_map_the_distribution_extensions() {
    let cases = [
        ("TANK1.SPW", Some(AssetClass::Sound)),
        ("AGT.FLC", Some(AssetClass::Flc)),
        ("SNOW_1.WRL", Some(AssetClass::Data)),
        ("MAIN_MSC.MSC", Some(AssetClass::Music)),
        ("MAXINT.MVE", Some(AssetClass::Video)),
        ("maxint.json", Some(AssetClass::Subtitles)),
        ("SAVE1.CAM", Some(AssetClass::Mission)),
        ("DESCR1.MPS", Some(AssetClass::Mission)),
        ("WIN9.CAM", Some(AssetClass::Mission)),
        ("MANUAL.PDF", None),
        ("MAX.INI", None),
        ("MAXRUN.EXE", None),
    ];

    for (name, expected) in cases {
        assert_eq!(
            AssetClass::from_file_name(name),
            expected,
            "class of {name}"
        );
        assert_eq!(
            AssetClass::from_extension(name.rsplit_once('.').expect("dot").1),
            expected,
            "class of the {name} extension"
        );
    }

    assert_eq!(AssetClass::from_extension("spw"), Some(AssetClass::Sound));
    assert_eq!(AssetClass::from_extension("SpW"), Some(AssetClass::Sound));
    assert_eq!(AssetClass::from_extension(""), None);
    assert_eq!(AssetClass::from_file_name("no-extension"), None);
}

#[test]
fn asset_classes_serialize_as_the_manifest_names() {
    for class in AssetClass::ALL {
        let json = serde_json::to_string(&class).expect("class serializes");

        assert_eq!(json, format!("\"{}\"", class.as_str()));
    }
}

#[test]
fn music_lists_cover_the_director_tags_without_duplicates() {
    assert_eq!(
        MUSIC_TRACKS,
        [
            MUSIC_PLAYLIST[0],
            MUSIC_PLAYLIST[1],
            MUSIC_PLAYLIST[2],
            MUSIC_PLAYLIST[3],
            MUSIC_PLAYLIST[4],
            MUSIC_PLAYLIST[5],
            MUSIC_PLAYLIST[6],
            MUSIC_PLAYLIST[7],
            MUSIC_PLAYLIST[8],
            MUSIC_PLAYLIST[9],
            PLANET_TRACKS[0],
            PLANET_TRACKS[1],
            PLANET_TRACKS[2],
            PLANET_TRACKS[3],
            OUTCOME_TRACKS[0],
            OUTCOME_TRACKS[1],
        ]
    );

    let unique: BTreeSet<&str> = MUSIC_TRACKS.into_iter().collect();

    assert_eq!(unique.len(), MUSIC_TRACKS.len(), "music tags are unique");

    for tag in ALLOWLISTED_MISSING_MUSIC {
        assert!(unique.contains(tag), "{tag} is a director tag");
    }
}

#[test]
fn only_the_known_retail_gaps_are_allowlisted() {
    assert_eq!(
        ALLOWLISTED_MISSING_MUSIC,
        ["BKG7_MSC", "BKG8_MSC", "BKG9_MSC"]
    );
    assert_eq!(ALLOWLISTED_MISSING_MISSIONS, ["SAVE2.DMO", "SAVE3.DMO"]);

    assert!(music_is_allowlisted(&["BKG7_MSC"]));
    assert!(music_is_allowlisted(&["BKG8_MSC", "BKG9_MSC"]));
    assert!(!music_is_allowlisted(&["BKG6_MSC"]));
    assert!(!music_is_allowlisted(&["BKG7_MSC", "MAIN_MSC"]));
    assert!(!music_is_allowlisted(&[]));
}

#[test]
fn archive_references_keep_only_ascii_file_names_of_known_classes() {
    let archive = ResArchive::parse(&build_archive(&[
        ("SOUND", b"tank1.spw\0"),
        ("MAP", b"snow_1.wrl\0"),
        ("MOVIE", b"maxint.mve\0"),
        ("SUB", b"maxint.json"),
        ("NOTE", b"readme.txt\0"),
        ("TEXT", b"a text resource with spaces\0"),
        ("SPRITE", &[0x18, 0x00, 0x62, 0x00]),
    ]))
    .expect("synthetic archive");

    let references = ArchiveReferences::collect(&archive);

    assert_eq!(references.total, 4);
    assert_eq!(
        references.unique.keys().cloned().collect::<Vec<_>>(),
        vec![
            "maxint.json".to_owned(),
            "maxint.mve".to_owned(),
            "snow_1.wrl".to_owned(),
            "tank1.spw".to_owned(),
        ]
    );
    assert_eq!(
        references.unique["tank1.spw"],
        BTreeSet::from(["SOUND".to_owned()])
    );
}

#[test]
fn duplicate_references_are_merged_by_file_name() {
    let archive = ResArchive::parse(&build_archive(&[
        ("TANK1", b"tank1.spw\0"),
        ("TANK14", b"tank1.spw\0"),
        ("TANK5", b"tank5.spw\0"),
    ]))
    .expect("synthetic archive");

    let references = ArchiveReferences::collect(&archive);

    assert_eq!(references.total, 3);
    assert_eq!(references.unique.len(), 2);
    assert_eq!(
        references.unique["tank1.spw"],
        BTreeSet::from(["TANK1".to_owned(), "TANK14".to_owned()])
    );
}

#[test]
fn a_duplicate_tag_does_not_hide_its_file_reference() {
    let archive = ResArchive::parse(&build_archive(&[
        ("D_DEFALT", &[0x18, 0x00, 0x62, 0x00]),
        ("D_DEFALT", b"tank1.spw\0"),
    ]))
    .expect("synthetic archive");

    let references = ArchiveReferences::collect(&archive);

    assert_eq!(references.total, 1);
    assert_eq!(
        references.unique["tank1.spw"],
        BTreeSet::from(["D_DEFALT".to_owned()])
    );
}

#[test]
fn large_payloads_are_not_file_references() {
    let mut payload = vec![b'a'; 128];
    payload[0..4].copy_from_slice(b"a.sp");
    payload.push(0);

    let archive =
        ResArchive::parse(&build_archive(&[("BIG", &payload)])).expect("synthetic archive");

    let references = ArchiveReferences::collect(&archive);

    assert_eq!(references.total, 0);
    assert!(references.unique.is_empty());
}

#[test]
fn disk_files_resolve_names_ignoring_ascii_case() {
    let dir = temp_dir("disk_files");

    fs::write(dir.join("TANK1.SPW"), b"x").expect("write fixture");
    fs::write(dir.join("snow_1.wrl"), b"x").expect("write fixture");
    fs::create_dir(dir.join("SUBDIR")).expect("create subdir");

    let disk = DiskFiles::scan(&dir).expect("scan");

    assert_eq!(disk.resolve("tank1.spw"), Some("TANK1.SPW"));
    assert_eq!(disk.resolve("SNOW_1.WRL"), Some("snow_1.wrl"));
    assert_eq!(disk.resolve("missing.spw"), None);
    assert_eq!(disk.resolve("SUBDIR"), None, "directories are not files");

    fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn case_collisions_resolve_to_the_lexicographically_first_name() {
    let dir = temp_dir("case_collision");

    fs::write(dir.join("b.spw"), b"x").expect("write fixture");
    fs::write(dir.join("A.SPW"), b"x").expect("write fixture");

    let disk = DiskFiles::scan(&dir).expect("scan");

    assert_eq!(disk.resolve("a.spw"), Some("A.SPW"));

    fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn sha256_hex_matches_the_known_vector() {
    let digest = sha2::Sha256::digest(b"abc");

    assert_eq!(
        hex_digest(&digest),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// Fresh directory under the system temp directory, unique per test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "max_assets_inventory_{name}_{}",
        std::process::id()
    ));

    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");

    dir
}
