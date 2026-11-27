//! CLI of the R2 build-time KTX2 encoder.
//!
//! `sample` turns real game art into lossless PNG sources; `encode` turns a source
//! directory into `.ktx2` files plus `remaster-manifest.json` under `target/remaster/`
//! (outside git). Both codecs, ETC1S and UASTC, are produced for every source.
//!
//! See `README.md` for the gate commands and `max-brh-docs/development/remaster-hd.md`
//! for the R2 step itself.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use remaster_encode::encode::{self, Codec};
use remaster_encode::sample;

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();

    match arguments.first().map(String::as_str) {
        Some("sample") => sample_command(&arguments),
        Some("encode") => encode_command(&arguments),
        Some("help" | "-h" | "--help") | None => {
            usage();

            Ok(())
        }
        Some(other) => {
            usage();
            anyhow::bail!("unknown subcommand: {other}")
        }
    }
}

fn usage() {
    println!(
        "remaster-encode — R2 build-time KTX2 encoder for the HD remaster\n\
         \n\
         usage: remaster-encode <command> [--root DIR] [--out DIR]\n\
         \x20      remaster-encode encode [--sources DIR] [--out DIR] [--primary CODEC] [--verify]\n\
         \n\
         commands:\n\
         \x20 sample   decode real game art (GREEN_1.WRL tile, FRAMEPIC, TANK frame)\n\
         \x20          into lossless PNG sources\n\
         \x20 encode   encode a source directory into .ktx2 (ETC1S + UASTC)\n\
         \x20          plus remaster-manifest.json\n\
         \n\
         options:\n\
         \x20 --root DIR      repository root with max-original/ (default: search upwards from the CWD)\n\
         \x20 --out DIR       sample: PNG directory (default <root>/target/remaster-sources)\n\
         \x20                 encode: .ktx2 + manifest directory (default <root>/target/remaster)\n\
         \x20 --sources DIR   encode: source directory (default <root>/target/remaster-sources)\n\
         \x20 --primary CODEC etc1s (default) or uastc — the codec `assets[].path` points at\n\
         \x20 --verify        encode every file twice and fail unless both runs are byte-identical"
    );
}

fn sample_command(arguments: &[String]) -> Result<()> {
    let root = find_root(arguments)?;
    let output = option(arguments, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target").join("remaster-sources"));

    let images = sample::build(&root)?;
    let written = sample::write(&output, &images)?;

    println!("## R2 sample sources (lossless PNG)");
    println!();
    println!("| file | size | scale |");
    println!("|---|---:|---:|");
    for image in &images {
        println!(
            "| {} | {}x{} | {} |",
            image.file_name, image.width, image.height, image.scale
        );
    }
    println!();
    println!("Wrote {} file(s) to {}", written.len(), output.display());

    Ok(())
}

fn encode_command(arguments: &[String]) -> Result<()> {
    let root = find_root(arguments)?;
    let sources = option(arguments, "--sources")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target").join("remaster-sources"));
    let output = option(arguments, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target").join("remaster"));
    let primary = match option(arguments, "--primary") {
        Some(value) => Codec::parse(&value)
            .with_context(|| format!("unknown codec `{value}` (expected `etc1s` or `uastc`)"))?,
        None => Codec::Etc1s,
    };
    let verify = arguments.iter().any(|argument| argument == "--verify");

    let summary = encode::run(&sources, &output, primary, verify)?;

    println!("## R2 .ktx2 encode");
    println!();
    println!("| tag | size | scale | ETC1S | UASTC |");
    println!("|---|---:|---:|---:|---:|");
    for asset in &summary.assets {
        println!(
            "| {} | {}x{} | {} | {} | {} |",
            asset.tag,
            asset.width,
            asset.height,
            asset.scale,
            variant_cell(asset, "etc1s"),
            variant_cell(asset, "uastc"),
        );
    }
    println!();
    println!(
        "encoder: {} {} (ktx-rs {}) · ETC1S quality {}, UASTC flags {}, {} thread",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        encode::KTX_RS_VERSION,
        encode::QUALITY_LEVEL,
        encode::UASTC_FLAGS,
        encode::THREAD_COUNT
    );
    println!(
        "primary codec: {} · deterministic: {}",
        primary.label(),
        if verify {
            "verified in-process (2x)"
        } else {
            "not self-checked (--verify off)"
        }
    );
    println!(
        "manifest: {} · sha256:{}",
        summary.manifest.display(),
        summary.manifest_sha256
    );
    println!(
        "total: {} tags, {} `.ktx2` files, {:.1} KiB",
        summary.assets.len(),
        summary.assets.len() * Codec::ALL.len(),
        summary.total_bytes as f64 / 1024.0
    );

    Ok(())
}

/// `bytes KiB + short hash` cell of the summary table.
fn variant_cell(asset: &encode::EncodedAsset, codec: &str) -> String {
    match asset.variants.get(codec) {
        Some(variant) => format!(
            "{:.1} KiB · {}",
            variant.bytes as f64 / 1024.0,
            short_hash(&variant.sha256)
        ),
        None => "-".to_string(),
    }
}

fn short_hash(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

fn option(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1))
        .cloned()
}

/// Repository root: `--root`, next to the crate, or upwards from the CWD.
fn find_root(arguments: &[String]) -> Result<PathBuf> {
    if let Some(root) = option(arguments, "--root") {
        let path = PathBuf::from(root);

        return Ok(path.canonicalize().unwrap_or(path));
    }

    let candidates = [
        std::env::current_dir()?,
        Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf(),
    ];

    for start in candidates {
        for ancestor in start.ancestors() {
            if ancestor.join("max-original").join("MAX.RES").is_file() {
                return Ok(ancestor.to_path_buf());
            }
        }
    }

    anyhow::bail!("cannot find the repository root (max-original/MAX.RES); pass --root")
}
