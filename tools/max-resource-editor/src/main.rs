//! M.A.X. resource editor: просмотр MAX.RES и пакетный экспорт изображений.
//!
//! ```text
//! max-resource-editor                      # UI (нужен --features sdl)
//! max-resource-editor --export MAX.RES COLOR.PAL max-assets
//! max-resource-editor --report MAX.RES
//! ```
//!
//! Без флагов открывается окно; `--export` работает headless и не требует SDL.

use max_resource_editor::export;
use max_resource_editor::model;
#[cfg(feature = "sdl")]
use max_resource_editor::ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use max_assets::ResArchive;
use max_render::palette::Palette;

use crate::export::ExportOptions;
use crate::model::ClassificationStats;

const DEFAULT_RES: &str = "max-original/MAX.RES";
const DEFAULT_PAL: &str = "max-original/COLOR.PAL";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--export") {
        return export_command(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("--report") {
        return report_command(&args[1..]);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(ExitCode::SUCCESS);
    }

    ui_command(&args)
}

fn print_help() {
    println!(
        "max-resource-editor — просмотр MAX.RES и экспорт изображений\n\
         \n\
         UI:\n  max-resource-editor [--res PATH] [--pal PATH] [--select TAG] [--shot OUT.png]\n\
         \n\
         Экспорт:\n  max-resource-editor --export <MAX.RES> <COLOR.PAL> <OUT_ROOT>\n\
         \n\
         Отчёт без записи файлов:\n  max-resource-editor --report <MAX.RES> [COLOR.PAL]\n\
         \n\
         UI требует сборки с --features sdl (SDL3)."
    );
}

fn export_command(args: &[String]) -> Result<ExitCode> {
    if args.len() < 3 {
        bail!("usage: max-resource-editor --export <MAX.RES> <COLOR.PAL> <OUT_ROOT>");
    }

    let archive = ResArchive::open(&args[0]).with_context(|| format!("чтение {}", args[0]))?;
    let palette = Palette::load(&args[1]).with_context(|| format!("чтение {}", args[1]))?;
    let infos = model::classify_archive(&archive);
    let report =
        export::export_archive(&archive, &palette, &args[2], &infos, &ExportOptions::all())?;

    print!("{}", export::summary(&report));
    println!("out: {}", report.out_root.display());

    if report.errors.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

fn report_command(args: &[String]) -> Result<ExitCode> {
    let Some(path) = args.first() else {
        bail!("usage: max-resource-editor --report <MAX.RES> [COLOR.PAL]");
    };

    let archive = ResArchive::open(path).with_context(|| format!("чтение {path}"))?;
    let infos = model::classify_archive(&archive);
    let stats = ClassificationStats::from_infos(&infos);

    println!("{}", report_text(&stats));

    Ok(ExitCode::SUCCESS)
}

fn report_text(stats: &ClassificationStats) -> String {
    let mut out = String::new();

    out.push_str(&format!("entries: {}\n", stats.entries));
    out.push_str(&format!(
        "images: {} (ui {}, sprites/units {}, misc {})\n",
        stats.images, stats.ui, stats.units, stats.misc
    ));
    out.push_str(&format!(
        "kinds: simple {}, big {}, sprite {}, shadow {}, cursor {}\n",
        stats.simple, stats.big, stats.sprite, stats.shadow, stats.cursor
    ));
    out.push_str(&format!("non-images: {}\n", stats.non_images));
    for (kind, count) in &stats.non_image_kinds {
        out.push_str(&format!("  {:<8} {}\n", kind.as_str(), count));
    }

    out
}

fn ui_command(args: &[String]) -> Result<ExitCode> {
    let mut res = PathBuf::from(DEFAULT_RES);
    let mut pal = PathBuf::from(DEFAULT_PAL);
    let mut select: Option<String> = None;
    let mut shot: Option<PathBuf> = None;
    let mut index = 0usize;

    while index < args.len() {
        match args[index].as_str() {
            "--res" => {
                index += 1;
                res = PathBuf::from(args.get(index).context("--res требует путь")?);
            }
            "--pal" => {
                index += 1;
                pal = PathBuf::from(args.get(index).context("--pal требует путь")?);
            }
            "--select" => {
                index += 1;
                select = Some(args.get(index).context("--select требует тег")?.clone());
            }
            "--shot" => {
                index += 1;
                shot = Some(PathBuf::from(
                    args.get(index).context("--shot требует путь")?,
                ));
            }
            other => bail!("неизвестный аргумент {other:?}; см. --help"),
        }
        index += 1;
    }

    open_ui(&res, &pal, select.as_deref(), shot.as_deref())
}

#[cfg(feature = "sdl")]
fn open_ui(res: &Path, pal: &Path, select: Option<&str>, shot: Option<&Path>) -> Result<ExitCode> {
    let archive = ResArchive::open(res).with_context(|| format!("чтение {}", res.display()))?;
    let palette = Palette::load(pal).with_context(|| format!("чтение {}", pal.display()))?;
    let state = ui::EditorState::new(archive, palette, select);

    max_resource_editor::app::run(state, shot)?;

    Ok(ExitCode::SUCCESS)
}

#[cfg(not(feature = "sdl"))]
fn open_ui(
    _res: &Path,
    _pal: &Path,
    _select: Option<&str>,
    _shot: Option<&Path>,
) -> Result<ExitCode> {
    bail!(
        "UI не собрано: нужен SDL3 и feature `sdl`.\n\
         Сборка: cargo build --release --features sdl-vcpkg\n\
         Headless-экспорт: max-resource-editor --export <MAX.RES> <COLOR.PAL> <OUT_ROOT>"
    )
}
