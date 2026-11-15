//! Пакетный экспорт изображений MAX.RES в PNG + манифест.
//!
//! Раскладка:
//! ```text
//! <out_root>/images/ui/*.png
//! <out_root>/images/sprites/units/*.png
//! <out_root>/images/misc/*.png
//! <out_root>/images/manifest.csv
//! <out_root>/images/EXPORT-REPORT.txt
//! ```
//!
//! Имена файлов: `<TAG>_<W>x<H>[_<N>f][_ownpal|_mask].png`. Для спрайтов/теней
//! `WxH` — первый кадр, `N` — число кадров; сами кадры лежат горизонтальной
//! лентой. `_ownpal` — big image со своей палитрой, `_mask` — тень.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use max_assets::ResArchive;
use max_render::palette::Palette;

use crate::model::{ClassificationStats, ResourceInfo};
use crate::render::render_resource;

/// Что и куда экспортировать.
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Ограничить набор тегов (для тестов/избранного).
    pub only: Option<HashSet<String>>,
    /// Писать manifest.csv и EXPORT-REPORT.txt.
    pub manifest: bool,
}

impl ExportOptions {
    /// Полный экспорт.
    pub fn all() -> Self {
        Self {
            only: None,
            manifest: true,
        }
    }

    /// Только перечисленные теги (манифест не нужен).
    pub fn only(names: &[&str]) -> Self {
        Self {
            only: Some(names.iter().map(|name| (*name).to_string()).collect()),
            manifest: false,
        }
    }
}

/// Результат экспорта.
#[derive(Debug, Clone, Default)]
pub struct ExportReport {
    /// Полная статистика архива (включая не-изображения).
    pub stats: ClassificationStats,
    /// Записей-изображений в архиве.
    pub images: usize,
    /// Реально записано PNG (без дубликатов путей).
    pub exported: usize,
    /// Дубликаты тега/имени файла (в MAX.RES — `PRFDNE_D`).
    pub duplicate_paths: usize,
    /// Суммарный размер PNG в байтах.
    pub bytes: u64,
    /// Относительные пути записанных файлов.
    pub files: Vec<String>,
    /// Ошибки рендера/записи (экспорт продолжается).
    pub errors: Vec<String>,
    /// Корень экспорта.
    pub out_root: PathBuf,
}

/// Экспортирует изображения архива.
pub fn export_archive(
    archive: &ResArchive,
    palette: &Palette,
    out_root: impl AsRef<Path>,
    infos: &[ResourceInfo],
    options: &ExportOptions,
) -> Result<ExportReport> {
    let out_root = out_root.as_ref().to_path_buf();
    let images_root = out_root.join("images");
    let mut report = ExportReport {
        stats: ClassificationStats::from_infos(infos),
        images: infos.iter().filter(|info| info.kind.is_image()).count(),
        out_root: out_root.clone(),
        ..ExportReport::default()
    };
    let mut written: HashSet<String> = HashSet::new();

    for info in infos {
        if !info.kind.is_image() {
            continue;
        }
        if let Some(only) = &options.only {
            if !only.contains(&info.name) {
                continue;
            }
        }

        let Some(relative) = info.relative_path() else {
            continue;
        };
        if !written.insert(relative.clone()) {
            report.duplicate_paths += 1;
            continue;
        }

        let path = out_root.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("создание каталога {}", parent.display()))?;
        }

        match render_resource(info, archive, palette) {
            Ok(image) => match write_with_retry(&path, &image) {
                Ok(()) => {
                    report.exported += 1;
                    report.bytes += fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                    report.files.push(relative);
                }
                Err(error) => report.errors.push(format!("{}: {error}", info.name)),
            },
            Err(error) => report.errors.push(format!("{}: {error:#}", info.name)),
        }
    }

    if options.manifest {
        fs::create_dir_all(&images_root)
            .with_context(|| format!("создание каталога {}", images_root.display()))?;
        write_manifest(&images_root.join("manifest.csv"), infos, &written)?;
        write_report(&images_root.join("EXPORT-REPORT.txt"), &report)?;
    }

    Ok(report)
}

