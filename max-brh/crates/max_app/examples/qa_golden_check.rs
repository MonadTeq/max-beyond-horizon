//! Слой 3 QA: сверка golden-кадров (попиксельно, без внешних крейтов).
//!
//! ```sh
//! # сверка после прогона visual-screens.qa
//! cargo run -p max_app --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden
//!
//! # перегенерация эталонов после осознанного визуального изменения
//! cargo run -p max_app --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden --update
//! ```
//!
//! Критерий дрейфа (тот же, что в `max-brh-qa/plans/qa-plan.md` и `ci.sh`): кадр считается
//! изменившимся, если больше **1 %** пикселей отличаются хотя бы одним каналом
//! больше чем на **30**. Для каждого такого кадра пишется `diff-<name>.png`
//! (белый фон, красные — разошедшиеся пиксели), а код выхода — 1.
//!
//! Эталоны сравниваются по имени файла: `golden/<name>.png` ↔
//! `<shots>/<name>.png`. Лишние кадры в `<shots>` (новый `shot` в скрипте)
//! печатаются как предупреждение — эталон нужно добавить через `--update`;
//! пропавший кадр (эталон есть, скриншота нет) — ошибка.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use max_app::qa::png;

/// Порог дельты канала (`max|Δканал| > 30` — изменение пикселя).
const DELTA_THRESHOLD: u8 = 30;
/// Допустимая доля изменившихся пикселей (1 %).
const CHANGED_RATIO: f64 = 0.01;

const USAGE: &str = "\
Использование: qa_golden_check <shots_dir> <golden_dir> [опции]

  --update            перезаписать эталоны кадрами из <shots_dir>
  --threshold N       порог по каналу (по умолчанию 30)
  --ratio R           допустимая доля изменившихся пикселей (по умолчанию 0.01)
  --diff-dir DIR      куда писать diff-*.png (по умолчанию <shots_dir>)
  -h, --help          эта справка
";

fn main() -> ExitCode {
    let mut shots: Option<PathBuf> = None;
    let mut golden: Option<PathBuf> = None;
    let mut diff_dir: Option<PathBuf> = None;
    let mut update = false;
    let mut threshold = DELTA_THRESHOLD;
    let mut ratio = CHANGED_RATIO;

    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--update" => update = true,
            "--threshold" => match args.next().and_then(|value| value.parse::<u8>().ok()) {
                Some(value) => threshold = value,
                None => return usage_error("--threshold требует число 0..=255"),
            },
            "--ratio" => match args.next().and_then(|value| value.parse::<f64>().ok()) {
                Some(value) if (0.0..=1.0).contains(&value) => ratio = value,
                _ => return usage_error("--ratio требует число 0.0..=1.0"),
            },
            "--diff-dir" => match args.next() {
                Some(value) => diff_dir = Some(PathBuf::from(value)),
                None => return usage_error("--diff-dir требует каталог"),
            },
            other if other.starts_with('-') => {
                return usage_error(&format!("неизвестная опция {other}"))
            }
            other => {
                if shots.is_none() {
                    shots = Some(PathBuf::from(other));
                } else if golden.is_none() {
                    golden = Some(PathBuf::from(other));
                } else {
                    return usage_error(&format!("лишний аргумент «{other}»"));
                }
            }
        }
    }

    let (Some(shots), Some(golden)) = (shots, golden) else {
        return usage_error("нужны <shots_dir> и <golden_dir>");
    };

    if update {
        return update_golden(&shots, &golden);
    }

    compare_dirs(
        &shots,
        &golden,
        diff_dir.as_deref().unwrap_or(&shots),
        threshold,
        ratio,
    )
}

fn png_files(dir: &Path) -> Result<BTreeSet<String>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|error| format!("каталог {}: {error}", dir.display()))?;
    let mut names = BTreeSet::new();

    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();

        if path.extension().is_some_and(|extension| extension == "png") {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                // `diff-*.png` — карты различий прошлых прогонов, не кадры.
                if !stem.starts_with("diff-") {
                    names.insert(stem.to_string());
                }
            }
        }
    }

    Ok(names)
}

