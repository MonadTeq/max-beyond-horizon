//! Lossless source discovery: a flat directory of PNG files and/or raw RGBA8 dumps.
//!
//! PNG is the normal authoring format (decoded with the `image` crate, RGBA8 output).
//! A raw dump needs its dimensions in the file name, since the bytes carry no header:
//!
//! * `<tag>.png` — `tag` is the manifest tag, `scale` defaults to 1;
//! * `<tag>@4x.png` — the same, authored for presentation scale 4;
//! * `<tag>_<W>x<H>.rgba` / `<tag>@2x_<W>x<H>.rgba` — `W * H * 4` RGBA8 bytes, row-major.
//!
//! Files are processed in file-name order and tags must be unique, so the resulting
//! manifest does not depend on directory enumeration order.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::hash::sha256_hex;

/// One decoded lossless source.
#[derive(Debug, Clone)]
pub struct Source {
    /// Manifest tag (file stem without the `@<N>x` / `_<W>x<H>` decorations).
    pub tag: String,
    /// Source file name, used in the manifest and the generated `.ktx2` names.
    pub file_name: String,
    /// Presentation scale the source was authored for.
    pub scale: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8 pixels, row-major: exactly `width * height * 4` bytes.
    pub rgba: Vec<u8>,
    /// SHA-256 of the source file bytes (`hex`), recorded in the manifest.
    pub sha256: String,
}

/// Collects every `.png` / `.rgba` file in `directory`, in file-name order.
pub fn discover(directory: &Path) -> Result<Vec<Source>> {
    anyhow::ensure!(
        directory.is_dir(),
        "source directory {} does not exist",
        directory.display()
    );

    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
        .with_context(|| format!("cannot list {}", directory.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("cannot list {}", directory.display()))?
        .into_iter()
        .filter(|path| path.is_file())
        .collect();

    paths.sort();

    let mut sources = Vec::new();
    for path in paths {
        let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let extension = extension.to_ascii_lowercase();

        if extension == "png" || extension == "rgba" {
            sources.push(load(&path, &extension)?);
        }
    }

    anyhow::ensure!(
        !sources.is_empty(),
        "no `.png`/`.rgba` sources in {}",
        directory.display()
    );

    let mut seen: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for source in &sources {
        if let Some(previous) = seen.insert(source.tag.as_str(), source.file_name.as_str()) {
            anyhow::bail!(
                "duplicate tag `{}`: {} and {}",
                source.tag,
                previous,
                source.file_name
            );
        }
    }

    Ok(sources)
}

/// Loads and validates one source file.
fn load(path: &Path, extension: &str) -> Result<Source> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("non-UTF-8 source file name: {}", path.display()))?
        .to_string();
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("non-UTF-8 source file name: {}", path.display()))?;
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let sha256 = sha256_hex(&bytes);

    let (tag, scale, width, height, rgba) = if extension == "rgba" {
        let (stem, width, height) = split_dimensions(stem).with_context(|| {
            format!(
                "raw RGBA source {} needs dimensions in its name: `<tag>_<W>x<H>.rgba`",
                file_name
            )
        })?;
        let (tag, scale) = split_scale(&stem);
        validate_tag(&tag, &file_name)?;

        let expected = width as usize * height as usize * 4;
        anyhow::ensure!(
            bytes.len() == expected,
            "raw RGBA source {} is {} bytes, expected {expected} for {width}x{height}",
            file_name,
            bytes.len()
        );

        (tag, scale, width, height, bytes)
    } else {
        let (tag, scale) = split_scale(stem);
        validate_tag(&tag, &file_name)?;

        let dynamic = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .with_context(|| format!("cannot decode PNG {file_name}"))?;
        let image = dynamic.to_rgba8();
        let (width, height) = image.dimensions();

        (tag, scale, width, height, image.into_raw())
    };

    anyhow::ensure!(width > 0 && height > 0, "source {file_name} has no pixels");

    Ok(Source {
        tag,
        file_name,
        scale,
        width,
        height,
        rgba,
        sha256,
    })
}

