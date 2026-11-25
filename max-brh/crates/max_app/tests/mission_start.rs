//! Campaign plan stage 1: mission start by category + index, headless.
//!
//! The flow under test is `menu.cpp:2052-2098` → `game_manager.cpp:1929-1961`
//! without any UI: [`MissionRegistry`] → descriptor → resolved V70 mission file →
//! `App::start_mission_from_registry` → [`App::start_local_game_from_save`].
//!
//! Gates:
//! 1. a registry start is **byte-identical** to a direct
//!    `start_local_game_from_save` of the same file (`snapshot_bytes`);
//! 2. all file-backed mission categories start (Campaign / Scenario / Training /
//!    MultiPlayerScenario), immediately turn-active, with no `CargoMenu`/`SelectSite`
//!    screen (`GAME_STATE_10_LOAD_GAME` is left in one step);
//! 3. `mission_category`/`mission_index` reach `GameState` from the **V70 header**
//!    (never zeroed) and `mission_win_loss` runs the mission script.
//!
//! Fixtures: `PATCHES.RES` from the stage-0 registry tests. Only `SAVE1.CAM`
//! (campaign 1) and `SAVE18.SCE` (scenario 18) are committed, so the training and
//! multiplayer-scenario files are the campaign bytes with a re-labelled V70 header
//! (`save_game_type`/`mission_index` are the only fields the start flow reads from
//! the header); the terrain is irrelevant here.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use max_app::local::{LocalPhase, LocalSetup};
use max_app::{App, AppConfig};
use max_assets::MissionRegistry;
use max_core::enums::{FileType, GameStateType, WinLossState};
use max_core::team::TeamId;
use max_core::victory::MissionCategory;

