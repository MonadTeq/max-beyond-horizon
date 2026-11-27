//! R7 presentation benchmark: how long one HD frame takes and how much VRAM the
//! remaster layers hold (`max-brh-docs/development/remaster-hd.md` §R7).
//!
//! Headless and presentation-only: the match is started exactly like the QA
//! driver / `soak_headless`, the frames are rendered through
//! [`max_app::App::render_frame_hd`] and timed. Nothing here touches the
//! simulation state, so the run can be compared with `r0-measurements.md` §1.3
//! (the pixel-perfect renderer at `S = 1`/`S = 2`/`S = 4`).
//!
//! ```sh
//! cargo run -p max_app --no-default-features --release --example hd_frame_bench -- \
//!     --scale 2 --frames 30 --steps 120 [--aa 2]
//! ```
//!
//! Output (stdout):
//!
//! * `frame=…` — one line per measured frame (`render_us`, `bytes`; with
//!   `--aa` also `resolve_ms`, the box downsample of `ssaa-design.md`);
//! * `HD-BENCH …` — the summary: median/mean frame time, the implied fps, the
//!   ratio to the 60 Hz budget (16.67 ms) and the VRAM accounting;
//! * `HD-BENCH VRAM … ok|OVER` — the quota gate of the scale (R0 §2.1);
//! * `HD-BENCH SSAA …` — the SSAA multiplier and the render frame it implies.
//!
//! The measurement machine of R0 is a release build; a debug build is roughly
//! an order of magnitude slower and is only useful for the VRAM part.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use max_app::hd::{vram_quota, HdPresenter};
use max_app::local::{LocalPhase, LocalSetup};
use max_app::{App, AppConfig, AppMode};
use max_core::enums::{PlayMode, TeamClan, VictoryType};
use max_core::team::TeamKind;
use max_render::render_scale::{AaMode, RenderScale};

const USAGE: &str = "\
Использование: hd_frame_bench [опции]

  --scale 2|4          кратность HD (по умолчанию 2)
  --aa 0|off|1|2|4     SSAA-множитель кадра: 0/1/off — выкл., 2 или 4 (по умолчанию 0)
  --frames N           измеряемых кадров (по умолчанию 30)
  --steps N            тиков симуляции перед первым кадром (по умолчанию 120)
  --data-dir DIR       каталог MAX.RES/COLOR.PAL/.WRL (по умолчанию <repo>/max-original)
  --remaster-dir DIR   каталог HD-ассетов (по умолчанию <repo>/max-brh/target/remaster)
  -h, --help           эта справка
";

/// Parsed arguments.
struct Options {
    scale: RenderScale,
    aa: AaMode,
    frames: u32,
    steps: u32,
    data_dir: PathBuf,
    remaster_dir: PathBuf,
}

impl Default for Options {
    fn default() -> Self {
        let root = repository_root();
        let data_dir = root
            .as_ref()
            .map(|root| root.join("max-original"))
            .unwrap_or_else(|| PathBuf::from("max-original"));

        Self {
            scale: RenderScale::Hd2,
            aa: AaMode::Off,
            frames: 30,
            steps: 120,
            remaster_dir: max_app::default_remaster_root(root.as_deref()),
            data_dir,
        }
    }
}

fn repository_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;

    for candidate in [cwd.clone(), cwd.join("..")] {
        let candidate = candidate.canonicalize().unwrap_or(candidate);
        if candidate.join("max-brh").is_dir() && candidate.join("max-original").is_dir() {
            return Some(candidate);
        }
    }

    None
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "--scale" => {
                let value: u32 = args
                    .next()
                    .ok_or("--scale требует число")?
                    .parse()
                    .map_err(|_| "--scale: ожидалась кратность 2 или 4".to_string())?;
                options.scale = RenderScale::from_scale(value)
                    .filter(|scale| scale.is_hd())
                    .ok_or_else(|| format!("--scale: ожидалось 2 или 4, получено {value}"))?;
            }
            "--aa" => {
                let value = args.next().ok_or("--aa требует значение")?;
                options.aa = AaMode::parse_flag(&value)
                    .map_err(|_| format!("--aa: ожидалось 0|off|1|2|4, получено {value}"))?;
            }
            "--frames" => {
                options.frames = args
                    .next()
                    .ok_or("--frames требует число")?
                    .parse()
                    .map_err(|_| "--frames: ожидалось число".to_string())?;
            }
            "--steps" => {
                options.steps = args
                    .next()
                    .ok_or("--steps требует число")?
                    .parse()
                    .map_err(|_| "--steps: ожидалось число".to_string())?;
            }
            "--data-dir" => {
                options.data_dir = PathBuf::from(args.next().ok_or("--data-dir требует каталог")?);
            }
            "--remaster-dir" => {
                options.remaster_dir =
                    PathBuf::from(args.next().ok_or("--remaster-dir требует каталог")?);
            }
            other => return Err(format!("неизвестная опция {other}")),
        }
    }

    Ok(Some(options))
}

