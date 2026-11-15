//! P4 soak: headless AI×AI партия на 100+ ходов с метриками времени и памяти.
//!
//! ```sh
//! # 120 ходов, RED (человек) ничего не делает, GREEN+BLUE — компьютеры
//! cargo run -q -p max_app --no-default-features --example soak_headless -- \
//!     --turns 120 --out qa-out/soak-headless
//!
//! # профиль тика и память попадают и в stdout, и в qa-out/soak-headless/soak.txt
//! ```
//!
//! Что делает: поднимает `App` с настоящими данными игры, играет локальную
//! партию `LocalSetup` (RED — человек, N компьютеров, Duration 200, таймер
//! выключен, сложность `--opponent`), закрывая каждый ход
//! `App::local_end_turn` и прокручивая ходы ИИ `App::local_update(1/60)` —
//! ровно как десктопный цикл. Возвращает `0`, если партия дошла до
//! запрошенного хода и память не разошлась (см. [`max_app::SoakReport`]).
//!
//! Код выхода: `0` — PASS, `1` — ранний конец/утечка/ошибка, `2` — аргументы.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use max_app::{run_soak, App, AppConfig, AppMode, SoakOptions};

const USAGE: &str = "\
Использование: soak_headless [опции]

  --turns N        сколько ходов играть (по умолчанию 120)
  --out DIR        каталог для soak.txt (по умолчанию qa-out/soak-headless)
  --data-dir DIR   каталог MAX.RES/COLOR.PAL (по умолчанию <repo>/max-original)
  --assets-dir DIR каталог PATCHES.RES (по умолчанию <repo>/assets)
  --ai N           компьютеров в партии, 1..=3 (по умолчанию 2)
  --world N        планета 0..=23 (по умолчанию 0)
  --seed N         seed генерации карты (по умолчанию 0)
  --opponent N     сложность ИИ 0..=5 (по умолчанию 0 — самая простая)
  --dt-ms F        dt кадра в миллисекундах (по умолчанию 16.667 = 60 Гц)
  --quiet          не печатать строки ходов в stdout
  -h, --help       эта справка
";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .to_path_buf()
}

fn main() -> ExitCode {
    let mut options = SoakOptions::default();
    let mut out_dir = PathBuf::from("qa-out/soak-headless");
    let mut data_dir = repo_root().join("max-original");
    let mut assets_dir = repo_root().join("assets");
    let mut quiet = false;
    let mut dt_ms: f32 = options.dt * 1000.0;

    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--quiet" => quiet = true,
            "--turns" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => options.turns = value,
                None => return usage_error("--turns требует число"),
            },
            "--out" => match args.next() {
                Some(value) => out_dir = PathBuf::from(value),
                None => return usage_error("--out требует каталог"),
            },
            "--data-dir" => match args.next() {
                Some(value) => data_dir = PathBuf::from(value),
                None => return usage_error("--data-dir требует каталог"),
            },
            "--assets-dir" => match args.next() {
                Some(value) => assets_dir = PathBuf::from(value),
                None => return usage_error("--assets-dir требует каталог"),
            },
            "--ai" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => options.ai_teams = value,
                None => return usage_error("--ai требует число 1..=3"),
            },
            "--world" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => options.world = value,
                None => return usage_error("--world требует число 0..=23"),
            },
            "--seed" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => options.seed = value,
                None => return usage_error("--seed требует число"),
            },
            "--opponent" => match args.next().and_then(|value| value.parse().ok()) {
                Some(value) => options.opponent = value,
                None => return usage_error("--opponent требует число 0..=5"),
            },
            "--dt-ms" => match args.next().and_then(|value| value.parse::<f32>().ok()) {
                Some(value) if value >= 0.0 => dt_ms = value,
                _ => return usage_error("--dt-ms требует неотрицательное число"),
            },
            other => return usage_error(&format!("неизвестный аргумент «{other}»")),
        }
    }

        options.dt = dt_ms / 1000.0;

    if let Err(error) = std::fs::create_dir_all(&out_dir) {
        eprintln!("soak_headless: каталог {}: {error}", out_dir.display());
        return ExitCode::FAILURE;
    }

    let config = AppConfig::default()
        .with_data_dir(&data_dir)
        .with_assets_dir(&assets_dir)
        .with_mode(AppMode::Demo)
        .with_autosave(false);

    let mut app = App::new(config);

    let report = match run_soak(&mut app, &options) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("soak_headless: {error}");
            return ExitCode::FAILURE;
        }
    };

    let log = report.log();
    let log_path = out_dir.join("soak.txt");

    if let Err(error) = std::fs::write(&log_path, &log) {
        eprintln!("soak_headless: лог {}: {error}", log_path.display());
        return ExitCode::FAILURE;
    }

    if !quiet {
        print!("{log}");
    }

    println!(
        "soak_headless: {} ходов, {} кадров, {:.2} c, лог {}",
        report.turns_played,
        report.frames,
        report.total.as_secs_f64(),
        log_path.display()
    );

    if report.passed() && report.memory_stable() {
        println!(
            "soak_headless: PASS · avg_turn={:.1} ms · avg_frame={:.1} us · max_frame={:.1} us",
            report.avg_turn().as_secs_f64() * 1000.0,
            report.profile.local_update.avg_ms() * 1000.0,
            report.profile.local_update.max_ms() * 1000.0
        );
        ExitCode::SUCCESS
    } else {
        if !report.passed() {
            eprintln!(
                "soak_headless: FAIL — партия закончилась на ходе {} (запрошено {})",
                report.final_turn, options.turns
            );
        }
        if !report.memory_stable() {
            eprintln!(
                "soak_headless: FAIL — RSS вырос на {} КиБ",
                report.memory_growth_kb()
            );
        }
        ExitCode::FAILURE
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("soak_headless: {message}\n\n{USAGE}");
    ExitCode::from(2)
}
