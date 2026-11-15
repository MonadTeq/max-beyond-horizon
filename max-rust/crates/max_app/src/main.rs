//! `max_app` — CLI клиента M.A.X.
//!
//! Режимы:
//!
//! * `--demo <save> [--turns N]` — headless: V70-сейв → N тиков → digest;
//! * `--new-game` — локальная партия на карте планеты + кадр в PPM (`--out`);
//! * `--connect <horizon> --join <game> [--sdl]` — Horizon + ENet-сессия;
//!   без `--sdl` цикл headless, с `--sdl` — окно SDL3 (feature `sdl`).
//!
//! Вся игровая логика — в библиотеке (`max_app`), здесь только разбор
//! аргументов и SDL-обвязка. Примеры — в `README.md` крейта.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use max_app::assets::AssetsReport;
#[cfg(feature = "sdl")]
use max_app::net::run_network_with_sink;
use max_app::net::{run_network, NetworkOptions, NetworkReport};
use max_app::{write_ppm, App, AppConfig, AppMode, NewGameConfig, NewGameReport};
use max_core::enums::{Planet, TeamClan};
use max_core::setup::ResourceLevel;

const DEFAULT_SAVE: &str = "max-original/SAVE1.CAM";
const DEFAULT_GRPC_PORT: u16 = 8082;

/// Что запускать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Demo,
    NewGame,
    Connect,
}

/// Разобранные аргументы CLI.
#[derive(Debug)]
struct Cli {
    mode: Mode,
    config: AppConfig,
    save: Option<PathBuf>,
    turns: Option<u32>,
    game_id: Option<String>,
    password: Option<String>,
    sdl: bool,
    out: Option<PathBuf>,
    seed: Option<u32>,
    raw: Option<u8>,
    fuel: Option<u8>,
    gold: Option<u8>,
    derelicts: Option<u8>,
}

