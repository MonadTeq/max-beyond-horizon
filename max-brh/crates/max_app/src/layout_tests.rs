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