/// Strips a trailing `@<N>x` scale tag: `"tile@4x"` -> `("tile", 4)`.
fn split_scale(stem: &str) -> (String, u32) {
    if let Some(at) = stem.rfind('@') {
        let suffix = &stem[at + 1..];

        if let Some(digits) = suffix.strip_suffix('x') {
            if let Ok(scale) = digits.parse::<u32>() {
                if scale > 0 && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    return (stem[..at].to_string(), scale);
                }
            }
        }
    }

    (stem.to_string(), 1)
}

/// Strips a trailing `_<W>x<H>`: `"tile_64x64"` -> `("tile", 64, 64)`.
fn split_dimensions(stem: &str) -> Option<(String, u32, u32)> {
    let underscore = stem.rfind('_')?;
    let (rest, suffix) = (&stem[..underscore], &stem[underscore + 1..]);
    let (width, height) = suffix.split_once('x')?;

    let width: u32 = width.parse().ok()?;
    let height: u32 = height.parse().ok()?;

    if width == 0 || height == 0 || rest.is_empty() {
        return None;
    }

    Some((rest.to_string(), width, height))
}

/// A tag becomes a manifest key and part of a file name, so it must be a plain name.
fn validate_tag(tag: &str, file_name: &str) -> Result<()> {
    anyhow::ensure!(!tag.is_empty(), "source {file_name} has an empty tag");
    anyhow::ensure!(
        !tag.contains(['/', '\\']) && !tag.chars().any(char::is_control),
        "source {file_name} has an unusable tag `{tag}`"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_suffix_is_parsed_and_stripped() {
        assert_eq!(split_scale("tile"), ("tile".to_string(), 1));
        assert_eq!(split_scale("tile@4x"), ("tile".to_string(), 4));
        assert_eq!(split_scale("ui.frame@2x"), ("ui.frame".to_string(), 2));
        assert_eq!(split_scale("tile@0x"), ("tile@0x".to_string(), 1));
        assert_eq!(split_scale("tile@x"), ("tile@x".to_string(), 1));
        assert_eq!(split_scale("tile@4"), ("tile@4".to_string(), 1));
    }

    #[test]
    fn raw_dimensions_are_parsed_and_stripped() {
        assert_eq!(
            split_dimensions("tile_64x64"),
            Some(("tile".to_string(), 64, 64))
        );
        assert_eq!(
            split_dimensions("tile@4x_256x256").map(|(tag, w, h)| (
                split_scale(&tag).0,
                w,
                h,
                split_scale(&tag).1
            )),
            Some(("tile".to_string(), 256, 256, 4))
        );
        assert_eq!(split_dimensions("tile"), None);
        assert_eq!(split_dimensions("tile_64"), None);
        assert_eq!(split_dimensions("tile_64x64x2"), None);
        assert_eq!(split_dimensions("tile_0x64"), None);
    }

    #[test]
    fn unusable_tags_are_rejected() {
        assert!(validate_tag("tile", "tile.png").is_ok());
        assert!(validate_tag("", "x.png").is_err());
        assert!(validate_tag("a/b", "x.png").is_err());
        assert!(validate_tag("a\\b", "x.png").is_err());
        assert!(validate_tag("a\nb", "x.png").is_err());
    }

    /// Round-trips a real PNG through `image` to prove the decode path is RGBA8.
    #[test]
    fn png_sources_decode_to_rgba8() {
        let directory = std::env::temp_dir().join(format!(
            "remaster-encode-sources-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();

        let pixels: Vec<u8> = (0..4 * 3 * 4).map(|index| (index * 7) as u8).collect();
        let path = directory.join("checker@2x.png");
        image::save_buffer_with_format(
            &path,
            &pixels,
            4,
            3,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .unwrap();

        let sources = discover(&directory).unwrap();
        std::fs::remove_dir_all(&directory).unwrap();

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].tag, "checker");
        assert_eq!(sources[0].scale, 2);
        assert_eq!((sources[0].width, sources[0].height), (4, 3));
        assert_eq!(sources[0].rgba, pixels);
        assert_eq!(sources[0].sha256.len(), 64);
    }

    #[test]
    fn a_missing_directory_is_an_error() {
        assert!(discover(Path::new("no-such-directory-remaster-encode")).is_err());
    }
}