impl Cli {
    fn parse() -> Result<Self> {
        let mut cli = Self {
            mode: Mode::Demo,
            config: AppConfig::default(),
            save: None,
            turns: None,
            game_id: None,
            password: None,
            sdl: false,
            out: None,
            seed: None,
            raw: None,
            fuel: None,
            gold: None,
            derelicts: None,
        };
        let mut mode_set = false;
        let mut data_dir_set = false;
        let mut assets_dir_set = false;
        let mut patches_explicit = false;
        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--demo" => {
                    let save = args.next().context("--demo требует путь к сейву")?;
                    cli.save = Some(PathBuf::from(save));
                    if !mode_set {
                        cli.mode = Mode::Demo;
                        mode_set = true;
                    }
                }
                "--new-game" => {
                    cli.mode = Mode::NewGame;
                    mode_set = true;
                }
                "--connect" => {
                    let endpoint = args
                        .next()
                        .context("--connect требует <horizon host[:port]>")?;
                    let (host, port) = parse_endpoint(&endpoint)?;
                    cli.config.horizon_host = host;
                    cli.config.horizon_port = port;
                    cli.mode = Mode::Connect;
                    mode_set = true;
                }
                "--join" => {
                    cli.game_id = Some(args.next().context("--join требует game_id")?);
                }
                "--password" => {
                    cli.password = Some(args.next().context("--password требует значение")?);
                }
                "--turns" => {
                    cli.turns = Some(parse_u32(&mut args, "--turns")?);
                }
                "--seed" => {
                    cli.seed = Some(parse_u32(&mut args, "--seed")?);
                }
                "--raw" => cli.raw = Some(parse_level(&mut args, "--raw")?),
                "--fuel" => cli.fuel = Some(parse_level(&mut args, "--fuel")?),
                "--gold" => cli.gold = Some(parse_level(&mut args, "--gold")?),
                "--derelicts" => cli.derelicts = Some(parse_level(&mut args, "--derelicts")?),
                "--world" | "--planet" => {
                    let value = parse_u8(&mut args, &arg)?;
                    cli.config.planet = Planet::try_from(value)
                        .map_err(|_| anyhow!("планета должна быть 0..=23, получено {value}"))?;
                }
                "--player" => {
                    cli.config.player_name = args.next().context("--player требует имя")?;
                }
                "--team" => {
                    let slot = parse_u8(&mut args, "--team")?;
                    if slot > 3 {
                        bail!("--team должен быть 0..=3, получено {slot}");
                    }
                    cli.config.team_slot = slot;
                }
                "--clan" => {
                    let value = parse_u8(&mut args, "--clan")?;
                    cli.config.clan = TeamClan::try_from(value)
                        .map_err(|_| anyhow!("клан должен быть 0..=8, получено {value}"))?;
                }
                "--data-dir" => {
                    let dir = args.next().context("--data-dir требует каталог")?;
                    cli.config = cli.config.with_data_dir(dir);
                    data_dir_set = true;
                }
                "--assets-dir" => {
                    let dir = args.next().context("--assets-dir требует каталог")?;
                    cli.config = cli.config.with_assets_dir(dir);
                    assets_dir_set = true;
                }
                "--world-dir" => {
                    let dir = args.next().context("--world-dir требует каталог")?;
                    cli.config = cli.config.with_world_dir(dir);
                }
                "--max-res" => {
                    cli.config.max_res =
                        PathBuf::from(args.next().context("--max-res требует путь")?);
                }
                "--patches" => {
                    cli.config.patches_res =
                        PathBuf::from(args.next().context("--patches требует путь")?);
                    patches_explicit = true;
                }
                "--handshake-patches" => {
                    cli.config.handshake_patches = Some(PathBuf::from(
                        args.next()
                            .context("--handshake-patches требует путь")?,
                    ));
                }
                "--palette" => {
                    cli.config.color_pal =
                        PathBuf::from(args.next().context("--palette требует путь")?);
                }
                "--width" => {
                    cli.config.screen_width = parse_u32(&mut args, "--width")?;
                }
                "--height" => {
                    cli.config.screen_height = parse_u32(&mut args, "--height")?;
                }
                "--out" => {
                    cli.out = Some(PathBuf::from(args.next().context("--out требует путь")?));
                }
                "--sdl" => cli.sdl = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => bail!("неизвестный аргумент: {other} (см. --help)"),
            }
        }

        if cli.sdl && cli.mode != Mode::Connect {
            bail!("--sdl имеет смысл только с --connect <horizon> --join <game>");
        }
        if cli.mode == Mode::Connect && cli.game_id.is_none() {
            bail!("--connect требует --join <game_id>");
        }

        cli.config.mode = match cli.mode {
            Mode::Demo => AppMode::Demo,
            Mode::NewGame => AppMode::NewGame,
            Mode::Connect => AppMode::Connect,
        };
        cli.config.save = cli.save.clone();
        cli.config.demo_turns = cli.turns.unwrap_or(5);

        // Каталоги по умолчанию — от корня репозитория; если не заданы явно и
        // относительный путь не существует, пробуем запуск из `max-rust/`.
        if !data_dir_set && !cli.config.max_res.exists() {
            if let Some(root) = repository_root() {
                cli.config = cli.config.with_data_dir(root.join("max-original"));
            }
        }
        // Явный --patches отключает fallback: клиент может намеренно указать
        // несуществующий файл, повторяя семантику C++-сервера (отсутствующий
        // файл даёт пустую часть data_hash в хендшейке).
        if !assets_dir_set && !patches_explicit && !cli.config.patches_res.exists() {
            if let Some(root) = repository_root() {
                cli.config = cli.config.with_assets_dir(root.join("assets"));
            }
        }

        Ok(cli)
    }

    fn new_game_config(&self) -> NewGameConfig {
        let mut config = NewGameConfig::p2_reference();

        config.planet = self.config.planet;
        if let Some(seed) = self.seed {
            config.seed = seed;
        }
        if let Some(raw) = self.raw {
            config.raw = ResourceLevel::new(raw).expect("level validated by CLI");
        }
        if let Some(fuel) = self.fuel {
            config.fuel = ResourceLevel::new(fuel).expect("level validated by CLI");
        }
        if let Some(gold) = self.gold {
            config.gold = ResourceLevel::new(gold).expect("level validated by CLI");
        }
        if let Some(derelicts) = self.derelicts {
            config.derelicts = ResourceLevel::new(derelicts).expect("level validated by CLI");
        }
        config.player_team = max_core::team::TeamId(self.config.team_slot);

        config
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt().init();

    match Cli::parse().and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("max_app: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;

    runtime.block_on(async move {
        match cli.mode {
            Mode::Demo => run_demo(&cli),
            Mode::NewGame => run_new_game(&cli),
            Mode::Connect => run_connect(&cli).await,
        }
    })
}

/// `--demo <save> [--turns N]`: headless-прогон сейва.
fn run_demo(cli: &Cli) -> Result<()> {
    let explicit = cli.save.is_some() || cli.config.save.is_some();
    let mut save = cli
        .save
        .clone()
        .or_else(|| cli.config.save.clone())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SAVE));

    // Путь по умолчанию задан от корня репозитория: пробуем тот же файл рядом
    // с MAX.RES (каталог данных уже найден). Явно указанный путь не подменяем.
    if !explicit && !save.is_file() {
        if let Some(candidate) = cli
            .config
            .max_res
            .parent()
            .map(|dir| dir.join("SAVE1.CAM"))
            .filter(|candidate| candidate.is_file())
        {
            save = candidate;
        }
    }

    let mut app = App::new(cli.config.clone());
    let turns = cli.turns.unwrap_or(5);
    let digest = app
        .run_demo(&save, turns)
        .with_context(|| format!("demo: {}", save.display()))?;

    println!(
        "demo: save={} turns={} units={}",
        save.display(),
        turns,
        digest.units
    );
    println!("{digest}");

    Ok(())
}

