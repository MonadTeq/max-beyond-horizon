//! M.A.X. resource editor: просмотр MAX.RES и пакетный экспорт изображений.
//!
//! ```text
//! max-resource-editor                      # UI (нужен --features sdl)
//! max-resource-editor --export MAX.RES COLOR.PAL max-assets
//! max-resource-editor --report MAX.RES
//! ```
//!
//! Без флагов открывается окно; `--export` работает headless и не требует SDL.
//!
//! При запуске UI путь к `MAX.RES` ищется в порядке: аргумент (`--res` или
//! позиционный) → рядом с `.exe` → `CWD` → вверх по дереву каталогов
//! (`max-original/MAX.RES`). Если ничего не найдено, показывается нативный
//! диалог «Укажите файл MAX.RES»; отмена — сообщение и код возврата 1.
//! `COLOR.PAL` ищется рядом с выбранным `MAX.RES`, его отсутствие — только
//! предупреждение (превью рисуется заглушкой-палитрой).

#[cfg(feature = "sdl")]
mod dialog;

use max_resource_editor::export;
use max_resource_editor::model;
#[cfg(feature = "sdl")]
use max_resource_editor::startup;
#[cfg(feature = "sdl")]
use max_resource_editor::ui;

use std::path::PathBuf;
use std::process::ExitCode;

#[cfg(feature = "sdl")]
use anyhow::anyhow;
use anyhow::{bail, Context, Result};
use max_assets::ResArchive;
use max_render::palette::Palette;

use crate::export::ExportOptions;
use crate::model::ClassificationStats;

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
         UI:\n  max-resource-editor [--res PATH] [--pal PATH] [--select TAG] [--shot OUT.png]\n  max-resource-editor [MAX.RES]\n\
         \n\
         Если MAX.RES не указан и не найден рядом с exe / в CWD / вверх по дереву,\n\
         открывается диалог выбора файла (фильтр *.RES). COLOR.PAL ищется рядом\n\
         с MAX.RES; если его нет, редактор работает с заглушкой-палитрой.\n\
         \n\
         Экспорт:\n  max-resource-editor --export <MAX.RES> <COLOR.PAL> <OUT_ROOT>\n\
         \n\
         Отчёт без записи файлов:\n  max-resource-editor --report <MAX.RES> [COLOR.PAL]\n\
         \n\
         UI требует сборки с --features sdl (SDL3); headless-команды работают без SDL."
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

/// Аргументы UI-режима.
struct UiOptions {
    /// `--res` или позиционный путь; `None` — искать дефолты/открыть диалог.
    res: Option<PathBuf>,
    /// `--pal`; `None` — искать `COLOR.PAL` рядом с `MAX.RES`.
    pal: Option<PathBuf>,
    select: Option<String>,
    shot: Option<PathBuf>,
}

impl UiOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Self {
            res: None,
            pal: None,
            select: None,
            shot: None,
        };
        let mut index = 0usize;

        while index < args.len() {
            match args[index].as_str() {
                "--res" => {
                    index += 1;
                    options.res = Some(PathBuf::from(
                        args.get(index).context("--res требует путь")?,
                    ));
                }
                "--pal" => {
                    index += 1;
                    options.pal = Some(PathBuf::from(
                        args.get(index).context("--pal требует путь")?,
                    ));
                }
                "--select" => {
                    index += 1;
                    options.select = Some(args.get(index).context("--select требует тег")?.clone());
                }
                "--shot" => {
                    index += 1;
                    options.shot = Some(PathBuf::from(
                        args.get(index).context("--shot требует путь")?,
                    ));
                }
                other if other.starts_with('-') => {
                    bail!("неизвестный аргумент {other:?}; см. --help")
                }
                other => {
                    if options.res.is_some() {
                        bail!("лишний аргумент {other:?}: путь MAX.RES уже задан");
                    }
                    options.res = Some(PathBuf::from(other));
                }
            }
            index += 1;
        }

        Ok(options)
    }
}

fn ui_command(args: &[String]) -> Result<ExitCode> {
    let options = UiOptions::parse(args)?;

    open_ui(&options)
}

/// Запуск UI: поиск `MAX.RES` (аргумент → дефолты → диалог), палитра, окно.
#[cfg(feature = "sdl")]
fn open_ui(options: &UiOptions) -> Result<ExitCode> {
    let sdl = sdl3::init().map_err(|error| anyhow!("SDL_Init: {error}"))?;
    let candidates = startup::default_candidates();

    let res = startup::resolve_startup_res(
        options.res.as_deref(),
        &candidates,
        |candidate| candidate.is_file(),
        || dialog::pick_max_res_file(&sdl),
    )?;

    let archive = ResArchive::open(&res).with_context(|| format!("чтение {}", res.display()))?;
    let palette = startup::load_palette(options.pal.as_deref(), &res);

    if let Some(warning) = &palette.warning {
        eprintln!("warning: {warning}");
    }

    let mut state = ui::EditorState::new(archive, palette.palette, options.select.as_deref());

    // В окне шрифт только ASCII, поэтому короткая английская строка; подробное
    // русское предупреждение с путями уже ушло в stderr выше.
    if palette.warning.is_some() {
        state.status =
            "WARN: COLOR.PAL not found near MAX.RES - preview uses grayscale stub palette"
                .to_string();
    }

    max_resource_editor::app::run_with_sdl(sdl, state, options.shot.as_deref())?;

    Ok(ExitCode::SUCCESS)
}

/// Без SDL UI недоступно: те же поиск/диалог собрать нельзя.
#[cfg(not(feature = "sdl"))]
fn open_ui(_options: &UiOptions) -> Result<ExitCode> {
    bail!(
        "UI не собрано: нужен SDL3 и feature `sdl`.\n\
         Сборка: cargo build --release --features sdl-vcpkg\n\
         Headless-экспорт: max-resource-editor --export <MAX.RES> <COLOR.PAL> <OUT_ROOT>"
    )
}
