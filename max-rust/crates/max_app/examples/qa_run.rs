//! Слой 2 QA: интерпретатор `.qa`-скриптов (in-process UI-драйвер без SDL).
//!
//! ```sh
//! cargo run -p max_app --example qa_run -- <script.qa> <out_dir>
//! cargo run -p max_app --example qa_run -- qa/sp-full.qa qa-out/sp-full \
//!     --data-dir ../max-original --assets-dir ../assets
//! ```
//!
//! Пишет в `<out_dir>`:
//!
//! * `log.txt` — каждый шаг, фактическое состояние после ассертов, итог;
//! * `<name>.png` — кадры команд `shot` (640×480, окно оригинала);
//! * `*.mxs` — контейнеры команд `save`.
//!
//! Код выхода: `0` — все шаги выполнились, `1` — упал `assert`/команда (текст
//! печатается в stderr и попадает в `log.txt`), `2` — неверные аргументы.
//!
//! Пути данных по умолчанию — из корня репозитория (`max-original/`,
//! `assets/`), поэтому пример работает и из любого каталога, и в Docker
//! (`-w /src/max-rust`).

use std::path::PathBuf;
use std::process::ExitCode;

use max_app::qa::{QaConfig, QaRunner};

const USAGE: &str = "\
Использование: qa_run <script.qa> <out_dir> [опции]

  --data-dir DIR     каталог MAX.RES/COLOR.PAL/.WRL (по умолчанию <repo>/max-original)
  --assets-dir DIR   каталог PATCHES.RES (по умолчанию <repo>/assets)
  --frame-limit N    предел кадров на wait/end_turn (по умолчанию 40000)
  -h, --help         эта справка
";

fn main() -> ExitCode {
    let mut script: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut data_dir: Option<PathBuf> = None;
    let mut assets_dir: Option<PathBuf> = None;
    let mut frame_limit: Option<usize> = None;

    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--data-dir" => match args.next() {
                Some(value) => data_dir = Some(PathBuf::from(value)),
                None => return usage_error("--data-dir требует каталог"),
            },
            "--assets-dir" => match args.next() {
                Some(value) => assets_dir = Some(PathBuf::from(value)),
                None => return usage_error("--assets-dir требует каталог"),
            },
            "--frame-limit" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => frame_limit = Some(value),
                None => return usage_error("--frame-limit требует число"),
            },
            other if other.starts_with('-') => {
                return usage_error(&format!("неизвестная опция {other}"))
            }
            other => {
                if script.is_none() {
                    script = Some(PathBuf::from(other));
                } else if out_dir.is_none() {
                    out_dir = Some(PathBuf::from(other));
                } else {
                    return usage_error(&format!("лишний аргумент «{other}»"));
                }
            }
        }
    }

    let (Some(script), Some(out_dir)) = (script, out_dir) else {
        return usage_error("нужны <script.qa> и <out_dir>");
    };

    let mut config = QaConfig::new(&out_dir);

    if let Some(dir) = data_dir {
        config = config.with_data_dir(dir);
    }
    if let Some(dir) = assets_dir {
        config = config.with_assets_dir(dir);
    }
    if let Some(limit) = frame_limit {
        config = config.with_frame_limit(limit);
    }

    let mut runner = match QaRunner::new(config.clone()) {
        Ok(runner) => runner,
        Err(error) => {
            eprintln!("qa_run: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "qa_run: скрипт={} вывод={} данные={} ассеты={}",
        script.display(),
        config.out_dir.display(),
        config.data_dir.display(),
        config.assets_dir.display()
    );

    let report = runner.run_file(&script);

    let log_path = match report.write_log(&config.out_dir) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("qa_run: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("qa_run: лог {}", log_path.display());
    println!("qa_run: {}", report.summary());
    println!("qa_run: кадров {}", report.frames);

    for shot in &report.shots {
        println!("  кадр {}", shot.display());
    }

    if report.passed {
        ExitCode::SUCCESS
    } else {
        for failure in &report.failures {
            eprintln!("qa_run: FAIL {failure}");
        }

        eprintln!(
            "qa_run: скрипт {} упал, см. {}",
            script.display(),
            log_path.display()
        );
        ExitCode::FAILURE
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("qa_run: {message}\n\n{USAGE}");

    ExitCode::from(2)
}