/// Запись PNG с повтором: Windows-антивирус/индексатор иногда держит файл
/// открытым (`os error 1224`, user-mapped section) сразу после создания.
fn write_with_retry(path: &Path, image: &crate::png::RgbaImage) -> Result<(), String> {
    let mut last = String::new();

    for attempt in 0..6u64 {
        match crate::png::write_rgba(path, image) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last = error;
                std::thread::sleep(std::time::Duration::from_millis(120 * (attempt + 1)));
            }
        }
    }

    Err(last)
}

/// CSV-манифест всех записей (включая не-изображения и не экспортированные).
pub fn write_manifest(
    path: &Path,
    infos: &[ResourceInfo],
    written: &HashSet<String>,
) -> Result<()> {
    let mut out = String::from(
        "index,name,offset_hex,size,kind,category,width,height,frames,palette,transparent,non_image,ref,file\n",
    );

    for info in infos {
        let relative = info.relative_path().unwrap_or_default();
        let file = if written.contains(&relative) {
            relative.as_str()
        } else {
            ""
        };
        let transparent = info
            .transparent
            .map(|value| value.to_string())
            .unwrap_or_default();
        let non_image = info.non_image.map(|kind| kind.as_str()).unwrap_or_default();

        out.push_str(&format!(
            "{},{},0x{:08X},{},{},{},{},{},{},{},{},{},{},{}\n",
            info.index,
            info.name,
            info.offset,
            info.size,
            info.kind.as_str(),
            info.category.as_str(),
            info.width,
            info.height,
            info.frame_count,
            info.palette.as_str(),
            transparent,
            non_image,
            info.file_ref.as_deref().unwrap_or(""),
            file
        ));
    }

    fs::write(path, out).with_context(|| format!("запись {}", path.display()))?;

    Ok(())
}

fn write_report(path: &Path, report: &ExportReport) -> Result<()> {
    let stats = &report.stats;
    let mut out = String::new();

    out.push_str("MAX.RES resource export\n");
    out.push_str("=======================\n\n");
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

    out.push_str(&format!(
        "\nwritten: {} PNG, {} bytes, duplicate paths: {}, errors: {}\n",
        report.exported,
        report.bytes,
        report.duplicate_paths,
        report.errors.len()
    ));

    for error in &report.errors {
        out.push_str(&format!("error: {error}\n"));
    }

    fs::write(path, out).with_context(|| format!("запись {}", path.display()))?;

    Ok(())
}

/// Человекочитаемая сводка для stdout.
pub fn summary(report: &ExportReport) -> String {
    let stats = &report.stats;
    let mut out = String::new();

    out.push_str(&format!(
        "entries: {}  images: {} (ui {}, sprites/units {}, misc {})\n",
        stats.entries, stats.images, stats.ui, stats.units, stats.misc
    ));
    out.push_str(&format!(
        "kinds: simple {}, big {}, sprite {}, shadow {}, cursor {}\n",
        stats.simple, stats.big, stats.sprite, stats.shadow, stats.cursor
    ));
    out.push_str(&format!("non-images: {}\n", stats.non_images));

    for (kind, count) in &stats.non_image_kinds {
        out.push_str(&format!("  {:<8} {}\n", kind.as_str(), count));
    }

    out.push_str(&format!(
        "written: {} PNG ({} bytes){}; errors: {}\n",
        report.exported,
        report.bytes,
        if report.duplicate_paths > 0 {
            format!(", duplicate paths skipped: {}", report.duplicate_paths)
        } else {
            String::new()
        },
        report.errors.len()
    ));

    for error in &report.errors {
        out.push_str(&format!("  error: {error}\n"));
    }

    out
}