/// `--new-game`: локальная генерация карты, кадр в PPM (`--out`).
fn run_new_game(cli: &Cli) -> Result<()> {
    let mut app = App::new(cli.config.clone());

    if let Err(error) = app.init_assets() {
        bail!(
            "new-game требует данные игры ({}): {error:#}",
            cli.config.max_res.display()
        );
    }
    print_assets(app.assets().map(|assets| assets.report()).as_ref());

    let report = app.new_game(&cli.new_game_config())?;
    print_new_game(&report);

    let (frame, stats) = app.render_frame_with_stats();
    println!(
        "frame: {}x{} rgba8 ({} байт), tiles={} units={} sprite={} marker={} team={:?}",
        frame.width(),
        frame.height(),
        frame.pixels().len(),
        stats.map_tiles,
        stats.units(),
        stats.units_drawn,
        stats.units_marker,
        app.state().player_team
    );

    if let Some(path) = &cli.out {
        write_ppm(path, &frame).with_context(|| format!("запись {}", path.display()))?;
        println!("ppm: {}", path.display());
    }

    println!("digest: {}", app.digest());

    Ok(())
}

/// `--connect <horizon> --join <game> [--sdl]`.
async fn run_connect(cli: &Cli) -> Result<()> {
    let mut app = App::new(cli.config.clone());
    let options = NetworkOptions {
        game_id: cli.game_id.clone().expect("validated by CLI"),
        password: cli.password.clone(),
        max_turns: cli.turns.unwrap_or(0),
        ..NetworkOptions::default()
    };

    let report = if cli.sdl {
        connect_interactive(&mut app, &options, &cli.config).await?
    } else {
        run_network(&mut app, &options).await?
    };

    print_network(&report);

    Ok(())
}

fn print_assets(report: Option<&AssetsReport>) {
    let Some(report) = report else { return };

    println!(
        "assets: MAX.RES={} PATCHES.RES={} world={} ({}; {}x{}) sprites={}",
        report.max_res_entries,
        report.patches_res_entries,
        report.world,
        report.world_source,
        report.world_size.0,
        report.world_size.1,
        report.sprite_count()
    );
    for sprite in &report.sprites {
        println!(
            "  sprite {:<10} type={:?} frames={} shadow={}",
            sprite.name, sprite.unit_type, sprite.frames, sprite.shadow_frames
        );
    }
    for warning in &report.warnings {
        println!("  warning: {warning}");
    }
}

fn print_new_game(report: &NewGameReport) {
    println!(
        "new-game: planet={:?} seed={} teams={} aliens={} start_units={} units={} surface_crc=0x{:04X} resources_crc=0x{:04X}",
        report.planet,
        report.seed,
        report.teams,
        report.aliens,
        report.start_units,
        report.units,
        report.surface_crc,
        report.resources_crc
    );
}

fn print_network(report: &NetworkReport) {
    println!(
        "network: game={} handshake={} lobby={} team={} ticks={} commands={} turns={} barrier_timeouts={}",
        report.game_id,
        report.handshake,
        report.lobby_complete,
        report.local_team.0,
        report.ticks,
        report.commands_applied,
        report.turns_finished,
        report.barrier_timeouts
    );
    println!("digest: {}", report.digest);
}

/// Окно SDL3: кадр библиотеки → текстура → экран.
#[cfg(feature = "sdl")]
struct WindowSink {
    window: max_render::SdlWindow,
}

#[cfg(feature = "sdl")]
impl max_app::net::FrameSink for WindowSink {
    fn frame(&mut self, app: &App) -> bool {
        let buffer = app.render_frame();

        if let Err(error) = self.window.present(&buffer) {
            eprintln!("SDL: {error}");
            return false;
        }

        self.window.poll()
    }
}

