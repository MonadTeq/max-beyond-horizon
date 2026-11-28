//! Portable game catalog layout (`max-brh-docs/development/distro-installers.md` §B).
//!
//! A distributed build is one directory the player selects. The installer writes
//! the executable, `data/` and `remaster/`; the game itself creates and owns
//! `saves/`, `missions/`, `preferences.ini` and `settings.jsonc` there. Nothing is
//! written to `%APPDATA%`, `$XDG_CONFIG_HOME` or any other system directory.
//!
//! The catalog is resolved once per launch from the executable directory and the
//! data marker (`max-original/` in a development checkout, `data/` in an installed
//! build) — see [`detect`]. [`AppConfig::with_catalog`] then fixes the data paths
//! and [`AppConfig::game_root`], which the save/load and preferences layers use as
//! the default writable root.
//!
//! A development checkout keeps its historical writable location next to the
//! `MAX.RES` directory when no catalog is known: every path here is a *default*,
//! so library users and QA that pass explicit paths are not affected.

use std::path::{Path, PathBuf};

use crate::assets::{self, DATA_DIR_NAMES};
use crate::AppConfig;

/// Player save directory inside the catalog (C++ `FilePathGamePref`).
pub const SAVES_DIR_NAME: &str = "saves";
/// Mission-category save directory inside the catalog (C++ `FilePathGameData`).
pub const MISSIONS_DIR_NAME: &str = "missions";
/// Environment variable the AppImage runtime sets to the running `.AppImage` path.
pub const APPIMAGE_ENV: &str = "APPIMAGE";

/// The resolved portable catalog: the game root and its data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    root: PathBuf,
    data_dir: PathBuf,
    marker: Option<&'static str>,
}

impl Catalog {
    /// Builds the catalog from the resolved data directory.
    ///
    /// When the last path component is one of [`DATA_DIR_NAMES`] the catalog root
    /// is its parent (`<install-dir>/data` → `<install-dir>`); otherwise the data
    /// directory itself is the catalog (a flat development layout).
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        let marker = data_dir
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| {
                DATA_DIR_NAMES
                    .iter()
                    .copied()
                    .find(|marker| *marker == name)
            });
        let root = match marker {
            Some(_) => data_dir
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| data_dir.clone()),
            None => data_dir.clone(),
        };

        Self {
            root,
            data_dir,
            marker,
        }
    }

    /// The game catalog (`<install-dir>` in the portable layout).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory with `MAX.RES`, `PATCHES.RES`, `COLOR.PAL` and the game media.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Which marker directory was found (`max-original`/`data`), if any.
    pub fn marker(&self) -> Option<&'static str> {
        self.marker
    }

    /// Default `FilePathGamePref` directory: `<catalog>/saves`.
    pub fn saves_dir(&self) -> PathBuf {
        self.root.join(SAVES_DIR_NAME)
    }

    /// Default `FilePathGameData` directory: `<catalog>/missions`.
    pub fn missions_dir(&self) -> PathBuf {
        self.root.join(MISSIONS_DIR_NAME)
    }

    /// Default `preferences.ini` path: `<catalog>/preferences.ini`.
    pub fn preferences_path(&self) -> PathBuf {
        self.root.join("preferences.ini")
    }

    /// Default `settings.jsonc` path: `<catalog>/settings.jsonc`.
    pub fn settings_path(&self) -> PathBuf {
        self.root.join(crate::settings::SETTINGS_FILE_NAME)
    }

    /// Default HD remaster root: `<catalog>/remaster`.
    pub fn remaster_dir(&self) -> PathBuf {
        self.root.join(crate::hd::REMASTER_DIR)
    }
}

/// Detects the catalog for a launch from `exe` with the given working directory.
///
/// The executable location is authoritative: the portable install is found by its
/// `data/` marker, the development checkout by `max-original/`, even when the
/// process was started from a directory that happens to contain a game data set.
/// Only when nothing is found next to the executable does the configured path /
/// working directory decide (a library-style launch), which keeps the historical
/// behavior. Returns [`None`] when no `MAX.RES` exists (no writable catalog is
/// known, so the historical defaults stay in effect).
///
/// A launch from an AppImage ([`APPIMAGE_ENV`]) keeps the data of the mounted
/// AppDir but writes next to the `.AppImage` file: the mount is read-only.
pub fn detect(config: &AppConfig, exe: Option<&Path>, cwd: Option<&Path>) -> Option<Catalog> {
    detect_with_appimage(config, exe, cwd, appimage_game_root())
}

/// [`detect`] with an explicit AppImage root — unit tests do not mutate the
/// process environment.
fn detect_with_appimage(
    config: &AppConfig,
    exe: Option<&Path>,
    cwd: Option<&Path>,
    appimage_root: Option<PathBuf>,
) -> Option<Catalog> {
    let exe_roots = exe.map(assets::exe_search_roots).unwrap_or_default();

    let catalog = if let Some(max_res) = assets::find_max_res_in(&exe_roots) {
        catalog_from(&max_res, cwd)
    } else {
        let roots = assets::data_search_roots_from(config, cwd, exe);
        let (max_res, _) = assets::resolve_max_res_at(config, &roots);

        if !max_res.is_file() {
            return None;
        }

        catalog_from(&max_res, cwd)
    }?;

    // The AppImage runtime mounts the AppDir read-only (squashfs), so the catalog
    // root becomes the directory of the `.AppImage` file itself: saves, missions,
    // `preferences.ini` and `settings.jsonc` are written next to it
    // (`distro-installers.md` §D).
    Some(match appimage_root {
        Some(root) => Catalog { root, ..catalog },
        None => catalog,
    })
}

/// The directory that holds the running `.AppImage` (`APPIMAGE`, set by the
/// runtime), or `None` for a regular launch.
fn appimage_game_root() -> Option<PathBuf> {
    let path = std::env::var_os(APPIMAGE_ENV)?;
    let path = Path::new(&path);

    // A bare file name has an empty parent: there is no directory to write to.
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

/// Builds the catalog from the resolved `MAX.RES`; a relative path (a development
/// checkout started from the repository root) is anchored to `cwd`, so the
/// writable root does not depend on the process-wide working directory later.
fn catalog_from(max_res: &Path, cwd: Option<&Path>) -> Option<Catalog> {
    let data_dir = max_res.parent()?;
    let data_dir = if data_dir.is_absolute() {
        data_dir.to_path_buf()
    } else {
        cwd?.join(data_dir)
    };

    Some(Catalog::new(data_dir))
}

impl AppConfig {
    /// Applies the portable catalog: `MAX.RES`/`COLOR.PAL` come from its data
    /// directory, and [`Self::game_root`] becomes the default writable root for
    /// saves, preferences and the remaster.
    pub fn with_catalog(mut self, catalog: &Catalog) -> Self {
        self = self.with_data_dir(catalog.data_dir());

        self.game_root = Some(catalog.root().to_path_buf());
        self
    }

    /// Records the catalog root without touching the configured data paths
    /// (used when `--data-dir`/`--max-res` were given explicitly).
    pub fn with_game_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.game_root = Some(root.into());
        self
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
