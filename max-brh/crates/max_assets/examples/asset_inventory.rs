//! Builds the exact distribution file manifest `data-inventory.json`.
//!
//! Stage 1 of the installer plan (`max-brh-docs/development/distro-installers.md`
//! §F): every file the standalone build must ship, with its SHA-256 digest and
//! its distribution class. The tool reads the data directory the same way the
//! game does — `MAX.RES`/`PATCHES.RES` references, the mission registry, the
//! music playlists — so the manifest cannot drift from the loaders.
//!
//! ```sh
//! cargo run -p max_assets --example asset_inventory -- ../max-original
//! # -> ./data-inventory.json
//!
//! cargo run -p max_assets --example asset_inventory -- ../max-original \
//!     --out ../max-brh-installer/data-inventory.json
//! ```
//!
//! The run is deterministic: two runs over the same data directory produce
//! byte-identical manifests. The exit code is 0 for a complete manifest and 1
//! when a required file is missing (the manifest is still written, with the
//! gaps in its `missing` array).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use max_assets::inventory::{DataInventory, INVENTORY_FILE};

const USAGE: &str = "\
usage: asset_inventory <data-dir> [--out <path>] [--stdout]

  <data-dir>   game data directory (MAX.RES, PATCHES.RES, COLOR.PAL, *.WRL,
               *.SPW, *.FLC, *.MSC, *.MVE, missions, maxint.json)
  --out <path> write the manifest here (default: ./data-inventory.json)
  --stdout     print the manifest JSON instead of writing a file
  -h, --help   show this help";

/// Parsed command line.
struct Args {
    data_dir: PathBuf,
    /// `None` with `stdout == true`, otherwise the manifest destination.
    out: PathBuf,
    stdout: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("asset_inventory: {message}");

            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args = parse_args()?;
    let inventory = max_assets::inventory::build(&args.data_dir)
        .map_err(|error| format!("cannot build the inventory: {error}"))?;

    if args.stdout {
        print!("{}", inventory.to_json());
    } else {
        inventory
            .write(&args.out)
            .map_err(|error| format!("cannot write {}: {error}", args.out.display()))?;
        print_summary(&inventory, &args.out);
    }

    if inventory.is_complete() {
        return Ok(ExitCode::SUCCESS);
    }

    for missing in inventory.missing() {
        eprintln!(
            "asset_inventory: [missing] {} ({}) — {}",
            missing.path,
            missing.class.as_str(),
            missing.reason
        );
    }
    eprintln!(
        "asset_inventory: the manifest is incomplete: {} required file(s) missing",
        inventory.missing().len()
    );

    Ok(ExitCode::from(1))
}

fn parse_args() -> Result<Args, String> {
    let mut data_dir = None;
    let mut out = None;
    let mut stdout = false;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                let path = args.next().ok_or("--out requires a path")?;

                out = Some(PathBuf::from(path));
            }
            "--stdout" => stdout = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other:?}\n{USAGE}"));
            }
            other => {
                if data_dir.is_some() {
                    return Err(format!(
                        "only one data directory is accepted (got {other:?})\n{USAGE}"
                    ));
                }

                data_dir = Some(PathBuf::from(other));
            }
        }
    }

    let data_dir = data_dir.ok_or_else(|| format!("missing <data-dir>\n{USAGE}"))?;

    if stdout && out.is_some() {
        return Err("--stdout and --out are mutually exclusive".to_owned());
    }

    Ok(Args {
        data_dir,
        out: out.unwrap_or_else(|| PathBuf::from(INVENTORY_FILE)),
        stdout,
    })
}

fn print_summary(inventory: &DataInventory, out: &Path) {
    let sources = inventory.sources();

    println!(
        "asset_inventory: MAX.RES {} entries, {} file references ({} unique, {} unresolved)",
        sources.max_res_entries,
        sources.max_res_file_references,
        sources.max_res_unique_references,
        sources.max_res_unresolved_references
    );
    println!(
        "asset_inventory: PATCHES.RES {} entries, {} file references ({} unique, {} unresolved)",
        sources.patches_res_entries,
        sources.patches_res_file_references,
        sources.patches_res_unique_references,
        sources.patches_res_unresolved_references
    );
    println!(
        "asset_inventory: missions {} descriptors, {} files ({} present, {} hashes verified, \
         {} mismatched), {} briefings",
        sources.mission_descriptors,
        sources.mission_files,
        sources.mission_files_present,
        sources.mission_hashes_verified,
        sources.mission_hashes_mismatched,
        sources.mission_briefing_files
    );
    println!(
        "asset_inventory: music {}/{} tracks ({} allowlisted missing, {} missing)",
        sources.music_files,
        sources.music_tracks,
        sources.music_allowlisted_missing,
        sources.music_missing
    );

    let classes: Vec<String> = inventory
        .classes()
        .iter()
        .map(|count| format!("{}={}", count.class.as_str(), count.files))
        .collect();
    let totals = inventory.totals();
    let mib = totals.bytes as f64 / (1024.0 * 1024.0);

    println!("asset_inventory: classes {}", classes.join(" "));
    println!(
        "asset_inventory: files {}, {} bytes ({mib:.1} MiB)",
        totals.files, totals.bytes
    );

    for gap in inventory.allowlisted_missing() {
        println!(
            "asset_inventory: allowlisted gap: {} ({}) — {}",
            gap.path,
            gap.class.as_str(),
            gap.reason
        );
    }

    println!("asset_inventory: wrote {}", out.display());
}
