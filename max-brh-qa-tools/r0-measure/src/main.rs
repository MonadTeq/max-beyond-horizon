//! R0 measurements for the HD remaster: render memory (current 640×480),
//! asset inventory with the HD volume at ×2/×4, and the KTX2 → RGBA8
//! transcoder benchmark.
//!
//! See `README.md` for the exact commands and the report they feed
//! (`max-brh-docs/development/r0-measurements.md`).

mod alloc;
mod inventory;
mod transcode;
mod vram;

use std::path::{Path, PathBuf};

use anyhow::Result;

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments
        .first()
        .cloned()
        .unwrap_or_else(|| "help".to_string());

    if matches!(command.as_str(), "help" | "-h" | "--help") {
        usage();
        return Ok(());
    }

    let root = find_root(&arguments)?;
    let csv = option(&arguments, "--csv").map(PathBuf::from);
    let dump = option(&arguments, "--dump").map(PathBuf::from);
    let iterations = option(&arguments, "--iterations").and_then(|value| value.parse().ok());

    match command.as_str() {
        "inventory" => inventory::run(&root, csv.as_deref()),
        "vram" => vram::run(&root),
        "transcode" => transcode::run(&root, iterations, dump.as_deref()),
        other => {
            usage();
            anyhow::bail!("unknown subcommand: {other}")
        }
    }
}

fn usage() {
    println!(
        "r0-measure — R0 measurements of the HD remaster\n\
         \n\
         usage: r0-measure <command> [--root DIR] [--csv FILE] [--dump DIR] [--iterations N]\n\
         \n\
         commands:\n\
         \x20 inventory  asset classes, pixel counts, RGBA volume at ×1/×2/×4\n\
         \x20 vram       render memory of the current 640×480 mode (heap-measured)\n\
         \x20 transcode  KTX2 → RGBA8: basisu vs ktx-rs on real game art\n\
         \n\
         --root        repository root with max-original/ (default: search upwards from the CWD)\n\
         --csv FILE    write the inventory as CSV\n\
         --dump DIR    keep the generated `.ktx2` test files (for the C reference measurement)\n\
         --iterations  decode repetitions per file (default: from the pixel count)"
    );
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
