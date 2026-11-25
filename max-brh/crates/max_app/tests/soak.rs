//! P4: integration test of the headless soak run (`max_app::run_soak`).
//!
//! A short run (6 turns, 2 computers) checks the same as the CI soak stage
//! at 120 turns, but fast: the match reaches the end, the metrics are filled,
//! the profile sees every frame, and two identical runs give one digest
//! (profiling and interpolation optimization do not change the simulation).
//!
//! The test requires the game data (`max-original/`); a missing data set fails the test.

use std::path::{Path, PathBuf};

use max_app::{run_soak, App, AppConfig, SoakOptions};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn config() -> AppConfig {
    AppConfig::default()
        .with_data_dir(repo_root().join("max-original"))
        .with_assets_dir(repo_root().join("tests/fixtures/assets"))
        .with_autosave(false)
}

/// `profile` — process counters (as intended for the soak log), so
/// the tests that reset and read them run one at a time.
fn soak_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn short_headless_soak_completes_and_is_deterministic() {
    let _guard = soak_lock();

    assert!(repo_root().join("max-original/MAX.RES").is_file(), concat!("required test fixture is missing: ", stringify!(!repo_root().join("max-original/MAX.RES").is_file())));

    let options = SoakOptions {
        turns: 6,
        ai_teams: 2,
        opponent: 0,
        ..SoakOptions::default()
    };

    let mut left = App::new(config());
    let left_report = run_soak(&mut left, &options).expect("первый soak");
    let left_digest = left.digest();

    let mut right = App::new(config());
    let right_report = run_soak(&mut right, &options).expect("второй soak");
    let right_digest = right.digest();

    assert_eq!(
        left_report.turns_played, options.turns,
        "партия должна дойти до запрошенного хода: {}",
        left_report.log()
    );
    assert!(left_report.passed(), "{}", left_report.log());
    assert!(!left_report.finished_early, "{}", left_report.log());
    assert_eq!(left_digest, right_digest, "soak недетерминирован");
    assert_eq!(left_report.frames, right_report.frames);
    assert_eq!(left_report.turns.len(), 6);
    assert!(left_report.frames > 0, "ходы ИИ прокручивают кадры");
    assert_eq!(
        left_report.profile.local_update.count, left_report.frames,
        "профиль обязан видеть каждый кадр"
    );
    assert!(
        left_report.profile.local_update.avg_ns() > 0,
        "среднее время кадра должно быть измерено"
    );
    assert!(
        left_report.profile.finish_turn.count >= 5,
        "{}",
        left_report.log()
    );

    // Memory: on supported OSes there is a measurement; the growth in a short run is small.
    if cfg!(any(target_os = "linux", windows)) {
        assert!(left_report.mem_start.is_some(), "RSS на старте");
        assert!(left_report.mem_end.is_some(), "RSS в конце");
    }
    assert!(
        left_report.memory_stable(),
        "рост RSS {} КиБ",
        left_report.memory_growth_kb()
    );

    // The log contains the same metrics that go to qa-out/soak-headless/soak.txt.
    let log = left_report.log();
    assert!(log.contains("SOAK RESULT: PASS"), "{log}");
    assert!(log.contains("PROFILE: local_update n="), "{log}");
    assert!(log.contains("MEMORY:"), "{log}");
}

#[test]
fn soak_budget_helper_reports_a_fraction_of_the_frame() {
    let _guard = soak_lock();

    assert!(repo_root().join("max-original/MAX.RES").is_file(), concat!("required test fixture is missing: ", stringify!(!repo_root().join("max-original/MAX.RES").is_file())));

    let options = SoakOptions {
        turns: 2,
        ai_teams: 1,
        human_ticks: 2,
        ..SoakOptions::default()
    };
    let mut app = App::new(config());
    let report = run_soak(&mut app, &options).expect("soak");

    let used = max_app::soak::budget_used_percent(&report);
    assert!(
        used > 0.0 && used < 100.0,
        "средний кадр не должен занимать весь бюджет 60 Гц: {used}%"
    );
}
