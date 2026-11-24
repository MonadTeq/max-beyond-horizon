//! Mission registry (`PATCHES.RES` `SC_*` descriptors) on real fixtures.
//!
//! Stage 0 gate: `Campaign` has 9 missions, the first is "Islands in the Sun"
//! with `SAVE1.CAM`; `Scenario` 24, `Training` 15, `MultiPlayerScenario` 12.
//!
//! Fixture: `tests/data/PATCHES.RES` (a copy of the C++ port's shared
//! `assets/PATCHES.RES`). No `SAVE*.CAM` files are committed, so mission-file
//! resolution is exercised with a temporary data directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use max_assets::mission::{xor_decode, MediaResource, MissionRegistry, DEFAULT_LANGUAGE};
use max_assets::ResArchive;
use max_types::victory::MissionCategory;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

fn load_registry() -> MissionRegistry {
    MissionRegistry::open(data_dir().join("PATCHES.RES"), data_dir())
        .expect("PATCHES.RES must parse")
}

/// Temporary data directory for mission-file resolution tests.
fn temp_data_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let dir = std::env::temp_dir().join(format!(
        "max_assets_mission_{name}_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    fs::create_dir_all(&dir).expect("create temp data dir");
    dir
}

// ---------------------------------------------------------------------------
// Stage 0 gate
// ---------------------------------------------------------------------------

/// `registry.missions(Campaign).len() == 9`, `[0].title == "Islands in the Sun"`,
/// `[0].mission` ends with `SAVE1.CAM`.
#[test]
fn campaign_has_nine_missions_starting_with_islands_in_the_sun() {
    let registry = load_registry();
    let campaign = registry.missions(MissionCategory::Campaign);

    assert_eq!(campaign.len(), 9, "Campaign: SC_C0001..SC_C0009");
    assert_eq!(campaign[0].title_text(), "Islands in the Sun");
    assert_eq!(campaign[0].mission_name.as_deref(), Some("SAVE1.CAM"));
    assert!(
        campaign[0]
            .mission_path()
            .expect("mission path")
            .ends_with("SAVE1.CAM"),
        "resolved to {:?}",
        campaign[0].mission_path()
    );
    assert_eq!(campaign[0].tag, "SC_C0001");
    assert_eq!(campaign[0].category, MissionCategory::Campaign);
}

/// Category sizes and C++ resource-id ordering inside each category.
#[test]
fn categories_have_cpp_sizes_and_mission_index_order() {
    let registry = load_registry();

    let expected: [(MissionCategory, usize); 8] = [
        (MissionCategory::Custom, 1),
        (MissionCategory::Training, 15),
        (MissionCategory::Campaign, 9),
        (MissionCategory::HotSeat, 1),
        (MissionCategory::Multiplayer, 1),
        (MissionCategory::Demo, 3),
        (MissionCategory::Scenario, 24),
        (MissionCategory::MultiPlayerScenario, 12),
    ];

    for (category, count) in expected {
        assert_eq!(
            registry.missions(category).len(),
            count,
            "{category:?} mission count"
        );
    }

    assert_eq!(registry.len(), 66, "SC_MIS_S..SC_MIS_E: 66 descriptors");

    // `mission_index` is the position in the category vector, so the resource
    // order must be preserved exactly (`missionmanager.cpp:74-86`).
    let ordered: [(MissionCategory, &str, usize); 5] = [
        (MissionCategory::Campaign, "SC_C", 9),
        (MissionCategory::Scenario, "SC_S", 24),
        (MissionCategory::Training, "SC_T", 15),
        (MissionCategory::MultiPlayerScenario, "SC_M", 12),
        (MissionCategory::Demo, "SC_D", 3),
    ];

    for (category, prefix, count) in ordered {
        let missions = registry.missions(category);

        for (index, descriptor) in missions.iter().enumerate() {
            assert_eq!(
                descriptor.tag,
                format!("{prefix}{:04}", index + 1),
                "{category:?} index {index}"
            );
        }

        assert_eq!(missions.len(), count);
    }

    // Mission file names follow the same slots (`SAVE1.CAM` …).
    let names: [(MissionCategory, &str, usize); 5] = [
        (MissionCategory::Campaign, "CAM", 9),
        (MissionCategory::Scenario, "SCE", 24),
        (MissionCategory::Training, "TRA", 15),
        (MissionCategory::MultiPlayerScenario, "MPS", 12),
        (MissionCategory::Demo, "DMO", 3),
    ];

    for (category, extension, count) in names {
        for (index, descriptor) in registry.missions(category).iter().enumerate() {
            let expected = format!("SAVE{}.{extension}", index + 1);

            assert_eq!(
                descriptor.mission_name.as_deref(),
                Some(expected.as_str()),
                "{category:?} index {index}"
            );
        }

        assert_eq!(registry.missions(category).len(), count);
    }
}

/// Registry descriptor equals the independently decoded `SC_C0001`.
#[test]
fn campaign_first_descriptor_matches_sc_c0001_decode() {
    let archive = ResArchive::open(data_dir().join("PATCHES.RES")).expect("PATCHES.RES");
    let decoded = xor_decode(archive.read("SC_C0001").expect("SC_C0001"));
    let json: serde_json::Value = serde_json::from_slice(&decoded).expect("SC_C0001 JSON");

    assert_eq!(json["category"], "Campaign");
    assert_eq!(json["mission"], "SAVE1.CAM");
    assert_eq!(json["title"]["text"]["en-US"], "Islands in the Sun");

    let registry = MissionRegistry::from_archive(&archive, data_dir());
    let descriptor = &registry.missions(MissionCategory::Campaign)[0];

    assert_eq!(descriptor.tag, "SC_C0001");
    assert_eq!(descriptor.author, json["author"]);
    assert_eq!(descriptor.copyright, json["copyright"]);
    assert_eq!(descriptor.license, json["license"]);
    assert_eq!(
        descriptor.title_text(),
        json["title"]["text"][DEFAULT_LANGUAGE].as_str().unwrap()
    );
    assert_eq!(
        descriptor.description_text(),
        json["description"]["text"][DEFAULT_LANGUAGE]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        descriptor.hash,
        json["hash"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect::<Vec<String>>()
    );
}

/// `intro`/`victory`/`defeat`, `winloss_conditions` and `game_rules` are decoded;
/// media `id` accepts a plain string as well as an array (`SC_C0001` video).
#[test]
fn story_blocks_and_scripts_are_decoded() {
    let registry = load_registry();
    let descriptor = &registry.missions(MissionCategory::Campaign)[0];

    assert!(descriptor.has_intro());
    assert!(descriptor.has_winloss_conditions());
    assert!(descriptor.has_game_rules());
    assert!(!descriptor.has_victory(), "SC_C0001 has no victory block");
    assert!(!descriptor.has_defeat(), "SC_C0001 has no defeat block");

    let intro = descriptor.intro.as_ref().expect("intro");
    assert!(intro
        .default_text()
        .starts_with("We are the Secret Council of Freedom"));
    assert_eq!(
        intro.background.as_ref().unwrap().resource,
        Some(MediaResource::Ids(vec![
            "ENDGAME1".to_owned(),
            "ENDGAME2".to_owned(),
            "ENDGAME3".to_owned(),
            "ENDGAME4".to_owned(),
            "ENDGAME5".to_owned(),
            "ENDGAME6".to_owned(),
            "ENDGAME7".to_owned(),
            "ENDGAME8".to_owned(),
            "ENDGAME9".to_owned(),
        ]))
    );
    assert_eq!(
        intro.video.get(DEFAULT_LANGUAGE).unwrap().resource,
        Some(MediaResource::Ids(vec!["DEMO2FLC".to_owned()]))
    );

    assert!(descriptor
        .winloss_conditions
        .as_deref()
        .unwrap()
        .contains("MAX_VICTORY_STATE"));
    assert!(descriptor
        .game_rules
        .as_deref()
        .unwrap()
        .contains("max_builder_capability_list"));
}

/// `MissionRegistry::GetMission(category, hash)` finds a descriptor by hash.
#[test]
fn mission_is_found_by_declared_hash() {
    let registry = load_registry();
    let campaign = registry.missions(MissionCategory::Campaign);
    let hash = campaign[0].hash[0].clone();

    let found = registry
        .get_mission(MissionCategory::Campaign, &hash)
        .expect("SC_C0001 by hash");

    assert_eq!(found.tag, "SC_C0001");
    assert_eq!(found.title_text(), "Islands in the Sun");
    assert!(registry
        .get_mission(MissionCategory::Campaign, "not-a-hash")
        .is_none());
}

/// `Custom`/`Hot-seat`/`Multiplayer` have no mission file and no hashes
/// (`mission.cpp:277-291`).
#[test]
fn special_categories_have_no_mission_file() {
    let registry = load_registry();

    for category in [
        MissionCategory::Custom,
        MissionCategory::HotSeat,
        MissionCategory::Multiplayer,
    ] {
        let descriptor = &registry.missions(category)[0];

        assert_eq!(descriptor.category, category);
        assert_eq!(descriptor.mission_name, None);
        assert_eq!(descriptor.mission, None);
        assert!(!descriptor.mission_found);
        assert!(descriptor.hash.is_empty());
        assert_eq!(descriptor.hash_verified, None);
    }
}

// ---------------------------------------------------------------------------
// Mission file resolution and hash warning
// ---------------------------------------------------------------------------

/// Mission files are resolved case-insensitively in the data directory; a
/// missing file leaves the descriptor in place and is a warning.
#[test]
fn mission_files_are_resolved_case_insensitively() {
    let dir = temp_data_dir("resolve");

    // `SAVE2.CAM` exists in lowercase, `SAVE3.CAM` in uppercase, `SAVE4.CAM` in
    // mixed case (the latter two exercise the uppercase/exact/scan fallbacks on
    // a case-sensitive filesystem).
    fs::write(dir.join("save2.cam"), b"stub").unwrap();
    fs::write(dir.join("SAVE3.CAM"), b"stub").unwrap();
    fs::write(dir.join("Save4.Cam"), b"stub").unwrap();

    let registry = MissionRegistry::open(data_dir().join("PATCHES.RES"), &dir).expect("registry");
    let campaign = registry.missions(MissionCategory::Campaign);

    assert_eq!(campaign.len(), 9);
    assert_eq!(
        campaign[0].mission_path().unwrap().file_name().unwrap(),
        "SAVE1.CAM"
    );
    assert!(!campaign[0].mission_found);

    // The first existing name wins with its lowercase form (C++ checks
    // `StringToLowerCase` first); on a case-insensitive filesystem the
    // lowercase candidate also matches the upper/mixed-case files.
    assert_eq!(
        campaign[1].mission_path().unwrap().file_name().unwrap(),
        "save2.cam"
    );
    assert!(campaign[1].mission_found);

    assert!(campaign[2].mission_found);
    assert_eq!(
        fs::canonicalize(campaign[2].mission_path().unwrap()).unwrap(),
        fs::canonicalize(dir.join("SAVE3.CAM")).unwrap()
    );

    assert!(campaign[3].mission_found);
    assert_eq!(
        fs::canonicalize(campaign[3].mission_path().unwrap()).unwrap(),
        fs::canonicalize(dir.join("Save4.Cam")).unwrap()
    );

    // All existing files have the wrong content → hash mismatch warnings only.
    assert_eq!(campaign[1].hash_verified, Some(false));
    assert_eq!(campaign[2].hash_verified, Some(false));
    assert_eq!(campaign[3].hash_verified, Some(false));

    assert!(registry
        .warnings()
        .iter()
        .any(|warning| warning.contains("SAVE1.CAM") && warning.contains("not found")));
    assert!(registry
        .warnings()
        .iter()
        .any(|warning| warning.contains("hash mismatch")));

    fs::remove_dir_all(&dir).unwrap();
}
