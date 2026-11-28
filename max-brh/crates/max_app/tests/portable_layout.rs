//! G2 staging smoke (`max-brh-docs/development/distro-installers.md` §F).
//!
//! The installer layout is a single catalog the player selects:
//!
//! ```text
//! <install-dir>/
//!   game.exe
//!   data/        MAX.RES, PATCHES.RES, COLOR.PAL
//!   remaster/    HD package
//!   saves/       player saves (created by the game)
//!   settings.jsonc
//! ```
//!
//! The test builds that layout, resolves the launch exactly like `main.rs` does
//! ([`max_app::layout::detect`] + [`max_app::AppConfig::with_catalog`]) and checks
//! that the game finds its data by the `data` marker, reads `settings.jsonc` from
//! the catalog and writes saves/preferences **inside** it — also when the process
//! was started from a foreign working directory.

use std::path::{Path, PathBuf};

use max_app::settings::Settings;
use max_app::{App, AppConfig};
use max_core::MissionCategory;
use max_render::RenderScale;

/// Path inside the repository (`max-brh/crates/max_app/../../..`).
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// Unique temporary directory of the test.
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "max_app_portable_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The installer copies the game data byte-for-byte; a hard link keeps the test
/// fast when both paths share a volume, otherwise a plain copy is made.
fn link_or_copy(src: &Path, dst: &Path) {
    assert!(
        src.is_file(),
        "required test fixture is missing: {}",
        src.display()
    );

    if std::fs::hard_link(src, dst).is_err() {
        std::fs::copy(src, dst).expect("copy game data");
    }
}

/// Staging catalog: `game.exe`, `data/`, `remaster/`, `saves/`.
struct Staging {
    root: PathBuf,
    data: PathBuf,
    saves: PathBuf,
    remaster: PathBuf,
    exe: PathBuf,
}

fn staging(tag: &str) -> Staging {
    let root = temp_dir(tag);
    let data = root.join("data");
    let saves = root.join("saves");
    let remaster = root.join("remaster");
    for dir in [&data, &saves, &remaster] {
        std::fs::create_dir_all(dir).expect("staging dir");
    }

    // The package payload: `MAX.RES`/`COLOR.PAL` from the Interplay oracle
    // (read-only source, copied into the catalog), `PATCHES.RES` from the
    // repository test fixture — exactly the files §B lists under `data/`.
    link_or_copy(&repo_path("max-original/MAX.RES"), &data.join("MAX.RES"));
    link_or_copy(
        &repo_path("max-original/COLOR.PAL"),
        &data.join("COLOR.PAL"),
    );
    link_or_copy(
        &repo_path("tests/fixtures/assets/PATCHES.RES"),
        &data.join("PATCHES.RES"),
    );
    // `*.WRL` maps are part of the package (§B): Snowcrab is the default planet,
    // Islandia is the planet of the V70 save fixture below.
    for map in ["SNOW_1.WRL", "GREEN_5.WRL"] {
        link_or_copy(&repo_path(&format!("max-original/{map}")), &data.join(map));
    }

    let exe = root.join("game.exe");
    std::fs::write(&exe, b"staging game.exe").expect("game.exe");

    Staging {
        root,
        data,
        saves,
        remaster,
        exe,
    }
}

