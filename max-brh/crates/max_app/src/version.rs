//! Product version of the client.
//!
//! The workspace `Cargo.toml` is the single source: Cargo requires a semver
//! version, so the pre-alpha build is `0.1.28-alpha`. The owner's displayed form
//! is `0.1.28a` (the hyphen is dropped and the pre-release tag is shortened to its
//! first letter); it is shown in the window title and in the startup log. The unit
//! test below keeps [`DISPLAY_VERSION`] in sync with the manifest version.

/// Semver version passed to Cargo (`0.1.28-alpha`).
pub const SEMVER: &str = env!("CARGO_PKG_VERSION");

/// Displayed version of the client (`0.1.28a`).
pub const DISPLAY_VERSION: &str = "0.1.28a";

/// Displayed form of a semver version: drop the hyphen before the pre-release tag
/// and keep its first letter (`0.1.28-alpha` → `0.1.28a`). A version without a
/// pre-release tag stays unchanged. Test-only: [`DISPLAY_VERSION`] is the constant
/// the binary uses, and the test below keeps it in sync with [`SEMVER`].
#[cfg(test)]
fn display_of(semver: &str) -> String {
    match semver.split_once('-') {
        Some((base, pre)) if !pre.is_empty() => {
            let tag = pre.chars().next().unwrap_or_default();

            format!("{base}{tag}")
        }
        _ => semver.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displayed_version_matches_the_cargo_version() {
        assert_eq!(
            DISPLAY_VERSION,
            display_of(SEMVER),
            "displayed version must follow the workspace Cargo.toml version"
        );
    }

    #[test]
    fn pre_release_tag_is_shortened_to_its_first_letter() {
        assert_eq!(display_of("0.1.28-alpha"), "0.1.28a");
        assert_eq!(display_of("0.2.0-beta"), "0.2.0b");
        assert_eq!(display_of("1.0.0"), "1.0.0");
    }
}