/// One human (RED) against one computer (GREEN) on a fixed planet/seed — the
/// same scene as the QA and presentation gates.
fn setup() -> LocalSetup {
    LocalSetup {
        player_name: "Bench".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        team_clans: [TeamClan::Random; 4],
        hot_seat: false,
        seed: 7,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

fn started_app(data_dir: &Path) -> Result<App, String> {
    if !data_dir.join("MAX.RES").is_file() {
        return Err(format!(
            "нет данных игры: {}\\MAX.RES (нужен max-original)",
            data_dir.display()
        ));
    }

    let config = AppConfig::default()
        .with_data_dir(data_dir)
        .with_mode(AppMode::Ui);
    let mut app = App::new(config);

    app.init_assets()
        .map_err(|error| format!("init_assets: {error:#}"))?;
    app.start_local_game(&setup())
        .map_err(|error| format!("start_local_game: {error}"))?;

    // Landing/AI phases until the human turn (`local_update(0.0)` ticks them
    // without advancing the visual clock).
    for _ in 0..900 {
        if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
            break;
        }
        if app.local().is_none_or(|local| local.finished()) {
            return Err("local game finished before the player turn".to_string());
        }
        app.local_update(0.0);
    }

    Ok(app)
}

fn median(mut samples: Vec<f64>) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }

    samples.sort_by(f64::total_cmp);
    let mid = samples.len() / 2;

    if samples.len() % 2 == 0 {
        (samples[mid - 1] + samples[mid]) / 2.0
    } else {
        samples[mid]
    }
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(Some(options)) => options,
        Ok(None) => return ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hd_frame_bench: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let mut app = match started_app(&options.data_dir) {
        Ok(app) => app,
        Err(error) => {
            eprintln!("hd_frame_bench: {error}");
            return ExitCode::FAILURE;
        }
    };

    let Some(assets) = app.assets() else {
        eprintln!("hd_frame_bench: ассеты не загрузились");
        return ExitCode::FAILURE;
    };
    let mut hd = HdPresenter::load(assets, options.scale, &options.remaster_dir);
    let stats = hd.stats();
    let scale_factor = options.scale.scale();
    let factor = scale_factor * options.aa.factor();

    println!(
        "hd_frame_bench: scale={} aa={} frame={}x{} resolved={}x{} frames={} steps={} data={} remaster={}",
        options.scale.label(),
        options.aa.label(),
        640 * factor,
        480 * factor,
        640 * scale_factor,
        480 * scale_factor,
        options.frames,
        options.steps,
        options.data_dir.display(),
        options.remaster_dir.display()
    );
    println!(
        "hd_frame_bench: HD assets tiles={} sprites={} shadows={} chrome={} fonts={} vram={:.1} MiB",
        stats.hd_tiles,
        stats.hd_sprites,
        stats.hd_shadows,
        stats.hd_chrome,
        stats.hd_fonts,
        stats.total_mib()
    );

    for _ in 0..options.steps {
        app.local_update(max_app::SIMULATION_DT);
    }

    // Warm-up frame: the first frame pays for the buffers/palette LUTs.
    let warm = app.render_frame_hd(&mut hd, options.aa);
    println!(
        "hd_frame_bench: warmup {}x{} bytes={}",
        warm.width(),
        warm.height(),
        warm.pixels().len()
    );

    // The box resolve returns the `xS` frame; verify the geometry once, then
    // time it per frame (`ssaa-design.md`).
    if options.aa.is_on() {
        let resolved = max_render::downscale_box(&warm, options.aa.factor());

        assert_eq!(
            (resolved.width(), resolved.height()),
            (640 * scale_factor, 480 * scale_factor),
            "the SSAA resolve must return the xS frame"
        );
    }

    let mut samples = Vec::with_capacity(options.frames as usize);
    let mut resolve_samples = Vec::with_capacity(options.frames as usize);
    let mut last_bytes = 0usize;

    for frame in 0..options.frames {
        let started = Instant::now();
        let buffer = app.render_frame_hd(&mut hd, options.aa);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;

        // The SSAA resolve is part of the presented frame: measure the box
        // downsample separately, it is pure integer CPU work on the game frame.
        let resolve_ms = if options.aa.is_on() {
            let resolve_started = Instant::now();
            let resolved = max_render::downscale_box(&buffer, options.aa.factor());
            let elapsed = resolve_started.elapsed().as_secs_f64() * 1000.0;

            // Keep the optimizer from dropping the resolve of a buffer nobody
            // reads: the timing must cover the real box filter.
            std::hint::black_box(&resolved);
            resolve_samples.push(elapsed);

            elapsed
        } else {
            0.0
        };

        last_bytes = buffer.pixels().len();
        samples.push(elapsed);
        println!(
            "frame={frame} render_ms={elapsed:.2} resolve_ms={resolve_ms:.2} bytes={last_bytes}"
        );
    }

    let median_ms = median(samples.clone());
    let mean_ms = if samples.is_empty() {
        0.0
    } else {
        samples.iter().sum::<f64>() / samples.len() as f64
    };
    let resolve_ms = median(resolve_samples);
    let budget_ms = 1000.0 / 60.0;
    let fps = if median_ms > 0.0 {
        1000.0 / median_ms
    } else {
        0.0
    };

    println!(
        "HD-BENCH median_ms={median_ms:.2} mean_ms={mean_ms:.2} fps={fps:.1} \
         budget60={:.1}% frame_bytes={last_bytes}",
        median_ms / budget_ms * 100.0
    );
    println!(
        "HD-BENCH SSAA aa={} render={}x{} resolved={}x{} resolve_median_ms={resolve_ms:.2} \
         total_median_ms={:.2}",
        options.aa.label(),
        640 * factor,
        480 * factor,
        640 * scale_factor,
        480 * scale_factor,
        median_ms + resolve_ms
    );

    let stats = hd.stats();
    let quota = vram_quota(options.scale);
    let over = stats.total_bytes() > quota;

    println!(
        "HD-BENCH VRAM bytes={} quota={} mib={:.1} cache_bytes={} substituted={} {}",
        stats.total_bytes(),
        quota,
        stats.total_mib(),
        stats.cache_bytes,
        stats.substituted(),
        if over { "OVER" } else { "ok" }
    );

    if over {
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