/// G2: a launch from the staging catalog reads the data by the `data` marker,
/// reads `settings.jsonc` from the catalog and writes the player files inside it.
#[test]
fn staging_catalog_reads_data_and_writes_player_files_inside() {
    let staging = staging("smoke");
    let cwd = staging.root.clone();
    std::fs::write(
        staging.root.join("settings.jsonc"),
        r#"{ "display": { "scale": 2 } }"#,
    )
    .expect("settings.jsonc");

    // Launch resolution (the same calls `main.rs` makes: the default config has
    // CWD-relative `max-original/` paths that do not exist here).
    let config = AppConfig::default();
    let catalog = max_app::layout::detect(&config, Some(&staging.exe), Some(&cwd))
        .expect("the data marker must be found from the executable");

    assert_eq!(catalog.data_dir(), staging.data, "data/ найден по маркеру");
    assert_eq!(
        catalog.root(),
        staging.root,
        "каталог игры — родитель data/"
    );
    assert_eq!(catalog.marker(), Some("data"));

    let config = config.with_catalog(&catalog);

    assert_eq!(config.max_res, staging.data.join("MAX.RES"));
    assert_eq!(config.color_pal, staging.data.join("COLOR.PAL"));

    // The game actually reads the staged data: assets load from `data/` and
    // `PATCHES.RES` is found next to `MAX.RES`.
    let (patches, _) = max_app::resolve_patches_res(&config);
    assert_eq!(patches, staging.data.join("PATCHES.RES"));

    let mut app = App::new(config);
    app.init_assets()
        .expect("the staged game data must be readable by the game");
    assert!(app.assets().is_some(), "ассеты загружены из каталога");

    // Saves: a real V70 state is written into `<catalog>/saves/`.
    app.load_v70_path(repo_path("max-brh/crates/max_app/tests/data/save1_v70.cam"))
        .expect("V70 fixture loads");
    app.universe_mut()
        .set_mission_category(MissionCategory::Custom);

    assert_eq!(app.save_dir(), staging.saves);
    assert_eq!(
        app.save_data_dir(),
        staging.root.join("missions"),
        "FilePathGameData — внутри каталога"
    );
    assert_eq!(
        app.preferences_path(),
        staging.root.join("preferences.ini"),
        "preferences.ini — в корне каталога (§B)"
    );

    let saved = app
        .save_game_to_slot(1, "Staging")
        .expect("save into the catalog");
    assert_eq!(saved, staging.saves.join("save1.dta"));
    assert!(saved.is_file());

    app.persist_preferences();
    assert!(
        staging.root.join("preferences.ini").is_file(),
        "preferences.ini записан в каталог игры"
    );

    // settings.jsonc is read from the catalog.
    let settings_path =
        max_app::settings::resolve_path(Some(&staging.exe), Some(catalog.root()), Some(&cwd), None);
    assert_eq!(settings_path, staging.root.join("settings.jsonc"));
    let settings = Settings::load(&settings_path)
        .expect("settings.jsonc")
        .expect("settings.jsonc exists");
    assert_eq!(settings.render_scale(), RenderScale::Hd2);

    // The HD package is next to the executable, not under a repository path.
    assert_eq!(
        max_app::default_remaster_root(Some(catalog.root())),
        staging.remaster
    );

    // No player file leaves the catalog (no `%APPDATA%`/`$XDG`, no CWD copy).
    for path in [&saved, &app.preferences_path()] {
        assert!(
            path.starts_with(&staging.root),
            "{} must stay inside the game catalog",
            path.display()
        );

        for variable in [
            "APPDATA",
            "LOCALAPPDATA",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
        ] {
            if let Some(dir) = std::env::var_os(variable) {
                assert!(
                    !path.starts_with(PathBuf::from(dir)),
                    "{} must not use {}",
                    path.display(),
                    variable
                );
            }
        }
    }
    assert!(!staging.data.join("save1.dta").exists());

    std::fs::remove_dir_all(staging.root).ok();
}

/// G2 portability: launching the installed catalog **from another working
/// directory** must not pick the foreign data or the foreign `settings.jsonc`.
#[test]
fn installed_launch_ignores_a_foreign_working_directory() {
    let staging = staging("foreign");
    std::fs::write(
        staging.root.join("settings.jsonc"),
        r#"{ "display": { "scale": 2 } }"#,
    )
    .expect("settings.jsonc");

    let cwd = temp_dir("foreign-cwd");
    // A decoy checkout in the working directory: the exe location must win.
    std::fs::create_dir_all(cwd.join("max-original")).expect("decoy data");
    std::fs::write(cwd.join("max-original/MAX.RES"), b"RES0decoy").expect("decoy MAX.RES");
    std::fs::write(
        cwd.join("settings.jsonc"),
        r#"{ "display": { "scale": 4 } }"#,
    )
    .expect("decoy settings.jsonc");

    let config = AppConfig::default();
    let catalog = max_app::layout::detect(&config, Some(&staging.exe), Some(&cwd))
        .expect("the installed catalog is found from the executable");

    assert_eq!(
        catalog.data_dir(),
        staging.data,
        "данные установленной игры, а не CWD-макет"
    );
    assert_eq!(catalog.root(), staging.root);

    let app = App::new(config.with_catalog(&catalog));

    assert_eq!(app.save_dir(), staging.saves);
    assert_eq!(app.save_data_dir(), staging.root.join("missions"));
    assert_eq!(app.preferences_path(), staging.root.join("preferences.ini"));

    let settings_path =
        max_app::settings::resolve_path(Some(&staging.exe), Some(catalog.root()), Some(&cwd), None);
    assert_eq!(
        settings_path,
        staging.root.join("settings.jsonc"),
        "settings.jsonc установленной игры, а не рабочий каталог"
    );

    std::fs::remove_dir_all(staging.root).ok();
    std::fs::remove_dir_all(cwd).ok();
}