/// Запуск `--connect --sdl`: сетевой цикл рисует кадры в окно.
async fn connect_interactive(
    app: &mut App,
    options: &NetworkOptions,
    config: &AppConfig,
) -> Result<NetworkReport> {
    #[cfg(feature = "sdl")]
    {
        let window = max_render::SdlWindow::new(
            "M.A.X. — max_app (R2.4)",
            config.screen_width,
            config.screen_height,
        )
        .map_err(|error| anyhow!("{error}"))?;
        let mut sink = WindowSink { window };

        println!(
            "SDL: окно {}x{}; Esc/закрытие — выход",
            config.screen_width, config.screen_height
        );

        run_network_with_sink(app, options, Some(&mut sink)).await
    }

    #[cfg(not(feature = "sdl"))]
    {
        let _ = (app, options, config);
        bail!(
            "--sdl требует сборки с feature sdl: \
             cargo run -p max_app --features sdl -- --connect ... --join ... --sdl"
        )
    }
}

// ---------------------------------------------------------------------------
// Разбор значений
// ---------------------------------------------------------------------------

fn parse_endpoint(value: &str) -> Result<(String, u16)> {
    match value.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port: u16 = port
                .parse()
                .with_context(|| format!("некорректный порт в {value}"))?;
            Ok((host.to_string(), port))
        }
        _ => Ok((value.to_string(), DEFAULT_GRPC_PORT)),
    }
}

fn parse_u32(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<u32> {
    let value = args
        .next()
        .with_context(|| format!("{flag} требует число"))?;
    value
        .parse()
        .with_context(|| format!("{flag}: ожидалось число, получено {value}"))
}

fn parse_u8(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<u8> {
    let value = args
        .next()
        .with_context(|| format!("{flag} требует число"))?;
    value
        .parse()
        .with_context(|| format!("{flag}: ожидалось число 0..=255, получено {value}"))
}

fn parse_level(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<u8> {
    let value = parse_u8(args, flag)?;

    if ResourceLevel::new(value).is_none() {
        bail!("{flag}: уровень ресурсов должен быть 0..=2, получено {value}");
    }

    Ok(value)
}

/// Корень репозитория, если `cargo run` запущен из `max-rust/` или корня.
fn repository_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;

    for candidate in [cwd.clone(), cwd.join("..")] {
        let candidate = candidate.canonicalize().unwrap_or(candidate);
        if candidate.join("max-rust").is_dir() && candidate.join("max-original").is_dir() {
            return Some(candidate);
        }
    }

    None
}

fn print_help() {
    println!(
        "\
max_app — клиент M.A.X. (core + net + assets + render)

ИСПОЛЬЗОВАНИЕ:
    max_app [--demo <save>] [--turns N] [ОПЦИИ]
    max_app --new-game [ОПЦИИ]
    max_app --connect <host[:port]> --join <game_id> [--sdl] [ОПЦИИ]

РЕЖИМЫ:
    --demo <save>        V70-сейв → N тиков → digest (headless)
    --new-game           локальная партия (setup::generate_map) + кадр
    --connect <host>     Horizon gRPC (порт по умолчанию {}) + ENet-сессия
    --join <game_id>     игра для --connect
    --sdl                окно SDL3 для --connect (нужна сборка --features sdl)

ИГРА:
    --turns N            тиков в --demo (5) / лимит ходов в --connect (0 — без лимита)
    --seed N             seed ГПСЧ для --new-game
    --raw/--fuel/--gold/--derelicts N   уровни ресурсов 0..=2 (по умолчанию 1/1/1/0)
    --world N            планета 0..=23 (по умолчанию 0 — Snowcrab)
    --player <name>      имя игрока в лобби
    --team N             слот команды 0..=3
    --clan N             клан 0..=8 (0 — случайный)
    --password <pw>      пароль игры
    --out <frame.ppm>    записать кадр --new-game в PPM

ДАННЫЕ:
    --data-dir <dir>     каталог MAX.RES/COLOR.PAL (по умолчанию max-original)
    --assets-dir <dir>   каталог PATCHES.RES (по умолчанию assets)
    --world-dir <dir>    каталог .WRL-карт (по умолчанию рядом с MAX.RES)
    --max-res/--patches/--palette <path>   отдельные пути
    --handshake-patches <path>             PATCHES.RES только для data_hash
                                           (сервер без PATCHES.RES: путь может
                                           не существовать — хэш без этой части)
    --width/--height N   размер кадра (по умолчанию 512x384)

ПРИМЕРЫ:
    cargo run -p max_app -- --demo ../max-original/SAVE1.CAM --turns 5
    cargo run -p max_app -- --new-game --world 12 --seed 42 --out frame.ppm
    cargo run -p max_app --features sdl -- --connect 127.0.0.1:8082 --join demo --sdl
",
        DEFAULT_GRPC_PORT
    );
}
