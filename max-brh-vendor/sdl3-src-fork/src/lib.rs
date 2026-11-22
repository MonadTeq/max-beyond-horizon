#![no_std]

/// Location of the SDL 3 source code.
///
/// VENDORED FORK of `sdl3-src`: unlike the crates.io crate (which exposes the
/// SDL source vendored inside the crate at `CARGO_MANIFEST_DIR/SDL`), this
/// fork points at the git submodule `max-brh-vendor/SDL` (the MonadTeq fork
/// of SDL 3.5.0, commit `c8da179`). The fork lives at
/// `max-brh-vendor/sdl3-src-fork/`, so `../SDL` resolves to
/// `max-brh-vendor/SDL`. See `max-brh-vendor/README.md`.
#[cfg(not(windows))]
pub const SOURCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../SDL");
#[cfg(windows)]
pub const SOURCE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "\\..\\SDL");

/// Revision
pub const REVISION: &str = "SDL-3.5.0";

/// Version part of the revision
pub const VERSION: &str = "3.5.0";

/// Tag part of the revision
pub const REVISION_TAG: &str = "main";

/// Tag part of the revision without version
pub const REVISION_TAG_BASE: &str = "main";

/// Offset from tag part of the revision
pub const REVISION_OFFSET: &str = "0";

/// Hash part of the revision
pub const REVISION_HASH: &str = "gc8da179";
