//! Mission registry (`PATCHES.RES` `SC_*` descriptors) on real fixtures.
//!
//! Stage 0 gate: `Campaign` has 9 missions, the first is "Islands in the Sun"
//! with `SAVE1.CAM`; `Scenario` 24, `Training` 15, `MultiPlayerScenario` 12.
//!
//! Fixture: `tests/data/PATCHES.RES` (a copy of the C++ port's shared
//! `assets/PATCHES.RES`). No `SAVE*.CAM` files are committed, so mission-file
//! resolution is exercised with a temporary data directory.
//!
//! Tooling (max-brh-docs/development/test-organization.md): the per-category
//! expectations are `#[rstest]` cases, the loader is an rstest `#[fixture]`.
//! The descriptor inventory is pinned with `insta` snapshots: the golden
//! infrastructure (`max-brh-qa/golden/`) holds rendered QA frames and does not
//! cover `PATCHES.RES` descriptors, so a snapshot here is new coverage rather
//! than a duplicate of a golden file (Ar5: insta is only for snapshots that have
//! no golden infrastructure). The snapshot text is portable on purpose — it
//! contains no absolute paths, so it is identical on Windows and Linux.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use insta::assert_snapshot;
use max_assets::mission::{xor_decode, MediaResource, MissionRegistry, DEFAULT_LANGUAGE};
use max_assets::ResArchive;
use max_types::victory::MissionCategory;
use rstest::{fixture, rstest};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

fn load_registry() -> MissionRegistry {
    MissionRegistry::open(data_dir().join("PATCHES.RES"), data_dir())
        .expect("PATCHES.RES must parse")
}

/// rstest fixture: the registry over the committed `PATCHES.RES` fixture.
#[fixture]
fn registry() -> MissionRegistry {
    load_registry()
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
#[rstest]
fn campaign_has_nine_missions_starting_with_islands_in_the_sun(registry: MissionRegistry) {
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
#[rstest]
fn story_blocks_and_scripts_are_decoded(registry: MissionRegistry) {
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
#[rstest]
fn mission_is_found_by_declared_hash(registry: MissionRegistry) {
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
#[rstest]
#[case(MissionCategory::Custom)]
#[case(MissionCategory::HotSeat)]
#[case(MissionCategory::Multiplayer)]
fn special_categories_have_no_mission_file(
    registry: MissionRegistry,
    #[case] category: MissionCategory,
) {
    let descriptor = &registry.missions(category)[0];

    assert_eq!(descriptor.category, category);
    assert_eq!(descriptor.mission_name, None);
    assert_eq!(descriptor.mission, None);
    assert!(!descriptor.mission_found);
    assert!(descriptor.hash.is_empty());
    assert_eq!(descriptor.hash_verified, None);
}

// ---------------------------------------------------------------------------
// Descriptor inventory snapshots (insta — no golden infrastructure for
// PATCHES.RES descriptors; see the module doc)
// ---------------------------------------------------------------------------

/// All resource categories in the C++ id order (`MISSION_CATEGORY_*`).
const CATEGORIES: [MissionCategory; 8] = [
    MissionCategory::Custom,
    MissionCategory::Training,
    MissionCategory::Campaign,
    MissionCategory::HotSeat,
    MissionCategory::Multiplayer,
    MissionCategory::Demo,
    MissionCategory::Scenario,
    MissionCategory::MultiPlayerScenario,
];

/// One portable line per descriptor: resource tag, mission file name, title and
/// the decoded story blocks. No paths — the snapshot must be identical on every
/// platform (`MissionDescriptor::mission` holds an absolute path).
fn descriptor_inventory(registry: &MissionRegistry) -> String {
    let mut report = String::new();

    for category in CATEGORIES {
        let missions = registry.missions(category);
        report.push_str(&format!("{category:?} ({})\n", missions.len()));

        for descriptor in missions {
            let mut blocks = Vec::new();
            if descriptor.has_intro() {
                blocks.push("intro");
            }
            if descriptor.has_victory() {
                blocks.push("victory");
            }
            if descriptor.has_defeat() {
                blocks.push("defeat");
            }
            if descriptor.has_winloss_conditions() {
                blocks.push("winloss");
            }
            if descriptor.has_game_rules() {
                blocks.push("rules");
            }

            report.push_str(&format!(
                "  {} | {} | {} | [{}]\n",
                descriptor.tag,
                descriptor.mission_name.as_deref().unwrap_or("-"),
                descriptor.title_text(),
                blocks.join(","),
            ));
        }
    }

    report
}

/// Stage 0 inventory of all 66 `SC_*` descriptors: category buckets, resource
/// order, mission file names, titles and story-block presence.
///
/// Why insta and not golden: `max-brh-qa/golden/` pins rendered QA frames for
/// `.qa` scenarios; it holds nothing for the `PATCHES.RES` descriptor set, so
/// this snapshot adds coverage instead of duplicating a golden file.
#[test]
fn descriptor_inventory_matches_the_snapshot() {
    let registry = load_registry();

    assert_snapshot!("descriptor_inventory", descriptor_inventory(&registry));
}

/// `SC_C0001` story references: `intro.background`/`intro.video` resource ids,
/// `intro.music`, and the `winloss_conditions`/`game_rules` script heads.
///
/// The same rationale applies as above: these are resource descriptors, not
/// rendered frames, so there is no golden to update.
#[test]
fn campaign_first_story_references_match_the_snapshot() {
    let registry = load_registry();
    let descriptor = &registry.missions(MissionCategory::Campaign)[0];
    let intro = descriptor.intro.as_ref().expect("SC_C0001 intro");

    let media_ids = |media: Option<&max_assets::mission::Media>| -> String {
        match media.and_then(|media| media.resource.as_ref()) {
            Some(MediaResource::Ids(ids)) => ids.join(","),
            Some(MediaResource::Files(files)) => files.join(","),
            None => "-".to_owned(),
        }
    };

    let mut report = String::new();
    report.push_str(&format!(
        "background: {}\n",
        media_ids(intro.background.as_ref())
    ));
    report.push_str(&format!(
        "video: {}\n",
        media_ids(intro.video.get(DEFAULT_LANGUAGE))
    ));
    report.push_str(&format!("music: {}\n", media_ids(intro.music.as_ref())));

    for (name, script) in [
        (
            "winloss_conditions",
            descriptor.winloss_conditions.as_deref(),
        ),
        ("game_rules", descriptor.game_rules.as_deref()),
    ] {
        let script = script.expect("SC_C0001 script");
        let first_line = script.lines().next().unwrap_or("");
        report.push_str(&format!(
            "{name}: {} line(s), first: {first_line}\n",
            script.lines().count()
        ));
    }

    assert_snapshot!("campaign_first_story_references", report);
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