/// `max-original/SAVE1.CAM` — campaign 1, the registry's `SC_C0001`.
const SAVE1_CAMPAIGN: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE18.SCE` — scenario 18, the registry's `SC_S0018`.
const SAVE18_SCENARIO: &[u8] = include_bytes!("../../max_core/tests/data/save18_v70.sce");

/// V70 header layout (`max_core/tests/phase4_save.rs`).
const OFF_FILE_TYPE: usize = 2;
const OFF_MISSION_INDEX: usize = 34;

fn patches_res() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../max_assets/tests/data/PATCHES.RES")
}

/// A copy of `source` with the V70 header declaring another mission.
fn reheader(source: &[u8], file_type: FileType, mission_index: u16) -> Vec<u8> {
    let mut bytes = source.to_vec();
    bytes[OFF_FILE_TYPE] = file_type as u8;
    bytes[OFF_MISSION_INDEX..OFF_MISSION_INDEX + 2].copy_from_slice(&mission_index.to_le_bytes());
    bytes
}

/// Temporary mission data directory with the four mission files under their
/// registry names.
fn mission_data_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let dir = std::env::temp_dir().join(format!(
        "max_app_mission_{name}_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    std::fs::create_dir_all(&dir).expect("create temp data dir");
    std::fs::write(dir.join("SAVE1.CAM"), SAVE1_CAMPAIGN).expect("SAVE1.CAM");
    std::fs::write(dir.join("SAVE18.SCE"), SAVE18_SCENARIO).expect("SAVE18.SCE");
    // Training (`FileType::Tutorial` = 1) and multiplayer scenario
    // (`FileType::MultiScenario` = 9): see the module header.
    std::fs::write(
        dir.join("SAVE1.TRA"),
        reheader(SAVE1_CAMPAIGN, FileType::Tutorial, 1),
    )
    .expect("SAVE1.TRA");
    std::fs::write(
        dir.join("SAVE1.MPS"),
        reheader(SAVE1_CAMPAIGN, FileType::MultiScenario, 1),
    )
    .expect("SAVE1.MPS");

    dir
}

fn load_registry(dir: &Path) -> MissionRegistry {
    MissionRegistry::open(patches_res(), dir).expect("PATCHES.RES parses")
}

/// Headless application without game assets: the start flow reads only the save.
fn app() -> App {
    App::new(AppConfig::default())
}

fn mission_setup() -> LocalSetup {
    LocalSetup::default()
}

/// A started mission is immediately turn-active: `GAME_STATE_10_LOAD_GAME` must not
/// leave the application in a menu (`GameManager_GameLoop` branches straight into
/// `GAME_STATE_11_TURN_ACTIVE`, `game_manager.cpp:1951`).
fn assert_headless_turn_active(app: &App, context: &str) {
    assert!(app.local_active(), "{context}: no local match");
    assert!(
        app.build_menu().is_none(),
        "{context}: CargoMenu/build menu opened"
    );
    assert!(app.build_site().is_none(), "{context}: SelectSite opened");
    assert_eq!(
        app.state_ref().game_state(),
        GameStateType::InGame,
        "{context}: the game state is not in-game"
    );

    let phase = app.local().expect("local match").phase;
    assert!(
        matches!(phase, LocalPhase::PlayerTurn | LocalPhase::AiTurn(_)),
        "{context}: the match is not turn-active ({phase:?})"
    );
}

// ---------------------------------------------------------------------------
// Gate: registry start == direct V70 load, all four mission categories
// ---------------------------------------------------------------------------

/// `(category, registry index, mission file, expected header mission_index)`.
const MISSIONS: [(MissionCategory, usize, &str, u16); 4] = [
    (MissionCategory::Campaign, 0, "SAVE1.CAM", 1),
    (MissionCategory::Scenario, 17, "SAVE18.SCE", 18),
    (MissionCategory::Training, 0, "SAVE1.TRA", 1),
    (MissionCategory::MultiPlayerScenario, 0, "SAVE1.MPS", 1),
];

/// Every file-backed mission category starts through the registry, comes up
/// turn-active, and its `snapshot_bytes` equals a direct `start_local_game_from_save`
/// of the same bytes — the stage-1 equivalence gate.
#[test]
fn registry_start_equals_a_direct_save_load_for_all_mission_categories() {
    let dir = mission_data_dir("gate");
    let registry = load_registry(&dir);
    let setup = mission_setup();

    for (category, index, file, header_index) in MISSIONS {
        let descriptor = registry
            .missions(category)
            .get(index)
            .unwrap_or_else(|| panic!("{category:?} index {index}"));
        let context = format!("{category:?}/{index}");

        assert_eq!(descriptor.category, category, "{context}");
        assert_eq!(
            descriptor.mission_name.as_deref(),
            Some(file),
            "{context}: resolved mission name"
        );
        assert!(
            descriptor.mission_found,
            "{context}: mission file is missing"
        );

        let mut from_registry = app();
        let start = from_registry
            .start_mission_from_registry(&registry, category, index, &setup)
            .unwrap_or_else(|error| panic!("{context}: {error}"));

        assert_eq!(start.category, category, "{context}: report category");
        assert_eq!(start.mission_index, header_index, "{context}: report index");
        assert_eq!(start.title, descriptor.title_text(), "{context}: title");
        assert!(
            start
                .path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(file)),
            "{context}: {:?}",
            start.path
        );

        // The mission identity reached `GameState` from the V70 header.
        assert_eq!(
            from_registry.state_ref().mission_category(),
            category,
            "{context}: GameState mission_category"
        );
        assert_eq!(
            from_registry.state_ref().mission_index(),
            header_index,
            "{context}: GameState mission_index"
        );
        assert_ne!(
            from_registry.state_ref().mission_index(),
            0,
            "{context}: mission_index must not be zeroed"
        );
        assert_headless_turn_active(&from_registry, &context);

        // Baseline: the same bytes through the public save loader.
        let bytes = std::fs::read(dir.join(file)).expect("mission file");
        let mut direct = app();
        direct
            .start_local_game_from_save(&setup, &bytes)
            .unwrap_or_else(|error| panic!("{context}: direct load: {error}"));

        assert_eq!(
            from_registry.snapshot(),
            direct.snapshot(),
            "{context}: registry start must equal a direct V70 load byte for byte"
        );
        assert_eq!(
            from_registry.state_ref().evaluate_status(),
            direct.state_ref().evaluate_status(),
            "{context}: win/loss status"
        );

        // The loaded mission is the one the win/loss evaluator sees.
        assert_eq!(
            from_registry.state_ref().evaluate_status().mission_type,
            category as i32,
            "{context}: mission_type"
        );

        // The start is not only equal at turn zero: five ticks of the loaded mission
        // stay byte-identical too (`campaign-missions.md` stage 1 gate,
        // "SAVE1.CAM 5 ticks determinism").
        for _ in 0..5 {
            from_registry.tick();
            direct.tick();
        }

        assert_eq!(
            from_registry.snapshot(),
            direct.snapshot(),
            "{context}: 5 ticks of the mission diverge from the direct load"
        );
        assert_eq!(
            from_registry.digest(),
            direct.digest(),
            "{context}: digest after 5 ticks"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// mission_win_loss on a real mission
// ---------------------------------------------------------------------------

/// Campaign 1 keeps its own win/loss script (`SC_C0001`): RED is `Pending` while
/// GREEN has a research centre and an eco sphere. The generic survival rule would
/// return `Generic` here, so the value proves `mission_win_loss` selected the
/// campaign branch instead of `Default`.
#[test]
fn campaign_mission_runs_its_own_win_loss_rule() {
    let dir = mission_data_dir("winloss");
    let registry = load_registry(&dir);
    let mut app = app();

    app.start_mission_from_registry(&registry, MissionCategory::Campaign, 0, &mission_setup())
        .expect("campaign 1 starts");

    let status = app.state_ref().evaluate_status();

    assert_eq!(status.mission_type, MissionCategory::Campaign as i32);
    assert_eq!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Pending,
        "SC_C0001: GREEN keeps its research centre and eco sphere"
    );
    assert_ne!(
        status.team_status[TeamId::RED.0 as usize],
        WinLossState::Generic,
        "the mission script must win over the generic rules"
    );

    // A re-save keeps the mission identity: `SaveLoad_Save` writes the mission
    // category into the V70 header (`saveload.cpp:391-396`) and the 1-based index
    // (`saveload.cpp:772`), so the copy stays a campaign save instead of a custom one.
    let bytes = app.save_v70_bytes("Campaign 1").expect("V70 write");
    let save = max_core::save::SaveFile::parse(&bytes).expect("the re-saved V70 parses");

    assert_eq!(save.header.save_game_type, FileType::Campaign);
    assert_eq!(save.header.mission_index, 1);

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// mission_index comes from the header, not from the registry index
// ---------------------------------------------------------------------------

/// The 1-based `mission_index` of the save header reaches `GameState` unchanged —
/// the registry index is only the lookup key (`saveload.cpp:772`, guardrail
/// "mission_index is not overwritten with 0").
#[test]
fn mission_index_is_taken_from_the_save_header() {
    let dir = mission_data_dir("header_index");
    // A header index that the registry index (0 → 1) does not predict.
    std::fs::write(
        dir.join("SAVE1.MPS"),
        reheader(SAVE1_CAMPAIGN, FileType::MultiScenario, 7),
    )
    .expect("SAVE1.MPS");
    let registry = load_registry(&dir);
    let mut app = app();

    let start = app
        .start_mission_from_registry(
            &registry,
            MissionCategory::MultiPlayerScenario,
            0,
            &mission_setup(),
        )
        .expect("MPS mission 1 starts");

    assert_eq!(start.mission_index, 7, "the header value, not index+1");
    assert_eq!(app.state_ref().mission_index(), 7);
    assert_eq!(
        app.state_ref().mission_category(),
        MissionCategory::MultiPlayerScenario
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Errors of the start flow
// ---------------------------------------------------------------------------

/// A wrong index, a category without a mission file, and a missing mission file are
/// reported instead of silently starting something else.
#[test]
fn mission_start_reports_invalid_selections() {
    let dir = mission_data_dir("errors");
    let registry = load_registry(&dir);
    let setup = mission_setup();
    let mut app = app();

    // Out of range (`game_file_number` beyond the category).
    let error = app
        .start_mission_from_registry(&registry, MissionCategory::Campaign, 9, &setup)
        .expect_err("no 10th campaign mission");
    assert!(error.contains("index 9"), "unexpected error: {error}");

    // `Custom`/`Hot-seat`/`Multiplayer` have no mission file
    // (`Mission_ProcessMission`, `mission.cpp:272-291`).
    for category in [
        MissionCategory::Custom,
        MissionCategory::HotSeat,
        MissionCategory::Multiplayer,
    ] {
        let descriptor = &registry.missions(category)[0];
        let error = app
            .start_mission_from_descriptor(descriptor, &setup)
            .expect_err("no mission file");
        assert!(
            error.contains("no mission file"),
            "{category:?}: unexpected error: {error}"
        );
    }

    // A descriptor whose file is absent: the path is reported, the start fails.
    std::fs::remove_file(dir.join("SAVE1.TRA")).expect("remove SAVE1.TRA");
    let registry = load_registry(&dir);
    let descriptor = &registry.missions(MissionCategory::Training)[0];
    let error = app
        .start_mission_from_descriptor(descriptor, &setup)
        .expect_err("missing mission file");
    assert!(error.contains("SAVE1.TRA"), "unexpected error: {error}");
    assert!(!app.local_active(), "a failed start must not leave a match");

    std::fs::remove_dir_all(&dir).ok();
}