fn update_golden(shots: &Path, golden: &Path) -> ExitCode {
    let names = match png_files(shots) {
        Ok(names) => names,
        Err(error) => {
            eprintln!("qa_golden_check: {error}");
            return ExitCode::FAILURE;
        }
    };

    if names.is_empty() {
        eprintln!(
            "qa_golden_check: в {} нет PNG — сначала прогоните visual-screens.qa",
            shots.display()
        );
        return ExitCode::FAILURE;
    }

    if let Err(error) = std::fs::create_dir_all(golden) {
        eprintln!("qa_golden_check: {}: {error}", golden.display());
        return ExitCode::FAILURE;
    }

    let mut total = 0u64;

    for name in &names {
        let from = shots.join(format!("{name}.png"));
        let to = golden.join(format!("{name}.png"));

        match std::fs::copy(&from, &to) {
            Ok(bytes) => {
                total += bytes;
                println!("эталон {name}.png ({bytes} байт)");
            }
            Err(error) => {
                eprintln!("qa_golden_check: {}: {error}", from.display());
                return ExitCode::FAILURE;
            }
        }
    }

    println!(
        "qa_golden_check: обновлено {} эталонов, {} КиБ — проверьте diff и закоммитьте",
        names.len(),
        total / 1024
    );

    ExitCode::SUCCESS
}

fn compare_dirs(shots: &Path, golden: &Path, diff_dir: &Path, threshold: u8, ratio: f64) -> ExitCode {
    let expected = match png_files(golden) {
        Ok(names) => names,
        Err(error) => {
            eprintln!("qa_golden_check: {error}");
            return ExitCode::FAILURE;
        }
    };

    if expected.is_empty() {
        eprintln!(
            "qa_golden_check: в {} нет эталонов (перегенерируйте: qa_golden_check … --update)",
            golden.display()
        );
        return ExitCode::FAILURE;
    }

    let actual = match png_files(shots) {
        Ok(names) => names,
        Err(error) => {
            eprintln!("qa_golden_check: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut drifted = Vec::new();
    let mut missing = Vec::new();
    let mut compared = 0usize;
    let mut total_bytes = 0u64;

    for name in &expected {
        let golden_path = golden.join(format!("{name}.png"));
        let shot_path = shots.join(format!("{name}.png"));

        if !actual.contains(name) {
            missing.push(name.clone());
            continue;
        }

        let left = match png::read(&golden_path) {
            Ok(image) => image,
            Err(error) => {
                eprintln!("qa_golden_check: {error}");
                return ExitCode::FAILURE;
            }
        };
        let right = match png::read(&shot_path) {
            Ok(image) => image,
            Err(error) => {
                eprintln!("qa_golden_check: {error}");
                return ExitCode::FAILURE;
            }
        };

        let diff = match png::compare(&left, &right, threshold) {
            Ok(diff) => diff,
            Err(error) => {
                eprintln!("qa_golden_check: {name}: {error}");
                return ExitCode::FAILURE;
            }
        };

        compared += 1;
        total_bytes += std::fs::metadata(&golden_path).map(|meta| meta.len()).unwrap_or(0);

        if diff.changed_ratio() > ratio {
            if let Err(error) = std::fs::create_dir_all(diff_dir) {
                eprintln!("qa_golden_check: {}: {error}", diff_dir.display());
                return ExitCode::FAILURE;
            }

            let diff_path = diff_dir.join(format!("diff-{name}.png"));

            if let Err(error) = png::write(&diff_path, &diff.image) {
                eprintln!("qa_golden_check: {error}");
                return ExitCode::FAILURE;
            }

            println!(
                "ДРЕЙФ {name}.png: {} пикселей ({:.3} %) из {}, max Δ={}, diff {}",
                diff.changed,
                diff.changed_ratio() * 100.0,
                diff.total,
                diff.max_delta,
                diff_path.display()
            );
            drifted.push(name.clone());
        } else {
            println!(
                "ok    {name}.png: {} пикселей ({:.3} %)",
                diff.changed,
                diff.changed_ratio() * 100.0
            );
        }
    }

    let extra: Vec<String> = actual.difference(&expected).cloned().collect();

    for name in &extra {
        println!("нет эталона для {name}.png — добавьте: qa_golden_check … --update");
    }

    println!(
        "qa_golden_check: сверено {compared} кадров ({} КиБ эталонов), порог Δ>{threshold} / {:.1} %",
        total_bytes / 1024,
        ratio * 100.0
    );

    if !missing.is_empty() {
        eprintln!(
            "qa_golden_check: нет кадров для эталонов: {}",
            missing
                .iter()
                .map(|name| format!("{name}.png"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    if drifted.is_empty() && missing.is_empty() {
        ExitCode::SUCCESS
    } else {
        if !drifted.is_empty() {
            eprintln!(
                "qa_golden_check: дрейф кадров: {}",
                drifted
                    .iter()
                    .map(|name| format!("{name}.png"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        eprintln!(
            "qa_golden_check: ВИЗУАЛЬНЫЙ РЕГРЕСС — см. diff-*.png и max-brh-qa/scripts/README.md"
        );

        ExitCode::FAILURE
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("qa_golden_check: {message}\n\n{USAGE}");

    ExitCode::from(2)
}
