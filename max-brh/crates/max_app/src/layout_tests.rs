//! Unit tests of the portable catalog layout (`max_app::layout`).

use super::*;

/// `<install-dir>/data` → the catalog is its parent.
#[test]
fn catalog_root_is_the_parent_of_a_marker_directory() {
    let catalog = Catalog::new(Path::new("D:/Games/MAX-BRH/data"));

    assert_eq!(catalog.root(), Path::new("D:/Games/MAX-BRH"));
    assert_eq!(catalog.data_dir(), Path::new("D:/Games/MAX-BRH/data"));
    assert_eq!(catalog.marker(), Some("data"));
}

/// A development checkout keeps `max-original/` as the marker.
#[test]
fn development_marker_keeps_the_repository_as_the_catalog() {
    let catalog = Catalog::new(Path::new("D:/dev/max-port-debug/max-original"));

    assert_eq!(catalog.root(), Path::new("D:/dev/max-port-debug"));
    assert_eq!(catalog.marker(), Some("max-original"));
}

/// A flat layout (`MAX.RES` directly in the game directory) is its own catalog.
#[test]
fn a_flat_data_directory_is_its_own_catalog() {
    let catalog = Catalog::new(Path::new("D:/Games/MAX-BRH"));

    assert_eq!(catalog.root(), Path::new("D:/Games/MAX-BRH"));
    assert_eq!(catalog.marker(), None);
}

/// Every writable default is inside the catalog (`distro-installers.md` §B).
#[test]
fn writable_defaults_live_inside_the_catalog() {
    let catalog = Catalog::new(Path::new("D:/Games/MAX-BRH/data"));

    assert_eq!(catalog.saves_dir(), Path::new("D:/Games/MAX-BRH/saves"));
    assert_eq!(
        catalog.missions_dir(),
        Path::new("D:/Games/MAX-BRH/missions")
    );
    assert_eq!(
        catalog.preferences_path(),
        Path::new("D:/Games/MAX-BRH/preferences.ini")
    );
    assert_eq!(
        catalog.settings_path(),
        Path::new("D:/Games/MAX-BRH/settings.jsonc")
    );
    assert_eq!(
        catalog.remaster_dir(),
        Path::new("D:/Games/MAX-BRH/remaster")
    );

    for path in [
        catalog.saves_dir(),
        catalog.missions_dir(),
        catalog.preferences_path(),
        catalog.settings_path(),
        catalog.remaster_dir(),
    ] {
        assert!(
            path.starts_with(catalog.root()),
            "{} must stay inside the catalog",
            path.display()
        );
    }
}

/// Unique temporary directory (the layout tests may run in parallel).
fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "max_app_layout_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));

    std::fs::create_dir_all(&dir).expect("temp dir");

    dir
}

/// An AppImage mount is read-only: the catalog root is the directory that holds
/// the `.AppImage` file, while the data stays inside the mounted AppDir
/// (`distro-installers.md` §D — saves are written next to the `.AppImage`).
#[test]
fn appimage_writes_beside_the_file_and_reads_the_mounted_data() {
    let dir = temp_dir("appimage");
    // The mounted AppDir: `usr/bin/max-brh` + `data/MAX.RES`.
    let appdir = dir.join("AppDir");
    let exe = appdir.join("usr/bin/max-brh");
    std::fs::create_dir_all(exe.parent().expect("exe dir")).expect("exe dir");
    std::fs::create_dir_all(appdir.join("data")).expect("data dir");
    std::fs::write(appdir.join("data/MAX.RES"), b"RES0").expect("write MAX.RES");
    // The player's directory: the `.AppImage` file and a foreign CWD.
    let appimage = dir.join("MAX-BRH-0.1.28a-x86_64.AppImage");
    std::fs::write(&appimage, b"").expect("write AppImage");
    let cwd = dir.join("foreign-cwd");
    std::fs::create_dir_all(&cwd).expect("cwd");

    let catalog = detect_with_appimage(
        &AppConfig::default(),
        Some(&exe),
        Some(&cwd),
        appimage.parent().map(Path::to_path_buf),
    )
    .expect("catalog");

    assert_eq!(catalog.data_dir(), appdir.join("data"), "data stays in the mount");
    assert_eq!(catalog.root(), dir, "the writable root is the .AppImage directory");
    assert_eq!(catalog.marker(), Some("data"));
    assert_eq!(catalog.saves_dir(), dir.join("saves"));
    assert_eq!(catalog.preferences_path(), dir.join("preferences.ini"));
    assert!(
        !catalog.root().starts_with(&appdir),
        "the AppDir mount itself must never be the writable root"
    );

    std::fs::remove_dir_all(dir).ok();
}

/// Without the AppImage marker the same tree resolves to the AppDir itself — the
/// regression this fix exists for.
#[test]
fn without_an_appimage_the_appdir_is_the_catalog_root() {
    let dir = temp_dir("plain");
    let appdir = dir.join("AppDir");
    let exe = appdir.join("usr/bin/max-brh");
    std::fs::create_dir_all(exe.parent().expect("exe dir")).expect("exe dir");
    std::fs::create_dir_all(appdir.join("data")).expect("data dir");
    std::fs::write(appdir.join("data/MAX.RES"), b"RES0").expect("write MAX.RES");

    let catalog = detect_with_appimage(&AppConfig::default(), Some(&exe), Some(&dir), None)
        .expect("catalog");

    assert_eq!(catalog.root(), appdir);
    assert_eq!(catalog.data_dir(), appdir.join("data"));

    std::fs::remove_dir_all(dir).ok();
}
