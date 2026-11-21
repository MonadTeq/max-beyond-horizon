//! CLI экспорта ресурсов по юнитам.
//!
//! Запуск из `max-brh-qa-tools/max-resources-exporter` (пути по умолчанию — относительно корня
//! репозитория):
//!
//! ```sh
//! cargo run --release
//! cargo run --release -- --out ../../max-rust-3d-models --only TANK,MININGST
//! ```
//!
//! Флаги: `--res`, `--pal`, `--flc-dir`, `--units`, `--out`, `--patches`,
//! `--no-patches-check`, `--only TAG[,TAG…]`, `--help`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Result};
use unit_asset_export::{export_all, export::summary, ExportOptions};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ошибка: {error:#}");

            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let Some(options) = parse_args()? else {
        return Ok(());
    };

    println!("max-resources-exporter: {} типов", 93);
    println!("  MAX.RES:    {}", options.max_res.display());
    println!("  COLOR.PAL:  {}", options.color_pal.display());
    println!("  FLC dir:    {}", options.flc_dir.display());
    println!("  маппинг:    {}", options.units_json.display());
    println!("  вывод:      {}", options.out_root.display());

    let report = export_all(&options)?;

    println!("\n{}", summary(&report));
    println!("manifest: {}", report.out_root.join("manifest.csv").display());
    println!("README:   {}", report.out_root.join("README.md").display());

    if !report.errors.is_empty() {
        bail!("{} тип(ов) не выгружено", report.errors.len());
    }

    Ok(())
}

/// Разбирает аргументы; `None` — `--help` (уже всё напечатано).
fn parse_args() -> Result<Option<ExportOptions>> {
    let repo_root = repo_root();
    let mut options = ExportOptions::defaults(&repo_root);
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        let mut value = || -> Result<String> {
            args.next()
                .ok_or_else(|| anyhow::anyhow!("флаг {arg} требует значения"))
        };

        match arg.as_str() {
            "--res" => options.max_res = PathBuf::from(value()?),
            "--pal" => options.color_pal = PathBuf::from(value()?),
            "--flc-dir" => options.flc_dir = PathBuf::from(value()?),
            "--units" => options.units_json = PathBuf::from(value()?),
            "--out" => options.out_root = PathBuf::from(value()?),
            "--patches" => {
                let path = PathBuf::from(value()?);

                options.patches = if path.as_os_str().is_empty() {
                    None
                } else {
                    Some(path)
                };
            }
            "--no-patches-check" => options.patches = None,
            "--only" => {
                let tokens: Vec<String> = value()?
                    .split(',')
                    .map(|token| token.trim().to_uppercase())
                    .filter(|token| !token.is_empty())
                    .collect();

                options.only = Some(tokens);
            }
            "--help" | "-h" => {
                usage();

                return Ok(None);
            }
            other => bail!("неизвестный флаг {other} (см. --help)"),
        }
    }

    Ok(Some(options))
}

/// Корень репозитория: `max-brh-qa-tools/max-resources-exporter` → `max-brh-qa-tools` → корень.
fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));

    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest.to_path_buf())
}

fn usage() {
    println!("max-resources-exporter — выгрузка ресурсов M.A.X. по каждому типу юнита\n");
    println!("Флаги (по умолчанию — пути из корня репозитория):");
    println!("  --res PATH        MAX.RES");
    println!("  --pal PATH        COLOR.PAL");
    println!("  --flc-dir PATH    каталог с *.FLC");
    println!("  --units PATH      cargo_unit_data.json (снимок SC_UNITS)");
    println!("  --out PATH        корень выгрузки (max-rust-3d-models)");
    println!("  --patches PATH    PATCHES.RES для сверки SC_UNITS");
    println!("  --no-patches-check  не сверять с SC_UNITS");
    println!("  --only T1,T2      выгрузить только эти токены");
    println!("  --help            эта справка");
}
